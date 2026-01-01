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
            && let Some(file) = ctx.get_file(mapped.file_id)
        {
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

    // Tests for SchemaError Display implementation
    #[test]
    fn test_schema_error_invalid_type_display() {
        let error = SchemaError::InvalidType("bad_type".to_string());
        assert_eq!(error.to_string(), "Invalid schema type: bad_type");
    }

    #[test]
    fn test_schema_error_invalid_structure_display() {
        use quarto_source_map::{FileId, SourceInfo};
        let location = SourceInfo::original(FileId(0), 10, 20);
        let error = SchemaError::InvalidStructure {
            message: "unexpected array".to_string(),
            location,
        };
        assert_eq!(
            error.to_string(),
            "Invalid schema structure: unexpected array (at offset 10)"
        );
    }

    #[test]
    fn test_schema_error_missing_field_display() {
        use quarto_source_map::{FileId, SourceInfo};
        let location = SourceInfo::original(FileId(0), 5, 15);
        let error = SchemaError::MissingField {
            field: "type".to_string(),
            location,
        };
        assert_eq!(
            error.to_string(),
            "Missing required field 'type' (at offset 5)"
        );
    }

    #[test]
    fn test_schema_error_unresolved_ref_display() {
        let error = SchemaError::UnresolvedRef("missing_schema".to_string());
        assert_eq!(
            error.to_string(),
            "Unresolved schema reference: missing_schema"
        );
    }

    #[test]
    fn test_schema_error_yaml_error_display() {
        let yaml_err = quarto_yaml::Error::ParseError {
            message: "invalid yaml".to_string(),
            location: None,
        };
        let error = SchemaError::YamlError(yaml_err);
        assert!(error.to_string().contains("YAML parsing error"));
    }

    #[test]
    fn test_schema_error_source() {
        use std::error::Error;
        // Test that YamlError variant returns the source error
        let yaml_err = quarto_yaml::Error::ParseError {
            message: "test".to_string(),
            location: None,
        };
        let error = SchemaError::YamlError(yaml_err);
        assert!(error.source().is_some());

        // Test that other variants return None
        let error = SchemaError::InvalidType("test".to_string());
        assert!(error.source().is_none());
    }

    #[test]
    fn test_schema_error_from_yaml_error() {
        let yaml_err = quarto_yaml::Error::ParseError {
            message: "parse failed".to_string(),
            location: None,
        };
        let schema_err: SchemaError = yaml_err.into();
        match schema_err {
            SchemaError::YamlError(_) => {} // expected
            _ => panic!("Expected YamlError variant"),
        }
    }

    // Tests for ValidationErrorKind::error_code
    #[test]
    fn test_error_code_missing_required_property() {
        let kind = ValidationErrorKind::MissingRequiredProperty {
            property: "foo".to_string(),
        };
        assert_eq!(kind.error_code(), "Q-1-10");
    }

    #[test]
    fn test_error_code_type_mismatch() {
        let kind = ValidationErrorKind::TypeMismatch {
            expected: "number".to_string(),
            got: "string".to_string(),
        };
        assert_eq!(kind.error_code(), "Q-1-11");
    }

    #[test]
    fn test_error_code_invalid_enum_value() {
        let kind = ValidationErrorKind::InvalidEnumValue {
            value: "bad".to_string(),
            allowed: vec!["a".to_string(), "b".to_string()],
        };
        assert_eq!(kind.error_code(), "Q-1-12");
    }

    #[test]
    fn test_error_code_array_length_invalid() {
        let kind = ValidationErrorKind::ArrayLengthInvalid {
            length: 5,
            min_items: Some(10),
            max_items: None,
        };
        assert_eq!(kind.error_code(), "Q-1-13");
    }

    #[test]
    fn test_error_code_string_pattern_mismatch() {
        let kind = ValidationErrorKind::StringPatternMismatch {
            value: "test".to_string(),
            pattern: "^[0-9]+$".to_string(),
        };
        assert_eq!(kind.error_code(), "Q-1-14");
    }

    #[test]
    fn test_error_code_number_out_of_range() {
        let kind = ValidationErrorKind::NumberOutOfRange {
            value: 100.0,
            minimum: Some(0.0),
            maximum: Some(50.0),
            exclusive_minimum: None,
            exclusive_maximum: None,
        };
        assert_eq!(kind.error_code(), "Q-1-15");
    }

    #[test]
    fn test_error_code_number_not_multiple_of() {
        let kind = ValidationErrorKind::NumberNotMultipleOf {
            value: 7.0,
            multiple_of: 3.0,
        };
        assert_eq!(kind.error_code(), "Q-1-15");
    }

    #[test]
    fn test_error_code_object_property_count_invalid() {
        let kind = ValidationErrorKind::ObjectPropertyCountInvalid {
            count: 5,
            min_properties: Some(10),
            max_properties: None,
        };
        assert_eq!(kind.error_code(), "Q-1-16");
    }

    #[test]
    fn test_error_code_unresolved_reference() {
        let kind = ValidationErrorKind::UnresolvedReference {
            ref_id: "missing".to_string(),
        };
        assert_eq!(kind.error_code(), "Q-1-17");
    }

    #[test]
    fn test_error_code_unknown_property() {
        let kind = ValidationErrorKind::UnknownProperty {
            property: "foo".to_string(),
        };
        assert_eq!(kind.error_code(), "Q-1-18");
    }

    #[test]
    fn test_error_code_array_items_not_unique() {
        let kind = ValidationErrorKind::ArrayItemsNotUnique;
        assert_eq!(kind.error_code(), "Q-1-19");
    }

    #[test]
    fn test_error_code_string_length_invalid() {
        let kind = ValidationErrorKind::StringLengthInvalid {
            length: 5,
            min_length: Some(10),
            max_length: None,
        };
        assert_eq!(kind.error_code(), "Q-1-20");
    }

    // Tests for ValidationErrorKind::message edge cases
    #[test]
    fn test_message_number_out_of_range_minimum() {
        let kind = ValidationErrorKind::NumberOutOfRange {
            value: -5.0,
            minimum: Some(0.0),
            maximum: None,
            exclusive_minimum: None,
            exclusive_maximum: None,
        };
        assert_eq!(kind.message(), "Number -5 is less than minimum 0");
    }

    #[test]
    fn test_message_number_out_of_range_maximum() {
        let kind = ValidationErrorKind::NumberOutOfRange {
            value: 100.0,
            minimum: None,
            maximum: Some(50.0),
            exclusive_minimum: None,
            exclusive_maximum: None,
        };
        assert_eq!(kind.message(), "Number 100 is greater than maximum 50");
    }

    #[test]
    fn test_message_number_out_of_range_exclusive_minimum() {
        let kind = ValidationErrorKind::NumberOutOfRange {
            value: 5.0,
            minimum: None,
            maximum: None,
            exclusive_minimum: Some(5.0),
            exclusive_maximum: None,
        };
        assert_eq!(kind.message(), "Number 5 is not greater than 5");
    }

    #[test]
    fn test_message_number_out_of_range_exclusive_maximum() {
        let kind = ValidationErrorKind::NumberOutOfRange {
            value: 10.0,
            minimum: None,
            maximum: None,
            exclusive_minimum: None,
            exclusive_maximum: Some(10.0),
        };
        assert_eq!(kind.message(), "Number 10 is not less than 10");
    }

    #[test]
    fn test_message_number_out_of_range_no_bounds() {
        let kind = ValidationErrorKind::NumberOutOfRange {
            value: 42.0,
            minimum: None,
            maximum: None,
            exclusive_minimum: None,
            exclusive_maximum: None,
        };
        assert_eq!(kind.message(), "Number 42 is out of range");
    }

    #[test]
    fn test_message_number_not_multiple_of() {
        let kind = ValidationErrorKind::NumberNotMultipleOf {
            value: 7.0,
            multiple_of: 3.0,
        };
        assert_eq!(kind.message(), "Number 7 is not a multiple of 3");
    }

    #[test]
    fn test_message_string_length_invalid_min() {
        let kind = ValidationErrorKind::StringLengthInvalid {
            length: 3,
            min_length: Some(10),
            max_length: None,
        };
        assert_eq!(kind.message(), "String length 3 is less than minimum 10");
    }

    #[test]
    fn test_message_string_length_invalid_max() {
        let kind = ValidationErrorKind::StringLengthInvalid {
            length: 100,
            min_length: None,
            max_length: Some(50),
        };
        assert_eq!(
            kind.message(),
            "String length 100 is greater than maximum 50"
        );
    }

    #[test]
    fn test_message_string_length_invalid_no_bounds() {
        let kind = ValidationErrorKind::StringLengthInvalid {
            length: 42,
            min_length: None,
            max_length: None,
        };
        assert_eq!(kind.message(), "String length 42 is invalid");
    }

    #[test]
    fn test_message_array_length_invalid_min() {
        let kind = ValidationErrorKind::ArrayLengthInvalid {
            length: 2,
            min_items: Some(5),
            max_items: None,
        };
        assert_eq!(kind.message(), "Array length 2 is less than minimum 5");
    }

    #[test]
    fn test_message_array_length_invalid_max() {
        let kind = ValidationErrorKind::ArrayLengthInvalid {
            length: 20,
            min_items: None,
            max_items: Some(10),
        };
        assert_eq!(kind.message(), "Array length 20 is greater than maximum 10");
    }

    #[test]
    fn test_message_array_length_invalid_no_bounds() {
        let kind = ValidationErrorKind::ArrayLengthInvalid {
            length: 42,
            min_items: None,
            max_items: None,
        };
        assert_eq!(kind.message(), "Array length 42 is invalid");
    }

    #[test]
    fn test_message_object_property_count_min() {
        let kind = ValidationErrorKind::ObjectPropertyCountInvalid {
            count: 1,
            min_properties: Some(3),
            max_properties: None,
        };
        assert_eq!(
            kind.message(),
            "Object has 1 properties, less than minimum 3"
        );
    }

    #[test]
    fn test_message_object_property_count_max() {
        let kind = ValidationErrorKind::ObjectPropertyCountInvalid {
            count: 15,
            min_properties: None,
            max_properties: Some(10),
        };
        assert_eq!(
            kind.message(),
            "Object has 15 properties, greater than maximum 10"
        );
    }

    #[test]
    fn test_message_object_property_count_no_bounds() {
        let kind = ValidationErrorKind::ObjectPropertyCountInvalid {
            count: 5,
            min_properties: None,
            max_properties: None,
        };
        assert_eq!(kind.message(), "Object has 5 properties (invalid)");
    }

    #[test]
    fn test_message_array_items_not_unique() {
        let kind = ValidationErrorKind::ArrayItemsNotUnique;
        assert_eq!(kind.message(), "Array items must be unique");
    }

    #[test]
    fn test_message_unknown_property() {
        let kind = ValidationErrorKind::UnknownProperty {
            property: "extra_field".to_string(),
        };
        assert_eq!(kind.message(), "Unknown property 'extra_field'");
    }

    #[test]
    fn test_message_invalid_enum_value() {
        let kind = ValidationErrorKind::InvalidEnumValue {
            value: "invalid".to_string(),
            allowed: vec!["a".to_string(), "b".to_string(), "c".to_string()],
        };
        assert_eq!(
            kind.message(),
            "Value must be one of: a, b, c, got 'invalid'"
        );
    }

    #[test]
    fn test_message_unresolved_reference() {
        let kind = ValidationErrorKind::UnresolvedReference {
            ref_id: "missing_schema".to_string(),
        };
        assert_eq!(
            kind.message(),
            "Unresolved schema reference: missing_schema"
        );
    }

    #[test]
    fn test_message_string_pattern_mismatch() {
        let kind = ValidationErrorKind::StringPatternMismatch {
            value: "abc".to_string(),
            pattern: "^[0-9]+$".to_string(),
        };
        assert_eq!(
            kind.message(),
            "String 'abc' does not match pattern '^[0-9]+$'"
        );
    }

    // Tests for ValidationError Display with location
    #[test]
    fn test_validation_error_display_with_location() {
        let mut path = InstancePath::new();
        path.push_key("format");
        let mut error = ValidationError::new(
            ValidationErrorKind::TypeMismatch {
                expected: "object".to_string(),
                got: "string".to_string(),
            },
            path,
        );
        error.location = Some(SourceLocation {
            file: "test.yml".to_string(),
            line: 10,
            column: 5,
        });
        assert_eq!(
            error.to_string(),
            "Validation error at test.yml:10:5: Expected object, got string"
        );
    }

    #[test]
    fn test_validation_error_display_without_location() {
        let mut path = InstancePath::new();
        path.push_key("format");
        path.push_key("html");
        let error = ValidationError::new(
            ValidationErrorKind::MissingRequiredProperty {
                property: "toc".to_string(),
            },
            path,
        );
        assert_eq!(
            error.to_string(),
            "Validation error at format.html: Missing required property 'toc'"
        );
    }

    // Tests for ValidationError::with_schema_path
    #[test]
    fn test_validation_error_with_schema_path() {
        let error = ValidationError::new(
            ValidationErrorKind::TypeMismatch {
                expected: "number".to_string(),
                got: "string".to_string(),
            },
            InstancePath::new(),
        );
        let mut schema_path = SchemaPath::new();
        schema_path.push("properties");
        schema_path.push("count");

        let error = error.with_schema_path(schema_path);
        assert_eq!(error.schema_path.len(), 2);
        assert_eq!(error.schema_path.segments()[0], "properties");
    }

    // Tests for InstancePath utility methods
    #[test]
    fn test_instance_path_pop() {
        let mut path = InstancePath::new();
        path.push_key("a");
        path.push_key("b");
        assert_eq!(path.len(), 2);

        let popped = path.pop();
        assert!(matches!(popped, Some(PathSegment::Key(k)) if k == "b"));
        assert_eq!(path.len(), 1);

        let popped = path.pop();
        assert!(matches!(popped, Some(PathSegment::Key(k)) if k == "a"));
        assert_eq!(path.len(), 0);

        let popped = path.pop();
        assert!(popped.is_none());
    }

    #[test]
    fn test_instance_path_is_empty() {
        let mut path = InstancePath::new();
        assert!(path.is_empty());

        path.push_key("test");
        assert!(!path.is_empty());
    }

    #[test]
    fn test_instance_path_segments() {
        let mut path = InstancePath::new();
        path.push_key("a");
        path.push_index(0);
        path.push_key("b");

        let segments = path.segments();
        assert_eq!(segments.len(), 3);
        assert!(matches!(&segments[0], PathSegment::Key(k) if k == "a"));
        assert!(matches!(&segments[1], PathSegment::Index(0)));
        assert!(matches!(&segments[2], PathSegment::Key(k) if k == "b"));
    }

    // Tests for SchemaPath utility methods
    #[test]
    fn test_schema_path_pop() {
        let mut path = SchemaPath::new();
        path.push("properties");
        path.push("format");
        assert_eq!(path.len(), 2);

        let popped = path.pop();
        assert_eq!(popped, Some("format".to_string()));
        assert_eq!(path.len(), 1);
    }

    #[test]
    fn test_schema_path_is_empty() {
        let mut path = SchemaPath::new();
        assert!(path.is_empty());

        path.push("test");
        assert!(!path.is_empty());
    }

    #[test]
    fn test_schema_path_segments() {
        let mut path = SchemaPath::new();
        path.push("properties");
        path.push("items");

        let segments = path.segments();
        assert_eq!(segments, &["properties", "items"]);
    }

    // Tests for PathSegment display
    #[test]
    fn test_path_segment_key_display() {
        let segment = PathSegment::Key("property_name".to_string());
        assert_eq!(segment.to_string(), "property_name");
    }

    #[test]
    fn test_path_segment_index_display() {
        let segment = PathSegment::Index(42);
        assert_eq!(segment.to_string(), "[42]");
    }
}
