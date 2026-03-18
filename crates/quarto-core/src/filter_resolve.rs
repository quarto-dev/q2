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
use quarto_system_runtime::SystemRuntime;

use crate::extension::discover::find_extension;
use crate::extension::types::{Extension, ExtensionFilter};

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
/// Bare extension names are resolved via `find_extension()`: if a filter
/// name doesn't match an existing file, it's looked up as an extension name.
/// Extension filters are expanded inline with their contributed filter paths.
///
/// Relative filter paths are resolved against `document_dir`.
pub fn resolve_filters(
    meta: &ConfigValue,
    document_dir: &Path,
    extensions: &[Extension],
    runtime: &dyn SystemRuntime,
) -> ResolvedFilters {
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

        // Try extension resolution for string-form items
        if let Some(s) = item.as_plain_text() {
            if s != "citeproc" && s != "quarto" {
                let file_exists = runtime
                    .path_exists(&document_dir.join(&s), None)
                    .unwrap_or(false);
                if !file_exists {
                    if let Some(ext_filters) = try_resolve_extension_filter(&s, extensions) {
                        for ef in &ext_filters {
                            let spec = FilterSpec::parse(&ef.path.to_string_lossy());
                            let ep_idx = ef
                                .at
                                .as_deref()
                                .and_then(entry_point_index)
                                .unwrap_or(default_idx);
                            annotated.push(AnnotatedFilter {
                                spec,
                                entry_point_index: ep_idx,
                                original_index: i,
                            });
                        }
                        continue;
                    }
                }
            }
        }
        // Try extension resolution for map-form items
        else if let Some(path_val) = item.get("path") {
            if let Some(path_str) = path_val.as_plain_text() {
                let file_exists = runtime
                    .path_exists(&document_dir.join(&path_str), None)
                    .unwrap_or(false);
                if !file_exists {
                    if let Some(ext_filters) = try_resolve_extension_filter(&path_str, extensions) {
                        let at_override = item.get("at").and_then(|v| v.as_plain_text());
                        for ef in &ext_filters {
                            let spec = FilterSpec::parse(&ef.path.to_string_lossy());
                            let ep_idx = at_override
                                .as_deref()
                                .or(ef.at.as_deref())
                                .and_then(entry_point_index)
                                .unwrap_or(default_idx);
                            annotated.push(AnnotatedFilter {
                                spec,
                                entry_point_index: ep_idx,
                                original_index: i,
                            });
                        }
                        continue;
                    }
                }
            }
        }

        // Fall through to parse_filter_item() (existing behavior)
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

/// Try to resolve a filter name as an extension contributing filters.
///
/// Returns `Some(filters)` if the extension exists and contributes at least one filter.
fn try_resolve_extension_filter(
    name: &str,
    extensions: &[Extension],
) -> Option<Vec<ExtensionFilter>> {
    let ext = find_extension(name, extensions)?;
    if ext.contributes.filters.is_empty() {
        return None;
    }
    Some(ext.contributes.filters.clone())
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
    use crate::extension::types::{Contributes, ExtensionFilter, ExtensionId};
    use quarto_pandoc_types::config_value::{ConfigMapEntry, ConfigValue};
    use quarto_source_map::SourceInfo;
    use std::collections::HashSet;
    use std::path::PathBuf;

    /// Test runtime that reports specific paths as existing.
    struct TestRuntime {
        existing_paths: HashSet<PathBuf>,
    }

    impl TestRuntime {
        fn new() -> Self {
            Self {
                existing_paths: HashSet::new(),
            }
        }

        fn with_existing(paths: Vec<PathBuf>) -> Self {
            Self {
                existing_paths: paths.into_iter().collect(),
            }
        }
    }

    impl quarto_system_runtime::SystemRuntime for TestRuntime {
        fn file_read(
            &self,
            _path: &std::path::Path,
        ) -> quarto_system_runtime::RuntimeResult<Vec<u8>> {
            Ok(vec![])
        }
        fn file_write(
            &self,
            _path: &std::path::Path,
            _contents: &[u8],
        ) -> quarto_system_runtime::RuntimeResult<()> {
            Ok(())
        }
        fn path_exists(
            &self,
            path: &std::path::Path,
            _kind: Option<quarto_system_runtime::PathKind>,
        ) -> quarto_system_runtime::RuntimeResult<bool> {
            Ok(self.existing_paths.contains(path))
        }
        fn canonicalize(
            &self,
            path: &std::path::Path,
        ) -> quarto_system_runtime::RuntimeResult<PathBuf> {
            Ok(path.to_path_buf())
        }
        fn path_metadata(
            &self,
            _path: &std::path::Path,
        ) -> quarto_system_runtime::RuntimeResult<quarto_system_runtime::PathMetadata> {
            unimplemented!()
        }
        fn file_copy(
            &self,
            _src: &std::path::Path,
            _dst: &std::path::Path,
        ) -> quarto_system_runtime::RuntimeResult<()> {
            Ok(())
        }
        fn path_rename(
            &self,
            _old: &std::path::Path,
            _new: &std::path::Path,
        ) -> quarto_system_runtime::RuntimeResult<()> {
            Ok(())
        }
        fn file_remove(&self, _path: &std::path::Path) -> quarto_system_runtime::RuntimeResult<()> {
            Ok(())
        }
        fn dir_create(
            &self,
            _path: &std::path::Path,
            _recursive: bool,
        ) -> quarto_system_runtime::RuntimeResult<()> {
            Ok(())
        }
        fn dir_remove(
            &self,
            _path: &std::path::Path,
            _recursive: bool,
        ) -> quarto_system_runtime::RuntimeResult<()> {
            Ok(())
        }
        fn dir_list(
            &self,
            _path: &std::path::Path,
        ) -> quarto_system_runtime::RuntimeResult<Vec<PathBuf>> {
            Ok(vec![])
        }
        fn cwd(&self) -> quarto_system_runtime::RuntimeResult<PathBuf> {
            Ok(PathBuf::from("/"))
        }
        fn temp_dir(
            &self,
            _template: &str,
        ) -> quarto_system_runtime::RuntimeResult<quarto_system_runtime::TempDir> {
            Ok(quarto_system_runtime::TempDir::new(PathBuf::from(
                "/tmp/test",
            )))
        }
        fn exec_pipe(
            &self,
            _command: &str,
            _args: &[&str],
            _stdin: &[u8],
        ) -> quarto_system_runtime::RuntimeResult<Vec<u8>> {
            Ok(vec![])
        }
        fn exec_command(
            &self,
            _command: &str,
            _args: &[&str],
            _stdin: Option<&[u8]>,
        ) -> quarto_system_runtime::RuntimeResult<quarto_system_runtime::CommandOutput> {
            Ok(quarto_system_runtime::CommandOutput {
                code: 0,
                stdout: vec![],
                stderr: vec![],
            })
        }
        fn env_get(&self, _name: &str) -> quarto_system_runtime::RuntimeResult<Option<String>> {
            Ok(None)
        }
        fn env_all(
            &self,
        ) -> quarto_system_runtime::RuntimeResult<std::collections::HashMap<String, String>>
        {
            Ok(std::collections::HashMap::new())
        }
        fn fetch_url(&self, _url: &str) -> quarto_system_runtime::RuntimeResult<(Vec<u8>, String)> {
            Err(quarto_system_runtime::RuntimeError::NotSupported(
                "test".to_string(),
            ))
        }
        fn os_name(&self) -> &'static str {
            "test"
        }
        fn arch(&self) -> &'static str {
            "test"
        }
        fn cpu_time(&self) -> quarto_system_runtime::RuntimeResult<u64> {
            Ok(0)
        }
        fn xdg_dir(
            &self,
            _kind: quarto_system_runtime::XdgDirKind,
            _subpath: Option<&std::path::Path>,
        ) -> quarto_system_runtime::RuntimeResult<PathBuf> {
            Ok(PathBuf::from("/xdg"))
        }
        fn stdout_write(&self, _data: &[u8]) -> quarto_system_runtime::RuntimeResult<()> {
            Ok(())
        }
        fn stderr_write(&self, _data: &[u8]) -> quarto_system_runtime::RuntimeResult<()> {
            Ok(())
        }
    }

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

    fn no_extensions() -> Vec<Extension> {
        vec![]
    }

    fn make_extension(name: &str, filters: Vec<ExtensionFilter>) -> Extension {
        Extension {
            id: ExtensionId::new(name),
            title: name.to_string(),
            author: "Test".to_string(),
            version: None,
            quarto_required: None,
            path: PathBuf::from(format!("/project/_extensions/{}", name)),
            contributes: Contributes {
                filters,
                ..Default::default()
            },
        }
    }

    // === Existing tests updated with new signature ===

    #[test]
    fn empty_meta_returns_empty() {
        let meta = cv_map(vec![]);
        let rt = TestRuntime::new();
        let result = resolve_filters(&meta, &doc_dir(), &no_extensions(), &rt);
        assert!(result.pre.is_empty());
        assert!(result.post.is_empty());
    }

    #[test]
    fn missing_filters_key_returns_empty() {
        let meta = cv_map(vec![("title", cv_str("Hello"))]);
        let rt = TestRuntime::new();
        let result = resolve_filters(&meta, &doc_dir(), &no_extensions(), &rt);
        assert!(result.pre.is_empty());
        assert!(result.post.is_empty());
    }

    #[test]
    fn empty_filters_array_returns_empty() {
        let meta = meta_with_filters(cv_array(vec![]));
        let rt = TestRuntime::new();
        let result = resolve_filters(&meta, &doc_dir(), &no_extensions(), &rt);
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
        let rt = TestRuntime::new();
        let result = resolve_filters(&meta, &doc_dir(), &no_extensions(), &rt);
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
        let rt = TestRuntime::new();
        let result = resolve_filters(&meta, &doc_dir(), &no_extensions(), &rt);
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
        let rt = TestRuntime::new();
        let result = resolve_filters(&meta, &doc_dir(), &no_extensions(), &rt);
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
        let rt = TestRuntime::new();
        let result = resolve_filters(&meta, &doc_dir(), &no_extensions(), &rt);

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
        let meta = meta_with_filters(cv_array(vec![
            cv_str("quarto"),
            cv_map(vec![
                ("path", cv_str("x.lua")),
                ("at", cv_str("pre-quarto")),
            ]),
        ]));
        let rt = TestRuntime::new();
        let result = resolve_filters(&meta, &doc_dir(), &no_extensions(), &rt);
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
        let rt = TestRuntime::new();
        let result = resolve_filters(&meta, &doc_dir(), &no_extensions(), &rt);
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
        let rt = TestRuntime::new();
        let result = resolve_filters(&meta, &doc_dir(), &no_extensions(), &rt);
        assert_eq!(result.pre.len(), 3);
        assert_eq!(result.pre[0], FilterSpec::Lua(doc_dir().join("first.lua")));
        assert_eq!(result.pre[1], FilterSpec::Lua(doc_dir().join("second.lua")));
        assert_eq!(result.pre[2], FilterSpec::Lua(doc_dir().join("third.lua")));
    }

    #[test]
    fn relative_paths_resolved_against_document_dir() {
        let meta = meta_with_filters(cv_array(vec![cv_str("my-filter.lua")]));
        let rt = TestRuntime::new();
        let result = resolve_filters(&meta, &doc_dir(), &no_extensions(), &rt);
        assert_eq!(
            result.pre[0],
            FilterSpec::Lua(PathBuf::from("/project/docs/my-filter.lua"))
        );
    }

    #[test]
    fn absolute_paths_preserved() {
        let meta = meta_with_filters(cv_array(vec![cv_str("/usr/local/filters/abs.lua")]));
        let rt = TestRuntime::new();
        let result = resolve_filters(&meta, &doc_dir(), &no_extensions(), &rt);
        assert_eq!(
            result.pre[0],
            FilterSpec::Lua(PathBuf::from("/usr/local/filters/abs.lua"))
        );
    }

    #[test]
    fn no_sentinel_means_all_pre() {
        let meta = meta_with_filters(cv_array(vec![cv_str("a.lua"), cv_str("b.lua")]));
        let rt = TestRuntime::new();
        let result = resolve_filters(&meta, &doc_dir(), &no_extensions(), &rt);
        assert_eq!(result.pre.len(), 2);
        assert_eq!(result.post.len(), 0);
    }

    #[test]
    fn citeproc_path_not_resolved() {
        let meta = meta_with_filters(cv_array(vec![cv_str("citeproc")]));
        let rt = TestRuntime::new();
        let result = resolve_filters(&meta, &doc_dir(), &no_extensions(), &rt);
        assert_eq!(result.pre.len(), 1);
        assert_eq!(result.pre[0], FilterSpec::Citeproc);
    }

    #[test]
    fn json_filter_detected_by_extension() {
        let meta = meta_with_filters(cv_array(vec![cv_str("filter.py")]));
        let rt = TestRuntime::new();
        let result = resolve_filters(&meta, &doc_dir(), &no_extensions(), &rt);
        assert_eq!(result.pre[0], FilterSpec::Json(doc_dir().join("filter.py")));
    }

    #[test]
    fn object_with_json_type() {
        let meta = meta_with_filters(cv_array(vec![cv_map(vec![
            ("type", cv_str("json")),
            ("path", cv_str("my-filter")),
        ])]));
        let rt = TestRuntime::new();
        let result = resolve_filters(&meta, &doc_dir(), &no_extensions(), &rt);
        assert_eq!(result.pre[0], FilterSpec::Json(doc_dir().join("my-filter")));
    }

    #[test]
    fn unknown_at_defaults_to_pre_quarto() {
        let meta = meta_with_filters(cv_array(vec![
            cv_str("quarto"),
            cv_map(vec![
                ("path", cv_str("x.lua")),
                ("at", cv_str("bogus-entry-point")),
            ]),
        ]));
        let rt = TestRuntime::new();
        let result = resolve_filters(&meta, &doc_dir(), &no_extensions(), &rt);
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
        let rt = TestRuntime::new();
        let result = resolve_filters(&meta, &doc_dir(), &no_extensions(), &rt);

        assert_eq!(result.pre.len(), 2);
        assert_eq!(result.pre[0], FilterSpec::Lua(doc_dir().join("early.lua")));
        assert_eq!(
            result.pre[1],
            FilterSpec::Lua(doc_dir().join("default-pre.lua"))
        );

        assert_eq!(result.post.len(), 2);
        assert_eq!(
            result.post[0],
            FilterSpec::Lua(doc_dir().join("default-post.lua"))
        );
        assert_eq!(result.post[1], FilterSpec::Lua(doc_dir().join("late.lua")));
    }

    // === Extension filter resolution tests (Phase 2.2) ===

    #[test]
    fn test_extension_name_resolves_to_filter() {
        let ext = make_extension(
            "lightbox",
            vec![ExtensionFilter {
                path: PathBuf::from("/project/_extensions/lightbox/lightbox.lua"),
                at: None,
            }],
        );
        let meta = meta_with_filters(cv_array(vec![cv_str("lightbox")]));
        let rt = TestRuntime::new();
        let result = resolve_filters(&meta, &doc_dir(), &[ext], &rt);
        assert_eq!(result.pre.len(), 1);
        assert_eq!(
            result.pre[0],
            FilterSpec::Lua(PathBuf::from("/project/_extensions/lightbox/lightbox.lua"))
        );
    }

    #[test]
    fn test_extension_name_multiple_filters() {
        let ext = make_extension(
            "multi",
            vec![
                ExtensionFilter {
                    path: PathBuf::from("/ext/a.lua"),
                    at: None,
                },
                ExtensionFilter {
                    path: PathBuf::from("/ext/b.lua"),
                    at: None,
                },
            ],
        );
        let meta = meta_with_filters(cv_array(vec![cv_str("multi")]));
        let rt = TestRuntime::new();
        let result = resolve_filters(&meta, &doc_dir(), &[ext], &rt);
        assert_eq!(result.pre.len(), 2);
        assert_eq!(result.pre[0], FilterSpec::Lua(PathBuf::from("/ext/a.lua")));
        assert_eq!(result.pre[1], FilterSpec::Lua(PathBuf::from("/ext/b.lua")));
    }

    #[test]
    fn test_extension_name_with_at() {
        let ext = make_extension(
            "myext",
            vec![ExtensionFilter {
                path: PathBuf::from("/ext/filter.lua"),
                at: Some("post-render".to_string()),
            }],
        );
        let meta = meta_with_filters(cv_array(vec![cv_str("myext")]));
        let rt = TestRuntime::new();
        let result = resolve_filters(&meta, &doc_dir(), &[ext], &rt);
        // Extension filter's own at: post-render overrides position default
        assert_eq!(result.pre.len(), 0);
        assert_eq!(result.post.len(), 1);
        assert_eq!(
            result.post[0],
            FilterSpec::Lua(PathBuf::from("/ext/filter.lua"))
        );
    }

    #[test]
    fn test_extension_name_with_sentinel() {
        let ext = make_extension(
            "myext",
            vec![
                ExtensionFilter {
                    path: PathBuf::from("/ext/a.lua"),
                    at: None,
                },
                ExtensionFilter {
                    path: PathBuf::from("/ext/b.lua"),
                    at: Some("post-render".to_string()),
                },
            ],
        );
        // Extension name before sentinel: default is pre-quarto
        let meta = meta_with_filters(cv_array(vec![
            cv_str("myext"),
            cv_str("quarto"),
            cv_str("user.lua"),
        ]));
        let rt = TestRuntime::new();
        let result = resolve_filters(&meta, &doc_dir(), &[ext], &rt);
        // a.lua: no own at, before sentinel → pre-quarto (Pre)
        // b.lua: own at: post-render → Post
        // user.lua: after sentinel → post-render (Post)
        assert_eq!(result.pre.len(), 1);
        assert_eq!(result.pre[0], FilterSpec::Lua(PathBuf::from("/ext/a.lua")));
        assert_eq!(result.post.len(), 2);
        // b.lua comes first (same entry point, lower original_index from expansion)
        assert_eq!(result.post[0], FilterSpec::Lua(PathBuf::from("/ext/b.lua")));
        assert_eq!(result.post[1], FilterSpec::Lua(doc_dir().join("user.lua")));
    }

    #[test]
    fn test_map_form_extension_reference() {
        let ext = make_extension(
            "lightbox",
            vec![ExtensionFilter {
                path: PathBuf::from("/ext/lightbox.lua"),
                at: None,
            }],
        );
        let meta = meta_with_filters(cv_array(vec![cv_map(vec![("path", cv_str("lightbox"))])]));
        let rt = TestRuntime::new();
        let result = resolve_filters(&meta, &doc_dir(), &[ext], &rt);
        assert_eq!(result.pre.len(), 1);
        assert_eq!(
            result.pre[0],
            FilterSpec::Lua(PathBuf::from("/ext/lightbox.lua"))
        );
    }

    #[test]
    fn test_map_form_at_propagation() {
        let ext = make_extension(
            "myext",
            vec![
                ExtensionFilter {
                    path: PathBuf::from("/ext/a.lua"),
                    at: None,
                },
                ExtensionFilter {
                    path: PathBuf::from("/ext/b.lua"),
                    at: Some("pre-ast".to_string()),
                },
            ],
        );
        // Map form at overrides ALL extension filter entry points
        let meta = meta_with_filters(cv_array(vec![cv_map(vec![
            ("path", cv_str("myext")),
            ("at", cv_str("post-render")),
        ])]));
        let rt = TestRuntime::new();
        let result = resolve_filters(&meta, &doc_dir(), &[ext], &rt);
        // Both filters forced to post-render by map at override
        assert_eq!(result.pre.len(), 0);
        assert_eq!(result.post.len(), 2);
        assert_eq!(result.post[0], FilterSpec::Lua(PathBuf::from("/ext/a.lua")));
        assert_eq!(result.post[1], FilterSpec::Lua(PathBuf::from("/ext/b.lua")));
    }

    #[test]
    fn test_unresolved_name_falls_through() {
        // Name with no matching extension → treated as file path
        let meta = meta_with_filters(cv_array(vec![cv_str("nonexistent")]));
        let rt = TestRuntime::new();
        let result = resolve_filters(&meta, &doc_dir(), &no_extensions(), &rt);
        assert_eq!(result.pre.len(), 1);
        // Falls through to FilterSpec::parse which treats it as Json (no .lua extension)
        assert_eq!(
            result.pre[0],
            FilterSpec::Json(doc_dir().join("nonexistent"))
        );
    }

    #[test]
    fn test_existing_file_shadows_extension() {
        let ext = make_extension(
            "filter.lua",
            vec![ExtensionFilter {
                path: PathBuf::from("/ext/filter.lua"),
                at: None,
            }],
        );
        // The file exists on disk → file wins over extension
        let rt = TestRuntime::with_existing(vec![doc_dir().join("filter.lua")]);
        let meta = meta_with_filters(cv_array(vec![cv_str("filter.lua")]));
        let result = resolve_filters(&meta, &doc_dir(), &[ext], &rt);
        assert_eq!(result.pre.len(), 1);
        // File path, not extension path
        assert_eq!(result.pre[0], FilterSpec::Lua(doc_dir().join("filter.lua")));
    }

    #[test]
    fn test_extension_filter_paths_absolute() {
        let ext = make_extension(
            "myext",
            vec![ExtensionFilter {
                path: PathBuf::from("/project/_extensions/myext/filter.lua"),
                at: None,
            }],
        );
        let meta = meta_with_filters(cv_array(vec![cv_str("myext")]));
        let rt = TestRuntime::new();
        let result = resolve_filters(&meta, &doc_dir(), &[ext], &rt);
        assert_eq!(result.pre.len(), 1);
        // Path is absolute — not joined with document_dir
        assert_eq!(
            result.pre[0],
            FilterSpec::Lua(PathBuf::from("/project/_extensions/myext/filter.lua"))
        );
    }
}
