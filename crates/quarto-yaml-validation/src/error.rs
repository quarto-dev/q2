// Error types for YAML validation

use quarto_yaml::{SourceInfo, YamlWithSourceInfo};
use std::fmt;
use thiserror::Error;

/// Errors that can occur during schema parsing from YAML
#[derive(Debug)]
pub enum SchemaError {
    /// Invalid schema type name
    InvalidType(String),

    /// Invalid schema structure
    InvalidStructure {
        message: String,
        location: SourceInfo,
    },

    /// Missing required field
    MissingField { field: String, location: SourceInfo },

    /// Unresolved schema reference
    UnresolvedRef(String),

    /// YAML parsing error
    YamlError(quarto_yaml::Error),
}

impl fmt::Display for SchemaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SchemaError::InvalidType(s) => write!(f, "Invalid schema type: {}", s),
            SchemaError::InvalidStructure { message, location } => {
                write!(
                    f,
                    "Invalid schema structure: {} (at offset {})",
                    message,
                    location.start_offset()
                )
            }
            SchemaError::MissingField { field, location } => {
                write!(
                    f,
                    "Missing required field '{}' (at offset {})",
                    field,
                    location.start_offset()
                )
            }
            SchemaError::UnresolvedRef(s) => write!(f, "Unresolved schema reference: {}", s),
            SchemaError::YamlError(e) => write!(f, "YAML parsing error: {}", e),
        }
    }
}

impl std::error::Error for SchemaError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            SchemaError::YamlError(e) => Some(e),
            _ => None,
        }
    }
}

impl From<quarto_yaml::Error> for SchemaError {
    fn from(e: quarto_yaml::Error) -> Self {
        SchemaError::YamlError(e)
    }
}

/// Result type for schema parsing operations
pub type SchemaResult<T> = Result<T, SchemaError>;

/// Result type for validation operations
pub type ValidationResult<T> = Result<T, ValidationError>;

/// Structured validation error kinds
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum ValidationErrorKind {
    /// Type mismatch
    TypeMismatch { expected: String, got: String },

    /// Missing required property
    MissingRequiredProperty { property: String },

    /// Unknown property in closed object
    UnknownProperty { property: String },

    /// Value not in enum
    InvalidEnumValue { value: String, allowed: Vec<String> },

    /// Number out of range
    NumberOutOfRange {
        value: f64,
        minimum: Option<f64>,
        maximum: Option<f64>,
        exclusive_minimum: Option<f64>,
        exclusive_maximum: Option<f64>,
    },

    /// Number not a multiple of
    NumberNotMultipleOf { value: f64, multiple_of: f64 },

    /// String length invalid
    StringLengthInvalid {
        length: usize,
        min_length: Option<usize>,
        max_length: Option<usize>,
    },

    /// String doesn't match pattern
    StringPatternMismatch { value: String, pattern: String },

    /// Array length invalid
    ArrayLengthInvalid {
        length: usize,
        min_items: Option<usize>,
        max_items: Option<usize>,
    },

    /// Array items not unique
    ArrayItemsNotUnique,

    /// Object property count invalid
    ObjectPropertyCountInvalid {
        count: usize,
        min_properties: Option<usize>,
        max_properties: Option<usize>,
    },

    /// Unresolved schema reference
    UnresolvedReference { ref_id: String },

    /// Other validation error
    ///
    /// **WARNING**: This is a last-resort variant for errors that don't fit any other category.
    /// Before using this, strongly consider whether the error should be represented as a new
    /// structured variant in ValidationErrorKind. Structured variants are preferable because:
    /// - They're machine-readable and can be matched on
    /// - They carry type-safe data
    /// - They enable better error reporting and hints
    ///
    /// Only use `Other` for truly unexpected or edge-case errors.
    Other { message: String },
}

