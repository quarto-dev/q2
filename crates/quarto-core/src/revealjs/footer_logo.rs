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

use quarto_pandoc_types::pandoc::Pandoc;
use quarto_pandoc_types::{ConfigValue, ConfigValueKind};
use quarto_source_map::{By, SourceInfo};

use crate::Result;
use crate::render::RenderContext;
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

    async fn transform(&self, ast: &mut Pandoc, _ctx: &mut RenderContext) -> Result<()> {
        if !ast.meta.contains_path(&["rendered", "reveal", "footer"])
            && let Some(html) = footer_slot_html(&ast.meta)
        {
            ast.meta.insert_path(
                &["rendered", "reveal", "footer"],
                ConfigValue::new_string(&html, SourceInfo::generated(By::revealjs())),
            );
        }

        if !ast.meta.contains_path(&["rendered", "reveal", "logo"])
            && let Some(html) = logo_slot_html(&ast.meta)
        {
            ast.meta.insert_path(
                &["rendered", "reveal", "logo"],
                ConfigValue::new_string(&html, SourceInfo::generated(By::revealjs())),
            );
        }

        Ok(())
    }
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

/// Build the reveal logo `<img>` from `logo:`, or `None` when unset/blank.
fn logo_slot_html(meta: &ConfigValue) -> Option<String> {
    let path = meta
        .get("logo")
        .and_then(|v| v.as_plain_text())
        .filter(|s| !s.trim().is_empty())?;
    Some(format!(
        r#"<img class="slide-logo" src="{}">"#,
        attr_escape(path.trim())
    ))
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
        let link = Inline::Link(Link {
            attr: (String::new(), vec![], LinkedHashMap::new()),
            content: vec![str_inline("Quarto")],
            target: ("https://quarto.org".to_string(), String::new()),
            source_info: si(),
            attr_source: quarto_pandoc_types::attr::AttrSourceInfo::empty(),
            target_source: quarto_pandoc_types::attr::TargetSourceInfo::empty(),
        });
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
        let m = map(vec![("logo", s("logo.png"))]);
        assert_eq!(
            logo_slot_html(&m).unwrap(),
            r#"<img class="slide-logo" src="logo.png">"#
        );
    }

    #[test]
    fn logo_slot_escapes_path_attr() {
        let m = map(vec![("logo", s(r#"a"b.png"#))]);
        assert_eq!(
            logo_slot_html(&m).unwrap(),
            r#"<img class="slide-logo" src="a&quot;b.png">"#
        );
    }

    #[test]
    fn logo_slot_none_when_absent_or_blank() {
        assert!(logo_slot_html(&map(vec![("title", s("T"))])).is_none());
        assert!(logo_slot_html(&map(vec![("logo", s("  "))])).is_none());
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
