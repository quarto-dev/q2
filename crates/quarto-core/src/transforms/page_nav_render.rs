/*
 * page_nav_render.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! Page-navigation (prev/next) Render transform.
//!
//! Reads the resolved [`PageNavigation`] from
//! `navigation.page_navigation` (populated upstream by
//! [`PageNavGenerateTransform`](super::PageNavGenerateTransform) or a
//! user override), rewrites `.qmd` hrefs to output hrefs via the
//! [`ProjectIndex`](crate::project::index::ProjectIndex), and emits
//! HTML via [`page_navigation_to_html`](quarto_navigation::render_html::page_navigation_to_html).
//! The result lands at `rendered.navigation.page_navigation` for the
//! template slot to consume.
//!
//! See `claude-notes/plans/2026-04-24-websites-phase-4.md` §Decisions
//! 6–8.
//!
//! ## Skip conditions
//!
//! - `page-navigation: false` (affirmative disable).
//! - `rendered.navigation.page_navigation` already populated — user
//!   pre-rendered HTML.
//! - `navigation.page_navigation` absent — nothing to render.

use quarto_navigation::page_nav::PageNavigation;
use quarto_navigation::render_html::page_navigation_to_html;
use quarto_pandoc_types::config_value::ConfigValue;
use quarto_pandoc_types::pandoc::Pandoc;
use quarto_source_map::SourceInfo;

use crate::Result;
use crate::render::RenderContext;
use crate::transform::AstTransform;
use crate::transforms::is_feature_disabled;
use crate::transforms::navigation_href::resolve_href_for_html;

pub struct PageNavRenderTransform;

impl PageNavRenderTransform {
    pub fn new() -> Self {
        Self
    }
}

impl Default for PageNavRenderTransform {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait(?Send)]
impl AstTransform for PageNavRenderTransform {
    fn name(&self) -> &str {
        "page-nav-render"
    }

    async fn transform(&self, ast: &mut Pandoc, ctx: &mut RenderContext) -> Result<()> {
        if is_feature_disabled(&ast.meta, "page-navigation") {
            return Ok(());
        }

        if ast
            .meta
            .contains_path(&["rendered", "navigation", "page_navigation"])
        {
            return Ok(());
        }

        let Some(cv) = ast.meta.get_path(&["navigation", "page_navigation"]) else {
            return Ok(());
        };

        let mut page_nav = PageNavigation::from_config_value(cv);

        let mut local_diags = std::mem::take(&mut ctx.diagnostics);
        if let Some(item) = page_nav.prev.as_mut() {
            if let Some(href) = item.href.as_mut() {
                *href = resolve_href_for_html(
                    href,
                    ctx.project_index.as_deref(),
                    Some("Page navigation"),
                    &mut local_diags,
                );
            }
        }
        if let Some(item) = page_nav.next.as_mut() {
            if let Some(href) = item.href.as_mut() {
                *href = resolve_href_for_html(
                    href,
                    ctx.project_index.as_deref(),
                    Some("Page navigation"),
                    &mut local_diags,
                );
            }
        }
        ctx.diagnostics = local_diags;

        let html = page_navigation_to_html(&page_nav);
        ast.meta.insert_path(
            &["rendered", "navigation", "page_navigation"],
            ConfigValue::new_string(&html, SourceInfo::default()),
        );

        Ok(())
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
    use quarto_navigation::NavigationItem;
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

    /// Run the Render transform; returns (meta, diagnostics).
    async fn run(
        meta: ConfigValue,
        index: Option<Arc<ProjectIndex>>,
    ) -> (ConfigValue, Vec<quarto_error_reporting::DiagnosticMessage>) {
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
        PageNavRenderTransform::new()
            .transform(&mut ast, &mut ctx)
            .await
            .unwrap();
        (ast.meta, ctx.diagnostics)
    }

    fn page_nav_cv(prev_href: Option<&str>, next_href: Option<&str>) -> ConfigValue {
        let mut entries = Vec::new();
        if let Some(h) = prev_href {
            entries.push((
                "prev",
                config_map(vec![("href", s(h)), ("text", s("PrevText"))]),
            ));
        }
        if let Some(h) = next_href {
            entries.push((
                "next",
                config_map(vec![("href", s(h)), ("text", s("NextText"))]),
            ));
        }
        config_map(entries)
    }

    /// Test 31 — no `navigation.page_navigation` → no
    /// `rendered.navigation.page_navigation`.
    #[tokio::test]
    async fn page_nav_render_skips_when_absent() {
        let meta = config_map(vec![]);
        let (out, _diags) = run(meta, None).await;
        assert!(!out.contains_path(&["rendered", "navigation", "page_navigation"]));
    }

    /// Test 32 — `page-navigation: false` skips.
    #[tokio::test]
    async fn page_nav_render_skips_when_feature_disabled() {
        let mut meta = config_map(vec![("page-navigation", b(false))]);
        meta.insert_path(
            &["navigation", "page_navigation"],
            page_nav_cv(None, Some("about.qmd")),
        );
        let (out, _diags) = run(meta, None).await;
        assert!(!out.contains_path(&["rendered", "navigation", "page_navigation"]));
    }

    /// Test 33 — pre-existing rendered html survives.
    #[tokio::test]
    async fn page_nav_render_skips_when_already_prerendered() {
        let mut meta = config_map(vec![]);
        meta.insert_path(
            &["navigation", "page_navigation"],
            page_nav_cv(None, Some("about.qmd")),
        );
        meta.insert_path(
            &["rendered", "navigation", "page_navigation"],
            s("<!-- user-rendered -->"),
        );
        let (out, _diags) = run(meta, None).await;
        let stored = out
            .get_path(&["rendered", "navigation", "page_navigation"])
            .unwrap()
            .as_plain_text()
            .unwrap();
        assert_eq!(stored, "<!-- user-rendered -->");
    }

    /// Test 34 — `.qmd` hrefs are rewritten to output hrefs via
    /// ProjectIndex.
    #[tokio::test]
    async fn page_nav_render_rewrites_qmd_hrefs_to_html() {
        let mut meta = config_map(vec![]);
        meta.insert_path(
            &["navigation", "page_navigation"],
            page_nav_cv(Some("a.qmd"), Some("c.qmd")),
        );
        let index = Arc::new(ProjectIndex::new(vec![
            make_profile("a.qmd", "A"),
            make_profile("c.qmd", "C"),
        ]));
        let (out, diags) = run(meta, Some(index)).await;
        let html = out
            .get_path(&["rendered", "navigation", "page_navigation"])
            .unwrap()
            .as_plain_text()
            .unwrap();
        assert!(
            html.contains("href=\"a.html\""),
            "prev href should be rewritten to .html; got:\n{}",
            html
        );
        assert!(
            html.contains("href=\"c.html\""),
            "next href should be rewritten to .html; got:\n{}",
            html
        );
        assert!(diags.is_empty(), "no diagnostics expected; got {:?}", diags);
    }

    /// Test 35 — external URLs pass through unchanged (defensive —
    /// Generate filters externals, but a user filter could insert one).
    #[tokio::test]
    async fn page_nav_render_passes_external_urls_through() {
        let mut meta = config_map(vec![]);
        meta.insert_path(
            &["navigation", "page_navigation"],
            page_nav_cv(None, Some("https://example.com/")),
        );
        let (out, _diags) = run(meta, None).await;
        let html = out
            .get_path(&["rendered", "navigation", "page_navigation"])
            .unwrap()
            .as_plain_text()
            .unwrap();
        assert!(html.contains("href=\"https://example.com/\""));
    }

    /// Test 36 — a `.qmd` href not in the index produces a "Page
    /// navigation" diagnostic (source_label).
    #[tokio::test]
    async fn page_nav_render_emits_diagnostic_for_unknown_qmd() {
        let mut meta = config_map(vec![]);
        meta.insert_path(
            &["navigation", "page_navigation"],
            page_nav_cv(None, Some("missing.qmd")),
        );
        // Provide an empty index so the resolver can detect the miss
        // (the no-index branch is silent — see Test 37).
        let index = Arc::new(ProjectIndex::new(vec![make_profile("a.qmd", "A")]));
        let (_out, diags) = run(meta, Some(index)).await;
        assert!(
            diags.iter().any(|d| d.title.contains("Page navigation")),
            "expected a diagnostic mentioning 'Page navigation'; got {:?}",
            diags
        );
    }

    /// Test 37 — without a project index, hrefs pass through verbatim
    /// and no diagnostic is emitted.
    #[tokio::test]
    async fn page_nav_render_no_index_passes_hrefs_through() {
        let mut meta = config_map(vec![]);
        meta.insert_path(
            &["navigation", "page_navigation"],
            page_nav_cv(Some("a.qmd"), Some("c.qmd")),
        );
        let (out, diags) = run(meta, None).await;
        let html = out
            .get_path(&["rendered", "navigation", "page_navigation"])
            .unwrap()
            .as_plain_text()
            .unwrap();
        assert!(
            html.contains("href=\"a.qmd\""),
            "prev href should pass through as .qmd; got:\n{}",
            html
        );
        assert!(
            html.contains("href=\"c.qmd\""),
            "next href should pass through as .qmd; got:\n{}",
            html
        );
        assert!(
            diags.is_empty(),
            "no diagnostic without index; got {:?}",
            diags
        );
    }

    /// Test 38 — happy path: rendered HTML lands at the expected
    /// metadata path and contains the expected HTML structure.
    #[tokio::test]
    async fn page_nav_render_populates_rendered_slot() {
        let mut meta = config_map(vec![]);
        meta.insert_path(
            &["navigation", "page_navigation"],
            page_nav_cv(Some("a.qmd"), Some("c.qmd")),
        );
        let index = Arc::new(ProjectIndex::new(vec![
            make_profile("a.qmd", "A"),
            make_profile("c.qmd", "C"),
        ]));
        let (out, _diags) = run(meta, Some(index)).await;
        let html = out
            .get_path(&["rendered", "navigation", "page_navigation"])
            .unwrap()
            .as_plain_text()
            .unwrap();
        assert!(html.contains("<nav class=\"page-navigation\">"));
        assert!(html.contains("nav-page-previous"));
        assert!(html.contains("nav-page-next"));
        // Hrefs got rewritten on the way out.
        assert!(html.contains("href=\"a.html\""));
        assert!(html.contains("href=\"c.html\""));
    }

    /// Sanity: round-trip a NavigationItem with `text` through the
    /// render path so the visible span carries the text we asked for.
    #[tokio::test]
    async fn page_nav_render_emits_text_in_visible_span() {
        let mut meta = config_map(vec![]);
        meta.insert_path(
            &["navigation", "page_navigation"],
            page_nav_cv(None, Some("c.qmd")),
        );
        let index = Arc::new(ProjectIndex::new(vec![make_profile("c.qmd", "C")]));
        let (out, _diags) = run(meta, Some(index)).await;
        let html = out
            .get_path(&["rendered", "navigation", "page_navigation"])
            .unwrap()
            .as_plain_text()
            .unwrap();
        assert!(
            html.contains("<span class=\"nav-page-text\">NextText</span>"),
            "expected text in the visible span; got:\n{}",
            html
        );
        // Suppress dead-code warnings on the unused NavigationItem
        // import without rewiring the test scaffolding.
        let _ = NavigationItem::default();
    }
}
