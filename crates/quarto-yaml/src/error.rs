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
                if let Some(loc) = location {
                    // Display with 1-indexed row/column
                    write!(
                        f,
                        " at {}:{}",
                        loc.range.start.row + 1,
                        loc.range.start.column + 1
                    )?;
                }
                Ok(())
            }
            Error::UnexpectedEof { location } => {
                write!(f, "Unexpected end of input")?;
                if let Some(loc) = location {
                    write!(
                        f,
                        " at {}:{}",
                        loc.range.start.row + 1,
                        loc.range.start.column + 1
                    )?;
                }
                Ok(())
            }
            Error::InvalidStructure { message, location } => {
                write!(f, "Invalid YAML structure: {}", message)?;
                if let Some(loc) = location {
                    write!(
                        f,
                        " at {}:{}",
                        loc.range.start.row + 1,
                        loc.range.start.column + 1
                    )?;
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
