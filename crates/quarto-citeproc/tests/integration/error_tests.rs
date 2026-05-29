//! Tests for citeproc error types.
//!
//! These tests verify that all error variants have correct Display implementations
//! and produce valid DiagnosticMessage output.

use quarto_citeproc::Error;
use quarto_source_map::{FileId, SourceInfo};

fn make_test_source_info() -> SourceInfo {
    SourceInfo::original(FileId(0), 0, 10)
}

// ============================================================================
// Display implementation tests
// ============================================================================

#[test]
fn test_csl_error_display() {
    let csl_err = quarto_csl::Error::InvalidVersion {
        version: "invalid".to_string(),
        location: make_test_source_info(),
    };
    let err = Error::CslError(csl_err);
    let display = err.to_string();
    assert!(display.contains("CSL error"), "Got: {}", display);
}

#[test]
fn test_locale_parse_error_display() {
    let err = Error::LocaleParseError {
        locale: "en-US".to_string(),
        message: "Invalid XML".to_string(),
    };
    let display = err.to_string();
    assert!(
        display.contains("Failed to parse locale 'en-US'"),
        "Got: {}",
        display
    );
    assert!(display.contains("Invalid XML"), "Got: {}", display);
}

#[test]
fn test_reference_not_found_display() {
    let err = Error::ReferenceNotFound {
        id: "smith2020".to_string(),
        location: None,
    };
    let display = err.to_string();
    assert!(
        display.contains("Reference 'smith2020' not found"),
        "Got: {}",
        display
    );
}

#[test]
fn test_reference_not_found_with_location_display() {
    let err = Error::ReferenceNotFound {
        id: "smith2020".to_string(),
        location: Some(make_test_source_info()),
    };
    let display = err.to_string();
    assert!(
        display.contains("Reference 'smith2020' not found"),
        "Got: {}",
        display
    );
}

#[test]
fn test_invalid_reference_display() {
    let err = Error::InvalidReference {
        id: "smith2020".to_string(),
        field: "author".to_string(),
        message: "expected array of names".to_string(),
    };
    let display = err.to_string();
    assert!(
        display.contains("Invalid reference 'smith2020' field 'author'"),
        "Got: {}",
        display
    );
    assert!(
        display.contains("expected array of names"),
        "Got: {}",
        display
    );
}

#[test]
fn test_missing_required_field_display() {
    let err = Error::MissingRequiredField {
        id: "smith2020".to_string(),
        field: "title".to_string(),
    };
    let display = err.to_string();
    assert!(
        display.contains("Reference 'smith2020' missing required field 'title'"),
        "Got: {}",
        display
    );
}

#[test]
fn test_invalid_date_display() {
    let err = Error::InvalidDate {
        id: "smith2020".to_string(),
        field: "issued".to_string(),
        value: "not-a-date".to_string(),
    };
    let display = err.to_string();
    assert!(
        display.contains("Invalid date 'not-a-date' in reference 'smith2020' field 'issued'"),
        "Got: {}",
        display
    );
}

#[test]
fn test_evaluation_error_display() {
    let err = Error::EvaluationError {
        message: "Failed to format date".to_string(),
        location: None,
    };
    let display = err.to_string();
    assert!(
        display.contains("Evaluation error: Failed to format date"),
        "Got: {}",
        display
    );
}

#[test]
fn test_evaluation_error_with_location_display() {
    let err = Error::EvaluationError {
        message: "Failed to format date".to_string(),
        location: Some(make_test_source_info()),
    };
    let display = err.to_string();
    assert!(
        display.contains("Evaluation error: Failed to format date"),
        "Got: {}",
        display
    );
}

// ============================================================================
// to_diagnostic implementation tests
// ============================================================================

#[test]
fn test_csl_error_diagnostic() {
    let csl_err = quarto_csl::Error::InvalidVersion {
        version: "invalid".to_string(),
        location: make_test_source_info(),
    };
    let err = Error::CslError(csl_err);
    let diag = err.to_diagnostic();
    // CslError delegates to quarto_csl, which has its own error codes
    assert!(diag.code.is_some());
}

