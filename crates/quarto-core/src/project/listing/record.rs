/*
 * project/listing/record.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! Inline `contents:` records → listing items
//! (bd-listing-inline-contents-tyy446ze, plan §D2/§D4/§D7).
//!
//! A record *is* the item (Q1 `listItemFromMeta`): curated keys map
//! to typed fields, everything else is a custom field in `extra`.
//! `path:` is captured raw — resolving it needs the project index
//! and the declaring file's directory, which the generate transform
//! has and this module does not.

use quarto_error_reporting::{DiagnosticMessage, DiagnosticMessageBuilder};
use quarto_pandoc_types::ConfigValue;
use quarto_source_map::SourceInfo;

use super::item::{ItemOrigin, ItemTarget, ListingItem, join_authors, rebase_image_from_dir};
use crate::document_profile::{LISTING_ITEM_KEYS, ListingItemInfo, UnknownKeyPolicy};

/// Keys this module owns: typed here, never forwarded to `extra`.
const RECORD_OWN_KEYS: &[&str] = &["author", "authors", "path", "order"];

/// One parsed inline record.
#[derive(Debug, Clone)]
pub struct ListingRecord {
    pub info: ListingItemInfo,
    pub authors: Vec<String>,
    pub order: Option<i32>,
    /// Raw `path:` value with its provenance.
    pub path: Option<(String, SourceInfo)>,
    /// The record's own span — for diagnostics that blame the whole record.
    pub source: SourceInfo,
}

pub fn parse_record(value: &ConfigValue, diags: &mut Vec<DiagnosticMessage>) -> ListingRecord {
    let info = ListingItemInfo::from_map(
        value,
        UnknownKeyPolicy::IntoExtra {
            except: RECORD_OWN_KEYS,
        },
    );
    let authors = crate::metadata::authors::parse_authors_model(value)
        .authors
        .iter()
        .map(|a| a.name.literal.clone())
        .collect();
    let order = value
        .get("order")
        .and_then(|v| v.as_int_lenient())
        .and_then(|i| i32::try_from(i).ok());
    let path = value
        .get("path")
        .and_then(|v| v.as_plain_text().map(|p| (p, v.source_info.clone())));

    diagnose_near_misses(value, diags);
    if info.title.is_none() && path.is_none() {
        diags.push(
            DiagnosticMessageBuilder::warning("Listing record has no `title:`")
                .with_code("Q-12-21")
                .with_location(value.source_info.clone())
                .problem(
                    "The record names no `path:` either, so there is nothing to derive a \
                     title from; the item renders with an empty title.",
                )
                .add_hint("Add `title:` to the record.")
                .build(),
        );
    }

    ListingRecord {
        info,
        authors,
        order,
        path,
        source: value.source_info.clone(),
    }
}

/// Build the item for a record that has no document behind it.
/// `base_dir` is the declaring file's project-relative directory —
/// relative `image:` values rebase onto it (path-resolution contract).
pub fn record_item(rec: ListingRecord, target: ItemTarget, base_dir: &str) -> ListingItem {
    let li = rec.info;
    let title = li
        .title
        .or_else(|| target.filename().map(|f| stem(&f)))
        .unwrap_or_default();
    ListingItem {
        title,
        subtitle: li.subtitle,
        description: li.description,
        author: join_authors(&rec.authors),
        authors: rec.authors,
        date: li.date,
        date_modified: li.date_modified,
        categories: li.categories,
        image: li.image.map(|img| rebase_image_from_dir(&img, base_dir)),
        image_alt: li.image_alt,
        image_lazy_loading: None,
        reading_time_minutes: li.reading_time_minutes,
        word_count: li.word_count,
        order: rec.order,
        target,
        origin: ItemOrigin::Record,
        extra: li.extra,
    }
}

/// Lay a record over a document's hydrated item: every field the
/// record sets wins; `categories` replaces rather than tag-merges
/// (Q1 spreads the record over the document's item).
pub fn overlay_record(mut item: ListingItem, rec: ListingRecord, base_dir: &str) -> ListingItem {
    let li = rec.info;
    if let Some(t) = li.title {
        item.title = t;
    }
    if li.subtitle.is_some() {
        item.subtitle = li.subtitle;
    }
    if li.description.is_some() {
        item.description = li.description;
    }
    if li.date.is_some() {
        item.date = li.date;
    }
    if li.date_modified.is_some() {
        item.date_modified = li.date_modified;
    }
    if let Some(img) = li.image {
        item.image = Some(rebase_image_from_dir(&img, base_dir));
    }
    if li.image_alt.is_some() {
        item.image_alt = li.image_alt;
    }
    if li.reading_time_minutes.is_some() {
        item.reading_time_minutes = li.reading_time_minutes;
    }
    if li.word_count.is_some() {
        item.word_count = li.word_count;
    }
    if !li.categories.is_empty() {
        item.categories = li.categories;
    }
    if !rec.authors.is_empty() {
        item.author = join_authors(&rec.authors);
        item.authors = rec.authors;
    }
    if rec.order.is_some() {
        item.order = rec.order;
    }
    item.extra.extend(li.extra);
    item.origin = ItemOrigin::RecordOverDocument;
    item
}

fn stem(filename: &str) -> String {
    match filename.rsplit_once('.') {
        Some((stem, _)) if !stem.is_empty() => stem.to_string(),
        _ => filename.to_string(),
    }
}

/// Curated keys an author might misspell.
const NEAR_MISS_TARGETS: &[&str] = &[
    "title",
    "subtitle",
    "description",
    "author",
    "authors",
    "date",
    "date-modified",
    "image",
    "image-alt",
    "categories",
    "path",
    "order",
    "reading-time-minutes",
    "word-count",
    "extra",
];

/// Q-12-22: unknown keys flow silently into `extra` (plan §D2), so a
/// typo'd curated key would otherwise be invisible.
fn diagnose_near_misses(value: &ConfigValue, diags: &mut Vec<DiagnosticMessage>) {
    let Some(entries) = value.as_map_entries() else {
        return;
    };
    for entry in entries {
        let key = entry.key.as_str();
        if NEAR_MISS_TARGETS.contains(&key) || LISTING_ITEM_KEYS.contains(&key) {
            continue;
        }
        let lower = key.to_ascii_lowercase();
        let limit = if lower.chars().count() <= 5 { 1 } else { 2 };
        let Some(target) = NEAR_MISS_TARGETS
            .iter()
            .find(|t| osa_distance(&lower, t) <= limit)
        else {
            continue;
        };
        diags.push(
            DiagnosticMessageBuilder::warning(format!(
                "Listing record key `{key}` looks like a misspelling of `{target}`"
            ))
            .with_code("Q-12-22")
            .with_location(entry.key_source.clone())
            .problem(format!(
                "`{key}` is not a listing field, so it was kept as the custom field \
                 `item.{key}`; the built-in templates will not display it."
            ))
            .add_hint(format!(
                "Rename the key to `{target}`, or keep it if a custom template reads `item.{key}`."
            ))
            .build(),
        );
    }
}

/// Optimal string alignment distance (Levenshtein + adjacent
/// transposition counted once). Small inputs; O(len·len) is fine.
fn osa_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let (n, m) = (a.len(), b.len());
    let mut d = vec![vec![0usize; m + 1]; n + 1];
    for (i, row) in d.iter_mut().enumerate() {
        row[0] = i;
    }
    for j in 0..=m {
        d[0][j] = j;
    }
    for i in 1..=n {
        for j in 1..=m {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            d[i][j] = (d[i - 1][j] + 1)
                .min(d[i][j - 1] + 1)
                .min(d[i - 1][j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                d[i][j] = d[i][j].min(d[i - 2][j - 2] + 1);
            }
        }
    }
    d[n][m]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::listing::ListingContents;
    use quarto_pandoc_types::config_value::ConfigMapEntry;
    use quarto_source_map::SourceInfo;

    fn s(v: &str) -> ConfigValue {
        ConfigValue::new_string(v, SourceInfo::for_test())
    }
    fn arr(items: Vec<ConfigValue>) -> ConfigValue {
        ConfigValue::new_array(items, SourceInfo::for_test())
    }
    fn map(entries: Vec<(&str, ConfigValue)>) -> ConfigValue {
        ConfigValue::new_map(
            entries
                .into_iter()
                .map(|(k, v)| ConfigMapEntry {
                    key: k.to_string(),
                    key_source: SourceInfo::for_test(),
                    value: v,
                })
                .collect(),
            SourceInfo::for_test(),
        )
    }
    fn parse(value: &ConfigValue) -> (ListingRecord, Vec<DiagnosticMessage>) {
        let mut diags = Vec::new();
        let rec = parse_record(value, &mut diags);
        (rec, diags)
    }
    fn codes(diags: &[DiagnosticMessage]) -> Vec<&str> {
        diags.iter().filter_map(|d| d.code.as_deref()).collect()
    }

    #[test]
    fn curated_keys_are_typed_and_unknown_keys_land_in_extra() {
        let (rec, diags) = parse(&map(vec![
            ("title", s("Get started")),
            ("description", s("Download and install Positron")),
            ("icon", s("bi-rocket-takeoff")),
            ("link", s("download.qmd")),
        ]));
        assert!(diags.is_empty(), "{diags:?}");
        assert_eq!(rec.info.title.as_deref(), Some("Get started"));
        assert_eq!(
            rec.info.description.as_deref(),
            Some("Download and install Positron")
        );
        assert_eq!(
            rec.info
                .extra
                .get("icon")
                .and_then(|v| v.as_plain_text())
                .as_deref(),
            Some("bi-rocket-takeoff")
        );
        assert_eq!(
            rec.info
                .extra
                .get("link")
                .and_then(|v| v.as_plain_text())
                .as_deref(),
            Some("download.qmd")
        );
        assert!(!rec.info.extra.contains_key("title"));
        assert_eq!(rec.path, None);
    }

    #[test]
    fn author_accepts_string_and_list_and_stays_out_of_extra() {
        let (one, _) = parse(&map(vec![("title", s("T")), ("author", s("Jane Doe"))]));
        assert_eq!(one.authors, vec!["Jane Doe"]);
        let (two, _) = parse(&map(vec![
            ("title", s("T")),
            ("author", arr(vec![s("Jane Doe"), s("John Roe")])),
        ]));
        assert_eq!(two.authors, vec!["Jane Doe", "John Roe"]);
        assert!(!two.info.extra.contains_key("author"));
    }

    #[test]
    fn path_and_order_are_owned_by_the_record() {
        let path_value = ConfigValue::new_string("download.qmd", SourceInfo::for_test());
        let expected_source = path_value.source_info.clone();
        let (rec, diags) = parse(&map(vec![("path", path_value), ("order", s("3"))]));
        assert!(
            diags.is_empty(),
            "a `path:` supplies the title fallback; {diags:?}"
        );
        assert_eq!(
            rec.path,
            Some(("download.qmd".to_string(), expected_source))
        );
        assert_eq!(rec.order, Some(3));
        assert!(!rec.info.extra.contains_key("path"));
        assert!(!rec.info.extra.contains_key("order"));
    }

    #[test]
    fn missing_title_without_path_warns_q_12_21() {
        let (_, diags) = parse(&map(vec![("description", s("no title here"))]));
        assert_eq!(codes(&diags), vec!["Q-12-21"]);
    }

    #[test]
    fn near_miss_keys_warn_q_12_22_with_the_intended_key() {
        let (rec, diags) = parse(&map(vec![
            ("title", s("T")),
            ("descripton", s("typo")),
            ("Title", s("case")),
        ]));
        let hits: Vec<&DiagnosticMessage> = diags
            .iter()
            .filter(|d| d.code.as_deref() == Some("Q-12-22"))
            .collect();
        assert_eq!(hits.len(), 2, "{diags:?}");
        assert!(hits[0].title.contains("`descripton`") && hits[0].title.contains("`description`"));
        assert!(hits[1].title.contains("`Title`") && hits[1].title.contains("`title`"));
        // The key is kept as a custom field regardless.
        assert!(rec.info.extra.contains_key("descripton"));
    }

    #[test]
    fn short_or_distant_keys_are_not_near_misses() {
        let (_, diags) = parse(&map(vec![
            ("title", s("T")),
            ("name", s("2 from `date` — not flagged at length 4")),
            ("link", s("x")),
            ("icon", s("y")),
            ("hide-profiles", arr(vec![s("positron")])),
        ]));
        assert!(codes(&diags).is_empty(), "{diags:?}");
    }

    #[test]
    fn osa_distance_counts_transpositions_once() {
        assert_eq!(osa_distance("titel", "title"), 1);
        assert_eq!(osa_distance("descripton", "description"), 1);
        assert_eq!(osa_distance("name", "date"), 2);
        assert_eq!(osa_distance("", "abc"), 3);
    }

    #[test]
    fn record_item_uses_record_fields_and_falls_back_to_href_stem() {
        let (rec, _) = parse(&map(vec![
            ("path", s("guides/report.pdf")),
            ("image", s("cover.png")),
        ]));
        let item = record_item(
            rec,
            ItemTarget::Href("guides/report.pdf".to_string()),
            "sub",
        );
        assert_eq!(item.title, "report");
        assert_eq!(item.origin, ItemOrigin::Record);
        assert_eq!(
            item.image.as_deref(),
            Some("sub/cover.png"),
            "image rebases onto the declaring dir"
        );
        assert_eq!(
            item.target,
            ItemTarget::Href("guides/report.pdf".to_string())
        );
    }

    #[test]
    fn overlay_record_fields_win_and_origin_flips() {
        use crate::document_profile::DocumentProfile;
        let profile = DocumentProfile {
            source_path: std::path::PathBuf::from("download.qmd"),
            output_href: "download.html".to_string(),
            format_id: "html".to_string(),
            title: Some("Download stub".to_string()),
            description: Some("from the document".to_string()),
            categories: vec!["doc-cat".to_string()],
            authors: vec!["Doc Author".to_string()],
            ..DocumentProfile::default()
        };
        let base = crate::project::listing::hydrate_item(&profile);
        let (rec, _) = parse(&map(vec![
            ("title", s("Get started")),
            ("path", s("download.qmd")),
            ("categories", arr(vec![s("rec-cat")])),
            ("icon", s("bi-rocket-takeoff")),
        ]));
        let item = overlay_record(base, rec, "");
        assert_eq!(item.title, "Get started", "record title wins");
        assert_eq!(
            item.description.as_deref(),
            Some("from the document"),
            "unset record fields keep the document's"
        );
        assert_eq!(
            item.categories,
            vec!["rec-cat"],
            "categories replace, not merge (Q1 spread)"
        );
        assert_eq!(
            item.authors,
            vec!["Doc Author"],
            "no record author → document authors kept"
        );
        assert_eq!(item.origin, ItemOrigin::RecordOverDocument);
        assert_eq!(
            item.target,
            ItemTarget::document("download.qmd", "download.html")
        );
        assert_eq!(
            item.extra
                .get("icon")
                .and_then(|v| v.as_plain_text())
                .as_deref(),
            Some("bi-rocket-takeoff")
        );
    }

    /// Q-12-22 must underline the misspelled *key*, not the record.
    #[test]
    fn q_12_22_underlines_the_offending_key() {
        use pampa::pandoc::yaml_to_config_value;
        use pampa::utils::diagnostic_collector::DiagnosticCollector;
        use quarto_config::{InterpretationContext, MergedConfig};
        const FIXTURE_FILE: &str = "index.qmd";
        let yaml = "\
listing:
  contents:
    - title: Inline
      descripton: oops
";
        let parsed = quarto_yaml::parse_file(yaml, FIXTURE_FILE).expect("valid yaml");
        let mut collector = DiagnosticCollector::new();
        let doc_config = yaml_to_config_value(
            parsed,
            InterpretationContext::DocumentMetadata,
            &mut collector,
        );
        let merged = MergedConfig::new(vec![&doc_config])
            .materialize()
            .expect("materialize");
        let listing_value = merged.get("listing").expect("`listing:` present");
        let mut diags = Vec::new();
        let listings = crate::project::listing::parse_listings(listing_value, &mut diags);
        let ListingContents::Inline(record) = &listings[0].contents[0] else {
            panic!("expected a record")
        };
        let (_, diags) = parse(record);
        let ctx = quarto_config::span_assert::context_for(FIXTURE_FILE, yaml);
        let d = diags
            .iter()
            .find(|d| d.code.as_deref() == Some("Q-12-22"))
            .expect("Q-12-22");
        let span = quarto_config::span_assert::resolve_diagnostic_span(d, &ctx).expect("real span");
        assert_eq!(span.text.trim(), "descripton", "got {:?}", span.text);
    }
}
