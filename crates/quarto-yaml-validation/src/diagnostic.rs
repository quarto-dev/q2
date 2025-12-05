//! Validation diagnostic with structured error information.
//!
//! This module provides `ValidationDiagnostic`, a wrapper around `DiagnosticMessage`
//! that preserves all validation-specific structure (instance paths, schema paths,
//! source ranges) for machine-readable JSON output while delegating text rendering
//! to `DiagnosticMessage`.

use crate::error::ValidationError;
use quarto_error_reporting::{DiagnosticMessage, DiagnosticMessageBuilder};
use quarto_source_map::{SourceContext, SourceInfo};
use serde::{Deserialize, Serialize};

/// A validation diagnostic with structured error information.
///
/// This type preserves all validation-specific structure (instance paths,
/// schema paths, source ranges) while delegating rendering to DiagnosticMessage.
///
/// # Example
///
/// ```ignore
/// let vd = ValidationDiagnostic::from_validation_error(&error, &source_ctx);
///
/// // Machine-readable JSON
/// println!("{}", serde_json::to_string_pretty(&vd.to_json())?);
///
/// // Human-readable text with ariadne
/// eprintln!("{}", vd.to_text(&source_ctx));
/// ```
#[derive(Debug, Clone)]
pub struct ValidationDiagnostic {
    /// Structured error kind - machine readable
    pub kind: crate::error::ValidationErrorKind,

    /// The validation error code (Q-1-xxx)
    pub code: String,

    /// Path through the YAML instance where the error occurred
    /// Example: ["format", "html", "toc"]
    pub instance_path: Vec<PathSegment>,

    /// Path through the schema that was being validated
    /// Example: ["properties", "format", "properties", "html", "properties", "toc"]
    pub schema_path: Vec<String>,

    /// Source location with filename and byte offsets/line numbers
    pub source_range: Option<SourceRange>,

    /// Internal: DiagnosticMessage for text rendering
    diagnostic: DiagnosticMessage,
}

/// A segment in an instance path (object key or array index)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value")]
pub enum PathSegment {
    /// Object property key
    Key(String),
    /// Array index
    Index(usize),
}

/// Source range with filename and both offset and line/column positions
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceRange {
    /// Filename (human-readable, not a file_id)
    pub filename: String,

    /// Start byte offset in the file
    pub start_offset: usize,

    /// End byte offset in the file
    pub end_offset: usize,

    /// Start line number (1-indexed)
    pub start_line: usize,

    /// Start column number (1-indexed)
    pub start_column: usize,

    /// End line number (1-indexed)
    pub end_line: usize,

    /// End column number (1-indexed)
    pub end_column: usize,
}

impl ValidationDiagnostic {
    /// Get human-readable message (lazily generated from kind)
    pub fn message(&self) -> String {
        self.kind.message()
    }

    /// Get hints (lazily generated from kind)
    pub fn hints(&self) -> Vec<String> {
        Self::suggest_fixes_from_kind(&self.kind)
    }

    /// Create a new ValidationDiagnostic from a ValidationError
    ///
    /// # Arguments
    ///
    /// * `error` - The validation error to convert
    /// * `source_ctx` - Source context for resolving file names and line/column positions
    ///
    /// # Example
    ///
    /// ```ignore
    /// let error = ValidationError::new("Expected number, got string", path);
    /// let vd = ValidationDiagnostic::from_validation_error(&error, &source_ctx);
    /// ```
    pub fn from_validation_error(error: &ValidationError, source_ctx: &SourceContext) -> Self {
        // Build the diagnostic message for text rendering
        let diagnostic = Self::build_diagnostic_message(error, source_ctx);

        // Extract source range with filename
        let source_range = error
            .yaml_node
            .as_ref()
            .and_then(|node| Self::extract_source_range(&node.source_info, source_ctx));

        // Convert instance path segments
        let instance_path = error
            .instance_path
            .segments()
            .iter()
            .map(|seg| match seg {
                crate::error::PathSegment::Key(k) => PathSegment::Key(k.clone()),
                crate::error::PathSegment::Index(i) => PathSegment::Index(*i),
            })
            .collect();

        Self {
            kind: error.kind.clone(),
            code: error.error_code().to_string(),
            instance_path,
            schema_path: error.schema_path.segments().to_vec(),
            source_range,
            diagnostic,
        }
    }

