// Error types for YAML validation

use quarto_yaml::{SourceInfo, YamlWithSourceInfo};
use std::fmt;
use thiserror::Error;

/// Errors that can occur during schema parsing from YAML
#[derive(Debug, Error)]
pub enum SchemaError {
    /// Invalid schema type name
    #[error("Invalid schema type: {0}")]
    InvalidType(String),

    /// Invalid schema structure
    #[error("Invalid schema structure: {message} (at line {}, col {})", .location.line, .location.col)]
    InvalidStructure {
        message: String,
        location: SourceInfo,
    },

    /// Missing required field
    #[error("Missing required field '{field}' (at line {}, col {})", .location.line, .location.col)]
    MissingField {
        field: String,
        location: SourceInfo,
    },

    /// Unresolved schema reference
    #[error("Unresolved schema reference: {0}")]
    UnresolvedRef(String),

    /// YAML parsing error
    #[error("YAML parsing error: {0}")]
    YamlError(#[from] quarto_yaml::Error),
}

/// Result type for schema parsing operations
pub type SchemaResult<T> = Result<T, SchemaError>;

/// Result type for validation operations
pub type ValidationResult<T> = Result<T, ValidationError>;

/// Validation error with source location information
#[derive(Debug, Clone, Error)]
pub struct ValidationError {
    /// The error message
    pub message: String,
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
        if let Some(loc) = &self.location {
            write!(
                f,
                "Validation error at {}:{}:{}: {}",
                loc.file, loc.line, loc.column, self.message
            )
        } else {
            write!(
                f,
                "Validation error at {}: {}",
                self.instance_path, self.message
            )
        }
    }
}

impl ValidationError {
    /// Create a new validation error
    pub fn new(message: impl Into<String>, instance_path: InstancePath) -> Self {
        Self {
            message: message.into(),
            instance_path,
            schema_path: SchemaPath::new(),
            yaml_node: None,
            location: None,
        }
    }

    /// Set the schema path for this error
    pub fn with_schema_path(mut self, schema_path: SchemaPath) -> Self {
        self.schema_path = schema_path;
        self
    }

    /// Set the YAML node for this error
    pub fn with_yaml_node(mut self, node: YamlWithSourceInfo) -> Self {
        // Extract location from the node
        self.location = Some(SourceLocation {
            file: node
                .source_info
                .file
                .clone()
                .unwrap_or_else(|| "<unknown>".to_string()),
            line: node.source_info.line,
            column: node.source_info.col,
        });
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

        let error = ValidationError::new("Invalid value", path);
        assert_eq!(error.message, "Invalid value");
        assert_eq!(error.instance_path.len(), 1);
    }
}
