/*
 * project/listing/post_render_upgrade/reader.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * HTML reader: listings-only subset of Q1's `readRenderedContents`.
 */

//! Listings-only HTML reader.
//!
//! Mirrors the listing-specific selectors and extractors in
//! `external-sources/quarto-cli/src/project/types/website/listing/`
//! (`website-listing-shared.ts` and `util/discover-meta.ts`).
//!
//! The two extractors:
//!
//! - [`extract_first_para`] returns the first non-empty `<p>` text
//!   from `main.content`, optionally truncated at a word boundary
//!   ≤ `max_length` chars. v1 returns plain text; HTML-aware
//!   extraction is a follow-up.
//! - [`extract_preview_image`] returns the first `<img>` matching
//!   Q1's selector chain: explicit `.preview-image` author marker,
//!   code-cell-wrapped marker, named-pattern src match
//!   (`preview` / `feature` / `cover` / `thumbnail` substring), and
//!   first image in `#quarto-document-content`.
//!
//! Both return `None` when nothing usable is found; the L7
//! substitution caller then strips just the begin/end markers,
//! leaving the L1 fallback in place (and emits Q-12-13 for the
//! description case).
//!
//! ## Reader extensibility
//!
//! L9 (RSS feeds) will need more from this reader: math handling,
//! syntax-highlight class maps, urls-to-absolute, anchor stripping.
//! The [`ReaderOptions`] struct is structured to accept new fields
//! without breaking L7's call sites — L7 always passes
//! [`ReaderOptions::default()`] for any new field, so new behavior
//! is opt-in. Each new transform should be a private function in
//! this file, guarded by its `ReaderOptions` flag.
//!
//! Do not introduce a trait-based plugin architecture in v1. Q1's
//! single-function reader has been stable for years; the cost of an
//! abstraction outweighs the benefit until at least L9.

use scraper::{Html, Selector};

/// Output of an HTML extraction.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RenderedExtraction {
    /// First-paragraph preview, already truncated if `max_length`
    /// was set in [`ReaderOptions`]. v1 emits plain text; HTML-aware
    /// extraction is a follow-up.
    pub first_para_html: Option<String>,
    /// First preview image, if any.
    pub preview_image: Option<PreviewImage>,
    /// Original HTML the extraction was computed from. Cached so the
    /// L7 substitution layer can re-run [`extract`] with per-envelope
    /// `max_length` truncation without re-reading the file. Set by
    /// the cache layer in `substitute.rs`; left `None` by direct
    /// callers of [`extract`].
    pub cached_html: Option<String>,
}

/// One discovered preview image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewImage {
    /// `src` attribute as it appears in the rendered HTML
    /// (relative to the sibling output dir, or absolute / data URI).
    /// L7's substitution code re-relativizes this against the
    /// listing host's output directory.
    pub src: String,
    /// `alt` attribute, if present.
    pub alt: Option<String>,
    /// `title` attribute, if present.
    pub title: Option<String>,
}

/// Options controlling [`extract`]'s behavior. All fields default
/// to "off" so L7 callers can pass `Default` and only opt into the
/// transforms they need.
#[derive(Debug, Clone, Default)]
pub struct ReaderOptions {
    /// Maximum character count for the truncated first-paragraph
    /// preview. `None` (or `Some(0)`) disables truncation. Mirrors
    /// Q1's `max-description-length` / `max-length` config — Q1
    /// treats `0` and missing as "no truncation."
    pub max_length: Option<usize>,
    /// L9 placeholder: when true, unwrap `<a>` tags in the first
    /// paragraph. v1 returns plain text from the paragraph so this
    /// option is currently a no-op (links unwrap by default in
    /// plain-text output); kept for forward compatibility.
    #[allow(dead_code)]
    pub remove_links: bool,
    /// L9 placeholder: when true, drop `<img>` tags. v1 returns
    /// plain text so img is dropped by default.
    #[allow(dead_code)]
    pub remove_images: bool,
}

