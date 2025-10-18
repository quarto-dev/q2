//! Conversion of ValidationError to DiagnosticMessage.

use quarto_error_reporting::{DiagnosticMessage, DiagnosticMessageBuilder};
use quarto_yaml_validation::ValidationError;

use crate::error_codes::{infer_error_code, suggest_fix};

/// Convert a ValidationError to a DiagnosticMessage.
///
/// This creates a structured, tidyverse-style error message with:
/// - Error code (Q-1-xxx)
/// - Problem statement
/// - Details about instance path, schema path, and location
/// - Hints for fixing the error
pub fn validation_error_to_diagnostic(error: &ValidationError) -> DiagnosticMessage {
    let mut builder = DiagnosticMessageBuilder::error("YAML Validation Failed")
        .with_code(infer_error_code(error))
        .problem(error.message.clone());

    // Add instance path as detail (where in the document the error occurred)
    if !error.instance_path.is_empty() {
        builder = builder.add_detail(format!(
            "At document path: `{}`",
            error.instance_path
        ));
    } else {
        builder = builder.add_detail("At document root");
    }

    // Add schema path as info (which schema constraint failed)
    if !error.schema_path.is_empty() {
        builder = builder.add_info(format!(
            "Schema constraint: {}",
            error.schema_path
        ));
    }

    // Add location as detail (file, line, column)
    if let Some(loc) = &error.location {
        builder = builder.add_detail(format!(
            "In file `{}` at line {}, column {}",
            loc.file, loc.line, loc.column
        ));
    }

    // Add hints based on error type
    if let Some(hint) = suggest_fix(error) {
        builder = builder.add_hint(hint);
    }

    builder.build()
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_yaml_validation::error::InstancePath;

    #[test]
    fn test_conversion_basic() {
        let error = ValidationError::new("Missing required property 'author'", InstancePath::new());
        let diagnostic = validation_error_to_diagnostic(&error);

        assert_eq!(diagnostic.title, "YAML Validation Failed");
        assert_eq!(diagnostic.code, Some("Q-1-10".to_string()));
        assert!(diagnostic.problem.is_some());
        assert!(!diagnostic.details.is_empty());
    }

    #[test]
    fn test_conversion_with_path() {
        let mut path = InstancePath::new();
        path.push_key("format");
        path.push_key("html");

        let error = ValidationError::new("Expected boolean, got string", path);
        let diagnostic = validation_error_to_diagnostic(&error);

        assert_eq!(diagnostic.code, Some("Q-1-11".to_string()));
        assert!(!diagnostic.hints.is_empty());
    }
}
