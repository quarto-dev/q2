/*
 * revealjs/footer_logo.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Reveal deck-level footer + logo: config alias + format-specific render.
 */

//! Reveal deck-level footer + logo (Stage D3).
//!
//! Two transforms implement the footer/logo, splitting **format-agnostic
//! generation** from **format-specific rendering** (the same split the
//! `format: html` chrome uses — navbar/sidebar/TOC/footer):
//!
//! 1. [`RevealFooterAliasTransform`] — a reveal-scoped config alias. Quarto 1
//!    reveal decks configure the footer with `footer:`; the format-agnostic
//!    [`FooterGenerateTransform`](crate::transforms::FooterGenerateTransform)
//!    reads `page-footer:`. This transform copies `footer:` → `page-footer:`
//!    (when `page-footer:` is absent) *before* generate, so a bare Q1 `footer:`
//!    string flows through the shared generate (string → `center` region) with
//!    no reveal-specific knowledge leaking into the generate step. Runs in the
//!    reveal branch *before* `FooterGenerateTransform`.
//!
//! 2. [`RevealFooterLogoTransform`] — the reveal-specific *render*. Reads the
//!    structured `navigation.footer` (produced by the shared generate) and emits
//!    reveal markup into `rendered.reveal.footer`; reads `logo:` (which has no
//!    format-agnostic generate — it is reveal-specific) and emits
//!    `rendered.reveal.logo`. Each render is skipped when its slot is already
//!    populated, so a user filter (or config) can pre-populate the slot to
//!    override. The reveal scaffold ([`super::assemble::render_revealjs_document`])
//!    reads these slots and places the markup as **direct children of `.reveal`,
//!    outside `.slides`** — where `position: fixed` survives reveal's per-slide
//!    CSS transforms (it would not if nested inside `.slides`).
//!
//! Why a transform + meta-slot rather than reading `footer:`/`logo:` in the
//! scaffold directly: it puts footer/logo on the same filter-manipulation
//! surface as the other chrome elements. The pipeline runs user filters around
//! the AST transforms (`UserFiltersStage::pre` → `AstTransformsStage` →
//! `…::post`), so a `pre` filter editing `footer`/`page-footer` flows through
//! generate + render, and a filter can pre-populate `rendered.reveal.*` to
//! override the rendered HTML outright.
//!
//! Scope (Stage D3): the `center` footer region only (Q1's reveal footer is a
//! single centered element; left/right are carried in the structured entry for
//! a future enhancement). `.qmd` hrefs inside a Text-region footer pass through
//! unrewritten, matching html's `FooterRenderTransform` (which defers
//! Text-region rewriting). WASM-safe (pure AST/metadata manipulation).
//!
//! `logo:` is a config-authored path (bd-7328beha). It resolves through
//! [`crate::config_paths`] — relative to the file that declared it, a leading
//! `/` meaning the project root — and then leaves through the resource-resolver
//! exit like any other static asset: the `<img src>` is page-relative
//! ([`ResourceResolverContext::page_url_for`](crate::resource_resolver::ResourceResolverContext::page_url_for)),
//! and a copy intent puts the file in the output tree, since no slide body
//! references it. A project-level `logo:` therefore serves a deck at any depth.

use std::path::Path;

use quarto_pandoc_types::pandoc::Pandoc;
use quarto_pandoc_types::{ConfigValue, ConfigValueKind};
use quarto_source_map::{By, SourceInfo};

use crate::Result;
use crate::config_paths::{PathCtx, ResolvedConfigPath, pivot_form, resolve_config_path};
use crate::render::{RenderContext, ResourceCopyIntent};
use crate::transform::{AstTransform, TransformPhase};

// --- config alias: footer: → page-footer: -----------------------------------

/// Reveal-scoped alias mapping `footer:` → `page-footer:` so Q1 decks flow
/// through the format-agnostic footer generate. Must run before
/// [`FooterGenerateTransform`](crate::transforms::FooterGenerateTransform).
pub struct RevealFooterAliasTransform;

impl RevealFooterAliasTransform {
    pub fn new() -> Self {
        Self
    }
}

