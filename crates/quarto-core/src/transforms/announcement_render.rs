/*
 * announcement_render.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! HTML rendering transform for the announcement banner
//! (bd-website-announcement-w1gtpzki).
//!
//! Reads the resolved structure from `navigation.announcement` (populated
//! by [`AnnouncementGenerateTransform`](super::AnnouncementGenerateTransform)
//! or a user override), rewrites Link/Image targets inside the content to
//! page-relative output hrefs (`.qmd` → output via the
//! [`ProjectIndex`](crate::project::index::ProjectIndex); misses warn
//! `Q-13-15`), emits HTML via
//! [`quarto_navigation::render_html::announcement_to_html`], and stores it
//! under **one of two keys**, by position:
//!
//! - `rendered.navigation.announcement-above` — before the navbar;
//! - `rendered.navigation.announcement-below` — after the navbar and the
//!   secondary nav.
//!
//! Two keys rather than HTML plus a position flag because the template
//! language has no equality test; each placement in
//! `QUARTO_HEADER_PARTIAL` is then a plain `$if$`. Either key also opens
//! the `#quarto-header` gate (template, `quarto_nav_js`, preview) via
//! [`page_has_header`](super::page_has_header).
//!
//! Must run before the transforms that read the header gate
//! (`QuartoNavJsTransform`).
//!
//! ## Skip conditions
//!
//! - not plain HTML (reveal decks have no site header; non-HTML formats
//!   have no template slot);
//! - either rendered key already populated (user override);
//! - `navigation.announcement` absent or without usable content.

use quarto_navigation::render_html::announcement_to_html;
use quarto_navigation::{Announcement, AnnouncementPosition};
use quarto_pandoc_types::config_value::ConfigValue;
use quarto_pandoc_types::pandoc::Pandoc;
use quarto_source_map::{By, SourceInfo};

use crate::Result;
use crate::format::{Format, is_revealjs_target};
use crate::language::LanguageTerms;
use crate::render::RenderContext;
use crate::transform::{AstTransform, TransformPhase};
use crate::transforms::navigation_href::{NavSurface, rewrite_config_text};

/// Rendered-metadata key for each position.
pub(crate) const ANNOUNCEMENT_ABOVE_KEY: &str = "announcement-above";
pub(crate) const ANNOUNCEMENT_BELOW_KEY: &str = "announcement-below";

/// English fallback for the close button's accessible name, used when the
/// language table lacks the q2-only `announcement-close` term (see
/// `resources/language/README.md`, "Quarto 2 additions").
const CLOSE_LABEL_FALLBACK: &str = "Close announcement";

pub struct AnnouncementRenderTransform;

impl AnnouncementRenderTransform {
    pub fn new() -> Self {
        Self
    }
}

impl Default for AnnouncementRenderTransform {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait(?Send)]
impl AstTransform for AnnouncementRenderTransform {
    fn name(&self) -> &str {
        "announcement-render"
    }

    fn phase(&self) -> TransformPhase {
        TransformPhase::Navigation
    }

    async fn transform(&self, ast: &mut Pandoc, ctx: &mut RenderContext) -> Result<()> {
        if !format_has_site_header(ctx.format) {
            return Ok(());
        }

        if [ANNOUNCEMENT_ABOVE_KEY, ANNOUNCEMENT_BELOW_KEY]
            .iter()
            .any(|key| ast.meta.contains_path(&["rendered", "navigation", key]))
        {
            return Ok(());
        }

        let Some(stored) = ast.meta.get_path(&["navigation", "announcement"]) else {
            return Ok(());
        };
        // The generate half already reported any configuration problem;
        // a user-populated value with no content just renders nothing.
        let (Some(mut announcement), _) = Announcement::from_config_value(stored) else {
            return Ok(());
        };

        let mut local_diags = std::mem::take(&mut ctx.diagnostics);
        rewrite_config_text(
            &mut announcement.content,
            ctx.resource_resolver.as_ref(),
            ctx.project_index.as_deref(),
            &NavSurface::Announcement,
            &mut local_diags,
        );
        ctx.diagnostics = local_diags;

        let terms = LanguageTerms::from_meta(&ast.meta);
        let close_label = terms
            .as_ref()
            .and_then(|t| t.get("announcement-close"))
            .unwrap_or(CLOSE_LABEL_FALLBACK);
        let html = announcement_to_html(&announcement, close_label);

        let key = match announcement.position {
            AnnouncementPosition::AboveNavbar => ANNOUNCEMENT_ABOVE_KEY,
            AnnouncementPosition::BelowNavbar => ANNOUNCEMENT_BELOW_KEY,
        };
        ast.meta.insert_path(
            &["rendered", "navigation", key],
            ConfigValue::new_string(&html, SourceInfo::generated(By::programmatic_config())),
        );

        Ok(())
    }
}

/// Plain HTML pages carry `#quarto-header`; reveal decks (whose
/// `Format::is_html` is also true) do not. Same predicate as the draft
/// banner's: checked on `target_format` so the `q2-preview` pseudo-format
/// gets the banner and `q2-slides` does not.
fn format_has_site_header(format: &Format) -> bool {
    format.is_html() && !is_revealjs_target(&format.target_format)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document_profile::DocumentProfile;
    use crate::project::index::ProjectIndex;
    use crate::project::{DocumentInfo, ProjectConfig, ProjectContext};
    use crate::render::BinaryDependencies;
    use crate::resource_resolver::ResourceResolverContext;
    use quarto_error_reporting::DiagnosticMessage;
    use quarto_pandoc_types::ConfigMapEntry;
    use quarto_pandoc_types::attr::{AttrSourceInfo, TargetSourceInfo};
    use quarto_pandoc_types::inline::{Inline, Link, Str};
    use std::path::PathBuf;
    use std::sync::Arc;

    fn make_test_project() -> ProjectContext {
        ProjectContext {
            dir: PathBuf::from("/project"),
            config: ProjectConfig::default(),
            is_single_file: false,
            files: vec![DocumentInfo::from_path("/project/guide/page.qmd")],
            output_dir: PathBuf::from("/project/_site"),
            ..Default::default()
        }
    }

    fn map(entries: Vec<(&str, ConfigValue)>) -> ConfigValue {
        ConfigValue::new_map(
            entries
                .into_iter()
                .map(|(k, v)| ConfigMapEntry {
                    key: k.to_string(),
                    key_source: SourceInfo::for_test(),
                    value: v,
                })
                .collect(),
            SourceInfo::for_test(),
        )
    }

    fn s(x: &str) -> ConfigValue {
        ConfigValue::new_string(x, SourceInfo::for_test())
    }

    fn stored(fields: Vec<(&str, ConfigValue)>) -> ConfigValue {
        let mut meta = map(vec![]);
        meta.insert_path(&["navigation", "announcement"], map(fields));
        meta
    }

    fn link(url: &str, text: &str) -> Inline {
        Inline::Link(Box::new(Link {
            attr: Default::default(),
            content: vec![Inline::Str(Str {
                text: text.to_string(),
                source_info: SourceInfo::for_test(),
            })],
            target: (url.to_string(), String::new()),
            source_info: SourceInfo::for_test(),
            attr_source: AttrSourceInfo::empty(),
            target_source: TargetSourceInfo::empty(),
        }))
    }

    struct Run {
        format: Format,
        index: Option<Arc<ProjectIndex>>,
        resolver: bool,
    }

    impl Run {
        /// Plain HTML, no index, no resolver.
        fn html() -> Self {
            Run {
                format: Format::html(),
                index: None,
                resolver: false,
            }
        }
    }

    async fn run_with(meta: ConfigValue, opts: Run) -> (ConfigValue, Vec<DiagnosticMessage>) {
        let mut ast = Pandoc {
            meta,
            blocks: vec![],
        };
        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/guide/page.qmd");
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &opts.format, &binaries);
        if opts.resolver {
            ctx = ctx.with_resource_resolver(ResourceResolverContext::website(
                "/project/_site",
                "/project/_site/guide/page.html",
                "site_libs",
                "page",
            ));
        }
        if let Some(idx) = opts.index {
            ctx = ctx.with_project_index(idx);
        }
        AnnouncementRenderTransform::new()
            .transform(&mut ast, &mut ctx)
            .await
            .unwrap();
        (ast.meta, ctx.diagnostics)
    }

    async fn run(meta: ConfigValue) -> ConfigValue {
        run_with(meta, Run::html()).await.0
    }

    fn rendered(meta: &ConfigValue, key: &str) -> Option<String> {
        meta.get_path(&["rendered", "navigation", key])
            .and_then(|v| v.as_plain_text())
    }

    #[tokio::test]
    async fn skips_without_navigation_announcement() {
        let meta = run(map(vec![])).await;
        assert!(!meta.contains_path(&["rendered", "navigation"]));
    }

    #[tokio::test]
    async fn above_navbar_is_the_default_key() {
        let meta = run(stored(vec![("content", s("Hello"))])).await;
        let html = rendered(&meta, ANNOUNCEMENT_ABOVE_KEY).expect("above key");
        assert!(html.starts_with("<div id=\"quarto-announcement\""));
        assert!(html.contains(">Hello</div>"));
        assert!(rendered(&meta, ANNOUNCEMENT_BELOW_KEY).is_none());
    }

    #[tokio::test]
    async fn below_navbar_uses_the_below_key() {
        let meta = run(stored(vec![
            ("content", s("Hello")),
            ("position", s("below-navbar")),
        ]))
        .await;
        assert!(rendered(&meta, ANNOUNCEMENT_BELOW_KEY).is_some());
        assert!(rendered(&meta, ANNOUNCEMENT_ABOVE_KEY).is_none());
    }

    #[tokio::test]
    async fn close_label_falls_back_to_english_without_language_table() {
        let meta = run(stored(vec![("content", s("Hello"))])).await;
        let html = rendered(&meta, ANNOUNCEMENT_ABOVE_KEY).unwrap();
        assert!(html.contains("aria-label=\"Close announcement\""));
    }

    #[tokio::test]
    async fn skips_for_revealjs() {
        let format = Format {
            target_format: "revealjs".to_string(),
            ..Format::html()
        };
        let (meta, _) = run_with(
            stored(vec![("content", s("Hello"))]),
            Run {
                format,
                ..Run::html()
            },
        )
        .await;
        assert!(rendered(&meta, ANNOUNCEMENT_ABOVE_KEY).is_none());
    }

    #[tokio::test]
    async fn prerendered_key_is_kept() {
        let mut meta = stored(vec![("content", s("Hello"))]);
        meta.insert_path(
            &["rendered", "navigation", ANNOUNCEMENT_BELOW_KEY],
            s("<div>custom</div>"),
        );
        let meta = run(meta).await;
        assert_eq!(
            rendered(&meta, ANNOUNCEMENT_BELOW_KEY).as_deref(),
            Some("<div>custom</div>")
        );
        assert!(rendered(&meta, ANNOUNCEMENT_ABOVE_KEY).is_none());
    }

    /// Content links are config space (project-root relative) and
    /// resolve to page-relative output hrefs at the page's depth.
    #[tokio::test]
    async fn content_qmd_link_resolves_at_depth() {
        let content =
            ConfigValue::new_inlines(vec![link("news.qmd", "news")], SourceInfo::for_test());
        let index = Arc::new(ProjectIndex::new(vec![DocumentProfile {
            source_path: PathBuf::from("news.qmd"),
            output_href: "news.html".to_string(),
            format_id: "html".to_string(),
            ..DocumentProfile::default()
        }]));
        let (meta, diags) = run_with(
            stored(vec![("content", content)]),
            Run {
                index: Some(index),
                resolver: true,
                ..Run::html()
            },
        )
        .await;
        let html = rendered(&meta, ANNOUNCEMENT_ABOVE_KEY).unwrap();
        assert!(html.contains("href=\"../news.html\""), "got {html}");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    }

    #[tokio::test]
    async fn missing_document_link_warns_q_13_15() {
        let content =
            ConfigValue::new_inlines(vec![link("missing.qmd", "gone")], SourceInfo::for_test());
        let (_, diags) = run_with(
            stored(vec![("content", content)]),
            Run {
                index: Some(Arc::new(ProjectIndex::new(vec![]))),
                resolver: true,
                ..Run::html()
            },
        )
        .await;
        assert_eq!(diags.len(), 1, "got {diags:?}");
        assert_eq!(diags[0].code.as_deref(), Some("Q-13-15"));
    }
}
