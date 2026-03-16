/*
 * filter_resolve.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Resolves the `filters` metadata key into pre/post filter lists
 * for the render pipeline.
 */

use std::path::Path;

use pampa::unified_filter::FilterSpec;
use quarto_pandoc_types::ConfigValue;

/// The result of resolving the `filters` metadata key.
///
/// Filters are split into two groups based on their entry point:
/// - `pre`: filters that run before `AstTransformsStage`
/// - `post`: filters that run after `AstTransformsStage`
#[derive(Debug, Default)]
pub struct ResolvedFilters {
    pub pre: Vec<FilterSpec>,
    pub post: Vec<FilterSpec>,
}

/// Entry points recognized by TS Quarto, in canonical execution order.
///
/// Each entry point maps to either the Pre or Post pipeline position.
const ENTRY_POINTS: &[(&str, Position)] = &[
    ("pre-ast", Position::Pre),
    ("post-ast", Position::Pre),
    ("pre-quarto", Position::Pre),
    ("post-quarto", Position::Post),
    ("pre-render", Position::Post),
    ("post-render", Position::Post),
    ("pre-finalize", Position::Post),
    ("post-finalize", Position::Post),
];

const DEFAULT_BEFORE_SENTINEL: &str = "pre-quarto";
const DEFAULT_AFTER_SENTINEL: &str = "post-render";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Position {
    Pre,
    Post,
}

struct AnnotatedFilter {
    spec: FilterSpec,
    entry_point_index: usize,
    original_index: usize,
}

fn entry_point_index(name: &str) -> Option<usize> {
    ENTRY_POINTS.iter().position(|(ep, _)| *ep == name)
}

/// Resolve the `filters` metadata key into pre/post filter lists.
///
/// Reads `meta["filters"]`, finds the `"quarto"` sentinel (if any),
/// assigns each filter an entry point, sorts by entry point order,
/// and splits into Pre/Post groups.
///
/// Relative filter paths are resolved against `document_dir`.
pub fn resolve_filters(meta: &ConfigValue, document_dir: &Path) -> ResolvedFilters {
    let filters_val = match meta.get("filters") {
        Some(v) => v,
        None => return ResolvedFilters::default(),
    };

    let items = match filters_val.as_array() {
        Some(items) if !items.is_empty() => items,
        _ => return ResolvedFilters::default(),
    };

    // Find the sentinel index.
    // Use as_plain_text() to handle both Scalar(String) and PandocInlines forms.
    let sentinel_index = items
        .iter()
        .position(|item| item.as_plain_text().map(|s| s == "quarto").unwrap_or(false));

    let default_before_idx = entry_point_index(DEFAULT_BEFORE_SENTINEL).unwrap();
    let default_after_idx = entry_point_index(DEFAULT_AFTER_SENTINEL).unwrap();

    let mut annotated: Vec<AnnotatedFilter> = Vec::new();

    for (i, item) in items.iter().enumerate() {
        if sentinel_index == Some(i) {
            continue;
        }

        let after_sentinel = sentinel_index.map(|si| i > si).unwrap_or(false);
        let default_idx = if after_sentinel {
            default_after_idx
        } else {
            default_before_idx
        };

        let (spec, ep_idx) = parse_filter_item(item, default_idx);

        annotated.push(AnnotatedFilter {
            spec,
            entry_point_index: ep_idx,
            original_index: i,
        });
    }

    // Stable sort by entry point order
    annotated.sort_by_key(|a| (a.entry_point_index, a.original_index));

    // Split into pre/post and resolve paths
    let mut result = ResolvedFilters::default();
    for ann in annotated {
        let position = ENTRY_POINTS[ann.entry_point_index].1;
        let spec = resolve_filter_path(ann.spec, document_dir);
        match position {
            Position::Pre => result.pre.push(spec),
            Position::Post => result.post.push(spec),
        }
    }

    result
}