/// Parse `html` and run both extractions in one pass. Returns the
/// per-file extraction; never panics on malformed HTML (`scraper` is
/// lenient).
pub fn extract(html: &str, opts: &ReaderOptions) -> RenderedExtraction {
    let doc = Html::parse_document(html);
    RenderedExtraction {
        first_para_html: extract_first_para(&doc, opts),
        preview_image: extract_preview_image(&doc),
        cached_html: None,
    }
}

// ─────────────────────────────────────────────────────────────────
// First-paragraph extractor
// ─────────────────────────────────────────────────────────────────

/// Find the first non-empty `<p>` inside `main.content` and return
/// its text content, truncated at the last word boundary ≤
/// `max_length` chars (when set). Returns `None` if `main.content`
/// is missing or contains no usable text.
///
/// **v1 limitation:** returns plain text, dropping any inline tags
/// (`<em>`, `<strong>`, `<a>`, etc.). Q1 returns plain text too
/// (its `truncateText` strips before truncating), so this matches
/// observable behavior. Richer HTML preservation is a follow-up.
fn extract_first_para(doc: &Html, opts: &ReaderOptions) -> Option<String> {
    let main_sel = Selector::parse("main.content").ok()?;
    let main = doc.select(&main_sel).next()?;

    // Walk `<p>` descendants in document order, skipping any whose
    // ancestry includes structural scaffolding that Quarto wraps
    // around the document title or navigation chrome:
    //
    // - `<header id="title-block-header">` carries the post's
    //   title — already shown as the listing item's title and
    //   would be redundant in the description.
    // - `<nav>`, `<aside>`, `<footer>` are page-level chrome.
    //
    // Direct-child `<p>` matches (the simple case for short
    // posts), and `<p>` inside `<section>` wrappers (Quarto's
    // section-wrapped content) also match.
    let p_sel = Selector::parse("p").ok()?;
    for p in main.select(&p_sel) {
        if has_skipped_ancestor_within(&p, &main) {
            continue;
        }
        let text = collect_text(&p);
        let trimmed = text.trim();
        if !trimmed.is_empty() {
            return Some(maybe_truncate(trimmed, opts.max_length));
        }
    }

    None
}

/// True when any ancestor of `elem` (up to but excluding `boundary`)
/// is a structural scaffolding element whose text content is page
/// chrome, not preview content. See [`extract_first_para`] for the
/// full list of skipped tag names.
fn has_skipped_ancestor_within(elem: &scraper::ElementRef, boundary: &scraper::ElementRef) -> bool {
    let mut cur = elem.parent();
    while let Some(node) = cur {
        if let Some(elem_ref) = scraper::ElementRef::wrap(node) {
            if elem_ref.id() == boundary.id() {
                return false;
            }
            let tag = elem_ref.value().name();
            if matches!(tag, "header" | "nav" | "aside" | "footer") {
                return true;
            }
        }
        cur = node.parent();
    }
    false
}

/// Concatenate descendant text nodes into a single string, with no
/// extra whitespace. Tag content is unwrapped (so anchor text and
/// `<em>` text both appear inline).
fn collect_text(elem: &scraper::ElementRef) -> String {
    elem.text().collect::<Vec<_>>().concat()
}

/// Truncate `s` at a word boundary ≤ `max_length` characters. Mirrors
/// Q1's `truncateText(s, n, "space")` — break at the last space
/// before `n`. `Some(0)` disables truncation (Q1 treats 0 as missing).
fn maybe_truncate(s: &str, max_length: Option<usize>) -> String {
    let max = match max_length {
        Some(0) | None => return s.to_string(),
        Some(n) => n,
    };
    if s.chars().count() <= max {
        return s.to_string();
    }
    // Walk char indices to find the truncation point at or before
    // `max` characters; back up to the last space.
    let mut last_space_byte: Option<usize> = None;
    let mut byte_at_max: Option<usize> = None;
    for (idx, (byte_idx, c)) in s.char_indices().enumerate() {
        if idx >= max {
            byte_at_max = Some(byte_idx);
            break;
        }
        if c.is_whitespace() {
            last_space_byte = Some(byte_idx);
        }
    }
    let cut = match (last_space_byte, byte_at_max) {
        (Some(b), _) => b,
        (None, Some(b)) => b, // No space within window — hard cut at max.
        (None, None) => return s.to_string(),
    };
    s[..cut].trim_end().to_string()
}

