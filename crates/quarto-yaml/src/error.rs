//! Error types for YAML parsing with source locations.

use crate::SourceInfo;
use std::fmt;

/// Result type alias for quarto-yaml operations.
pub type Result<T> = std::result::Result<T, Error>;

/// Errors that can occur during YAML parsing.
#[derive(Debug, Clone, PartialEq)]
pub enum Error {
    /// YAML syntax error
    ParseError {
        message: String,
        location: Option<SourceInfo>,
    },

    /// Unexpected end of input
    UnexpectedEof { location: Option<SourceInfo> },

    /// Invalid YAML structure
    InvalidStructure {
        message: String,
        location: Option<SourceInfo>,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::ParseError { message, location } => {
                write!(f, "Parse error: {}", message)?;
                // TODO: Proper location display requires SourceContext to map offsets to row/column.
                // For now, we only show the error message without location details.
                // To fix: refactor Error type to carry SourceContext or resolve locations before creating errors.
                if let Some(_loc) = location {
                    // Location information available but cannot display without SourceContext
                }
                Ok(())
            }
            Error::UnexpectedEof { location } => {
                write!(f, "Unexpected end of input")?;
                // TODO: Proper location display requires SourceContext to map offsets to row/column.
                // For now, we only show the error message without location details.
                // To fix: refactor Error type to carry SourceContext or resolve locations before creating errors.
                if let Some(_loc) = location {
                    // Location information available but cannot display without SourceContext
                }
                Ok(())
            }
            Error::InvalidStructure { message, location } => {
                write!(f, "Invalid YAML structure: {}", message)?;
                // TODO: Proper location display requires SourceContext to map offsets to row/column.
                // For now, we only show the error message without location details.
                // To fix: refactor Error type to carry SourceContext or resolve locations before creating errors.
                if let Some(_loc) = location {
                    // Location information available but cannot display without SourceContext
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for Error {}

impl From<yaml_rust2::ScanError> for Error {
    fn from(err: yaml_rust2::ScanError) -> Self {
        Error::ParseError {
            message: err.to_string(),
            location: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_source_map::FileId;

    #[test]
    fn test_parse_error_display_no_location() {
        let error = Error::ParseError {
            message: "unexpected token".to_string(),
            location: None,
        };
        assert_eq!(error.to_string(), "Parse error: unexpected token");
    }

    #[test]
    fn test_parse_error_display_with_location() {
        let location = SourceInfo::original(FileId(0), 10, 20);
        let error = Error::ParseError {
            message: "invalid syntax".to_string(),
            location: Some(location),
        };
        // Location is not displayed currently (see TODO in code)
        assert_eq!(error.to_string(), "Parse error: invalid syntax");
    }

    #[test]
    fn test_unexpected_eof_display_no_location() {
        let error = Error::UnexpectedEof { location: None };
        assert_eq!(error.to_string(), "Unexpected end of input");
    }

    #[test]
    fn test_unexpected_eof_display_with_location() {
        let location = SourceInfo::original(FileId(0), 100, 100);
        let error = Error::UnexpectedEof {
            location: Some(location),
        };
        assert_eq!(error.to_string(), "Unexpected end of input");
    }

    #[test]
    fn test_invalid_structure_display_no_location() {
        let error = Error::InvalidStructure {
            message: "expected mapping".to_string(),
            location: None,
        };
        assert_eq!(
            error.to_string(),
            "Invalid YAML structure: expected mapping"
        );
    }

    #[test]
    fn test_invalid_structure_display_with_location() {
        let location = SourceInfo::original(FileId(0), 50, 60);
        let error = Error::InvalidStructure {
            message: "duplicate key".to_string(),
            location: Some(location),
        };
        assert_eq!(error.to_string(), "Invalid YAML structure: duplicate key");
    }

    #[test]
    fn test_error_is_std_error() {
        let error = Error::ParseError {
            message: "test".to_string(),
            location: None,
        };
        // Verify that Error implements std::error::Error
        fn assert_error<T: std::error::Error>(_: &T) {}
        assert_error(&error);
    }
}
