//! Diagnostic extraction from Quarto documents.
//!
//! This module provides functions to extract diagnostics (errors and warnings)
//! from QMD documents by parsing them with `pampa`.

use crate::document::Document;
use crate::types::{Diagnostic, DiagnosticRelatedInformation, DiagnosticSeverity, Position, Range};
use quarto_error_reporting::DiagnosticMessage;
use quarto_source_map::SourceContext;

/// Result of analyzing a document for diagnostics.
#[derive(Debug)]
pub struct DiagnosticResult {
    /// The diagnostics found in the document.
    pub diagnostics: Vec<Diagnostic>,
    /// The source context used for location mapping.
    pub source_context: SourceContext,
}

/// Get diagnostics for a document.
///
/// This parses the document with `pampa` and converts any parse errors
/// and warnings into LSP-compatible diagnostics.
///
/// # Example
///
/// ```rust,ignore
/// use quarto_lsp_core::{Document, get_diagnostics};
///
/// let doc = Document::new("test.qmd", "# Hello\n\nInvalid ```");
/// let result = get_diagnostics(&doc);
/// for diag in &result.diagnostics {
///     println!("{}: {}", diag.severity, diag.message);
/// }
/// ```
pub fn get_diagnostics(doc: &Document) -> DiagnosticResult {
    let source_context = doc.create_source_context();

    // Parse with pampa
    let result = pampa::readers::qmd::read(
        doc.content_bytes(),
        false, // loose mode
        doc.filename(),
        &mut std::io::sink(), // discard verbose output
        true,                 // prune_errors
        None,                 // parent_source_info
    );

    let diagnostics = match result {
        Ok((_pandoc, _ast_context, warnings)) => {
            // Parsing succeeded, convert warnings to diagnostics
            warnings
                .iter()
                .filter_map(|msg| convert_diagnostic(msg, &source_context))
                .collect()
        }
        Err(errors) => {
            // Parsing failed, convert errors to diagnostics
            errors
                .iter()
                .filter_map(|msg| convert_diagnostic(msg, &source_context))
                .collect()
        }
    };

    DiagnosticResult {
        diagnostics,
        source_context,
    }
}

/// Convert a quarto-error-reporting DiagnosticMessage to our Diagnostic type.
fn convert_diagnostic(msg: &DiagnosticMessage, ctx: &SourceContext) -> Option<Diagnostic> {
    // Get the range from the diagnostic location
    let range = if let Some(loc) = &msg.location {
        // Map start position
        let start_mapped = loc.map_offset(0, ctx);
        // Map end position (use length of span, fallback to start if it fails)
        let end_mapped = loc
            .map_offset(loc.length(), ctx)
            .or_else(|| {
                if loc.length() > 0 {
                    loc.map_offset(loc.length().saturating_sub(1), ctx)
                } else {
                    None
                }
            })
            .or_else(|| start_mapped.clone());

        match (start_mapped, end_mapped) {
            (Some(start), Some(end)) => Range::new(
                Position::new(start.location.row as u32, start.location.column as u32),
                Position::new(end.location.row as u32, end.location.column as u32),
            ),
            (Some(start), None) => {
                let pos = Position::new(start.location.row as u32, start.location.column as u32);
                Range::point(pos)
            }
            _ => {
                // No location available, use start of document
                Range::default()
            }
        }
    } else {
        // No location, use start of document
        Range::default()
    };

    // Build the message, including problem statement if available
    let message = if let Some(problem) = &msg.problem {
        format!("{}: {}", msg.title, problem.as_str())
    } else {
        msg.title.clone()
    };

    // Create the diagnostic
    let mut diagnostic = Diagnostic::new(
        range,
        DiagnosticSeverity::from_diagnostic_kind(msg.kind),
        message,
    );

    // Set the error code if available
    if let Some(code) = &msg.code {
        diagnostic = diagnostic.with_code(code.clone());
    }

    // Add related information from details
    for detail in &msg.details {
        if let Some(loc) = &detail.location {
            let start_mapped = loc.map_offset(0, ctx);
            let end_mapped = loc.map_offset(loc.length(), ctx).or(start_mapped.clone());

            if let (Some(start), Some(end)) = (start_mapped, end_mapped) {
                let detail_range = Range::new(
                    Position::new(start.location.row as u32, start.location.column as u32),
                    Position::new(end.location.row as u32, end.location.column as u32),
                );
                diagnostic
                    .related_information
                    .push(DiagnosticRelatedInformation {
                        range: detail_range,
                        message: detail.content.as_str().to_string(),
                    });
            }
        }
    }

    Some(diagnostic)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_valid_document() {
        let doc = Document::new("test.qmd", "# Hello\n\nThis is valid markdown.");
        let result = get_diagnostics(&doc);
        // A valid document should have no errors
        // (may have warnings depending on content)
        assert!(
            result
                .diagnostics
                .iter()
                .all(|d| d.severity != DiagnosticSeverity::Error),
            "Valid document should have no errors"
        );
    }

    #[test]
    fn parse_document_with_yaml_frontmatter() {
        let doc = Document::new(
            "test.qmd",
            r#"---
title: "Test"
author: "Test Author"
---

# Introduction

This is the content.
"#,
        );
        let result = get_diagnostics(&doc);
        // Should parse without errors
        assert!(
            result
                .diagnostics
                .iter()
                .all(|d| d.severity != DiagnosticSeverity::Error),
            "Document with valid YAML should have no errors"
        );
    }

    #[test]
    fn diagnostic_has_source() {
        let doc = Document::new("test.qmd", "# Hello");
        let result = get_diagnostics(&doc);
        // Even empty results should work
        for diag in &result.diagnostics {
            assert_eq!(diag.source, Some("quarto".to_string()));
        }
    }

    #[test]
    fn parse_document_with_invalid_yaml() {
        // This tests YAML frontmatter with syntax errors.
        // The parser should return diagnostics rather than panicking.
        let doc = Document::new(
            "test.qmd",
            r#"---
title: "Test
missing_close: {
  nested: value
---

# Content
"#,
        );

        // This should NOT panic - it should return diagnostics
        let result = get_diagnostics(&doc);

        // Should have at least one error diagnostic for the YAML parse error
        let errors: Vec<_> = result
            .diagnostics
            .iter()
            .filter(|d| d.severity == DiagnosticSeverity::Error)
            .collect();

        assert!(
            !errors.is_empty(),
            "Invalid YAML should produce at least one error diagnostic"
        );

        // The error message should mention YAML parsing
        let has_yaml_error = errors
            .iter()
            .any(|d| d.message.to_lowercase().contains("yaml"));
        assert!(
            has_yaml_error,
            "Error should mention YAML in message, got: {:?}",
            errors.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }
}