/// Parse a single filter item from the metadata array.
///
/// Returns the `FilterSpec` and its entry point index.
fn parse_filter_item(item: &ConfigValue, default_ep_idx: usize) -> (FilterSpec, usize) {
    // String form: "citeproc", "filter.lua", "filter.py"
    // Use as_plain_text() to handle both Scalar(String) and PandocInlines forms.
    if let Some(s) = item.as_plain_text() {
        return (FilterSpec::parse(&s), default_ep_idx);
    }

    // Map form: {type: "lua", path: "filter.lua"} or {path: "filter.lua", at: "pre-ast"}
    if let Some(path_val) = item.get("path") {
        if let Some(path_str) = path_val.as_plain_text() {
            // Determine filter type from explicit `type` field or path extension
            let type_val = item.get("type").and_then(|v| v.as_plain_text());
            let spec = match type_val.as_deref() {
                Some("lua") => FilterSpec::Lua(path_str.into()),
                Some("json") => FilterSpec::Json(path_str.into()),
                _ => FilterSpec::parse(&path_str),
            };

            // Check for explicit `at` entry point
            let ep_idx = if let Some(at_str) = item.get("at").and_then(|v| v.as_plain_text()) {
                match entry_point_index(&at_str) {
                    Some(idx) => idx,
                    None => {
                        tracing::warn!(
                            "Unknown filter entry point '{}', defaulting to '{}'",
                            at_str,
                            DEFAULT_BEFORE_SENTINEL
                        );
                        entry_point_index(DEFAULT_BEFORE_SENTINEL).unwrap()
                    }
                }
            } else {
                default_ep_idx
            };

            return (spec, ep_idx);
        }
    }

    // Unrecognized format
    tracing::warn!("Unrecognized filter specification in metadata, skipping");
    (FilterSpec::Json("".into()), default_ep_idx)
}

