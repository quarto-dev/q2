//! Context for XML parsing with diagnostic collection.

use quarto_error_reporting::DiagnosticMessage;

/// Context for XML parsing that collects diagnostics.
///
/// This follows the pattern used in `QmdWriterContext` - diagnostics are
/// accumulated during parsing and can be retrieved afterwards. This allows
/// for warnings and lints even on successful parses.
///
/// # Example
///
/// ```rust
/// use quarto_xml::{parse_with_context, XmlParseContext};
///
/// let mut ctx = XmlParseContext::new();
/// match parse_with_context("<root/>", &mut ctx) {
///     Ok(xml) => {
///         // Check for warnings even on success
///         if ctx.has_diagnostics() {
///             for diag in ctx.diagnostics() {
///                 eprintln!("Warning: {}", diag.title);
///             }
///         }
///     }
///     Err(errors) => {
///         for err in errors {
///             eprintln!("Error: {}", err.title);
///         }
///     }
/// }
/// ```
#[derive(Debug, Default)]
pub struct XmlParseContext {
    /// Accumulated diagnostic messages during parsing.
    diagnostics: Vec<DiagnosticMessage>,
}

impl XmlParseContext {
    /// Create a new XML parse context.
    pub fn new() -> Self {
        Self {
            diagnostics: Vec::new(),
        }
    }

    /// Add a diagnostic message to the context.
    pub fn add_diagnostic(&mut self, diagnostic: DiagnosticMessage) {
        self.diagnostics.push(diagnostic);
    }

    /// Check if any diagnostics have been collected.
    pub fn has_diagnostics(&self) -> bool {
        !self.diagnostics.is_empty()
    }

    /// Get all collected diagnostics.
    pub fn diagnostics(&self) -> &[DiagnosticMessage] {
        &self.diagnostics
    }

    /// Take all collected diagnostics, leaving the context empty.
    pub fn take_diagnostics(&mut self) -> Vec<DiagnosticMessage> {
        std::mem::take(&mut self.diagnostics)
    }

    /// Check if any errors (not warnings) have been collected.
    pub fn has_errors(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|d| d.kind == quarto_error_reporting::DiagnosticKind::Error)
    }
}
