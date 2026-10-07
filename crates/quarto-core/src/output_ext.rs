/*
 * output_ext.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * The `output-ext` lookup shared by the native and wasm runtimes.
 */

//! `output-ext` resolution for a format key.
//!
//! Q1 treats `output-ext` as a free string. For `typst` the default (`pdf`)
//! means "pandoc, then `typst compile`"; any other value is taken literally
//! and means "stop after pandoc, write `<stem>.<value>`".
//!
//! This is a pure, pre-pipeline lookup over two metadata layers. Precedence,
//! highest first:
//!
//! 1. document `format.<key>.output-ext`
//! 2. document top-level `output-ext`
//! 3. project `format.<key>.output-ext`
//! 4. project top-level `output-ext`
//!
//! Known gap, shared with format-key resolution: directory `_metadata.yml`,
//! included metadata and `-M` flags are not seen here (Q1 merges them).

use quarto_config::resolve_format_config;
use quarto_pandoc_types::ConfigValue;

use crate::format::extract_yaml_frontmatter;

/// The value of `output-ext` that selects the compile recipe for typst.
pub const TYPST_COMPILE_EXT: &str = "pdf";

/// A string `output-ext` in a parsed YAML mapping, if any. Non-string and
/// empty values are ignored, so a lower layer can still apply.
fn string_ext(map: &serde_yaml::Value) -> Option<String> {
    map.get("output-ext")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// The document layer: `format.<key>.output-ext`, then top-level `output-ext`.
fn document_ext(content: &str, format_key: &str) -> Option<String> {
    let yaml = extract_yaml_frontmatter(content)?;
    let value: serde_yaml::Value = serde_yaml::from_str(&yaml).ok()?;
    value
        .get("format")
        .and_then(|f| f.get(format_key))
        .and_then(string_ext)
        .or_else(|| string_ext(&value))
}

/// The project layer: the merged top-level and `format.<key>` settings.
fn project_ext(project_metadata: &ConfigValue, format_key: &str) -> Option<String> {
    resolve_format_config(project_metadata, format_key)
        .get("output-ext")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// The `output-ext` configured for `format_key`, or `None` when no layer sets one.
///
/// `content` is the document source (its leading YAML front matter is read);
/// `project_metadata` is the whole project metadata map, if any.
pub fn resolve_output_ext(
    content: &str,
    project_metadata: Option<&ConfigValue>,
    format_key: &str,
) -> Option<String> {
    document_ext(content, format_key)
        .or_else(|| project_metadata.and_then(|m| project_ext(m, format_key)))
}

/// Whether `ext` selects the typst compile recipe. `None` is the default (`pdf`).
pub fn is_typst_compile_ext(ext: Option<&str>) -> bool {
    ext.is_none_or(|e| e == TYPST_COMPILE_EXT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_pandoc_types::{ConfigMapEntry, ConfigValue};
    use quarto_source_map::SourceInfo;

    fn s(v: &str) -> ConfigValue {
        ConfigValue::new_string(v, SourceInfo::for_test())
    }

    fn map(entries: Vec<(&str, ConfigValue)>) -> ConfigValue {
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

    fn project(top: Option<&str>, per_format: Option<&str>) -> ConfigValue {
        let mut entries = Vec::new();
        if let Some(t) = top {
            entries.push(("output-ext", s(t)));
        }
        if let Some(p) = per_format {
            entries.push((
                "format",
                map(vec![("typst", map(vec![("output-ext", s(p))]))]),
            ));
        }
        map(entries)
    }

    #[test]
    fn none_when_unset() {
        assert_eq!(
            resolve_output_ext("---\nformat: typst\n---\n", None, "typst"),
            None
        );
        assert_eq!(resolve_output_ext("no front matter", None, "typst"), None);
    }

    #[test]
    fn document_per_format_beats_document_top_level() {
        let doc = "---\noutput-ext: a\nformat:\n  typst:\n    output-ext: b\n---\n";
        assert_eq!(resolve_output_ext(doc, None, "typst").as_deref(), Some("b"));
    }

    #[test]
    fn document_top_level_applies() {
        let doc = "---\noutput-ext: typ\nformat: typst\n---\n";
        assert_eq!(
            resolve_output_ext(doc, None, "typst").as_deref(),
            Some("typ")
        );
    }

    #[test]
    fn document_beats_project() {
        let doc = "---\noutput-ext: doc\n---\n";
        let p = project(Some("proj-top"), Some("proj-fmt"));
        assert_eq!(
            resolve_output_ext(doc, Some(&p), "typst").as_deref(),
            Some("doc")
        );
    }

    #[test]
    fn project_per_format_beats_project_top_level() {
        let p = project(Some("proj-top"), Some("proj-fmt"));
        assert_eq!(
            resolve_output_ext("", Some(&p), "typst").as_deref(),
            Some("proj-fmt")
        );
    }

    #[test]
    fn project_top_level_applies_to_any_format() {
        let p = project(Some("proj-top"), None);
        assert_eq!(
            resolve_output_ext("", Some(&p), "typst").as_deref(),
            Some("proj-top")
        );
    }

    #[test]
    fn project_per_format_ignored_for_other_formats() {
        let p = project(None, Some("proj-fmt"));
        assert_eq!(resolve_output_ext("", Some(&p), "html"), None);
    }

    #[test]
    fn non_string_values_fall_through_to_lower_layers() {
        let doc = "---\noutput-ext: 5\nformat:\n  typst:\n    output-ext: [a]\n---\n";
        let p = project(Some("proj-top"), None);
        assert_eq!(
            resolve_output_ext(doc, Some(&p), "typst").as_deref(),
            Some("proj-top")
        );
        assert_eq!(resolve_output_ext(doc, None, "typst"), None);
    }

    #[test]
    fn empty_string_is_ignored() {
        assert_eq!(
            resolve_output_ext("---\noutput-ext: ''\n---\n", None, "typst"),
            None
        );
    }

    #[test]
    fn compile_ext_is_pdf_or_unset() {
        assert!(is_typst_compile_ext(None));
        assert!(is_typst_compile_ext(Some("pdf")));
        assert!(!is_typst_compile_ext(Some("typ")));
        assert!(!is_typst_compile_ext(Some("foo")));
    }
}
