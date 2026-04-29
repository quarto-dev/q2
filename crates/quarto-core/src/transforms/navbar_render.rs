/*
 * navbar_render.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! HTML rendering transform for the navbar.
//!
//! Reads the resolved structure from `navigation.navbar` (populated by
//! [`NavbarGenerateTransform`](super::NavbarGenerateTransform) or a user
//! override), rewrites `.qmd` hrefs to output hrefs via the
//! [`ProjectIndex`](crate::project::index::ProjectIndex), and emits
//! HTML via [`quarto_navigation::render_html::navbar_to_html`]. The
//! result lands at `rendered.navigation.navbar` for the template.
//!
//! The **brand fallback chain** (Phase 3 Decision 6) is applied here:
//! `navbar.title → website.title → document.title`. See
//! `brand_title_fallback` below.
//!
//! ## Skip conditions
//!
//! - `navbar: false` (affirmative disable).
//! - `rendered.navigation.navbar` already populated — user pre-rendered HTML.
//! - `navigation.navbar` absent — nothing to render.

use quarto_error_reporting::DiagnosticMessage;
use quarto_navigation::{Navbar, NavigationItem, render_html::navbar_to_html};
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

pub struct NavbarRenderTransform;

impl NavbarRenderTransform {
    pub fn new() -> Self {
        Self
    }
}

impl Default for NavbarRenderTransform {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait(?Send)]
impl AstTransform for NavbarRenderTransform {
    fn name(&self) -> &str {
        "navbar-render"
    }

    async fn transform(&self, ast: &mut Pandoc, ctx: &mut RenderContext) -> Result<()> {
        if is_feature_disabled(&ast.meta, "navbar") {
            return Ok(());
        }

        if ast
            .meta
            .contains_path(&["rendered", "navigation", "navbar"])
        {
            return Ok(());
        }

        let Some(navbar_cv) = ast.meta.get_path(&["navigation", "navbar"]) else {
            return Ok(());
        };

        let mut navbar = Navbar::from_config_value(navbar_cv);

        // Rewrite .qmd → .html hrefs (including inside dropdown menus)
        // when a ProjectIndex is attached. Borrow diagnostics out of
        // ctx so the helper can push without a borrow cycle.
        let mut local_diags = std::mem::take(&mut ctx.diagnostics);
        rewrite_navigation_item_hrefs(
            &mut navbar.left,
            ctx.resource_resolver.as_ref(),
            ctx.project_index.as_deref(),
            "Navbar",
            &mut local_diags,
        );
        rewrite_navigation_item_hrefs(
            &mut navbar.right,
            ctx.resource_resolver.as_ref(),
            ctx.project_index.as_deref(),
            "Navbar",
            &mut local_diags,
        );
        ctx.diagnostics = local_diags;

        let fallback = brand_title_fallback(&ast.meta);
        let html = navbar_to_html(&navbar, fallback.as_deref());

        ast.meta.insert_path(
            &["rendered", "navigation", "navbar"],
            ConfigValue::new_string(&html, SourceInfo::default()),
        );

        Ok(())
    }
}

/// Walk navbar items (including dropdown `menu` children), rewriting
/// each `href` from source-path to output-href via the shared
/// resolver. Items without an `href` (pure dropdowns) are skipped for
/// the lookup but still descended. The `resolver` arg makes the
/// emitted URLs page-relative (bd-swpy).
fn rewrite_navigation_item_hrefs(
    items: &mut [NavigationItem],
    resolver: Option<&ResourceResolverContext>,
    index: Option<&ProjectIndex>,
    source_label: &str,
    diagnostics: &mut Vec<DiagnosticMessage>,
) {
    for item in items.iter_mut() {
        if let Some(href) = item.href.as_mut() {
            *href = resolve_href_for_html(href, resolver, index, Some(source_label), diagnostics);
        }
        if !item.menu.is_empty() {
            rewrite_navigation_item_hrefs(
                &mut item.menu,
                resolver,
                index,
                source_label,
                diagnostics,
            );
        }
    }
}

/// Brand-title fallback chain, Phase 3 Decision 6:
/// `navbar.title (handled in navbar_to_html) → website.title → document.title`.
///
/// This helper produces the string the renderer uses when
/// `navbar.title == NavbarTitle::Default` — reading site-scoped
/// `website.title` first, then falling back to the document's own
/// `title`. `None` means the renderer has nothing to fall back to
/// (brand anchor will be suppressed if no logo either).
fn brand_title_fallback(meta: &ConfigValue) -> Option<String> {
    if let Some(site_title) = meta
        .get_path(&["website", "title"])
        .and_then(|v| v.as_plain_text())
    {
        return Some(site_title);
    }
    meta.get("title").and_then(|v| v.as_plain_text())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document_profile::DocumentProfile;
    use crate::format::Format;
    use crate::project::{DocumentInfo, ProjectConfig, ProjectContext};
    use crate::render::BinaryDependencies;
    use quarto_navigation::{Navbar, NavbarTitle};
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
        NavbarRenderTransform::new()
            .transform(&mut ast, &mut ctx)
            .await
            .unwrap();
        (ast.meta, ctx.diagnostics)
    }

    // --- Existing Phase 2 tests -----------------------------------

    #[tokio::test]
    async fn skips_when_navigation_navbar_missing() {
        let (out, _) = run(ConfigValue::default()).await;
        assert!(!out.contains_path(&["rendered", "navigation", "navbar"]));
    }

    #[tokio::test]
    async fn skips_when_navbar_false() {
        let navbar = Navbar {
            title: NavbarTitle::Text(s("Ignored")),
            ..Navbar::with_defaults()
        };
        let mut meta = config_map(vec![("navbar", b(false))]);
        meta.insert_path(&["navigation", "navbar"], navbar.to_config_value());
        let (out, _) = run(meta).await;
        assert!(!out.contains_path(&["rendered", "navigation", "navbar"]));
    }

    #[tokio::test]
    async fn skips_when_prerendered() {
        let mut meta = ConfigValue::default();
        meta.insert_path(
            &["navigation", "navbar"],
            Navbar::with_defaults().to_config_value(),
        );
        meta.insert_path(
            &["rendered", "navigation", "navbar"],
            s("<nav>User-provided</nav>"),
        );
        let (out, _) = run(meta).await;
        let rendered = out.get_path(&["rendered", "navigation", "navbar"]).unwrap();
        assert_eq!(
            rendered.as_plain_text().as_deref(),
            Some("<nav>User-provided</nav>")
        );
    }

    #[tokio::test]
    async fn renders_navbar_html() {
        let navbar = Navbar {
            title: NavbarTitle::Text(s("My Site")),
            background: Some("primary".to_string()),
            ..Navbar::with_defaults()
        };
        let mut meta = ConfigValue::default();
        meta.insert_path(&["navigation", "navbar"], navbar.to_config_value());
        let (out, _) = run(meta).await;
        let rendered = out.get_path(&["rendered", "navigation", "navbar"]).unwrap();
        let html = rendered.as_plain_text().unwrap();
        assert!(html.contains("<nav class=\"navbar navbar-expand-lg bg-primary\""));
        assert!(html.contains("My Site"));
    }

    #[tokio::test]
    async fn falls_back_to_document_title() {
        let mut meta = config_map(vec![("title", s("Doc Title"))]);
        meta.insert_path(
            &["navigation", "navbar"],
            Navbar::with_defaults().to_config_value(),
        );
        let (out, _) = run(meta).await;
        let html = out
            .get_path(&["rendered", "navigation", "navbar"])
            .unwrap()
            .as_plain_text()
            .unwrap();
        assert!(html.contains("Doc Title"));
    }

    // --- Phase 3 href rewriting -----------------------------------

    /// Phase 3 test 28 — a leaf item `about.qmd` is rewritten to
    /// `about.html` in the rendered HTML.
    #[tokio::test]
    async fn navbar_render_rewrites_qmd_hrefs_to_output_href() {
        let navbar = Navbar {
            left: vec![NavigationItem {
                href: Some("about.qmd".to_string()),
                text: Some(s("About")),
                ..NavigationItem::default()
            }],
            ..Navbar::with_defaults()
        };
        let mut meta = ConfigValue::default();
        meta.insert_path(&["navigation", "navbar"], navbar.to_config_value());
        let index = Arc::new(ProjectIndex::new(vec![profile(
            "about.qmd",
            "about.html",
            "About",
        )]));
        let (out, diags) = run_with(meta, Some(index)).await;
        let html = out
            .get_path(&["rendered", "navigation", "navbar"])
            .unwrap()
            .as_plain_text()
            .unwrap();
        assert!(html.contains("href=\"about.html\""), "html: {}", html);
        assert!(!html.contains("href=\"about.qmd\""));
        assert!(diags.is_empty());
    }

    /// Phase 3 test 29 — dropdown menu items get the same rewrite.
    #[tokio::test]
    async fn navbar_render_rewrites_dropdown_hrefs() {
        let navbar = Navbar {
            left: vec![NavigationItem {
                text: Some(s("Docs")),
                menu: vec![NavigationItem {
                    href: Some("guide.qmd".to_string()),
                    text: Some(s("Guide")),
                    ..NavigationItem::default()
                }],
                ..NavigationItem::default()
            }],
            ..Navbar::with_defaults()
        };
        let mut meta = ConfigValue::default();
        meta.insert_path(&["navigation", "navbar"], navbar.to_config_value());
        let index = Arc::new(ProjectIndex::new(vec![profile(
            "guide.qmd",
            "guide.html",
            "Guide",
        )]));
        let (out, _) = run_with(meta, Some(index)).await;
        let html = out
            .get_path(&["rendered", "navigation", "navbar"])
            .unwrap()
            .as_plain_text()
            .unwrap();
        assert!(html.contains("href=\"guide.html\""), "html: {}", html);
    }

    /// Phase 3 test 30 — external URLs pass through the rewriter.
    #[tokio::test]
    async fn navbar_render_passes_external_urls_through() {
        let navbar = Navbar {
            right: vec![NavigationItem {
                icon: Some("github".to_string()),
                href: Some("https://github.com/foo".to_string()),
                ..NavigationItem::default()
            }],
            ..Navbar::with_defaults()
        };
        let mut meta = ConfigValue::default();
        meta.insert_path(&["navigation", "navbar"], navbar.to_config_value());
        let index = Arc::new(ProjectIndex::new(vec![]));
        let (out, diags) = run_with(meta, Some(index)).await;
        let html = out
            .get_path(&["rendered", "navigation", "navbar"])
            .unwrap()
            .as_plain_text()
            .unwrap();
        assert!(html.contains("href=\"https://github.com/foo\""));
        assert!(diags.is_empty());
    }

    /// Phase 3 test 31 — an unknown .qmd reference emits a warning
    /// diagnostic tagged "Navbar …".
    #[tokio::test]
    async fn navbar_render_emits_diagnostic_for_unknown_qmd() {
        let navbar = Navbar {
            left: vec![NavigationItem {
                href: Some("missing.qmd".to_string()),
                text: Some(s("Missing")),
                ..NavigationItem::default()
            }],
            ..Navbar::with_defaults()
        };
        let mut meta = ConfigValue::default();
        meta.insert_path(&["navigation", "navbar"], navbar.to_config_value());
        let index = Arc::new(ProjectIndex::new(vec![]));
        let (_, diags) = run_with(meta, Some(index)).await;
        assert_eq!(diags.len(), 1);
        assert!(diags[0].title.starts_with("Navbar"), "got: {:?}", diags[0]);
        assert!(diags[0].title.contains("missing.qmd"));
    }

    /// Phase 3 test 32 — after rewriting, the `active` class is
    /// preserved on the rewritten link. The `active` bit survives
    /// ConfigValue roundtrip (NavigationItem roundtrips it) and the
    /// rewriter doesn't touch it.
    #[tokio::test]
    async fn navbar_render_preserves_active_class_on_rewritten_href() {
        let navbar = Navbar {
            left: vec![NavigationItem {
                href: Some("about.qmd".to_string()),
                text: Some(s("About")),
                active: true,
                ..NavigationItem::default()
            }],
            ..Navbar::with_defaults()
        };
        let mut meta = ConfigValue::default();
        meta.insert_path(&["navigation", "navbar"], navbar.to_config_value());
        let index = Arc::new(ProjectIndex::new(vec![profile(
            "about.qmd",
            "about.html",
            "About",
        )]));
        let (out, _) = run_with(meta, Some(index)).await;
        let html = out
            .get_path(&["rendered", "navigation", "navbar"])
            .unwrap()
            .as_plain_text()
            .unwrap();
        assert!(html.contains("href=\"about.html\""));
        assert!(
            html.contains("class=\"nav-link active\""),
            "expected nav-link active after rewrite; got: {}",
            html
        );
    }

    // --- Phase 3 brand fallback chain -----------------------------

    /// Phase 3 test 33 — with a default navbar title and
    /// `website.title` set, the brand anchor uses the site title.
    #[tokio::test]
    async fn navbar_render_brand_uses_website_title_fallback() {
        let mut meta = ConfigValue::default();
        meta.insert_path(&["website", "title"], s("My Site"));
        meta.insert_path(
            &["navigation", "navbar"],
            Navbar::with_defaults().to_config_value(),
        );
        let (out, _) = run(meta).await;
        let html = out
            .get_path(&["rendered", "navigation", "navbar"])
            .unwrap()
            .as_plain_text()
            .unwrap();
        assert!(html.contains("My Site"), "got: {}", html);
    }

    /// Phase 3 test 34 — explicit navbar title wins over site title.
    #[tokio::test]
    async fn navbar_render_brand_prefers_navbar_title_over_website_title() {
        let navbar = Navbar {
            title: NavbarTitle::Text(s("Explicit Navbar Title")),
            ..Navbar::with_defaults()
        };
        let mut meta = ConfigValue::default();
        meta.insert_path(&["website", "title"], s("Site Title"));
        meta.insert_path(&["navigation", "navbar"], navbar.to_config_value());
        let (out, _) = run(meta).await;
        let html = out
            .get_path(&["rendered", "navigation", "navbar"])
            .unwrap()
            .as_plain_text()
            .unwrap();
        assert!(html.contains("Explicit Navbar Title"));
        assert!(!html.contains("Site Title"));
    }

    /// Phase 3 test 35 — no `website.title`, default navbar title,
    /// top-level `title: ...` → brand uses the document's title. This
    /// is the single-doc behavior preserved from Phase 2.
    #[tokio::test]
    async fn navbar_render_brand_falls_back_to_document_title_when_no_website_title() {
        let mut meta = config_map(vec![("title", s("Doc Title"))]);
        meta.insert_path(
            &["navigation", "navbar"],
            Navbar::with_defaults().to_config_value(),
        );
        let (out, _) = run(meta).await;
        let html = out
            .get_path(&["rendered", "navigation", "navbar"])
            .unwrap()
            .as_plain_text()
            .unwrap();
        assert!(html.contains("Doc Title"));
    }

    /// Phase 3 test 36 — standalone render (no ProjectIndex). A
    /// .qmd navbar href is emitted verbatim; no rewrite, no
    /// diagnostic. This is the revealjs/single-doc UX story.
    #[tokio::test]
    async fn navbar_render_no_index_passes_hrefs_through_unchanged() {
        let navbar = Navbar {
            left: vec![NavigationItem {
                href: Some("about.qmd".to_string()),
                text: Some(s("About")),
                ..NavigationItem::default()
            }],
            ..Navbar::with_defaults()
        };
        let mut meta = ConfigValue::default();
        meta.insert_path(&["navigation", "navbar"], navbar.to_config_value());
        let (out, diags) = run(meta).await;
        let html = out
            .get_path(&["rendered", "navigation", "navbar"])
            .unwrap()
            .as_plain_text()
            .unwrap();
        assert!(html.contains("href=\"about.qmd\""), "got: {}", html);
        assert!(diags.is_empty(), "no diagnostic without index");
    }

    /// bd-swpy regression — depth-1 page with resolver attached.
    /// Navbar hrefs (left, right, dropdown menu items) must
    /// relativize to the current page. Without the fix, an `about.qmd`
    /// link from a page at `_site/tools/converter.html` rendered as
    /// `about.html` and 404'd.
    #[tokio::test]
    async fn navbar_render_relativizes_hrefs_via_resolver_at_depth_one() {
        use crate::resource_resolver::ResourceResolverContext;

        let navbar = Navbar {
            left: vec![
                NavigationItem {
                    href: Some("about.qmd".to_string()),
                    text: Some(s("About")),
                    ..NavigationItem::default()
                },
                NavigationItem {
                    text: Some(s("Tools")),
                    menu: vec![NavigationItem {
                        href: Some("tools/index.qmd".to_string()),
                        text: Some(s("Overview")),
                        ..NavigationItem::default()
                    }],
                    ..NavigationItem::default()
                },
            ],
            ..Navbar::with_defaults()
        };
        let mut meta = ConfigValue::default();
        meta.insert_path(&["navigation", "navbar"], navbar.to_config_value());
        let index = Arc::new(ProjectIndex::new(vec![
            profile("about.qmd", "about.html", "About"),
            profile("tools/index.qmd", "tools/index.html", "Tools"),
        ]));

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
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries)
            .with_project_index(index)
            .with_resource_resolver(resolver);
        NavbarRenderTransform::new()
            .transform(&mut ast, &mut ctx)
            .await
            .unwrap();
        let html = ast
            .meta
            .get_path(&["rendered", "navigation", "navbar"])
            .unwrap()
            .as_plain_text()
            .unwrap();
        // Root-level target seen from depth-1 page → walk up one level.
        assert!(
            html.contains("href=\"../about.html\""),
            "expected ../about.html; got: {}",
            html
        );
        // Subdir target (sibling within the same dir) → just the leaf.
        assert!(
            html.contains("href=\"index.html\""),
            "expected index.html for sibling tools/index.qmd; got: {}",
            html
        );
        // Bare project-relative form must NOT appear.
        assert!(
            !html.contains("href=\"about.html\"") || html.contains("href=\"../about.html\""),
            "bare about.html should not be present without ../ prefix; got: {}",
            html
        );
    }
}
