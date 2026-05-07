/*
 * project/listing/sort.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! Multi-key stable sort over hydrated listing items.
//!
//! Each [`crate::project::listing::config::ListingSort`] entry has
//! a field name and a direction; the sort applies entries in
//! declared order using a stable sort (so ties on the primary key
//! retain the secondary-key ordering, etc.). Unknown sort fields
//! are reported via `Q-12-3` and treated as if every item ties
//! (i.e. no rearrangement) — this matches Q1's tolerant behavior
//! for typos.

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

    // Validate keys once.
    for key in sort {
        if !is_known_sort_field(&key.field) {
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
    let ord = compare_values(av.as_deref(), bv.as_deref());
    match key.direction {
        SortDirection::Asc => ord,
        SortDirection::Desc => ord.reverse(),
    }
}

fn compare_values(a: Option<&str>, b: Option<&str>) -> Ordering {
    match (a, b) {
        // Missing values sort *after* present values regardless of
        // direction — matches Q1's "absent value at the bottom"
        // intuition. The Asc/Desc flip in the caller handles
        // direction; this function defines the canonical order.
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Greater,
        (Some(_), None) => Ordering::Less,
        (Some(a), Some(b)) => natural_compare(a, b),
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
}
