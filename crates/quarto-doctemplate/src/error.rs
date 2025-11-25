/*
 * error.rs
 * Copyright (c) 2025 Posit, PBC
 */

//! Error types for template parsing and evaluation.

use thiserror::Error;

/// Errors that can occur during template operations.
#[derive(Debug, Error)]
pub enum TemplateError {
    /// Error parsing the template syntax.
    #[error("Parse error: {message}")]
    ParseError {
        message: String,
        // TODO: Add source location when we integrate with quarto-parse-errors
    },

    /// Error evaluating the template.
    #[error("Evaluation error: {message}")]
    EvaluationError { message: String },

    /// Error loading a partial template.
    #[error("Partial not found: {name}")]
    PartialNotFound { name: String },

    /// Recursive partial inclusion detected.
    #[error("Recursive partial inclusion detected (depth > {max_depth}): {name}")]
    RecursivePartial { name: String, max_depth: usize },

    /// Unknown pipe name.
    #[error("Unknown pipe: {name}")]
    UnknownPipe { name: String },

    /// Invalid pipe arguments.
    #[error("Invalid arguments for pipe '{pipe}': {message}")]
    InvalidPipeArgs { pipe: String, message: String },

    /// I/O error (e.g., reading partial file).
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

/// Result type for template operations.
pub type TemplateResult<T> = Result<T, TemplateError>;