impl Default for RevealFooterAliasTransform {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait(?Send)]
impl AstTransform for RevealFooterAliasTransform {
    fn name(&self) -> &str {
        "reveal-footer-alias"
    }

    fn phase(&self) -> TransformPhase {
        TransformPhase::Normalization
    }

    async fn transform(&self, ast: &mut Pandoc, _ctx: &mut RenderContext) -> Result<()> {
        // `page-footer:` wins if the user set both (it's the canonical key).
        if ast.meta.contains_path(&["page-footer"]) {
            return Ok(());
        }
        let Some(footer) = ast.meta.get("footer").cloned() else {
            return Ok(());
        };
        ast.meta.insert_path(&["page-footer"], footer);
        Ok(())
    }
}

// --- reveal-specific render: navigation.footer + logo → rendered.reveal.* ----

/// Renders the reveal deck-level footer (from the format-agnostic
/// `navigation.footer`) and logo (from `logo:`) into `rendered.reveal.footer` /
/// `rendered.reveal.logo`. Skips a slot that is already populated (override).
pub struct RevealFooterLogoTransform;

impl RevealFooterLogoTransform {
    pub fn new() -> Self {
        Self
    }
}

impl Default for RevealFooterLogoTransform {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait(?Send)]
impl AstTransform for RevealFooterLogoTransform {
    fn name(&self) -> &str {
        "reveal-footer-logo"
    }

    fn phase(&self) -> TransformPhase {
        TransformPhase::Navigation
    }

    async fn transform(&self, ast: &mut Pandoc, ctx: &mut RenderContext) -> Result<()> {
        if !ast.meta.contains_path(&["rendered", "reveal", "footer"])
            && let Some(html) = footer_slot_html(&ast.meta)
        {
            ast.meta.insert_path(
                &["rendered", "reveal", "footer"],
                ConfigValue::new_string(&html, SourceInfo::generated(By::revealjs())),
            );
        }

        if !ast.meta.contains_path(&["rendered", "reveal", "logo"])
            && let Some(logo) = resolve_logo(&ast.meta, ctx)
        {
            if let Some(copy) = logo.copy {
                ctx.resource_copies.push(copy);
            }
            ast.meta.insert_path(
                &["rendered", "reveal", "logo"],
                ConfigValue::new_string(
                    logo_slot_html(&logo.src),
                    SourceInfo::generated(By::revealjs()),
                ),
            );
        }

        Ok(())
    }
}

/// A config-authored asset path (a `logo:`) after resolution: the `src`
/// to emit and, for a file inside the project, the copy that puts it in
/// the output tree.
pub(crate) struct ResolvedLogo {
    pub(crate) src: String,
    pub(crate) copy: Option<ResourceCopyIntent>,
}

/// Resolve `logo:` for emission, or `None` when unset/blank.
///
/// - An external URL is emitted verbatim.
/// - A file inside the project is emitted page-relative through the
///   resource resolver (project-relative when the render has none —
///   out-of-band drivers) and registered for copying — except in
///   VFS-root mode, where the hub serves it straight from the VFS
///   (the listing-image rule, bd-yqlbfrln).
/// - A file outside the project cannot be served from the site; it is
///   emitted as written, today's behaviour.
fn resolve_logo(meta: &ConfigValue, ctx: &RenderContext) -> Option<ResolvedLogo> {
    resolve_config_asset(meta.get("logo")?, ctx)
}

/// [`resolve_logo`] for any config-authored asset path `value` (its
/// `SourceInfo` says which file declared it); shared with the dashboard
/// navbar logo.
pub(crate) fn resolve_config_asset(
    value: &ConfigValue,
    ctx: &RenderContext,
) -> Option<ResolvedLogo> {
    let raw = value.as_plain_text()?;
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    let path_ctx = PathCtx::for_document(ctx.source_context, &ctx.project.dir, &ctx.document.input);
    let abs = match resolve_config_path(value, &path_ctx)? {
        ResolvedConfigPath::External(url) => {
            return Some(ResolvedLogo {
                src: url,
                copy: None,
            });
        }
        ResolvedConfigPath::File(abs) => abs,
    };
    let rel = pivot_form(&abs, &ctx.project.dir);
    if quarto_util::is_rooted(Path::new(&rel)) || Path::new(&rel).is_absolute() {
        return Some(ResolvedLogo {
            src: raw.to_string(),
            copy: None,
        });
    }
    let resolver = ctx.resource_resolver.as_ref();
    let src = resolver.map_or_else(|| rel.clone(), |r| r.page_url_for(&rel));
    let copy = (!resolver.is_some_and(|r| r.is_vfs_root_mode())).then(|| ResourceCopyIntent {
        src: abs,
        dest: ctx.project.output_dir.join(&rel),
        origin: value.source_info.clone(),
    });
    Some(ResolvedLogo { src, copy })
}