impl ValidationErrorKind {
    /// Get the error code for this error kind
    pub fn error_code(&self) -> &'static str {
        match self {
            ValidationErrorKind::MissingRequiredProperty { .. } => "Q-1-10",
            ValidationErrorKind::TypeMismatch { .. } => "Q-1-11",
            ValidationErrorKind::InvalidEnumValue { .. } => "Q-1-12",
            ValidationErrorKind::ArrayLengthInvalid { .. } => "Q-1-13",
            ValidationErrorKind::StringPatternMismatch { .. } => "Q-1-14",
            ValidationErrorKind::NumberOutOfRange { .. }
            | ValidationErrorKind::NumberNotMultipleOf { .. } => "Q-1-15",
            ValidationErrorKind::ObjectPropertyCountInvalid { .. } => "Q-1-16",
            ValidationErrorKind::UnresolvedReference { .. } => "Q-1-17",
            ValidationErrorKind::UnknownProperty { .. } => "Q-1-18",
            ValidationErrorKind::ArrayItemsNotUnique => "Q-1-19",
            ValidationErrorKind::StringLengthInvalid { .. } => "Q-1-20",
            ValidationErrorKind::Other { .. } => "Q-1-99",
        }
    }

    /// Format a human-readable message from this error kind
    pub fn message(&self) -> String {
        match self {
            ValidationErrorKind::TypeMismatch { expected, got } => {
                format!("Expected {}, got {}", expected, got)
            }
            ValidationErrorKind::MissingRequiredProperty { property } => {
                format!("Missing required property '{}'", property)
            }
            ValidationErrorKind::UnknownProperty { property } => {
                format!("Unknown property '{}'", property)
            }
            ValidationErrorKind::InvalidEnumValue { value, allowed } => {
                format!(
                    "Value must be one of: {}, got '{}'",
                    allowed.join(", "),
                    value
                )
            }
            ValidationErrorKind::NumberOutOfRange {
                value,
                minimum,
                maximum,
                exclusive_minimum,
                exclusive_maximum,
            } => {
                if let Some(min) = minimum {
                    format!("Number {} is less than minimum {}", value, min)
                } else if let Some(max) = maximum {
                    format!("Number {} is greater than maximum {}", value, max)
                } else if let Some(min) = exclusive_minimum {
                    format!("Number {} is not greater than {}", value, min)
                } else if let Some(max) = exclusive_maximum {
                    format!("Number {} is not less than {}", value, max)
                } else {
                    format!("Number {} is out of range", value)
                }
            }
            ValidationErrorKind::NumberNotMultipleOf { value, multiple_of } => {
                format!("Number {} is not a multiple of {}", value, multiple_of)
            }
            ValidationErrorKind::StringLengthInvalid {
                length,
                min_length,
                max_length,
            } => {
                if let Some(min) = min_length {
                    format!("String length {} is less than minimum {}", length, min)
                } else if let Some(max) = max_length {
                    format!("String length {} is greater than maximum {}", length, max)
                } else {
                    format!("String length {} is invalid", length)
                }
            }
            ValidationErrorKind::StringPatternMismatch { value, pattern } => {
                format!("String '{}' does not match pattern '{}'", value, pattern)
            }
            ValidationErrorKind::ArrayLengthInvalid {
                length,
                min_items,
                max_items,
            } => {
                if let Some(min) = min_items {
                    format!("Array length {} is less than minimum {}", length, min)
                } else if let Some(max) = max_items {
                    format!("Array length {} is greater than maximum {}", length, max)
                } else {
                    format!("Array length {} is invalid", length)
                }
            }
            ValidationErrorKind::ArrayItemsNotUnique => "Array items must be unique".to_string(),
            ValidationErrorKind::ObjectPropertyCountInvalid {
                count,
                min_properties,
                max_properties,
            } => {
                if let Some(min) = min_properties {
                    format!("Object has {} properties, less than minimum {}", count, min)
                } else if let Some(max) = max_properties {
                    format!(
                        "Object has {} properties, greater than maximum {}",
                        count, max
                    )
                } else {
                    format!("Object has {} properties (invalid)", count)
                }
            }
            ValidationErrorKind::UnresolvedReference { ref_id } => {
                format!("Unresolved schema reference: {}", ref_id)
            }
            ValidationErrorKind::Other { message } => message.clone(),
        }
    }
}

/// Validation error with source location information
#[derive(Debug, Clone, Error)]
pub struct ValidationError {
    /// The structured error kind
    pub kind: ValidationErrorKind,
    /// Instance path where the error occurred (e.g., ["format", "html", "toc"])
    pub instance_path: InstancePath,
    /// Schema path that failed (e.g., ["properties", "format", "properties", "html"])
    pub schema_path: SchemaPath,
    /// The YAML node where the error occurred (if available)
    pub yaml_node: Option<YamlWithSourceInfo>,
    /// Source location (file, line, column) for error reporting
    pub location: Option<SourceLocation>,
}

impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = self.kind.message();
        if let Some(loc) = &self.location {
            write!(
                f,
                "Validation error at {}:{}:{}: {}",
                loc.file, loc.line, loc.column, message
            )
        } else {
            write!(f, "Validation error at {}: {}", self.instance_path, message)
        }
    }
}

impl ValidationError {
    /// Create a new validation error with a structured kind
    pub fn new(kind: ValidationErrorKind, instance_path: InstancePath) -> Self {
        Self {
            kind,
            instance_path,
            schema_path: SchemaPath::new(),
            yaml_node: None,
            location: None,
        }
    }

    /// Get the human-readable message for this error
    pub fn message(&self) -> String {
        self.kind.message()
    }

