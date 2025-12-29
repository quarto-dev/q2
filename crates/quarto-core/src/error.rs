//! Error types for quarto-core

use quarto_error_reporting::DiagnosticMessage;
use quarto_source_map::SourceContext;
use thiserror::Error;

/// Structured parse error with diagnostics and source context.
///
/// This preserves the rich diagnostic information from parsing,
/// allowing for ariadne-style source snippets in error messages.
#[derive(Debug, Clone)]
pub struct ParseError {
    /// Diagnostic messages from parsing
    pub diagnostics: Vec<DiagnosticMessage>,
    /// Source context for ariadne rendering (contains file content)
    pub source_context: SourceContext,
}

impl ParseError {
    /// Create a new parse error with diagnostics and source context.
    pub fn new(diagnostics: Vec<DiagnosticMessage>, source_context: SourceContext) -> Self {
        Self {
            diagnostics,
            source_context,
        }
    }

    /// Render all diagnostics with ariadne source context.
    ///
    /// This produces beautiful error messages with source snippets,
    /// line numbers, and visual markers pointing to error locations.
    pub fn render(&self) -> String {
        self.diagnostics
            .iter()
            .map(|d| d.to_text(Some(&self.source_context)))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.render())
    }
}

impl std::error::Error for ParseError {}

#[derive(Error, Debug)]
pub enum QuartoError {
    #[error("Command not yet implemented: {0}")]
    NotImplemented(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("{0}")]
    Parse(#[source] ParseError),

    #[error("Transform error: {0}")]
    Transform(String),

    #[error("Render error: {0}")]
    Render(String),

    #[error("{0}")]
    Other(String),
}

impl QuartoError {
    /// Create an error from any message.
    pub fn other(msg: impl Into<String>) -> Self {
        Self::Other(msg.into())
    }
}

pub type Result<T> = std::result::Result<T, QuartoError>;
