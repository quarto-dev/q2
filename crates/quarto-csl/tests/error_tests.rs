//! Tests for CSL error types.
//!
//! These tests verify that all error variants have correct Display implementations
//! and produce valid DiagnosticMessage output.

use quarto_csl::Error;
use quarto_source_map::{FileId, SourceInfo};

fn make_test_source_info() -> SourceInfo {
    SourceInfo::original(FileId(0), 0, 10)
}

// ============================================================================
// Display implementation tests
// ============================================================================

#[test]
fn test_xml_error_display() {
    let xml_err = quarto_xml::Error::UnexpectedEof {
        expected: "element content".to_string(),
        location: Some(make_test_source_info()),
    };
    let err = Error::XmlError(xml_err);
    let display = err.to_string();
    assert!(display.contains("XML error"), "Got: {}", display);
}

#[test]
fn test_missing_attribute_display() {
    let err = Error::MissingAttribute {
        element: "style".to_string(),
        attribute: "class".to_string(),
        location: make_test_source_info(),
    };
    let display = err.to_string();
    assert!(
        display.contains("Missing required attribute 'class' on <style>"),
        "Got: {}",
        display
    );
}

#[test]
fn test_invalid_attribute_value_display() {
    let err = Error::InvalidAttributeValue {
        element: "style".to_string(),
        attribute: "class".to_string(),
        value: "invalid".to_string(),
        expected: "in-text or note".to_string(),
        location: make_test_source_info(),
    };
    let display = err.to_string();
    assert!(
        display.contains("Invalid value 'invalid' for attribute 'class' on <style>"),
        "Got: {}",
        display
    );
    assert!(
        display.contains("expected in-text or note"),
        "Got: {}",
        display
    );
}

#[test]
fn test_missing_element_display() {
    let err = Error::MissingElement {
        parent: "style".to_string(),
        element: "citation".to_string(),
        location: make_test_source_info(),
    };
    let display = err.to_string();
    assert!(
        display.contains("Missing required element <citation> in <style>"),
        "Got: {}",
        display
    );
}

#[test]
fn test_unexpected_element_display() {
    let err = Error::UnexpectedElement {
        element: "foo".to_string(),
        context: "bibliography".to_string(),
        location: make_test_source_info(),
    };
    let display = err.to_string();
    assert!(
        display.contains("Unexpected element <foo> in bibliography"),
        "Got: {}",
        display
    );
}

#[test]
fn test_missing_text_source_display() {
    let err = Error::MissingTextSource {
        location: make_test_source_info(),
    };
    let display = err.to_string();
    assert!(
        display.contains("Text element must have one of"),
        "Got: {}",
        display
    );
    assert!(
        display.contains("variable, macro, term, or value"),
        "Got: {}",
        display
    );
}

#[test]
fn test_undefined_macro_display() {
    let err = Error::UndefinedMacro {
        name: "author".to_string(),
        reference_location: make_test_source_info(),
        suggestion: None,
    };
    let display = err.to_string();
    assert!(
        display.contains("Undefined macro 'author'"),
        "Got: {}",
        display
    );
}

#[test]
fn test_circular_macro_display() {
    let err = Error::CircularMacro {
        chain: vec![
            "a".to_string(),
            "b".to_string(),
            "c".to_string(),
            "a".to_string(),
        ],
        location: make_test_source_info(),
    };
    let display = err.to_string();
    assert!(
        display.contains("Circular macro dependency: a -> b -> c -> a"),
        "Got: {}",
        display
    );
}

#[test]
fn test_duplicate_macro_display() {
    let err = Error::DuplicateMacro {
        name: "author".to_string(),
        first_location: make_test_source_info(),
        second_location: make_test_source_info(),
    };
    let display = err.to_string();
    assert!(
        display.contains("Duplicate macro definition: 'author'"),
        "Got: {}",
        display
    );
}

#[test]
fn test_invalid_version_display() {
    let err = Error::InvalidVersion {
        version: "2.0".to_string(),
        location: make_test_source_info(),
    };
    let display = err.to_string();
    assert!(
        display.contains("Invalid CSL version: '2.0'"),
        "Got: {}",
        display
    );
}

#[test]
fn test_invalid_root_element_display() {
    let err = Error::InvalidRootElement {
        found: "locale".to_string(),
        location: make_test_source_info(),
    };
    let display = err.to_string();
    assert!(
        display.contains("Expected <style> root element, found <locale>"),
        "Got: {}",
        display
    );
}

// ============================================================================
// to_diagnostic implementation tests
// ============================================================================

#[test]
fn test_xml_error_diagnostic() {
    let xml_err = quarto_xml::Error::UnexpectedEof {
        expected: "element content".to_string(),
        location: Some(make_test_source_info()),
    };
    let err = Error::XmlError(xml_err);
    let diag = err.to_diagnostic();
    // XmlError delegates to quarto_xml, which has its own error codes
    assert!(diag.code.is_some());
}

#[test]
fn test_missing_attribute_diagnostic() {
    let err = Error::MissingAttribute {
        element: "style".to_string(),
        attribute: "class".to_string(),
        location: make_test_source_info(),
    };
    let diag = err.to_diagnostic();
    assert_eq!(diag.code.as_deref(), Some("Q-9-7"));
    assert_eq!(diag.title, "Missing Required Attribute");
    assert!(diag.problem.is_some());
}

