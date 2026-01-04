/*
 * filter_context.rs
 * Copyright (c) 2025 Posit, PBC
 */

//! Context for filter execution, enabling diagnostics and source tracking.

use crate::utils::diagnostic_collector::DiagnosticCollector;
use quarto_error_reporting::DiagnosticMessage;
use quarto_source_map::SourceInfo;

/// Context for filter execution, enabling diagnostics and source tracking.
///
/// This context is threaded through filter traversal functions to allow
/// filters to emit warnings and errors with proper source locations.
pub struct FilterContext {
    /// Accumulated diagnostics (warnings and non-fatal errors)
    pub diagnostics: DiagnosticCollector,
}

impl FilterContext {
    /// Create a new empty filter context
    pub fn new() -> Self {
        Self {
            diagnostics: DiagnosticCollector::new(),
        }
    }

    /// Add a warning
    pub fn warn(&mut self, message: impl Into<String>) {
        self.diagnostics.warn(message);
    }

    /// Add a warning with source location
    pub fn warn_at(&mut self, message: impl Into<String>, location: SourceInfo) {
        self.diagnostics.warn_at(message, location);
    }

    /// Add an error
    pub fn error(&mut self, message: impl Into<String>) {
        self.diagnostics.error(message);
    }

    /// Add an error with source location
    pub fn error_at(&mut self, message: impl Into<String>, location: SourceInfo) {
        self.diagnostics.error_at(message, location);
    }

    /// Check if any errors were collected
    pub fn has_errors(&self) -> bool {
        self.diagnostics.has_errors()
    }

    /// Consume context and return diagnostics
    pub fn into_diagnostics(self) -> Vec<DiagnosticMessage> {
        self.diagnostics.into_diagnostics()
    }

    /// Get reference to diagnostics
    pub fn diagnostics(&self) -> &[DiagnosticMessage] {
        self.diagnostics.diagnostics()
    }
}

impl Default for FilterContext {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_filter_context() {
        let ctx = FilterContext::new();
        assert!(!ctx.has_errors());
        assert!(ctx.diagnostics().is_empty());
    }

    #[test]
    fn test_default_filter_context() {
        let ctx = FilterContext::default();
        assert!(!ctx.has_errors());
    }

    #[test]
    fn test_warn() {
        let mut ctx = FilterContext::new();
        ctx.warn("Test warning");
        assert!(!ctx.has_errors()); // Warnings don't count as errors
        assert_eq!(ctx.diagnostics().len(), 1);
    }

    #[test]
    fn test_error() {
        let mut ctx = FilterContext::new();
        ctx.error("Test error");
        assert!(ctx.has_errors());
        assert_eq!(ctx.diagnostics().len(), 1);
    }

    #[test]
    fn test_into_diagnostics() {
        let mut ctx = FilterContext::new();
        ctx.warn("Warning 1");
        ctx.error("Error 1");
        let diagnostics = ctx.into_diagnostics();
        assert_eq!(diagnostics.len(), 2);
    }

    #[test]
    fn test_multiple_diagnostics() {
        let mut ctx = FilterContext::new();
        ctx.warn("Warning 1");
        ctx.warn("Warning 2");
        ctx.error("Error 1");
        assert!(ctx.has_errors());
        assert_eq!(ctx.diagnostics().len(), 3);
    }

    #[test]
    fn test_warn_at() {
        let mut ctx = FilterContext::new();
        let source_info = SourceInfo::original(quarto_source_map::FileId(1), 10, 20);
        ctx.warn_at("Warning with location", source_info);
        assert!(!ctx.has_errors()); // Warnings don't count as errors
        assert_eq!(ctx.diagnostics().len(), 1);
    }

    #[test]
    fn test_error_at() {
        let mut ctx = FilterContext::new();
        let source_info = SourceInfo::original(quarto_source_map::FileId(1), 10, 20);
        ctx.error_at("Error with location", source_info);
        assert!(ctx.has_errors());
        assert_eq!(ctx.diagnostics().len(), 1);
    }

    #[test]
    fn test_mixed_diagnostics_with_locations() {
        let mut ctx = FilterContext::new();
        let source_info1 = SourceInfo::original(quarto_source_map::FileId(1), 0, 10);
        let source_info2 = SourceInfo::original(quarto_source_map::FileId(1), 20, 30);

        ctx.warn("Plain warning");
        ctx.warn_at("Warning at line 1", source_info1);
        ctx.error("Plain error");
        ctx.error_at("Error at line 2", source_info2);

        assert!(ctx.has_errors());
        assert_eq!(ctx.diagnostics().len(), 4);
    }
}