    /// Get the error code for this error
    pub fn error_code(&self) -> &'static str {
        self.kind.error_code()
    }

    /// Set the schema path for this error
    pub fn with_schema_path(mut self, schema_path: SchemaPath) -> Self {
        self.schema_path = schema_path;
        self
    }

    /// Set the YAML node for this error
    pub fn with_yaml_node(
        mut self,
        node: YamlWithSourceInfo,
        ctx: &quarto_source_map::SourceContext,
    ) -> Self {
        // Extract location from the node using SourceContext
        // Map the offset to get proper file/line/column information
        if let Some(mapped) = node.source_info.map_offset(0, ctx)
            && let Some(file) = ctx.get_file(mapped.file_id) {
                self.location = Some(SourceLocation {
                    file: file.path.clone(),
                    line: mapped.location.row + 1, // 1-indexed for display
                    column: mapped.location.column + 1, // 1-indexed for display
                });
            }

        // Still store the node for potential future use
        self.yaml_node = Some(node);
        self
    }
}

/// Instance path (e.g., ["format", "html", "toc"])
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InstancePath {
    segments: Vec<PathSegment>,
}

impl InstancePath {
    /// Create a new empty instance path
    pub fn new() -> Self {
        Self {
            segments: Vec::new(),
        }
    }

    /// Push a key segment onto the path
    pub fn push_key(&mut self, key: impl Into<String>) {
        self.segments.push(PathSegment::Key(key.into()));
    }

    /// Push an index segment onto the path
    pub fn push_index(&mut self, index: usize) {
        self.segments.push(PathSegment::Index(index));
    }

    /// Pop the last segment from the path
    pub fn pop(&mut self) -> Option<PathSegment> {
        self.segments.pop()
    }

    /// Get the segments as a slice
    pub fn segments(&self) -> &[PathSegment] {
        &self.segments
    }

    /// Check if the path is empty
    pub fn is_empty(&self) -> bool {
        self.segments.is_empty()
    }

    /// Get the length of the path
    pub fn len(&self) -> usize {
        self.segments.len()
    }
}

impl fmt::Display for InstancePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.segments.is_empty() {
            write!(f, "(root)")
        } else {
            for (i, segment) in self.segments.iter().enumerate() {
                if i > 0 {
                    write!(f, ".")?;
                }
                write!(f, "{}", segment)?;
            }
            Ok(())
        }
    }
}

/// Schema path (e.g., ["properties", "format", "properties", "html"])
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SchemaPath {
    segments: Vec<String>,
}

impl SchemaPath {
    /// Create a new empty schema path
    pub fn new() -> Self {
        Self {
            segments: Vec::new(),
        }
    }

    /// Push a segment onto the path
    pub fn push(&mut self, segment: impl Into<String>) {
        self.segments.push(segment.into());
    }

    /// Pop the last segment from the path
    pub fn pop(&mut self) -> Option<String> {
        self.segments.pop()
    }

    /// Get the segments as a slice
    pub fn segments(&self) -> &[String] {
        &self.segments
    }

    /// Check if the path is empty
    pub fn is_empty(&self) -> bool {
        self.segments.is_empty()
    }

    /// Get the length of the path
    pub fn len(&self) -> usize {
        self.segments.len()
    }
}

impl fmt::Display for SchemaPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.segments.is_empty() {
            write!(f, "(root)")
        } else {
            write!(f, "{}", self.segments.join(" > "))
        }
    }
}

/// A segment in an instance path
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathSegment {
    /// Object key
    Key(String),
    /// Array index
    Index(usize),
}

impl fmt::Display for PathSegment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PathSegment::Key(key) => write!(f, "{}", key),
            PathSegment::Index(index) => write!(f, "[{}]", index),
        }
    }
}

/// Source location for error reporting
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceLocation {
    pub file: String,
    pub line: usize,
    pub column: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_instance_path_display() {
        let mut path = InstancePath::new();
        assert_eq!(path.to_string(), "(root)");

        path.push_key("format");
        assert_eq!(path.to_string(), "format");

        path.push_key("html");
        assert_eq!(path.to_string(), "format.html");

        path.push_index(0);
        assert_eq!(path.to_string(), "format.html.[0]");
    }

    #[test]
    fn test_schema_path_display() {
        let mut path = SchemaPath::new();
        assert_eq!(path.to_string(), "(root)");

        path.push("properties");
        path.push("format");
        assert_eq!(path.to_string(), "properties > format");
    }

    #[test]
    fn test_validation_error_creation() {
        let mut path = InstancePath::new();
        path.push_key("format");

        let error = ValidationError::new(
            ValidationErrorKind::TypeMismatch {
                expected: "number".to_string(),
                got: "string".to_string(),
            },
            path,
        );
        assert_eq!(error.message(), "Expected number, got string");
        assert_eq!(error.instance_path.len(), 1);
    }

    #[test]
    fn test_validation_error_other_variant() {
        let error = ValidationError::new(
            ValidationErrorKind::Other {
                message: "Something unexpected happened".to_string(),
            },
            InstancePath::new(),
        );
        assert_eq!(error.message(), "Something unexpected happened");
        assert_eq!(error.error_code(), "Q-1-99");
    }
}
