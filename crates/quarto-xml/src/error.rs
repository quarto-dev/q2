//! Error types for XML parsing with source locations.

use quarto_source_map::SourceInfo;
use std::fmt;

/// Result type alias for quarto-xml operations.
pub type Result<T> = std::result::Result<T, Error>;

/// Errors that can occur during XML parsing.
#[derive(Debug, Clone)]
pub enum Error {
    /// XML syntax error from quick-xml.
    XmlSyntax {
        message: String,
        /// Byte offset where the error occurred.
        position: Option<u64>,
    },

    /// Unexpected end of input.
    UnexpectedEof {
        /// What was expected when EOF was encountered.
        expected: String,
        location: Option<SourceInfo>,
    },

    /// Mismatched end tag.
    MismatchedEndTag {
        /// The expected tag name.
        expected: String,
        /// The actual tag name found.
        found: String,
        location: Option<SourceInfo>,
    },

    /// Invalid XML structure.
    InvalidStructure {
        message: String,
        location: Option<SourceInfo>,
    },

    /// Empty document (no root element).
    EmptyDocument,

    /// Multiple root elements.
    MultipleRoots { location: Option<SourceInfo> },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::XmlSyntax { message, position } => {
                write!(f, "XML syntax error: {}", message)?;
                if let Some(pos) = position {
                    write!(f, " at byte {}", pos)?;
                }
                Ok(())
            }
            Error::UnexpectedEof { expected, .. } => {
                write!(f, "Unexpected end of input, expected {}", expected)
            }
            Error::MismatchedEndTag {
                expected, found, ..
            } => {
                write!(
                    f,
                    "Mismatched end tag: expected </{}>, found </{}>",
                    expected, found
                )
            }
            Error::InvalidStructure { message, .. } => {
                write!(f, "Invalid XML structure: {}", message)
            }
            Error::EmptyDocument => {
                write!(f, "Empty XML document: no root element found")
            }
            Error::MultipleRoots { .. } => {
                write!(f, "Invalid XML: multiple root elements")
            }
        }
    }
}

impl std::error::Error for Error {}

impl From<quick_xml::Error> for Error {
    fn from(err: quick_xml::Error) -> Self {
        Error::XmlSyntax {
            message: err.to_string(),
            position: None,
        }
    }
}

impl From<quick_xml::events::attributes::AttrError> for Error {
    fn from(err: quick_xml::events::attributes::AttrError) -> Self {
        Error::XmlSyntax {
            message: format!("Attribute error: {}", err),
            position: None,
        }
    }
}
