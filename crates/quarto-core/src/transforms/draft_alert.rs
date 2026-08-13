/*
 * draft_alert.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * AST transform: mark `draft: true` pages so the HTML template can emit
 * Quarto 1's `#quarto-draft-alert` banner.
 */

//! Emit Quarto 1's draft alert banner for pages with `draft: true`.
//!
//! Quarto 1 renders a warning strip at the top of every draft page:
//!
//! ```html
//! <div id="quarto-draft-alert" class="alert alert-warning"><i class="bi bi-pencil-square"></i>Draft</div>
//! ```
//!
//! It is what tells an author previewing a site that the page they are
//! looking at is unpublished. Without it a draft page is
//! indistinguishable from a finished one — which is how four pages of
//! the Posit Connect docs port came to publish unmarked
//! (bd-draft-banner-missing-hgx1gkqm).
//!
//! ## What this transform does
//!
//! Two metadata writes, both consumed downstream without further wiring:
//!
//! 1. `rendered.draft-alert-text` — the **localized** banner label.
//!    [`FULL_HTML_TEMPLATE`](crate::template) gates its markup on this
//!    key, so this transform is the single place that decides whether a
//!    banner appears at all.
//! 2. `rendered.includes.header` — a `<meta name="quarto:status"
//!    content="draft">` tag, matching Q1's `normalize/draft.lua`.
//!
//! ## Why the markup lives in the template, not here
//!
//! Q1 builds this banner in a **DOM postprocessor**
//! (`format-html.ts:902`, keying off the `quarto:status` meta tag it
//! wrote earlier). Q2 has no post-Pandoc DOM stage and must not grow one
//! (see `CLAUDE.md`), and an AST transform cannot help either: the banner
//! sits *outside* `#quarto-content`, where document blocks land. So the
//! template owns the markup and this transform owns the decision plus the
//! one piece of text that needs computing. The split also keeps the
//! localized string testable without rendering a document.
//!
//! ## Localization
//!
//! Q1 uses `format.language.draft || "Draft"`. The `draft` term is
//! already in `resources/language/_language.yml` and already translated
//! across the `_language-*.yml` set, so the label routes through
//! [`LanguageTerms`] with the same precedence
//! [`TocGenerateTransform`](crate::transforms::TocGenerateTransform) uses:
//! localized term first, English literal as the fallback that covers
//! stage-less unit tests (where `LanguageTerms::from_meta` returns
//! `None`).
//!
//! ## Scope
//!
//! HTML only, and **not** reveal.js slides — mirroring Q1's
//! `isHtmlOutput() and not isHtmlSlideOutput()` guard. Note that Q1's
//! exclusion of slides is *incidental*: the guard predates the banner by
//! three commits and exists for the feature that empties draft documents
//! (`quarto-cli` `99e47b461`), which the banner then inherited by keying
//! off the meta tag only that guarded filter emits. Supporting decks is
//! tracked as **bd-4c7n9o1h**; it needs a separate template
//! (`revealjs::render_revealjs_document` bypasses doctemplate), separate
//! CSS (the reveal bundle has neither `#quarto-draft-alert` nor
//! Bootstrap's `.alert-warning`), and an original placement decision that
//! Q1 ships no reference output for.
//!
//! ## Interaction with `draft-mode`
//!
//! Q1 suppresses the banner for `draft-mode: gone`, where the page is
//! emptied and marked `draft-remove` instead. Q2 has no `draft-mode`
//! option yet (**bd-w0o9**), so every q2 draft is in Q1's
//! "any other mode" case and an unconditional banner is exactly
//! Q1-correct today. When `draft-mode` lands, its suppression belongs in
//! [`apply_draft_alert`] — the single gate.

use quarto_pandoc_types::ConfigValue;
use quarto_pandoc_types::pandoc::Pandoc;

use crate::Result;
use crate::format::{Format, is_revealjs_target};
use crate::language::LanguageTerms;
use crate::render::RenderContext;
use crate::transform::{AstTransform, TransformPhase};
use crate::transforms::website_favicon::append_to_rendered_header;

/// The `<meta>` tag Q1's `normalize/draft.lua` writes for a visible
/// draft. Nothing in q2 consumes it yet — it is emitted for parity and
/// forward compatibility, because the two Q1 features that read it
/// (`llms.txt` generation, `website-draft.ts`) are both on q2's roadmap
/// and this is cheaper than retrofitting later.
const DRAFT_STATUS_META: &str = r#"<meta name="quarto:status" content="draft">"#;

/// Substring identifying [`DRAFT_STATUS_META`] in an existing include
/// list, so a re-run cannot append it twice.
const DRAFT_STATUS_SENTINEL: &str = r#"name="quarto:status""#;

/// AST transform: flag `draft: true` pages for the HTML template.
pub struct DraftAlertTransform;

impl DraftAlertTransform {
    pub fn new() -> Self {
        Self
    }
}

