/*
 * engine/knitr/types.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Request/response types for knitr R communication.
 */

//! Request and response types for knitr R subprocess communication.
//!
//! These types define the JSON protocol between Rust and the R scripts.
//! The request is sent via stdin, and the response is read from a temp file.
//!
//! # Request Format
//!
//! ```json
//! {
//!   "action": "execute",
//!   "params": {
//!     "input": "/path/to/doc.qmd",
//!     "markdown": "# Hello\n\n```{r}\n1+1\n```",
//!     "format": { ... },
//!     "tempDir": "/tmp/quarto-xxx",
//!     "resourceDir": "/path/to/resources",
//!     "handledLanguages": ["ojs", "mermaid"]
//!   },
//!   "results": "/tmp/r-results-xxx.json",
//!   "wd": "/project"
//! }
//! ```
//!
//! # Response Format
//!
//! ```json
//! {
//!   "engine": "knitr",
//!   "markdown": "# Hello\n\n::: {.cell}\n...\n:::",
//!   "supporting": ["/path/to/doc_files"],
//!   "filters": ["rmarkdown/pagebreak.lua"],
//!   "includes": { "include-in-header": "/tmp/header.html" },
//!   "postProcess": false
//! }
//! ```

use std::path::PathBuf;

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

use super::format::KnitrFormatConfig;

/// Parameters for the knitr execute action.
///
/// This is serialized to JSON and sent to R via stdin.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KnitrExecuteParams {
    /// Path to the input document
    pub input: PathBuf,

    /// Markdown content (with YAML frontmatter removed and inline R resolved)
    pub markdown: String,

    /// Format configuration
    pub format: KnitrFormatConfig,

    /// Directory for temporary files
    pub temp_dir: PathBuf,

    /// Library directory for output files (e.g., lib/ for self-contained: false)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lib_dir: Option<PathBuf>,

    /// Whether to compute dependencies
    pub dependencies: bool,

    /// Current working directory
    pub cwd: PathBuf,

    /// Document parameters (from YAML params key)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,

    /// Path to Quarto resources directory
    pub resource_dir: PathBuf,

    /// Languages handled by Quarto (pass-through, don't execute)
    pub handled_languages: Vec<String>,
}

/// Result from the knitr execute action.
///
/// This is read from the JSON results file after R execution.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KnitrExecuteResult {
    /// Engine name (always "knitr")
    #[allow(dead_code)]
    pub engine: String,

    /// Processed markdown output
    pub markdown: String,

    /// Supporting files/directories (e.g., doc_files/)
    #[serde(default)]
    pub supporting: Vec<String>,

    /// Pandoc filters to apply
    #[serde(default)]
    pub filters: Vec<String>,

    /// Include files for Pandoc
    #[serde(default, deserialize_with = "deserialize_includes")]
    pub includes: Option<KnitrIncludes>,

    /// Engine-specific dependencies (e.g., htmlwidgets)
    #[serde(default)]
    pub engine_dependencies: Option<Value>,

    /// Content to preserve during post-processing
    #[serde(default)]
    pub preserve: Option<Value>,

    /// Whether post-processing is needed
    #[serde(default)]
    pub post_process: bool,
}

/// Include files from knitr execution.
///
/// These are file paths to content that should be included in specific
/// locations of the final document.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct KnitrIncludes {
    /// Content to include in document header (e.g., CSS, JS)
    #[serde(default)]
    pub include_in_header: Option<PathBuf>,

    /// Content to include before body
    #[serde(default)]
    pub include_before_body: Option<PathBuf>,

    /// Content to include after body
    #[serde(default)]
    pub include_after_body: Option<PathBuf>,
}

