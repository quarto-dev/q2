/*
 * sidebar_render.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! HTML rendering transform for the sidebar.
//!
//! Reads the resolved `navigation.sidebar` (populated by
//! [`SidebarGenerateTransform`](super::SidebarGenerateTransform) or
//! a user override), rewrites any project-relative `.qmd` hrefs to
//! their output hrefs via the [`ProjectIndex`], and emits the result
//! as HTML at `rendered.navigation.sidebar` for the template to
//! inject.
//!
//! This transform **is** format-specific — it's the stage where the
//! format-agnostic `Sidebar` is committed to HTML output. See
//! `claude-notes/plans/2026-04-24-websites-phase-2.md` §Decision 7/8.
//!
//! ## Skip conditions
//!
//! - `sidebar: false` at the document level.
//! - `rendered.navigation.sidebar` already populated (user
//!   pre-rendered HTML).
//! - `navigation.sidebar` absent.

use quarto_error_reporting::DiagnosticMessage;
use quarto_navigation::{Sidebar, SidebarEntry, render_html::sidebar_to_html};
use quarto_pandoc_types::config_value::ConfigValue;
use quarto_pandoc_types::pandoc::Pandoc;
use quarto_source_map::SourceInfo;

use crate::Result;
use crate::project::index::ProjectIndex;
use crate::render::RenderContext;
use crate::resource_resolver::ResourceResolverContext;
use crate::transform::AstTransform;
use crate::transforms::is_feature_disabled;
use crate::transforms::navigation_href::resolve_href_for_html;

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
        if ast
            .meta
            .contains_path(&["rendered", "navigation", "sidebar"])
        {
            return Ok(());
        }

        let Some(sidebar_cv) = ast.meta.get_path(&["navigation", "sidebar"]) else {
            return Ok(());
        };

        let mut sidebar = Sidebar::from_config_value(sidebar_cv);
        let sidebar_id = sidebar.id.clone();

        // Rewrite hrefs in-place via ProjectIndex. Diagnostics land in
        // `ctx.diagnostics` via a local buffer that we swap in/out
        // so the helpers can push without a borrow cycle.
        let mut local_diags = std::mem::take(&mut ctx.diagnostics);
        let label = sidebar_id
            .as_deref()
            .map(|id| format!("Sidebar '{}'", id))
            .unwrap_or_else(|| "Sidebar".to_string());
        rewrite_hrefs(
            &mut sidebar.contents,
            ctx.resource_resolver.as_ref(),
            ctx.project_index.as_deref(),
            Some(label.as_str()),
            &mut local_diags,
        );
        ctx.diagnostics = local_diags;

        let html = sidebar_to_html(&sidebar);

        ast.meta.insert_path(
            &["rendered", "navigation", "sidebar"],
            ConfigValue::new_string(&html, SourceInfo::default()),
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
    source_label: Option<&str>,
    diagnostics: &mut Vec<DiagnosticMessage>,
) {
    for entry in entries.iter_mut() {
        match entry {
            SidebarEntry::Link { item } => {
                if let Some(href) = item.href.as_mut() {
                    *href = resolve_href_for_html(href, resolver, index, source_label, diagnostics);
                }
            }
            SidebarEntry::Section { href, contents, .. } => {
                if let Some(h) = href.as_mut() {
                    *h = resolve_href_for_html(h, resolver, index, source_label, diagnostics);
                }
                rewrite_hrefs(contents, resolver, index, source_label, diagnostics);
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
    use quarto_navigation::{NavigationItem, Sidebar, SidebarEntry};
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
            ConfigValue::new_array(entries, SourceInfo::default()),
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

    /// Test 29 — a missing .qmd reference emits a diagnostic; the
    /// href is preserved (dangling link, for transparency).
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
        assert!(
            diags[0].title.contains("missing.qmd"),
            "warning should name the missing doc; got: {:?}",
            diags[0]
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
}
