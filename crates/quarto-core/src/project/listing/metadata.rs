/*
 * project/listing/metadata.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! The `meta.listings.<id>` bridge (bd-2oqen84t, M1).
//!
//! Every listing a host page declares is resolved to a
//! [`ResolvedListing`] and, until this module, lived only in the typed
//! `RenderContext::resolved_listings` field that the render transform,
//! the categories sidebar and the feed read. Nothing Lua-visible could
//! see the items (the L3 plan's D2 was revised to the typed field
//! because no Lua slot consumed `meta.listings.<id>` at the time; see
//! bd-0fd0). This module publishes each resolved listing into the
//! document's metadata, so a user filter (`Meta` handler, then build
//! the listing in `Div` or wherever) and an lmd cell
//! (`meta.listings.posts.items:map(...)`) both get the items.
//!
//! # The contract
//!
//! One map per listing at `listings.<id>`, with the **template
//! binding's vocabulary** (`binding.rs`, the L8 public contract for
//! custom templates): the same key names, the same values, in their
//! native `ConfigValue` kinds rather than pre-rendered strings.
//!
//! - Prose (`title`, `subtitle`, `description`) is `PandocInlines`,
//!   prepared as the binding prepares it (links and notes dropped, one
//!   line; the description truncated at the listing's
//!   `max-description-length`). A Lua consumer gets `Inlines`.
//! - Paths and other literal strings (`path`, `image`, `author`,
//!   `date`, `filename`, …) are `Scalar` strings, so Lua gets strings
//!   usable as link targets with no `stringify`.
//! - Counts (`order`, `word-count`, `reading-time-minutes`) are
//!   `Scalar` integers, so Lua gets numbers.
//! - Lists (`authors`, `categories`) are always present, empty when the
//!   item has none, so `#it.categories` and `ipairs` never need a
//!   guard. The binding omits empty lists instead (so `$if$` works);
//!   this is the one presence rule the two contracts do not share.
//!   Optional scalars and prose (`subtitle`, `date`, `image`, `path`,
//!   …) are absent keys when the item has no value, as in the binding.
//! - `date` and `date-modified` are the **display** strings the
//!   binding shows (listing `date-format`, then document
//!   `date-format`, then `medium`; decision 2 of the plan); `date-iso`
//!   and `date-modified-iso` carry the stored values for sorting and
//!   grouping.
//!
//! Three kinds of binding key are deliberately **not** published:
//! the `-html` twins (`title-html`, `image-html`, `category-html`, …;
//! a cell has `pandoc.write`), the artifacts of the logic-less
//! template (`metadata-attrs`, `table-row`, `show.*`,
//! `show-description`), and the four L7 placeholder markers
//! (decision 3: the L1 fallback is what a Lua listing gets in v1). Two
//! keys are renamed to the metadata's kebab-case: `outputHref` is
//! `output-href`. Custom fields are published under `extra` only, not
//! flattened into the item map as the binding does, so a custom field
//! can never shadow a curated one in Lua. Three keys exist here and
//! not in the binding: `description-source` and `image-source` (which
//! step of the hydration chain the value came from, so a cell can
//! tell authored from derived), and `origin` (document, record, or a
//! record laid over a document). `tests::key_parity_with_the_template_binding`
//! pins all of this against the binding itself.
//!
//! Published for **every** listing type (decision 5), not only
//! `type: metadata`: the read half of bd-0fd0 for free, and one data
//! contract. Whether the render transform should read a mutated
//! `meta.listings.<id>` back is bd-0fd0's question; this module writes
//! one key in the binding's vocabulary and nothing else, so it does
//! not foreclose that.

use quarto_pandoc_types::inline::Inlines;
use quarto_pandoc_types::{ConfigMapEntry, ConfigValue};
use quarto_source_map::{By, SourceInfo};
use yaml_rust2::Yaml;

use super::ResolvedListing;
use super::binding::{
    display_date, effective_date_style, effective_fields, host_relative_qmd, prose_inlines,
};
use super::config::{Listing, ListingCategoriesMode, SortDirection};
use super::helpers::truncate_inlines_at_space;
use super::item::{FieldSource, ItemOrigin, ItemTarget, ListingItem};
use crate::dates::DateStyle;

/// The top-level metadata key the bridge writes under.
pub const LISTINGS_KEY: &str = "listings";

/// Publish every resolved listing into `meta` at
/// `listings.<id>`. `host_dir` is the host page's project-relative
/// directory (forward-slash, no trailing slash, empty at the root),
/// the same value the render transform passes to the binding, so
/// `path` comes out host-directory-relative in both.
///
/// An existing `listings.<id>` entry is replaced: the resolved listing
/// is the source of truth for the page being rendered.
pub fn publish_listings(meta: &mut ConfigValue, resolved: &[ResolvedListing], host_dir: &str) {
    // Build first, insert after: the date style reads `meta`.
    let values: Vec<(String, ConfigValue)> = resolved
        .iter()
        .map(|r| {
            (
                r.listing.id.clone(),
                listing_to_config_value(r, host_dir, meta),
            )
        })
        .collect();
    for (id, value) in values {
        meta.insert_path(&[LISTINGS_KEY, &id], value);
    }
}

/// One resolved listing as the map described in the module docs.
/// `project_meta` is the host page's metadata (for `date-format`).
pub fn listing_to_config_value(
    r: &ResolvedListing,
    host_dir: &str,
    project_meta: &ConfigValue,
) -> ConfigValue {
    let listing = &r.listing;
    let date_style = effective_date_style(listing, project_meta);
    let fields = effective_fields(listing, &r.items);

    let mut m = MapBuilder::new();
    m.string("id", &listing.id);
    m.string("type", listing.kind.name());
    m.strings("fields", &fields);
    if let Some(sort) = &listing.sort {
        m.array(
            "sort",
            sort.iter()
                .map(|s| {
                    let mut e = MapBuilder::new();
                    e.string("field", &s.field);
                    e.string(
                        "direction",
                        match s.direction {
                            SortDirection::Asc => "asc",
                            SortDirection::Desc => "desc",
                        },
                    );
                    e.finish()
                })
                .collect(),
        );
    }
    m.int("page-size", i64::from(listing.page_size()));
    if let Some(max) = listing.max_items {
        m.int("max-items", i64::from(max));
    }
    m.int(
        "max-description-length",
        i64::from(listing.max_description_length),
    );
    if let Some(f) = &listing.date_format {
        m.string("date-format", f);
    }
    if let Some(p) = &listing.image_placeholder {
        m.string("image-placeholder", p);
    }
    m.string(
        "categories",
        match listing.categories {
            ListingCategoriesMode::Disabled => "disabled",
            ListingCategoriesMode::Default => "default",
            ListingCategoriesMode::Unnumbered => "unnumbered",
            ListingCategoriesMode::Cloud => "cloud",
        },
    );
    if !listing.template_params.is_empty() {
        let mut tp = MapBuilder::new();
        for (k, v) in &listing.template_params {
            tp.value(k, v.clone());
        }
        m.value("template-params", tp.finish());
    }
    m.array(
        "items",
        r.items
            .iter()
            .map(|item| item_to_config_value(listing, item, host_dir, &date_style))
            .collect(),
    );
    m.finish()
}

/// One item as the map described in the module docs.
pub fn item_to_config_value(
    listing: &Listing,
    item: &ListingItem,
    host_dir: &str,
    date_style: &DateStyle,
) -> ConfigValue {
    let mut m = MapBuilder::new();

    m.inlines("title", prose_inlines(&item.title));
    if let Some(s) = item.subtitle.as_deref() {
        m.inlines("subtitle", prose_inlines(s));
    }
    if let Some(s) = item.description.as_deref() {
        // The listing's limit, as the binding applies it
        // (bd-pcmdb7qg): the same item may sit in two listings with
        // different limits, so this is per listing, not per item.
        let description = if listing.max_description_length > 0 {
            truncate_inlines_at_space(s, listing.max_description_length as usize)
        } else {
            s.to_vec()
        };
        m.inlines("description", prose_inlines(&description));
    }
    m.string(
        "description-source",
        field_source_name(item.description_source),
    );

    if let Some(s) = item.author.as_deref() {
        m.string("author", s);
    }
    // Lists are always present, empty when the item has none: Lua code
    // iterates them (`#it.categories`, `ipairs`), and a guard on every
    // use is the kind of friction this contract exists to remove. The
    // binding omits them instead so `$if(it.categories)$` works; that
    // is the one place the two contracts differ in presence.
    m.strings("authors", &item.authors);
    if let Some(s) = item.date.as_deref() {
        m.string("date", &display_date(s, date_style));
        m.string("date-iso", s);
    }
    if let Some(s) = item.date_modified.as_deref() {
        m.string("date-modified", &display_date(s, date_style));
        m.string("date-modified-iso", s);
    }
    m.strings("categories", &item.categories);

    if let Some(s) = item.image.as_deref() {
        m.string("image", s);
    }
    if let Some(s) = item.image_alt.as_deref() {
        m.string("image-alt", s);
    }
    m.string("image-source", field_source_name(item.image_source));

    if let Some(n) = item.reading_time_minutes {
        m.string("reading-time", &format!("{} min read", n));
        m.int("reading-time-minutes", i64::from(n));
    }
    if let Some(n) = item.word_count {
        m.int("word-count", i64::from(n));
    }
    if let Some(n) = item.order {
        m.int("order", i64::from(n));
    }

    // Links, as the binding computes them: a document's `path` is the
    // host-directory-relative `.qmd` source path, which
    // `LinkRewriteTransform` rewrites to the output URL when a Lua
    // consumer puts it in a `pandoc.Link`; a record's literal href is
    // passed through; an unlinked record has no `path`.
    let path: Option<String> = match &item.target {
        ItemTarget::Document { source_path, .. } => Some(host_relative_qmd(source_path, host_dir)),
        ItemTarget::Href(href) => Some(href.clone()),
        ItemTarget::None => None,
    };
    if let Some(p) = &path {
        m.string("path", p);
    }
    if let Some(href) = item.target.href() {
        m.string("output-href", href);
    }
    if let Some(name) = item.target.filename() {
        m.string("filename", &name);
    }
    m.string(
        "origin",
        match item.origin {
            ItemOrigin::Document => "document",
            ItemOrigin::Record => "record",
            ItemOrigin::RecordOverDocument => "record-over-document",
        },
    );

    if !item.extra.is_empty() {
        let mut extra = MapBuilder::new();
        for (k, v) in &item.extra {
            extra.value(k, v.clone());
        }
        m.value("extra", extra.finish());
    }

    m.finish()
}

/// The hydration step a description or image came from, as the
/// metadata spells it.
fn field_source_name(source: FieldSource) -> &'static str {
    match source {
        FieldSource::ListingItem => "listing-item",
        FieldSource::Document => "document",
        FieldSource::Abstract => "abstract",
        FieldSource::Record => "record",
        FieldSource::Derived => "derived",
        FieldSource::Absent => "absent",
    }
}

fn generated() -> SourceInfo {
    SourceInfo::generated(By::programmatic_config())
}

/// Ordered map construction with generated provenance on every node.
struct MapBuilder {
    entries: Vec<ConfigMapEntry>,
}

impl MapBuilder {
    fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    fn value(&mut self, key: &str, value: ConfigValue) {
        self.entries.push(ConfigMapEntry {
            key: key.to_string(),
            key_source: generated(),
            value,
        });
    }

    fn string(&mut self, key: &str, s: &str) {
        self.value(key, ConfigValue::new_string(s, generated()));
    }

    fn int(&mut self, key: &str, n: i64) {
        self.value(key, ConfigValue::new_scalar(Yaml::Integer(n), generated()));
    }

    fn inlines(&mut self, key: &str, inlines: Inlines) {
        self.value(key, ConfigValue::new_inlines(inlines, generated()));
    }

    fn strings(&mut self, key: &str, items: &[String]) {
        self.array(
            key,
            items
                .iter()
                .map(|s| ConfigValue::new_string(s, generated()))
                .collect(),
        );
    }

    fn array(&mut self, key: &str, items: Vec<ConfigValue>) {
        self.value(key, ConfigValue::new_array(items, generated()));
    }

    fn finish(self) -> ConfigValue {
        ConfigValue::new_map(self.entries, generated())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::listing::binding::build_listing_context;
    use crate::project::listing::config::{ListingSort, ListingType, apply_type_defaults};
    use quarto_doctemplate::TemplateValue;
    use quarto_pandoc_types::config_value::ConfigValueKind;
    use std::collections::{BTreeMap, BTreeSet};

    fn item(title: &str) -> ListingItem {
        ListingItem {
            description_source: FieldSource::Derived,
            image_source: FieldSource::Absent,
            title: crate::document_profile::text(title),
            subtitle: None,
            description: Some(crate::document_profile::text("A description that goes on.")),
            author: Some("Jane, Joe".to_string()),
            authors: vec!["Jane".to_string(), "Joe".to_string()],
            date: Some("2026-01-15".to_string()),
            date_modified: None,
            categories: vec!["news".to_string()],
            image: None,
            image_alt: None,
            image_lazy_loading: None,
            reading_time_minutes: Some(5),
            word_count: Some(812),
            order: Some(3),
            target: ItemTarget::document("posts/foo.qmd", "posts/foo.html"),
            origin: ItemOrigin::Document,
            extra: BTreeMap::new(),
        }
    }

    fn listing(id: &str) -> Listing {
        let mut l = Listing {
            id: id.to_string(),
            kind: ListingType::Default,
            ..Listing::default()
        };
        apply_type_defaults(&mut l);
        l
    }

    fn resolved(id: &str, items: Vec<ListingItem>) -> ResolvedListing {
        ResolvedListing {
            listing: listing(id),
            items,
        }
    }

    fn parse_inlines(md: &str) -> Inlines {
        let mut sink = std::io::sink();
        let (doc, _, diags) =
            pampa::readers::qmd::read(md.as_bytes(), false, "t.qmd", &mut sink, true, None)
                .unwrap_or_else(|d| panic!("`{md}` does not parse: {d:?}"));
        assert!(
            diags.is_empty(),
            "`{md}` parses with diagnostics: {diags:?}"
        );
        match doc.blocks.as_slice() {
            [quarto_pandoc_types::block::Block::Paragraph(p)] => p.content.clone(),
            other => panic!("`{md}` is not one paragraph: {other:?}"),
        }
    }

    fn empty_meta() -> ConfigValue {
        ConfigValue::new_map(vec![], SourceInfo::for_test())
    }

    fn keys(v: &ConfigValue) -> BTreeSet<String> {
        v.as_map_entries()
            .expect("a map")
            .iter()
            .map(|e| e.key.clone())
            .collect()
    }

    fn first_item(meta: &ConfigValue, id: &str) -> ConfigValue {
        meta.get_path(&[LISTINGS_KEY, id, "items"])
            .and_then(|v| v.as_array())
            .expect("items array")[0]
            .clone()
    }

    #[test]
    fn publishes_one_map_per_listing_under_listings() {
        let mut meta = empty_meta();
        publish_listings(
            &mut meta,
            &[
                resolved("posts", vec![item("A"), item("B")]),
                resolved("talks", vec![]),
            ],
            "",
        );
        assert_eq!(
            keys(meta.get(LISTINGS_KEY).unwrap()),
            ["posts", "talks"].iter().map(|s| s.to_string()).collect()
        );
        assert_eq!(
            meta.get_path(&["listings", "posts", "items"])
                .and_then(|v| v.as_array())
                .map(|a| a.len()),
            Some(2)
        );
        assert_eq!(
            meta.get_path(&["listings", "talks", "items"])
                .and_then(|v| v.as_array())
                .map(|a| a.len()),
            Some(0)
        );
        assert_eq!(
            meta.get_path(&["listings", "posts", "id"])
                .and_then(|v| v.as_str()),
            Some("posts")
        );
        assert_eq!(
            meta.get_path(&["listings", "posts", "type"])
                .and_then(|v| v.as_str()),
            Some("default")
        );
    }

    #[test]
    fn republishing_replaces_the_listing_entry() {
        let mut meta = empty_meta();
        publish_listings(&mut meta, &[resolved("posts", vec![item("A")])], "");
        publish_listings(
            &mut meta,
            &[resolved("posts", vec![item("A"), item("B")])],
            "",
        );
        assert_eq!(
            meta.get_path(&["listings", "posts", "items"])
                .and_then(|v| v.as_array())
                .map(|a| a.len()),
            Some(2)
        );
    }

    #[test]
    fn prose_is_inlines_strings_are_scalars_counts_are_integers() {
        let mut meta = empty_meta();
        publish_listings(
            &mut meta,
            &[resolved("posts", vec![item("Hello *world*")])],
            "",
        );
        let it = first_item(&meta, "posts");

        assert!(
            matches!(
                it.get("title").unwrap().value,
                ConfigValueKind::PandocInlines(_)
            ),
            "title is inlines"
        );
        assert!(
            matches!(
                it.get("description").unwrap().value,
                ConfigValueKind::PandocInlines(_)
            ),
            "description is inlines"
        );
        assert_eq!(it.get("path").unwrap().as_str(), Some("posts/foo.qmd"));
        assert_eq!(
            it.get("output-href").unwrap().as_str(),
            Some("posts/foo.html")
        );
        assert_eq!(it.get("filename").unwrap().as_str(), Some("foo.qmd"));
        assert_eq!(it.get("order").unwrap().as_int(), Some(3));
        assert_eq!(it.get("word-count").unwrap().as_int(), Some(812));
        assert_eq!(it.get("reading-time-minutes").unwrap().as_int(), Some(5));
        assert_eq!(it.get("reading-time").unwrap().as_str(), Some("5 min read"));
        assert_eq!(it.get("author").unwrap().as_str(), Some("Jane, Joe"));
        assert_eq!(
            it.get("authors")
                .unwrap()
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_str().unwrap().to_string())
                .collect::<Vec<_>>(),
            vec!["Jane", "Joe"]
        );
        assert_eq!(
            it.get("categories").unwrap().as_array().unwrap()[0].as_str(),
            Some("news")
        );
        assert_eq!(
            it.get("description-source").unwrap().as_str(),
            Some("derived")
        );
        assert_eq!(it.get("image-source").unwrap().as_str(), Some("absent"));
        assert_eq!(it.get("origin").unwrap().as_str(), Some("document"));
    }

    #[test]
    fn absent_optional_fields_are_absent_keys() {
        let mut it = item("A");
        it.subtitle = None;
        it.description = None;
        it.author = None;
        it.authors = vec![];
        it.date = None;
        it.categories = vec![];
        it.image = None;
        it.reading_time_minutes = None;
        it.word_count = None;
        it.order = None;
        it.target = ItemTarget::None;
        let mut meta = empty_meta();
        publish_listings(&mut meta, &[resolved("posts", vec![it])], "");
        let it = first_item(&meta, "posts");
        for absent in [
            "subtitle",
            "description",
            "author",
            "date",
            "date-iso",
            "image",
            "reading-time",
            "reading-time-minutes",
            "word-count",
            "order",
            "path",
            "output-href",
            "filename",
            "extra",
        ] {
            assert!(it.get(absent).is_none(), "{absent} should be absent");
        }
        // Always present, so a cell can branch on them.
        for present in ["title", "description-source", "image-source", "origin"] {
            assert!(it.get(present).is_some(), "{present} should be present");
        }
        // Lists are present and empty, never absent.
        for list in ["authors", "categories"] {
            assert_eq!(
                it.get(list).and_then(|v| v.as_array()).map(|a| a.len()),
                Some(0),
                "{list} should be an empty list"
            );
        }
    }

    #[test]
    fn date_is_the_display_string_and_date_iso_the_stored_value() {
        let mut meta = empty_meta();
        publish_listings(&mut meta, &[resolved("posts", vec![item("A")])], "");
        let it = first_item(&meta, "posts");
        assert_eq!(it.get("date-iso").unwrap().as_str(), Some("2026-01-15"));
        assert_eq!(it.get("date").unwrap().as_str(), Some("Jan 15, 2026"));
    }

    #[test]
    fn date_style_follows_listing_then_document_date_format() {
        // Listing-level wins.
        let mut r = resolved("posts", vec![item("A")]);
        r.listing.date_format = Some("iso".to_string());
        let mut meta = empty_meta();
        meta.insert_path(
            &["date-format"],
            ConfigValue::new_string("long", SourceInfo::for_test()),
        );
        publish_listings(&mut meta, &[r], "");
        assert_eq!(
            first_item(&meta, "posts").get("date").unwrap().as_str(),
            Some("2026-01-15")
        );
        // Document-level otherwise.
        let mut meta = empty_meta();
        meta.insert_path(
            &["date-format"],
            ConfigValue::new_string("long", SourceInfo::for_test()),
        );
        publish_listings(&mut meta, &[resolved("posts", vec![item("A")])], "");
        assert_eq!(
            first_item(&meta, "posts").get("date").unwrap().as_str(),
            Some("January 15, 2026")
        );
    }

    #[test]
    fn description_is_truncated_at_the_listing_limit() {
        let mut r = resolved("posts", vec![item("A")]);
        r.listing.max_description_length = 10;
        let mut meta = empty_meta();
        publish_listings(&mut meta, &[r], "");
        let it = first_item(&meta, "posts");
        let text = it.get("description").unwrap().as_plain_text().unwrap();
        assert!(
            text.len() <= "A description…".len() && text.starts_with('A'),
            "truncated, got {text:?}"
        );
        // 0 disables.
        let mut r = resolved("posts", vec![item("A")]);
        r.listing.max_description_length = 0;
        let mut meta = empty_meta();
        publish_listings(&mut meta, &[r], "");
        assert_eq!(
            first_item(&meta, "posts")
                .get("description")
                .unwrap()
                .as_plain_text()
                .as_deref(),
            Some("A description that goes on.")
        );
    }

    #[test]
    fn title_drops_links_like_the_binding() {
        let mut it = item("A");
        it.title = parse_inlines("See [the docs](u.html) now");
        let mut meta = empty_meta();
        publish_listings(&mut meta, &[resolved("posts", vec![it])], "");
        let it = first_item(&meta, "posts");
        let title = it.get("title").unwrap();
        assert_eq!(title.as_plain_text().as_deref(), Some("See the docs now"));
        let ConfigValueKind::PandocInlines(inlines) = &title.value else {
            panic!("inlines")
        };
        assert!(
            !inlines
                .iter()
                .any(|i| matches!(i, quarto_pandoc_types::inline::Inline::Link(_))),
            "no Link survives: {inlines:?}"
        );
    }

    #[test]
    fn path_is_host_relative_for_documents_and_literal_for_records() {
        let mut rec = item("Ext");
        rec.target = ItemTarget::Href("https://example.com/x".to_string());
        rec.origin = ItemOrigin::Record;
        let mut meta = empty_meta();
        publish_listings(
            &mut meta,
            &[resolved("posts", vec![item("Doc"), rec])],
            "posts",
        );
        let items = meta
            .get_path(&["listings", "posts", "items"])
            .and_then(|v| v.as_array())
            .unwrap();
        assert_eq!(items[0].get("path").unwrap().as_str(), Some("foo.qmd"));
        assert_eq!(
            items[1].get("path").unwrap().as_str(),
            Some("https://example.com/x")
        );
        assert_eq!(items[1].get("origin").unwrap().as_str(), Some("record"));
    }

    #[test]
    fn listing_map_carries_config_and_effective_fields() {
        let mut r = resolved("posts", vec![item("A")]);
        r.listing.sort = Some(vec![ListingSort {
            field: "date".to_string(),
            direction: SortDirection::Desc,
        }]);
        r.listing.max_items = Some(7);
        r.listing.template_params.insert(
            "columns".to_string(),
            ConfigValue::new_scalar(Yaml::Integer(2), SourceInfo::for_test()),
        );
        let mut meta = empty_meta();
        publish_listings(&mut meta, &[r], "");
        let l = meta.get_path(&["listings", "posts"]).unwrap();
        assert_eq!(l.get("max-items").unwrap().as_int(), Some(7));
        assert_eq!(l.get("page-size").unwrap().as_int(), Some(25));
        assert_eq!(l.get("max-description-length").unwrap().as_int(), Some(175));
        assert_eq!(l.get("categories").unwrap().as_str(), Some("disabled"));
        let sort = l.get("sort").unwrap().as_array().unwrap();
        assert_eq!(sort[0].get("field").unwrap().as_str(), Some("date"));
        assert_eq!(sort[0].get("direction").unwrap().as_str(), Some("desc"));
        assert_eq!(
            l.get_path(&["template-params", "columns"])
                .unwrap()
                .as_int(),
            Some(2)
        );
        // `fields` is the effective set: defaults presence-filtered
        // against the items, as the binding computes it.
        let fields: Vec<String> = l
            .get("fields")
            .unwrap()
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect();
        assert!(fields.contains(&"title".to_string()), "{fields:?}");
        assert!(
            !fields.contains(&"subtitle".to_string()),
            "no item has a subtitle: {fields:?}"
        );
    }

    #[test]
    fn custom_fields_live_under_extra_only() {
        let mut it = item("A");
        it.extra.insert(
            "status".to_string(),
            ConfigValue::new_string("stable", SourceInfo::for_test()),
        );
        // A custom field that collides with a curated name must not
        // shadow it.
        it.extra.insert(
            "title".to_string(),
            ConfigValue::new_string("not the title", SourceInfo::for_test()),
        );
        let mut meta = empty_meta();
        publish_listings(&mut meta, &[resolved("posts", vec![it])], "");
        let it = first_item(&meta, "posts");
        assert_eq!(
            it.get_path(&["extra", "status"]).unwrap().as_str(),
            Some("stable")
        );
        assert_eq!(
            it.get("title").unwrap().as_plain_text().as_deref(),
            Some("A")
        );
        assert!(it.get("status").is_none(), "not flattened");
    }

    /// The metadata item is the template binding's item in native
    /// kinds. This pins the two vocabularies against each other, so
    /// a key added to one without the other (or without a line in
    /// the lists below) fails here rather than drifting.
    #[test]
    fn key_parity_with_the_template_binding() {
        let l = listing("posts");
        let mut it = item("A");
        it.subtitle = Some(crate::document_profile::text("Sub"));
        it.date_modified = Some("2026-02-01".to_string());
        it.image = Some("posts/a.png".to_string());
        it.image_alt = Some("alt".to_string());
        let items = vec![it];

        let template_ctx = build_listing_context(&l, &items, "", &empty_meta());
        let Some(TemplateValue::List(list)) = template_ctx.get("items") else {
            panic!("items list")
        };
        let Some(TemplateValue::Map(binding)) = list.first() else {
            panic!("item map")
        };
        let binding_keys: BTreeSet<String> = binding.keys().cloned().collect();

        let date_style = effective_date_style(&l, &empty_meta());
        let meta_keys = keys(&item_to_config_value(&l, &items[0], "", &date_style));

        // Binding keys the metadata leaves out, by design (module docs).
        let omitted: BTreeSet<String> = [
            "title-html",
            "subtitle-html",
            "description-html",
            "image-html",
            "category-html",
            "metadata-attrs",
            "table-row",
            "show",
            "show-description",
            "description-placeholder-begin",
            "description-placeholder-end",
            "image-placeholder-begin",
            "image-placeholder-end",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        // Binding key → metadata key.
        let renamed: BTreeMap<String, String> = [("outputHref", "output-href")]
            .iter()
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect();
        // Metadata keys with no binding counterpart.
        let added: BTreeSet<String> = [
            "description-source",
            "image-source",
            "origin",
            "date-iso",
            "date-modified-iso",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();

        for k in omitted.iter().chain(renamed.keys()) {
            assert!(
                binding_keys.contains(k),
                "`{k}` is listed as a binding key but the binding no longer has it"
            );
        }
        for k in &added {
            assert!(
                !binding_keys.contains(k),
                "`{k}` is listed as metadata-only but the binding has it too"
            );
        }

        let expected: BTreeSet<String> = binding_keys
            .iter()
            .filter(|k| !omitted.contains(*k))
            .map(|k| renamed.get(k).cloned().unwrap_or_else(|| k.clone()))
            .chain(added.iter().cloned())
            .collect();
        assert_eq!(meta_keys, expected);
    }
}
