//! Format-specific configuration resolution.
//!
//! This module provides functionality for extracting format-specific configuration
//! from metadata. Quarto documents can specify settings at the top level or nested
//! under `format.{format_name}`. Format-specific settings override top-level settings.
//!
//! # Example
//!
//! ```yaml
//! title: "My Document"
//! toc: true
//! format:
//!   html:
//!     toc: false       # Overrides top-level toc
//!     theme: cosmo     # HTML-specific setting
//!   pdf:
//!     documentclass: article
//! ```
//!
//! When rendering to HTML, `resolve_format_config()` will return:
//! ```yaml
//! title: "My Document"
//! toc: false           # From format.html
//! theme: cosmo         # From format.html
//! ```
//!
//! Note: `pdf` settings are ignored when rendering to HTML.

use crate::types::{ConfigMapEntry, ConfigValue, ConfigValueKind};
use yaml_rust2::Yaml;

/// Extract and flatten format-specific configuration.
///
/// Given a ConfigValue containing metadata and a target format name,
/// returns a new ConfigValue with:
/// - All top-level settings (except `format`)
/// - Format-specific settings from `format.{target}` merged on top
///
/// # Arguments
///
/// * `metadata` - The full metadata ConfigValue (from document frontmatter or _quarto.yml)
/// * `target_format` - The format name to extract settings for (e.g., "html", "pdf")
///
/// # Returns
///
/// A new ConfigValue containing the flattened configuration for the target format.
/// The `format` key is removed from the result, and format-specific settings
/// override top-level settings with the same key.
///
/// # Example
///
/// ```rust,ignore
/// let metadata = parse_yaml(r#"
/// title: "Hello"
/// toc: true
/// format:
///   html:
///     toc: false
///     theme: cosmo
/// "#);
///
/// let resolved = resolve_format_config(&metadata, "html");
/// // resolved = { title: "Hello", toc: false, theme: "cosmo" }
/// ```
///
/// # Edge Cases
///
/// - If `metadata` is not a map, returns an empty map
/// - If there's no `format` key, returns top-level settings as-is (without `format` key)
/// - If target format isn't present in `format`, returns top-level settings only
/// - `format: html` shorthand (string instead of object) is handled as `format: { html: {} }`
pub fn resolve_format_config(metadata: &ConfigValue, target_format: &str) -> ConfigValue {
    // 1. If metadata is not a map, return empty map
    let entries = match &metadata.value {
        ConfigValueKind::Map(e) => e,
        _ => return ConfigValue::new_map(vec![], metadata.source_info.clone()),
    };

    // 2. Start with all top-level entries EXCEPT "format", with bare
    //    execute options folded into `execute:` (Q1's `metadataAsFormat`).
    let top_level: Vec<ConfigMapEntry> = entries
        .iter()
        .filter(|e| e.key != "format")
        .cloned()
        .collect();
    let mut result_entries = fold_execute_options(top_level);

    // 3. Find format key and extract target format settings
    if let Some(format_entry) = entries.iter().find(|e| e.key == "format") {
        match &format_entry.value.value {
            // Handle format: { html: { ... }, pdf: { ... } }
            ConfigValueKind::Map(format_map) => {
                if let Some(target_entry) = format_map.iter().find(|e| e.key == target_format)
                    && let ConfigValueKind::Map(target_settings) = &target_entry.value.value
                {
                    // Merge target settings (override existing keys). The
                    // sublevel is folded the same way as the top level.
                    for setting in fold_execute_options(target_settings.clone()) {
                        if setting.key == "execute"
                            && let Some(merged) = merge_execute_maps(&result_entries, &setting)
                        {
                            // `execute:` merges one level deep, as in Q1:
                            // a `format.<fmt>.execute` key overrides the
                            // same top-level key and leaves the others.
                            let existing = result_entries
                                .iter_mut()
                                .find(|e| e.key == "execute")
                                .expect("merge_execute_maps found a top-level execute");
                            *existing = merged;
                            continue;
                        }
                        // Remove existing entry with same key
                        result_entries.retain(|e| e.key != setting.key);
                        // Add the format-specific entry
                        result_entries.push(setting);
                    }
                }
                // If target_entry exists but isn't a map (e.g., format: { html: true }),
                // we don't extract any settings from it
            }
            // Handle format: "html" shorthand
            ConfigValueKind::Scalar {
                yaml: Yaml::String(s),
                ..
            } if s == target_format => {
                // Shorthand matches target format, but there are no additional settings
                // to merge - just means "use this format with defaults"
            }
            // Handle format: "html" as PandocInlines (in case it was parsed as markdown)
            ConfigValueKind::PandocInlines(inlines) => {
                // Extract text from inlines and check if it matches target
                let text = inlines_to_text(inlines);
                if text.trim() == target_format {
                    // Shorthand matches, no additional settings
                }
            }
            _ => {
                // Unknown format structure, ignore
            }
        }
    }

    ConfigValue::new_map(result_entries, metadata.source_info.clone())
}

