/*
 * project/listing/helpers.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! Pre-rendered helper strings for listing items.
//!
//! doctemplate has no function-call surface, so the per-item
//! TemplateValue::Map carries a handful of pre-rendered HTML
//! strings the templates can splice with `$item.<helper>$`.
//! These replace Q1's `listing.utilities.*` calls.
//!
//! All helpers return strings (possibly empty) — never `None`. The
//! template uses `$if(<helper>)$` to decide whether to emit
//! surrounding markup; an empty string is falsy and skips the
//! surrounding block.

use base64::Engine;

use super::config::Listing;
use super::item::ListingItem;
use super::placeholders;

/// Build the `<img>` HTML string for a listing item, or an empty
/// string when no image was discovered. Intentionally minimal in
/// v1: no `srcset`, no responsive sizing — Q1 produces the same
/// shape for static `image:` values.
///
/// The template emits this verbatim (e.g. inside a `<a>` thumbnail
/// wrapper) so the markup must already be self-contained HTML.
pub fn image_html(item: &ListingItem, listing: &Listing, host_dir: &str) -> String {
    let Some(src) = item.image.as_deref() else {
        return String::new();
    };
    // `item.image` is project-relative after hydration's rebase
    // (bd-qv2lsab0); the emitted src must resolve from the host
    // page. Remote/absolute forms pass through.
    let src = host_relative_url(src, host_dir);
    let src = src.as_str();
    let alt = item
        .image_alt
        .as_deref()
        .unwrap_or("")
        .replace('"', "&quot;");
    let lazy = item
        .image_lazy_loading
        .unwrap_or_else(|| listing.image_lazy_loading.unwrap_or(true));
    let lazy_attr = if lazy { r#" loading="lazy""# } else { "" };
    format!(
        r#"<img src="{}" class="thumbnail-image" alt="{}"{}>"#,
        escape_attr(src),
        alt,
        lazy_attr
    )
}

/// Convert a project-relative URL to a host-page-relative one.
/// `host_dir` is the host page's project-relative directory
/// (empty at the project root). Walks up out of the host dir with
/// `..` segments when the target is not beneath it. Remote URLs,
/// `data:` URIs, and root-absolute paths pass through unchanged.
pub(crate) fn host_relative_url(project_relative: &str, host_dir: &str) -> String {
    if is_external_src(project_relative) {
        return project_relative.to_string();
    }
    if host_dir.is_empty() {
        return project_relative.to_string();
    }
    let host_segs: Vec<&str> = host_dir.split('/').filter(|s| !s.is_empty()).collect();
    let target_segs: Vec<&str> = project_relative
        .split('/')
        .filter(|s| !s.is_empty())
        .collect();
    let common = host_segs
        .iter()
        .zip(target_segs.iter())
        .take_while(|(a, b)| a == b)
        .count();
    let mut out: Vec<&str> = std::iter::repeat_n("..", host_segs.len() - common).collect();
    out.extend(&target_segs[common..]);
    out.join("/")
}

/// True for src values that name no file in the project tree:
/// remote URLs, `data:` URIs, and root-absolute paths. Shared by
/// the hydration rebase, host-relativization, and the copy-intent
/// registration so they agree on what "local" means (bd-qv2lsab0).
pub(crate) fn is_external_src(src: &str) -> bool {
    let lower = src.to_ascii_lowercase();
    lower.starts_with("http://")
        || lower.starts_with("https://")
        || lower.starts_with("data:")
        || src.starts_with('/')
}

/// Build the `data-*` attributes string used by `list.min.js` for
/// per-item filter / sort / category tracking. Empty when the
/// listing has no `metadata-attrs`-relevant fields configured.
///
/// Q1 emits e.g. `data-categories="rust,design" data-listing-date="…"`.
/// v1 emits `data-index` plus `data-categories` if any. The
/// list.min.js sort/filter UI is gated on these attrs.
pub fn metadata_attrs(item: &ListingItem, item_index: usize) -> String {
    let mut parts = vec![format!(r#"data-index="{}""#, item_index)];
    if !item.categories.is_empty() {
        let cats = item
            .categories
            .iter()
            .map(|c| c.replace('"', "&quot;"))
            .collect::<Vec<_>>()
            .join(",");
        parts.push(format!(r#"data-categories="{}""#, cats));
    }
    parts.join(" ")
}

/// Build the description envelope's begin marker for an item.
///
/// L3 emits this just before the L1 fallback `$description$` block
/// in the listing item template; L7's post-render upgrade scans
/// for the begin/end pair and substitutes the engine-rendered first
/// paragraph between them when available, or strips just the
/// markers (keeping the L1 fallback) when not. Both states render
/// correctly — see the L1 safeguard contract.
pub fn description_placeholder_begin(item: &ListingItem, listing: &Listing) -> String {
    placeholders::description_placeholder_begin(
        &listing.id,
        listing.max_description_length,
        &item.output_href,
    )
}

/// Build the description envelope's end marker. Token-only; paired
/// with the begin marker via the L7 regex.
pub fn description_placeholder_end() -> String {
    placeholders::description_placeholder_end()
}

/// Build the image envelope's begin marker for an item.
///
/// `idx` is the item's index in the listing (used by Q1's interactive
/// JS for stable per-item ids; carried verbatim through the marker).
///
/// The marker carries the listing's configured `image-placeholder:`
/// URL base64-encoded with [`base64::engine::general_purpose::URL_SAFE_NO_PAD`].
/// Empty when the listing has no `image-placeholder:` set; the L7
/// substitution then falls through to the empty-div placeholder.
/// Embedding it here means L7 doesn't have to walk source profiles
/// to find listing config at substitution time — see plan
/// §"Architecture: image-placeholder cascade in detail".
///
/// `attrs` is fixed at `progressive=false, height=, lazy=true` in
/// v1 (matching Q1's emission shape). Wiring `listing.image_height`
/// / `listing.image_lazy_loading` into the substituted `<img>` is a
/// follow-up; v1 emits the static defaults verbatim.
pub fn image_placeholder_begin(item: &ListingItem, listing: &Listing, idx: usize) -> String {
    use base64::Engine;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    let attrs = "progressive=false, height=, lazy=true";
    let b64_default = listing
        .image_placeholder
        .as_deref()
        .map(|url| URL_SAFE_NO_PAD.encode(url.as_bytes()))
        .unwrap_or_default();
    placeholders::image_placeholder_begin(&listing.id, idx, &item.output_href, attrs, &b64_default)
}

/// Build the image envelope's end marker. Token-only; paired with
/// the begin marker via the L7 regex.
pub fn image_placeholder_end() -> String {
    placeholders::image_placeholder_end()
}

/// Build the per-item categories chip block. Empty when the item has
/// no categories (the template's `$if(category-html)$` guard skips
/// the surrounding markup).
///
/// Mirrors Q1's `item-default.ejs.md` per-item category emission
/// (lines 67–73): a wrapping `<div class="listing-categories">` and
/// one `<div class="listing-category">` per category, each with an
/// `onclick` that calls `window.quartoListingCategory(<b64>)`.
///
/// The `<b64>` argument matches Q1's `b64EncodeUnicode`
/// (`btoa(encodeURIComponent(s))`) so the vendored
/// `quarto-listing.js` decoder (`decodeURIComponent(atob(b64))`)
/// round-trips correctly for non-ASCII categories. See `bd-754f` for
/// the planned review of this scheme.
pub fn category_html(item: &ListingItem) -> String {
    if item.categories.is_empty() {
        return String::new();
    }
    let mut s = String::from(r#"<div class="listing-categories">"#);
    for cat in &item.categories {
        let b64 = b64_encode_unicode(cat);
        s.push_str(&format!(
            r#"<div class="listing-category" onclick="window.quartoListingCategory('{}'); return false;">{}</div>"#,
            escape_attr(&b64),
            html_escape_text(cat),
        ));
    }
    s.push_str("</div>");
    s
}

/// Mirror Q1's `b64EncodeUnicode` from `core/base64.ts`:
/// `btoa(encodeURIComponent(s))`. The vendored `quarto-listing.js`
/// decodes with `decodeURIComponent(atob(b64))`, so the Rust side
/// must percent-encode UTF-8 before base64-encoding.
fn b64_encode_unicode(s: &str) -> String {
    let percent = encode_uri_component(s);
    base64::engine::general_purpose::STANDARD.encode(percent.as_bytes())
}

/// JavaScript-compatible `encodeURIComponent`. Encodes every UTF-8
/// byte as `%XX` except the unreserved set:
/// `A-Z a-z 0-9 - _ . ! ~ * ' ( )`.
fn encode_uri_component(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for byte in s.as_bytes() {
        let b = *byte;
        let unreserved = b.is_ascii_alphanumeric()
            || matches!(
                b,
                b'-' | b'_' | b'.' | b'!' | b'~' | b'*' | b'\'' | b'(' | b')'
            );
        if unreserved {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{:02X}", b));
        }
    }
    out
}

fn escape_attr(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Escape text content for HTML. We don't strictly need `"` / `'`
/// escaping in PCDATA, but escaping them too keeps the output safe
/// inside ambiguous contexts (e.g. when the text is later embedded
/// in attributes by a Lua filter).
fn html_escape_text(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::listing::config::{Listing, ListingType};
    use crate::project::listing::item::ListingItem;
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    fn make_item_with_image(image: Option<&str>) -> ListingItem {
        ListingItem {
            title: "Title".to_string(),
            subtitle: None,
            description: None,
            author: None,
            authors: vec![],
            date: None,
            date_modified: None,
            categories: vec![],
            image: image.map(String::from),
            image_alt: None,
            image_lazy_loading: None,
            reading_time_minutes: None,
            word_count: None,
            source_path: PathBuf::from("posts/foo.qmd"),
            output_href: "posts/foo.html".to_string(),
            extra: BTreeMap::new(),
        }
    }

    fn make_listing() -> Listing {
        Listing {
            id: "listing-1".to_string(),
            kind: ListingType::Default,
            max_description_length: 175,
            ..Listing::default()
        }
    }

    #[test]
    fn image_html_when_image_present() {
        let item = make_item_with_image(Some("img.png"));
        let html = image_html(&item, &make_listing(), "");
        assert!(html.contains(r#"src="img.png""#));
        assert!(html.contains(r#"class="thumbnail-image""#));
        assert!(html.contains(r#"loading="lazy""#));
    }

    #[test]
    fn image_html_empty_when_no_image() {
        let item = make_item_with_image(None);
        assert_eq!(image_html(&item, &make_listing(), ""), "");
    }

    #[test]
    fn image_html_escapes_attribute_chars() {
        let mut item = make_item_with_image(Some("a&b.png"));
        item.image_alt = Some(r#"some "alt" text"#.to_string());
        let html = image_html(&item, &make_listing(), "");
        assert!(html.contains(r#"src="a&amp;b.png""#));
        assert!(html.contains(r#"alt="some &quot;alt&quot; text""#));
    }

    #[test]
    fn image_html_relativizes_against_host_dir() {
        // Project-relative image, host inside the same dir → bare name.
        let item = make_item_with_image(Some("posts/cover.png"));
        let html = image_html(&item, &make_listing(), "posts");
        assert!(html.contains(r#"src="cover.png""#), "got: {html}");
        // Host at project root → project-relative passes through.
        let html = image_html(&item, &make_listing(), "");
        assert!(html.contains(r#"src="posts/cover.png""#), "got: {html}");
    }

    #[test]
    fn host_relative_url_walks_up_with_dotdot() {
        assert_eq!(
            host_relative_url("shared/x.png", "posts"),
            "../shared/x.png"
        );
        assert_eq!(host_relative_url("posts/a/x.png", "posts/b"), "../a/x.png");
        assert_eq!(
            host_relative_url("https://example.com/x.png", "posts"),
            "https://example.com/x.png"
        );
        assert_eq!(host_relative_url("/abs.png", "posts"), "/abs.png");
    }

    #[test]
    fn metadata_attrs_includes_index() {
        let item = make_item_with_image(None);
        let attrs = metadata_attrs(&item, 7);
        assert!(attrs.contains(r#"data-index="7""#));
    }

    #[test]
    fn metadata_attrs_includes_categories_when_present() {
        let mut item = make_item_with_image(None);
        item.categories = vec!["rust".to_string(), "design".to_string()];
        let attrs = metadata_attrs(&item, 0);
        assert!(attrs.contains(r#"data-categories="rust,design""#));
    }

    #[test]
    fn description_placeholder_begin_carries_max_len_and_href() {
        let item = make_item_with_image(None);
        let comment = description_placeholder_begin(&item, &make_listing());
        assert_eq!(
            comment,
            "<!-- desc-begin(5A0113B34292)[max=175]:posts/foo.html -->"
        );
    }

    #[test]
    fn description_placeholder_end_is_token_only() {
        // No item / listing args: end marker is token-only.
        let comment = description_placeholder_end();
        assert_eq!(comment, "<!-- desc-end(5A0113B34292) -->");
    }

    #[test]
    fn image_placeholder_begin_emits_static_attrs_when_no_default() {
        let item = make_item_with_image(None);
        let listing = make_listing(); // image_placeholder: None
        let comment = image_placeholder_begin(&item, &listing, 4);
        // Verify the attrs string + listing-id + idx + href and that
        // the b64-default segment is empty.
        assert_eq!(
            comment,
            "<!-- img-begin(9CEB782EFEE6)[progressive=false, height=, lazy=true]:listing-1:4:posts/foo.html: -->"
        );
    }

    #[test]
    fn image_placeholder_begin_b64_encodes_listing_default_url() {
        use base64::Engine;
        use base64::engine::general_purpose::URL_SAFE_NO_PAD;
        let item = make_item_with_image(None);
        let mut listing = make_listing();
        listing.image_placeholder = Some("assets/site/default.png".to_string());
        let comment = image_placeholder_begin(&item, &listing, 0);
        let expected_b64 = URL_SAFE_NO_PAD.encode("assets/site/default.png".as_bytes());
        // The b64 of the URL must appear verbatim at the trailing
        // `:b64 -->` slot.
        let expected_suffix = format!(":{} -->", expected_b64);
        assert!(
            comment.ends_with(&expected_suffix),
            "expected marker to end with `{}`, got: {}",
            expected_suffix,
            comment
        );
    }

    #[test]
    fn image_placeholder_end_is_token_only() {
        let comment = image_placeholder_end();
        assert_eq!(comment, "<!-- img-end(9CEB782EFEE6) -->");
    }

    fn make_item_with_categories(categories: Vec<&str>) -> ListingItem {
        let mut item = make_item_with_image(None);
        item.categories = categories.into_iter().map(String::from).collect();
        item
    }

    // L5 plan §"Tests" #1
    #[test]
    fn category_html_empty_when_item_has_no_categories() {
        let item = make_item_with_image(None);
        assert_eq!(category_html(&item), "");
    }

    // L5 plan §"Tests" #2
    #[test]
    fn category_html_emits_one_div_per_category() {
        let item = make_item_with_categories(vec!["rust", "design"]);
        let html = category_html(&item);
        // Outer wrapper
        assert!(html.starts_with(r#"<div class="listing-categories">"#));
        assert!(html.ends_with("</div>"));
        // Two listing-category chips
        assert_eq!(html.matches(r#"<div class="listing-category""#).count(), 2);
        // Both display strings present
        assert!(html.contains(">rust<"));
        assert!(html.contains(">design<"));
    }

    // L5 plan §"Tests" #3 — Q1-compatible encoding round-trip
    // Q1's `b64EncodeUnicode` is `btoa(encodeURIComponent(s))`, decoded
    // by `decodeURIComponent(atob(b64))` in the vendored quarto-listing.js.
    #[test]
    fn category_html_b64_encodes_handler_arg() {
        // Non-ASCII: "café" -> encodeURIComponent -> "caf%C3%A9" -> btoa -> Y2FmJUMzJUE5
        let item = make_item_with_categories(vec!["café"]);
        let html = category_html(&item);
        assert!(
            html.contains(r#"window.quartoListingCategory('Y2FmJUMzJUE5')"#),
            "expected Q1-style b64(percent-encoded UTF-8); got: {html}"
        );

        // ASCII-only: "rust" -> "rust" -> btoa -> cnVzdA==
        // (locks the ASCII path; identical under raw-bytes encoding too)
        let item = make_item_with_categories(vec!["rust"]);
        let html = category_html(&item);
        assert!(
            html.contains(r#"window.quartoListingCategory('cnVzdA==')"#),
            "expected ASCII b64; got: {html}"
        );
    }

    // L5 plan §"Tests" #4
    #[test]
    fn category_html_html_escapes_display_text() {
        let item = make_item_with_categories(vec!["<script>"]);
        let html = category_html(&item);
        assert!(html.contains("&lt;script&gt;"));
        // No unescaped `<script>` substring anywhere in the output.
        assert!(
            !html.contains("<script>"),
            "raw <script> must not appear; got: {html}"
        );
    }

    // Lock down the JS-encodeURIComponent compatible encoder. JS's
    // encodeURIComponent unreserved set is: A-Z a-z 0-9 - _ . ! ~ * ' ( )
    #[test]
    fn encode_uri_component_unreserved_passes_through() {
        // Every char in the unreserved set is preserved verbatim.
        let unreserved = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_.!~*'()";
        assert_eq!(encode_uri_component(unreserved), unreserved);
    }

    #[test]
    fn encode_uri_component_reserved_ascii_percent_encoded() {
        // A handful of important reserved chars. Compare to Node:
        //   > encodeURIComponent(' "%/:;<=>?@[\\]^`{|}')
        //   '%20%22%25%2F%3A%3B%3C%3D%3E%3F%40%5B%5C%5D%5E%60%7B%7C%7D'
        assert_eq!(encode_uri_component(" "), "%20");
        assert_eq!(encode_uri_component("/"), "%2F");
        assert_eq!(encode_uri_component(":"), "%3A");
        assert_eq!(encode_uri_component("&"), "%26");
        assert_eq!(encode_uri_component("="), "%3D");
        assert_eq!(encode_uri_component("%"), "%25");
        assert_eq!(encode_uri_component("\""), "%22");
        assert_eq!(encode_uri_component("\\"), "%5C");
        assert_eq!(encode_uri_component("{}"), "%7B%7D");
    }

    #[test]
    fn encode_uri_component_utf8_bytes_percent_encoded() {
        // "é" is U+00E9 → UTF-8 bytes 0xC3 0xA9 → "%C3%A9"
        assert_eq!(encode_uri_component("é"), "%C3%A9");
        // "café" combines unreserved + reserved
        assert_eq!(encode_uri_component("café"), "caf%C3%A9");
        // "✓" is U+2713 → UTF-8 0xE2 0x9C 0x93 → "%E2%9C%93"
        assert_eq!(encode_uri_component("✓"), "%E2%9C%93");
    }

    #[test]
    fn b64_encode_unicode_matches_q1() {
        // Q1's b64EncodeUnicode("café") -> btoa("caf%C3%A9") -> "Y2FmJUMzJUE5"
        assert_eq!(b64_encode_unicode("café"), "Y2FmJUMzJUE5");
        // Empty string round-trips to empty.
        assert_eq!(b64_encode_unicode(""), "");
        // ASCII unreserved: btoa("rust") -> "cnVzdA=="
        assert_eq!(b64_encode_unicode("rust"), "cnVzdA==");
    }
}