    /// Render as JSON for machine consumption
    ///
    /// # Example
    ///
    /// ```ignore
    /// let json = vd.to_json();
    /// println!("{}", serde_json::to_string_pretty(&json)?);
    /// ```
    pub fn to_json(&self) -> serde_json::Value {
        use serde_json::json;

        let mut obj = json!({
            "error_kind": self.kind,  // Structured, machine-readable
            "code": self.code,
            "instance_path": self.instance_path,
            "schema_path": self.schema_path,
        });

        if let Some(range) = &self.source_range {
            obj["source_range"] = json!(range);
        }

        // Include human-readable fields for convenience
        obj["message"] = json!(self.kind.message());

        let hints = Self::suggest_fixes_from_kind(&self.kind);
        if !hints.is_empty() {
            obj["hints"] = json!(hints);
        }

        obj
    }

    /// Render as text for human consumption (uses ariadne/tidyverse)
    ///
    /// # Example
    ///
    /// ```ignore
    /// let text = vd.to_text(&source_ctx);
    /// eprintln!("{}", text);
    /// ```
    pub fn to_text(&self, source_ctx: &SourceContext) -> String {
        self.diagnostic.to_text(Some(source_ctx))
    }

    /// Helper: Build DiagnosticMessage for text rendering
    fn build_diagnostic_message(
        error: &ValidationError,
        _source_ctx: &SourceContext,
    ) -> DiagnosticMessage {
        let mut builder = DiagnosticMessageBuilder::error("YAML Validation Failed")
            .with_code(error.error_code())
            .problem(error.message());

        // Attach full SourceInfo for ariadne rendering
        if let Some(yaml_node) = &error.yaml_node {
            builder = builder.with_location(yaml_node.source_info.clone());
        }

        // Add human-readable details
        if !error.instance_path.is_empty() {
            builder = builder.add_detail(format!("At document path: `{}`", error.instance_path));
        } else {
            builder = builder.add_detail("At document root");
        }

        if !error.schema_path.is_empty() {
            builder = builder.add_info(format!("Schema constraint: {}", error.schema_path));
        }

        // Add hints
        for hint in Self::suggest_fixes_from_kind(&error.kind) {
            builder = builder.add_hint(hint);
        }

        builder.build()
    }

    /// Helper: Extract SourceRange from SourceInfo
    fn extract_source_range(
        source_info: &SourceInfo,
        source_ctx: &SourceContext,
    ) -> Option<SourceRange> {
        // Map the start of the range (offset 0 in SourceInfo coordinates)
        // This will handle Substring/Concat/Original correctly
        let start_mapped = source_info.map_offset(0, source_ctx)?;

        // Map the end of the range (length in SourceInfo coordinates)
        // For SourceInfo, the end offset is relative to the same base as start_offset
        let length = source_info.end_offset() - source_info.start_offset();
        let end_mapped = source_info.map_offset(length, source_ctx)?;

        // Get filename
        let file = source_ctx.get_file(start_mapped.file_id)?;

        Some(SourceRange {
            filename: file.path.clone(),
            start_offset: source_info.start_offset(),
            end_offset: source_info.end_offset(),
            start_line: start_mapped.location.row + 1, // 1-indexed
            start_column: start_mapped.location.column + 1, // 1-indexed
            end_line: end_mapped.location.row + 1,
            end_column: end_mapped.location.column + 1,
        })
    }

    // No longer needed - error codes come from ValidationErrorKind::error_code()

