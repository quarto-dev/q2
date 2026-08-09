/*
 * project/listing/sort.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! Multi-key stable sort over hydrated listing items.
//!
//! Each [`crate::project::listing::config::ListingSort`] entry has
//! a field name and a direction; the sort applies entries in
//! declared order using a stable sort (so ties on the primary key
//! retain the secondary-key ordering, etc.). A sort field that is
//! neither built-in nor present on any item is reported via
//! `Q-12-3` and treated as if every item ties (i.e. no
//! rearrangement) — this matches Q1's tolerant behavior for typos
//! while staying silent for working custom-field sorts through the
//! `extra` map (bd-listing-declared-order-3ixcvc4o).

use quarto_error_reporting::{DiagnosticMessage, DiagnosticMessageBuilder};
use std::cmp::Ordering;

use super::config::{ListingSort, SortDirection};
use super::item::ListingItem;

/// Sort `items` in place according to the given multi-key spec.
/// Each key is applied in reverse order using a stable sort,
/// effectively producing the lexicographic ordering on (k1, k2, …)
/// requested by the author.
pub fn apply_sort(
    items: &mut [ListingItem],
    sort: &[ListingSort],
    diagnostics: &mut Vec<DiagnosticMessage>,
) {
    if sort.is_empty() {
        return;
    }

    // Validate keys once. Warn only when the sort has no information
    // to work with: the field is not built-in AND no item carries a
    // value for it (custom fields sort via the `extra` fallthrough
    // in `field_value`, and a field present on only some items still
    // sorts meaningfully — missing values last).
    for key in sort {
        if !is_known_sort_field(&key.field)
            && !items
                .iter()
                .any(|item| field_value(item, &key.field).is_some())
        {
            diagnostics.push(
                DiagnosticMessageBuilder::warning(format!(
                    "Unknown sort field `{}`; values will compare as equal.",
                    key.field
                ))
                .with_code("Q-12-3")
                .build(),
            );
        }
    }

    // Apply keys right-to-left with a stable sort: the LAST key
    // becomes the primary key only after the previous-key sort is
    // already in place. Iterating in reverse preserves the L2
    // contract that "first declared key is primary".
    for key in sort.iter().rev() {
        items.sort_by(|a, b| compare_items(a, b, key));
    }
}

fn compare_items(a: &ListingItem, b: &ListingItem, key: &ListingSort) -> Ordering {
    let av = field_value(a, &key.field);
    let bv = field_value(b, &key.field);
    match (av.as_deref(), bv.as_deref()) {
        // Missing values sort *after* present values regardless of
        // direction — the Asc/Desc flip applies only to the
        // value-to-value comparison below. (Flipping the whole
        // comparison floated missing-value items to the top of desc
        // sorts; found during bd-listing-declared-order-3ixcvc4o.)
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Greater,
        (Some(_), None) => Ordering::Less,
        (Some(a), Some(b)) => {
            let ord = natural_compare(a, b);
            match key.direction {
                SortDirection::Asc => ord,
                SortDirection::Desc => ord.reverse(),
            }
        }
    }
}

/// Natural string comparison with a numeric-prefix bias: strings
/// that parse as integers compare numerically; other strings use
/// lexicographic order (case-insensitive). This handles the common
/// "page-1, page-2, page-10" sort intuition without going full
/// human-numeric.
fn natural_compare(a: &str, b: &str) -> Ordering {
    if let (Ok(an), Ok(bn)) = (a.trim().parse::<i64>(), b.trim().parse::<i64>()) {
        return an.cmp(&bn);
    }
    a.to_lowercase().cmp(&b.to_lowercase())
}

fn field_value(item: &ListingItem, field: &str) -> Option<String> {
    match field {
        "title" => Some(item.title.clone()),
        "subtitle" => item.subtitle.clone(),
        "description" => item.description.clone(),
        "author" => item.author.clone(),
        "date" => item.date.clone(),
        "date-modified" => item.date_modified.clone(),
        "image" => item.image.clone(),
        "image-alt" => item.image_alt.clone(),
        "filename" => item
            .source_path
            .file_name()
            .and_then(|s| s.to_str())
            .map(String::from),
        "path" => Some(item.source_path.display().to_string()),
        "output-href" => Some(item.output_href.clone()),
        "reading-time" => item.reading_time_minutes.map(|n| n.to_string()),
        "word-count" => item.word_count.map(|n| n.to_string()),
        "order" => item.order.map(|n| n.to_string()),
        // Fall through to extra map.
        _ => item.extra.get(field).and_then(|v| v.as_plain_text()),
    }
}

fn is_known_sort_field(field: &str) -> bool {
    matches!(
        field,
        "title"
            | "subtitle"
            | "description"
            | "author"
            | "date"
            | "date-modified"
            | "image"
            | "image-alt"
            | "filename"
            | "path"
            | "output-href"
            | "reading-time"
            | "word-count"
            | "order"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::listing::item::ListingItem;
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    fn make_item(title: &str, date: Option<&str>) -> ListingItem {
        ListingItem {
            title: title.to_string(),
            subtitle: None,
            description: None,
            author: None,
            authors: vec![],
            date: date.map(String::from),
            date_modified: None,
            categories: vec![],
            image: None,
            image_alt: None,
            image_lazy_loading: None,
            reading_time_minutes: None,
            word_count: None,
            order: None,
            source_path: PathBuf::from(format!("posts/{}.qmd", title)),
            output_href: format!("posts/{}.html", title),
            extra: BTreeMap::new(),
        }
    }

    #[test]
    fn sort_by_date_asc() {
        let mut items = vec![
            make_item("c", Some("2026-03-01")),
            make_item("a", Some("2026-01-01")),
            make_item("b", Some("2026-02-01")),
        ];
        let sort = vec![ListingSort {
            field: "date".to_string(),
            direction: SortDirection::Asc,
        }];
        let mut diags = Vec::new();
        apply_sort(&mut items, &sort, &mut diags);
        assert_eq!(items[0].title, "a");
        assert_eq!(items[1].title, "b");
        assert_eq!(items[2].title, "c");
        assert!(diags.is_empty());
    }

    #[test]
    fn sort_by_date_desc() {
        let mut items = vec![
            make_item("a", Some("2026-01-01")),
            make_item("b", Some("2026-02-01")),
            make_item("c", Some("2026-03-01")),
        ];
        let sort = vec![ListingSort {
            field: "date".to_string(),
            direction: SortDirection::Desc,
        }];
        let mut diags = Vec::new();
        apply_sort(&mut items, &sort, &mut diags);
        assert_eq!(items[0].title, "c");
        assert_eq!(items[1].title, "b");
        assert_eq!(items[2].title, "a");
    }

    // Multi-key: primary by date desc, tiebreaker title asc.
    #[test]
    fn sort_multi_key_primary_then_secondary() {
        let mut items = vec![
            make_item("c", Some("2026-01-01")),
            make_item("a", Some("2026-01-01")),
            make_item("b", Some("2026-02-01")),
        ];
        let sort = vec![
            ListingSort {
                field: "date".to_string(),
                direction: SortDirection::Desc,
            },
            ListingSort {
                field: "title".to_string(),
                direction: SortDirection::Asc,
            },
        ];
        let mut diags = Vec::new();
        apply_sort(&mut items, &sort, &mut diags);
        assert_eq!(items[0].title, "b"); // newer date first
        assert_eq!(items[1].title, "a"); // tied dates → title asc
        assert_eq!(items[2].title, "c");
    }

    #[test]
    fn missing_dates_sort_to_end_in_asc() {
        let mut items = vec![
            make_item("a", None),
            make_item("b", Some("2026-01-01")),
            make_item("c", None),
        ];
        let sort = vec![ListingSort {
            field: "date".to_string(),
            direction: SortDirection::Asc,
        }];
        let mut diags = Vec::new();
        apply_sort(&mut items, &sort, &mut diags);
        // Present-value items first; missing items last; relative
        // order among missing items preserved (stable).
        assert_eq!(items[0].title, "b");
        assert!(items[1].title == "a" || items[1].title == "c");
    }

    // `order` is a known sort field (Q1's front-matter curation
    // field, primary key of the default sort) and compares
    // numerically: 2 < 10; missing order sorts last.
    #[test]
    fn sort_by_order_field_is_known_and_numeric() {
        let with_order = |title: &str, order: Option<i32>| {
            let mut item = make_item(title, None);
            item.order = order;
            item
        };
        let mut items = vec![
            with_order("c", Some(10)),
            with_order("plain", None),
            with_order("b", Some(2)),
            with_order("a", Some(1)),
        ];
        let sort = vec![ListingSort {
            field: "order".to_string(),
            direction: SortDirection::Asc,
        }];
        let mut diags = Vec::new();
        apply_sort(&mut items, &sort, &mut diags);
        assert!(diags.is_empty(), "order is a known field; got {:?}", diags);
        assert_eq!(items[0].title, "a");
        assert_eq!(items[1].title, "b");
        assert_eq!(items[2].title, "c");
        assert_eq!(items[3].title, "plain");
    }

    // The documented rule is "missing values sort after present
    // values regardless of direction" — the Desc flip must apply to
    // the value comparison only, not to the missing-value rule.
    // (Latent bug found during bd-listing-declared-order-3ixcvc4o:
    // the flip was applied to the whole comparison, floating
    // missing-value items to the top of desc sorts.)
    #[test]
    fn missing_dates_sort_to_end_in_desc_too() {
        let mut items = vec![
            make_item("a", None),
            make_item("b", Some("2026-01-01")),
            make_item("c", None),
        ];
        let sort = vec![ListingSort {
            field: "date".to_string(),
            direction: SortDirection::Desc,
        }];
        let mut diags = Vec::new();
        apply_sort(&mut items, &sort, &mut diags);
        assert_eq!(items[0].title, "b");
        // Stable among the missing-value items.
        assert_eq!(items[1].title, "a");
        assert_eq!(items[2].title, "c");
    }

    #[test]
    fn unknown_sort_field_emits_q_12_3() {
        let mut items = vec![make_item("a", None)];
        let sort = vec![ListingSort {
            field: "nosuchfield".to_string(),
            direction: SortDirection::Asc,
        }];
        let mut diags = Vec::new();
        apply_sort(&mut items, &sort, &mut diags);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code.as_deref(), Some("Q-12-3"));
    }

    fn make_item_with_extra(title: &str, key: &str, value: &str) -> ListingItem {
        use quarto_pandoc_types::ConfigValue;
        use quarto_source_map::SourceInfo;

        let mut item = make_item(title, None);
        item.extra.insert(
            key.to_string(),
            ConfigValue::new_string(value, SourceInfo::for_test()),
        );
        item
    }

    // bd-listing-declared-order-3ixcvc4o: a custom-field sort that
    // works via the `extra` fallthrough must not be diagnosed as an
    // unknown field — warn only when no item carries a value for it.
    #[test]
    fn extra_field_sort_sorts_and_emits_no_q_12_3() {
        let mut items = vec![
            make_item_with_extra("b", "difficulty", "2"),
            make_item_with_extra("c", "difficulty", "10"),
            make_item_with_extra("a", "difficulty", "1"),
        ];
        let sort = vec![ListingSort {
            field: "difficulty".to_string(),
            direction: SortDirection::Asc,
        }];
        let mut diags = Vec::new();
        apply_sort(&mut items, &sort, &mut diags);
        assert!(
            diags.is_empty(),
            "working extra-field sort must not warn; got {:?}",
            diags
        );
        // Numeric comparison via natural_compare: 1, 2, 10.
        assert_eq!(items[0].title, "a");
        assert_eq!(items[1].title, "b");
        assert_eq!(items[2].title, "c");
    }

    // The suppression is any-item: a field present on only SOME items
    // still sorts meaningfully (missing values last), so no warning.
    #[test]
    fn sparse_extra_field_sort_emits_no_q_12_3() {
        let mut items = vec![
            make_item("plain", None),
            make_item_with_extra("tagged", "difficulty", "1"),
        ];
        let sort = vec![ListingSort {
            field: "difficulty".to_string(),
            direction: SortDirection::Asc,
        }];
        let mut diags = Vec::new();
        apply_sort(&mut items, &sort, &mut diags);
        assert!(
            diags.is_empty(),
            "sparse field must not warn; got {:?}",
            diags
        );
        assert_eq!(items[0].title, "tagged");
        assert_eq!(items[1].title, "plain");
    }
}
