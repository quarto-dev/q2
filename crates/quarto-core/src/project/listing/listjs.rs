/*
 * project/listing/listjs.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! The client-side list bootstrap for built-in listings (bd-nbv80e33).
//!
//! Q1 makes every listing interactive in the browser: an inline script
//! per listing creates a [List.js](https://listjs.com) instance over the
//! listing's `.list` element and registers it in
//! `window['quarto-listings']`, where the vendored `quarto-listing.js`
//! drives pagination, category filtering, progressive images and the
//! "No matching items" placeholder from it
//! (`website-listing-template.ts`, `templateJsScript` and
//! `_pagination.ejs.md`). This module produces the same two pieces of
//! markup: the init script and the pagination `<nav>` List.js fills.
//!
//! What List.js reads back from the page is set up elsewhere: the
//! container id is [`Listing::container_id`], and the per-item
//! `data-*` attributes come from [`super::helpers::metadata_attrs`].
//!
//! Only the built-in `default` and `grid` layouts get the bootstrap —
//! see [`supports_listjs`].

use serde_json::{Value, json};

use super::config::{ColumnType, Listing, ListingCategoriesMode, ListingType};
use super::helpers::{escape_attr, sort_attr_name};

/// Whether a listing of this type gets the List.js bootstrap.
///
/// - `default` / `grid`: yes — their `.list` children are the items.
/// - `table`: not yet. The table layout is a markdown pipe table with
///   no `.list` element for List.js to attach to; interactive tables
///   are bd-bl1e00r6.
/// - `custom`: no, deliberately. A custom template owns its markup,
///   and a script rearranging that DOM would compete with anything
///   else driving it (Q1 attaches whenever the template happens to
///   contain a `.list`).
pub fn supports_listjs(kind: ListingType) -> bool {
    matches!(kind, ListingType::Default | ListingType::Grid)
}

/// The List.js options object for `listing` — Q1's `templateJsScript`:
///
/// - `valueNames`: what List.js reads from each item — the
///   `.listing-<field>` element of every field, the `data-index` and
///   `data-categories` attributes, and a `data-listing-<field>-sort`
///   attribute for each `field-sort` entry that sorts by a typed value
///   (see [`uses_sort_target`]);
/// - `page` / `pagination`: only when the items don't fit on one page;
/// - `searchColumns`: the value names a filter query searches —
///   `field-filter:`, defaulting to every field.
///
/// `fields` is the listing's effective field set (see
/// [`super::binding::effective_fields`]); `item_count` the number of
/// items rendered.
pub fn list_options(listing: &Listing, fields: &[String], item_count: usize) -> Value {
    let fields = value_fields(listing, fields);

    let mut value_names: Vec<Value> = fields.iter().map(|f| json!(value_name(f))).collect();
    value_names.push(json!({ "data": ["index"] }));
    value_names.push(json!({ "data": ["categories"] }));
    for field in &listing.field_sort {
        if uses_sort_target(listing, field) {
            value_names.push(json!({ "data": [sort_attr_name(field)] }));
        }
    }

    let filter_fields = if listing.field_filter.is_empty() {
        &fields
    } else {
        &listing.field_filter
    };
    let search_columns: Vec<String> = filter_fields.iter().map(|f| value_name(f)).collect();

    let mut options = serde_json::Map::new();
    options.insert("valueNames".to_string(), Value::Array(value_names));
    let page_size = effective_page_size(listing);
    if item_count > page_size {
        options.insert("page".to_string(), json!(page_size));
        options.insert(
            "pagination".to_string(),
            json!({ "item": "<li class='page-item'><a class='page page-link' href='#'></a></li>" }),
        );
    }
    options.insert("searchColumns".to_string(), json!(search_columns));
    Value::Object(options)
}

/// The `<script>` that creates the listing's List.js instance once
/// the page has loaded and hands it to `quarto-listing.js` — Q1's
/// `templateJsScript`, which this follows line for line, except that
/// it finds the container with `getElementById` rather than a CSS
/// selector built from the id.
///
/// Author text (the listing id, field names) only reaches the script
/// as JSON with `<` escaped, so it can neither leave a string literal
/// nor close the `<script>` element.
pub fn init_script(listing: &Listing, fields: &[String], item_count: usize) -> String {
    let id = script_json(&json!(listing.container_id()));
    let options = script_json(&list_options(listing, fields, item_count));
    format!(
        r#"<script>
window.document.addEventListener("DOMContentLoaded", function (_event) {{
  const listingEl = window.document.getElementById({id});
  if (!listingEl || !listingEl.querySelector(".list")) {{
    // No listing discovered, do not attach.
    return;
  }}
  const options = {options};
  window['quarto-listings'] = window['quarto-listings'] || {{}};
  window['quarto-listings'][{id}] = new List({id}, options);
  if (window['quarto-listing-loaded']) {{
    window['quarto-listing-loaded']();
  }}
}});
window.addEventListener('hashchange', () => {{
  if (window['quarto-listing-loaded']) {{
    window['quarto-listing-loaded']();
  }}
}});
</script>"#
    )
}

/// The `<nav>` List.js renders page links into — Q1's
/// `_pagination.ejs.md`. `None` when everything fits on one page.
/// List.js finds it as the `.pagination` inside the listing container,
/// so it must be placed there.
pub fn pagination_nav(listing: &Listing, item_count: usize) -> Option<String> {
    if item_count <= effective_page_size(listing) {
        return None;
    }
    Some(format!(
        "<nav id=\"{}-pagination\" class=\"listing-pagination\" \
         aria-label=\"Page Navigation\">\n  <ul class=\"pagination\"></ul>\n</nav>",
        escape_attr(&listing.id)
    ))
}

/// Items per page as the browser sees it. Q1's script reads the page
/// size as `listing['page-size'] || 50`, so an author's `page-size: 0`
/// means 50.
fn effective_page_size(listing: &Listing) -> usize {
    match listing.page_size() {
        0 => 50,
        n => n as usize,
    }
}

/// The fields List.js reads: the listing's fields, plus `categories`
/// for a listing with categories on (Q1 adds it to the hydrated fields
/// of default and grid listings, which are the only ones bootstrapped
/// here).
fn value_fields(listing: &Listing, fields: &[String]) -> Vec<String> {
    let mut out = fields.to_vec();
    if listing.categories != ListingCategoriesMode::Disabled
        && !out.iter().any(|f| f == "categories")
    {
        out.push("categories".to_string());
    }
    out
}

/// The class of the element holding `field`'s value in an item — the
/// built-in templates' `.listing-<field>`.
fn value_name(field: &str) -> String {
    format!("listing-{field}")
}

/// Q1's `useSortTarget`: a field sorts by its `data-listing-<field>-sort`
/// value rather than its displayed text when it is linked (the text is
/// wrapped in an anchor) or typed as a date or number (the text is
/// formatted for reading).
fn uses_sort_target(listing: &Listing, field: &str) -> bool {
    let linked = listing
        .field_links
        .as_deref()
        .unwrap_or_default()
        .iter()
        .any(|f| f == field);
    let typed = matches!(
        listing.field_types.get(field),
        Some(ColumnType::Date | ColumnType::Number | ColumnType::Minutes)
    );
    linked || typed
}

/// Serialize `value` for embedding in an inline `<script>`: JSON is a
/// JavaScript expression, and escaping `<` keeps a `</script>` (or
/// `<!--`) inside a string from ending the element.
fn script_json(value: &Value) -> String {
    value.to_string().replace('<', "\\u003c")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::listing::config::apply_type_defaults;

    fn listing(kind: ListingType) -> Listing {
        let mut l = Listing {
            id: "posts".to_string(),
            kind,
            ..Listing::default()
        };
        apply_type_defaults(&mut l);
        l
    }

    fn strings(v: &Value) -> Vec<String> {
        v.as_array()
            .unwrap_or_else(|| panic!("not an array: {v}"))
            .iter()
            .map(|x| x.to_string())
            .collect()
    }

    fn fields(names: &[&str]) -> Vec<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn supports_default_and_grid_only() {
        assert!(supports_listjs(ListingType::Default));
        assert!(supports_listjs(ListingType::Grid));
        assert!(!supports_listjs(ListingType::Table));
        assert!(!supports_listjs(ListingType::Custom));
    }

    // Q1: `listing-<field>` per field, then the index and categories
    // data attrs, then a data sort target per sortable `field-sort`
    // entry that is typed (date / number / minutes) or linked.
    #[test]
    fn value_names_follow_q1_order() {
        let l = listing(ListingType::Default);
        let opts = list_options(&l, &fields(&["date", "title", "author"]), 3);
        assert_eq!(
            strings(&opts["valueNames"]),
            vec![
                r#""listing-date""#,
                r#""listing-title""#,
                r#""listing-author""#,
                r#"{"data":["index"]}"#,
                r#"{"data":["categories"]}"#,
                // field-sort default: title, date, author, filename,
                // file-modified — of which date and file-modified are
                // date-typed.
                r#"{"data":["listing-date-sort"]}"#,
                r#"{"data":["listing-file-modified-sort"]}"#,
            ]
        );
    }

    #[test]
    fn linked_sort_fields_get_a_data_sort_target() {
        let mut l = listing(ListingType::Grid);
        l.field_links = Some(vec!["title".to_string()]);
        l.field_sort = vec!["title".to_string()];
        let opts = list_options(&l, &fields(&["title"]), 1);
        assert!(
            strings(&opts["valueNames"])
                .contains(&r#"{"data":["listing-title-sort"]}"#.to_string()),
            "{opts}"
        );
    }

    #[test]
    fn untyped_unlinked_sort_fields_get_no_data_sort_target() {
        let mut l = listing(ListingType::Default);
        l.field_sort = vec!["author".to_string()];
        l.field_types
            .insert("author".to_string(), ColumnType::String);
        let opts = list_options(&l, &fields(&["author"]), 1);
        assert!(!opts.to_string().contains("-sort"), "{opts}");
    }

    // Q1's `kFieldFilter` defaults to the listing's fields.
    #[test]
    fn search_columns_default_to_the_fields() {
        let l = listing(ListingType::Default);
        let opts = list_options(&l, &fields(&["title", "author"]), 1);
        assert_eq!(
            strings(&opts["searchColumns"]),
            vec![r#""listing-title""#, r#""listing-author""#]
        );
    }

    #[test]
    fn search_columns_follow_author_field_filter() {
        let mut l = listing(ListingType::Default);
        l.field_filter = vec!["title".to_string()];
        let opts = list_options(&l, &fields(&["title", "author"]), 1);
        assert_eq!(strings(&opts["searchColumns"]), vec![r#""listing-title""#]);
    }

    // Q1 adds `categories` to a default/grid listing's fields when
    // categories are on, even if the presence-filtered set dropped it.
    #[test]
    fn categories_join_the_fields_when_enabled() {
        let mut l = listing(ListingType::Default);
        l.categories = ListingCategoriesMode::Default;
        let opts = list_options(&l, &fields(&["title"]), 1);
        assert!(
            strings(&opts["valueNames"]).contains(&r#""listing-categories""#.to_string()),
            "{opts}"
        );
        assert!(
            strings(&opts["searchColumns"]).contains(&r#""listing-categories""#.to_string()),
            "{opts}"
        );
    }

    #[test]
    fn pagination_options_only_when_items_exceed_page_size() {
        let mut l = listing(ListingType::Default);
        l.page_size = Some(2);
        let opts = list_options(&l, &fields(&["title"]), 2);
        assert!(opts.get("page").is_none(), "{opts}");
        assert!(opts.get("pagination").is_none(), "{opts}");

        let opts = list_options(&l, &fields(&["title"]), 3);
        assert_eq!(opts["page"], json!(2));
        assert_eq!(
            opts["pagination"]["item"],
            json!("<li class='page-item'><a class='page page-link' href='#'></a></li>")
        );
    }

    // Q1 reads `page-size: 0` as its fallback of 50 (`|| 50`).
    #[test]
    fn page_size_zero_falls_back_to_fifty() {
        let mut l = listing(ListingType::Default);
        l.page_size = Some(0);
        let opts = list_options(&l, &fields(&["title"]), 51);
        assert_eq!(opts["page"], json!(50));
        assert!(pagination_nav(&l, 50).is_none());
        assert!(pagination_nav(&l, 51).is_some());
    }

    #[test]
    fn init_script_creates_and_registers_the_list() {
        let l = listing(ListingType::Default);
        let script = init_script(&l, &fields(&["title"]), 1);
        assert!(script.starts_with("<script>"), "{script}");
        assert!(script.trim_end().ends_with("</script>"), "{script}");
        assert!(
            script.contains(r#"new List("listing-posts", options)"#),
            "{script}"
        );
        assert!(
            script.contains(r#"window['quarto-listings']["listing-posts"]"#),
            "{script}"
        );
        assert!(
            script.contains("window['quarto-listing-loaded']()"),
            "{script}"
        );
        assert!(script.contains("DOMContentLoaded"), "{script}");
        assert!(script.contains("hashchange"), "{script}");
        assert!(script.contains(r#""valueNames""#), "{script}");
    }

    // Ids and field names are author text: they must not be able to
    // end the script element or break out of a JS string.
    #[test]
    fn init_script_escapes_author_text() {
        let mut l = listing(ListingType::Default);
        l.id = r#"a"</script><script>alert(1)//"#.to_string();
        let script = init_script(&l, &fields(&["</script>"]), 1);
        assert_eq!(script.matches("</script>").count(), 1, "{script}");
        assert_eq!(script.matches("<script>").count(), 1, "{script}");
        assert!(!script.contains(r#"a"</"#), "{script}");
    }

    #[test]
    fn pagination_nav_matches_q1_markup() {
        let mut l = listing(ListingType::Default);
        l.page_size = Some(2);
        assert!(pagination_nav(&l, 2).is_none());
        let nav = pagination_nav(&l, 3).expect("nav when items exceed page size");
        assert_eq!(
            nav,
            "<nav id=\"posts-pagination\" class=\"listing-pagination\" \
             aria-label=\"Page Navigation\">\n  <ul class=\"pagination\"></ul>\n</nav>"
        );
    }

    #[test]
    fn pagination_nav_escapes_the_id() {
        let mut l = listing(ListingType::Default);
        l.page_size = Some(1);
        l.id = r#"x"><b"#.to_string();
        let nav = pagination_nav(&l, 2).unwrap();
        assert!(nav.contains(r#"id="x&quot;&gt;&lt;b-pagination""#), "{nav}");
    }
}