// ─────────────────────────────────────────────────────────────────
// Preview-image extractor
// ─────────────────────────────────────────────────────────────────

/// Find the first preview image, walking Q1's selector chain in
/// order. Returns `None` if no image matches.
///
/// Q1 selectors (in `findPreviewImgEl`):
///   1. `img.preview-image` (explicit author marker).
///   2. `div.preview-image div.cell-output-display img` (code-cell
///      wrapped marker).
///   3. Any `<img>` whose `src` matches the named-pattern regex
///      (`preview` / `feature` / `cover` / `thumbnail` substring,
///      case-insensitive, with image extension) — *or* whose src
///      starts with `data:` (data URI; e.g. matplotlib output).
///   4. `#quarto-document-content img` (first local image).
///   5. None.
fn extract_preview_image(doc: &Html) -> Option<PreviewImage> {
    // 1. Explicit marker.
    if let Some(img) = first_match(doc, "img.preview-image") {
        return Some(img);
    }
    // 2. Code-cell wrapped marker.
    if let Some(img) = first_match(doc, "div.preview-image div.cell-output-display img") {
        return Some(img);
    }
    // 3. Named-pattern / data-URI scan over all `<img>`.
    let all_img = Selector::parse("img").ok()?;
    let pattern = named_image_regex();
    for img_el in doc.select(&all_img) {
        let Some(src) = img_el.value().attr("src") else {
            continue;
        };
        if src.starts_with("data:") || pattern.is_match(src) {
            return Some(img_from_element(img_el));
        }
    }
    // 4. First image in `#quarto-document-content`.
    if let Some(img) = first_match(doc, "#quarto-document-content img") {
        return Some(img);
    }
    None
}

/// Compile (once per call — micro-optimization not warranted; L7's
/// caller invokes this at most once per sibling) the named-pattern
/// regex used in step 3 above. Mirrors Q1's `kNamedFilePattern`
/// from `util/discover-meta.ts:49` exactly: case-insensitive,
/// matches `preview` / `feature` / `cover` / `thumbnail` substring
/// followed by an image extension.
fn named_image_regex() -> regex::Regex {
    regex::RegexBuilder::new(
        r".*?(preview|feature|cover|thumbnail).*?\.(png|gif|jpg|jpeg|webp|svg)",
    )
    .case_insensitive(true)
    .build()
    .expect("named_image_regex must compile")
}

fn first_match(doc: &Html, css: &str) -> Option<PreviewImage> {
    let sel = Selector::parse(css).ok()?;
    let img_el = doc.select(&sel).next()?;
    Some(img_from_element(img_el))
}

fn img_from_element(img_el: scraper::ElementRef) -> PreviewImage {
    let attr = |name: &str| img_el.value().attr(name).map(str::to_string);
    PreviewImage {
        src: attr("src").unwrap_or_default(),
        alt: attr("alt"),
        title: attr("title"),
    }
}