/// The execute options Quarto 1 accepts outside `execute:`.
///
/// Verbatim copy of Q1's `kExecuteDefaultsKeys`
/// (`quarto-cli/src/config/constants.ts`). Q1's `metadataAsFormat` moves
/// each of these keys, found at the top level of a metadata layer or of its
/// `format.<fmt>` map, into `format.execute`, so `fig-width: 9` in the front
/// matter means `execute: fig-width: 9` (GH #1001).
pub const EXECUTE_DEFAULTS_KEYS: &[&str] = &[
    "fig-width",
    "fig-height",
    "fig-format",
    "fig-dpi",
    "fig-asp",
    "mermaid-format",
    "df-print",
    "error",
    "eval",
    "engine",
    "cache",
    "freeze",
    "echo",
    "output",
    "warning",
    "include",
    "keep-md",
    "keep-ipynb",
    "enabled",
    "ipynb",
    "daemon",
    "daemon-restart",
    "debug",
    "ipynb-filter",
    "ipynb-filters",
    "ipynb-shell-interactivity",
    "plotly-connected",
];

/// Copy the bare execute options of one map level into its `execute:` map.
///
/// Follows Q1's `metadataAsFormat`: entries are visited in YAML order and
/// both a bare key and an `execute:` map write into the same `execute`
/// map, so on a conflict the later entry wins. A boolean `execute:` becomes
/// `enabled:`. The bare keys are also kept where they are, so code that
/// reads a top-level key (e.g. `engine`) is unaffected.
///
/// A level with no bare execute options is returned unchanged, including a
/// scalar `execute:` value.
fn fold_execute_options(entries: Vec<ConfigMapEntry>) -> Vec<ConfigMapEntry> {
    let is_bare = |e: &ConfigMapEntry| EXECUTE_DEFAULTS_KEYS.contains(&e.key.as_str());
    let Some(first_bare) = entries.iter().find(|e| is_bare(e)) else {
        return entries;
    };
    let first_bare_source = first_bare.key_source.clone();

    let mut execute: Vec<ConfigMapEntry> = Vec::new();
    let mut upsert = |entry: ConfigMapEntry| {
        execute.retain(|e| e.key != entry.key);
        execute.push(entry);
    };
    let mut original_execute: Option<&ConfigMapEntry> = None;
    for entry in &entries {
        if entry.key == "execute" {
            original_execute = Some(entry);
            match &entry.value.value {
                ConfigValueKind::Map(inner) => inner.iter().cloned().for_each(&mut upsert),
                ConfigValueKind::Scalar {
                    yaml: Yaml::Boolean(_),
                    ..
                } => upsert(ConfigMapEntry {
                    key: "enabled".to_string(),
                    key_source: entry.key_source.clone(),
                    value: entry.value.clone(),
                }),
                _ => {}
            }
        } else if is_bare(entry) {
            upsert(entry.clone());
        }
    }

    let execute_entry = match original_execute {
        Some(orig) => ConfigMapEntry {
            key: "execute".to_string(),
            key_source: orig.key_source.clone(),
            value: ConfigValue {
                value: ConfigValueKind::Map(execute),
                source_info: orig.value.source_info.clone(),
                merge_op: orig.value.merge_op,
            },
        },
        None => ConfigMapEntry {
            key: "execute".to_string(),
            key_source: first_bare_source.clone(),
            value: ConfigValue::new_map(execute, first_bare_source),
        },
    };

    let mut folded: Vec<ConfigMapEntry> = Vec::with_capacity(entries.len() + 1);
    let mut placed = false;
    for entry in entries {
        if entry.key == "execute" {
            if !placed {
                folded.push(execute_entry.clone());
                placed = true;
            }
        } else {
            folded.push(entry);
        }
    }
    if !placed {
        folded.push(execute_entry);
    }
    folded
}

