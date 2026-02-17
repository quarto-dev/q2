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

    // 2. Start with all top-level entries EXCEPT "format"
    let mut result_entries: Vec<ConfigMapEntry> = Vec::new();
    for entry in entries {
        if entry.key != "format" {
            result_entries.push(entry.clone());
        }
    }

    // 3. Find format key and extract target format settings
    if let Some(format_entry) = entries.iter().find(|e| e.key == "format") {
        match &format_entry.value.value {
            // Handle format: { html: { ... }, pdf: { ... } }
            ConfigValueKind::Map(format_map) => {
                if let Some(target_entry) = format_map.iter().find(|e| e.key == target_format) {
                    if let ConfigValueKind::Map(target_settings) = &target_entry.value.value {
                        // Merge target settings (override existing keys)
                        for setting in target_settings {
                            // Remove existing entry with same key
                            result_entries.retain(|e| e.key != setting.key);
                            // Add the format-specific entry
                            result_entries.push(setting.clone());
                        }
                    }
                    // If target_entry exists but isn't a map (e.g., format: { html: true }),
                    // we don't extract any settings from it
                }
            }
            // Handle format: "html" shorthand
            ConfigValueKind::Scalar(Yaml::String(s)) if s == target_format => {
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
        ConfigValue::new_scalar(Yaml::String(s.into()), SourceInfo::default())
    }

    // Helper to create a bool ConfigValue
    fn bool_val(b: bool) -> ConfigValue {
        ConfigValue::new_scalar(Yaml::Boolean(b), SourceInfo::default())
    }

    // Helper to create an int ConfigValue
    fn int_val(i: i64) -> ConfigValue {
        ConfigValue::new_scalar(Yaml::Integer(i), SourceInfo::default())
    }

    // Helper to create a map ConfigValue
    fn map(entries: Vec<(&str, ConfigValue)>) -> ConfigValue {
        let map_entries: Vec<ConfigMapEntry> = entries
            .into_iter()
            .map(|(k, v)| ConfigMapEntry {
                key: k.to_string(),
                key_source: SourceInfo::default(),
                value: v,
            })
            .collect();
        ConfigValue::new_map(map_entries, SourceInfo::default())
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
}
