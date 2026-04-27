/*
 * sidebar_generate.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! Sidebar resolution transform.
//!
//! Reads `ast.meta.website.sidebar`, picks the sidebar that applies
//! to the current page, expands any `auto:` directives against the
//! [`ProjectIndex`], marks the active entry and its ancestors, and
//! stores the resolved sidebar at `navigation.sidebar`.
//!
//! This transform is **format-agnostic**. Sidebar entry hrefs remain
//! as author source paths (`about.qmd`). The HTML-specific
//! `.qmd → .html` rewrite happens later in
//! [`SidebarRenderTransform`](super::SidebarRenderTransform).
//!
//! See `claude-notes/plans/2026-04-24-websites-phase-2.md`
//! §Decision 7/8 for the Generate/Render split.
//!
//! ## Skip conditions
//!
//! - `sidebar: false` at the document level — affirmative disable.
//! - `navigation.sidebar` already populated — treat as user override.
//! - `website.sidebar` absent — nothing to resolve.

use quarto_navigation::{Sidebar, resolve_active_state, sidebar_for_page};
use quarto_pandoc_types::pandoc::Pandoc;

use crate::Result;
use crate::render::RenderContext;
use crate::transform::AstTransform;
use crate::transforms::is_feature_disabled;
use crate::transforms::navigation_active::page_relative_source;
use crate::transforms::sidebar_auto::{expand_auto, strip_auto};

pub struct SidebarGenerateTransform;

impl SidebarGenerateTransform {
    pub fn new() -> Self {
        Self
    }
}

impl Default for SidebarGenerateTransform {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait(?Send)]
impl AstTransform for SidebarGenerateTransform {
    fn name(&self) -> &str {
        "sidebar-generate"
    }

    async fn transform(&self, ast: &mut Pandoc, ctx: &mut RenderContext) -> Result<()> {
        if is_feature_disabled(&ast.meta, "sidebar") {
            return Ok(());
        }
        if ast.meta.contains_path(&["navigation", "sidebar"]) {
            // User filter or prior transform already provided a resolved
            // sidebar — don't clobber.
            return Ok(());
        }

        let Some(sidebar_cv) = ast.meta.get_path(&["website", "sidebar"]) else {
            return Ok(());
        };

        let sidebars = Sidebar::parse_list_from_config(sidebar_cv);
        if sidebars.is_empty() {
            return Ok(());
        }

        // The current page's project-relative source path, in
        // forward-slash form. `DocumentProfile.source_path` is
        // project-relative by construction (Phase 0); we derive the
        // same thing from `ctx.document.input` so the helpers don't
        // need to consult the index just to know "which page is this".
        let page_source = page_relative_source(ctx);

        let Some(picked) = sidebar_for_page(&sidebars, &page_source, &ast.meta) else {
            // No sidebar matches this page. That's not an error —
            // the template slot is conditional; absence is fine.
            return Ok(());
        };

        let mut resolved: Sidebar = picked.clone();

        // Pull diagnostics out of RenderContext temporarily so
        // helpers can borrow it mutably. We put them back before
        // returning.
        let mut local_diags = std::mem::take(&mut ctx.diagnostics);

        if let Some(index) = ctx.project_index.as_deref() {
            expand_auto(&mut resolved, index, &mut local_diags);
            // Enrich hand-written bare-path entries (e.g. `- about.qmd`)
            // with the referenced document's title when no `text:`
            // was supplied. Format-agnostic: fills in `text`, never
            // touches `href`.
            enrich_text_from_index(&mut resolved.contents, index);
            // Active-state: does the current page appear in this
            // sidebar's resolved contents? If so, mark and expand.
            let _ = resolve_active_state(&mut resolved, &page_source);
        } else {
            // Standalone render: no project index, so any `Auto`
            // entries can't be expanded. They are dropped with a
            // warning.
            strip_auto(&mut resolved, &mut local_diags);
        }

        ctx.diagnostics = local_diags;

        ast.meta
            .insert_path(&["navigation", "sidebar"], resolved.to_config_value());

        Ok(())
    }
}

/// Fill in `text` on any sidebar entry that carries only an `href`
/// (the common "bare path" YAML shorthand: `- about.qmd`).
///
/// `Link` entries delegate to the shared navbar/footer helper
/// ([`super::navigation_enrich::enrich_one`]). `Section` entries
/// have their own enrichment path (sections can be text-less with an
/// href, same shape as a Link but wrapped in the enum), then recurse
/// into children.
fn enrich_text_from_index(
    entries: &mut [quarto_navigation::SidebarEntry],
    index: &crate::project::index::ProjectIndex,
) {
    use quarto_navigation::SidebarEntry;
    use quarto_pandoc_types::config_value::ConfigValue;
    use quarto_source_map::SourceInfo;

    for entry in entries.iter_mut() {
        match entry {
            SidebarEntry::Link { item } => {
                super::navigation_enrich::enrich_one(item, index);
            }
            SidebarEntry::Section {
                text,
                href,
                contents,
                ..
            } => {
                // If the section has an href and no text, pull the
                // index profile's title.
                if text.is_none() {
                    if let Some(h) = href.as_deref() {
                        if let Some(profile) = index.lookup_by_source(std::path::Path::new(h)) {
                            if let Some(title) = &profile.title {
                                *text = Some(ConfigValue::new_string(title, SourceInfo::default()));
                            }
                        }
                    }
                }
                enrich_text_from_index(contents, index);
            }
            SidebarEntry::Separator | SidebarEntry::Heading(_) | SidebarEntry::Auto(_) => {}
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

    fn config_map(entries: Vec<(&str, ConfigValue)>) -> ConfigValue {
        let map_entries: Vec<ConfigMapEntry> = entries
            .into_iter()
            .map(|(k, v)| ConfigMapEntry {
                key: k.to_string(),
                key_source: SourceInfo::default(),
                value: v,
            })
            .collect();
        ConfigValue::new_map(map_entries, SourceInfo::default())
    }

    fn s(x: &str) -> ConfigValue {
        ConfigValue::new_string(x, SourceInfo::default())
    }

    fn b(x: bool) -> ConfigValue {
        ConfigValue::new_bool(x, SourceInfo::default())
    }

    fn arr(items: Vec<ConfigValue>) -> ConfigValue {
        ConfigValue::new_array(items, SourceInfo::default())
    }

    fn make_profile(source: &str, title: &str) -> DocumentProfile {
        DocumentProfile {
            source_path: PathBuf::from(source),
            output_href: source.replace(".qmd", ".html"),
            format_id: "html".to_string(),
            title: Some(title.to_string()),
            ..DocumentProfile::default()
        }
    }

    fn make_project() -> ProjectContext {
        ProjectContext {
            dir: PathBuf::from("/project"),
            config: ProjectConfig::default(),
            is_single_file: false,
            files: vec![DocumentInfo::from_path("/project/about.qmd")],
            output_dir: PathBuf::from("/project/_site"),
        }
    }

    async fn run_transform_with(
        meta: ConfigValue,
        index: Option<Arc<ProjectIndex>>,
        page: &str,
    ) -> (ConfigValue, Vec<quarto_error_reporting::DiagnosticMessage>) {
        let mut ast = Pandoc {
            meta,
            blocks: vec![],
        };
        let project = make_project();
        let doc = DocumentInfo::from_path(format!("/project/{}", page));
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);
        if let Some(idx) = index {
            ctx = ctx.with_project_index(idx);
        }
        SidebarGenerateTransform::new()
            .transform(&mut ast, &mut ctx)
            .await
            .unwrap();
        (ast.meta, ctx.diagnostics)
    }

    /// Test 30 — `sidebar: false` at document level suppresses Generate.
    #[tokio::test]
    async fn sidebar_generate_skips_when_feature_disabled() {
        let sidebar_cv = config_map(vec![("contents", arr(vec![s("about.qmd")]))]);
        let meta = config_map(vec![
            ("sidebar", b(false)),
            ("website", config_map(vec![("sidebar", sidebar_cv)])),
        ]);
        let (out, _diags) = run_transform_with(meta, None, "about.qmd").await;
        assert!(!out.contains_path(&["navigation", "sidebar"]));
    }

    /// Test 31 — no `website.sidebar` means no `navigation.sidebar`.
    #[tokio::test]
    async fn sidebar_generate_skips_when_absent() {
        let meta = config_map(vec![]);
        let (out, _diags) = run_transform_with(meta, None, "about.qmd").await;
        assert!(!out.contains_path(&["navigation", "sidebar"]));
    }

    /// Test 32 — a pre-populated `navigation.sidebar` survives the
    /// transform untouched.
    #[tokio::test]
    async fn sidebar_generate_honors_user_override() {
        let sidebar_cv = config_map(vec![("contents", arr(vec![s("about.qmd")]))]);
        let pre = config_map(vec![("contents", arr(vec![s("PRE")]))]);
        let mut meta = config_map(vec![("website", config_map(vec![("sidebar", sidebar_cv)]))]);
        meta.insert_path(&["navigation", "sidebar"], pre);
        let (out, _diags) = run_transform_with(meta, None, "about.qmd").await;
        let stored = out.get_path(&["navigation", "sidebar"]).unwrap();
        let contents = stored.get("contents").and_then(|v| v.as_array()).unwrap();
        assert_eq!(
            contents[0].as_plain_text().as_deref(),
            Some("PRE"),
            "user-supplied navigation.sidebar must win"
        );
    }

    /// Test 33 — end-to-end: YAML in → resolved tree out, with auto
    /// expanded and active marked.
    #[tokio::test]
    async fn sidebar_generate_produces_resolved_tree() {
        let sidebar_cv = config_map(vec![(
            "contents",
            arr(vec![config_map(vec![("auto", b(true))])]),
        )]);
        let meta = config_map(vec![("website", config_map(vec![("sidebar", sidebar_cv)]))]);

        let index = Arc::new(ProjectIndex::new(vec![
            make_profile("index.qmd", "Home"),
            make_profile("about.qmd", "About"),
        ]));

        let (out, _diags) = run_transform_with(meta, Some(index), "about.qmd").await;
        let stored = out.get_path(&["navigation", "sidebar"]).unwrap();

        // auto: true expanded — the index.qmd should be skipped
        // (top-level index exclusion), so we end up with just
        // `about.qmd`.
        let contents = stored.get("contents").and_then(|v| v.as_array()).unwrap();
        assert_eq!(contents.len(), 1);

        // The remaining entry is for `about.qmd`, and it must be
        // marked active because that's the current page.
        let entry = &contents[0];
        assert_eq!(
            entry.get("href").and_then(|v| v.as_plain_text()).as_deref(),
            Some("about.qmd"),
            "Generate keeps source path (format-agnostic); got: {:?}",
            entry
        );
        assert_eq!(
            entry.get("active").and_then(|v| v.as_bool()),
            Some(true),
            "current page's sidebar entry should be active"
        );
    }

    /// With no ProjectIndex, `auto:` entries drop with a warning
    /// diagnostic but the rest of the sidebar still resolves.
    #[tokio::test]
    async fn sidebar_generate_drops_auto_without_index() {
        let sidebar_cv = config_map(vec![(
            "contents",
            arr(vec![s("intro.qmd"), config_map(vec![("auto", b(true))])]),
        )]);
        let meta = config_map(vec![("website", config_map(vec![("sidebar", sidebar_cv)]))]);
        let (out, diags) = run_transform_with(meta, None, "intro.qmd").await;
        let stored = out.get_path(&["navigation", "sidebar"]).unwrap();
        let contents = stored.get("contents").and_then(|v| v.as_array()).unwrap();
        assert_eq!(contents.len(), 1, "only the hand-written entry remains");
        assert!(
            diags.iter().any(|d| d.title.contains("no project index")),
            "should warn about the dropped auto entry; got: {:?}",
            diags
        );
    }

    /// Source paths are preserved (format-agnostic invariant).
    #[tokio::test]
    async fn sidebar_generate_keeps_qmd_paths() {
        let sidebar_cv = config_map(vec![(
            "contents",
            arr(vec![s("index.qmd"), s("about.qmd")]),
        )]);
        let meta = config_map(vec![("website", config_map(vec![("sidebar", sidebar_cv)]))]);
        let (out, _diags) = run_transform_with(meta, None, "about.qmd").await;
        let stored = out.get_path(&["navigation", "sidebar"]).unwrap();
        let contents = stored.get("contents").and_then(|v| v.as_array()).unwrap();
        for entry in contents {
            let href = entry.get("href").and_then(|v| v.as_plain_text()).unwrap();
            assert!(
                href.ends_with(".qmd"),
                "Generate must not rewrite .qmd to .html; got: {}",
                href
            );
        }
    }

    /// Bare-path entries (`- about.qmd`, no `text:`) get their text
    /// filled in from the referenced profile's title. This keeps the
    /// rewrite in Render format-agnostic while still giving users
    /// readable sidebars from the common shorthand.
    #[tokio::test]
    async fn sidebar_generate_enriches_missing_text_from_index() {
        let sidebar_cv = config_map(vec![(
            "contents",
            arr(vec![s("index.qmd"), s("about.qmd")]),
        )]);
        let meta = config_map(vec![("website", config_map(vec![("sidebar", sidebar_cv)]))]);
        let index = Arc::new(ProjectIndex::new(vec![
            make_profile("index.qmd", "Home"),
            make_profile("about.qmd", "About Us"),
        ]));
        let (out, _diags) = run_transform_with(meta, Some(index), "about.qmd").await;
        let stored = out.get_path(&["navigation", "sidebar"]).unwrap();
        let contents = stored.get("contents").and_then(|v| v.as_array()).unwrap();
        for entry in contents {
            let text = entry
                .get("text")
                .and_then(|v| v.as_plain_text())
                .expect("bare-path entry should gain a text field from the profile title");
            assert!(
                text == "Home" || text == "About Us",
                "unexpected enriched text: {}",
                text
            );
        }
    }

    /// If the user supplied their own `text:`, the enrichment pass
    /// does not clobber it.
    #[tokio::test]
    async fn sidebar_generate_does_not_clobber_explicit_text() {
        let sidebar_cv = config_map(vec![(
            "contents",
            arr(vec![config_map(vec![
                ("href", s("about.qmd")),
                ("text", s("User Text")),
            ])]),
        )]);
        let meta = config_map(vec![("website", config_map(vec![("sidebar", sidebar_cv)]))]);
        let index = Arc::new(ProjectIndex::new(vec![make_profile(
            "about.qmd",
            "Profile Title",
        )]));
        let (out, _diags) = run_transform_with(meta, Some(index), "about.qmd").await;
        let stored = out.get_path(&["navigation", "sidebar"]).unwrap();
        let contents = stored.get("contents").and_then(|v| v.as_array()).unwrap();
        let text = contents[0]
            .get("text")
            .and_then(|v| v.as_plain_text())
            .unwrap();
        assert_eq!(text, "User Text");
    }
}
