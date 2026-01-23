//! Error types for SASS operations.
//!
//! Copyright (c) 2025 Posit, PBC

use thiserror::Error;

/// Errors that can occur during SASS operations
#[derive(Debug, Error)]
pub enum SassError {
    /// Layer parsing failed - no boundary markers found
    #[error("SCSS content doesn't contain any layer boundary markers (/*-- scss:defaults --*/, /*-- scss:rules --*/, etc.){}", .hint.as_ref().map(|h| format!(" in {}", h)).unwrap_or_default())]
    NoBoundaryMarkers { hint: Option<String> },

    /// SASS compilation failed
    #[error("SASS compilation failed: {message}")]
    CompilationFailed { message: String },

    /// File I/O error
    #[error("Failed to read SASS file: {0}")]
    Io(#[from] std::io::Error),
}
