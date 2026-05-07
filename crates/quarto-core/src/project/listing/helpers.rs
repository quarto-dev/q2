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

fn escape_attr(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('"', "&quot;")
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
}
