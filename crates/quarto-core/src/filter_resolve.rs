/*
 * filter_resolve.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Resolves the `filters` metadata key into pre/post filter lists
 * for the render pipeline.
 */

use std::path::PathBuf;

use pampa::unified_filter::FilterSpec;
use quarto_pandoc_types::ConfigValue;
use quarto_system_runtime::SystemRuntime;

use crate::config_paths::{FileCandidate, PathCtx, resolve_config_file};
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
    /// The entry-point name (e.g. `"post-quarto"`, `"pre-render"`) each
    /// `post` filter was resolved from, same length and order as `post`.
    /// Carried alongside rather than folded into `post`'s element type so
    /// every existing `result.post[i] == FilterSpec::...` assertion keeps
    /// compiling unchanged. book-projects P2b's `PandocWriteStage`-side
    /// contributor reads this to rebuild the exact `at:` a Pandoc-hybrid
    /// render must forward each filter to in `quarto-filters.entryPoints`
    /// — `Position::Post` bundles five distinct Q1 entry-point names
    /// (`post-quarto`, `pre-render`, `post-render`, `pre-finalize`,
    /// `post-finalize`), so the bucket alone isn't enough information.
    pub post_entry_points: Vec<&'static str>,
    /// Filter entries that named an extension Quarto now builds in
    /// (`quarto-ext/lightbox`): dropped, as in Q1, with the entry's id
    /// and location so the caller can tell the user to remove it.
    pub superseded: Vec<(String, quarto_source_map::SourceInfo)>,
}

/// Extensions from the `quarto-ext` organization whose feature Quarto now
/// provides itself. A filter entry resolving to one of them is dropped
/// rather than run on top of the built-in (Q1 `kBuiltInExtNames`,
/// `filterBuiltInExtensions` in `src/extension/extension.ts`).
const SUPERSEDED_EXTENSIONS: &[(&str, &str)] = &[("quarto-ext", "lightbox")];

/// The id of the superseded extension `name` resolves to, if any.
fn superseded_extension(name: &str, extensions: &[Extension]) -> Option<String> {
    let ext = find_extension(name, extensions)?;
    let org = ext.id.organization.as_deref()?;
    SUPERSEDED_EXTENSIONS
        .iter()
        .any(|(o, n)| *o == org && *n == ext.id.name)
        .then(|| format!("{org}/{}", ext.id.name))
}

/// Whether the document renders citations as margin notes
/// (`citation-location: margin`). On Typst the vendored
/// `quarto-post/typst.lua` margin handler owns citation rendering in that
/// mode (it runs `pandoc.utils.citeproc` on a side copy and needs the
/// original `Cite` nodes), so Q2's whole-document citeproc filter must
/// stay out of its way.
pub fn margin_citations(meta: &ConfigValue) -> bool {
    meta.get("citation-location")
        .and_then(|v| v.as_plain_text())
        .is_some_and(|s| s == "margin")
}

/// Which citeproc defaults a render applies before filter resolution.
///
/// Mirrors Quarto 1's `citeMethod()`
/// (`src/command/render/filters.ts`), which decides per format whether
/// citations get citeproc without the user asking for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CiteprocDefaults {
    /// A `bibliography` or `references` key alone turns citeproc on.
    /// True for every Q1 format except Typst, whose format registration
    /// sets `citeproc: false` so Typst's own `#bibliography()` handles
    /// citations by default.
    pub from_bibliography: bool,
    /// `citation-location: margin` hands citation rendering to another
    /// handler, so the whole-document filter stays out of its way. True
    /// on the Pandoc-hybrid leg, where Typst's `quarto-post/typst.lua`
    /// margin handler needs the original `Cite` nodes (see
    /// [`margin_citations`]).
    pub margin_handled_elsewhere: bool,
}

