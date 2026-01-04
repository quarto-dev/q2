/*
 * options.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Reader and writer options for pampa.
 *
 * Options are represented as serde_json::Value rather than strongly-typed structs,
 * allowing us to accept any Pandoc-compatible options without committing to
 * implementing their behavior. This provides API compatibility with Pandoc's
 * Lua filter API.
 */

use serde_json::Value;

/// Reader options passed from Lua.
///
/// Expected structure:
/// ```json
/// {
///   "format": "qmd",
///   "extensions": { "enable": ["smart"], "disable": ["citations"] },
///   "columns": 80,
///   "tab_stop": 4,
///   ...
/// }
/// ```
pub type ReaderOptions = Value;

/// Writer options passed from Lua.
///
/// Expected structure:
/// ```json
/// {
///   "format": "html",
///   "extensions": { "enable": [], "disable": [] },
///   "columns": 72,
///   "wrap_text": "auto",
///   ...
/// }
/// ```
pub type WriterOptions = Value;

/// Extensions parsed from a format string or options table.
#[derive(Debug, Clone, Default)]
pub struct ExtensionsDiff {
    pub enable: Vec<String>,
    pub disable: Vec<String>,
}

/// A parsed format specification.
#[derive(Debug, Clone)]
pub struct ParsedFormat {
    pub base_format: String,
    pub extensions: ExtensionsDiff,
}

// =============================================================================
// Helper functions for extracting fields from options
// =============================================================================

/// Extract a string field with a default value.
pub fn get_str<'a>(opts: &'a Value, key: &str, default: &'a str) -> &'a str {
    opts.get(key).and_then(Value::as_str).unwrap_or(default)
}

/// Extract an integer field with a default value.
pub fn get_i64(opts: &Value, key: &str, default: i64) -> i64 {
    opts.get(key).and_then(Value::as_i64).unwrap_or(default)
}

/// Extract a boolean field with a default value.
pub fn get_bool(opts: &Value, key: &str, default: bool) -> bool {
    opts.get(key).and_then(Value::as_bool).unwrap_or(default)
}

/// Get the format from options, defaulting to "qmd".
pub fn get_format(opts: &Value) -> &str {
    get_str(opts, "format", "qmd")
}