#[test]
fn test_invalid_attribute_value_diagnostic() {
    let err = Error::InvalidAttributeValue {
        element: "style".to_string(),
        attribute: "class".to_string(),
        value: "invalid".to_string(),
        expected: "in-text or note".to_string(),
        location: make_test_source_info(),
    };
    let diag = err.to_diagnostic();
    assert_eq!(diag.code.as_deref(), Some("Q-9-8"));
    assert_eq!(diag.title, "Invalid Attribute Value");
}

#[test]
fn test_missing_element_diagnostic() {
    let err = Error::MissingElement {
        parent: "style".to_string(),
        element: "citation".to_string(),
        location: make_test_source_info(),
    };
    let diag = err.to_diagnostic();
    assert_eq!(diag.code.as_deref(), Some("Q-9-9"));
    assert_eq!(diag.title, "Missing Required Element");
}

#[test]
fn test_unexpected_element_diagnostic() {
    let err = Error::UnexpectedElement {
        element: "foo".to_string(),
        context: "bibliography".to_string(),
        location: make_test_source_info(),
    };
    let diag = err.to_diagnostic();
    assert_eq!(diag.code.as_deref(), Some("Q-9-10"));
    assert_eq!(diag.title, "Unexpected Element");
}

#[test]
fn test_missing_text_source_diagnostic() {
    let err = Error::MissingTextSource {
        location: make_test_source_info(),
    };
    let diag = err.to_diagnostic();
    assert_eq!(diag.code.as_deref(), Some("Q-9-11"));
    assert_eq!(diag.title, "Missing Text Source");
    // Should have a hint
    assert!(!diag.hints.is_empty());
}

#[test]
fn test_undefined_macro_diagnostic_without_suggestion() {
    let err = Error::UndefinedMacro {
        name: "author".to_string(),
        reference_location: make_test_source_info(),
        suggestion: None,
    };
    let diag = err.to_diagnostic();
    assert_eq!(diag.code.as_deref(), Some("Q-9-12"));
    assert_eq!(diag.title, "Undefined Macro");
    // No suggestion, so no hint
    assert!(diag.hints.is_empty());
}

#[test]
fn test_undefined_macro_diagnostic_with_suggestion() {
    let err = Error::UndefinedMacro {
        name: "autor".to_string(),
        reference_location: make_test_source_info(),
        suggestion: Some("author".to_string()),
    };
    let diag = err.to_diagnostic();
    assert_eq!(diag.code.as_deref(), Some("Q-9-12"));
    assert_eq!(diag.title, "Undefined Macro");
    // Has suggestion, so should have hint
    assert!(!diag.hints.is_empty());
    assert!(
        diag.hints[0].as_str().contains("Did you mean 'author'?"),
        "Got: {:?}",
        diag.hints
    );
}

#[test]
fn test_circular_macro_diagnostic() {
    let err = Error::CircularMacro {
        chain: vec!["a".to_string(), "b".to_string(), "a".to_string()],
        location: make_test_source_info(),
    };
    let diag = err.to_diagnostic();
    assert_eq!(diag.code.as_deref(), Some("Q-9-13"));
    assert_eq!(diag.title, "Circular Macro Dependency");
}

#[test]
fn test_duplicate_macro_diagnostic() {
    let err = Error::DuplicateMacro {
        name: "author".to_string(),
        first_location: make_test_source_info(),
        second_location: make_test_source_info(),
    };
    let diag = err.to_diagnostic();
    assert_eq!(diag.code.as_deref(), Some("Q-9-14"));
    assert_eq!(diag.title, "Duplicate Macro Definition");
}

#[test]
fn test_invalid_version_diagnostic() {
    let err = Error::InvalidVersion {
        version: "2.0".to_string(),
        location: make_test_source_info(),
    };
    let diag = err.to_diagnostic();
    assert_eq!(diag.code.as_deref(), Some("Q-9-15"));
    assert_eq!(diag.title, "Invalid CSL Version");
}

#[test]
fn test_invalid_root_element_diagnostic() {
    let err = Error::InvalidRootElement {
        found: "locale".to_string(),
        location: make_test_source_info(),
    };
    let diag = err.to_diagnostic();
    assert_eq!(diag.code.as_deref(), Some("Q-9-16"));
    assert_eq!(diag.title, "Invalid Root Element");
}

// ============================================================================
// From implementation tests
// ============================================================================

#[test]
fn test_from_xml_error() {
    let xml_err = quarto_xml::Error::UnexpectedEof {
        expected: "content".to_string(),
        location: Some(make_test_source_info()),
    };
    let err: Error = xml_err.into();
    assert!(matches!(err, Error::XmlError(_)));
}

// ============================================================================
// Error trait implementation test
// ============================================================================

#[test]
fn test_error_trait() {
    // Verify Error implements std::error::Error
    let err: Box<dyn std::error::Error> = Box::new(Error::MissingAttribute {
        element: "test".to_string(),
        attribute: "attr".to_string(),
        location: make_test_source_info(),
    });
    // Should be able to call std::error::Error methods
    let _ = err.to_string();
}
