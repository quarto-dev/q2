//! Error types for XML parsing with source locations.

use quarto_error_reporting::{DiagnosticMessage, DiagnosticMessageBuilder};
use quarto_source_map::SourceInfo;
use std::fmt;

/// Result type alias for quarto-xml operations.
pub type Result<T> = std::result::Result<T, Error>;

/// Result type for parse operations that return diagnostics.
pub type ParseResult<T> = std::result::Result<T, Vec<DiagnosticMessage>>;

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

impl Error {
    /// Convert this error to a DiagnosticMessage with the appropriate Q-9-* error code.
    pub fn to_diagnostic(&self) -> DiagnosticMessage {
        match self {
            Error::XmlSyntax { message, position } => {
                let mut builder = DiagnosticMessageBuilder::error("XML Syntax Error")
                    .with_code("Q-9-1")
                    .problem(message.clone());

                if let Some(pos) = position {
                    builder = builder.add_detail(format!("Error at byte offset {}", pos));
                }

                builder.build()
            }

            Error::UnexpectedEof { expected, location } => {
                let mut builder = DiagnosticMessageBuilder::error("Unexpected End of XML Input")
                    .with_code("Q-9-2")
                    .problem(format!(
                        "The XML document ended unexpectedly; expected {}",
                        expected
                    ));

                if let Some(loc) = location {
                    builder = builder.with_location(loc.clone());
                }

                builder.build()
            }

            Error::MismatchedEndTag {
                expected,
                found,
                location,
            } => {
                let mut builder = DiagnosticMessageBuilder::error("Mismatched XML End Tag")
                    .with_code("Q-9-3")
                    .problem(format!(
                        "End tag </{}> does not match start tag <{}>",
                        found, expected
                    ))
                    .add_detail(format!("Expected: </{}>", expected))
                    .add_detail(format!("Found: </{}>", found))
                    .add_hint("Check that all opening tags have matching closing tags?");

                if let Some(loc) = location {
                    builder = builder.with_location(loc.clone());
                }

                builder.build()
            }

            Error::InvalidStructure { message, location } => {
                let mut builder = DiagnosticMessageBuilder::error("Invalid XML Structure")
                    .with_code("Q-9-4")
                    .problem(message.clone());

                if let Some(loc) = location {
                    builder = builder.with_location(loc.clone());
                }

                builder.build()
            }

            Error::EmptyDocument => DiagnosticMessageBuilder::error("Empty XML Document")
                .with_code("Q-9-5")
                .problem("The XML document contains no root element")
                .add_hint("Add a root element to the document?")
                .build(),

            Error::MultipleRoots { location } => {
                let mut builder = DiagnosticMessageBuilder::error("Multiple XML Root Elements")
                    .with_code("Q-9-6")
                    .problem("The XML document contains multiple root elements")
                    .add_detail("XML documents must have exactly one root element")
                    .add_hint("Wrap multiple elements in a single container element?");

                if let Some(loc) = location {
                    builder = builder.with_location(loc.clone());
                }

                builder.build()
            }
        }
    }
}

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
