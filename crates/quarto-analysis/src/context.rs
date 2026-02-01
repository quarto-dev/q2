//! Analysis context trait and implementations.
//!
//! The [`AnalysisContext`] trait defines the interface for document analysis operations.
//! It provides diagnostic reporting without requiring the full rendering infrastructure.
//!
//! Note: Document metadata is accessed directly from the `Pandoc` AST passed to transforms,
//! not from the context. This avoids duplication and ensures transforms always see
//! the current metadata state.

use quarto_error_reporting::DiagnosticMessage;

/// Trait for contexts that support document analysis operations.
///
/// This trait defines the interface for analysis transforms that can run
/// at "LSP speed" - no I/O, no code execution, just AST manipulation
/// based on document metadata and structure.
///
/// Document metadata is accessed directly from the `Pandoc` AST passed to
/// [`AnalysisTransform::transform`], not from this context. This ensures
/// transforms always see the current metadata and avoids duplication.
///
/// # Implementations
///
/// - [`DocumentAnalysisContext`] - Lightweight implementation for LSP
/// - `RenderContext` (in quarto-core) - Full implementation for rendering
pub trait AnalysisContext {
    /// Report a diagnostic (warning, error, or info) during analysis.
    fn add_diagnostic(&mut self, msg: DiagnosticMessage);
}

/// Lightweight analysis context for LSP and standalone analysis.
///
/// This struct provides a minimal implementation of [`AnalysisContext`]
/// suitable for use in the LSP server and other contexts where full
/// rendering is not needed.
///
/// # Example
///
/// ```rust,ignore
/// use quarto_analysis::DocumentAnalysisContext;
///
/// let ctx = DocumentAnalysisContext::new();
/// // ... run analysis transforms (they access pandoc.meta directly) ...
/// let diagnostics = ctx.into_diagnostics();
/// ```
pub struct DocumentAnalysisContext {
    diagnostics: Vec<DiagnosticMessage>,
}

impl DocumentAnalysisContext {
    /// Create a new analysis context.
    pub fn new() -> Self {
        Self {
            diagnostics: Vec::new(),
        }
    }

    /// Get the collected diagnostics.
    pub fn diagnostics(&self) -> &[DiagnosticMessage] {
        &self.diagnostics
    }

    /// Consume the context and return the collected diagnostics.
    pub fn into_diagnostics(self) -> Vec<DiagnosticMessage> {
        self.diagnostics
    }
}

impl Default for DocumentAnalysisContext {
    fn default() -> Self {
        Self::new()
    }
}

impl AnalysisContext for DocumentAnalysisContext {
    fn add_diagnostic(&mut self, msg: DiagnosticMessage) {
        self.diagnostics.push(msg);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_document_analysis_context_basic() {
        let mut ctx = DocumentAnalysisContext::new();

        // Initially no diagnostics
        assert!(ctx.diagnostics().is_empty());

        // Add a diagnostic
        let diag = quarto_error_reporting::DiagnosticMessageBuilder::warning("Test warning")
            .problem("This is a test")
            .build();
        ctx.add_diagnostic(diag);

        // Now has one diagnostic
        assert_eq!(ctx.diagnostics().len(), 1);
        assert_eq!(ctx.diagnostics()[0].title, "Test warning");
    }
}
