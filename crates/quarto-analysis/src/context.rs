//! Analysis context trait and implementations.
//!
//! The [`AnalysisContext`] trait defines the interface for document analysis operations.
//! It provides source tracking and diagnostic reporting without requiring the full
//! rendering infrastructure.
//!
//! Note: Document metadata is accessed directly from the `Pandoc` AST passed to transforms,
//! not from the context. This avoids duplication and ensures transforms always see
//! the current metadata state.

use quarto_error_reporting::DiagnosticMessage;
use quarto_source_map::SourceContext;

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
pub trait AnalysisContext {
    /// Access source context for location mapping.
    fn source_context(&self) -> &SourceContext;

    /// Report a diagnostic (warning or error) during analysis.
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
/// let ctx = DocumentAnalysisContext::new(source_context);
/// // ... run analysis transforms (they access pandoc.meta directly) ...
/// let diagnostics = ctx.into_diagnostics();
/// ```
pub struct DocumentAnalysisContext {
    source_context: SourceContext,
    diagnostics: Vec<DiagnosticMessage>,
}

impl DocumentAnalysisContext {
    /// Create a new analysis context.
    pub fn new(source_context: SourceContext) -> Self {
        Self {
            source_context,
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

impl AnalysisContext for DocumentAnalysisContext {
    fn source_context(&self) -> &SourceContext {
        &self.source_context
    }

    fn add_diagnostic(&mut self, msg: DiagnosticMessage) {
        self.diagnostics.push(msg);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_document_analysis_context_basic() {
        let source_context = SourceContext::default();

        let mut ctx = DocumentAnalysisContext::new(source_context);

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