/// Build the reveal footer markup from `navigation.footer`'s `center` region,
/// or `None` when there is no center content. The `.footer-default` class
/// matches Quarto 1 (and our SCSS in `quarto-revealjs.scss`).
fn footer_slot_html(meta: &ConfigValue) -> Option<String> {
    let center = meta
        .get_path(&["navigation", "footer"])
        .and_then(|f| f.get("center"))?;
    let inner = config_field_to_html(center);
    let inner = inner.trim();
    if inner.is_empty() {
        return None;
    }
    Some(format!(
        r#"<div class="footer footer-default">{inner}</div>"#
    ))
}

/// Build the reveal logo `<img>` for an already-resolved `src`.
fn logo_slot_html(src: &str) -> String {
    format!(r#"<img class="slide-logo" src="{}">"#, attr_escape(src))
}

/// Render a metadata field's inline/block Pandoc content to HTML (preserving
/// links/emphasis); a bare scalar is HTML-escaped. Mirrors
/// `template::titleblock_field_to_html` and the navigation footer's Text-region
/// handling.
fn config_field_to_html(value: &ConfigValue) -> String {
    match &value.value {
        ConfigValueKind::PandocInlines(inlines) => {
            let mut out: Vec<u8> = Vec::new();
            if pampa::writers::html::write_inlines_to(inlines, &mut out).is_ok() {
                return String::from_utf8_lossy(&out).into_owned();
            }
            String::new()
        }
        ConfigValueKind::PandocBlocks(blocks) => {
            let mut out: Vec<u8> = Vec::new();
            if pampa::writers::html::write_blocks_to(blocks, &mut out).is_ok() {
                return String::from_utf8_lossy(&out).into_owned();
            }
            String::new()
        }
        _ => escape_html(&value.as_plain_text().unwrap_or_default()),
    }
}

/// Minimal HTML-text escape (text content).
fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Escape for a double-quoted HTML attribute value.
fn attr_escape(s: &str) -> String {
    escape_html(s).replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use hashlink::LinkedHashMap;
    use quarto_pandoc_types::ConfigMapEntry;
    use quarto_pandoc_types::inline::{Inline, Link, Str};

    fn si() -> SourceInfo {
        SourceInfo::generated(By::revealjs())
    }

    fn map(entries: Vec<(&str, ConfigValue)>) -> ConfigValue {
        ConfigValue::new_map(
            entries
                .into_iter()
                .map(|(k, v)| ConfigMapEntry {
                    key: k.to_string(),
                    key_source: si(),
                    value: v,
                })
                .collect(),
            si(),
        )
    }

    fn s(v: &str) -> ConfigValue {
        ConfigValue::new_string(v, si())
    }

    fn inlines(content: Vec<Inline>) -> ConfigValue {
        ConfigValue {
            value: ConfigValueKind::PandocInlines(content),
            source_info: si(),
            merge_op: Default::default(),
        }
    }

    fn str_inline(text: &str) -> Inline {
        Inline::Str(Str {
            text: text.to_string(),
            source_info: si(),
        })
    }

    /// A footer whose `center` is the given ConfigValue, shaped like the output
    /// of `FooterGenerateTransform` (which stores `navigation.footer.center`).
    fn meta_with_footer_center(center: ConfigValue) -> ConfigValue {
        map(vec![(
            "navigation",
            map(vec![("footer", map(vec![("center", center)]))]),
        )])
    }

    #[test]
    fn footer_slot_from_center_string() {
        let m = meta_with_footer_center(s("© 2026 Me"));
        let html = footer_slot_html(&m).expect("footer slot");
        assert_eq!(
            html,
            r#"<div class="footer footer-default">© 2026 Me</div>"#
        );
    }

    #[test]
    fn footer_slot_renders_inline_links() {
        let link = Inline::Link(Box::new(Link {
            attr: (String::new(), vec![], LinkedHashMap::new()),
            content: vec![str_inline("Quarto")],
            target: ("https://quarto.org".to_string(), String::new()),
            source_info: si(),
            attr_source: quarto_pandoc_types::attr::AttrSourceInfo::empty(),
            target_source: quarto_pandoc_types::attr::TargetSourceInfo::empty(),
        }));
        let m = meta_with_footer_center(inlines(vec![str_inline("© "), link]));
        let html = footer_slot_html(&m).expect("footer slot");
        assert!(
            html.contains(r#"<a href="https://quarto.org">Quarto</a>"#),
            "footer link should render as an anchor; got: {html}"
        );
    }

    #[test]
    fn footer_slot_escapes_scalar_text() {
        let m = meta_with_footer_center(s("a < b & c"));
        let html = footer_slot_html(&m).unwrap();
        assert!(html.contains("a &lt; b &amp; c"), "got: {html}");
    }

    #[test]
    fn footer_slot_none_when_no_footer() {
        assert!(footer_slot_html(&map(vec![("title", s("T"))])).is_none());
    }

    #[test]
    fn footer_slot_none_when_center_empty() {
        let m = meta_with_footer_center(s("   "));
        assert!(footer_slot_html(&m).is_none(), "blank center → no footer");
    }

    #[test]
    fn logo_slot_builds_img() {
        assert_eq!(
            logo_slot_html("logo.png"),
            r#"<img class="slide-logo" src="logo.png">"#
        );
    }

    #[test]
    fn logo_slot_escapes_path_attr() {
        assert_eq!(
            logo_slot_html(r#"a"b.png"#),
            r#"<img class="slide-logo" src="a&quot;b.png">"#
        );
    }

    /// The borrowed pieces a `RenderContext` needs: project at
    /// `/project` with output tree `_site`, deck at `talks/deck.qmd`.
    struct LogoFixture {
        project: crate::project::ProjectContext,
        document: crate::project::DocumentInfo,
        format: crate::format::Format,
        binaries: crate::render::BinaryDependencies,
    }

    fn logo_fixture() -> LogoFixture {
        LogoFixture {
            project: crate::project::ProjectContext {
                dir: std::path::PathBuf::from("/project"),
                output_dir: std::path::PathBuf::from("/project/_site"),
                is_single_file: false,
                ..Default::default()
            },
            document: crate::project::DocumentInfo::from_path("/project/talks/deck.qmd"),
            format: crate::format::Format::from_format_string("revealjs").unwrap(),
            binaries: crate::render::BinaryDependencies::new(),
        }
    }

    /// A render context for `resolve_logo` with a website resolver for
    /// the deck's page.
    fn logo_ctx(f: &LogoFixture) -> RenderContext<'_> {
        let resolver = crate::resource_resolver::ResourceResolverContext::website(
            "/project/_site",
            "/project/_site/talks/deck.html",
            "site_libs",
            "deck",
        );
        RenderContext::new(&f.project, &f.document, &f.format, &f.binaries)
            .with_resource_resolver(resolver)
    }

    #[test]
    fn logo_without_provenance_resolves_against_the_deck_dir() {
        let f = logo_fixture();
        let ctx = logo_ctx(&f);
        let m = map(vec![("logo", s("img/logo.png"))]);
        let logo = resolve_logo(&m, &ctx).unwrap();
        assert_eq!(logo.src, "img/logo.png");
        let copy = logo.copy.expect("in-project logo is copied");
        assert_eq!(
            copy.src,
            std::path::PathBuf::from("/project/talks/img/logo.png")
        );
        assert_eq!(
            copy.dest,
            std::path::PathBuf::from("/project/_site/talks/img/logo.png")
        );
    }

    #[test]
    fn rooted_logo_anchors_at_the_project_root_and_relativizes() {
        let f = logo_fixture();
        let ctx = logo_ctx(&f);
        let m = map(vec![("logo", s("/brand/logo.png"))]);
        let logo = resolve_logo(&m, &ctx).unwrap();
        assert_eq!(logo.src, "../brand/logo.png");
        let copy = logo.copy.unwrap();
        assert_eq!(
            copy.src,
            std::path::PathBuf::from("/project/brand/logo.png")
        );
        assert_eq!(
            copy.dest,
            std::path::PathBuf::from("/project/_site/brand/logo.png")
        );
    }

    #[test]
    fn external_logo_passes_through_uncopied() {
        let f = logo_fixture();
        let ctx = logo_ctx(&f);
        let m = map(vec![("logo", s("https://example.com/logo.png"))]);
        let logo = resolve_logo(&m, &ctx).unwrap();
        assert_eq!(logo.src, "https://example.com/logo.png");
        assert!(logo.copy.is_none());
    }

    #[test]
    fn blank_logo_is_unset() {
        let f = logo_fixture();
        let ctx = logo_ctx(&f);
        assert!(resolve_logo(&map(vec![("logo", s("  "))]), &ctx).is_none());
        assert!(resolve_logo(&map(vec![("title", s("T"))]), &ctx).is_none());
    }

    // --- transform-level: slot writing + override hook + alias ---------------

    async fn run<T: AstTransform>(t: T, meta: ConfigValue) -> ConfigValue {
        use crate::format::Format;
        use crate::project::{DocumentInfo, ProjectConfig, ProjectContext};
        use crate::render::BinaryDependencies;
        use std::path::PathBuf;

        let mut ast = Pandoc {
            meta,
            blocks: vec![],
        };
        let project = ProjectContext {
            dir: PathBuf::from("/project"),
            config: ProjectConfig::default(),
            is_single_file: true,
            files: vec![DocumentInfo::from_path("/project/doc.qmd")],
            output_dir: PathBuf::from("/project"),

            ..Default::default()
        };
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);
        t.transform(&mut ast, &mut ctx).await.unwrap();
        ast.meta
    }

    #[tokio::test]
    async fn render_writes_both_slots() {
        let meta = {
            let mut m = meta_with_footer_center(s("Footy"));
            m.insert_path(&["logo"], s("l.png"));
            m
        };
        let out = run(RevealFooterLogoTransform::new(), meta).await;
        assert!(
            out.get_path(&["rendered", "reveal", "footer"])
                .and_then(|v| v.as_plain_text())
                .unwrap()
                .contains("Footy")
        );
        assert!(
            out.get_path(&["rendered", "reveal", "logo"])
                .and_then(|v| v.as_plain_text())
                .unwrap()
                .contains("slide-logo")
        );
    }

    #[tokio::test]
    async fn render_respects_pre_populated_footer_slot() {
        let mut meta = meta_with_footer_center(s("Generated"));
        meta.insert_path(&["rendered", "reveal", "footer"], s("OVERRIDE"));
        let out = run(RevealFooterLogoTransform::new(), meta).await;
        assert_eq!(
            out.get_path(&["rendered", "reveal", "footer"])
                .and_then(|v| v.as_plain_text())
                .as_deref(),
            Some("OVERRIDE"),
            "pre-populated slot must win (filter/config override hook)"
        );
    }

    #[tokio::test]
    async fn alias_copies_footer_to_page_footer() {
        let meta = map(vec![("footer", s("Hi"))]);
        let out = run(RevealFooterAliasTransform::new(), meta).await;
        assert_eq!(
            out.get("page-footer")
                .and_then(|v| v.as_plain_text())
                .as_deref(),
            Some("Hi")
        );
    }

    #[tokio::test]
    async fn alias_does_not_override_existing_page_footer() {
        let meta = map(vec![("footer", s("Hi")), ("page-footer", s("Keep"))]);
        let out = run(RevealFooterAliasTransform::new(), meta).await;
        assert_eq!(
            out.get("page-footer")
                .and_then(|v| v.as_plain_text())
                .as_deref(),
            Some("Keep"),
            "explicit page-footer wins over the footer alias"
        );
    }
}