/// Merge a `format.<fmt>.execute` entry into the top level's `execute` map.
///
/// Returns `None` (meaning "replace as usual") unless both values are maps
/// and the format-level map is not tagged `!prefer`.
fn merge_execute_maps(
    top_level: &[ConfigMapEntry],
    format_execute: &ConfigMapEntry,
) -> Option<ConfigMapEntry> {
    if format_execute.value.merge_op == crate::types::MergeOp::Prefer {
        return None;
    }
    let base = top_level.iter().find(|e| e.key == "execute")?;
    let ConfigValueKind::Map(base_entries) = &base.value.value else {
        return None;
    };
    let ConfigValueKind::Map(override_entries) = &format_execute.value.value else {
        return None;
    };
    let mut merged = base_entries.clone();
    for entry in override_entries {
        merged.retain(|e| e.key != entry.key);
        merged.push(entry.clone());
    }
    Some(ConfigMapEntry {
        key: "execute".to_string(),
        key_source: format_execute.key_source.clone(),
        value: ConfigValue {
            value: ConfigValueKind::Map(merged),
            source_info: format_execute.value.source_info.clone(),
            merge_op: format_execute.value.merge_op,
        },
    })
}

/// Extract plain text from Pandoc inlines.
fn inlines_to_text(inlines: &quarto_pandoc_types::Inlines) -> String {
    use quarto_pandoc_types::Inline;
    let mut text = String::new();
    for inline in inlines {
        match inline {
            Inline::Str(s) => text.push_str(&s.text),
            Inline::Space(_) => text.push(' '),
            _ => {}
        }
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_source_map::SourceInfo;

    // Helper to create a scalar ConfigValue
    fn scalar(s: &str) -> ConfigValue {
        ConfigValue::new_scalar(Yaml::String(s.into()), SourceInfo::for_test())
    }

    // Helper to create a bool ConfigValue
    fn bool_val(b: bool) -> ConfigValue {
        ConfigValue::new_scalar(Yaml::Boolean(b), SourceInfo::for_test())
    }

    // Helper to create an int ConfigValue
    fn int_val(i: i64) -> ConfigValue {
        ConfigValue::new_scalar(Yaml::Integer(i), SourceInfo::for_test())
    }

    // Helper to create a map ConfigValue
    fn map(entries: Vec<(&str, ConfigValue)>) -> ConfigValue {
        let map_entries: Vec<ConfigMapEntry> = entries
            .into_iter()
            .map(|(k, v)| ConfigMapEntry {
                key: k.to_string(),
                key_source: SourceInfo::for_test(),
                value: v,
            })
            .collect();
        ConfigValue::new_map(map_entries, SourceInfo::for_test())
    }

    #[test]
    fn test_resolve_format_top_level_only() {
        // Input: { title: "Hello", toc: true }
        // Target: "html"
        // Output: { title: "Hello", toc: true }
        let metadata = map(vec![("title", scalar("Hello")), ("toc", bool_val(true))]);

        let result = resolve_format_config(&metadata, "html");

        assert!(result.is_map());
        assert_eq!(result.get("title").unwrap().as_str(), Some("Hello"));
        assert_eq!(result.get("toc").unwrap().as_bool(), Some(true));
        assert!(result.get("format").is_none()); // format key removed
    }

    #[test]
    fn test_resolve_format_specific_overrides_top_level() {
        // Input: { toc: true, format: { html: { toc: false } } }
        // Target: "html"
        // Output: { toc: false }
        let metadata = map(vec![
            ("toc", bool_val(true)),
            (
                "format",
                map(vec![("html", map(vec![("toc", bool_val(false))]))]),
            ),
        ]);

        let result = resolve_format_config(&metadata, "html");

        assert_eq!(result.get("toc").unwrap().as_bool(), Some(false));
        assert!(result.get("format").is_none());
    }

    #[test]
    fn test_resolve_format_merges_non_overlapping() {
        // Input: { title: "Hello", format: { html: { theme: "cosmo" } } }
        // Target: "html"
        // Output: { title: "Hello", theme: "cosmo" }
        let metadata = map(vec![
            ("title", scalar("Hello")),
            (
                "format",
                map(vec![("html", map(vec![("theme", scalar("cosmo"))]))]),
            ),
        ]);

        let result = resolve_format_config(&metadata, "html");

        assert_eq!(result.get("title").unwrap().as_str(), Some("Hello"));
        assert_eq!(result.get("theme").unwrap().as_str(), Some("cosmo"));
        assert!(result.get("format").is_none());
    }

    #[test]
    fn test_resolve_format_nested_objects() {
        // Input: { format: { html: { code-fold: true, code-tools: { source: true } } } }
        // Target: "html"
        // Output: { code-fold: true, code-tools: { source: true } }
        let metadata = map(vec![(
            "format",
            map(vec![(
                "html",
                map(vec![
                    ("code-fold", bool_val(true)),
                    ("code-tools", map(vec![("source", bool_val(true))])),
                ]),
            )]),
        )]);

        let result = resolve_format_config(&metadata, "html");

        assert_eq!(result.get("code-fold").unwrap().as_bool(), Some(true));
        let code_tools = result.get("code-tools").unwrap();
        assert_eq!(code_tools.get("source").unwrap().as_bool(), Some(true));
    }

    #[test]
    fn test_resolve_format_missing_target() {
        // Input: { title: "Hello", format: { pdf: { documentclass: "article" } } }
        // Target: "html"
        // Output: { title: "Hello" }  // pdf settings ignored
        let metadata = map(vec![
            ("title", scalar("Hello")),
            (
                "format",
                map(vec![(
                    "pdf",
                    map(vec![("documentclass", scalar("article"))]),
                )]),
            ),
        ]);

        let result = resolve_format_config(&metadata, "html");

        assert_eq!(result.get("title").unwrap().as_str(), Some("Hello"));
        assert!(result.get("documentclass").is_none());
        assert!(result.get("format").is_none());
    }

    #[test]
    fn test_resolve_format_shorthand_string() {
        // Input: { format: "html" }  // shorthand, not object
        // Target: "html"
        // Output: {}  // format: html means html with defaults
        let metadata = map(vec![("format", scalar("html"))]);

        let result = resolve_format_config(&metadata, "html");

        // Just empty (no settings beyond the shorthand)
        assert!(result.is_map());
        assert!(result.get("format").is_none());
    }

    #[test]
    fn test_resolve_format_shorthand_non_matching() {
        // Input: { title: "Hello", format: "pdf" }
        // Target: "html"
        // Output: { title: "Hello" }
        let metadata = map(vec![("title", scalar("Hello")), ("format", scalar("pdf"))]);

        let result = resolve_format_config(&metadata, "html");

        assert_eq!(result.get("title").unwrap().as_str(), Some("Hello"));
        assert!(result.get("format").is_none());
    }

    #[test]
    fn test_resolve_format_multiple_formats() {
        // Input: { format: { html: { theme: "cosmo" }, pdf: { documentclass: "article" } } }
        // Target: "html"
        // Output: { theme: "cosmo" }  // only html settings
        let metadata = map(vec![(
            "format",
            map(vec![
                ("html", map(vec![("theme", scalar("cosmo"))])),
                ("pdf", map(vec![("documentclass", scalar("article"))])),
            ]),
        )]);

        let result = resolve_format_config(&metadata, "html");

        assert_eq!(result.get("theme").unwrap().as_str(), Some("cosmo"));
        assert!(result.get("documentclass").is_none());
    }

    #[test]
    fn test_resolve_format_empty_target() {
        // Input: { title: "Hello", format: { html: {} } }
        // Target: "html"
        // Output: { title: "Hello" }
        let metadata = map(vec![
            ("title", scalar("Hello")),
            ("format", map(vec![("html", map(vec![]))])),
        ]);

        let result = resolve_format_config(&metadata, "html");

        assert_eq!(result.get("title").unwrap().as_str(), Some("Hello"));
        assert!(result.get("format").is_none());
    }

    #[test]
    fn test_resolve_format_non_map_metadata() {
        // Input is a scalar, not a map
        let metadata = scalar("not a map");

        let result = resolve_format_config(&metadata, "html");

        assert!(result.is_map());
        assert!(result.is_empty());
    }

    #[test]
    fn test_resolve_format_complex_override() {
        // Test the example from the plan
        // Input: { title: "Project", toc: true, format: { html: { toc-depth: 3, theme: "cosmo" } } }
        // Target: "html"
        // Output: { title: "Project", toc: true, toc-depth: 3, theme: "cosmo" }
        let metadata = map(vec![
            ("title", scalar("Project")),
            ("toc", bool_val(true)),
            (
                "format",
                map(vec![(
                    "html",
                    map(vec![("toc-depth", int_val(3)), ("theme", scalar("cosmo"))]),
                )]),
            ),
        ]);

        let result = resolve_format_config(&metadata, "html");

        assert_eq!(result.get("title").unwrap().as_str(), Some("Project"));
        assert_eq!(result.get("toc").unwrap().as_bool(), Some(true));
        assert_eq!(result.get("toc-depth").unwrap().as_int(), Some(3));
        assert_eq!(result.get("theme").unwrap().as_str(), Some("cosmo"));
    }

    #[test]
    fn test_resolve_format_preserves_source_info() {
        use quarto_source_map::FileId;

        // Create metadata with specific source info
        let source = SourceInfo::original(FileId(42), 10, 50);
        let mut metadata = map(vec![("title", scalar("Hello"))]);
        metadata.source_info = source;

        let result = resolve_format_config(&metadata, "html");

        // Result should preserve the source info from the original metadata
        match &result.source_info {
            SourceInfo::Original {
                file_id,
                start_offset,
                end_offset,
            } => {
                assert_eq!(*file_id, FileId(42));
                assert_eq!(*start_offset, 10);
                assert_eq!(*end_offset, 50);
            }
            _ => panic!("Expected Original source info"),
        }
    }

    #[test]
    fn test_resolve_format_format_value_not_map() {
        // Input: { format: { html: true } } - html value is bool, not map
        // Target: "html"
        // Should handle gracefully
        let metadata = map(vec![("format", map(vec![("html", bool_val(true))]))]);

        let result = resolve_format_config(&metadata, "html");

        // Should be empty since html value isn't a map with settings
        assert!(result.is_map());
        assert!(result.get("format").is_none());
    }

    // ---- GH #1001: execute options outside `execute:` ----

    fn exec<'a>(result: &'a ConfigValue, key: &str) -> Option<&'a ConfigValue> {
        result.get("execute").and_then(|e| e.get(key))
    }

    #[test]
    fn test_bare_execute_option_folds_into_execute() {
        // fig-format: svg  →  execute: { fig-format: svg }, bare key kept
        let metadata = map(vec![
            ("title", scalar("t")),
            ("fig-format", scalar("svg")),
            ("echo", bool_val(false)),
        ]);
        let result = resolve_format_config(&metadata, "html");
        assert_eq!(exec(&result, "fig-format").unwrap().as_str(), Some("svg"));
        assert_eq!(exec(&result, "echo").unwrap().as_bool(), Some(false));
        assert_eq!(result.get("fig-format").unwrap().as_str(), Some("svg"));
        assert!(
            exec(&result, "title").is_none(),
            "non-execute keys stay out"
        );
    }

    #[test]
    fn test_bare_execute_option_merges_with_execute_map() {
        let metadata = map(vec![
            ("execute", map(vec![("echo", bool_val(false))])),
            ("fig-format", scalar("svg")),
        ]);
        let result = resolve_format_config(&metadata, "html");
        assert_eq!(exec(&result, "echo").unwrap().as_bool(), Some(false));
        assert_eq!(exec(&result, "fig-format").unwrap().as_str(), Some("svg"));
    }

    #[test]
    fn test_same_level_conflict_later_entry_wins() {
        // Q1 visits keys in YAML order; the later one wins either way.
        let execute_last = map(vec![
            ("fig-format", scalar("svg")),
            ("execute", map(vec![("fig-format", scalar("pdf"))])),
        ]);
        let result = resolve_format_config(&execute_last, "html");
        assert_eq!(exec(&result, "fig-format").unwrap().as_str(), Some("pdf"));

        let bare_last = map(vec![
            ("execute", map(vec![("fig-format", scalar("pdf"))])),
            ("fig-format", scalar("svg")),
        ]);
        let result = resolve_format_config(&bare_last, "html");
        assert_eq!(exec(&result, "fig-format").unwrap().as_str(), Some("svg"));
    }

    #[test]
    fn test_format_level_bare_execute_option_folds() {
        let metadata = map(vec![(
            "format",
            map(vec![("html", map(vec![("fig-format", scalar("svg"))]))]),
        )]);
        let result = resolve_format_config(&metadata, "html");
        assert_eq!(exec(&result, "fig-format").unwrap().as_str(), Some("svg"));
        // Not for another target.
        let pdf = resolve_format_config(&metadata, "pdf");
        assert!(pdf.get("execute").is_none());
    }

    #[test]
    fn test_format_level_execute_option_beats_top_level_execute_map() {
        // execute: { fig-format: png }; format.html: fig-format: svg  → svg
        let metadata = map(vec![
            ("execute", map(vec![("fig-format", scalar("png"))])),
            (
                "format",
                map(vec![("html", map(vec![("fig-format", scalar("svg"))]))]),
            ),
        ]);
        let result = resolve_format_config(&metadata, "html");
        assert_eq!(exec(&result, "fig-format").unwrap().as_str(), Some("svg"));
    }

    #[test]
    fn test_format_level_execute_map_deep_merges_with_top_level() {
        // Top-level fig-format: svg and format.html.execute.echo: false
        // must both survive (previously format.html.execute replaced the
        // top-level execute map wholesale).
        let metadata = map(vec![
            ("fig-format", scalar("svg")),
            (
                "format",
                map(vec![(
                    "html",
                    map(vec![("execute", map(vec![("echo", bool_val(false))]))]),
                )]),
            ),
        ]);
        let result = resolve_format_config(&metadata, "html");
        assert_eq!(exec(&result, "fig-format").unwrap().as_str(), Some("svg"));
        assert_eq!(exec(&result, "echo").unwrap().as_bool(), Some(false));
    }

    #[test]
    fn test_format_level_execute_map_tagged_prefer_replaces() {
        let mut preferred = map(vec![("echo", bool_val(false))]);
        preferred.merge_op = crate::types::MergeOp::Prefer;
        let metadata = map(vec![
            ("execute", map(vec![("fig-format", scalar("svg"))])),
            (
                "format",
                map(vec![("html", map(vec![("execute", preferred)]))]),
            ),
        ]);
        let result = resolve_format_config(&metadata, "html");
        assert!(exec(&result, "fig-format").is_none());
        assert_eq!(exec(&result, "echo").unwrap().as_bool(), Some(false));
    }

    #[test]
    fn test_boolean_execute_becomes_enabled_when_folding() {
        let metadata = map(vec![
            ("execute", bool_val(false)),
            ("echo", bool_val(false)),
        ]);
        let result = resolve_format_config(&metadata, "html");
        assert_eq!(exec(&result, "enabled").unwrap().as_bool(), Some(false));
        assert_eq!(exec(&result, "echo").unwrap().as_bool(), Some(false));
    }

    #[test]
    fn test_no_bare_execute_options_leaves_scalar_execute_untouched() {
        let metadata = map(vec![("execute", bool_val(false)), ("title", scalar("t"))]);
        let result = resolve_format_config(&metadata, "html");
        assert_eq!(result.get("execute").unwrap().as_bool(), Some(false));
    }
}
