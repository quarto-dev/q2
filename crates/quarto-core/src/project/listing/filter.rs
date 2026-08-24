/*
 * project/listing/filter.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! `include` / `exclude` predicate evaluation.
//!
//! L3 D12 (user-confirmed 2026-05-06): the lookup chain for a
//! filter key K on an item I is
//!
//!   1. Curated [`ListingItem`] field named K (`title`, `author`,
//!      `categories`, etc.).
//!   2. Otherwise look up `I.extra[K]`.
//!   3. Absent → predicate fails (no match).
//!
//! This preserves Q1 parity for blogs that filter on free-form
//! custom fields like `status: published`. See
//! `claude-notes/plans/2026-05-06-listings-L3-resolve-transform.md`
//! §"Settled inputs" for the rationale.
//!
//! Predicate semantics:
//! - Multiple keys inside one record = AND.
//! - Multiple records inside `include` / `exclude` = OR.
//! - Scalar field: literal string equality (case-sensitive).
//! - List field: any-element string equality.

use quarto_pandoc_types::ConfigValue;
use quarto_pandoc_types::config_value::ConfigValueKind;

use super::config::ListingFilter;
use super::item::ListingItem;

/// Apply `include` / `exclude` filters to a list of items in
/// place. Items that fail `include` (when `include` is non-empty)
/// or pass `exclude` (when set) are removed.
pub fn apply_filters(
    items: &mut Vec<ListingItem>,
    include: &[ListingFilter],
    exclude: &[ListingFilter],
) {
    items.retain(|item| {
        let included =
            include.is_empty() || include.iter().any(|filter| matches_filter(item, filter));
        if !included {
            return false;
        }
        let excluded = exclude.iter().any(|filter| matches_filter(item, filter));
        !excluded
    });
}

/// True iff every key in `filter.fields` matches the item.
fn matches_filter(item: &ListingItem, filter: &ListingFilter) -> bool {
    filter
        .fields
        .iter()
        .all(|(key, expected)| matches_key(item, key, expected))
}

/// Look up `key` in `item` (curated → `extra` fallback) and test
/// against `expected`.
fn matches_key(item: &ListingItem, key: &str, expected: &ConfigValue) -> bool {
    let expected_text = match expected.as_plain_text() {
        Some(s) => s,
        None => return false,
    };
    if let Some(curated) = lookup_curated(item, key) {
        return curated_matches(&curated, &expected_text);
    }
    if let Some(extra) = item.extra.get(key) {
        return extra_matches(extra, &expected_text);
    }
    false
}

/// Lookup view over the curated [`ListingItem`] fields. Each
/// field returns either a single string or a list of strings; the
/// representation is read by [`curated_matches`].
enum Curated {
    Scalar(Option<String>),
    List(Vec<String>),
}

fn lookup_curated(item: &ListingItem, key: &str) -> Option<Curated> {
    let v = match key {
        "title" => Curated::Scalar(Some(item.title.clone())),
        "subtitle" => Curated::Scalar(item.subtitle.clone()),
        "description" => Curated::Scalar(item.description.clone()),
        "author" => Curated::List(item.authors.clone()),
        "date" => Curated::Scalar(item.date.clone()),
        "date-modified" => Curated::Scalar(item.date_modified.clone()),
        "categories" => Curated::List(item.categories.clone()),
        "image" => Curated::Scalar(item.image.clone()),
        "image-alt" => Curated::Scalar(item.image_alt.clone()),
        "path" => Curated::Scalar(item.target.filter_path()),
        "output-href" => Curated::Scalar(item.target.href().map(String::from)),
        _ => return None,
    };
    Some(v)
}

fn curated_matches(curated: &Curated, expected: &str) -> bool {
    match curated {
        Curated::Scalar(Some(s)) => s == expected,
        Curated::Scalar(None) => false,
        Curated::List(items) => items.iter().any(|s| s == expected),
    }
}

fn extra_matches(extra: &ConfigValue, expected: &str) -> bool {
    match &extra.value {
        ConfigValueKind::Scalar { .. } => extra.as_plain_text().as_deref() == Some(expected),
        ConfigValueKind::Array(items) => items
            .iter()
            .any(|v| v.as_plain_text().as_deref() == Some(expected)),
        // Map-valued extras can't be matched against a scalar in v1.
        // A future grammar (e.g. `extra.status: published`) could
        // descend; for now, no match.
        _ => extra.as_plain_text().as_deref() == Some(expected),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::listing::item::{ItemOrigin, ItemTarget, ListingItem};
    use quarto_pandoc_types::ConfigValue;
    use quarto_source_map::SourceInfo;
    use std::collections::BTreeMap;

    fn s(value: &str) -> ConfigValue {
        ConfigValue::new_string(value, SourceInfo::for_test())
    }

    fn make_item(
        title: &str,
        authors: Vec<&str>,
        categories: Vec<&str>,
        extra: Vec<(&str, ConfigValue)>,
    ) -> ListingItem {
        let mut extra_map = BTreeMap::new();
        for (k, v) in extra {
            extra_map.insert(k.to_string(), v);
        }
        ListingItem {
            title: title.to_string(),
            subtitle: None,
            description: None,
            author: if authors.is_empty() {
                None
            } else {
                Some(authors.join(", "))
            },
            authors: authors.into_iter().map(String::from).collect(),
            date: None,
            date_modified: None,
            categories: categories.into_iter().map(String::from).collect(),
            image: None,
            image_alt: None,
            image_lazy_loading: None,
            reading_time_minutes: None,
            word_count: None,
            order: None,
            target: ItemTarget::document("posts/foo.qmd", "posts/foo.html"),
            origin: ItemOrigin::Document,
            extra: extra_map,
        }
    }

    fn filter(entries: Vec<(&str, ConfigValue)>) -> ListingFilter {
        let mut fields = BTreeMap::new();
        for (k, v) in entries {
            fields.insert(k.to_string(), v);
        }
        ListingFilter { fields }
    }

    // 11. include filter on string field (curated)
    #[test]
    fn include_filter_matches_string_field() {
        let mut items = vec![
            make_item("a", vec!["Foo"], vec![], vec![]),
            make_item("b", vec!["Bar"], vec![], vec![]),
        ];
        apply_filters(&mut items, &[filter(vec![("author", s("Foo"))])], &[]);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "a");
    }

    // 12. include filter on list field (curated)
    #[test]
    fn include_filter_matches_list_field() {
        let mut items = vec![
            make_item("a", vec![], vec!["rust", "design"], vec![]),
            make_item("b", vec![], vec!["other"], vec![]),
        ];
        apply_filters(&mut items, &[filter(vec![("categories", s("rust"))])], &[]);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "a");
    }

    // 12b. include filter falls through to extra (D12)
    #[test]
    fn include_filter_falls_through_to_extra() {
        let mut items = vec![
            make_item("a", vec![], vec![], vec![("status", s("published"))]),
            make_item("b", vec![], vec![], vec![("status", s("draft"))]),
        ];
        apply_filters(&mut items, &[filter(vec![("status", s("published"))])], &[]);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "a");
    }

    // 12c. curated field shadows extra with the same name
    #[test]
    fn include_filter_curated_shadows_extra() {
        // Item has curated `categories: [rust]` AND
        // `extra.categories: [draft]`. The filter looks up the
        // curated field first; `categories: rust` matches via
        // curated, `categories: draft` does NOT match (because
        // curated doesn't contain "draft" and the curated lookup
        // wins; we don't fall through to extra after a curated hit).
        let mut items_a = vec![make_item(
            "a",
            vec![],
            vec!["rust"],
            vec![(
                "categories",
                ConfigValue::new_array(vec![s("draft")], SourceInfo::for_test()),
            )],
        )];
        apply_filters(
            &mut items_a,
            &[filter(vec![("categories", s("rust"))])],
            &[],
        );
        assert_eq!(items_a.len(), 1, "rust matches curated");

        let mut items_b = vec![make_item(
            "b",
            vec![],
            vec!["rust"],
            vec![(
                "categories",
                ConfigValue::new_array(vec![s("draft")], SourceInfo::for_test()),
            )],
        )];
        apply_filters(
            &mut items_b,
            &[filter(vec![("categories", s("draft"))])],
            &[],
        );
        assert!(
            items_b.is_empty(),
            "draft must NOT match because curated shadows extra"
        );
    }

    // include with multiple keys = AND
    #[test]
    fn include_with_multiple_keys_requires_all_match() {
        let mut items = vec![
            make_item("a", vec!["Foo"], vec!["rust"], vec![]),
            make_item("b", vec!["Foo"], vec!["js"], vec![]),
            make_item("c", vec!["Bar"], vec!["rust"], vec![]),
        ];
        apply_filters(
            &mut items,
            &[filter(vec![
                ("author", s("Foo")),
                ("categories", s("rust")),
            ])],
            &[],
        );
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "a");
    }

    // multiple include records = OR
    #[test]
    fn multiple_include_records_or_match() {
        let mut items = vec![
            make_item("a", vec!["Foo"], vec![], vec![]),
            make_item("b", vec!["Bar"], vec![], vec![]),
            make_item("c", vec!["Baz"], vec![], vec![]),
        ];
        apply_filters(
            &mut items,
            &[
                filter(vec![("author", s("Foo"))]),
                filter(vec![("author", s("Bar"))]),
            ],
            &[],
        );
        assert_eq!(items.len(), 2);
    }

    // exclude removes matching items
    #[test]
    fn exclude_removes_matching_items() {
        let mut items = vec![
            make_item("a", vec!["Foo"], vec![], vec![]),
            make_item("b", vec!["Bar"], vec![], vec![]),
        ];
        apply_filters(&mut items, &[], &[filter(vec![("author", s("Foo"))])]);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "b");
    }
}