    /// Suggest fixes based on error kind
    fn suggest_fixes_from_kind(kind: &crate::error::ValidationErrorKind) -> Vec<String> {
        use crate::error::ValidationErrorKind;
        let mut hints = Vec::new();

        match kind {
            ValidationErrorKind::MissingRequiredProperty { property } => {
                hints.push(format!(
                    "Add the `{}` property to your YAML document?",
                    property
                ));
            }
            ValidationErrorKind::TypeMismatch { expected, .. } => match expected.as_str() {
                "boolean" => {
                    hints.push("Use `true` or `false` (YAML 1.2 standard)?".to_string());
                }
                "number" => {
                    hints.push("Use a numeric value without quotes?".to_string());
                }
                "string" => {
                    hints.push(
                        "Ensure the value is a string (quoted if it contains special characters)?"
                            .to_string(),
                    );
                }
                "array" => {
                    hints.push(
                        "Use YAML array syntax: `[item1, item2]` or list format?".to_string(),
                    );
                }
                "object" => {
                    hints.push("Use YAML mapping syntax with key-value pairs?".to_string());
                }
                _ => {}
            },
            ValidationErrorKind::InvalidEnumValue { .. } => {
                hints.push("Check the schema for allowed values?".to_string());
            }
            ValidationErrorKind::StringPatternMismatch { .. } => {
                hints.push("Check that the string matches the expected format?".to_string());
            }
            ValidationErrorKind::NumberOutOfRange { .. }
            | ValidationErrorKind::NumberNotMultipleOf { .. } => {
                hints.push("Check the allowed value range in the schema?".to_string());
            }
            ValidationErrorKind::UnknownProperty { .. } => {
                hints.push(
                    "Check for typos in property names or remove unrecognized properties?"
                        .to_string(),
                );
            }
            ValidationErrorKind::ArrayItemsNotUnique => {
                hints.push("Remove duplicate items from the array?".to_string());
            }
            _ => {
                // No specific hints for other error kinds
            }
        }

        hints
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::InstancePath;

    #[test]
    fn test_path_segment_serialization() {
        let key = PathSegment::Key("format".to_string());
        let json = serde_json::to_value(&key).unwrap();
        assert_eq!(json["type"], "Key");
        assert_eq!(json["value"], "format");

        let index = PathSegment::Index(42);
        let json = serde_json::to_value(&index).unwrap();
        assert_eq!(json["type"], "Index");
        assert_eq!(json["value"], 42);
    }

    #[test]
    fn test_source_range_serialization() {
        let range = SourceRange {
            filename: "test.yaml".to_string(),
            start_offset: 10,
            end_offset: 20,
            start_line: 1,
            start_column: 5,
            end_line: 1,
            end_column: 15,
        };

        let json = serde_json::to_value(&range).unwrap();
        assert_eq!(json["filename"], "test.yaml");
        assert_eq!(json["start_offset"], 10);
        assert_eq!(json["end_offset"], 20);
        assert_eq!(json["start_line"], 1);
        assert_eq!(json["start_column"], 5);
    }

    #[test]
    fn test_error_code() {
        use crate::error::ValidationErrorKind;

        let error = ValidationError::new(
            ValidationErrorKind::MissingRequiredProperty {
                property: "author".to_string(),
            },
            InstancePath::new(),
        );
        assert_eq!(error.error_code(), "Q-1-10");

        let error = ValidationError::new(
            ValidationErrorKind::TypeMismatch {
                expected: "number".to_string(),
                got: "string".to_string(),
            },
            InstancePath::new(),
        );
        assert_eq!(error.error_code(), "Q-1-11");

        let error = ValidationError::new(
            ValidationErrorKind::InvalidEnumValue {
                value: "foo".to_string(),
                allowed: vec!["html".to_string(), "pdf".to_string()],
            },
            InstancePath::new(),
        );
        assert_eq!(error.error_code(), "Q-1-12");

        let error = ValidationError::new(
            ValidationErrorKind::UnknownProperty {
                property: "foo".to_string(),
            },
            InstancePath::new(),
        );
        assert_eq!(error.error_code(), "Q-1-18");
    }

    #[test]
    fn test_suggest_fixes() {
        use crate::error::ValidationErrorKind;

        let kind = ValidationErrorKind::MissingRequiredProperty {
            property: "author".to_string(),
        };
        let hints = ValidationDiagnostic::suggest_fixes_from_kind(&kind);
        assert_eq!(hints.len(), 1);
        assert!(hints[0].contains("author"));

        let kind = ValidationErrorKind::TypeMismatch {
            expected: "boolean".to_string(),
            got: "string".to_string(),
        };
        let hints = ValidationDiagnostic::suggest_fixes_from_kind(&kind);
        assert_eq!(hints.len(), 1);
        assert!(hints[0].contains("true"));

        let kind = ValidationErrorKind::TypeMismatch {
            expected: "number".to_string(),
            got: "string".to_string(),
        };
        let hints = ValidationDiagnostic::suggest_fixes_from_kind(&kind);
        assert_eq!(hints.len(), 1);
        assert!(hints[0].contains("numeric"));
    }
}