impl Default for DraftAlertTransform {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait(?Send)]
impl AstTransform for DraftAlertTransform {
    fn name(&self) -> &str {
        "draft-alert"
    }

    fn phase(&self) -> TransformPhase {
        TransformPhase::Normalization
    }

    async fn transform(&self, ast: &mut Pandoc, ctx: &mut RenderContext) -> Result<()> {
        if !format_supports_draft_alert(ctx.format) {
            return Ok(());
        }
        apply_draft_alert(&mut ast.meta);
        Ok(())
    }
}

/// Whether `format` should get a draft banner: HTML, but not reveal.js.
///
/// `Format::is_html` is true for reveal too (both are "HTML-based"), so
/// the slide check is a separate term rather than redundant. The check is
/// on `target_format` because that is what distinguishes the preview
/// pseudo-formats: `q2-preview` resolves to the `Html` identifier and
/// *should* get a banner, while `q2-slides` is reveal and should not.
fn format_supports_draft_alert(format: &Format) -> bool {
    format.is_html() && !is_revealjs_target(&format.target_format)
}

/// Write the draft-alert metadata for a draft page. No-op otherwise.
///
/// This is the single gate on whether a banner renders — see the
/// module docs on `draft-mode` (bd-w0o9).
fn apply_draft_alert(meta: &mut ConfigValue) {
    // `as_bool`, not a truthiness check: `draft: false` must behave
    // exactly like an absent key.
    if meta.get("draft").and_then(|v| v.as_bool()) != Some(true) {
        return;
    }

    let label = LanguageTerms::from_meta(meta)
        .and_then(|terms| terms.get("draft").map(|s| s.to_string()))
        // English literal fallback, matching Q1's `|| "Draft"`. Reached
        // when `LanguageResolveStage` has not run (stage-less unit tests).
        .unwrap_or_else(|| "Draft".to_string());

    let source_info = meta.source_info.clone();
    meta.insert_path(
        &["rendered", "draft-alert-text"],
        // Escaped here rather than in the template: doctemplate does not
        // escape interpolations (`$rendered.navigation.navbar$` is raw
        // HTML by design), and a term file is user-editable via the
        // `language:` key. Q1 gets this for free from `createTextNode`.
        ConfigValue::new_string(escape_html_text(&label), source_info),
    );

    append_to_rendered_header_once(meta, DRAFT_STATUS_META.to_string());
}

/// Append `html` to `rendered.includes.header` unless an entry already
/// contains [`DRAFT_STATUS_SENTINEL`], keeping re-runs idempotent.
fn append_to_rendered_header_once(meta: &mut ConfigValue, html: String) {
    let already_present = meta
        .get_path(&["rendered", "includes", "header"])
        .and_then(|v| v.as_array())
        .is_some_and(|items| {
            items.iter().any(|item| {
                item.as_str()
                    .is_some_and(|s| s.contains(DRAFT_STATUS_SENTINEL))
            })
        });
    if already_present {
        return;
    }
    append_to_rendered_header(meta, html);
}

