/*
 * project/listing/feed/reader_ext.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! Listings-RSS subset of Q1's `readRenderedContents`.
//!
//! Used by the L9 post-render step (`complete_staged_feeds`) to
//! substitute the placeholder envelopes left in staged feed files
//! with engine-rendered preview content drawn from each item's
//! sibling HTML output.
//!
//! The two extractors map directly to the two non-trivial feed
//! types:
//!
//! - [`extract_first_para_html`] — inner HTML of the first
//!   non-empty `<p>` in `main.content`, with `<a>` tags unwrapped
//!   (Q1's `partial` mode strips anchors so subscribers don't
//!   navigate into a Quarto-themed page from a feed reader). For
//!   `partial` feeds.
//! - [`extract_full_contents`] — inner HTML of `main.content` with:
//!     - `<header id="title-block-header">` removed (the post
//!       title is already in `<title>`),
//!     - `<a href="...">` rewritten to absolute URLs (relative
//!       targets resolved against `site_url` + the sibling's
//!       output directory),
//!     - `<img src="...">` similarly rewritten,
//!     - `<a href="#section-x">…</a>` unwrapped to its text
//!       content.
//!   For `full` feeds.
//!
//! ## Bracketing
//!
//! Per the L7 sub-plan §"Reader extensibility" and the L9 plan
//! decision D11, this file is a **sibling** of
//! `crates/quarto-core/src/project/listing/post_render_upgrade/reader.rs`,
//! not an extension of it. L7's reader stays scoped to listings-
//! display extraction (first-para text + preview image) and is
//! private to the `post_render_upgrade` module. L9's RSS reader
//! lives here, in the `feed` submodule, and is private to the
//! feed completion step. Shared helpers may emerge over time but
//! aren't introduced speculatively in v1 — the bracketing rule
//! prefers duplication to a premature abstraction.
//!
//! ## Limitations (v1; tracked as L9 close-out follow-ups)
//!
//! - Truncation under `max_length`: when a long first paragraph
//!   needs to be truncated, the result is **plain text** (HTML
//!   tags dropped) rather than a tag-balanced subset of the
//!   original HTML. Q1 walks the DOM to truncate text nodes
//!   while preserving tag structure; we do not, because
//!   `scraper`'s API is read-only. Subscribers see truncated
//!   plain text; the linked-to HTML page has the full version.
//! - Math (`<span class="math">…`) and syntax-highlight class
//!   maps pass through verbatim. A subscriber whose reader does
//!   not load Quarto's CSS will see source notation rather than
//!   pretty rendering. Filed as a follow-up bd at L9 close-out.

use scraper::{Html, Selector};

/// Extract the inner HTML of the first non-empty `<p>` in
/// `main.content`. Strips `<a>` tags (unwraps to inner content)
/// to match Q1's `partial` feed semantics.
///
/// `max_length` (in characters of visible text) controls
/// truncation. `0` is treated as "no truncation" (Q1 parity).
/// When truncation is needed, the result degrades to plain text
/// — see the module-level limitations note.
///
/// Returns `None` when `main.content` is missing or has no
/// usable paragraph.
pub fn extract_first_para_html(html: &str, max_length: u32) -> Option<String> {
    let doc = Html::parse_document(html);
    let main_sel = Selector::parse("main.content").ok()?;
    let main = doc.select(&main_sel).next()?;
    let p_sel = Selector::parse("p").ok()?;
    for p in main.select(&p_sel) {
        if has_skipped_ancestor_within(&p, &main) {
            continue;
        }
        let inner = p.inner_html();
        let trimmed = inner.trim();
        if trimmed.is_empty() {
            continue;
        }
        let unwrapped = strip_anchor_tags_unwrap(trimmed);
        return Some(maybe_truncate_visible(&unwrapped, max_length as usize));
    }
    None
}

/// Extract `main.content`'s inner HTML with the L9 `full` feed
/// transforms applied: title-block-header removed, relative URLs
/// rewritten to absolute, local-anchor (`href="#…"`) `<a>` tags
/// unwrapped.
///
/// `site_url` is the project's `website.site-url` (with or without
/// a trailing slash). `sibling_output_href` is the sibling's
/// project-relative output path (e.g. `"posts/foo.html"`); its
/// directory is used to resolve relative URLs.
///
/// Returns `None` when `main.content` is missing.
pub fn extract_full_contents(
    html: &str,
    site_url: &str,
    sibling_output_href: &str,
) -> Option<String> {
    let doc = Html::parse_document(html);
    let main_sel = Selector::parse("main.content").ok()?;
    let main = doc.select(&main_sel).next()?;

    let mut inner = main.inner_html();
    inner = strip_title_block_header(&inner);

    let sibling_dir = parent_href_string(sibling_output_href);
    inner = rewrite_relative_urls(&inner, site_url, &sibling_dir);
    inner = strip_local_anchor_links(&inner);

    Some(inner)
}

// ─────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────

/// Skip `<p>` whose nearest non-`main.content` ancestor is page
/// chrome (`<header>`, `<nav>`, `<aside>`, `<footer>`). Mirrors
/// the L7 reader's strategy at
/// `post_render_upgrade/reader.rs::has_skipped_ancestor_within`.
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

/// Strip every `<a ...>…</a>` element by replacing it with its
/// inner content. Non-greedy `.*?` allows the body to contain
/// arbitrary inline elements but does **not** handle nested
/// `<a>` tags (which are invalid HTML in the first place). The
/// regex is hand-rolled rather than tree-walking because
/// `scraper` is read-only and DOM mutation isn't available.
fn strip_anchor_tags_unwrap(html: &str) -> String {
    use regex::RegexBuilder;
    let re = RegexBuilder::new(r"(?s)<a\b[^>]*>(.*?)</a>")
        .build()
        .expect("anchor regex must compile");
    re.replace_all(html, "$1").into_owned()
}

/// Strip just `<a href="#...">…</a>` — the local-anchor variant
/// used by `extract_full_contents`. External / relative-path
/// anchors are preserved (and rewritten to absolute by
/// [`rewrite_relative_urls`]).
fn strip_local_anchor_links(html: &str) -> String {
    use regex::RegexBuilder;
    // Match an opening <a ...> that contains href="#..." (any
    // ordering of attributes), capturing through the matching
    // </a>. Non-greedy body. Same nested-<a> caveat as
    // `strip_anchor_tags_unwrap`.
    let re = RegexBuilder::new(r##"(?s)<a\b[^>]*\bhref="#[^"]*"[^>]*>(.*?)</a>"##)
        .build()
        .expect("local-anchor regex must compile");
    re.replace_all(html, "$1").into_owned()
}

/// Strip a `<header id="title-block-header">…</header>` element.
/// Q1 removes this from the cloned DOM; we do a regex-based
/// removal on the inner-HTML string. Preserves everything else.
fn strip_title_block_header(html: &str) -> String {
    use regex::RegexBuilder;
    let re = RegexBuilder::new(r##"(?s)<header\b[^>]*id="title-block-header"[^>]*>.*?</header>"##)
        .build()
        .expect("title-block regex must compile");
    re.replace_all(html, "").into_owned()
}

/// Rewrite relative `href`s and `src`s to absolute URLs.
/// - Already-absolute (`http://`, `https://`, `//`, `mailto:`,
///   `data:`, `javascript:`) and fragment-only (`#…`) refs are
///   left untouched.
/// - A leading `/` is treated as site-rooted (relative to
///   `site_url`'s host).
/// - All other paths are resolved against `sibling_dir`
///   (forward-slashed; empty = output-dir root).
fn rewrite_relative_urls(html: &str, site_url: &str, sibling_dir: &str) -> String {
    use regex::{Captures, RegexBuilder};
    let base = site_url.trim_end_matches('/');

    let re_href = RegexBuilder::new(r##"(?s)<(a|link)\b([^>]*?)\bhref="([^"]+)"([^>]*)>"##)
        .build()
        .expect("href regex must compile");
    let re_src =
        RegexBuilder::new(r##"(?s)<(img|source|video|audio)\b([^>]*?)\bsrc="([^"]+)"([^>]*)>"##)
            .build()
            .expect("src regex must compile");

    let rewrite_url = |raw: &str| -> String {
        // Leave fragments + already-absolute refs alone.
        if raw.is_empty() || raw.starts_with('#') || is_external_url(raw) {
            return raw.to_string();
        }
        if let Some(rest) = raw.strip_prefix('/') {
            return format!("{}/{}", base, rest);
        }
        // Resolve against sibling_dir (one or more `..` allowed).
        let resolved = if sibling_dir.is_empty() {
            raw.to_string()
        } else {
            collapse_relative(&format!("{}/{}", sibling_dir, raw))
        };
        format!("{}/{}", base, resolved.trim_start_matches('/'))
    };

    let after_href = re_href
        .replace_all(html, |caps: &Captures| {
            let tag = caps.get(1).map_or("", |m| m.as_str());
            let pre = caps.get(2).map_or("", |m| m.as_str());
            let url = caps.get(3).map_or("", |m| m.as_str());
            let post = caps.get(4).map_or("", |m| m.as_str());
            format!(r#"<{}{}href="{}"{}>"#, tag, pre, rewrite_url(url), post)
        })
        .into_owned();

    re_src
        .replace_all(&after_href, |caps: &Captures| {
            let tag = caps.get(1).map_or("", |m| m.as_str());
            let pre = caps.get(2).map_or("", |m| m.as_str());
            let url = caps.get(3).map_or("", |m| m.as_str());
            let post = caps.get(4).map_or("", |m| m.as_str());
            format!(r#"<{}{}src="{}"{}>"#, tag, pre, rewrite_url(url), post)
        })
        .into_owned()
}

/// True if `s` is already an absolute reference. Includes the
/// scheme-relative `//host/path` form and the common pseudo-
/// schemes (`mailto:`, `javascript:`, `data:`) that should pass
/// through unchanged.
fn is_external_url(s: &str) -> bool {
    s.starts_with("http://")
        || s.starts_with("https://")
        || s.starts_with("//")
        || s.starts_with("mailto:")
        || s.starts_with("javascript:")
        || s.starts_with("data:")
        || s.starts_with("tel:")
        || s.starts_with("ftp://")
}

/// Collapse `..` / `.` segments in a forward-slash path. Naive
/// (no normalization across protocol boundaries; we never see
/// those here because the caller filters first).
fn collapse_relative(path: &str) -> String {
    let mut stack: Vec<&str> = Vec::new();
    for seg in path.split('/') {
        match seg {
            "" | "." => {} // strip empty + current-dir segments
            ".." => {
                stack.pop();
            }
            _ => stack.push(seg),
        }
    }
    stack.join("/")
}

/// Forward-slash parent of an output href. `"posts/foo.html"` →
/// `"posts"`; `"foo.html"` → `""`.
fn parent_href_string(href: &str) -> String {
    match href.rfind('/') {
        Some(idx) => href[..idx].to_string(),
        None => String::new(),
    }
}

/// Truncate visible-text length of `html` at a word boundary per
/// Q1's `truncateText(s, n, "space")`, appending `…`. `0` disables
/// truncation. When truncation actually fires, the result is plain
/// text (HTML tags stripped) — see module-level limitations. The
/// fits-check uses strict `<` for Q1 parity (an exactly-`max`-char
/// text is truncated); the cut itself is
/// [`crate::project::listing::helpers::truncate_text_at_space`],
/// shared with the display-side readers.
fn maybe_truncate_visible(html: &str, max: usize) -> String {
    if max == 0 {
        return html.to_string();
    }
    let visible = visible_text(html);
    if visible.chars().count() < max {
        return html.to_string();
    }
    crate::project::listing::helpers::truncate_text_at_space(&visible, max)
}

/// Return the visible-text projection of an HTML fragment: drops
/// everything between `<` and `>` and unescapes a small set of
/// common entities. Sufficient for length estimation; not a
/// full HTML→text engine.
fn visible_text(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    for ch in html.chars() {
        match (in_tag, ch) {
            (false, '<') => in_tag = true,
            (true, '>') => in_tag = false,
            (false, _) => out.push(ch),
            _ => {}
        }
    }
    decode_basic_entities(&out)
}

fn decode_basic_entities(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&nbsp;", " ")
}

// ─────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ---- Plan test #30: returns inner HTML --------------------------

    #[test]
    fn extract_first_para_html_returns_inner_html() {
        let html = r#"<html><body><main class="content"><p>Hello <em>world</em>.</p></main></body></html>"#;
        let out = extract_first_para_html(html, 0);
        assert_eq!(out.as_deref(), Some("Hello <em>world</em>."));
    }

    #[test]
    fn extract_first_para_html_returns_none_when_no_main() {
        let html = r#"<html><body><p>orphan</p></body></html>"#;
        assert_eq!(extract_first_para_html(html, 0), None);
    }

    #[test]
    fn extract_first_para_html_skips_empty_p() {
        let html = r#"<html><body><main class="content">
<p>   </p>
<p>Second wins.</p>
</main></body></html>"#;
        let out = extract_first_para_html(html, 0);
        assert_eq!(out.as_deref(), Some("Second wins."));
    }

    // ---- Plan test #31: anchors unwrapped ---------------------------

    #[test]
    fn extract_first_para_html_strips_anchors() {
        let html = r##"<html><body><main class="content"><p>Click <a href="#x">here</a>.</p></main></body></html>"##;
        let out = extract_first_para_html(html, 0);
        assert_eq!(out.as_deref(), Some("Click here."));
    }

    #[test]
    fn extract_first_para_html_strips_anchors_with_inline_children() {
        let html = r#"<html><body><main class="content"><p>See <a href="x"><strong>here</strong></a>.</p></main></body></html>"#;
        let out = extract_first_para_html(html, 0);
        // Anchor unwraps; <strong> survives.
        assert_eq!(out.as_deref(), Some("See <strong>here</strong>."));
    }

    // ---- Plan test #32: word-boundary truncation -------------------

    // Expectation updated for bd-listing-ellipsis-no-matching-l963osy1:
    // the cut mirrors Q1's `truncateText(s, 20, "space")` — take the
    // first 20 visible chars "The quick brown fox ", drop one, cut at
    // the last space, append `…`. (The full parity battery lives on
    // `maybe_truncate` in `post_render_upgrade/reader.rs`; both
    // wrappers delegate to the same helper.)
    #[test]
    fn extract_first_para_html_truncates_at_word_boundary() {
        let html = r#"<html><body><main class="content"><p>The quick brown fox jumps over the lazy dog.</p></main></body></html>"#;
        let out = extract_first_para_html(html, 20).expect("para found");
        assert_eq!(out, "The quick brown…");
    }

    // Truncation fires on *visible* length; the result is plain text
    // (tags stripped) ending in the ellipsis, with the trailing-comma
    // strip applied before it.
    #[test]
    fn extract_first_para_html_truncated_output_strips_tags_and_comma() {
        // Visible text: "Hello there, world of examples" (30 chars).
        // max=18 → first 18 "Hello there, world", drop one, last
        // space at 12 → "Hello there," → comma stripped → ellipsis.
        let html = r#"<html><body><main class="content"><p>Hello <em>there</em>, world of examples</p></main></body></html>"#;
        let out = extract_first_para_html(html, 18).expect("para found");
        assert_eq!(out, "Hello there…");
    }

    #[test]
    fn extract_first_para_html_no_truncate_when_max_zero() {
        let html = r#"<html><body><main class="content"><p>Hello <em>world</em>.</p></main></body></html>"#;
        let out = extract_first_para_html(html, 0);
        assert_eq!(out.as_deref(), Some("Hello <em>world</em>."));
    }

    // ---- Plan test #33: rewrites relative → absolute --------------

    #[test]
    fn extract_full_contents_rewrites_relative_to_absolute() {
        let html = r#"<html><body><main class="content"><p><a href="../foo.html">link</a></p></main></body></html>"#;
        let out = extract_full_contents(html, "https://example.com", "posts/foo.html")
            .expect("main.content found");
        assert!(
            out.contains(r#"<a href="https://example.com/foo.html">link</a>"#),
            "expected absolute href; got: {}",
            out
        );
    }

    #[test]
    fn extract_full_contents_rewrites_image_src() {
        let html = r#"<html><body><main class="content"><p><img src="../img.png"></p></main></body></html>"#;
        let out = extract_full_contents(html, "https://example.com/", "posts/foo.html")
            .expect("main.content found");
        assert!(
            out.contains(r#"src="https://example.com/img.png""#),
            "expected absolute src; got: {}",
            out
        );
    }

    #[test]
    fn extract_full_contents_passes_external_url_through() {
        let html = r#"<html><body><main class="content"><p><a href="https://other.example/x">x</a> <a href="mailto:hi@example.com">mail</a></p></main></body></html>"#;
        let out = extract_full_contents(html, "https://example.com", "posts/foo.html")
            .expect("main.content found");
        assert!(
            out.contains(r#"href="https://other.example/x""#),
            "external URL must pass through; got: {}",
            out
        );
        assert!(
            out.contains(r#"href="mailto:hi@example.com""#),
            "mailto must pass through; got: {}",
            out
        );
    }

    #[test]
    fn extract_full_contents_resolves_site_rooted_path() {
        let html = r#"<html><body><main class="content"><p><a href="/about.html">About</a></p></main></body></html>"#;
        let out = extract_full_contents(html, "https://example.com", "posts/foo.html")
            .expect("main.content found");
        assert!(
            out.contains(r#"<a href="https://example.com/about.html">About</a>"#),
            "expected absolute href; got: {}",
            out
        );
    }

    // ---- Plan test #34: local anchor links stripped ---------------

    #[test]
    fn extract_full_contents_strips_local_anchor_hrefs() {
        let html = r##"<html><body><main class="content"><p>See <a href="#section-1">Top</a>.</p></main></body></html>"##;
        let out = extract_full_contents(html, "https://example.com", "posts/foo.html")
            .expect("main.content found");
        // Anchor element gone; text preserved.
        assert!(
            out.contains("See Top.") || out.contains("See Top ."),
            "expected anchor unwrapped to inner text; got: {}",
            out
        );
        assert!(
            !out.contains("href=\"#section-1\""),
            "anchor href should be stripped; got: {}",
            out
        );
    }

    #[test]
    fn extract_full_contents_keeps_external_anchors_intact() {
        // External anchors should not be unwrapped — only `href="#…"` are.
        let html = r#"<html><body><main class="content"><p><a href="https://example.com/x">x</a></p></main></body></html>"#;
        let out = extract_full_contents(html, "https://example.com", "posts/foo.html")
            .expect("main.content found");
        assert!(
            out.contains(r#"<a href="https://example.com/x">x</a>"#),
            "external anchor must remain intact; got: {}",
            out
        );
    }

    // ---- Plan test #35: title-block-header skipped ----------------

    #[test]
    fn extract_full_contents_skips_title_block_header() {
        let html = r#"<html><body><main class="content">
<header id="title-block-header"><h1 class="title">My Post</h1></header>
<p>Body of the post.</p>
</main></body></html>"#;
        let out = extract_full_contents(html, "https://example.com", "posts/foo.html")
            .expect("main.content found");
        assert!(
            !out.contains("title-block-header"),
            "title-block-header should be removed; got: {}",
            out
        );
        assert!(
            !out.contains("My Post"),
            "title text should be removed; got: {}",
            out
        );
        assert!(
            out.contains("Body of the post."),
            "body should remain; got: {}",
            out
        );
    }

    // ---- Plan test #36: returns None when no main ------

    #[test]
    fn extract_full_contents_returns_none_when_no_main() {
        let html = r#"<html><body><div>orphan</div></body></html>"#;
        assert_eq!(
            extract_full_contents(html, "https://example.com", "posts/foo.html"),
            None
        );
    }

    // ---- Helpers ------

    #[test]
    fn collapse_relative_strips_dotdot() {
        assert_eq!(collapse_relative("posts/../about.html"), "about.html");
        assert_eq!(collapse_relative("a/b/../../c"), "c");
        assert_eq!(collapse_relative("./a"), "a");
    }

    #[test]
    fn parent_href_string_handles_root_and_nested() {
        assert_eq!(parent_href_string("foo.html"), "");
        assert_eq!(parent_href_string("posts/foo.html"), "posts");
        assert_eq!(parent_href_string("a/b/c.html"), "a/b");
    }

    #[test]
    fn visible_text_drops_tags_and_decodes_entities() {
        assert_eq!(visible_text("<p>a &amp; b</p>"), "a & b");
        assert_eq!(visible_text("plain"), "plain");
    }
}