/// Custom deserializer that handles both `{}` and `[]` for includes.
///
/// The R scripts sometimes return an empty array `[]` instead of an empty
/// object `{}` when there are no includes. This deserializer handles both cases.
fn deserialize_includes<'de, D>(deserializer: D) -> Result<Option<KnitrIncludes>, D::Error>
where
    D: Deserializer<'de>,
{
    use serde::de::Error;

    let value: Value = Deserialize::deserialize(deserializer)?;

    match value {
        // null -> None
        Value::Null => Ok(None),

        // Empty array [] -> None (quirk from R)
        Value::Array(arr) if arr.is_empty() => Ok(None),

        // Non-empty array is an error
        Value::Array(_) => Err(D::Error::custom(
            "expected object or empty array for includes, got non-empty array",
        )),

        // Object -> deserialize as KnitrIncludes
        Value::Object(_) => {
            let includes: KnitrIncludes =
                serde_json::from_value(value).map_err(D::Error::custom)?;

            // If all fields are None, return None
            if includes.include_in_header.is_none()
                && includes.include_before_body.is_none()
                && includes.include_after_body.is_none()
            {
                Ok(None)
            } else {
                Ok(Some(includes))
            }
        }

        // Other types are errors
        _ => Err(D::Error::custom(
            "expected object, array, or null for includes",
        )),
    }
}

/// Request wrapper sent to R via stdin.
///
/// This wraps the action-specific params with metadata needed by rmd.R.
#[derive(Debug, Clone, Serialize)]
pub struct KnitrRequest<T: Serialize> {
    /// Action to perform ("execute", "dependencies", etc.)
    pub action: String,

    /// Action-specific parameters
    pub params: T,

    /// Path to write results JSON
    pub results: PathBuf,

    /// Working directory for R
    pub wd: PathBuf,
}

impl<T: Serialize> KnitrRequest<T> {
    /// Create a new request.
    pub fn new(action: impl Into<String>, params: T, results: PathBuf, wd: PathBuf) -> Self {
        Self {
            action: action.into(),
            params,
            results,
            wd,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_execute_params_serialization() {
        let params = KnitrExecuteParams {
            input: PathBuf::from("/project/doc.qmd"),
            markdown: "# Hello".to_string(),
            format: KnitrFormatConfig::new("html"),
            temp_dir: PathBuf::from("/tmp/quarto"),
            lib_dir: None,
            dependencies: true,
            cwd: PathBuf::from("/project"),
            params: None,
            resource_dir: PathBuf::from("/usr/share/quarto"),
            handled_languages: vec!["ojs".to_string(), "mermaid".to_string()],
        };

        let json = serde_json::to_string(&params).unwrap();

        assert!(json.contains("\"input\":\"/project/doc.qmd\""));
        assert!(json.contains("\"markdown\":\"# Hello\""));
        assert!(json.contains("\"tempDir\":\"/tmp/quarto\""));
        assert!(json.contains("\"dependencies\":true"));
        assert!(json.contains("\"handledLanguages\":[\"ojs\",\"mermaid\"]"));

        // lib_dir is None, should not appear
        assert!(!json.contains("\"libDir\""));
    }

    #[test]
    fn test_execute_result_deserialization() {
        let json = r##"{
            "engine": "knitr",
            "markdown": "# Output",
            "supporting": ["/path/to/doc_files"],
            "filters": ["rmarkdown/pagebreak.lua"],
            "includes": {},
            "postProcess": false
        }"##;

        let result: KnitrExecuteResult = serde_json::from_str(json).unwrap();

        assert_eq!(result.engine, "knitr");
        assert_eq!(result.markdown, "# Output");
        assert_eq!(result.supporting, vec!["/path/to/doc_files"]);
        assert_eq!(result.filters, vec!["rmarkdown/pagebreak.lua"]);
        assert!(!result.post_process);
    }

    #[test]
    fn test_includes_empty_object() {
        let json = r#"{
            "engine": "knitr",
            "markdown": "test",
            "includes": {}
        }"#;

        let result: KnitrExecuteResult = serde_json::from_str(json).unwrap();
        assert!(result.includes.is_none());
    }

    #[test]
    fn test_includes_empty_array() {
        // R sometimes returns [] instead of {}
        let json = r#"{
            "engine": "knitr",
            "markdown": "test",
            "includes": []
        }"#;