/// Whether `meta` names a bibliography: a non-empty `bibliography` or
/// `references` value, the keys Q1's `citeMethod()` checks.
fn has_bibliography(meta: &ConfigValue) -> bool {
    ["bibliography", "references"].iter().any(|key| {
        meta.get(key).is_some_and(|v| match v.as_array() {
            Some(items) => !items.is_empty(),
            None => v.as_plain_text().is_none_or(|s| !s.is_empty()),
        })
    })
}

/// Whether `meta` picks a LaTeX bibliography engine (`cite-method:
/// natbib` or `biblatex`), which replaces citeproc in Q1.
fn has_non_citeproc_cite_method(meta: &ConfigValue) -> bool {
    meta.get("cite-method")
        .and_then(|v| v.as_plain_text())
        .is_some_and(|m| m != "citeproc")
}

/// Add the `"citeproc"` entry to `meta["filters"]` that
/// [`resolve_filters`] reads, when the render's citation settings call for
/// citeproc. Returns `true` when the metadata was changed.
///
/// Citeproc is called for, as in Q1's `citeMethod()`, when
/// - `citeproc: true` is set, or
/// - `defaults.from_bibliography` holds, `meta` names a bibliography
///   ([`has_bibliography`]), and no LaTeX bibliography engine is chosen
///   (GH #1026: before this, an HTML document with `bibliography:` left
///   every citation as literal `@key`).
///
/// `citeproc: false` always wins, as does an explicit `filters` list that
/// already names `"citeproc"` (its position stays the user's choice). In
/// margin citation mode, nothing is added when
/// `defaults.margin_handled_elsewhere` is set.
///
/// The entry is placed after the `"quarto"` sentinel (added if absent) so
/// it lands in the post-crossref group, as Q1 runs citeproc after
/// cross-reference resolution: a pre-crossref citeproc would consume
/// `@fig-x` crossref `Cite` nodes as unresolved citations.
pub fn apply_citeproc_default(meta: &mut ConfigValue, defaults: CiteprocDefaults) -> bool {
    use quarto_pandoc_types::config_value::ConfigValueKind;

    let explicit = meta.get("citeproc").and_then(|v| v.as_bool());
    let wanted = match explicit {
        Some(enabled) => enabled,
        None => {
            defaults.from_bibliography
                && has_bibliography(meta)
                && !has_non_citeproc_cite_method(meta)
        }
    };
    if !wanted || (defaults.margin_handled_elsewhere && margin_citations(meta)) {
        return false;
    }
    let si = meta
        .get("citeproc")
        .or_else(|| meta.get("bibliography"))
        .or_else(|| meta.get("references"))
        .map(|v| v.source_info.clone())
        .unwrap_or_default();
    let mut items: Vec<ConfigValue> = match meta.get("filters").map(|f| &f.value) {
        Some(ConfigValueKind::Array(items)) => items.clone(),
        _ => Vec::new(),
    };
    let listed = |name: &str| {
        items
            .iter()
            .any(|i| i.as_plain_text().is_some_and(|s| s == name))
    };
    if listed("citeproc") {
        return false;
    }
    if !listed("quarto") {
        items.push(ConfigValue::new_string("quarto", si.clone()));
    }
    items.push(ConfigValue::new_string("citeproc", si.clone()));
    meta.insert_path(&["filters"], ConfigValue::new_array(items, si));
    true
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
    /// The declaration-resolved file for a user-declared Lua/JSON
    /// filter (`None` for `citeproc` and for extension-contributed
    /// filters, whose paths are already absolute).
    resolved: Option<PathBuf>,
}

