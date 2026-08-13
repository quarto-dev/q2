/*
 * footer_render.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! HTML rendering transform for the page footer.
//!
//! Reads the resolved structure from `navigation.footer` (populated by
//! [`FooterGenerateTransform`](super::FooterGenerateTransform) or a
//! user override), rewrites `.qmd` hrefs inside `FooterRegion::Items`
//! regions to output hrefs via the
//! [`ProjectIndex`](crate::project::index::ProjectIndex), emits HTML
//! via [`quarto_navigation::render_html::page_footer_to_html`], and
//! stores the result at `rendered.navigation.footer`.
//!
//! Mirrors [`NavbarRenderTransform`](super::NavbarRenderTransform).
//! Footer items do **not** get active marking (Phase 3 Decision 8).
//!
//! ## Skip conditions
//!
//! - `page-footer: false` (affirmative disable).
//! - `rendered.navigation.footer` already populated (user override).
//! - `navigation.footer` absent.

use quarto_error_reporting::DiagnosticMessage;
use quarto_navigation::{
    FooterRegion, NavigationItem, PageFooter, render_html::page_footer_to_html,
};
use quarto_pandoc_types::config_value::ConfigValue;
use quarto_pandoc_types::pandoc::Pandoc;
use quarto_source_map::{By, SourceInfo};

use crate::Result;
use crate::project::index::ProjectIndex;
use crate::render::RenderContext;
use crate::resource_resolver::ResourceResolverContext;
use crate::transform::{AstTransform, TransformPhase};
use crate::transforms::is_feature_disabled;
use crate::transforms::navigation_href::{NavSurface, resolve_href_for_html};

pub struct FooterRenderTransform;

impl FooterRenderTransform {
    pub fn new() -> Self {
        Self
    }
}

impl Default for FooterRenderTransform {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait(?Send)]
impl AstTransform for FooterRenderTransform {
    fn name(&self) -> &str {
        "footer-render"
    }

    fn phase(&self) -> TransformPhase {
        TransformPhase::Navigation
    }

    async fn transform(&self, ast: &mut Pandoc, ctx: &mut RenderContext) -> Result<()> {
        if is_feature_disabled(&ast.meta, "page-footer") {
            return Ok(());
        }

        if ast
            .meta
            .contains_path(&["rendered", "navigation", "footer"])
        {
            return Ok(());
        }

        let Some(footer_cv) = ast.meta.get_path(&["navigation", "footer"]) else {
            return Ok(());
        };

        let mut footer = PageFooter::from_config_value(footer_cv);

        // Rewrite hrefs in each Items region, and Link/Image targets
        // inside Text regions' parsed markdown
        // (bd-root-relative-paths-design-fc5pvkcv, Case C — the AST
        // body's Phase-6 rewriter never sees footer inlines, which
        // live in metadata, so this transform owns their resolution).
        let mut local_diags = std::mem::take(&mut ctx.diagnostics);
        rewrite_region_hrefs(
            &mut footer.left,
            ctx.resource_resolver.as_ref(),
            ctx.project_index.as_deref(),
            &mut local_diags,
        );
        rewrite_region_hrefs(
            &mut footer.center,
            ctx.resource_resolver.as_ref(),
            ctx.project_index.as_deref(),
            &mut local_diags,
        );
        rewrite_region_hrefs(
            &mut footer.right,
            ctx.resource_resolver.as_ref(),
            ctx.project_index.as_deref(),
            &mut local_diags,
        );
        ctx.diagnostics = local_diags;

        let html = page_footer_to_html(&footer);

        ast.meta.insert_path(
            &["rendered", "navigation", "footer"],
            ConfigValue::new_string(&html, SourceInfo::generated(By::programmatic_config())),
        );

        Ok(())
    }
}

fn rewrite_region_hrefs(
    region: &mut FooterRegion,
    resolver: Option<&ResourceResolverContext>,
    index: Option<&ProjectIndex>,
    diagnostics: &mut Vec<DiagnosticMessage>,
) {
    match region {
        FooterRegion::Items(items) => {
            rewrite_items_hrefs(items, resolver, index, diagnostics);
        }
        FooterRegion::Text(cv) => {
            if let quarto_pandoc_types::config_value::ConfigValueKind::PandocInlines(inlines) =
                &mut cv.value
            {
                crate::transforms::navigation_href::rewrite_config_inlines(
                    inlines,
                    resolver,
                    index,
                    &NavSurface::PageFooter,
                    diagnostics,
                );
            }
        }
        FooterRegion::Empty => {}
    }
}

fn rewrite_items_hrefs(
    items: &mut [NavigationItem],
    resolver: Option<&ResourceResolverContext>,
    index: Option<&ProjectIndex>,
    diagnostics: &mut Vec<DiagnosticMessage>,
) {
    for item in items.iter_mut() {
        if let Some(href) = item.href.as_mut() {
            // bd-qor9a — pass the item's SourceInfo through so any
            // Q-13-3 diagnostic points at the YAML scalar.
            let location = Some(item.href_source.clone());
            *href = resolve_href_for_html(
                href,
                resolver,
                index,
                NavSurface::PageFooter,
                location,
                diagnostics,
            );
        }
        // Footer items rarely nest `menu`, but the type allows it —
        // handle symmetrically with navbar.
        if !item.menu.is_empty() {
            rewrite_items_hrefs(&mut item.menu, resolver, index, diagnostics);
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
    use quarto_navigation::{FooterRegion, NavigationItem, PageFooter};
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

    fn s(x: &str) -> ConfigValue {
        ConfigValue::new_string(x, SourceInfo::for_test())
    }

    fn b(x: bool) -> ConfigValue {
        ConfigValue::new_bool(x, SourceInfo::for_test())
    }

    fn profile(source: &str, output_href: &str, title: &str) -> DocumentProfile {
        DocumentProfile {
            source_path: PathBuf::from(source),
            output_href: output_href.to_string(),
            format_id: "html".to_string(),
            title: Some(title.to_string()),
            ..DocumentProfile::default()
        }
    }

    async fn run(meta: ConfigValue) -> (ConfigValue, Vec<DiagnosticMessage>) {
        run_with(meta, None).await
    }

    async fn run_with(
        meta: ConfigValue,
        index: Option<Arc<ProjectIndex>>,
    ) -> (ConfigValue, Vec<DiagnosticMessage>) {
        let mut ast = Pandoc {
            meta,
            blocks: vec![],
        };
        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);
        if let Some(idx) = index {
            ctx = ctx.with_project_index(idx);
        }
        FooterRenderTransform::new()
            .transform(&mut ast, &mut ctx)
            .await
            .unwrap();
        (ast.meta, ctx.diagnostics)
    }

    /// Like `run_with`, but wires a website resolver pinned at a
    /// depth-1 page (`tools/converter.html`) so page-relative
    /// resolution is observable.
    async fn run_at_depth(
        meta: ConfigValue,
        index: Option<Arc<ProjectIndex>>,
    ) -> (ConfigValue, Vec<DiagnosticMessage>) {
        use crate::resource_resolver::ResourceResolverContext;
        let mut ast = Pandoc {
            meta,
            blocks: vec![],
        };
        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/tools/converter.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let resolver = ResourceResolverContext::website(
            "/project/_site",
            "/project/_site/tools/converter.html",
            "site_libs",
            "converter",
        );
        let mut ctx =
            RenderContext::new(&project, &doc, &format, &binaries).with_resource_resolver(resolver);
        if let Some(idx) = index {
            ctx = ctx.with_project_index(idx);
        }
        FooterRenderTransform::new()
            .transform(&mut ast, &mut ctx)
            .await
            .unwrap();
        (ast.meta, ctx.diagnostics)
    }

    /// A `ConfigValue` holding parsed markdown inlines — the shape
    /// footer text regions actually take in production (both
    /// `_quarto.yml` and document metadata parse markdown strings to
    /// `PandocInlines`).
    fn inlines_cv(inlines: Vec<quarto_pandoc_types::inline::Inline>) -> ConfigValue {
        ConfigValue {
            value: quarto_pandoc_types::config_value::ConfigValueKind::PandocInlines(inlines),
            source_info: SourceInfo::for_test(),
            merge_op: Default::default(),
        }
    }

    fn footer_image(url: &str, alt: &str) -> quarto_pandoc_types::inline::Inline {
        use quarto_pandoc_types::attr::{AttrSourceInfo, TargetSourceInfo};
        use quarto_pandoc_types::inline::{Image, Inline, Str};
        Inline::Image(Image {
            attr: Default::default(),
            content: vec![Inline::Str(Str {
                text: alt.to_string(),
                source_info: SourceInfo::for_test(),
            })],
            target: (url.to_string(), String::new()),
            source_info: SourceInfo::for_test(),
            attr_source: AttrSourceInfo::empty(),
            target_source: TargetSourceInfo::empty(),
        })
    }

    fn footer_link(url: &str, text: &str) -> quarto_pandoc_types::inline::Inline {
        use quarto_pandoc_types::attr::{AttrSourceInfo, TargetSourceInfo};
        use quarto_pandoc_types::inline::{Inline, Link, Str};
        Inline::Link(Link {
            attr: Default::default(),
            content: vec![Inline::Str(Str {
                text: text.to_string(),
                source_info: SourceInfo::for_test(),
            })],
            target: (url.to_string(), String::new()),
            source_info: SourceInfo::for_test(),
            attr_source: AttrSourceInfo::empty(),
            target_source: TargetSourceInfo::empty(),
        })
    }

    // --- Phase 2 behavior preserved -------------------------------

    #[tokio::test]
    async fn skips_when_navigation_footer_missing() {
        let (out, _) = run(ConfigValue::default()).await;
        assert!(!out.contains_path(&["rendered", "navigation", "footer"]));
    }

    #[tokio::test]
    async fn skips_when_page_footer_false() {
        let footer = PageFooter {
            center: FooterRegion::Text(s("Ignored")),
            ..PageFooter::default()
        };
        let mut meta = config_map(vec![("page-footer", b(false))]);
        meta.insert_path(&["navigation", "footer"], footer.to_config_value());
        let (out, _) = run(meta).await;
        assert!(!out.contains_path(&["rendered", "navigation", "footer"]));
    }

    #[tokio::test]
    async fn skips_when_prerendered() {
        let mut meta = ConfigValue::default();
        meta.insert_path(
            &["navigation", "footer"],
            PageFooter::default().to_config_value(),
        );
        meta.insert_path(
            &["rendered", "navigation", "footer"],
            s("<footer>User</footer>"),
        );
        let (out, _) = run(meta).await;
        assert_eq!(
            out.get_path(&["rendered", "navigation", "footer"])
                .unwrap()
                .as_plain_text()
                .as_deref(),
            Some("<footer>User</footer>")
        );
    }

    #[tokio::test]
    async fn renders_footer_html() {
        let footer = PageFooter {
            center: FooterRegion::Text(s("Copyright 2026")),
            ..PageFooter::default()
        };
        let mut meta = ConfigValue::default();
        meta.insert_path(&["navigation", "footer"], footer.to_config_value());
        let (out, _) = run(meta).await;
        let html = out
            .get_path(&["rendered", "navigation", "footer"])
            .unwrap()
            .as_plain_text()
            .unwrap();
        assert!(html.contains("<footer class=\"footer\">"));
        assert!(html.contains("nav-footer-center"));
        assert!(html.contains("Copyright 2026"));
    }

    // --- Phase 3 href rewriting -----------------------------------

    /// Phase 3 test 41 — leaf items in a footer Items region get
    /// their hrefs rewritten to output hrefs.
    #[tokio::test]
    async fn footer_render_rewrites_qmd_hrefs_in_items_region() {
        let footer = PageFooter {
            right: FooterRegion::Items(vec![NavigationItem {
                href: Some("about.qmd".to_string()),
                text: Some(s("About")),
                ..NavigationItem::default()
            }]),
            ..PageFooter::default()
        };
        let mut meta = ConfigValue::default();
        meta.insert_path(&["navigation", "footer"], footer.to_config_value());
        let index = Arc::new(ProjectIndex::new(vec![profile(
            "about.qmd",
            "about.html",
            "About",
        )]));
        let (out, diags) = run_with(meta, Some(index)).await;
        let html = out
            .get_path(&["rendered", "navigation", "footer"])
            .unwrap()
            .as_plain_text()
            .unwrap();
        assert!(html.contains("href=\"about.html\""), "got: {}", html);
        assert!(!html.contains("href=\"about.qmd\""));
        assert!(diags.is_empty());
    }

    /// ~~Phase 3 test 42~~ — DELIBERATELY INVERTED by
    /// bd-root-relative-paths-design-fc5pvkcv. The original test
    /// pinned Text regions as pass-through on the theory that
    /// "body-content link rewriting is Phase 6" — but Phase 6 walks
    /// the AST body and never sees the footer HTML stashed in
    /// metadata, so a `.qmd` link in a footer text region survived
    /// into production output verbatim. Text-region links now resolve
    /// exactly like Items-region hrefs.
    ///
    /// A bare *scalar* Text region (no parsed inlines) still passes
    /// through: there are no Link nodes in a plain string.
    #[tokio::test]
    async fn footer_render_text_region_links_resolve_like_items() {
        let footer = PageFooter {
            center: FooterRegion::Text(inlines_cv(vec![footer_link("docs.qmd", "docs")])),
            ..PageFooter::default()
        };
        let mut meta = ConfigValue::default();
        meta.insert_path(&["navigation", "footer"], footer.to_config_value());
        let index = Arc::new(ProjectIndex::new(vec![profile(
            "docs.qmd",
            "docs.html",
            "Docs",
        )]));
        let (out, diags) = run_with(meta, Some(index)).await;
        let html = out
            .get_path(&["rendered", "navigation", "footer"])
            .unwrap()
            .as_plain_text()
            .unwrap();
        assert!(
            html.contains("href=\"docs.html\""),
            "text-region .qmd link must resolve to its output href; got: {}",
            html
        );
        assert!(
            !html.contains("docs.qmd\""),
            "raw .qmd href must not survive; got: {}",
            html
        );
        assert!(
            diags.is_empty(),
            "text region rewrite should not emit diagnostics"
        );
    }

    /// Case C (bd-root-relative-paths-design-fc5pvkcv): a markdown
    /// image in a footer Text region resolves page-relative — the
    /// site-root form (`/images/x.svg`) and the bare project-root form
    /// (`images/x.svg`) both emit `../images/x.svg` from a depth-1
    /// page. This is the incentive-removal fix: config-declared
    /// footer imagery no longer needs raw HTML.
    #[tokio::test]
    async fn footer_render_text_region_image_resolves_page_relative() {
        let footer = PageFooter {
            left: FooterRegion::Text(inlines_cv(vec![footer_image("/images/x.svg", "L")])),
            center: FooterRegion::Text(inlines_cv(vec![footer_image("images/y.svg", "C")])),
            ..PageFooter::default()
        };
        let mut meta = ConfigValue::default();
        meta.insert_path(&["navigation", "footer"], footer.to_config_value());
        let (out, diags) = run_at_depth(meta, None).await;
        let html = out
            .get_path(&["rendered", "navigation", "footer"])
            .unwrap()
            .as_plain_text()
            .unwrap();
        assert!(
            html.contains("<img src=\"../images/x.svg\" alt=\"L\">"),
            "site-root image must relativize from the depth-1 page; got: {}",
            html
        );
        assert!(
            html.contains("<img src=\"../images/y.svg\" alt=\"C\">"),
            "project-root image must relativize identically; got: {}",
            html
        );
        assert!(diags.is_empty(), "got diagnostics: {:?}", diags);
    }

    /// Text-region links relativize per page too: from a depth-1 page,
    /// a `/index.qmd` (site-root) link resolves to `../index.html`.
    #[tokio::test]
    async fn footer_render_text_region_root_link_resolves_at_depth() {
        let footer = PageFooter {
            right: FooterRegion::Text(inlines_cv(vec![footer_link("/index.qmd", "root")])),
            ..PageFooter::default()
        };
        let mut meta = ConfigValue::default();
        meta.insert_path(&["navigation", "footer"], footer.to_config_value());
        let index = Arc::new(ProjectIndex::new(vec![profile(
            "index.qmd",
            "index.html",
            "Home",
        )]));
        let (out, diags) = run_at_depth(meta, Some(index)).await;
        let html = out
            .get_path(&["rendered", "navigation", "footer"])
            .unwrap()
            .as_plain_text()
            .unwrap();
        assert!(
            html.contains("href=\"../index.html\""),
            "site-root .qmd link must resolve page-relative; got: {}",
            html
        );
        assert!(diags.is_empty(), "got diagnostics: {:?}", diags);
    }

    /// Phase 3 test 43 — unknown .qmd href in a footer item emits a
    /// structured Q-13-3 diagnostic (bd-8d6rk migration).
    #[tokio::test]
    async fn footer_render_emits_diagnostic_for_unknown_qmd() {
        let footer = PageFooter {
            right: FooterRegion::Items(vec![NavigationItem {
                href: Some("missing.qmd".to_string()),
                text: Some(s("Missing")),
                ..NavigationItem::default()
            }]),
            ..PageFooter::default()
        };
        let mut meta = ConfigValue::default();
        meta.insert_path(&["navigation", "footer"], footer.to_config_value());
        let index = Arc::new(ProjectIndex::new(vec![]));
        let (_, diags) = run_with(meta, Some(index)).await;
        assert_eq!(diags.len(), 1);
        let d = &diags[0];
        assert_eq!(d.code.as_deref(), Some("Q-13-3"));
        assert!(
            d.title.starts_with("Page footer"),
            "got title: {:?}",
            d.title
        );
        assert!(
            d.problem
                .as_ref()
                .is_some_and(|p| p.as_str().contains("missing.qmd")),
            "Q-13-3 problem must mention missing.qmd; got {:?}",
            d.problem
        );
    }

    /// Phase 3 test 44 — standalone render (no ProjectIndex).
    /// `.qmd` hrefs survive unchanged, no diagnostic.
    #[tokio::test]
    async fn footer_render_no_index_passes_hrefs_through() {
        let footer = PageFooter {
            right: FooterRegion::Items(vec![NavigationItem {
                href: Some("about.qmd".to_string()),
                text: Some(s("About")),
                ..NavigationItem::default()
            }]),
            ..PageFooter::default()
        };
        let mut meta = ConfigValue::default();
        meta.insert_path(&["navigation", "footer"], footer.to_config_value());
        let (out, diags) = run(meta).await;
        let html = out
            .get_path(&["rendered", "navigation", "footer"])
            .unwrap()
            .as_plain_text()
            .unwrap();
        assert!(html.contains("href=\"about.qmd\""));
        assert!(diags.is_empty());
    }

    /// bd-swpy regression — footer item hrefs relativize to the
    /// current page when a resolver is attached. Same shape as the
    /// navbar / sidebar regression: page is one directory deep,
    /// item targets a root-level page; output must walk up one
    /// level.
    #[tokio::test]
    async fn footer_render_relativizes_hrefs_via_resolver_at_depth_one() {
        use crate::resource_resolver::ResourceResolverContext;

        let footer = PageFooter {
            center: FooterRegion::Items(vec![NavigationItem {
                href: Some("about.qmd".to_string()),
                text: Some(s("About")),
                ..NavigationItem::default()
            }]),
            ..PageFooter::default()
        };
        let mut meta = ConfigValue::default();
        meta.insert_path(&["navigation", "footer"], footer.to_config_value());
        let index = Arc::new(ProjectIndex::new(vec![profile(
            "about.qmd",
            "about.html",
            "About",
        )]));

        let mut ast = Pandoc {
            meta,
            blocks: vec![],
        };
        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/docs/api.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let resolver = ResourceResolverContext::website(
            "/project/_site",
            "/project/_site/docs/api.html",
            "site_libs",
            "api",
        );
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries)
            .with_project_index(index)
            .with_resource_resolver(resolver);
        FooterRenderTransform::new()
            .transform(&mut ast, &mut ctx)
            .await
            .unwrap();
        let html = ast
            .meta
            .get_path(&["rendered", "navigation", "footer"])
            .unwrap()
            .as_plain_text()
            .unwrap();
        assert!(
            html.contains("href=\"../about.html\""),
            "expected ../about.html; got: {}",
            html
        );
    }
}
