/*
 * project/listing/placeholders.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! Stable hex tokens and envelope markers for description / image
//! placeholders, shared between the L3 listing render transform
//! (which emits them) and the L7 post-render upgrade (which consumes
//! them).
//!
//! The `5A0113B34292` / `9CEB782EFEE6` hex tokens come verbatim from
//! Q1's `website-listing-template.ts` so anyone reading the rendered
//! HTML sees the same magic strings Q1 emits. The `-begin` / `-end`
//! suffix is a Q2-specific extension: Q1 emits a single comment as
//! a substitution marker, but Q2's L1-fallback contract requires the
//! marker delimit a *region* (the L1 fallback content) so L7 knows
//! exactly what to replace with engine-rendered content.
//!
//! See `claude-notes/plans/2026-05-07-listings-L7-postrender-upgrade.md`
//! §"Architecture: marker design" for the full rationale.

/// Hex token used in the `desc-begin` / `desc-end` envelope:
/// `<!-- desc-begin(5A0113B34292)[max=<n>]:<output-href> -->`
/// `<!-- desc-end(5A0113B34292) -->`
pub const DESC_TOKEN: &str = "5A0113B34292";

/// Hex token used in the `img-begin` / `img-end` envelope:
/// `<!-- img-begin(9CEB782EFEE6)[<attrs>]:<id>:<idx>:<href>:<b64-default> -->`
/// `<!-- img-end(9CEB782EFEE6) -->`
pub const IMG_TOKEN: &str = "9CEB782EFEE6";

// ─────────────────────────────────────────────────────────────────
// Description envelope
// ─────────────────────────────────────────────────────────────────

/// Build the description envelope's begin marker.
///
/// Shape: `<!-- desc-begin(<TOKEN>)[max=<n>]:<output-href> -->`.
/// `_listing_id` is unused in the marker itself (the begin/end pair
/// is token-keyed, so the regex can find pairs without per-listing
/// disambiguation). Accepted for symmetry with the helpers.rs
/// caller signature in case a future use case wants to thread it.
pub fn description_placeholder_begin(
    _listing_id: &str,
    max_length: u32,
    output_href: &str,
) -> String {
    format!(
        "<!-- desc-begin({})[max={}]:{} -->",
        DESC_TOKEN, max_length, output_href
    )
}

/// Build the description envelope's end marker.
///
/// Shape: `<!-- desc-end(<TOKEN>) -->`. Token-only — paired with the
/// begin marker by token literal, with `(.*?)` non-greedy matching
/// in [`DESC_REGEX`] so two adjacent envelopes pair correctly.
pub fn description_placeholder_end() -> String {
    format!("<!-- desc-end({}) -->", DESC_TOKEN)
}

/// Regex source for the description envelope. Captures:
///
/// 1. `max-length` integer (from the begin marker).
/// 2. `output-href` (from the begin marker; no spaces allowed).
/// 3. inner region (the L1 fallback content; surrounded by
///    optional whitespace that the regex strips).
///
/// Compile with [`regex::RegexBuilder::dot_matches_new_line(true)`]
/// so the inner-region capture spans across `<p>` tags emitted by
/// Pandoc. The token is baked in literally (the `regex` crate has
/// no backreference support); if [`DESC_TOKEN`] ever changes, the
/// `regex_matches_builder_output` test below will fail.
pub const DESC_REGEX: &str = r"<!-- desc-begin\(5A0113B34292\)\[max=([0-9]+)\]:([^ ]+) -->\s*(.*?)\s*<!-- desc-end\(5A0113B34292\) -->";

// ─────────────────────────────────────────────────────────────────
// Image envelope
// ─────────────────────────────────────────────────────────────────

/// Build the image envelope's begin marker.
///
/// Shape: `<!-- img-begin(<TOKEN>)[<attrs>]:<id>:<idx>:<href>:<b64-default> -->`.
///
/// `b64_default` is the listing's configured `image-placeholder` URL
/// encoded with [`base64::engine::general_purpose::URL_SAFE_NO_PAD`]
/// — empty when the listing has no `image-placeholder:` set.
/// Embedding it here keeps L7's post_render code self-contained: it
/// never has to walk source profiles to find listing config.
pub fn image_placeholder_begin(
    listing_id: &str,
    item_index: usize,
    output_href: &str,
    attrs: &str,
    b64_default: &str,
) -> String {
    format!(
        "<!-- img-begin({})[{}]:{}:{}:{}:{} -->",
        IMG_TOKEN, attrs, listing_id, item_index, output_href, b64_default
    )
}

/// Build the image envelope's end marker.
///
/// Shape: `<!-- img-end(<TOKEN>) -->`. Same token-only contract as
/// the description end marker.
pub fn image_placeholder_end() -> String {
    format!("<!-- img-end({}) -->", IMG_TOKEN)
}

/// Regex source for the image envelope. Captures:
///
/// 1. `attrs` (free-form; `]` is the only forbidden character).
/// 2. `listing-id` (no `:` allowed).
/// 3. `item-index` (digits only).
/// 4. `output-href` (no `:` or space allowed).
/// 5. `b64-default` URL using URL_SAFE_NO_PAD alphabet
///    (`[A-Za-z0-9_-]*`); empty when the listing has no
///    `image-placeholder:` set.
/// 6. inner region (the empty placeholder div).
///
/// Compile with DOTALL so the inner capture spans across the
/// `<div>...</div>` line breaks Pandoc emits. As with [`DESC_REGEX`],
/// the token is baked in literally; the `regex_matches_builder_output`
/// test below catches any drift between [`IMG_TOKEN`] and this regex.
pub const IMG_REGEX: &str = r"<!-- img-begin\(9CEB782EFEE6\)\[([^\]]*)\]:([^:]*):(\d+):([^: ]+):([A-Za-z0-9_-]*) -->\s*(.*?)\s*<!-- img-end\(9CEB782EFEE6\) -->";

// ─────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;
    use regex::RegexBuilder;

    // L7 plan §"Tests" Phase 1 #1
    #[test]
    fn description_placeholder_begin_matches_shape() {
        let s = description_placeholder_begin("my-listing", 175, "posts/foo.html");
        assert_eq!(
            s,
            "<!-- desc-begin(5A0113B34292)[max=175]:posts/foo.html -->"
        );
    }

    // L7 plan §"Tests" Phase 1 #2
    #[test]
    fn description_placeholder_end_matches_shape() {
        let s = description_placeholder_end();
        assert_eq!(s, "<!-- desc-end(5A0113B34292) -->");
    }

    // L7 plan §"Tests" Phase 1 #3
    #[test]
    fn image_placeholder_begin_matches_shape_no_default() {
        let s = image_placeholder_begin(
            "my-listing",
            3,
            "posts/foo.html",
            "progressive=false, height=, lazy=true",
            "",
        );
        assert_eq!(
            s,
            "<!-- img-begin(9CEB782EFEE6)[progressive=false, height=, lazy=true]:my-listing:3:posts/foo.html: -->"
        );
    }

    // L7 plan §"Tests" Phase 1 #4
    #[test]
    fn image_placeholder_begin_matches_shape_with_default() {
        // Encode a known URL with URL_SAFE_NO_PAD; assert the marker
        // carries the encoded form verbatim.
        let url = "assets/default.png";
        let encoded = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(url.as_bytes());
        let s = image_placeholder_begin(
            "main",
            0,
            "posts/no-image.html",
            "progressive=false, height=, lazy=true",
            &encoded,
        );
        // The encoded URL should appear at the very end (just before
        // the trailing ` -->`).
        let expected_suffix = format!(":{} -->", encoded);
        assert!(
            s.ends_with(&expected_suffix),
            "expected marker to end with `{}`, got: {}",
            expected_suffix,
            s
        );
        assert!(s.starts_with("<!-- img-begin(9CEB782EFEE6)["));
    }

    // L7 plan §"Tests" Phase 1 #5
    #[test]
    fn description_placeholder_regex_round_trip() {
        let begin = description_placeholder_begin("listing-1", 175, "posts/foo.html");
        let end = description_placeholder_end();
        let inner = "<p>L1 fallback first paragraph.</p>";
        let html = format!("...prefix...\n{begin}\n{inner}\n{end}\n...suffix...");

        let re = RegexBuilder::new(DESC_REGEX)
            .dot_matches_new_line(true)
            .build()
            .expect("DESC_REGEX must compile");
        let caps = re.captures(&html).expect("regex must match envelope");
        assert_eq!(&caps[1], "175");
        assert_eq!(&caps[2], "posts/foo.html");
        assert_eq!(caps[3].trim(), inner);
    }

    // L7 plan §"Tests" Phase 1 #6 — empty b64-default
    #[test]
    fn image_placeholder_regex_round_trip_empty_default() {
        let begin = image_placeholder_begin(
            "main",
            7,
            "posts/foo.html",
            "progressive=false, height=, lazy=true",
            "",
        );
        let end = image_placeholder_end();
        let inner = r#"<div class="listing-item-img-placeholder card-img-top">&nbsp;</div>"#;
        let html = format!("{begin}\n{inner}\n{end}");

        let re = RegexBuilder::new(IMG_REGEX)
            .dot_matches_new_line(true)
            .build()
            .expect("IMG_REGEX must compile");
        let caps = re.captures(&html).expect("regex must match envelope");
        assert_eq!(&caps[1], "progressive=false, height=, lazy=true");
        assert_eq!(&caps[2], "main");
        assert_eq!(&caps[3], "7");
        assert_eq!(&caps[4], "posts/foo.html");
        assert_eq!(&caps[5], "");
        assert_eq!(caps[6].trim(), inner);
    }

    // L7 plan §"Tests" Phase 1 #6 — non-empty b64-default
    #[test]
    fn image_placeholder_regex_round_trip_with_default() {
        let url = "assets/site/listing-default.png";
        let encoded = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(url.as_bytes());
        let begin = image_placeholder_begin(
            "main",
            0,
            "posts/foo.html",
            "progressive=false, height=, lazy=true",
            &encoded,
        );
        let end = image_placeholder_end();
        let html = format!("{begin}\nINNER\n{end}");

        let re = RegexBuilder::new(IMG_REGEX)
            .dot_matches_new_line(true)
            .build()
            .expect("IMG_REGEX must compile");
        let caps = re.captures(&html).expect("regex must match envelope");
        assert_eq!(&caps[5], encoded);
    }

    // Non-greedy `.*?` plus DOTALL must pair adjacent same-token
    // envelopes correctly: two listing items on one page each get
    // their own match, the second begin marker doesn't get treated
    // as inner content for the first envelope.
    #[test]
    fn description_regex_handles_two_envelopes_on_one_page() {
        let envelope_a = format!(
            "{}\n<p>A fallback.</p>\n{}",
            description_placeholder_begin("l", 100, "a.html"),
            description_placeholder_end()
        );
        let envelope_b = format!(
            "{}\n<p>B fallback.</p>\n{}",
            description_placeholder_begin("l", 200, "b.html"),
            description_placeholder_end()
        );
        let html = format!("{envelope_a}\n\n{envelope_b}");

        let re = RegexBuilder::new(DESC_REGEX)
            .dot_matches_new_line(true)
            .build()
            .unwrap();
        let matches: Vec<_> = re.captures_iter(&html).collect();
        assert_eq!(matches.len(), 2, "expected two envelope matches");
        assert_eq!(&matches[0][2], "a.html");
        assert_eq!(&matches[1][2], "b.html");
        assert_eq!(matches[0][3].trim(), "<p>A fallback.</p>");
        assert_eq!(matches[1][3].trim(), "<p>B fallback.</p>");
    }

    // Drift guard: the regex's literal token must match the constant.
    // If someone changes DESC_TOKEN or IMG_TOKEN without updating
    // the regex, this test catches it.
    #[test]
    fn regex_literal_tokens_match_constants() {
        assert!(
            DESC_REGEX.contains(DESC_TOKEN),
            "DESC_REGEX must literally contain DESC_TOKEN; if you change \
             one, change the other. DESC_REGEX={DESC_REGEX}, DESC_TOKEN={DESC_TOKEN}"
        );
        assert!(
            IMG_REGEX.contains(IMG_TOKEN),
            "IMG_REGEX must literally contain IMG_TOKEN; if you change \
             one, change the other. IMG_REGEX={IMG_REGEX}, IMG_TOKEN={IMG_TOKEN}"
        );
    }
}