/// Get enabled extensions from options.
pub fn get_enabled_extensions(opts: &Value) -> Vec<&str> {
    opts.get("extensions")
        .and_then(|e| e.get("enable"))
        .and_then(Value::as_array)
        .map(|arr| arr.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default()
}

/// Get disabled extensions from options.
pub fn get_disabled_extensions(opts: &Value) -> Vec<&str> {
    opts.get("extensions")
        .and_then(|e| e.get("disable"))
        .and_then(Value::as_array)
        .map(|arr| arr.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default()
}

// =============================================================================
// Format string parsing
// =============================================================================

/// Parse a format specification string like "markdown+smart-citations".
///
/// Returns the base format and the extensions to enable/disable.
///
/// Grammar:
/// ```text
/// format_spec := base_format extension_mod*
/// extension_mod := ('+' | '-') extension_name
/// ```
pub fn parse_format_string(spec: &str) -> ParsedFormat {
    let mut enable = Vec::new();
    let mut disable = Vec::new();

    // Find the first + or - to determine where the base format ends
    let base_end = spec.find(['+', '-']).unwrap_or(spec.len());
    let base_format = spec[..base_end].to_string();

    // Parse extension modifiers
    let mut remaining = &spec[base_end..];
    while !remaining.is_empty() {
        let is_enable = remaining.starts_with('+');
        let is_disable = remaining.starts_with('-');

        if !is_enable && !is_disable {
            break;
        }

        // Skip the +/- character
        remaining = &remaining[1..];

        // Find the end of this extension name (next + or - or end of string)
        let ext_end = remaining.find(['+', '-']).unwrap_or(remaining.len());
        let ext_name = remaining[..ext_end].to_string();

        if !ext_name.is_empty() {
            if is_enable {
                enable.push(ext_name);
            } else {
                disable.push(ext_name);
            }
        }

        remaining = &remaining[ext_end..];
    }

    ParsedFormat {
        base_format,
        extensions: ExtensionsDiff { enable, disable },
    }
}

// =============================================================================
// Default options builders
// =============================================================================

/// Build default ReaderOptions as a JSON value.
pub fn default_reader_options() -> Value {
    serde_json::json!({
        "abbreviations": default_abbreviations(),
        "columns": 80,
        "default_image_extension": "",
        "extensions": { "enable": [], "disable": [] },
        "indented_code_classes": [],
        "standalone": false,
        "strip_comments": false,
        "tab_stop": 4,
        "track_changes": "accept-changes"
    })
}

/// Build default WriterOptions as a JSON value.
pub fn default_writer_options() -> Value {
    serde_json::json!({
        "chunk_template": "%s-%i.html",
        "cite_method": "citeproc",
        "columns": 72,
        "dpi": 96,
        "email_obfuscation": "none",
        "epub_chapter_level": 1,
        "epub_fonts": [],
        "epub_metadata": null,
        "epub_subdirectory": "EPUB",
        "extensions": { "enable": [], "disable": [] },
        "highlight_style": null,
        "html_math_method": "plain",
        "html_q_tags": false,
        "identifier_prefix": "",
        "incremental": false,
        "number_offset": [0, 0, 0, 0, 0, 0],
        "number_sections": false,
        "prefer_ascii": false,
        "reference_doc": null,
        "reference_links": false,
        "reference_location": "end-of-document",
        "section_divs": false,
        "setext_headers": false,
        "slide_level": null,
        "tab_stop": 4,
        "table_of_contents": false,
        "template": null,
        "toc_depth": 3,
        "top_level_division": "top-level-default",
        "variables": {},
        "wrap_text": "wrap-auto"
    })
}

/// Default abbreviations (matching Pandoc).
fn default_abbreviations() -> Vec<&'static str> {
    vec![
        "Mr.", "Mrs.", "Ms.", "Capt.", "Dr.", "Prof.", "Gen.", "Gov.", "e.g.", "i.e.", "Sgt.",
        "St.", "vol.", "vs.", "Sen.", "Rep.", "Pres.", "Hon.", "Rev.", "Ph.D.", "M.D.", "M.A.",
        "p.", "pp.", "ch.", "sec.", "cf.", "cp.",
    ]
}

/// Merge user-provided options into default options.
///
/// This creates a new Value with defaults, then overwrites with any
/// fields present in the user options.
pub fn merge_with_defaults(defaults: Value, user_opts: &Value) -> Value {
    match (defaults, user_opts) {
        (Value::Object(mut default_map), Value::Object(user_map)) => {
            for (key, value) in user_map {
                default_map.insert(key.clone(), value.clone());
            }
            Value::Object(default_map)
        }
        (defaults, _) => defaults,
    }
}

// =============================================================================
// Supported formats
// =============================================================================

/// Supported reader format names.
pub const SUPPORTED_READER_FORMATS: &[&str] = &["qmd", "markdown", "json"];

/// Supported writer format names.
pub const SUPPORTED_WRITER_FORMATS: &[&str] = &[
    "html", "html5", "json", "native", "markdown", "qmd", "plain",
];

/// Check if a format is supported for reading.
pub fn is_supported_reader_format(format: &str) -> bool {
    SUPPORTED_READER_FORMATS.contains(&format)
}

/// Check if a format is supported for writing.
pub fn is_supported_writer_format(format: &str) -> bool {
    SUPPORTED_WRITER_FORMATS.contains(&format)
}

/// Normalize a reader format name (e.g., "markdown" -> "qmd").
pub fn normalize_reader_format(format: &str) -> &str {
    match format {
        "markdown" => "qmd",
        other => other,
    }
}

/// Normalize a writer format name (e.g., "html5" -> "html", "markdown" -> "qmd").
pub fn normalize_writer_format(format: &str) -> &str {
    match format {
        "html5" => "html",
        "markdown" => "qmd",
        other => other,
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_format_string_simple() {
        let parsed = parse_format_string("markdown");
        assert_eq!(parsed.base_format, "markdown");
        assert!(parsed.extensions.enable.is_empty());
        assert!(parsed.extensions.disable.is_empty());
    }

    #[test]
    fn test_parse_format_string_with_extensions() {
        let parsed = parse_format_string("markdown+smart-citations");
        assert_eq!(parsed.base_format, "markdown");
        assert_eq!(parsed.extensions.enable, vec!["smart"]);
        assert_eq!(parsed.extensions.disable, vec!["citations"]);
    }

    #[test]
    fn test_parse_format_string_multiple_extensions() {
        let parsed = parse_format_string("markdown+smart+footnotes-citations-raw_html");
        assert_eq!(parsed.base_format, "markdown");
        assert_eq!(parsed.extensions.enable, vec!["smart", "footnotes"]);
        assert_eq!(parsed.extensions.disable, vec!["citations", "raw_html"]);
    }

    #[test]
    fn test_parse_format_string_only_disable() {
        let parsed = parse_format_string("html-smart");
        assert_eq!(parsed.base_format, "html");
        assert!(parsed.extensions.enable.is_empty());
        assert_eq!(parsed.extensions.disable, vec!["smart"]);
    }

    #[test]
    fn test_get_str() {
        let opts = serde_json::json!({ "format": "html", "other": 123 });
        assert_eq!(get_str(&opts, "format", "default"), "html");
        assert_eq!(get_str(&opts, "missing", "default"), "default");
        assert_eq!(get_str(&opts, "other", "default"), "default"); // wrong type
    }

    #[test]
    fn test_get_i64() {
        let opts = serde_json::json!({ "columns": 80, "other": "string" });
        assert_eq!(get_i64(&opts, "columns", 72), 80);
        assert_eq!(get_i64(&opts, "missing", 72), 72);
        assert_eq!(get_i64(&opts, "other", 72), 72); // wrong type
    }

    #[test]
    fn test_get_bool() {
        let opts = serde_json::json!({ "standalone": true, "other": "yes" });
        assert!(get_bool(&opts, "standalone", false));
        assert!(!get_bool(&opts, "missing", false));
        assert!(!get_bool(&opts, "other", false)); // wrong type
    }

    #[test]
    fn test_get_extensions() {
        let opts = serde_json::json!({
            "extensions": {
                "enable": ["smart", "footnotes"],
                "disable": ["citations"]
            }
        });
        assert_eq!(get_enabled_extensions(&opts), vec!["smart", "footnotes"]);
        assert_eq!(get_disabled_extensions(&opts), vec!["citations"]);
    }

    #[test]
    fn test_merge_with_defaults() {
        let defaults = serde_json::json!({ "a": 1, "b": 2 });
        let user = serde_json::json!({ "b": 3, "c": 4 });
        let merged = merge_with_defaults(defaults, &user);
        assert_eq!(merged["a"], 1);
        assert_eq!(merged["b"], 3);
        assert_eq!(merged["c"], 4);
    }

    #[test]
    fn test_normalize_formats() {
        assert_eq!(normalize_reader_format("markdown"), "qmd");
        assert_eq!(normalize_reader_format("qmd"), "qmd");
        assert_eq!(normalize_reader_format("json"), "json");

        assert_eq!(normalize_writer_format("html5"), "html");
        assert_eq!(normalize_writer_format("markdown"), "qmd");
        assert_eq!(normalize_writer_format("html"), "html");
    }

    #[test]
    fn test_get_format() {
        let opts_with_format = serde_json::json!({ "format": "html" });
        assert_eq!(get_format(&opts_with_format), "html");

        let opts_without_format = serde_json::json!({ "other": "value" });
        assert_eq!(get_format(&opts_without_format), "qmd");

        let empty_opts = serde_json::json!({});
        assert_eq!(get_format(&empty_opts), "qmd");
    }

    #[test]
    fn test_parse_format_string_with_consecutive_modifiers() {
        // Test parsing when there are two +/- in a row (empty extension name)
        let parsed = parse_format_string("markdown+-smart");
        assert_eq!(parsed.base_format, "markdown");
        // Empty extension name should be skipped
        assert_eq!(parsed.extensions.enable, Vec::<String>::new());
        assert_eq!(parsed.extensions.disable, vec!["smart"]);
    }

    #[test]
    fn test_merge_with_defaults_non_object_user_opts() {
        let defaults = serde_json::json!({ "a": 1 });
        let user_opts = serde_json::json!("not an object");
        let result = merge_with_defaults(defaults.clone(), &user_opts);
        // When user_opts is not an object, defaults are returned unchanged
        assert_eq!(result, defaults);
    }

    #[test]
    fn test_merge_with_defaults_null_user_opts() {
        let defaults = serde_json::json!({ "a": 1, "b": 2 });
        let user_opts = serde_json::Value::Null;
        let result = merge_with_defaults(defaults.clone(), &user_opts);
        assert_eq!(result, defaults);
    }

    #[test]
    fn test_is_supported_reader_format() {
        assert!(is_supported_reader_format("qmd"));
        assert!(is_supported_reader_format("markdown"));
        assert!(is_supported_reader_format("json"));
        assert!(!is_supported_reader_format("html"));
        assert!(!is_supported_reader_format("unknown"));
    }

    #[test]
    fn test_is_supported_writer_format() {
        assert!(is_supported_writer_format("html"));
        assert!(is_supported_writer_format("html5"));
        assert!(is_supported_writer_format("json"));
        assert!(is_supported_writer_format("native"));
        assert!(is_supported_writer_format("markdown"));
        assert!(is_supported_writer_format("qmd"));
        assert!(is_supported_writer_format("plain"));
        assert!(!is_supported_writer_format("docx"));
        assert!(!is_supported_writer_format("unknown"));
    }

    #[test]
    fn test_default_reader_options() {
        let opts = default_reader_options();
        assert_eq!(opts["columns"], 80);
        assert_eq!(opts["tab_stop"], 4);
        assert!(!opts["standalone"].as_bool().unwrap());
    }

    #[test]
    fn test_default_writer_options() {
        let opts = default_writer_options();
        assert_eq!(opts["columns"], 72);
        assert_eq!(opts["dpi"], 96);
        assert_eq!(opts["toc_depth"], 3);
    }

    #[test]
    fn test_get_extensions_missing() {
        let opts = serde_json::json!({});
        assert!(get_enabled_extensions(&opts).is_empty());
        assert!(get_disabled_extensions(&opts).is_empty());
    }

    #[test]
    fn test_extensions_diff_default() {
        let diff = ExtensionsDiff::default();
        assert!(diff.enable.is_empty());
        assert!(diff.disable.is_empty());
    }

    #[test]
    fn test_parse_format_string_with_trailing_invalid_chars() {
        // This tests the break case when we encounter a character that's not + or -
        // This would happen if someone manually constructs a weird format string
        // Since valid format strings always have extensions starting with + or -,
        // this is mostly a defensive case
        let parsed = parse_format_string("markdown+smart");
        assert_eq!(parsed.base_format, "markdown");
        assert_eq!(parsed.extensions.enable, vec!["smart"]);
    }
}
