//! Error types for CSL parsing with source locations.

use quarto_error_reporting::{DiagnosticMessage, DiagnosticMessageBuilder};
use quarto_source_map::SourceInfo;
use std::fmt;

/// Result type alias for quarto-csl operations.
pub type Result<T> = std::result::Result<T, Error>;

/// Errors that can occur during CSL parsing.
#[derive(Debug, Clone)]
pub enum Error {
    /// XML parsing error from quarto-xml.
    XmlError(quarto_xml::Error),

    /// Missing required attribute.
    MissingAttribute {
        element: String,
        attribute: String,
        location: SourceInfo,
    },

    /// Invalid attribute value.
    InvalidAttributeValue {
        element: String,
        attribute: String,
        value: String,
        expected: String,
        location: SourceInfo,
    },

    /// Missing required element.
    MissingElement {
        parent: String,
        element: String,
        location: SourceInfo,
    },

    /// Unexpected element.
    UnexpectedElement {
        element: String,
        context: String,
        location: SourceInfo,
    },

    /// Missing text source in <text> element.
    MissingTextSource { location: SourceInfo },

    /// Undefined macro reference.
    UndefinedMacro {
        name: String,
        reference_location: SourceInfo,
        /// Suggestion for similar macro name, if any.
        suggestion: Option<String>,
    },

    /// Circular macro dependency.
    CircularMacro {
        /// The chain of macro names forming the cycle.
        chain: Vec<String>,
        location: SourceInfo,
    },

    /// Duplicate macro definition.
    DuplicateMacro {
        name: String,
        first_location: SourceInfo,
        second_location: SourceInfo,
    },

    /// Invalid CSL version.
    InvalidVersion {
        version: String,
        location: SourceInfo,
    },

    /// Root element is not <style>.
    InvalidRootElement { found: String, location: SourceInfo },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::XmlError(e) => write!(f, "XML error: {}", e),
            Error::MissingAttribute {
                element, attribute, ..
            } => {
                write!(
                    f,
                    "Missing required attribute '{}' on <{}>",
                    attribute, element
                )
            }
            Error::InvalidAttributeValue {
                element,
                attribute,
                value,
                expected,
                ..
            } => {
                write!(
                    f,
                    "Invalid value '{}' for attribute '{}' on <{}>: expected {}",
                    value, attribute, element, expected
                )
            }
            Error::MissingElement {
                parent, element, ..
            } => {
                write!(f, "Missing required element <{}> in <{}>", element, parent)
            }
            Error::UnexpectedElement {
                element, context, ..
            } => {
                write!(f, "Unexpected element <{}> in {}", element, context)
            }
            Error::MissingTextSource { .. } => {
                write!(
                    f,
                    "Text element must have one of: variable, macro, term, or value attribute"
                )
            }
            Error::UndefinedMacro { name, .. } => {
                write!(f, "Undefined macro '{}'", name)
            }
            Error::CircularMacro { chain, .. } => {
                write!(f, "Circular macro dependency: {}", chain.join(" -> "))
            }
            Error::DuplicateMacro { name, .. } => {
                write!(f, "Duplicate macro definition: '{}'", name)
            }
            Error::InvalidVersion { version, .. } => {
                write!(f, "Invalid CSL version: '{}'", version)
            }
            Error::InvalidRootElement { found, .. } => {
                write!(f, "Expected <style> root element, found <{}>", found)
            }
        }
    }
}

impl std::error::Error for Error {}

impl From<quarto_xml::Error> for Error {
    fn from(err: quarto_xml::Error) -> Self {
        Error::XmlError(err)
    }
}

impl Error {
    /// Convert this error to a DiagnosticMessage with the appropriate error code.
    ///
    /// CSL errors use Q-9-7 through Q-9-16 (subsystem 9, continuing from XML errors).
    pub fn to_diagnostic(&self) -> DiagnosticMessage {
        match self {
            Error::XmlError(e) => e.to_diagnostic(),

            Error::MissingAttribute {
                element,
                attribute,
                location,
            } => DiagnosticMessageBuilder::error("Missing Required Attribute")
                .with_code("Q-9-7")
                .with_location(location.clone())
                .problem(format!(
                    "Element <{}> requires attribute '{}'",
                    element, attribute
                ))
                .add_hint(format!("Add {}=\"...\" to the element?", attribute))
                .build(),

            Error::InvalidAttributeValue {
                element,
                attribute,
                value,
                expected,
                location,
            } => DiagnosticMessageBuilder::error("Invalid Attribute Value")
                .with_code("Q-9-8")
                .with_location(location.clone())
                .problem(format!(
                    "Invalid value '{}' for attribute '{}' on <{}>",
                    value, attribute, element
                ))
                .add_detail(format!("Expected: {}", expected))
                .build(),

            Error::MissingElement {
                parent,
                element,
                location,
            } => DiagnosticMessageBuilder::error("Missing Required Element")
                .with_code("Q-9-9")
                .with_location(location.clone())
                .problem(format!("Element <{}> requires child <{}>", parent, element))
                .build(),

            Error::UnexpectedElement {
                element,
                context,
                location,
            } => DiagnosticMessageBuilder::error("Unexpected Element")
                .with_code("Q-9-10")
                .with_location(location.clone())
                .problem(format!("Element <{}> is not valid in {}", element, context))
                .build(),

            Error::MissingTextSource { location } => DiagnosticMessageBuilder::error(
                "Missing Text Source",
            )
            .with_code("Q-9-11")
            .with_location(location.clone())
            .problem("Text element must specify a source using variable, macro, term, or value")
            .add_hint(
                "Add one of: variable=\"...\", macro=\"...\", term=\"...\", or value=\"...\"?",
            )
            .build(),

            Error::UndefinedMacro {
                name,
                reference_location,
                suggestion,
            } => {
                let mut builder = DiagnosticMessageBuilder::error("Undefined Macro")
                    .with_code("Q-9-12")
                    .with_location(reference_location.clone())
                    .problem(format!("Macro '{}' is not defined", name));

                if let Some(suggestion) = suggestion {
                    builder = builder.add_hint(format!("Did you mean '{}'?", suggestion));
                }

                builder.build()
            }

            Error::CircularMacro { chain, location } => {
                DiagnosticMessageBuilder::error("Circular Macro Dependency")
                    .with_code("Q-9-13")
                    .with_location(location.clone())
                    .problem("Macro references form a cycle")
                    .add_detail(format!("Cycle: {}", chain.join(" -> ")))
                    .build()
            }

            Error::DuplicateMacro {
                name,
                second_location,
                ..
            } => DiagnosticMessageBuilder::error("Duplicate Macro Definition")
                .with_code("Q-9-14")
                .with_location(second_location.clone())
                .problem(format!("Macro '{}' is already defined", name))
                .build(),

            Error::InvalidVersion { version, location } => {
                DiagnosticMessageBuilder::error("Invalid CSL Version")
                    .with_code("Q-9-15")
                    .with_location(location.clone())
                    .problem(format!("'{}' is not a valid CSL version", version))
                    .add_detail("Expected format: major.minor (e.g., '1.0')")
                    .build()
            }

            Error::InvalidRootElement { found, location } => {
                DiagnosticMessageBuilder::error("Invalid Root Element")
                    .with_code("Q-9-16")
                    .with_location(location.clone())
                    .problem(format!(
                        "CSL document must have <style> as root, found <{}>",
                        found
                    ))
                    .build()
            }
        }
    }
}
