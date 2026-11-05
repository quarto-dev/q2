/*
 * navbar_generate.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! Navbar resolution transform.
//!
//! Reads the top-level `navbar:` YAML from merged metadata (Phase 3
//! Decision 1 — navbar config stays at the top level, because it's
//! feature-scoped rather than website-scoped; see
//! `claude-notes/plans/2026-04-24-websites-phase-3.md`), hands it to
//! [`quarto_navigation::resolve_navbar`], and stores the resolved
//! structure at `navigation.navbar`.
//!
//! When a [`ProjectIndex`](crate::project::index::ProjectIndex) is
//! attached to the [`RenderContext`], this transform also:
//!
//! - Enriches bare-href items with the referenced document's title
//!   (via [`navigation_enrich::enrich_navigation_items`]).
//! - Marks the item whose href matches the current page's source path
//!   as `active` (via [`navigation_active::mark_active`]). Recurses
//!   into dropdown `menu` children. Matches the sidebar convention
//!   from Phase 2: Generate is format-agnostic; hrefs stay as source
//!   paths; the HTML-aware rewrite happens in `NavbarRenderTransform`.
//!
//! ## Skip conditions
//!
//! - `navbar: false` (affirmative disable).
//! - `navbar` absent or `navbar: true`.
//! - `navigation.navbar` already populated (user override).

use quarto_config::resolve_website_value;
use quarto_error_reporting::{DiagnosticMessage, DiagnosticMessageBuilder};
use quarto_navigation::{
    Breakpoint, NavigationItem, ResponsiveTextWarning, resolve_navbar, responsive_text_warnings,
};
use quarto_pandoc_types::config_value::ConfigValue;
use quarto_pandoc_types::pandoc::Pandoc;

use crate::Result;
use crate::render::RenderContext;
use crate::transform::{AstTransform, TransformPhase};
use crate::transforms::is_feature_disabled;
use crate::transforms::navigation_active::{mark_active, page_relative_source};
use crate::transforms::navigation_drafts::prune_unlinkable_items;
use crate::transforms::navigation_enrich::enrich_navigation_items;
use crate::transforms::navigation_href::resolve_metadata_path;

/// Transform that resolves the user's `navbar:` config and stores it at
/// `navigation.navbar`.
pub struct NavbarGenerateTransform;

impl NavbarGenerateTransform {
    pub fn new() -> Self {
        Self
    }
}

impl Default for NavbarGenerateTransform {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait(?Send)]
impl AstTransform for NavbarGenerateTransform {
    fn name(&self) -> &str {
        "navbar-generate"
    }

    fn phase(&self) -> TransformPhase {
        TransformPhase::Navigation
    }

    async fn transform(&self, ast: &mut Pandoc, ctx: &mut RenderContext) -> Result<()> {
        if is_feature_disabled(&ast.meta, "navbar") {
            return Ok(());
        }

        if ast.meta.contains_path(&["navigation", "navbar"]) {
            return Ok(());
        }

        let Some(mut navbar) = resolve_navbar(&ast.meta) else {
            return Ok(());
        };

        // Breakpoint-keyed `text:` maps (bd-navbar-responsive-text-am98rw95):
        // checked on the authored config, because parsing drops unknown
        // keys together with their source locations.
        for diagnostic in responsive_text_diagnostics(&ast.meta) {
            ctx.diagnostics.push(diagnostic);
        }

        // Dark-mode toggle: emitted whenever the format has a dark
        // theme variant (bd-0pic6 A4). Derived from the THEME config,
        // not `navbar:` YAML — same trigger as Q1's
        // `formatDarkMode(format) !== undefined` navbar/sidebar
        // darkToggle. Parse errors mean the theme stage will fail the
        // render anyway; treat as "no toggle" here.
        // The pair decision recorded by CompileThemeCssStage
        // (`rendered.theme.dark-is-default`) is the source of truth —
        // it includes dark variants synthesized from unified-brand
        // content, which the config re-parse below cannot see. The
        // re-derivation stays as a fallback for pipelines that skip
        // the theme stage.
        let recorded_pair = ast
            .meta
            .get("rendered")
            .and_then(|v| v.get("theme"))
            .and_then(|v| v.get("dark-is-default"))
            .is_some();
        navbar.dark_mode_toggle = recorded_pair
            || quarto_sass::ThemeConfig::from_config_value(&ast.meta)
                .is_ok_and(|c| c.dark.is_some());

        // Search box (bd-eip0naf4): an explicit `navbar.search` wins;
        // unset follows `website.search.location` (Quarto 1's
        // `navbarEjsData` rule). The localized label rides along so
        // the renderer needs no language table.
        navbar.search = Some(crate::project::search::ui::navbar_search_enabled(
            &ast.meta,
            navbar.search,
        ));
        navbar.search_label = Some(crate::project::search::ui::search_label(&ast.meta));

        // bd-qor9a — resolve each href against the YAML file it was
        // authored in. Frontmatter-rooted hrefs become project-root-
        // relative; `_quarto.yml`-rooted ones (the common case for
        // navbars) degrade cleanly to today's behaviour.
        if let Some(source_context) = ctx.source_context {
            resolve_item_hrefs(&mut navbar.left, source_context, &ctx.project.dir);
            resolve_item_hrefs(&mut navbar.right, source_context, &ctx.project.dir);
            if let Some(logo_href) = navbar.logo_href.as_mut() {
                *logo_href = resolve_metadata_path(
                    logo_href,
                    &navbar.logo_href_source,
                    source_context,
                    &ctx.project.dir,
                );
            }
            // Same treatment for the logo paths themselves (Case A of
            // bd-root-relative-paths-design-fc5pvkcv): a logo authored
            // in a doc's frontmatter or a directory `_metadata.yml`
            // resolves against the authoring file's directory;
            // `_quarto.yml`-rooted values degrade to project-root-
            // relative unchanged. Each light/dark variant resolves
            // against its own authoring scalar's source.
            if let Some(logo) = navbar.logo.as_mut() {
                for variant in [&mut logo.light, &mut logo.dark] {
                    variant.path = resolve_metadata_path(
                        &variant.path,
                        &variant.source,
                        source_context,
                        &ctx.project.dir,
                    );
                }
            }
        }

        // Post-process with project-scoped data when we have a
        // ProjectIndex. Standalone single-doc renders (no index)
        // skip enrichment and active-marking and store the navbar
        // exactly as authored.
        if let Some(index) = ctx.project_index.as_deref() {
            // Drop links to unlinkable drafts (bd-2hy80x1j).
            prune_unlinkable_items(&mut navbar.left, index);
            prune_unlinkable_items(&mut navbar.right, index);
            enrich_navigation_items(&mut navbar.left, index);
            enrich_navigation_items(&mut navbar.right, index);
            let page_source = page_relative_source(ctx);
            mark_active(&mut navbar.left, &page_source);
            mark_active(&mut navbar.right, &page_source);
        }

        ast.meta
            .insert_path(&["navigation", "navbar"], navbar.to_config_value());

        Ok(())
    }
}

/// `Q-13-18` warnings for the navbar's breakpoint-keyed `text:` maps.
fn responsive_text_diagnostics(meta: &ConfigValue) -> Vec<DiagnosticMessage> {
    let Some(navbar_cv) = resolve_website_value(meta, "navbar") else {
        return Vec::new();
    };
    let valid = Breakpoint::ALL
        .iter()
        .map(|bp| format!("`{}`", bp.as_str()))
        .collect::<Vec<_>>()
        .join(", ");
    ["left", "right"]
        .into_iter()
        .filter_map(|side| navbar_cv.get(side))
        .flat_map(responsive_text_warnings)
        .map(|warning| match warning {
            ResponsiveTextWarning::UnknownBreakpoint { key, location } => {
                DiagnosticMessageBuilder::warning("Unknown breakpoint in navbar item text")
                    .with_code("Q-13-18")
                    .problem(format!(
                        "`{key}` is not a breakpoint name, so the text given for it is never \
                         shown."
                    ))
                    .add_hint(format!("The breakpoint names are {valid}."))
                    .with_location(location)
                    .build()
            }
            ResponsiveTextWarning::NoBreakpoints { location } => {
                DiagnosticMessageBuilder::warning("Unknown breakpoint in navbar item text")
                    .with_code("Q-13-18")
                    .problem(
                        "This navbar item's `text:` is a map, but none of its keys is a \
                         breakpoint name, so it has no text of its own.",
                    )
                    .add_hint(format!(
                        "Key each text variant by a breakpoint name ({valid}), or give \
                         `text:` a single string."
                    ))
                    .with_location(location)
                    .build()
            }
        })
        .collect()
}

/// Walk navbar items (including nested dropdown `menu` children) and
/// resolve each item's `href` against the YAML file it was authored
/// in. Mirrors `sidebar_generate::resolve_hrefs`.
fn resolve_item_hrefs(
    items: &mut [NavigationItem],
    source_context: &quarto_source_map::SourceContext,
    project_root: &std::path::Path,
) {
    for item in items.iter_mut() {
        if let Some(href) = item.href.as_ref() {
            let resolved =
                resolve_metadata_path(href, &item.href_source, source_context, project_root);
            item.href = Some(resolved);
        }
        if !item.menu.is_empty() {
            resolve_item_hrefs(&mut item.menu, source_context, project_root);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document_profile::DocumentProfile;
    use crate::format::Format;
    use crate::project::index::ProjectIndex;
    use crate::project::{DocumentInfo, ProjectConfig, ProjectContext};
    use crate::render::BinaryDependencies;
    use quarto_pandoc_types::ConfigMapEntry;
    use quarto_pandoc_types::config_value::ConfigValue;
    use quarto_source_map::SourceInfo;
    use std::path::PathBuf;
    use std::sync::Arc;

    fn make_test_project() -> ProjectContext {
        ProjectContext {
            dir: PathBuf::from("/project"),
            config: ProjectConfig::default(),
            is_single_file: true,
            files: vec![DocumentInfo::from_path("/project/doc.qmd")],
            output_dir: PathBuf::from("/project"),

            ..Default::default()
        }
    }

    fn config_map(entries: Vec<(&str, ConfigValue)>) -> ConfigValue {
        let map_entries: Vec<ConfigMapEntry> = entries
            .into_iter()
            .map(|(k, v)| ConfigMapEntry {
                key: k.to_string(),
                key_source: SourceInfo::for_test(),
                value: v,
            })
            .collect();
        ConfigValue::new_map(map_entries, SourceInfo::for_test())
    }

    fn bool_value(b: bool) -> ConfigValue {
        ConfigValue::new_bool(b, SourceInfo::for_test())
    }

    fn str_value(s: &str) -> ConfigValue {
        ConfigValue::new_string(s, SourceInfo::for_test())
    }

    fn array_value(items: Vec<ConfigValue>) -> ConfigValue {
        ConfigValue::new_array(items, SourceInfo::for_test())
    }

    fn make_profile(source: &str, output_href: &str, title: &str) -> DocumentProfile {
        DocumentProfile {
            source_path: PathBuf::from(source),
            output_href: output_href.to_string(),
            format_id: "html".to_string(),
            title: Some(crate::document_profile::text(title)),
            ..DocumentProfile::default()
        }
    }

    async fn run_transform(meta: ConfigValue) -> ConfigValue {
        run_transform_with(meta, None, "doc.qmd").await
    }

    async fn run_transform_with(
        meta: ConfigValue,
        index: Option<Arc<ProjectIndex>>,
        page: &str,
    ) -> ConfigValue {
        let mut ast = Pandoc {
            meta,
            blocks: vec![],
        };
        let project = ProjectContext {
            dir: PathBuf::from("/project"),
            config: ProjectConfig::default(),
            is_single_file: index.is_none(),
            files: vec![DocumentInfo::from_path(format!("/project/{}", page))],
            output_dir: PathBuf::from("/project/_site"),

            ..Default::default()
        };
        let doc = DocumentInfo::from_path(format!("/project/{}", page));
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);
        if let Some(idx) = index {
            ctx = ctx.with_project_index(idx);
        }
        NavbarGenerateTransform::new()
            .transform(&mut ast, &mut ctx)
            .await
            .unwrap();
        ast.meta
    }

    // bd-navbar-responsive-text-am98rw95 — breakpoint-keyed text.

    /// Run the transform on `meta` and return its diagnostics' codes and
    /// problem text, plus the stored navbar.
    async fn run_collecting_diagnostics(meta: ConfigValue) -> (Vec<String>, ConfigValue) {
        let mut ast = Pandoc {
            meta,
            blocks: vec![],
        };
        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);
        NavbarGenerateTransform::new()
            .transform(&mut ast, &mut ctx)
            .await
            .unwrap();
        let diags = ctx
            .diagnostics
            .iter()
            .map(|d| format!("{} {}", d.code.clone().unwrap_or_default(), d.to_text(None)))
            .collect();
        (diags, ast.meta)
    }

    #[tokio::test]
    async fn responsive_text_variants_survive_generate() {
        let item = config_map(vec![
            ("href", str_value("docs.qmd")),
            (
                "text",
                config_map(vec![
                    ("lg", str_value("Docs")),
                    ("xl", str_value("Documentation")),
                ]),
            ),
        ]);
        let meta = config_map(vec![(
            "website",
            config_map(vec![(
                "navbar",
                config_map(vec![("left", array_value(vec![item]))]),
            )]),
        )]);
        let (diags, out) = run_collecting_diagnostics(meta).await;
        assert!(diags.is_empty(), "{diags:?}");
        let stored = out.get_path(&["navigation", "navbar", "left"]).unwrap();
        let text = stored.as_array().unwrap()[0].get("text").unwrap();
        assert_eq!(
            text.get("lg").and_then(|v| v.as_plain_text()).as_deref(),
            Some("Docs")
        );
        assert_eq!(
            text.get("xl").and_then(|v| v.as_plain_text()).as_deref(),
            Some("Documentation")
        );
    }

    #[tokio::test]
    async fn responsive_text_unknown_keys_warn_q_13_18() {
        let typo = config_map(vec![
            ("href", str_value("a.qmd")),
            (
                "text",
                config_map(vec![("md2", str_value("A")), ("lg", str_value("B"))]),
            ),
        ]);
        let keyless = config_map(vec![
            ("text", str_value("Menu")),
            (
                "menu",
                array_value(vec![config_map(vec![
                    ("href", str_value("b.qmd")),
                    ("text", config_map(vec![("en", str_value("B"))])),
                ])]),
            ),
        ]);
        let meta = config_map(vec![(
            "navbar",
            config_map(vec![
                ("left", array_value(vec![typo])),
                ("right", array_value(vec![keyless])),
            ]),
        )]);
        let (diags, _) = run_collecting_diagnostics(meta).await;
        assert_eq!(diags.len(), 3, "{diags:#?}");
        assert!(
            diags.iter().all(|d| d.starts_with("Q-13-18 ")),
            "{diags:#?}"
        );
        assert!(diags[0].contains("`md2`"), "{}", diags[0]);
        assert!(diags[1].contains("`en`"), "{}", diags[1]);
        assert!(diags[2].contains("none of its keys"), "{}", diags[2]);
    }

    // --- Existing Phase 2 tests (preserved; Phase 3 doesn't touch
    // these behaviors since the YAML surface is unchanged). --------

    #[tokio::test]
    async fn skips_when_absent() {
        let meta = run_transform(ConfigValue::default()).await;
        assert!(!meta.contains_path(&["navigation", "navbar"]));
    }

    #[tokio::test]
    async fn skips_when_false() {
        let meta = run_transform(config_map(vec![("navbar", bool_value(false))])).await;
        assert!(!meta.contains_path(&["navigation", "navbar"]));
    }

    #[tokio::test]
    async fn skips_when_bare_true() {
        // `navbar: true` alone carries no content; resolve_navbar returns None.
        let meta = run_transform(config_map(vec![("navbar", bool_value(true))])).await;
        assert!(!meta.contains_path(&["navigation", "navbar"]));
    }

    #[tokio::test]
    async fn skips_when_navigation_navbar_already_set() {
        // Pre-populate navigation.navbar to simulate a user or prior filter
        // providing an override. The transform must not overwrite it.
        let pre = config_map(vec![("title", str_value("Pre-existing"))]);
        let mut meta = config_map(vec![(
            "navbar",
            config_map(vec![("title", str_value("Fresh"))]),
        )]);
        meta.insert_path(&["navigation", "navbar"], pre.clone());
        let out = run_transform(meta).await;
        let stored = out.get_path(&["navigation", "navbar"]).unwrap();
        assert_eq!(
            stored
                .get("title")
                .and_then(|v| v.as_plain_text())
                .as_deref(),
            Some("Pre-existing")
        );
    }

    #[tokio::test]
    async fn populates_navigation_navbar_from_full_config() {
        let navbar_cv = config_map(vec![
            ("title", str_value("My Site")),
            ("background", str_value("primary")),
            (
                "left",
                array_value(vec![str_value("index.qmd"), str_value("about.qmd")]),
            ),
        ]);
        let meta = run_transform(config_map(vec![("navbar", navbar_cv)])).await;
        assert!(meta.contains_path(&["navigation", "navbar"]));
        let stored = meta.get_path(&["navigation", "navbar"]).unwrap();
        assert_eq!(
            stored
                .get("background")
                .and_then(|v| v.as_plain_text())
                .as_deref(),
            Some("primary")
        );
        let left = stored.get("left").and_then(|v| v.as_array()).unwrap();
        assert_eq!(left.len(), 2);
    }

    // --- Phase 3 tests ---------------------------------------------

    /// Phase 3 test 23 — when a ProjectIndex is present and the
    /// current page's source path matches a navbar item, that item's
    /// `active` serializes as `true` in `navigation.navbar`.
    #[tokio::test]
    async fn navbar_generate_marks_active_item_for_current_page() {
        let meta = config_map(vec![(
            "navbar",
            config_map(vec![(
                "left",
                array_value(vec![str_value("index.qmd"), str_value("about.qmd")]),
            )]),
        )]);
        let index = Arc::new(ProjectIndex::new(vec![
            make_profile("index.qmd", "index.html", "Home"),
            make_profile("about.qmd", "about.html", "About"),
        ]));
        let meta = run_transform_with(meta, Some(index), "about.qmd").await;
        let stored = meta.get_path(&["navigation", "navbar"]).unwrap();
        let left = stored.get("left").and_then(|v| v.as_array()).unwrap();
        // First item (index.qmd) is not active; second (about.qmd) is.
        assert_eq!(
            left[0].get("active").and_then(|v| v.as_bool()),
            None,
            "inactive item should omit the active key"
        );
        assert_eq!(
            left[1].get("active").and_then(|v| v.as_bool()),
            Some(true),
            "matching item must be marked active"
        );
    }

    /// Phase 3 test 24 — without a ProjectIndex (standalone render),
    /// active marking is a no-op: no item carries `active: true`.
    #[tokio::test]
    async fn navbar_generate_does_not_mark_active_without_index() {
        let meta = config_map(vec![(
            "navbar",
            config_map(vec![(
                "left",
                array_value(vec![str_value("index.qmd"), str_value("about.qmd")]),
            )]),
        )]);
        let meta = run_transform_with(meta, None, "about.qmd").await;
        let stored = meta.get_path(&["navigation", "navbar"]).unwrap();
        let left = stored.get("left").and_then(|v| v.as_array()).unwrap();
        for item in left {
            assert_eq!(
                item.get("active").and_then(|v| v.as_bool()),
                None,
                "no active flag should be set without an index"
            );
        }
    }

    /// Phase 3 test 25 — bare-path navbar items get their `text` from
    /// the matching profile when a ProjectIndex is present.
    #[tokio::test]
    async fn navbar_generate_enriches_item_text_from_index() {
        let meta = config_map(vec![(
            "navbar",
            config_map(vec![("left", array_value(vec![str_value("about.qmd")]))]),
        )]);
        let index = Arc::new(ProjectIndex::new(vec![make_profile(
            "about.qmd",
            "about.html",
            "About Us",
        )]));
        let meta = run_transform_with(meta, Some(index), "index.qmd").await;
        let stored = meta.get_path(&["navigation", "navbar"]).unwrap();
        let left = stored.get("left").and_then(|v| v.as_array()).unwrap();
        assert_eq!(
            left[0]
                .get("text")
                .and_then(|v| v.as_plain_text())
                .as_deref(),
            Some("About Us"),
            "bare-href item should be enriched with profile title"
        );
    }

    /// Phase 3 test 26 — `.qmd` hrefs survive Generate. The resolved
    /// `navigation.navbar` still carries source paths; rewriting
    /// happens at Render time (format-agnostic invariant).
    #[tokio::test]
    async fn navbar_generate_keeps_qmd_paths() {
        let meta = config_map(vec![(
            "navbar",
            config_map(vec![("left", array_value(vec![str_value("about.qmd")]))]),
        )]);
        let index = Arc::new(ProjectIndex::new(vec![make_profile(
            "about.qmd",
            "about.html",
            "About",
        )]));
        let meta = run_transform_with(meta, Some(index), "index.qmd").await;
        let stored = meta.get_path(&["navigation", "navbar"]).unwrap();
        let left = stored.get("left").and_then(|v| v.as_array()).unwrap();
        assert_eq!(
            left[0]
                .get("href")
                .and_then(|v| v.as_plain_text())
                .as_deref(),
            Some("about.qmd"),
            "href should still be the source path (format-agnostic)"
        );
    }

    /// Phase 3 test 27 — standalone render: the navbar is stored
    /// verbatim, no enrichment, no active marking, no diagnostics.
    /// This is the revealjs / single-doc UX story.
    #[tokio::test]
    async fn navbar_generate_no_index_passes_through_unchanged() {
        let meta = config_map(vec![(
            "navbar",
            config_map(vec![(
                "left",
                array_value(vec![config_map(vec![
                    ("href", str_value("deck.qmd")),
                    ("text", str_value("Deck")),
                ])]),
            )]),
        )]);
        let meta = run_transform_with(meta, None, "deck.qmd").await;
        let stored = meta.get_path(&["navigation", "navbar"]).unwrap();
        let left = stored.get("left").and_then(|v| v.as_array()).unwrap();
        assert_eq!(
            left[0]
                .get("href")
                .and_then(|v| v.as_plain_text())
                .as_deref(),
            Some("deck.qmd"),
            "href unchanged"
        );
        assert_eq!(
            left[0]
                .get("text")
                .and_then(|v| v.as_plain_text())
                .as_deref(),
            Some("Deck"),
            "explicit text unchanged"
        );
        // No active flag — even though deck.qmd matches the page,
        // without an index the marking step doesn't run.
        assert_eq!(
            left[0].get("active").and_then(|v| v.as_bool()),
            None,
            "no active marking without ProjectIndex"
        );
    }

    // Keep the minimal harness around so the file compiles when the
    // above tests refer to it via `make_test_project`.
    #[allow(dead_code)]
    fn _unused_make_test_project() -> ProjectContext {
        make_test_project()
    }
}
