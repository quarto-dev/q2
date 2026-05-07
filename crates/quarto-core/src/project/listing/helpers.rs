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
pub fn image_html(item: &ListingItem, listing: &Listing) -> String {
    let Some(src) = item.image.as_deref() else {
        return String::new();
    };
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

/// Build the description placeholder comment for an item. The L7
/// post-render upgrade scans the rendered HTML for these comments
/// and substitutes the engine-rendered first-paragraph when
/// available.
///
/// Even when L7 doesn't run (hub-client / future `quarto preview`),
/// the L1 fallback `description` already lives next to the
/// placeholder in the rendered markup, so the listing renders
/// correctly without substitution.
pub fn description_placeholder(item: &ListingItem, listing: &Listing) -> String {
    placeholders::description_placeholder(
        &listing.id,
        listing.max_description_length,
        &item.output_href,
    )
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
        let html = image_html(&item, &make_listing());
        assert!(html.contains(r#"src="img.png""#));
        assert!(html.contains(r#"class="thumbnail-image""#));
        assert!(html.contains(r#"loading="lazy""#));
    }

    #[test]
    fn image_html_empty_when_no_image() {
        let item = make_item_with_image(None);
        assert_eq!(image_html(&item, &make_listing()), "");
    }

    #[test]
    fn image_html_escapes_attribute_chars() {
        let mut item = make_item_with_image(Some("a&b.png"));
        item.image_alt = Some(r#"some "alt" text"#.to_string());
        let html = image_html(&item, &make_listing());
        assert!(html.contains(r#"src="a&amp;b.png""#));
        assert!(html.contains(r#"alt="some &quot;alt&quot; text""#));
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
    fn description_placeholder_uses_listing_id_and_max_len() {
        let item = make_item_with_image(None);
        let comment = description_placeholder(&item, &make_listing());
        assert!(comment.contains("desc(5A0113B34292)"));
        assert!(comment.contains("[max=175]"));
        assert!(comment.contains("posts/foo.html"));
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
