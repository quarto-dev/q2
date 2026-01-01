/*
 * eval_context.rs
 * Copyright (c) 2025 Posit, PBC
 */

//! Evaluation context for template rendering.
//!
//! This module provides [`EvalContext`], which is threaded through all evaluation
//! functions to support:
//!
//! 1. **Diagnostics**: Collect errors and warnings with source locations
//! 2. **State tracking**: Partial nesting depth for recursion protection
//! 3. **Configuration**: Strict mode for treating warnings as errors

use crate::context::TemplateContext;
use quarto_error_reporting::{DiagnosticKind, DiagnosticMessage, DiagnosticMessageBuilder};
use quarto_source_map::SourceInfo;

/// Collector for diagnostic messages during template evaluation.
///
/// This is a simplified version of the DiagnosticCollector from pampa,
/// tailored for template evaluation.
#[derive(Debug, Default)]
pub struct DiagnosticCollector {
    diagnostics: Vec<DiagnosticMessage>,
}

impl DiagnosticCollector {
    /// Create a new empty diagnostic collector.
    pub fn new() -> Self {
        Self {
            diagnostics: Vec::new(),
        }
    }

    /// Add a diagnostic message.
    pub fn add(&mut self, diagnostic: DiagnosticMessage) {
        self.diagnostics.push(diagnostic);
    }

    /// Add an error message with source location.
    pub fn error_at(&mut self, message: impl Into<String>, location: SourceInfo) {
        let diagnostic = DiagnosticMessageBuilder::error(message)
            .with_location(location)
            .build();
        self.add(diagnostic);
    }

    /// Add a warning message with source location.
    pub fn warn_at(&mut self, message: impl Into<String>, location: SourceInfo) {
        let diagnostic = DiagnosticMessageBuilder::warning(message)
            .with_location(location)
            .build();
        self.add(diagnostic);
    }

    /// Add an error message with error code and source location.
    pub fn error_with_code(
        &mut self,
        code: &str,
        message: impl Into<String>,
        location: SourceInfo,
    ) {
        let diagnostic = DiagnosticMessageBuilder::error(message)
            .with_code(code)
            .with_location(location)
            .build();
        self.add(diagnostic);
    }

    /// Add a warning message with error code and source location.
    pub fn warn_with_code(&mut self, code: &str, message: impl Into<String>, location: SourceInfo) {
        let diagnostic = DiagnosticMessageBuilder::warning(message)
            .with_code(code)
            .with_location(location)
            .build();
        self.add(diagnostic);
    }

    /// Check if any errors were collected (warnings don't count).
    pub fn has_errors(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|d| d.kind == DiagnosticKind::Error)
    }

    /// Get a reference to the collected diagnostics.
    pub fn diagnostics(&self) -> &[DiagnosticMessage] {
        &self.diagnostics
    }

    /// Consume the collector and return the diagnostics, sorted by source location.
    pub fn into_diagnostics(mut self) -> Vec<DiagnosticMessage> {
        self.diagnostics
            .sort_by_key(|diag| diag.location.as_ref().map_or(0, |loc| loc.start_offset()));
        self.diagnostics
    }

    /// Check if the collector is empty.
    pub fn is_empty(&self) -> bool {
        self.diagnostics.is_empty()
    }
}

/// Context for template evaluation.
///
/// This struct is threaded through all evaluation functions to:
/// 1. Collect diagnostics (errors and warnings) with source locations
/// 2. Track evaluation state (e.g., partial nesting depth)
/// 3. Provide access to the variable context
pub struct EvalContext<'a> {
    /// Variable bindings for template interpolation.
    pub variables: &'a TemplateContext,

    /// Diagnostic collector for errors and warnings.
    pub diagnostics: DiagnosticCollector,

    /// Current partial nesting depth (for recursion protection).
    pub partial_depth: usize,

    /// Maximum partial nesting depth before error.
    pub max_partial_depth: usize,

    /// Strict mode: treat warnings (e.g., undefined variables) as errors.
    pub strict_mode: bool,
}

impl<'a> EvalContext<'a> {
    /// Create a new evaluation context with the given variable bindings.
    pub fn new(variables: &'a TemplateContext) -> Self {
        Self {
            variables,
            diagnostics: DiagnosticCollector::new(),
            partial_depth: 0,
            max_partial_depth: 50,
            strict_mode: false,
        }
    }