/// The file a user-declared filter entry denotes, resolved against the
/// file that declared it (`crate::config_paths`), with whether it
/// exists. `None` for a URL or a non-string value.
fn resolve_filter_file(
    value: &ConfigValue,
    ctx: &PathCtx<'_>,
    runtime: &dyn SystemRuntime,
) -> Option<FileCandidate> {
    resolve_config_file(value, ctx, runtime)
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
/// Relative filter paths resolve against the file that declared them
/// (project root for `_quarto.yml`, the `_metadata.yml`'s directory, the
/// document's directory for front matter), and a leading `/` anchors at
/// the project root — `crate::config_paths::resolve_config_path`, fed by
/// `ctx`. GH #945 / bd-nqq41zgx: this used to join every relative entry
/// onto the consuming document's directory, so a project-level filter
/// failed for every page in a subdirectory.
pub fn resolve_filters(
    meta: &ConfigValue,
    ctx: &PathCtx<'_>,
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
        .position(|item| item.as_plain_text().is_some_and(|s| s == "quarto"));

    let default_before_idx = entry_point_index(DEFAULT_BEFORE_SENTINEL).unwrap();
    let default_after_idx = entry_point_index(DEFAULT_AFTER_SENTINEL).unwrap();

    let mut annotated: Vec<AnnotatedFilter> = Vec::new();
    let mut superseded = Vec::new();

    for (i, item) in items.iter().enumerate() {
        if sentinel_index == Some(i) {
            continue;
        }

        let after_sentinel = sentinel_index.is_some_and(|si| i > si);
        let default_idx = if after_sentinel {
            default_after_idx
        } else {
            default_before_idx
        };

        // The declaration-resolved file the entry names, if it names
        // one: a string entry (unless it is a reserved name), or the
        // `path:` of a map entry. Probed for existence before the name
        // is tried as an extension — an existing file shadows an
        // extension of the same name, as in Quarto 1.
        let mut resolved: Option<PathBuf> = None;
        // Try extension resolution for string-form items
        if let Some(s) = item.as_plain_text() {
            if s != "citeproc" && s != "quarto" {
                let candidate = resolve_filter_file(item, ctx, runtime);
                let file_exists = candidate.as_ref().is_some_and(|c| c.exists);
                resolved = candidate.map(|c| c.path);
                if !file_exists && let Some(id) = superseded_extension(&s, extensions) {
                    superseded.push((id, item.source_info.clone()));
                    continue;
                }
                if !file_exists
                    && let Some(ext_filters) = try_resolve_extension_filter(&s, extensions)
                {
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
                            resolved: None,
                        });
                    }
                    continue;
                }
            }
        }
        // Try extension resolution for map-form items
        else if let Some(path_val) = item.get("path")
            && let Some(path_str) = path_val.as_plain_text()
        {
            let candidate = resolve_filter_file(path_val, ctx, runtime);
            let file_exists = candidate.as_ref().is_some_and(|c| c.exists);
            resolved = candidate.map(|c| c.path);
            if !file_exists && let Some(id) = superseded_extension(&path_str, extensions) {
                superseded.push((id, item.source_info.clone()));
                continue;
            }
            if !file_exists
                && let Some(ext_filters) = try_resolve_extension_filter(&path_str, extensions)
            {
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
                        resolved: None,
                    });
                }
                continue;
            }
        }

        // Fall through to parse_filter_item() (existing behavior)
        let (spec, ep_idx) = parse_filter_item(item, default_idx);

        annotated.push(AnnotatedFilter {
            spec,
            entry_point_index: ep_idx,
            original_index: i,
            resolved,
        });
    }

    // Stable sort by entry point order
    annotated.sort_by_key(|a| (a.entry_point_index, a.original_index));

    // Split into pre/post and resolve paths
    let mut result = ResolvedFilters {
        superseded,
        ..ResolvedFilters::default()
    };
    for ann in annotated {
        let (entry_point, position) = ENTRY_POINTS[ann.entry_point_index];
        let spec = resolve_filter_path(ann.spec, ann.resolved);
        match position {
            Position::Pre => result.pre.push(spec),
            Position::Post => {
                result.post.push(spec);
                result.post_entry_points.push(entry_point);
            }
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
    if let Some(path_val) = item.get("path")
        && let Some(path_str) = path_val.as_plain_text()
    {
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

    // Unrecognized format
    tracing::warn!("Unrecognized filter specification in metadata, skipping");
    (FilterSpec::Json("".into()), default_ep_idx)
}

/// Replace a user-declared Lua/JSON filter's authored path with its
/// declaration-resolved file (`resolved`, computed by
/// [`resolve_filter_file`]). Entries with no resolved file — `citeproc`,
/// extension-contributed filters (already absolute), URLs — keep their
/// spec as parsed.
fn resolve_filter_path(spec: FilterSpec, resolved: Option<PathBuf>) -> FilterSpec {
    match (spec, resolved) {
        (FilterSpec::Lua(_), Some(path)) => FilterSpec::Lua(path),
        (FilterSpec::Json(_), Some(path)) => FilterSpec::Json(path),
        (spec, _) => spec,
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

    #[async_trait::async_trait]
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
                "/nonexistent/q2-mock-temp-dir",
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
        async fn fetch_url(
            &self,
            _url: &str,
        ) -> quarto_system_runtime::RuntimeResult<(Vec<u8>, String)> {
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
        ConfigValue::new_string(s, SourceInfo::for_test())
    }

    fn cv_array(items: Vec<ConfigValue>) -> ConfigValue {
        ConfigValue::new_array(items, SourceInfo::for_test())
    }

    fn cv_map(entries: Vec<(&str, ConfigValue)>) -> ConfigValue {
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

    fn cv_true() -> ConfigValue {
        ConfigValue::new_bool(true, SourceInfo::for_test())
    }

    fn filter_names(meta: &ConfigValue) -> Vec<String> {
        meta.get("filters")
            .and_then(|f| f.as_array())
            .map(|items| items.iter().filter_map(|i| i.as_plain_text()).collect())
            .unwrap_or_default()
    }

    fn cv_false() -> ConfigValue {
        ConfigValue::new_bool(false, SourceInfo::for_test())
    }

    /// The defaults an HTML render applies: a bibliography alone turns
    /// citeproc on, and nothing else owns margin citations.
    const HTML: CiteprocDefaults = CiteprocDefaults {
        from_bibliography: true,
        margin_handled_elsewhere: false,
    };

    /// The defaults a Typst render applies: Q1 registers Typst with
    /// `citeproc: false`, and its Lua margin handler owns margin citations.
    const TYPST: CiteprocDefaults = CiteprocDefaults {
        from_bibliography: false,
        margin_handled_elsewhere: true,
    };

    #[test]
    fn citeproc_true_adds_post_group_citeproc() {
        let mut meta = cv_map(vec![("citeproc", cv_true())]);
        assert!(apply_citeproc_default(&mut meta, TYPST));
        assert_eq!(filter_names(&meta), vec!["quarto", "citeproc"]);
        let result = resolve(&meta, &[], &TestRuntime::new());
        assert!(result.pre.is_empty());
        assert_eq!(result.post, vec![FilterSpec::Citeproc]);
    }

    #[test]
    fn citeproc_true_reuses_existing_sentinel_and_keeps_user_filters() {
        let mut meta = cv_map(vec![
            ("citeproc", cv_true()),
            (
                "filters",
                cv_array(vec![cv_str("a.lua"), cv_str("quarto"), cv_str("b.lua")]),
            ),
        ]);
        assert!(apply_citeproc_default(&mut meta, TYPST));
        assert_eq!(
            filter_names(&meta),
            vec!["a.lua", "quarto", "b.lua", "citeproc"]
        );
    }

    #[test]
    fn citeproc_default_is_noop_when_unset_false_listed_or_margin() {
        let mut unset = cv_map(vec![]);
        assert!(!apply_citeproc_default(&mut unset, HTML));
        assert!(unset.get("filters").is_none());

        let mut off = cv_map(vec![("citeproc", cv_false())]);
        assert!(!apply_citeproc_default(&mut off, HTML));
        assert!(off.get("filters").is_none());

        let mut listed = cv_map(vec![
            ("citeproc", cv_true()),
            ("filters", cv_array(vec![cv_str("citeproc")])),
        ]);
        assert!(!apply_citeproc_default(&mut listed, HTML));
        assert_eq!(filter_names(&listed), vec!["citeproc"]);

        let mut margin = cv_map(vec![
            ("citeproc", cv_true()),
            ("citation-location", cv_str("margin")),
        ]);
        assert!(!apply_citeproc_default(&mut margin, TYPST));
        assert!(margin.get("filters").is_none());
    }

    /// GH #1026: Q1's `citeMethod()` runs citeproc whenever a bibliography
    /// is set, unless `citeproc: false`. Without this, an HTML document
    /// with `bibliography:` rendered every citation as literal `@key`.
    #[test]
    fn bibliography_alone_implies_citeproc() {
        let mut meta = cv_map(vec![("bibliography", cv_str("refs.bib"))]);
        assert!(apply_citeproc_default(&mut meta, HTML));
        assert_eq!(filter_names(&meta), vec!["quarto", "citeproc"]);

        let mut list = cv_map(vec![(
            "bibliography",
            cv_array(vec![cv_str("a.bib"), cv_str("b.bib")]),
        )]);
        assert!(apply_citeproc_default(&mut list, HTML));
        assert_eq!(filter_names(&list), vec!["quarto", "citeproc"]);
    }

    /// Inline `references:` count as a bibliography, as in Q1.
    #[test]
    fn references_alone_implies_citeproc() {
        let mut meta = cv_map(vec![(
            "references",
            cv_array(vec![cv_map(vec![("id", cv_str("doe2020"))])]),
        )]);
        assert!(apply_citeproc_default(&mut meta, HTML));
        assert_eq!(filter_names(&meta), vec!["quarto", "citeproc"]);
    }

    #[test]
    fn bibliography_does_not_imply_citeproc_when_disabled() {
        // Explicit opt-out wins over the bibliography.
        let mut off = cv_map(vec![
            ("bibliography", cv_str("refs.bib")),
            ("citeproc", cv_false()),
        ]);
        assert!(!apply_citeproc_default(&mut off, HTML));
        assert!(off.get("filters").is_none());

        // Typst's format default is `citeproc: false`.
        let mut typst = cv_map(vec![("bibliography", cv_str("refs.bib"))]);
        assert!(!apply_citeproc_default(&mut typst, TYPST));
        assert!(typst.get("filters").is_none());

        // An empty bibliography list names no bibliography.
        let mut empty = cv_map(vec![("bibliography", cv_array(vec![]))]);
        assert!(!apply_citeproc_default(&mut empty, HTML));
        assert!(empty.get("filters").is_none());
    }

    /// A LaTeX bibliography engine (`cite-method: natbib|biblatex`) replaces
    /// citeproc in Q1, so the bibliography default stands aside for it;
    /// `cite-method: citeproc` is the default spelled out.
    #[test]
    fn non_citeproc_cite_method_suppresses_the_bibliography_default() {
        for engine in ["natbib", "biblatex"] {
            let mut meta = cv_map(vec![
                ("bibliography", cv_str("refs.bib")),
                ("cite-method", cv_str(engine)),
            ]);
            assert!(!apply_citeproc_default(&mut meta, HTML), "{engine}");
            assert!(meta.get("filters").is_none(), "{engine}");
        }
        let mut spelled_out = cv_map(vec![
            ("bibliography", cv_str("refs.bib")),
            ("cite-method", cv_str("citeproc")),
        ]);
        assert!(apply_citeproc_default(&mut spelled_out, HTML));
    }

    /// The margin exemption exists for Typst's Lua margin handler. HTML
    /// has no such handler, so margin mode must not leave its citations
    /// unresolved.
    #[test]
    fn margin_citations_still_get_citeproc_when_nothing_else_owns_them() {
        let mut meta = cv_map(vec![
            ("bibliography", cv_str("refs.bib")),
            ("citation-location", cv_str("margin")),
        ]);
        assert!(apply_citeproc_default(&mut meta, HTML));
        assert_eq!(filter_names(&meta), vec!["quarto", "citeproc"]);
    }

    fn meta_with_filters(filters: ConfigValue) -> ConfigValue {
        cv_map(vec![("filters", filters)])
    }

    fn doc_dir() -> PathBuf {
        PathBuf::from("/project/docs")
    }

    /// Resolve with no source context: every entry falls back to the
    /// document dir (`/project/docs`); the project root is `/project`.
    fn resolve(meta: &ConfigValue, extensions: &[Extension], rt: &TestRuntime) -> ResolvedFilters {
        let project_dir = PathBuf::from("/project");
        let document_dir = doc_dir();
        let ctx = PathCtx {
            source_context: None,
            project_dir: &project_dir,
            document_dir: &document_dir,
        };
        resolve_filters(meta, &ctx, extensions, rt)
    }

    fn no_extensions() -> Vec<Extension> {
        vec![]
    }

    fn make_extension(name: &str, filters: Vec<ExtensionFilter>) -> Extension {
        Extension {
            id: ExtensionId::new(name),
            title: Some(name.to_string()),
            author: Some("Test".to_string()),
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
        let result = resolve(&meta, &no_extensions(), &rt);
        assert!(result.pre.is_empty());
        assert!(result.post.is_empty());
    }

    #[test]
    fn missing_filters_key_returns_empty() {
        let meta = cv_map(vec![("title", cv_str("Hello"))]);
        let rt = TestRuntime::new();
        let result = resolve(&meta, &no_extensions(), &rt);
        assert!(result.pre.is_empty());
        assert!(result.post.is_empty());
    }

    #[test]
    fn empty_filters_array_returns_empty() {
        let meta = meta_with_filters(cv_array(vec![]));
        let rt = TestRuntime::new();
        let result = resolve(&meta, &no_extensions(), &rt);
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
        let result = resolve(&meta, &no_extensions(), &rt);
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
        let result = resolve(&meta, &no_extensions(), &rt);
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
        let result = resolve(&meta, &no_extensions(), &rt);
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
        let result = resolve(&meta, &no_extensions(), &rt);

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

        // book-projects P2b: `post_entry_points` preserves each post
        // filter's *specific* Q1 entry-point name, not just "it's in the
        // Post bucket" — the whole bucket is not one name.
        assert_eq!(
            result.post_entry_points,
            vec![
                "post-quarto",
                "pre-render",
                "post-render",
                "pre-finalize",
                "post-finalize",
            ]
        );
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
        let result = resolve(&meta, &no_extensions(), &rt);
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
        let result = resolve(&meta, &no_extensions(), &rt);
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
        let result = resolve(&meta, &no_extensions(), &rt);
        assert_eq!(result.pre.len(), 3);
        assert_eq!(result.pre[0], FilterSpec::Lua(doc_dir().join("first.lua")));
        assert_eq!(result.pre[1], FilterSpec::Lua(doc_dir().join("second.lua")));
        assert_eq!(result.pre[2], FilterSpec::Lua(doc_dir().join("third.lua")));
    }

    #[test]
    fn relative_paths_resolved_against_document_dir() {
        let meta = meta_with_filters(cv_array(vec![cv_str("my-filter.lua")]));
        let rt = TestRuntime::new();
        let result = resolve(&meta, &no_extensions(), &rt);
        assert_eq!(
            result.pre[0],
            FilterSpec::Lua(PathBuf::from("/project/docs/my-filter.lua"))
        );
    }

    #[test]
    fn rooted_paths_anchor_at_the_project_root() {
        // Contract rule 2: a leading `/` is the project root, never the
        // filesystem root (bd-rdcvjy2s).
        let meta = meta_with_filters(cv_array(vec![cv_str("/filters/abs.lua")]));
        let rt = TestRuntime::new();
        let result = resolve(&meta, &no_extensions(), &rt);
        assert_eq!(
            result.pre[0],
            FilterSpec::Lua(PathBuf::from("/project/filters/abs.lua"))
        );
    }

    #[test]
    fn relative_paths_resolve_against_the_declaring_file() {
        // GH #945: an entry written in `_quarto.yml` resolves against the
        // project root even when the consuming document is in `docs/`.
        let mut sc = quarto_source_map::SourceContext::new();
        let quarto_yml = sc.add_file("/project/_quarto.yml".into(), Some(String::new()));
        let span = SourceInfo::Original {
            file_id: quarto_yml,
            start_offset: 0,
            end_offset: 1,
        };
        let entry = ConfigValue::new_string("my-filter.lua", span);
        let meta = meta_with_filters(cv_array(vec![entry]));
        let project_dir = PathBuf::from("/project");
        let document_dir = doc_dir();
        let ctx = PathCtx {
            source_context: Some(&sc),
            project_dir: &project_dir,
            document_dir: &document_dir,
        };
        let result = resolve_filters(&meta, &ctx, &no_extensions(), &TestRuntime::new());
        assert_eq!(
            result.pre[0],
            FilterSpec::Lua(PathBuf::from("/project/my-filter.lua"))
        );
    }

    #[test]
    fn map_form_path_resolves_against_the_declaring_file() {
        let mut sc = quarto_source_map::SourceContext::new();
        let quarto_yml = sc.add_file("/project/_quarto.yml".into(), Some(String::new()));
        let span = SourceInfo::Original {
            file_id: quarto_yml,
            start_offset: 0,
            end_offset: 1,
        };
        let path = ConfigValue::new_string("f.lua", span);
        let meta = meta_with_filters(cv_array(vec![cv_map(vec![("path", path)])]));
        let project_dir = PathBuf::from("/project");
        let document_dir = doc_dir();
        let ctx = PathCtx {
            source_context: Some(&sc),
            project_dir: &project_dir,
            document_dir: &document_dir,
        };
        let result = resolve_filters(&meta, &ctx, &no_extensions(), &TestRuntime::new());
        assert_eq!(
            result.pre[0],
            FilterSpec::Lua(PathBuf::from("/project/f.lua"))
        );
    }

    #[test]
    fn marked_path_entry_is_joined_onto_the_project_root() {
        // An explicit `!path` (or a merge-marked entry) arrives in pivot
        // form (project-root-relative); the resolver honours that form.
        let entry = ConfigValue {
            value: quarto_pandoc_types::ConfigValueKind::Path("hello.lua".into()),
            source_info: SourceInfo::for_test(),
            merge_op: Default::default(),
        };
        let meta = meta_with_filters(cv_array(vec![entry]));
        let result = resolve(&meta, &no_extensions(), &TestRuntime::new());
        assert_eq!(
            result.pre[0],
            FilterSpec::Lua(PathBuf::from("/project/hello.lua"))
        );
    }

    #[test]
    fn no_sentinel_means_all_pre() {
        let meta = meta_with_filters(cv_array(vec![cv_str("a.lua"), cv_str("b.lua")]));
        let rt = TestRuntime::new();
        let result = resolve(&meta, &no_extensions(), &rt);
        assert_eq!(result.pre.len(), 2);
        assert_eq!(result.post.len(), 0);
    }

    #[test]
    fn citeproc_path_not_resolved() {
        let meta = meta_with_filters(cv_array(vec![cv_str("citeproc")]));
        let rt = TestRuntime::new();
        let result = resolve(&meta, &no_extensions(), &rt);
        assert_eq!(result.pre.len(), 1);
        assert_eq!(result.pre[0], FilterSpec::Citeproc);
    }

    #[test]
    fn json_filter_detected_by_extension() {
        let meta = meta_with_filters(cv_array(vec![cv_str("filter.py")]));
        let rt = TestRuntime::new();
        let result = resolve(&meta, &no_extensions(), &rt);
        assert_eq!(result.pre[0], FilterSpec::Json(doc_dir().join("filter.py")));
    }

    #[test]
    fn object_with_json_type() {
        let meta = meta_with_filters(cv_array(vec![cv_map(vec![
            ("type", cv_str("json")),
            ("path", cv_str("my-filter")),
        ])]));
        let rt = TestRuntime::new();
        let result = resolve(&meta, &no_extensions(), &rt);
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
        let result = resolve(&meta, &no_extensions(), &rt);
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
        let result = resolve(&meta, &no_extensions(), &rt);

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
        let result = resolve(&meta, &[ext], &rt);
        assert_eq!(result.pre.len(), 1);
        assert_eq!(
            result.pre[0],
            FilterSpec::Lua(PathBuf::from("/project/_extensions/lightbox/lightbox.lua"))
        );
    }

    /// Q1 `kBuiltInExtNames`: `quarto-ext/lightbox` is built in now, so a
    /// filter entry resolving to it is dropped (bd-kcpkgftw) and reported.
    #[test]
    fn superseded_quarto_ext_lightbox_is_dropped() {
        let mut ext = make_extension(
            "lightbox",
            vec![ExtensionFilter {
                path: PathBuf::from("/project/_extensions/quarto-ext/lightbox/lightbox.lua"),
                at: None,
            }],
        );
        ext.id = ExtensionId {
            name: "lightbox".to_string(),
            organization: Some("quarto-ext".to_string()),
        };
        let rt = TestRuntime::new();
        for entry in [
            cv_str("lightbox"),
            cv_str("quarto-ext/lightbox"),
            cv_map(vec![("path", cv_str("lightbox"))]),
        ] {
            let meta = meta_with_filters(cv_array(vec![entry, cv_str("other.lua")]));
            let result = resolve(&meta, std::slice::from_ref(&ext), &rt);
            assert_eq!(
                result.pre,
                [FilterSpec::Lua(doc_dir().join("other.lua"))],
                "only the superseded entry is dropped"
            );
            assert_eq!(result.superseded.len(), 1);
            assert_eq!(result.superseded[0].0, "quarto-ext/lightbox");
        }
    }

    /// A `lightbox` extension from any other organization (or none) is the
    /// user's own and still runs, as in Q1.
    #[test]
    fn other_lightbox_extensions_still_resolve() {
        let mut ext = make_extension(
            "lightbox",
            vec![ExtensionFilter {
                path: PathBuf::from("/ext/lightbox.lua"),
                at: None,
            }],
        );
        ext.id.organization = Some("someone".to_string());
        let meta = meta_with_filters(cv_array(vec![cv_str("lightbox")]));
        let rt = TestRuntime::new();
        let result = resolve(&meta, &[ext], &rt);
        assert_eq!(
            result.pre,
            [FilterSpec::Lua(PathBuf::from("/ext/lightbox.lua"))]
        );
        assert!(result.superseded.is_empty());
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
        let result = resolve(&meta, &[ext], &rt);
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
        let result = resolve(&meta, &[ext], &rt);
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
        let result = resolve(&meta, &[ext], &rt);
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
        let result = resolve(&meta, &[ext], &rt);
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
        let result = resolve(&meta, &[ext], &rt);
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
        let result = resolve(&meta, &no_extensions(), &rt);
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
        let result = resolve(&meta, &[ext], &rt);
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
        let result = resolve(&meta, &[ext], &rt);
        assert_eq!(result.pre.len(), 1);
        // Path is absolute — not joined with document_dir
        assert_eq!(
            result.pre[0],
            FilterSpec::Lua(PathBuf::from("/project/_extensions/myext/filter.lua"))
        );
    }
}