        let result: KnitrExecuteResult = serde_json::from_str(json).unwrap();
        assert!(result.includes.is_none());
    }

    #[test]
    fn test_includes_null() {
        let json = r#"{
            "engine": "knitr",
            "markdown": "test",
            "includes": null
        }"#;

        let result: KnitrExecuteResult = serde_json::from_str(json).unwrap();
        assert!(result.includes.is_none());
    }

    #[test]
    fn test_includes_with_values() {
        let json = r#"{
            "engine": "knitr",
            "markdown": "test",
            "includes": {
                "include-in-header": "/tmp/header.html",
                "include-before-body": "/tmp/before.html"
            }
        }"#;

        let result: KnitrExecuteResult = serde_json::from_str(json).unwrap();
        let includes = result.includes.unwrap();

        assert_eq!(
            includes.include_in_header,
            Some(PathBuf::from("/tmp/header.html"))
        );
        assert_eq!(
            includes.include_before_body,
            Some(PathBuf::from("/tmp/before.html"))
        );
        assert!(includes.include_after_body.is_none());
    }

    #[test]
    fn test_includes_kebab_case() {
        let json = r#"{
            "engine": "knitr",
            "markdown": "test",
            "includes": {
                "include-in-header": "/tmp/h.html",
                "include-before-body": "/tmp/b.html",
                "include-after-body": "/tmp/a.html"
            }
        }"#;

        let result: KnitrExecuteResult = serde_json::from_str(json).unwrap();
        let includes = result.includes.unwrap();

        assert!(includes.include_in_header.is_some());
        assert!(includes.include_before_body.is_some());
        assert!(includes.include_after_body.is_some());
    }

    #[test]
    fn test_execute_result_missing_optional_fields() {
        // Minimal response
        let json = r##"{
            "engine": "knitr",
            "markdown": "# Output"
        }"##;

        let result: KnitrExecuteResult = serde_json::from_str(json).unwrap();

        assert_eq!(result.markdown, "# Output");
        assert!(result.supporting.is_empty());
        assert!(result.filters.is_empty());
        assert!(result.includes.is_none());
        assert!(!result.post_process);
    }

    #[test]
    fn test_knitr_request_serialization() {
        let params = KnitrExecuteParams {
            input: PathBuf::from("/doc.qmd"),
            markdown: "test".to_string(),
            format: KnitrFormatConfig::new("html"),
            temp_dir: PathBuf::from("/tmp"),
            lib_dir: None,
            dependencies: true,
            cwd: PathBuf::from("/project"),
            params: None,
            resource_dir: PathBuf::from("/resources"),
            handled_languages: vec![],
        };

        let request = KnitrRequest::new(
            "execute",
            params,
            PathBuf::from("/tmp/results.json"),
            PathBuf::from("/project"),
        );

        let json = serde_json::to_string(&request).unwrap();

        assert!(json.contains("\"action\":\"execute\""));
        assert!(json.contains("\"results\":\"/tmp/results.json\""));
        assert!(json.contains("\"wd\":\"/project\""));
        assert!(json.contains("\"params\":{"));
    }

    #[test]
    fn test_engine_dependencies_preserved() {
        let json = r#"{
            "engine": "knitr",
            "markdown": "test",
            "engineDependencies": {
                "htmlwidgets": {
                    "version": "1.5.4"
                }
            }
        }"#;

        let result: KnitrExecuteResult = serde_json::from_str(json).unwrap();

        assert!(result.engine_dependencies.is_some());
        let deps = result.engine_dependencies.unwrap();
        assert!(deps.get("htmlwidgets").is_some());
    }

    #[test]
    fn test_preserve_field() {
        let json = r#"{
            "engine": "knitr",
            "markdown": "test",
            "preserve": {
                "uuid1": "<div>preserved content</div>"
            },
            "postProcess": true
        }"#;

        let result: KnitrExecuteResult = serde_json::from_str(json).unwrap();

        assert!(result.preserve.is_some());
        assert!(result.post_process);
    }
}