    /// Enable or disable strict mode.
    ///
    /// In strict mode, warnings (like undefined variables) are treated as errors.
    pub fn with_strict_mode(mut self, strict: bool) -> Self {
        self.strict_mode = strict;
        self
    }

    /// Set the maximum partial nesting depth.
    pub fn with_max_partial_depth(mut self, depth: usize) -> Self {
        self.max_partial_depth = depth;
        self
    }

    /// Create a child context for nested evaluation (e.g., for loops).
    ///
    /// The child context has fresh diagnostics but inherits configuration
    /// like strict_mode and max_partial_depth.
    pub fn child(&self, child_variables: &'a TemplateContext) -> EvalContext<'a> {
        EvalContext {
            variables: child_variables,
            diagnostics: DiagnosticCollector::new(),
            partial_depth: self.partial_depth,
            max_partial_depth: self.max_partial_depth,
            strict_mode: self.strict_mode,
        }
    }

    /// Merge diagnostics from a child context into this context.
    pub fn merge_diagnostics(&mut self, child: EvalContext) {
        for diag in child.diagnostics.into_diagnostics() {
            self.diagnostics.add(diag);
        }
    }

    /// Add an error with source location.
    pub fn error_at(&mut self, message: impl Into<String>, location: &SourceInfo) {
        self.diagnostics.error_at(message, location.clone());
    }

    /// Add a warning with source location.
    pub fn warn_at(&mut self, message: impl Into<String>, location: &SourceInfo) {
        self.diagnostics.warn_at(message, location.clone());
    }

    /// Add an error or warning depending on strict mode.
    ///
    /// In strict mode, this adds an error. Otherwise, it adds a warning.
    pub fn warn_or_error_at(&mut self, message: impl Into<String>, location: &SourceInfo) {
        if self.strict_mode {
            self.error_at(message, location);
        } else {
            self.warn_at(message, location);
        }
    }

    /// Add an error with error code and source location.
    pub fn error_with_code(
        &mut self,
        code: &str,
        message: impl Into<String>,
        location: &SourceInfo,
    ) {
        self.diagnostics
            .error_with_code(code, message, location.clone());
    }

    /// Add a warning with error code and source location.
    pub fn warn_with_code(
        &mut self,
        code: &str,
        message: impl Into<String>,
        location: &SourceInfo,
    ) {
        self.diagnostics
            .warn_with_code(code, message, location.clone());
    }

    /// Add an error or warning with error code depending on strict mode.
    ///
    /// In strict mode, this adds an error. Otherwise, it adds a warning.
    pub fn warn_or_error_with_code(
        &mut self,
        code: &str,
        message: impl Into<String>,
        location: &SourceInfo,
    ) {
        if self.strict_mode {
            self.error_with_code(code, message, location);
        } else {
            self.warn_with_code(code, message, location);
        }
    }

    /// Add a structured diagnostic message.
    pub fn add_diagnostic(&mut self, diagnostic: DiagnosticMessage) {
        self.diagnostics.add(diagnostic);
    }

    /// Check if any errors have been collected.
    pub fn has_errors(&self) -> bool {
        self.diagnostics.has_errors()
    }

    /// Consume the context and return collected diagnostics.
    pub fn into_diagnostics(self) -> Vec<DiagnosticMessage> {
        self.diagnostics.into_diagnostics()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_diagnostic_collector_new() {
        let collector = DiagnosticCollector::new();
        assert!(collector.is_empty());
        assert!(!collector.has_errors());
    }

    #[test]
    fn test_diagnostic_collector_error() {
        let mut collector = DiagnosticCollector::new();
        let location = SourceInfo::default();
        collector.error_at("Test error", location);

        assert!(!collector.is_empty());
        assert!(collector.has_errors());
        assert_eq!(collector.diagnostics().len(), 1);
    }

    #[test]
    fn test_diagnostic_collector_warning() {
        let mut collector = DiagnosticCollector::new();
        let location = SourceInfo::default();
        collector.warn_at("Test warning", location);

        assert!(!collector.is_empty());
        assert!(!collector.has_errors()); // Warnings don't count as errors
        assert_eq!(collector.diagnostics().len(), 1);
    }

    #[test]
    fn test_eval_context_new() {
        let vars = TemplateContext::new();
        let ctx = EvalContext::new(&vars);

        assert!(!ctx.strict_mode);
        assert_eq!(ctx.partial_depth, 0);
        assert_eq!(ctx.max_partial_depth, 50);
        assert!(!ctx.has_errors());
    }

    #[test]
    fn test_eval_context_strict_mode() {
        let vars = TemplateContext::new();
        let ctx = EvalContext::new(&vars).with_strict_mode(true);

        assert!(ctx.strict_mode);
    }

    #[test]
    fn test_eval_context_warn_or_error() {
        let vars = TemplateContext::new();
        let location = SourceInfo::default();

        // Normal mode: warning
        let mut ctx = EvalContext::new(&vars);
        ctx.warn_or_error_at("Test", &location);
        assert!(!ctx.has_errors());

        // Strict mode: error
        let mut ctx_strict = EvalContext::new(&vars).with_strict_mode(true);
        ctx_strict.warn_or_error_at("Test", &location);
        assert!(ctx_strict.has_errors());
    }

    #[test]
    fn test_eval_context_child() {
        let vars = TemplateContext::new();
        let ctx = EvalContext::new(&vars)
            .with_strict_mode(true)
            .with_max_partial_depth(25);

        let child_vars = TemplateContext::new();
        let child = ctx.child(&child_vars);

        // Child inherits configuration
        assert!(child.strict_mode);
        assert_eq!(child.max_partial_depth, 25);
        // Child has fresh diagnostics
        assert!(!child.has_errors());
    }

    #[test]
    fn test_eval_context_merge_diagnostics() {
        let vars = TemplateContext::new();
        let location = SourceInfo::default();

        let mut parent = EvalContext::new(&vars);
        parent.warn_at("Parent warning", &location);

        let child_vars = TemplateContext::new();
        let mut child = parent.child(&child_vars);
        child.error_at("Child error", &location);

        parent.merge_diagnostics(child);

        assert!(parent.has_errors());
        let diagnostics = parent.into_diagnostics();
        assert_eq!(diagnostics.len(), 2);
    }

    #[test]
    fn test_diagnostic_collector_error_with_code() {
        let mut collector = DiagnosticCollector::new();
        let location = SourceInfo::default();
        collector.error_with_code("Q-10-2", "Undefined variable: foo", location);

        assert!(!collector.is_empty());
        assert!(collector.has_errors());
        assert_eq!(collector.diagnostics().len(), 1);
        assert_eq!(collector.diagnostics()[0].code.as_deref(), Some("Q-10-2"));
    }

    #[test]
    fn test_diagnostic_collector_warn_with_code() {
        let mut collector = DiagnosticCollector::new();
        let location = SourceInfo::default();
        collector.warn_with_code("Q-10-2", "Undefined variable: foo", location);

        assert!(!collector.is_empty());
        assert!(!collector.has_errors()); // Warnings don't count as errors
        assert_eq!(collector.diagnostics().len(), 1);
        assert_eq!(collector.diagnostics()[0].code.as_deref(), Some("Q-10-2"));
    }

    #[test]
    fn test_eval_context_warn_or_error_with_code() {
        let vars = TemplateContext::new();
        let location = SourceInfo::default();

        // Normal mode: warning with code
        let mut ctx = EvalContext::new(&vars);
        ctx.warn_or_error_with_code("Q-10-2", "Test", &location);
        assert!(!ctx.has_errors());
        let diagnostics = ctx.into_diagnostics();
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code.as_deref(), Some("Q-10-2"));
        assert_eq!(diagnostics[0].kind, DiagnosticKind::Warning);

        // Strict mode: error with code
        let mut ctx_strict = EvalContext::new(&vars).with_strict_mode(true);
        ctx_strict.warn_or_error_with_code("Q-10-2", "Test", &location);
        assert!(ctx_strict.has_errors());
        let diagnostics = ctx_strict.into_diagnostics();
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code.as_deref(), Some("Q-10-2"));
        assert_eq!(diagnostics[0].kind, DiagnosticKind::Error);
    }
}
