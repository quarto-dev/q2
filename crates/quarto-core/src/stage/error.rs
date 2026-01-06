/*
 * stage/error.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Pipeline error types.
 */

//! Error types for the render pipeline.
//!
//! [`PipelineError`] represents errors that can occur during pipeline execution,
//! while [`PipelineValidationError`] represents errors in pipeline construction.

use quarto_error_reporting::DiagnosticMessage;

use super::data::PipelineDataKind;

/// Error that occurs during pipeline validation (construction).
#[derive(Debug, Clone)]
pub enum PipelineValidationError {
    /// Pipeline has no stages
    Empty,

    /// Stage output type doesn't match next stage's input type
    TypeMismatch {
        /// Name of the stage producing the output
        stage_a: String,
        /// Name of the stage expecting the input
        stage_b: String,
        /// Type produced by stage_a
        output: PipelineDataKind,
        /// Type expected by stage_b
        input: PipelineDataKind,
    },
}

impl std::fmt::Display for PipelineValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PipelineValidationError::Empty => {
                write!(f, "Pipeline has no stages")
            }
            PipelineValidationError::TypeMismatch {
                stage_a,
                stage_b,
                output,
                input,
            } => {
                write!(
                    f,
                    "Type mismatch: stage '{}' produces {} but stage '{}' expects {}",
                    stage_a, output, stage_b, input
                )
            }
        }
    }
}

impl std::error::Error for PipelineValidationError {}

/// Error that occurs during pipeline execution.
#[derive(Debug)]
pub enum PipelineError {
    /// Wrong input type for stage
    UnexpectedInput {
        /// Name of the stage that received wrong input
        stage: String,
        /// Type the stage expected
        expected: PipelineDataKind,
        /// Type the stage received
        got: PipelineDataKind,
    },

    /// Stage execution failed with diagnostics
    StageError {
        /// Name of the stage that failed
        stage: String,
        /// Diagnostic messages describing the error
        diagnostics: Vec<DiagnosticMessage>,
    },

    /// Pipeline was cancelled (e.g., Ctrl+C)
    Cancelled,

    /// Pipeline validation failed
    Validation(PipelineValidationError),

    /// I/O error during stage execution
    Io(std::io::Error),

    /// Other error with message
    Other(String),
}

impl PipelineError {
    /// Create an UnexpectedInput error.
    pub fn unexpected_input(
        stage: impl Into<String>,
        expected: PipelineDataKind,
        got: PipelineDataKind,
    ) -> Self {
        Self::UnexpectedInput {
            stage: stage.into(),
            expected,
            got,
        }
    }

    /// Create a StageError with a single message.
    pub fn stage_error(stage: impl Into<String>, message: impl Into<String>) -> Self {
        Self::StageError {
            stage: stage.into(),
            diagnostics: vec![DiagnosticMessage::error(message.into())],
        }
    }

    /// Create a StageError with multiple diagnostics.
    pub fn stage_error_with_diagnostics(
        stage: impl Into<String>,
        diagnostics: Vec<DiagnosticMessage>,
    ) -> Self {
        Self::StageError {
            stage: stage.into(),
            diagnostics,
        }
    }

    /// Create an Other error.
    pub fn other(message: impl Into<String>) -> Self {
        Self::Other(message.into())
    }

    /// Check if this is a cancellation error.
    pub fn is_cancelled(&self) -> bool {
        matches!(self, Self::Cancelled)
    }
}

impl std::fmt::Display for PipelineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PipelineError::UnexpectedInput {
                stage,
                expected,
                got,
            } => {
                write!(
                    f,
                    "Stage '{}' expected input type {} but got {}",
                    stage, expected, got
                )
            }
            PipelineError::StageError { stage, diagnostics } => {
                if diagnostics.is_empty() {
                    write!(f, "Stage '{}' failed", stage)
                } else {
                    write!(f, "Stage '{}' failed: {}", stage, diagnostics[0].title)
                }
            }
            PipelineError::Cancelled => write!(f, "Pipeline execution was cancelled"),
            PipelineError::Validation(e) => write!(f, "Pipeline validation error: {}", e),
            PipelineError::Io(e) => write!(f, "I/O error: {}", e),
            PipelineError::Other(msg) => write!(f, "{}", msg),
        }
    }
}

impl std::error::Error for PipelineError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            PipelineError::Validation(e) => Some(e),
            PipelineError::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for PipelineError {
    fn from(e: std::io::Error) -> Self {
        PipelineError::Io(e)
    }
}

impl From<PipelineValidationError> for PipelineError {
    fn from(e: PipelineValidationError) -> Self {
        PipelineError::Validation(e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validation_error_display() {
        let err = PipelineValidationError::Empty;
        assert!(err.to_string().contains("no stages"));

        let err = PipelineValidationError::TypeMismatch {
            stage_a: "parse".to_string(),
            stage_b: "render".to_string(),
            output: PipelineDataKind::LoadedSource,
            input: PipelineDataKind::DocumentAst,
        };
        let msg = err.to_string();
        assert!(msg.contains("parse"));
        assert!(msg.contains("render"));
        assert!(msg.contains("LoadedSource"));
        assert!(msg.contains("DocumentAst"));
    }

    #[test]
    fn test_pipeline_error_display() {
        let err = PipelineError::unexpected_input(
            "parse",
            PipelineDataKind::DocumentSource,
            PipelineDataKind::LoadedSource,
        );
        let msg = err.to_string();
        assert!(msg.contains("parse"));
        assert!(msg.contains("DocumentSource"));
        assert!(msg.contains("LoadedSource"));
    }

    #[test]
    fn test_stage_error_with_message() {
        let err = PipelineError::stage_error("parse", "Syntax error on line 5");
        let msg = err.to_string();
        assert!(msg.contains("parse"));
        assert!(msg.contains("Syntax error"));
    }

    #[test]
    fn test_cancelled_error() {
        let err = PipelineError::Cancelled;
        assert!(err.is_cancelled());
        assert!(err.to_string().contains("cancelled"));
    }

    #[test]
    fn test_io_error_conversion() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let err: PipelineError = io_err.into();
        assert!(matches!(err, PipelineError::Io(_)));
        assert!(err.to_string().contains("I/O error"));
    }

    #[test]
    fn test_validation_error_conversion() {
        let val_err = PipelineValidationError::Empty;
        let err: PipelineError = val_err.into();
        assert!(matches!(err, PipelineError::Validation(_)));
    }
}