// ─────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn opts(max: Option<usize>) -> ReaderOptions {
        ReaderOptions {
            max_length: max,
            remove_links: true,
            remove_images: true,
        }
    }

    fn parse(html: &str) -> Html {
        Html::parse_document(html)
    }

    // L7 plan §"Tests" Phase 3 #14
    #[test]
    fn extract_first_para_returns_first_p_text() {
        let html = r#"<html><body><main class="content"><p>Hello.</p></main></body></html>"#;
        let result = extract_first_para(&parse(html), &opts(None));
        assert_eq!(result.as_deref(), Some("Hello."));
    }

    // L7 plan §"Tests" Phase 3 #15
    #[test]
    fn extract_first_para_skips_empty_p() {
        let html = r#"<html><body><main class="content">
<p>   </p>
<p>Second paragraph wins.</p>
</main></body></html>"#;
        let result = extract_first_para(&parse(html), &opts(None));
        assert_eq!(result.as_deref(), Some("Second paragraph wins."));
    }

    // L7 plan §"Tests" Phase 3 #16
    #[test]
    fn extract_first_para_truncates_to_max_length() {
        // 30-char text; max=20 → truncate at last word boundary ≤ 20.
        let html = r#"<html><body><main class="content"><p>The quick brown fox jumps over.</p></main></body></html>"#;
        let result = extract_first_para(&parse(html), &opts(Some(20))).unwrap();
        assert!(
            result.chars().count() <= 20,
            "expected ≤ 20 chars, got {}: `{}`",
            result.chars().count(),
            result
        );
        // Truncation happens at the last space before char-20.
        // "The quick brown fox " is 20 chars; the last space is
        // before 'jumps'. Result: "The quick brown fox" (trimmed).
        assert_eq!(result, "The quick brown fox");
    }

    // L7 plan §"Tests" Phase 3 #17
    #[test]
    fn extract_first_para_remove_links_unwraps_anchors() {
        // v1 returns plain text from `text()`; anchors auto-unwrap
        // in plain text. The remove_links option is forward-compat
        // for L9.
        let html = r#"<html><body><main class="content">
<p>Click <a href="x">here</a>.</p>
</main></body></html>"#;
        let result = extract_first_para(&parse(html), &opts(None));
        assert_eq!(result.as_deref(), Some("Click here."));
    }

    // L7 plan §"Tests" Phase 3 #18
    #[test]
    fn extract_first_para_remove_images_drops_imgs() {
        // <img> contributes no text content; plain-text extraction
        // already drops it. Forward-compat option.
        let html = r#"<html><body><main class="content">
<p><img src="x" alt="ignored">Hi</p>
</main></body></html>"#;
        let result = extract_first_para(&parse(html), &opts(None));
        assert_eq!(result.as_deref(), Some("Hi"));
    }

    // L7 plan §"Tests" Phase 3 #19 — revised after end-to-end
    // verification (Phase 6): the original "any node" fallback
    // was too eager for Q2's rendered output, where Quarto wraps
    // the document title in `<header id="title-block-header">`
    // inside `main.content`. Picking up that header's text
    // produced the post's title as the listing description, which
    // duplicated the listing item's title field.
    //
    // New behavior: descendant `<p>` only. If no `<p>` exists, the
    // extractor returns `None` and L7 keeps the L1 fallback in
    // place.
    #[test]
    fn extract_first_para_finds_p_inside_quarto_section_wrapper() {
        // Quarto wraps content under headings in `<section>`, with
        // the actual `<p>` nested. A descendant search picks it up.
        let html = r#"<html><body><main class="content">
<header id="title-block-header"><h1>Title chrome</h1></header>
<section id="x" class="section level1">
  <h1>Heading</h1>
  <p>Body para inside section.</p>
</section>
</main></body></html>"#;
        let result = extract_first_para(&parse(html), &opts(None));
        assert_eq!(result.as_deref(), Some("Body para inside section."));
    }

    // Regression for the heading-only case found during Phase 6
    // end-to-end verification: a post with only a heading must
    // produce no preview, so L7 keeps the L1 fallback in place.
    #[test]
    fn extract_first_para_returns_none_when_main_has_only_heading() {
        let html = r#"<html><body><main class="content">
<header id="title-block-header"><h1 class="title">Title</h1></header>
<section id="x" class="section level1"><h1>Just a heading</h1></section>
</main></body></html>"#;
        assert_eq!(extract_first_para(&parse(html), &opts(None)), None);
    }

    // Regression: text inside `<header id="title-block-header">`
    // (Quarto's title block) must be skipped.
    #[test]
    fn extract_first_para_skips_title_block_header_paragraphs() {
        // Pathological case: title block contains a `<p>` (e.g. a
        // subtitle). The extractor must skip the header's `<p>`
        // and find the body paragraph.
        let html = r#"<html><body><main class="content">
<header id="title-block-header">
  <p class="subtitle">Subtitle text</p>
</header>
<p>Real body paragraph.</p>
</main></body></html>"#;
        let result = extract_first_para(&parse(html), &opts(None));
        assert_eq!(result.as_deref(), Some("Real body paragraph."));
    }

    // L7 plan §"Tests" Phase 3 #20
    #[test]
    fn extract_first_para_returns_none_when_main_empty() {
        // Empty main.content.
        let html = r#"<html><body><main class="content"></main></body></html>"#;
        assert_eq!(extract_first_para(&parse(html), &opts(None)), None);
        // No main at all.
        let html2 = r#"<html><body><p>Outside.</p></body></html>"#;
        assert_eq!(extract_first_para(&parse(html2), &opts(None)), None);
    }

    // L7 plan §"Tests" Phase 3 #21
    #[test]
    fn extract_preview_image_finds_explicit_preview_class() {
        let html = r#"<html><body><main class="content">
<img src="hero.jpg" class="preview-image" alt="Hero">
<img src="ignored.jpg">
</main></body></html>"#;
        let result = extract_preview_image(&parse(html)).unwrap();
        assert_eq!(result.src, "hero.jpg");
        assert_eq!(result.alt.as_deref(), Some("Hero"));
    }

    // L7 plan §"Tests" Phase 3 #22
    #[test]
    fn extract_preview_image_finds_cell_output_wrapper() {
        let html = r#"<html><body><main class="content">
<div class="preview-image">
  <div class="cell-output-display">
    <img src="cell-out.png">
  </div>
</div>
</main></body></html>"#;
        let result = extract_preview_image(&parse(html)).unwrap();
        assert_eq!(result.src, "cell-out.png");
    }

    // L7 plan §"Tests" Phase 3 #23
    #[test]
    fn extract_preview_image_finds_named_pattern() {
        let html = r#"<html><body>
<img src="path/preview-image.png">
<img src="other.png">
</body></html>"#;
        let result = extract_preview_image(&parse(html)).unwrap();
        assert_eq!(result.src, "path/preview-image.png");
    }

    #[test]
    fn extract_preview_image_finds_data_uri() {
        // matplotlib-style inline image. Q1 catches `data:` URIs in
        // the same step as named patterns.
        let html = r#"<html><body>
<img src="data:image/png;base64,iVBORw0KGgo">
</body></html>"#;
        let result = extract_preview_image(&parse(html)).unwrap();
        assert!(result.src.starts_with("data:image/png"));
    }

    // L7 plan §"Tests" Phase 3 #24
    #[test]
    fn extract_preview_image_finds_first_in_quarto_doc() {
        // None of the earlier selectors match; falls through to
        // `#quarto-document-content img`.
        let html = r#"<html><body>
<div id="quarto-document-content">
  <p>some body</p>
  <img src="local.png">
  <img src="second.png">
</div>
</body></html>"#;
        let result = extract_preview_image(&parse(html)).unwrap();
        assert_eq!(result.src, "local.png");
    }

    // L7 plan §"Tests" Phase 3 #25
    #[test]
    fn extract_preview_image_returns_none_when_no_img() {
        let html = r#"<html><body><main class="content">
<p>Just text, no images.</p>
</main></body></html>"#;
        assert_eq!(extract_preview_image(&parse(html)), None);
    }

    // Drift guard: max_length 0 means "no truncation" (matches Q1).
    #[test]
    fn extract_first_para_max_length_zero_is_no_truncation() {
        let html = r#"<html><body><main class="content"><p>The quick brown fox jumps over the lazy dog.</p></main></body></html>"#;
        let full = extract_first_para(&parse(html), &opts(None)).unwrap();
        let zero = extract_first_para(&parse(html), &opts(Some(0))).unwrap();
        assert_eq!(full, zero);
        assert_eq!(zero, "The quick brown fox jumps over the lazy dog.");
    }
}