/// Escape a string for HTML *text* context (not attributes).
fn escape_html_text(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_pandoc_types::ConfigMapEntry;
    use quarto_source_map::SourceInfo;

    fn s(value: &str) -> ConfigValue {
        ConfigValue::new_string(value.to_string(), SourceInfo::for_test())
    }

    fn b(value: bool) -> ConfigValue {
        ConfigValue::new_bool(value, SourceInfo::for_test())
    }

    fn map(entries: Vec<(&str, ConfigValue)>) -> ConfigValue {
        let entries = entries
            .into_iter()
            .map(|(k, v)| ConfigMapEntry {
                key: k.to_string(),
                key_source: SourceInfo::for_test(),
                value: v,
            })
            .collect();
        ConfigValue::new_map(entries, SourceInfo::for_test())
    }

    /// Metadata carrying a resolved language table at `quarto.language`,
    /// the shape `LanguageResolveStage` injects and
    /// `LanguageTerms::from_meta` reads back.
    fn meta_with_language(
        lang: &str,
        draft_term: &str,
        extra: Vec<(&str, ConfigValue)>,
    ) -> ConfigValue {
        let mut entries = vec![
            ("lang", s(lang)),
            (
                "quarto",
                map(vec![("language", map(vec![("draft", s(draft_term))]))]),
            ),
        ];
        entries.extend(extra);
        map(entries)
    }

    fn alert_text(meta: &ConfigValue) -> Option<String> {
        meta.get_path(&["rendered", "draft-alert-text"])
            .and_then(|v| v.as_str().map(|s| s.to_string()))
    }

    fn header_includes(meta: &ConfigValue) -> Vec<String> {
        meta.get_path(&["rendered", "includes", "header"])
            .and_then(|v| v.as_array())
            .map(|items| {
                items
                    .iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default()
    }

    // === Gating ===

    #[test]
    fn no_draft_key_is_a_no_op() {
        let mut meta = map(vec![("title", s("Doc"))]);
        apply_draft_alert(&mut meta);
        assert_eq!(alert_text(&meta), None);
        assert!(header_includes(&meta).is_empty());
    }

    /// `draft: false` must be indistinguishable from an absent key. A
    /// presence-based check would pass `no_draft_key_is_a_no_op` and
    /// still fail here.
    #[test]
    fn draft_false_is_a_no_op() {
        let mut meta = map(vec![("title", s("Doc")), ("draft", b(false))]);
        apply_draft_alert(&mut meta);
        assert_eq!(alert_text(&meta), None);
        assert!(header_includes(&meta).is_empty());
    }

    #[test]
    fn draft_true_sets_text_and_status_meta() {
        let mut meta = map(vec![("title", s("Doc")), ("draft", b(true))]);
        apply_draft_alert(&mut meta);
        assert_eq!(alert_text(&meta).as_deref(), Some("Draft"));
        assert_eq!(
            header_includes(&meta),
            vec![r#"<meta name="quarto:status" content="draft">"#]
        );
    }

    // === Localization ===

    /// The label comes from the `draft` language term, not a literal.
    #[test]
    fn draft_label_is_localized() {
        let mut meta = meta_with_language("es", "Borrador", vec![("draft", b(true))]);
        apply_draft_alert(&mut meta);
        assert_eq!(alert_text(&meta).as_deref(), Some("Borrador"));
    }

    /// The `quarto:status` meta tag stays English regardless of `lang` —
    /// it is a machine-readable marker, not display text.
    #[test]
    fn status_meta_is_not_localized() {
        let mut meta = meta_with_language("es", "Borrador", vec![("draft", b(true))]);
        apply_draft_alert(&mut meta);
        assert_eq!(
            header_includes(&meta),
            vec![r#"<meta name="quarto:status" content="draft">"#]
        );
    }

    /// Without a resolved language table (stage-less contexts) the
    /// English literal stands in, matching Q1's `|| "Draft"`.
    #[test]
    fn falls_back_to_english_without_language_table() {
        let mut meta = map(vec![("lang", s("es")), ("draft", b(true))]);
        apply_draft_alert(&mut meta);
        assert_eq!(alert_text(&meta).as_deref(), Some("Draft"));
    }

    /// A term file is user-editable through the `language:` key, so the
    /// label reaches an HTML text context and must be escaped.
    #[test]
    fn label_is_html_escaped() {
        let mut meta = meta_with_language("en", "Draft <script>", vec![("draft", b(true))]);
        apply_draft_alert(&mut meta);
        assert_eq!(alert_text(&meta).as_deref(), Some("Draft &lt;script&gt;"));
    }

    // === Idempotency ===

    /// Re-running must not append a second `quarto:status` tag.
    #[test]
    fn rerun_does_not_duplicate_status_meta() {
        let mut meta = map(vec![("draft", b(true))]);
        apply_draft_alert(&mut meta);
        apply_draft_alert(&mut meta);
        assert_eq!(header_includes(&meta).len(), 1);
        assert_eq!(alert_text(&meta).as_deref(), Some("Draft"));
    }

    /// Existing header includes are preserved, not replaced.
    #[test]
    fn status_meta_appends_to_existing_header_includes() {
        let mut meta = map(vec![("draft", b(true))]);
        append_to_rendered_header(&mut meta, "<link rel=\"icon\" href=\"f.ico\">".to_string());
        apply_draft_alert(&mut meta);
        assert_eq!(
            header_includes(&meta),
            vec![
                "<link rel=\"icon\" href=\"f.ico\">".to_string(),
                r#"<meta name="quarto:status" content="draft">"#.to_string(),
            ]
        );
    }

    // === Format gating ===

    #[test]
    fn html_supports_the_banner() {
        assert!(format_supports_draft_alert(&Format::html()));
    }

    /// The preview pseudo-format resolves to the HTML identifier and is
    /// where the banner matters most (an author previewing a site).
    #[test]
    fn preview_pseudo_format_supports_the_banner() {
        let format =
            Format::from_format_string("q2-preview").expect("q2-preview is a known format");
        assert!(format_supports_draft_alert(&format));
    }

    /// Reveal decks are deliberately excluded pending bd-4c7n9o1h.
    /// `is_html()` alone would be true here, which is why the gate needs
    /// the second term.
    #[test]
    fn revealjs_does_not_support_the_banner() {
        let format = Format::from_format_string("revealjs").expect("revealjs is a known format");
        assert!(
            format.is_html(),
            "guard would be vacuous if reveal were not html-based"
        );
        assert!(!format_supports_draft_alert(&format));
    }

    #[test]
    fn slides_pseudo_format_does_not_support_the_banner() {
        let format = Format::from_format_string("q2-slides").expect("q2-slides is a known format");
        assert!(!format_supports_draft_alert(&format));
    }

    #[test]
    fn pdf_does_not_support_the_banner() {
        let format = Format::from_format_string("pdf").expect("pdf is a known format");
        assert!(!format_supports_draft_alert(&format));
    }
}
