/*
 * sidebar_render.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! HTML rendering transform for the sidebar.
//!
//! Reads the resolved `navigation.sidebar` (populated by
//! [`SidebarGenerateTransform`](super::SidebarGenerateTransform) or
//! a user override) and produces two pieces of rendered metadata for
//! the template:
//!
//! - `rendered.navigation.sidebar` — the sidebar HTML fragment, with
//!   project-relative `.qmd` hrefs rewritten to their output hrefs
//!   via the [`ProjectIndex`].
//! - `rendered.navigation.body-classes` — the CSS classes the body
//!   element needs so the SCSS grid layout produces a left sidebar
//!   column. Currently `"nav-sidebar floating"` (or `"nav-sidebar
//!   docked"`), driven by [`Sidebar::style`]. Without these classes
//!   the sidebar falls below the page content; see bd-mgoh.
//!
//! This transform **is** format-specific — it's the stage where the
//! format-agnostic `Sidebar` is committed to HTML output. See
//! `claude-notes/plans/2026-04-24-websites-phase-2.md` §Decision 7/8.
//!
//! ## Skip conditions
//!
//! - `sidebar: false` at the document level → neither output is set.
//! - `navigation.sidebar` absent → neither output is set.
//! - `rendered.navigation.sidebar` already populated → the HTML
//!   output is left alone (user override). The body-classes output
//!   is computed independently with its own user-override check, so
//!   a user filter can replace one without losing the other.
//! - `rendered.navigation.body-classes` already populated → the
//!   body-classes output is left alone (user override).

use quarto_error_reporting::DiagnosticMessage;
use quarto_navigation::{Sidebar, SidebarEntry, render_html::sidebar_to_html};
use quarto_pandoc_types::config_value::ConfigValue;
use quarto_pandoc_types::pandoc::Pandoc;
use quarto_source_map::{By, SourceInfo};

use crate::Result;
use crate::project::index::ProjectIndex;
use crate::render::RenderContext;
use crate::resource_resolver::ResourceResolverContext;
use crate::transform::AstTransform;
use crate::transforms::is_feature_disabled;
use crate::transforms::navigation_href::{NavSurface, resolve_href_for_html};

pub struct SidebarRenderTransform;

impl SidebarRenderTransform {
    pub fn new() -> Self {
        Self
    }
}

impl Default for SidebarRenderTransform {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait(?Send)]
impl AstTransform for SidebarRenderTransform {
    fn name(&self) -> &str {
        "sidebar-render"
    }

    async fn transform(&self, ast: &mut Pandoc, ctx: &mut RenderContext) -> Result<()> {
        if is_feature_disabled(&ast.meta, "sidebar") {
            return Ok(());
        }

        let Some(sidebar_cv) = ast.meta.get_path(&["navigation", "sidebar"]) else {
            return Ok(());
        };

        let mut sidebar = Sidebar::from_config_value(sidebar_cv);

        // Body classes drive the SCSS grid layout (see
        // resources/scss/bootstrap/_bootstrap-rules.scss: body.floating
        // selects the page-columns-float-wide() mixin which provides
        // the left sidebar column). Compute independently of the
        // sidebar HTML so a user filter can override one without losing
        // the other; honor any pre-existing override.
        if !ast
            .meta
            .contains_path(&["rendered", "navigation", "body-classes"])
        {
            let body_classes = format!("nav-sidebar {}", sidebar.style.as_str());
            ast.meta.insert_path(
                &["rendered", "navigation", "body-classes"],
                ConfigValue::new_string(
                    &body_classes,
                    SourceInfo::generated(By::programmatic_config()),
                ),
            );
        }

        // Sidebar HTML — skip if already pre-rendered (user override).
        if ast
            .meta
            .contains_path(&["rendered", "navigation", "sidebar"])
        {
            return Ok(());
        }

        let sidebar_id = sidebar.id.clone();

        // Rewrite hrefs in-place via ProjectIndex. Diagnostics land in
        // `ctx.diagnostics` via a local buffer that we swap in/out
        // so the helpers can push without a borrow cycle.
        let mut local_diags = std::mem::take(&mut ctx.diagnostics);
        let surface = NavSurface::Sidebar {
            id: sidebar_id.as_deref(),
        };
        rewrite_hrefs(
            &mut sidebar.contents,
            ctx.resource_resolver.as_ref(),
            ctx.project_index.as_deref(),
            surface,
            &mut local_diags,
        );
        ctx.diagnostics = local_diags;

        // Compute the page-relative URL of the site root directory
        // for the sidebar title's home link. Without a resolver
        // (single-doc / out-of-band callers) fall back to `./`,
        // which is correct at the project root. See bd-jgeu.
        let home_url = ctx
            .resource_resolver
            .as_ref()
            .map(|r| r.page_url_for_site_root_dir())
            .unwrap_or_else(|| "./".to_string());
        let html = sidebar_to_html(&sidebar, &home_url);

        ast.meta.insert_path(
            &["rendered", "navigation", "sidebar"],
            ConfigValue::new_string(&html, SourceInfo::generated(By::programmatic_config())),
        );

        Ok(())
    }
}

/// Walk the sidebar, rewriting each entry's href from source-path to
/// output-href via the project index, and relativizing the result
/// to the current page via the resolver. See bd-swpy /
/// `claude-notes/plans/2026-04-29-bd-swpy-nav-href-relativization.md`.
fn rewrite_hrefs(
    entries: &mut [SidebarEntry],
    resolver: Option<&ResourceResolverContext>,
    index: Option<&ProjectIndex>,
    surface: NavSurface<'_>,
    diagnostics: &mut Vec<DiagnosticMessage>,
) {
    for entry in entries.iter_mut() {
        match entry {
            SidebarEntry::Link { item } => {
                if let Some(href) = item.href.as_mut() {
                    // bd-qor9a — pass the href's SourceInfo through to
                    // `resolve_href_for_html` so any missing-document
                    // diagnostic (Q-13-1) carries the YAML scalar's
                    // location.
                    let location = Some(item.href_source.clone());
                    *href = resolve_href_for_html(
                        href,
                        resolver,
                        index,
                        surface.clone(),
                        location,
                        diagnostics,
                    );
                }
            }
            SidebarEntry::Section {
                href,
                href_source,
                contents,
                ..
            } => {
                if let Some(h) = href.as_mut() {
                    let location = Some(href_source.clone());
                    *h = resolve_href_for_html(
                        h,
                        resolver,
                        index,
                        surface.clone(),
                        location,
                        diagnostics,
                    );
                }
                rewrite_hrefs(contents, resolver, index, surface.clone(), diagnostics);
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
    use crate::project::{DocumentInfo, ProjectConfig, ProjectContext};
    use crate::render::BinaryDependencies;
    use quarto_navigation::{NavigationItem, Sidebar, SidebarEntry, SidebarStyle};
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
                key_source: SourceInfo::for_test(),
                value: v,
            })
            .collect();
        ConfigValue::new_map(map_entries, SourceInfo::for_test())
    }

    fn s(x: &str) -> ConfigValue {
        ConfigValue::new_string(x, SourceInfo::for_test())
    }

    fn b(x: bool) -> ConfigValue {
        ConfigValue::new_bool(x, SourceInfo::for_test())
    }

    fn make_profile(source: &str, output_href: &str, title: &str) -> DocumentProfile {
        DocumentProfile {
            source_path: PathBuf::from(source),
            output_href: output_href.to_string(),
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

    /// Build a minimal `navigation.sidebar` ConfigValue holding a list
    /// of links with the given hrefs.
    fn sidebar_with_links(hrefs: &[&str]) -> ConfigValue {
        let entries: Vec<ConfigValue> = hrefs.iter().map(|h| s(h)).collect();
        config_map(vec![(
            "contents",
            ConfigValue::new_array(entries, SourceInfo::for_test()),
        )])
    }

    async fn run_render(
        meta: ConfigValue,
        index: Option<Arc<ProjectIndex>>,
    ) -> (ConfigValue, Vec<DiagnosticMessage>) {
        let mut ast = Pandoc {
            meta,
            blocks: vec![],
        };
        let project = make_project();
        let doc = DocumentInfo::from_path("/project/about.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);
        if let Some(idx) = index {
            ctx = ctx.with_project_index(idx);
        }
        SidebarRenderTransform::new()
            .transform(&mut ast, &mut ctx)
            .await
            .unwrap();
        (ast.meta, ctx.diagnostics)
    }

    /// Test 28 — .qmd leaf href gets rewritten to the profile's
    /// output href.
    #[tokio::test]
    async fn render_rewrites_qmd_hrefs_to_output_href() {
        let meta = ConfigValue::default();
        let mut meta = meta;
        meta.insert_path(
            &["navigation", "sidebar"],
            sidebar_with_links(&["about.qmd"]),
        );
        let index = Arc::new(ProjectIndex::new(vec![make_profile(
            "about.qmd",
            "about.html",
            "About",
        )]));
        let (out, diags) = run_render(meta, Some(index)).await;
        let html = out
            .get_path(&["rendered", "navigation", "sidebar"])
            .unwrap()
            .as_plain_text()
            .unwrap();
        assert!(
            html.contains("href=\"about.html\""),
            "expected rewritten href; got: {}",
            html
        );
        assert!(!html.contains("href=\"about.qmd\""));
        assert!(
            diags.is_empty(),
            "no diagnostics expected; got: {:?}",
            diags
        );
    }

    /// Test 28a — subdirectory-scoped source paths are rewritten
    /// preserving the subdirectory structure.
    #[tokio::test]
    async fn render_rewrites_nested_qmd_hrefs() {
        let mut meta = ConfigValue::default();
        meta.insert_path(
            &["navigation", "sidebar"],
            sidebar_with_links(&["docs/api.qmd"]),
        );
        let index = Arc::new(ProjectIndex::new(vec![make_profile(
            "docs/api.qmd",
            "docs/api.html",
            "API",
        )]));
        let (out, _) = run_render(meta, Some(index)).await;
        let html = out
            .get_path(&["rendered", "navigation", "sidebar"])
            .unwrap()
            .as_plain_text()
            .unwrap();
        assert!(html.contains("href=\"docs/api.html\""), "html: {}", html);
    }

    /// Test 28b — external URLs pass through untouched.
    #[tokio::test]
    async fn render_passes_external_urls_through_unchanged() {
        let mut meta = ConfigValue::default();
        meta.insert_path(
            &["navigation", "sidebar"],
            sidebar_with_links(&["https://example.com"]),
        );
        let index = Arc::new(ProjectIndex::new(vec![]));
        let (out, diags) = run_render(meta, Some(index)).await;
        let html = out
            .get_path(&["rendered", "navigation", "sidebar"])
            .unwrap()
            .as_plain_text()
            .unwrap();
        assert!(html.contains("href=\"https://example.com\""));
        assert!(diags.is_empty());
    }

    /// Test 28c — fragment anchors pass through untouched (and no
    /// diagnostic).
    #[tokio::test]
    async fn render_passes_fragment_anchors_unchanged() {
        let mut meta = ConfigValue::default();
        meta.insert_path(
            &["navigation", "sidebar"],
            sidebar_with_links(&["#section"]),
        );
        let (out, diags) = run_render(meta, None).await;
        let html = out
            .get_path(&["rendered", "navigation", "sidebar"])
            .unwrap()
            .as_plain_text()
            .unwrap();
        assert!(html.contains("href=\"#section\""));
        assert!(diags.is_empty());
    }

    /// Test 29 — a missing .qmd reference emits a structured Q-13-1
    /// diagnostic; the href is preserved (dangling link, for
    /// transparency). (bd-8d6rk migration: assert on `code` and on
    /// `problem` rather than title-substring.)
    #[tokio::test]
    async fn render_qmd_href_lookup_miss_emits_diagnostic() {
        let mut meta = ConfigValue::default();
        meta.insert_path(
            &["navigation", "sidebar"],
            sidebar_with_links(&["missing.qmd"]),
        );
        let index = Arc::new(ProjectIndex::new(vec![]));
        let (out, diags) = run_render(meta, Some(index)).await;
        let html = out
            .get_path(&["rendered", "navigation", "sidebar"])
            .unwrap()
            .as_plain_text()
            .unwrap();
        assert!(html.contains("href=\"missing.qmd\""));
        assert_eq!(diags.len(), 1);
        let d = &diags[0];
        assert_eq!(d.code.as_deref(), Some("Q-13-1"));
        assert!(d.title.starts_with("Sidebar"), "got title: {:?}", d.title);
        assert!(
            d.problem
                .as_ref()
                .map(|p| p.as_str().contains("missing.qmd"))
                .unwrap_or(false),
            "Q-13-1 problem must mention missing.qmd; got {:?}",
            d.problem
        );
    }

    /// Test 29a — no `ProjectIndex` just passes hrefs through. Raw
    /// external URLs still render; no diagnostics for them.
    #[tokio::test]
    async fn render_works_without_project_index() {
        let mut meta = ConfigValue::default();
        meta.insert_path(
            &["navigation", "sidebar"],
            sidebar_with_links(&["https://example.com", "#foo"]),
        );
        let (out, diags) = run_render(meta, None).await;
        let html = out
            .get_path(&["rendered", "navigation", "sidebar"])
            .unwrap()
            .as_plain_text()
            .unwrap();
        assert!(html.contains("href=\"https://example.com\""));
        assert!(html.contains("href=\"#foo\""));
        assert!(diags.is_empty());
    }

    /// Test 34 — no `navigation.sidebar` means no `rendered.navigation.sidebar`.
    #[tokio::test]
    async fn sidebar_render_skips_when_missing() {
        let meta = ConfigValue::default();
        let (out, _) = run_render(meta, None).await;
        assert!(!out.contains_path(&["rendered", "navigation", "sidebar"]));
    }

    /// Test 35 — end-to-end: `navigation.sidebar` produces HTML at
    /// `rendered.navigation.sidebar`.
    #[tokio::test]
    async fn sidebar_render_produces_html() {
        let mut meta = ConfigValue::default();
        let sb = Sidebar {
            contents: vec![SidebarEntry::Link {
                item: NavigationItem {
                    href: Some("about.qmd".to_string()),
                    text: Some(s("About")),
                    active: true,
                    ..NavigationItem::default()
                },
            }],
            ..Sidebar::with_defaults()
        };
        meta.insert_path(&["navigation", "sidebar"], sb.to_config_value());
        let index = Arc::new(ProjectIndex::new(vec![make_profile(
            "about.qmd",
            "about.html",
            "About",
        )]));
        let (out, _) = run_render(meta, Some(index)).await;
        let html = out
            .get_path(&["rendered", "navigation", "sidebar"])
            .unwrap()
            .as_plain_text()
            .unwrap();
        assert!(html.contains("<nav id=\"quarto-sidebar\""));
        assert!(html.contains("href=\"about.html\""));
        assert!(html.contains("sidebar-link active"));
    }

    /// `sidebar: false` at document level suppresses render.
    #[tokio::test]
    async fn sidebar_render_skips_when_feature_disabled() {
        let mut meta = config_map(vec![("sidebar", b(false))]);
        let sb = Sidebar {
            contents: vec![SidebarEntry::Link {
                item: NavigationItem {
                    href: Some("a.qmd".to_string()),
                    text: Some(s("A")),
                    ..NavigationItem::default()
                },
            }],
            ..Sidebar::with_defaults()
        };
        meta.insert_path(&["navigation", "sidebar"], sb.to_config_value());
        let (out, _) = run_render(meta, None).await;
        assert!(!out.contains_path(&["rendered", "navigation", "sidebar"]));
    }

    /// A pre-populated `rendered.navigation.sidebar` survives.
    #[tokio::test]
    async fn sidebar_render_honors_user_override() {
        let mut meta = ConfigValue::default();
        meta.insert_path(
            &["navigation", "sidebar"],
            sidebar_with_links(&["about.qmd"]),
        );
        meta.insert_path(
            &["rendered", "navigation", "sidebar"],
            s("<nav>User-provided</nav>"),
        );
        let (out, _) = run_render(meta, None).await;
        let rendered = out
            .get_path(&["rendered", "navigation", "sidebar"])
            .unwrap();
        assert_eq!(
            rendered.as_plain_text().as_deref(),
            Some("<nav>User-provided</nav>")
        );
    }

    /// Query strings survive rewrite.
    #[tokio::test]
    async fn render_preserves_query_and_fragment_after_rewrite() {
        let mut meta = ConfigValue::default();
        meta.insert_path(
            &["navigation", "sidebar"],
            sidebar_with_links(&["about.qmd#bio"]),
        );
        let index = Arc::new(ProjectIndex::new(vec![make_profile(
            "about.qmd",
            "about.html",
            "About",
        )]));
        let (out, _) = run_render(meta, Some(index)).await;
        let html = out
            .get_path(&["rendered", "navigation", "sidebar"])
            .unwrap()
            .as_plain_text()
            .unwrap();
        assert!(
            html.contains("href=\"about.html#bio\""),
            "expected fragment preserved; got: {}",
            html
        );
    }

    /// bd-swpy regression — when the page being rendered is one
    /// directory deep and a resolver is attached, sidebar hrefs to
    /// root-level targets must walk up one level. Without the fix,
    /// the helper emitted bare `about.html` (project-root-relative),
    /// which a browser at `/_site/guide/installation.html` would
    /// resolve as `/_site/guide/about.html` and 404.
    #[tokio::test]
    async fn render_relativizes_sidebar_hrefs_via_resolver_at_depth_one() {
        use crate::resource_resolver::ResourceResolverContext;

        let mut meta = ConfigValue::default();
        meta.insert_path(
            &["navigation", "sidebar"],
            sidebar_with_links(&["about.qmd"]),
        );
        let index = Arc::new(ProjectIndex::new(vec![make_profile(
            "about.qmd",
            "about.html",
            "About",
        )]));

        // Build the same context the project pipeline would: page
        // lives at `/project/_site/guide/installation.html`.
        let mut ast = Pandoc {
            meta,
            blocks: vec![],
        };
        let project = make_project();
        let doc = DocumentInfo::from_path("/project/guide/installation.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let resolver = ResourceResolverContext::website(
            "/project/_site",
            "/project/_site/guide/installation.html",
            "site_libs",
            "installation",
        );
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries)
            .with_project_index(index)
            .with_resource_resolver(resolver);
        SidebarRenderTransform::new()
            .transform(&mut ast, &mut ctx)
            .await
            .unwrap();
        let html = ast
            .meta
            .get_path(&["rendered", "navigation", "sidebar"])
            .unwrap()
            .as_plain_text()
            .unwrap();
        assert!(
            html.contains("href=\"../about.html\""),
            "expected page-relative href ../about.html; got: {}",
            html
        );
        assert!(
            !html.contains("href=\"about.html\""),
            "bare project-relative href should NOT appear; got: {}",
            html
        );
    }

    /// bd-jgeu test 13 — sidebar title's home link is page-relative.
    /// Page lives at depth 1; the title anchor must point to `../`,
    /// not the hardcoded `./` (which would loop back into the same
    /// directory).
    #[tokio::test]
    async fn render_relativizes_sidebar_title_home_link_at_depth_one() {
        use crate::resource_resolver::ResourceResolverContext;

        let mut meta = ConfigValue::default();
        // Sidebar with an explicit text title — required for the
        // header to be emitted at all.
        let sb = Sidebar {
            title: quarto_navigation::SidebarTitle::Text(s("Site")),
            contents: vec![SidebarEntry::Link {
                item: NavigationItem {
                    href: Some("about.qmd".to_string()),
                    text: Some(s("About")),
                    ..NavigationItem::default()
                },
            }],
            ..Sidebar::with_defaults()
        };
        meta.insert_path(&["navigation", "sidebar"], sb.to_config_value());

        let mut ast = Pandoc {
            meta,
            blocks: vec![],
        };
        let project = make_project();
        let doc = DocumentInfo::from_path("/project/guide/installation.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let resolver = ResourceResolverContext::website(
            "/project/_site",
            "/project/_site/guide/installation.html",
            "site_libs",
            "installation",
        );
        let mut ctx =
            RenderContext::new(&project, &doc, &format, &binaries).with_resource_resolver(resolver);
        SidebarRenderTransform::new()
            .transform(&mut ast, &mut ctx)
            .await
            .unwrap();
        let html = ast
            .meta
            .get_path(&["rendered", "navigation", "sidebar"])
            .unwrap()
            .as_plain_text()
            .unwrap();
        assert!(
            html.contains("<a href=\"../\">Site</a>"),
            "title home link should be page-relative ../; got: {}",
            html
        );
        assert!(
            !html.contains("<a href=\"./\">Site</a>"),
            "the hardcoded ./ fallback should not appear; got: {}",
            html
        );
    }

    /// bd-jgeu test 14 — when no resolver is attached (single-doc
    /// fallback / out-of-band callers) the title anchor falls back
    /// to `./` so behavior at the project root is preserved.
    #[tokio::test]
    async fn render_uses_dot_slash_home_link_when_no_resolver() {
        let mut meta = ConfigValue::default();
        let sb = Sidebar {
            title: quarto_navigation::SidebarTitle::Text(s("Site")),
            contents: vec![SidebarEntry::Link {
                item: NavigationItem {
                    href: Some("about.qmd".to_string()),
                    text: Some(s("About")),
                    ..NavigationItem::default()
                },
            }],
            ..Sidebar::with_defaults()
        };
        meta.insert_path(&["navigation", "sidebar"], sb.to_config_value());
        let (out, _) = run_render(meta, None).await;
        let html = out
            .get_path(&["rendered", "navigation", "sidebar"])
            .unwrap()
            .as_plain_text()
            .unwrap();
        assert!(
            html.contains("<a href=\"./\">Site</a>"),
            "no-resolver fallback should be ./; got: {}",
            html
        );
    }

    /// `bd-mgoh` — body-class derivation: a Floating sidebar yields
    /// `nav-sidebar floating` at `rendered.navigation.body-classes`.
    /// Q1's body-class set drives the SCSS grid layout (see
    /// resources/scss/bootstrap/_bootstrap-rules.scss); without these
    /// classes the sidebar lacks a left column and falls below the
    /// page content.
    #[tokio::test]
    async fn sidebar_render_writes_body_classes_floating() {
        let mut meta = ConfigValue::default();
        let sb = Sidebar {
            style: SidebarStyle::Floating,
            contents: vec![SidebarEntry::Link {
                item: NavigationItem {
                    href: Some("about.qmd".to_string()),
                    text: Some(s("About")),
                    ..NavigationItem::default()
                },
            }],
            ..Sidebar::with_defaults()
        };
        meta.insert_path(&["navigation", "sidebar"], sb.to_config_value());
        let (out, _) = run_render(meta, None).await;
        let body_classes = out
            .get_path(&["rendered", "navigation", "body-classes"])
            .and_then(|v| v.as_plain_text());
        assert_eq!(body_classes.as_deref(), Some("nav-sidebar floating"));
    }

    /// `bd-mgoh` — Docked sidebar yields `nav-sidebar docked`.
    #[tokio::test]
    async fn sidebar_render_writes_body_classes_docked() {
        let mut meta = ConfigValue::default();
        let sb = Sidebar {
            style: SidebarStyle::Docked,
            contents: vec![SidebarEntry::Link {
                item: NavigationItem {
                    href: Some("about.qmd".to_string()),
                    text: Some(s("About")),
                    ..NavigationItem::default()
                },
            }],
            ..Sidebar::with_defaults()
        };
        meta.insert_path(&["navigation", "sidebar"], sb.to_config_value());
        let (out, _) = run_render(meta, None).await;
        let body_classes = out
            .get_path(&["rendered", "navigation", "body-classes"])
            .and_then(|v| v.as_plain_text());
        assert_eq!(body_classes.as_deref(), Some("nav-sidebar docked"));
    }

    /// `bd-mgoh` — when `sidebar: false` suppresses the feature, no
    /// body-classes are written either.
    #[tokio::test]
    async fn sidebar_render_skips_body_classes_when_disabled() {
        let mut meta = config_map(vec![("sidebar", b(false))]);
        let sb = Sidebar {
            contents: vec![SidebarEntry::Link {
                item: NavigationItem {
                    href: Some("a.qmd".to_string()),
                    text: Some(s("A")),
                    ..NavigationItem::default()
                },
            }],
            ..Sidebar::with_defaults()
        };
        meta.insert_path(&["navigation", "sidebar"], sb.to_config_value());
        let (out, _) = run_render(meta, None).await;
        assert!(
            !out.contains_path(&["rendered", "navigation", "body-classes"]),
            "body-classes must not be set when sidebar feature is disabled"
        );
    }

    /// `bd-mgoh` — a pre-populated `rendered.navigation.body-classes`
    /// (e.g. from a user filter) survives. Mirrors the existing
    /// `rendered.navigation.sidebar` user-override behavior.
    #[tokio::test]
    async fn sidebar_render_honors_user_body_classes_override() {
        let mut meta = ConfigValue::default();
        meta.insert_path(
            &["navigation", "sidebar"],
            sidebar_with_links(&["about.qmd"]),
        );
        meta.insert_path(
            &["rendered", "navigation", "body-classes"],
            s("user-override-class"),
        );
        let (out, _) = run_render(meta, None).await;
        let body_classes = out
            .get_path(&["rendered", "navigation", "body-classes"])
            .and_then(|v| v.as_plain_text());
        assert_eq!(body_classes.as_deref(), Some("user-override-class"));
    }
}
