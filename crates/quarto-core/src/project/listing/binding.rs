/*
 * project/listing/binding.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! Build the per-listing [`TemplateValue::Map`] binding consumed by
//! the doctemplate built-in templates.
//!
//! See L2 §"Per-item template binding" for the canonical shape.
//! This module is the *load-bearing public contract* for L8 custom
//! templates: adding a key is non-breaking, but renaming or
//! removing a key is breaking and must be called out in commits +
//! the L11 close-out report.
//!
//! Note: this v1 binding intentionally does **not** include:
//! - `other-metadata-html` — defer to bd-0wyo.
//!
//! `category-html` was added by L5 (bd-5vsr); see
//! [`helpers::category_html`].

use std::collections::HashMap;

use quarto_doctemplate::{TemplateContext, TemplateValue};
use quarto_pandoc_types::ConfigValue;

use super::config::{GridItemAlign, ImageAlign, Listing, ListingCategoriesMode, ListingType};
use super::helpers;
use super::item::ListingItem;

/// Build the full [`TemplateContext`] for one [`Listing`] +
/// hydrated items. The returned context has a single entry,
/// keyed by `"listing"`, plus an `"items"` entry — matching the
/// L2 binding contract — plus a `"project"` map carrying any
/// project-wide values the templates read.
///
/// Arguments:
///
/// - `host_dir` is the host page's project-relative directory
///   (forward-slash, no trailing slash; empty string when the
///   host is at the project root). Used to compute
///   host-dir-relative `.qmd` link targets so
///   [`crate::transforms::LinkRewriteTransform`] can rewrite them
///   downstream.
/// - `project_meta` is the host page's merged metadata; we read
///   `website.site-url` and `website.title` from it for the
///   `project.*` binding.
pub fn build_listing_context(
    listing: &Listing,
    items: &[ListingItem],
    host_dir: &str,
    project_meta: &ConfigValue,
) -> TemplateContext {
    let mut ctx = TemplateContext::new();

    ctx.insert("listing", build_listing_map(listing));
    ctx.insert("items", build_items_list(listing, items, host_dir));
    ctx.insert("project", build_project_map(project_meta));

    ctx
}

fn build_listing_map(listing: &Listing) -> TemplateValue {
    let mut m = HashMap::new();
    m.insert("id".to_string(), TemplateValue::String(listing.id.clone()));
    m.insert(
        "type".to_string(),
        TemplateValue::String(listing_type_name(listing.kind).to_string()),
    );
    m.insert(
        "fields".to_string(),
        TemplateValue::List(
            listing
                .fields
                .iter()
                .map(|s| TemplateValue::String(s.clone()))
                .collect(),
        ),
    );
    m.insert(
        "page-size".to_string(),
        TemplateValue::String(listing.page_size.to_string()),
    );
    if let Some(max) = listing.max_items {
        m.insert(
            "max-items".to_string(),
            TemplateValue::String(max.to_string()),
        );
    }
    m.insert(
        "filter-ui".to_string(),
        TemplateValue::Bool(listing.filter_ui),
    );
    m.insert("sort-ui".to_string(), TemplateValue::Bool(listing.sort_ui));
    m.insert(
        "max-description-length".to_string(),
        TemplateValue::String(listing.max_description_length.to_string()),
    );
    m.insert(
        "categories".to_string(),
        TemplateValue::String(categories_mode_name(listing.categories).to_string()),
    );

    // Type-specific knobs.
    if let Some(align) = listing.image_align {
        m.insert(
            "image-align".to_string(),
            TemplateValue::String(image_align_name(align).to_string()),
        );
    }
    if let Some(h) = listing.image_height.as_deref() {
        m.insert(
            "image-height".to_string(),
            TemplateValue::String(h.to_string()),
        );
    }
    if let Some(lazy) = listing.image_lazy_loading {
        m.insert("image-lazy-loading".to_string(), TemplateValue::Bool(lazy));
    }
    if let Some(cols) = listing.grid_columns {
        m.insert(
            "grid-columns".to_string(),
            TemplateValue::String(cols.to_string()),
        );
    }
    if let Some(border) = listing.grid_item_border {
        m.insert("grid-item-border".to_string(), TemplateValue::Bool(border));
    }
    if let Some(align) = listing.grid_item_align {
        m.insert(
            "grid-item-align".to_string(),
            TemplateValue::String(grid_item_align_name(align).to_string()),
        );
    }
    if let Some(striped) = listing.table_striped {
        m.insert("table-striped".to_string(), TemplateValue::Bool(striped));
    }
    if let Some(hover) = listing.table_hover {
        m.insert("table-hover".to_string(), TemplateValue::Bool(hover));
    }

    // Author-supplied template-params get exposed verbatim under
    // `listing.template-params` for L8 custom templates. v1 is
    // permissive: any ConfigValue scalars become strings.
    if !listing.template_params.is_empty() {
        let mut tp = HashMap::new();
        for (k, v) in &listing.template_params {
            tp.insert(k.clone(), config_value_to_template_value(v));
        }
        m.insert("template-params".to_string(), TemplateValue::Map(tp));
    }

    TemplateValue::Map(m)
}

fn build_items_list(listing: &Listing, items: &[ListingItem], host_dir: &str) -> TemplateValue {
    TemplateValue::List(
        items
            .iter()
            .enumerate()
            .map(|(i, item)| build_item_map(listing, item, i, host_dir))
            .collect(),
    )
}

fn build_item_map(
    listing: &Listing,
    item: &ListingItem,
    index: usize,
    host_dir: &str,
) -> TemplateValue {
    let mut m = HashMap::new();

    // Curated fields. Optional fields are only inserted when
    // present so $if(<field>)$ correctly skips undefined values
    // (rather than seeing an empty-string truthy false).
    m.insert(
        "title".to_string(),
        TemplateValue::String(item.title.clone()),
    );
    if let Some(s) = item.subtitle.as_deref() {
        m.insert("subtitle".to_string(), TemplateValue::String(s.to_string()));
    }
    if let Some(s) = item.description.as_deref() {
        m.insert(
            "description".to_string(),
            TemplateValue::String(s.to_string()),
        );
    }
    if let Some(s) = item.author.as_deref() {
        m.insert("author".to_string(), TemplateValue::String(s.to_string()));
    }
    if !item.authors.is_empty() {
        m.insert(
            "authors".to_string(),
            TemplateValue::List(
                item.authors
                    .iter()
                    .map(|a| TemplateValue::String(a.clone()))
                    .collect(),
            ),
        );
    }
    if let Some(s) = item.date.as_deref() {
        m.insert("date".to_string(), TemplateValue::String(s.to_string()));
    }
    if let Some(s) = item.date_modified.as_deref() {
        m.insert(
            "date-modified".to_string(),
            TemplateValue::String(s.to_string()),
        );
    }
    if !item.categories.is_empty() {
        m.insert(
            "categories".to_string(),
            TemplateValue::List(
                item.categories
                    .iter()
                    .map(|c| TemplateValue::String(c.clone()))
                    .collect(),
            ),
        );
    }
    if let Some(s) = item.image.as_deref() {
        m.insert("image".to_string(), TemplateValue::String(s.to_string()));
    }
    if let Some(s) = item.image_alt.as_deref() {
        m.insert(
            "image-alt".to_string(),
            TemplateValue::String(s.to_string()),
        );
    }
    if let Some(n) = item.reading_time_minutes {
        m.insert(
            "reading-time".to_string(),
            TemplateValue::String(format!("{} min read", n)),
        );
        m.insert(
            "reading-time-minutes".to_string(),
            TemplateValue::String(n.to_string()),
        );
    }
    if let Some(n) = item.word_count {
        m.insert(
            "word-count".to_string(),
            TemplateValue::String(n.to_string()),
        );
    }

    // Path bookkeeping.
    //
    // `path` is the link target the templates use. We emit the
    // item's source-path-relative-to-the-host-page (e.g.
    // `a-second-listing.qmd` from `posts/index.qmd`) — the same
    // form a body link `[label](other.qmd)` would take.
    // `LinkRewriteTransform` (which runs after this transform in
    // the AstTransformsStage Finalization phase) then rewrites the
    // .qmd to the page-relative output URL via the active
    // resolver. This is what makes the listing's links navigate
    // correctly in:
    //   - native CLI: rewrites to e.g. `a-second-listing.html`
    //   - hub-client/WASM (vfs_root resolver): rewrites to a
    //     `/.quarto/project-artifacts/...` URL that
    //     iframePostProcessor reverse-maps to .qmd for in-app
    //     navigation (case 3 in iframePostProcessor.ts).
    //
    // `outputHref` retains the rendered .html path for templates
    // (e.g. L7's placeholder href, RSS feed item URLs) that need
    // the post-render output href specifically.
    m.insert(
        "path".to_string(),
        TemplateValue::String(host_relative_qmd(&item.source_path, host_dir)),
    );
    m.insert(
        "outputHref".to_string(),
        TemplateValue::String(item.output_href.clone()),
    );
    m.insert(
        "filename".to_string(),
        TemplateValue::String(
            item.source_path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string(),
        ),
    );

    // Pre-rendered helper strings.
    let img_html = helpers::image_html(item, listing);
    m.insert("image-html".to_string(), TemplateValue::String(img_html));
    m.insert(
        "metadata-attrs".to_string(),
        TemplateValue::String(helpers::metadata_attrs(item, index)),
    );
    // Description envelope (L7 plan §"How the begin / end markers
    // reach the templates"). Both keys are always populated;
    // `$description-placeholder-begin$` and `-end$` flank the L1
    // fallback `$description$` block in the templates so the L7
    // post-render step has a region to substitute.
    m.insert(
        "description-placeholder-begin".to_string(),
        TemplateValue::String(helpers::description_placeholder_begin(item, listing)),
    );
    m.insert(
        "description-placeholder-end".to_string(),
        TemplateValue::String(helpers::description_placeholder_end()),
    );
    // Image envelope. Always populated regardless of whether
    // `item.image` is set: the templates only reference these keys
    // inside the `$if(image-html)$ ... $else$ ... $endif$` block, so
    // markers for image-present items never reach the rendered
    // output. Keeping the binding unconditional keeps the template
    // logic simple — the decision lives in `$if(image-html)$`, not
    // in the binding.
    m.insert(
        "image-placeholder-begin".to_string(),
        TemplateValue::String(helpers::image_placeholder_begin(item, listing, index)),
    );
    m.insert(
        "image-placeholder-end".to_string(),
        TemplateValue::String(helpers::image_placeholder_end()),
    );
    m.insert(
        "category-html".to_string(),
        TemplateValue::String(helpers::category_html(item)),
    );

    // Free-form author fields.
    if !item.extra.is_empty() {
        let mut extra = HashMap::new();
        for (k, v) in &item.extra {
            extra.insert(k.clone(), config_value_to_template_value(v));
        }
        m.insert("extra".to_string(), TemplateValue::Map(extra));
    }

    // Per-item show flags computed from listing.fields. Templates
    // that want to know "did the listing config say to show field
    // X?" read `$item.show.<field>$` (Q1 semantics).
    let mut show = HashMap::new();
    for field in &listing.fields {
        show.insert(field.clone(), TemplateValue::Bool(true));
    }
    m.insert("show".to_string(), TemplateValue::Map(show));

    TemplateValue::Map(m)
}

/// Compute a host-page-relative forward-slash path string for
/// the item's source file. Empty `host_dir` means the host is at
/// the project root; otherwise we strip the host-dir prefix when
/// the item is inside it. Items outside the host's directory
/// stay project-relative — the `LinkRewriteTransform` resolver
/// handles either form, but the host-relative form keeps the
/// emitted markdown closer to what an author would write by hand.
fn host_relative_qmd(source_path: &std::path::Path, host_dir: &str) -> String {
    let project_relative = source_path
        .components()
        .filter_map(|c| match c {
            std::path::Component::Normal(os) => os.to_str().map(str::to_string),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/");
    if host_dir.is_empty() {
        return project_relative;
    }
    let prefix = format!("{}/", host_dir);
    project_relative
        .strip_prefix(&prefix)
        .map(str::to_string)
        .unwrap_or(project_relative)
}

fn build_project_map(meta: &ConfigValue) -> TemplateValue {
    let mut m = HashMap::new();
    if let Some(url) = meta
        .get_path(&["website", "site-url"])
        .and_then(|v| v.as_plain_text())
    {
        m.insert("site-url".to_string(), TemplateValue::String(url));
    }
    if let Some(title) = meta
        .get_path(&["website", "title"])
        .and_then(|v| v.as_plain_text())
    {
        m.insert("title".to_string(), TemplateValue::String(title));
    }
    TemplateValue::Map(m)
}

/// Lightweight `ConfigValue → TemplateValue` for the binding's
/// `extra` and `template-params` slots. Forwards to pampa's
/// authoritative bridge so PandocInlines/Blocks render correctly
/// for HTML.
fn config_value_to_template_value(cv: &ConfigValue) -> TemplateValue {
    use pampa::template::config_merge::{ConfigConversionContext, config_to_template_value};
    use pampa::template::context::MetaWriter;
    let mut conv = ConfigConversionContext::new(MetaWriter::Html);
    config_to_template_value(cv, &mut conv)
}

fn listing_type_name(t: ListingType) -> &'static str {
    match t {
        ListingType::Default => "default",
        ListingType::Grid => "grid",
        ListingType::Table => "table",
        ListingType::Custom => "custom",
    }
}

fn image_align_name(a: ImageAlign) -> &'static str {
    match a {
        ImageAlign::Left => "left",
        ImageAlign::Right => "right",
    }
}

fn grid_item_align_name(a: GridItemAlign) -> &'static str {
    match a {
        GridItemAlign::Left => "left",
        GridItemAlign::Right => "right",
        GridItemAlign::Center => "center",
    }
}

fn categories_mode_name(m: ListingCategoriesMode) -> &'static str {
    match m {
        ListingCategoriesMode::Disabled => "",
        ListingCategoriesMode::Default => "default",
        ListingCategoriesMode::Unnumbered => "unnumbered",
        ListingCategoriesMode::Cloud => "cloud",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::listing::config::apply_type_defaults;
    use crate::project::listing::config::{Listing, ListingType};
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    fn item(title: &str) -> ListingItem {
        ListingItem {
            title: title.to_string(),
            subtitle: None,
            description: Some("A descr.".to_string()),
            author: Some("Jane".to_string()),
            authors: vec!["Jane".to_string()],
            date: Some("2026-01-01".to_string()),
            date_modified: None,
            categories: vec![],
            image: None,
            image_alt: None,
            image_lazy_loading: None,
            reading_time_minutes: Some(5),
            word_count: None,
            source_path: PathBuf::from("posts/foo.qmd"),
            output_href: "posts/foo.html".to_string(),
            extra: BTreeMap::new(),
        }
    }

    fn listing() -> Listing {
        let mut l = Listing {
            id: "main".to_string(),
            kind: ListingType::Default,
            ..Listing::default()
        };
        apply_type_defaults(&mut l);
        l
    }

    #[test]
    fn binding_includes_listing_id_and_items_array() {
        let ctx = build_listing_context(
            &listing(),
            &[item("A"), item("B")],
            "posts",
            &ConfigValue::default(),
        );
        let listing_map = ctx.get("listing").unwrap();
        assert_eq!(
            listing_map.get_path(&["id"]),
            Some(&TemplateValue::String("main".to_string()))
        );
        let items = ctx.get("items").unwrap();
        if let TemplateValue::List(arr) = items {
            assert_eq!(arr.len(), 2);
        } else {
            panic!("expected items list, got {:?}", items);
        }
    }

    #[test]
    fn item_binding_has_curated_fields() {
        let ctx = build_listing_context(
            &listing(),
            &[item("Hello")],
            "posts",
            &ConfigValue::default(),
        );
        let items = ctx.get("items").unwrap();
        let TemplateValue::List(arr) = items else {
            panic!("items not a list");
        };
        let TemplateValue::Map(m) = &arr[0] else {
            panic!("item not a map");
        };
        assert_eq!(
            m.get("title"),
            Some(&TemplateValue::String("Hello".to_string()))
        );
        assert_eq!(
            m.get("date"),
            Some(&TemplateValue::String("2026-01-01".to_string()))
        );
        assert_eq!(
            m.get("description"),
            Some(&TemplateValue::String("A descr.".to_string()))
        );
        assert!(m.contains_key("path"));
        assert!(m.contains_key("outputHref"));
        // Description envelope keys (L7 plan Phase 2 #7).
        assert!(m.contains_key("description-placeholder-begin"));
        assert!(m.contains_key("description-placeholder-end"));
        // Image envelope keys (always populated; L7 plan Phase 2 #8/#9).
        assert!(m.contains_key("image-placeholder-begin"));
        assert!(m.contains_key("image-placeholder-end"));
        // image-html is empty when no image
        assert_eq!(
            m.get("image-html"),
            Some(&TemplateValue::String(String::new()))
        );
    }

    // L7 plan §"Tests" Phase 2 #7
    #[test]
    fn binding_emits_description_begin_end_pair() {
        let ctx = build_listing_context(
            &listing(),
            &[item("Hello")],
            "posts",
            &ConfigValue::default(),
        );
        let TemplateValue::List(arr) = ctx.get("items").unwrap() else {
            panic!()
        };
        let TemplateValue::Map(m) = &arr[0] else {
            panic!()
        };
        let TemplateValue::String(begin) = m
            .get("description-placeholder-begin")
            .expect("description-placeholder-begin present")
        else {
            panic!("description-placeholder-begin not a string");
        };
        let TemplateValue::String(end) = m
            .get("description-placeholder-end")
            .expect("description-placeholder-end present")
        else {
            panic!("description-placeholder-end not a string");
        };
        assert!(
            begin.starts_with("<!-- desc-begin(5A0113B34292)["),
            "begin marker shape: {begin}"
        );
        assert_eq!(end, "<!-- desc-end(5A0113B34292) -->");
    }

    // L7 plan §"Tests" Phase 2 #8
    #[test]
    fn binding_emits_image_placeholder_begin_end_when_no_image() {
        let mut i = item("X");
        i.image = None;
        let ctx = build_listing_context(&listing(), &[i], "posts", &ConfigValue::default());
        let TemplateValue::List(arr) = ctx.get("items").unwrap() else {
            panic!()
        };
        let TemplateValue::Map(m) = &arr[0] else {
            panic!()
        };
        let TemplateValue::String(begin) = m
            .get("image-placeholder-begin")
            .expect("image-placeholder-begin present")
        else {
            panic!("image-placeholder-begin not a string");
        };
        let TemplateValue::String(end) = m
            .get("image-placeholder-end")
            .expect("image-placeholder-end present")
        else {
            panic!("image-placeholder-end not a string");
        };
        assert!(
            begin.starts_with("<!-- img-begin(9CEB782EFEE6)["),
            "begin marker shape: {begin}"
        );
        assert!(
            !begin.is_empty(),
            "begin marker must be non-empty when no image"
        );
        assert_eq!(end, "<!-- img-end(9CEB782EFEE6) -->");
    }

    // L7 plan §"Tests" Phase 2 #9 — clarified per the implementation
    // notes: we always populate the envelope markers, regardless of
    // `item.image`. The template's `$if(image-html)$ ... $else$ ...`
    // controls visibility, not the binding.
    #[test]
    fn binding_image_placeholder_present_even_when_image_set() {
        let mut i = item("X");
        i.image = Some("static.png".to_string());
        let ctx = build_listing_context(&listing(), &[i], "posts", &ConfigValue::default());
        let TemplateValue::List(arr) = ctx.get("items").unwrap() else {
            panic!()
        };
        let TemplateValue::Map(m) = &arr[0] else {
            panic!()
        };
        // Both keys exist and are non-empty even when image is set —
        // they're inert because the template's $else$ branch never
        // fires for image-present items.
        assert!(m.contains_key("image-placeholder-begin"));
        assert!(m.contains_key("image-placeholder-end"));
    }

    // L7 plan §"Tests" Phase 2 #13
    #[test]
    fn binding_image_placeholder_default_url_b64_encoded_into_marker() {
        use base64::Engine;
        use base64::engine::general_purpose::URL_SAFE_NO_PAD;
        let mut l = listing();
        l.image_placeholder = Some("assets/default.png".to_string());
        let mut i = item("X");
        i.image = None;
        let ctx = build_listing_context(&l, &[i], "posts", &ConfigValue::default());
        let TemplateValue::List(arr) = ctx.get("items").unwrap() else {
            panic!()
        };
        let TemplateValue::Map(m) = &arr[0] else {
            panic!()
        };
        let TemplateValue::String(begin) = m.get("image-placeholder-begin").unwrap() else {
            panic!()
        };
        let expected_b64 = URL_SAFE_NO_PAD.encode("assets/default.png".as_bytes());
        assert!(
            begin.contains(&format!(":{} -->", expected_b64)),
            "marker should carry b64-encoded default URL `{}`; got: {}",
            expected_b64,
            begin
        );
    }

    #[test]
    fn item_binding_omits_unset_optionals() {
        let mut i = item("X");
        i.subtitle = None;
        i.image = None;
        let ctx = build_listing_context(&listing(), &[i], "posts", &ConfigValue::default());
        let TemplateValue::List(arr) = ctx.get("items").unwrap() else {
            panic!()
        };
        let TemplateValue::Map(m) = &arr[0] else {
            panic!()
        };
        assert!(!m.contains_key("subtitle"));
        // `image` field absent (no image), but `image-html` is
        // always present (as empty string) so $if($image-html)$
        // skips cleanly.
        assert!(!m.contains_key("image"));
        assert_eq!(
            m.get("image-html"),
            Some(&TemplateValue::String(String::new()))
        );
    }

    #[test]
    fn item_binding_extra_passes_through_via_pampa_bridge() {
        use quarto_pandoc_types::ConfigValue;
        use quarto_source_map::SourceInfo;

        let mut i = item("X");
        i.extra.insert(
            "status".to_string(),
            ConfigValue::new_string("draft", SourceInfo::default()),
        );
        let ctx = build_listing_context(&listing(), &[i], "posts", &ConfigValue::default());
        let TemplateValue::List(arr) = ctx.get("items").unwrap() else {
            panic!()
        };
        let TemplateValue::Map(m) = &arr[0] else {
            panic!()
        };
        let extra = m.get("extra").expect("extra present");
        let TemplateValue::Map(extra_map) = extra else {
            panic!("extra not a map: {:?}", extra)
        };
        assert_eq!(
            extra_map.get("status"),
            Some(&TemplateValue::String("draft".to_string()))
        );
    }

    // L5 phase 2: per-item binding picks up `category-html`.
    #[test]
    fn item_binding_category_html_present_with_categories() {
        let mut i = item("X");
        i.categories = vec!["rust".to_string(), "design".to_string()];
        let ctx = build_listing_context(&listing(), &[i], "posts", &ConfigValue::default());
        let TemplateValue::List(arr) = ctx.get("items").unwrap() else {
            panic!()
        };
        let TemplateValue::Map(m) = &arr[0] else {
            panic!()
        };
        let TemplateValue::String(html) = m
            .get("category-html")
            .expect("category-html present on per-item map")
        else {
            panic!("category-html not a string");
        };
        // Two chips, b64 onclick args, plain category text.
        assert_eq!(
            html.matches(r#"<div class="listing-category""#).count(),
            2,
            "expected two chips, got: {html}"
        );
        assert!(html.contains(">rust<"));
        assert!(html.contains(">design<"));
    }

    #[test]
    fn item_binding_category_html_empty_when_no_categories() {
        // Default `item()` fixture has empty categories.
        let ctx = build_listing_context(&listing(), &[item("X")], "posts", &ConfigValue::default());
        let TemplateValue::List(arr) = ctx.get("items").unwrap() else {
            panic!()
        };
        let TemplateValue::Map(m) = &arr[0] else {
            panic!()
        };
        // Always present (so $if(category-html)$ never sees "undefined"),
        // but empty so the template skips the surrounding markup.
        assert_eq!(
            m.get("category-html"),
            Some(&TemplateValue::String(String::new()))
        );
    }

    #[test]
    fn project_binding_pulls_website_keys_when_present() {
        use quarto_pandoc_types::ConfigMapEntry;
        use quarto_pandoc_types::ConfigValue;
        use quarto_source_map::SourceInfo;
        let website = ConfigValue::new_map(
            vec![
                ConfigMapEntry {
                    key: "site-url".to_string(),
                    key_source: SourceInfo::default(),
                    value: ConfigValue::new_string("https://example.com", SourceInfo::default()),
                },
                ConfigMapEntry {
                    key: "title".to_string(),
                    key_source: SourceInfo::default(),
                    value: ConfigValue::new_string("My Site", SourceInfo::default()),
                },
            ],
            SourceInfo::default(),
        );
        let meta = ConfigValue::new_map(
            vec![ConfigMapEntry {
                key: "website".to_string(),
                key_source: SourceInfo::default(),
                value: website,
            }],
            SourceInfo::default(),
        );
        let ctx = build_listing_context(&listing(), &[item("A")], "posts", &meta);
        let TemplateValue::Map(p) = ctx.get("project").unwrap() else {
            panic!()
        };
        assert_eq!(
            p.get("site-url"),
            Some(&TemplateValue::String("https://example.com".to_string()))
        );
        assert_eq!(
            p.get("title"),
            Some(&TemplateValue::String("My Site".to_string()))
        );
    }
}