/// Resolve relative filter paths against the document directory.
fn resolve_filter_path(spec: FilterSpec, document_dir: &Path) -> FilterSpec {
    match spec {
        FilterSpec::Citeproc => FilterSpec::Citeproc,
        FilterSpec::Lua(path) => {
            if path.is_absolute() {
                FilterSpec::Lua(path)
            } else {
                FilterSpec::Lua(document_dir.join(path))
            }
        }
        FilterSpec::Json(path) => {
            if path.is_absolute() {
                FilterSpec::Json(path)
            } else {
                FilterSpec::Json(document_dir.join(path))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_pandoc_types::config_value::{ConfigMapEntry, ConfigValue};
    use quarto_source_map::SourceInfo;
    use std::path::PathBuf;

    fn cv_str(s: &str) -> ConfigValue {
        ConfigValue::new_string(s, SourceInfo::default())
    }

    fn cv_array(items: Vec<ConfigValue>) -> ConfigValue {
        ConfigValue::new_array(items, SourceInfo::default())
    }

    fn cv_map(entries: Vec<(&str, ConfigValue)>) -> ConfigValue {
        ConfigValue::new_map(
            entries
                .into_iter()
                .map(|(k, v)| ConfigMapEntry {
                    key: k.to_string(),
                    key_source: SourceInfo::default(),
                    value: v,
                })
                .collect(),
            SourceInfo::default(),
        )
    }

    fn meta_with_filters(filters: ConfigValue) -> ConfigValue {
        cv_map(vec![("filters", filters)])
    }

    fn doc_dir() -> PathBuf {
        PathBuf::from("/project/docs")
    }

    #[test]
    fn empty_meta_returns_empty() {
        let meta = cv_map(vec![]);
        let result = resolve_filters(&meta, &doc_dir());
        assert!(result.pre.is_empty());
        assert!(result.post.is_empty());
    }

    #[test]
    fn missing_filters_key_returns_empty() {
        let meta = cv_map(vec![("title", cv_str("Hello"))]);
        let result = resolve_filters(&meta, &doc_dir());
        assert!(result.pre.is_empty());
        assert!(result.post.is_empty());
    }

    #[test]
    fn empty_filters_array_returns_empty() {
        let meta = meta_with_filters(cv_array(vec![]));
        let result = resolve_filters(&meta, &doc_dir());
        assert!(result.pre.is_empty());
        assert!(result.post.is_empty());
    }

    #[test]
    fn string_filters_default_to_pre_quarto() {
        let meta = meta_with_filters(cv_array(vec![
            cv_str("a.lua"),
            cv_str("b.py"),
            cv_str("citeproc"),
        ]));
        let result = resolve_filters(&meta, &doc_dir());
        assert_eq!(result.pre.len(), 3);
        assert_eq!(result.post.len(), 0);
        assert_eq!(result.pre[0], FilterSpec::Lua(doc_dir().join("a.lua")));
        assert_eq!(result.pre[1], FilterSpec::Json(doc_dir().join("b.py")));
        assert_eq!(result.pre[2], FilterSpec::Citeproc);
    }

    #[test]
    fn object_filter_with_type_and_path() {
        let meta = meta_with_filters(cv_array(vec![cv_map(vec![
            ("type", cv_str("lua")),
            ("path", cv_str("a.lua")),
        ])]));
        let result = resolve_filters(&meta, &doc_dir());
        assert_eq!(result.pre.len(), 1);
        assert_eq!(result.pre[0], FilterSpec::Lua(doc_dir().join("a.lua")));
    }

    #[test]
    fn quarto_sentinel_splits_pre_post() {
        let meta = meta_with_filters(cv_array(vec![
            cv_str("pre.lua"),
            cv_str("quarto"),
            cv_str("post.lua"),
        ]));
        let result = resolve_filters(&meta, &doc_dir());
        assert_eq!(result.pre.len(), 1);
        assert_eq!(result.post.len(), 1);
        assert_eq!(result.pre[0], FilterSpec::Lua(doc_dir().join("pre.lua")));
        assert_eq!(result.post[0], FilterSpec::Lua(doc_dir().join("post.lua")));
    }

    #[test]
    fn all_eight_entry_points_map_correctly() {
        let filters: Vec<ConfigValue> = vec![
            ("a.lua", "pre-ast"),
            ("b.lua", "post-ast"),
            ("c.lua", "pre-quarto"),
            ("d.lua", "post-quarto"),
            ("e.lua", "pre-render"),
            ("f.lua", "post-render"),
            ("g.lua", "pre-finalize"),
            ("h.lua", "post-finalize"),
        ]
        .into_iter()
        .map(|(path, at)| cv_map(vec![("path", cv_str(path)), ("at", cv_str(at))]))
        .collect();

        let meta = meta_with_filters(cv_array(filters));
        let result = resolve_filters(&meta, &doc_dir());

        // Pre: pre-ast, post-ast, pre-quarto
        assert_eq!(result.pre.len(), 3);
        assert_eq!(result.pre[0], FilterSpec::Lua(doc_dir().join("a.lua")));
        assert_eq!(result.pre[1], FilterSpec::Lua(doc_dir().join("b.lua")));
        assert_eq!(result.pre[2], FilterSpec::Lua(doc_dir().join("c.lua")));

        // Post: post-quarto, pre-render, post-render, pre-finalize, post-finalize
        assert_eq!(result.post.len(), 5);
        assert_eq!(result.post[0], FilterSpec::Lua(doc_dir().join("d.lua")));
        assert_eq!(result.post[1], FilterSpec::Lua(doc_dir().join("e.lua")));
        assert_eq!(result.post[2], FilterSpec::Lua(doc_dir().join("f.lua")));
        assert_eq!(result.post[3], FilterSpec::Lua(doc_dir().join("g.lua")));
        assert_eq!(result.post[4], FilterSpec::Lua(doc_dir().join("h.lua")));
    }

    #[test]
    fn at_overrides_sentinel_position() {
        // After sentinel, but `at: "pre-quarto"` forces it to Pre
        let meta = meta_with_filters(cv_array(vec![
            cv_str("quarto"),
            cv_map(vec![
                ("path", cv_str("x.lua")),
                ("at", cv_str("pre-quarto")),
            ]),
        ]));
        let result = resolve_filters(&meta, &doc_dir());
        assert_eq!(result.pre.len(), 1);
        assert_eq!(result.post.len(), 0);
        assert_eq!(result.pre[0], FilterSpec::Lua(doc_dir().join("x.lua")));
    }

    #[test]
    fn sorting_by_entry_point_order() {
        let meta = meta_with_filters(cv_array(vec![
            cv_map(vec![
                ("path", cv_str("b.lua")),
                ("at", cv_str("post-render")),
            ]),
            cv_map(vec![
                ("path", cv_str("a.lua")),
                ("at", cv_str("pre-quarto")),
            ]),
        ]));
        let result = resolve_filters(&meta, &doc_dir());
        assert_eq!(result.pre.len(), 1);
        assert_eq!(result.post.len(), 1);
        assert_eq!(result.pre[0], FilterSpec::Lua(doc_dir().join("a.lua")));
        assert_eq!(result.post[0], FilterSpec::Lua(doc_dir().join("b.lua")));
    }

    #[test]
    fn same_entry_point_preserves_relative_order() {
        let meta = meta_with_filters(cv_array(vec![
            cv_str("first.lua"),
            cv_str("second.lua"),
            cv_str("third.lua"),
        ]));
        let result = resolve_filters(&meta, &doc_dir());
        assert_eq!(result.pre.len(), 3);
        assert_eq!(result.pre[0], FilterSpec::Lua(doc_dir().join("first.lua")));
        assert_eq!(result.pre[1], FilterSpec::Lua(doc_dir().join("second.lua")));
        assert_eq!(result.pre[2], FilterSpec::Lua(doc_dir().join("third.lua")));
    }

    #[test]
    fn relative_paths_resolved_against_document_dir() {
        let meta = meta_with_filters(cv_array(vec![cv_str("my-filter.lua")]));
        let result = resolve_filters(&meta, &doc_dir());
        assert_eq!(
            result.pre[0],
            FilterSpec::Lua(PathBuf::from("/project/docs/my-filter.lua"))
        );
    }

    #[test]
    fn absolute_paths_preserved() {
        let meta = meta_with_filters(cv_array(vec![cv_str("/usr/local/filters/abs.lua")]));
        let result = resolve_filters(&meta, &doc_dir());
        assert_eq!(
            result.pre[0],
            FilterSpec::Lua(PathBuf::from("/usr/local/filters/abs.lua"))
        );
    }

    #[test]
    fn no_sentinel_means_all_pre() {
        let meta = meta_with_filters(cv_array(vec![cv_str("a.lua"), cv_str("b.lua")]));
        let result = resolve_filters(&meta, &doc_dir());
        assert_eq!(result.pre.len(), 2);
        assert_eq!(result.post.len(), 0);
    }

    #[test]
    fn citeproc_path_not_resolved() {
        let meta = meta_with_filters(cv_array(vec![cv_str("citeproc")]));
        let result = resolve_filters(&meta, &doc_dir());
        assert_eq!(result.pre.len(), 1);
        assert_eq!(result.pre[0], FilterSpec::Citeproc);
    }

    #[test]
    fn json_filter_detected_by_extension() {
        let meta = meta_with_filters(cv_array(vec![cv_str("filter.py")]));
        let result = resolve_filters(&meta, &doc_dir());
        assert_eq!(result.pre[0], FilterSpec::Json(doc_dir().join("filter.py")));
    }

    #[test]
    fn object_with_json_type() {
        let meta = meta_with_filters(cv_array(vec![cv_map(vec![
            ("type", cv_str("json")),
            ("path", cv_str("my-filter")),
        ])]));
        let result = resolve_filters(&meta, &doc_dir());
        assert_eq!(result.pre[0], FilterSpec::Json(doc_dir().join("my-filter")));
    }

    #[test]
    fn unknown_at_defaults_to_pre_quarto() {
        // Unknown `at` value should default to pre-quarto regardless of sentinel
        let meta = meta_with_filters(cv_array(vec![
            cv_str("quarto"),
            cv_map(vec![
                ("path", cv_str("x.lua")),
                ("at", cv_str("bogus-entry-point")),
            ]),
        ]));
        let result = resolve_filters(&meta, &doc_dir());
        // Even though it's after sentinel, unknown `at` → pre-quarto → Pre
        assert_eq!(result.pre.len(), 1);
        assert_eq!(result.post.len(), 0);
        assert_eq!(result.pre[0], FilterSpec::Lua(doc_dir().join("x.lua")));
    }

    #[test]
    fn mixed_pre_and_post_with_multiple_entry_points() {
        let meta = meta_with_filters(cv_array(vec![
            cv_map(vec![
                ("path", cv_str("early.lua")),
                ("at", cv_str("pre-ast")),
            ]),
            cv_str("default-pre.lua"),
            cv_str("quarto"),
            cv_str("default-post.lua"),
            cv_map(vec![
                ("path", cv_str("late.lua")),
                ("at", cv_str("post-finalize")),
            ]),
        ]));
        let result = resolve_filters(&meta, &doc_dir());

        // Pre: early.lua (pre-ast), default-pre.lua (pre-quarto)
        assert_eq!(result.pre.len(), 2);
        assert_eq!(result.pre[0], FilterSpec::Lua(doc_dir().join("early.lua")));
        assert_eq!(
            result.pre[1],
            FilterSpec::Lua(doc_dir().join("default-pre.lua"))
        );

        // Post: default-post.lua (post-render), late.lua (post-finalize)
        assert_eq!(result.post.len(), 2);
        assert_eq!(
            result.post[0],
            FilterSpec::Lua(doc_dir().join("default-post.lua"))
        );
        assert_eq!(result.post[1], FilterSpec::Lua(doc_dir().join("late.lua")));
    }
}
