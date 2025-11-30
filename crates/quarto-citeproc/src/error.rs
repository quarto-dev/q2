//! Error types for citation processing.

use quarto_error_reporting::{DiagnosticMessage, DiagnosticMessageBuilder};
use quarto_source_map::SourceInfo;
use std::fmt;

/// Result type alias for quarto-citeproc operations.
pub type Result<T> = std::result::Result<T, Error>;

/// Errors that can occur during citation processing.
#[derive(Debug, Clone)]
pub enum Error {
    /// CSL parsing error.
    CslError(quarto_csl::Error),

    /// Locale file parsing error.
    LocaleParseError { locale: String, message: String },

    /// Reference not found.
    ReferenceNotFound {
        id: String,
        location: Option<SourceInfo>,
    },

    /// Invalid reference data.
    InvalidReference {
        id: String,
        field: String,
        message: String,
    },

    /// Missing required field in reference.
    MissingRequiredField { id: String, field: String },

    /// Invalid date format.
    InvalidDate {
        id: String,
        field: String,
        value: String,
    },

    /// Evaluation error during citation processing.
    EvaluationError {
        message: String,
        location: Option<SourceInfo>,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::CslError(e) => write!(f, "CSL error: {}", e),
            Error::LocaleParseError { locale, message } => {
                write!(f, "Failed to parse locale '{}': {}", locale, message)
            }
            Error::ReferenceNotFound { id, .. } => {
                write!(f, "Reference '{}' not found", id)
            }
            Error::InvalidReference { id, field, message } => {
                write!(
                    f,
                    "Invalid reference '{}' field '{}': {}",
                    id, field, message
                )
            }
            Error::MissingRequiredField { id, field } => {
                write!(f, "Reference '{}' missing required field '{}'", id, field)
            }
            Error::InvalidDate { id, field, value } => {
                write!(
                    f,
                    "Invalid date '{}' in reference '{}' field '{}'",
                    value, id, field
                )
            }
            Error::EvaluationError { message, .. } => {
                write!(f, "Evaluation error: {}", message)
            }
        }
    }
}

impl std::error::Error for Error {}

impl From<quarto_csl::Error> for Error {
    fn from(err: quarto_csl::Error) -> Self {
        Error::CslError(err)
    }
}

impl Error {
    /// Convert this error to a DiagnosticMessage.
    ///
    /// Citeproc errors use Q-10-* error codes (subsystem 10).
    pub fn to_diagnostic(&self) -> DiagnosticMessage {
        match self {
            Error::CslError(e) => e.to_diagnostic(),

            Error::LocaleParseError { locale, message } => {
                DiagnosticMessageBuilder::error("Locale Parse Error")
                    .with_code("Q-10-1")
                    .problem(format!("Failed to parse locale '{}'", locale))
                    .add_detail(message.clone())
                    .build()
            }

            Error::ReferenceNotFound { id, location } => {
                let mut builder = DiagnosticMessageBuilder::error("Reference Not Found")
                    .with_code("Q-10-2")
                    .problem(format!("Reference '{}' is not defined", id));

                if let Some(loc) = location {
                    builder = builder.with_location(loc.clone());
                }

                builder.build()
            }

            Error::InvalidReference { id, field, message } => {
                DiagnosticMessageBuilder::error("Invalid Reference")
                    .with_code("Q-10-3")
                    .problem(format!("Reference '{}' has invalid field '{}'", id, field))
                    .add_detail(message.clone())
                    .build()
            }

            Error::MissingRequiredField { id, field } => {
                DiagnosticMessageBuilder::error("Missing Required Field")
                    .with_code("Q-10-4")
                    .problem(format!(
                        "Reference '{}' is missing required field '{}'",
                        id, field
                    ))
                    .build()
            }

            Error::InvalidDate { id, field, value } => {
                DiagnosticMessageBuilder::error("Invalid Date")
                    .with_code("Q-10-5")
                    .problem(format!(
                        "Invalid date '{}' in reference '{}' field '{}'",
                        value, id, field
                    ))
                    .add_hint("Dates should be in [year, month, day] format")
                    .build()
            }

            Error::EvaluationError { message, location } => {
                let mut builder = DiagnosticMessageBuilder::error("Evaluation Error")
                    .with_code("Q-10-6")
                    .problem(message.clone());

                if let Some(loc) = location {
                    builder = builder.with_location(loc.clone());
                }

                builder.build()
            }
        }
    }
}