#[test]
fn test_locale_parse_error_diagnostic() {
    let err = Error::LocaleParseError {
        locale: "en-US".to_string(),
        message: "Invalid XML".to_string(),
    };
    let diag = err.to_diagnostic();
    assert_eq!(diag.code.as_deref(), Some("Q-10-1"));
    assert_eq!(diag.title, "Locale Parse Error");
}

#[test]
fn test_reference_not_found_diagnostic_without_location() {
    let err = Error::ReferenceNotFound {
        id: "smith2020".to_string(),
        location: None,
    };
    let diag = err.to_diagnostic();
    assert_eq!(diag.code.as_deref(), Some("Q-10-2"));
    assert_eq!(diag.title, "Reference Not Found");
    assert!(diag.location.is_none());
}

#[test]
fn test_reference_not_found_diagnostic_with_location() {
    let err = Error::ReferenceNotFound {
        id: "smith2020".to_string(),
        location: Some(make_test_source_info()),
    };
    let diag = err.to_diagnostic();
    assert_eq!(diag.code.as_deref(), Some("Q-10-2"));
    assert_eq!(diag.title, "Reference Not Found");
    assert!(diag.location.is_some());
}

#[test]
fn test_invalid_reference_diagnostic() {
    let err = Error::InvalidReference {
        id: "smith2020".to_string(),
        field: "author".to_string(),
        message: "expected array of names".to_string(),
    };
    let diag = err.to_diagnostic();
    assert_eq!(diag.code.as_deref(), Some("Q-10-3"));
    assert_eq!(diag.title, "Invalid Reference");
}

#[test]
fn test_missing_required_field_diagnostic() {
    let err = Error::MissingRequiredField {
        id: "smith2020".to_string(),
        field: "title".to_string(),
    };
    let diag = err.to_diagnostic();
    assert_eq!(diag.code.as_deref(), Some("Q-10-4"));
    assert_eq!(diag.title, "Missing Required Field");
}

#[test]
fn test_invalid_date_diagnostic() {
    let err = Error::InvalidDate {
        id: "smith2020".to_string(),
        field: "issued".to_string(),
        value: "not-a-date".to_string(),
    };
    let diag = err.to_diagnostic();
    assert_eq!(diag.code.as_deref(), Some("Q-10-5"));
    assert_eq!(diag.title, "Invalid Date");
    // Should have a hint about date format
    assert!(!diag.hints.is_empty());
}

#[test]
fn test_evaluation_error_diagnostic_without_location() {
    let err = Error::EvaluationError {
        message: "Failed to format date".to_string(),
        location: None,
    };
    let diag = err.to_diagnostic();
    assert_eq!(diag.code.as_deref(), Some("Q-10-6"));
    assert_eq!(diag.title, "Evaluation Error");
    assert!(diag.location.is_none());
}

#[test]
fn test_evaluation_error_diagnostic_with_location() {
    let err = Error::EvaluationError {
        message: "Failed to format date".to_string(),
        location: Some(make_test_source_info()),
    };
    let diag = err.to_diagnostic();
    assert_eq!(diag.code.as_deref(), Some("Q-10-6"));
    assert_eq!(diag.title, "Evaluation Error");
    assert!(diag.location.is_some());
}

// ============================================================================
// From implementation tests
// ============================================================================

#[test]
fn test_from_csl_error() {
    let csl_err = quarto_csl::Error::InvalidVersion {
        version: "invalid".to_string(),
        location: make_test_source_info(),
    };
    let err: Error = csl_err.into();
    assert!(matches!(err, Error::CslError(_)));
}

// ============================================================================
// Error trait implementation test
// ============================================================================

#[test]
fn test_error_trait() {
    // Verify Error implements std::error::Error
    let err: Box<dyn std::error::Error> = Box::new(Error::ReferenceNotFound {
        id: "test".to_string(),
        location: None,
    });
    // Should be able to call std::error::Error methods
    let _ = err.to_string();
}
