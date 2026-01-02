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

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_error_reporting::DiagnosticMessageBuilder;

    // === ParseError tests ===

    #[test]
    fn test_parse_error_new() {
        let diagnostics = vec![
            DiagnosticMessageBuilder::error("Test error")
                .problem("Something went wrong")
                .build(),
        ];
        let source_context = SourceContext::default();

        let error = ParseError::new(diagnostics.clone(), source_context);

        assert_eq!(error.diagnostics.len(), 1);
    }

    #[test]
    fn test_parse_error_render_empty() {
        let error = ParseError::new(vec![], SourceContext::default());
        let rendered = error.render();
        assert_eq!(rendered, "");
    }

    #[test]
    fn test_parse_error_render_single() {
        let diagnostics = vec![
            DiagnosticMessageBuilder::error("Test error")
                .problem("Something went wrong")
                .build(),
        ];
        let error = ParseError::new(diagnostics, SourceContext::default());

        let rendered = error.render();
        assert!(rendered.contains("Test error"));
        assert!(rendered.contains("Something went wrong"));
    }

    #[test]
    fn test_parse_error_render_multiple() {
        let diagnostics = vec![
            DiagnosticMessageBuilder::error("Error 1")
                .problem("Problem 1")
                .build(),
            DiagnosticMessageBuilder::warning("Warning 1")
                .problem("Problem 2")
                .build(),
        ];
        let error = ParseError::new(diagnostics, SourceContext::default());

        let rendered = error.render();
        assert!(rendered.contains("Error 1"));
        assert!(rendered.contains("Warning 1"));
    }

    #[test]
    fn test_parse_error_display() {
        let diagnostics = vec![
            DiagnosticMessageBuilder::error("Display test")
                .problem("Testing display")
                .build(),
        ];
        let error = ParseError::new(diagnostics, SourceContext::default());

        let displayed = format!("{}", error);
        assert!(displayed.contains("Display test"));
    }

    #[test]
    fn test_parse_error_is_error_trait() {
        let diagnostics = vec![DiagnosticMessageBuilder::error("Error trait test").build()];
        let error = ParseError::new(diagnostics, SourceContext::default());

        // Verify it implements std::error::Error
        let _: &dyn std::error::Error = &error;
    }

    #[test]
    fn test_parse_error_clone() {
        let diagnostics = vec![DiagnosticMessageBuilder::error("Clone test").build()];
        let error = ParseError::new(diagnostics, SourceContext::default());
        let cloned = error.clone();

        assert_eq!(error.diagnostics.len(), cloned.diagnostics.len());
    }

    // === QuartoError tests ===

    #[test]
    fn test_quarto_error_other() {
        let error = QuartoError::other("Custom error message");
        let display = format!("{}", error);
        assert_eq!(display, "Custom error message");
    }

    #[test]
    fn test_quarto_error_other_from_string() {
        let error = QuartoError::other(String::from("String error"));
        let display = format!("{}", error);
        assert_eq!(display, "String error");
    }

    #[test]
    fn test_quarto_error_not_implemented() {
        let error = QuartoError::NotImplemented("render".to_string());
        let display = format!("{}", error);
        assert_eq!(display, "Command not yet implemented: render");
    }

    #[test]
    fn test_quarto_error_transform() {
        let error = QuartoError::Transform("Failed to transform AST".to_string());
        let display = format!("{}", error);
        assert_eq!(display, "Transform error: Failed to transform AST");
    }

    #[test]
    fn test_quarto_error_render() {
        let error = QuartoError::Render("Template not found".to_string());
        let display = format!("{}", error);
        assert_eq!(display, "Render error: Template not found");
    }

    #[test]
    fn test_quarto_error_io_from() {
        let io_error = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let error: QuartoError = io_error.into();

        let display = format!("{}", error);
        assert!(display.contains("IO error"));
        assert!(display.contains("file not found"));
    }

    #[test]
    fn test_quarto_error_parse() {
        let diagnostics = vec![DiagnosticMessageBuilder::error("Parse failed").build()];
        let parse_error = ParseError::new(diagnostics, SourceContext::default());
        let error = QuartoError::Parse(parse_error);

        let display = format!("{}", error);
        assert!(display.contains("Parse failed"));
    }

    #[test]
    fn test_quarto_error_debug() {
        let error = QuartoError::other("Debug test");
        let debug = format!("{:?}", error);
        assert!(debug.contains("Other"));
        assert!(debug.contains("Debug test"));
    }

    #[test]
    fn test_result_type_alias() {
        fn returns_ok() -> Result<i32> {
            Ok(42)
        }

        fn returns_err() -> Result<i32> {
            Err(QuartoError::other("error"))
        }

        assert_eq!(returns_ok().unwrap(), 42);
        assert!(returns_err().is_err());
    }
}
