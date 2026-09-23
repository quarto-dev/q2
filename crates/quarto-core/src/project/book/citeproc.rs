/*
 * citeproc.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Citeproc deferral for single-file book merges (book-projects P2,
 * plan "Decisions" — Bibliography). Q2's own citeproc filter runs
 * inside `UserFiltersStage::pre()`, *before* `AstTransformsStage` —
 * so without intervention every book-merge chapter would resolve its
 * citations and build its own separate bibliography before the merge
 * ever sees it (N non-deduplicated bibliographies, wrong in-text
 * numbers across chapters).
 *
 * The fix has two halves, both driven from
 * `run_with_book_support()`'s single-file-merge branch:
 *
 * 1. [`strip_citeproc_from_filters`] — remove `"citeproc"` from each
 *    chapter's `meta["filters"]` (this module). Invoked by
 *    `UserFiltersStage::pre()` itself, immediately before filter
 *    resolution, whenever the render context's `defer_citeproc` flag is
 *    set (the book-merge driver sets it on every per-chapter paused
 *    render). This is a *metadata* mutation, not a resolved-filter-list
 *    edit: `resolve_filters` recomputes fresh from `meta["filters"]`
 *    inside `UserFiltersStage::run()`, so the metadata is the only
 *    reachable seam — and stripping there also keeps the merged
 *    document's metadata clean downstream. String form only —
 *    `"citeproc"` is a reserved name with no map form.
 * 2. After merge assembly, the driver calls
 *    [`pampa::citeproc_filter::apply_citeproc_filter`] once, directly,
 *    on the merged document, before the single Crossref–Finalization
 *    pass. Placement into the references chapter is free via
 *    citeproc's own `{#refs}`-div-filling convention.
 */

use quarto_pandoc_types::ConfigValue;
use quarto_pandoc_types::config_value::ConfigValueKind;

/// Remove every `"citeproc"` entry from `meta["filters"]` (whatever
/// its position relative to the `quarto` sentinel — users can order
/// it into either the `.pre` or `.post` group). Returns `true` when
/// anything was stripped. A missing/empty/non-array `filters` key is
/// a no-op; an array left empty by stripping is kept (it resolves to
/// no filters either way).
pub fn strip_citeproc_from_filters(meta: &mut ConfigValue) -> bool {
    let Some(filters) = meta.get("filters") else {
        return false;
    };
    let ConfigValueKind::Array(items) = &filters.value else {
        return false;
    };
    let original_len = items.len();
    let kept: Vec<ConfigValue> = items
        .iter()
        .filter(|item| item.as_plain_text().is_none_or(|s| s != "citeproc"))
        .cloned()
        .collect();
    let stripped = kept.len() != original_len;
    if stripped {
        meta.insert_path(
            &["filters"],
            ConfigValue::new_array(kept, filters.source_info.clone()),
        );
    }
    stripped
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_pandoc_types::config_value::ConfigMapEntry;
    use quarto_source_map::{By, SourceInfo};

    fn si() -> SourceInfo {
        SourceInfo::generated(By::programmatic_config())
    }

    fn s(v: &str) -> ConfigValue {
        ConfigValue::new_string(v, si())
    }

    fn arr(items: Vec<ConfigValue>) -> ConfigValue {
        ConfigValue::new_array(items, si())
    }

    fn meta_with_filters(filters: ConfigValue) -> ConfigValue {
        ConfigValue::new_map(
            vec![ConfigMapEntry {
                key: "filters".to_string(),
                key_source: si(),
                value: filters,
            }],
            si(),
        )
    }

    fn filter_strings(meta: &ConfigValue) -> Vec<String> {
        meta.get("filters")
            .and_then(|f| f.as_array())
            .map(|items| {
                items
                    .iter()
                    .map(|i| i.as_plain_text().unwrap_or_default())
                    .collect()
            })
            .unwrap_or_default()
    }

    #[test]
    fn citeproc_stripped_in_both_orderings() {
        // citeproc before the quarto sentinel (.pre ordering)…
        let mut pre = meta_with_filters(arr(vec![s("citeproc"), s("quarto"), s("a.lua")]));
        assert!(strip_citeproc_from_filters(&mut pre));
        assert_eq!(filter_strings(&pre), vec!["quarto", "a.lua"]);

        // …and after it (.post ordering).
        let mut post = meta_with_filters(arr(vec![s("a.lua"), s("quarto"), s("citeproc")]));
        assert!(strip_citeproc_from_filters(&mut post));
        assert_eq!(filter_strings(&post), vec!["a.lua", "quarto"]);
    }

    #[test]
    fn no_citeproc_leaves_array_untouched() {
        let mut meta = meta_with_filters(arr(vec![s("a.lua"), s("quarto"), s("b.lua")]));
        assert!(!strip_citeproc_from_filters(&mut meta));
        assert_eq!(filter_strings(&meta), vec!["a.lua", "quarto", "b.lua"]);
    }

    #[test]
    fn missing_filters_key_is_noop() {
        let mut meta = ConfigValue::new_map(Vec::<ConfigMapEntry>::new(), si());
        assert!(!strip_citeproc_from_filters(&mut meta));
        assert!(meta.get("filters").is_none());
    }

    #[test]
    fn citeproc_alone_leaves_an_empty_array() {
        let mut meta = meta_with_filters(arr(vec![s("citeproc")]));
        assert!(strip_citeproc_from_filters(&mut meta));
        assert_eq!(filter_strings(&meta), Vec::<String>::new());
        // The key stays (as an empty array) — resolves to no filters.
        assert!(meta.get("filters").is_some());
    }
}
