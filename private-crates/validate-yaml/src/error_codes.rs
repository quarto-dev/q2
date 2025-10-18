//! Error code mapping and hint suggestions for YAML validation errors.

use quarto_yaml_validation::ValidationError;

/// Infer an error code from a validation error.
///
/// Maps validation error patterns to Q-1-xxx error codes (YAML and Configuration subsystem).
pub fn infer_error_code(error: &ValidationError) -> &'static str {
    let msg = &error.message;

    // Missing required property
    if msg.contains("Missing required property") || msg.contains("Missing required field") {
        return "Q-1-10";
    }

    // Type mismatches
    if msg.contains("Expected") && msg.contains("got") {
        return "Q-1-11";
    }

    // Enum validation
    if msg.contains("must be one of") || msg.contains("Value must be one of") {
        return "Q-1-12";
    }

    // Array length constraints
    if msg.contains("Array length") && (msg.contains("less than minimum") || msg.contains("greater than maximum")) {
        return "Q-1-13";
    }

    // String pattern mismatch
    if msg.contains("does not match pattern") {
        return "Q-1-14";
    }

    // Number range violations
    if msg.contains("less than minimum") || msg.contains("greater than maximum") || msg.contains("not a multiple of") {
        return "Q-1-15";
    }

    // Object property count
    if msg.contains("Object has") && msg.contains("properties") {
        return "Q-1-16";
    }

    // Unresolved references
    if msg.contains("Unresolved schema reference") {
        return "Q-1-17";
    }

    // Unknown property in closed object
    if msg.contains("Unknown property") {
        return "Q-1-18";
    }

    // Array uniqueness violation
    if msg.contains("must be unique") {
        return "Q-1-19";
    }

    // Generic validation error
    "Q-1-99"
}

/// Suggest a fix for a validation error based on its type.
///
/// Returns a helpful hint (ending with ?) or None if no specific hint applies.
pub fn suggest_fix(error: &ValidationError) -> Option<String> {
    let msg = &error.message;

    // Missing required property
    if msg.contains("Missing required property") {
        // Extract property name if possible
        if let Some(start) = msg.find('\'') {
            if let Some(end) = msg[start + 1..].find('\'') {
                let prop = &msg[start + 1..start + 1 + end];
                return Some(format!("Add the `{}` property to your YAML document?", prop));
            }
        }
        return Some("Add the required property to your YAML document?".to_string());
    }

    // Type mismatches - boolean
    if msg.contains("Expected boolean") {
        return Some("Use `true` or `false` (YAML 1.2 standard)?".to_string());
    }

    // Type mismatches - number
    if msg.contains("Expected number") {
        return Some("Use a numeric value without quotes?".to_string());
    }

    // Type mismatches - string
    if msg.contains("Expected string") {
        return Some("Ensure the value is a string (quoted if it contains special characters)?".to_string());
    }

    // Type mismatches - array
    if msg.contains("Expected array") {
        return Some("Use YAML array syntax: `[item1, item2]` or list format?".to_string());
    }

    // Type mismatches - object
    if msg.contains("Expected object") {
        return Some("Use YAML mapping syntax with key-value pairs?".to_string());
    }

    // Enum validation
    if msg.contains("must be one of") {
        return Some("Check the schema for allowed values?".to_string());
    }

    // String pattern
    if msg.contains("does not match pattern") {
        return Some("Check that the string matches the expected format?".to_string());
    }

    // Number range
    if msg.contains("less than minimum") || msg.contains("greater than maximum") {
        return Some("Check the allowed value range in the schema?".to_string());
    }

    // Unknown property
    if msg.contains("Unknown property") {
        return Some("Check for typos in property names or remove unrecognized properties?".to_string());
    }

    // Array uniqueness
    if msg.contains("must be unique") {
        return Some("Remove duplicate items from the array?".to_string());
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_yaml_validation::error::InstancePath;

    fn make_error(message: &str) -> ValidationError {
        ValidationError::new(message, InstancePath::new())
    }

    #[test]
    fn test_infer_missing_required() {
        let error = make_error("Missing required property 'author'");
        assert_eq!(infer_error_code(&error), "Q-1-10");
    }

    #[test]
    fn test_infer_type_mismatch() {
        let error = make_error("Expected boolean, got string");
        assert_eq!(infer_error_code(&error), "Q-1-11");
    }

    #[test]
    fn test_infer_enum() {
        let error = make_error("Value must be one of: html, pdf");
        assert_eq!(infer_error_code(&error), "Q-1-12");
    }

    #[test]
    fn test_infer_array_length() {
        let error = make_error("Array length 2 is less than minimum 3");
        assert_eq!(infer_error_code(&error), "Q-1-13");
    }

    #[test]
    fn test_infer_pattern() {
        let error = make_error("String 'foo' does not match pattern '[0-9]+'");
        assert_eq!(infer_error_code(&error), "Q-1-14");
    }

    #[test]
    fn test_infer_generic() {
        let error = make_error("Something went wrong");
        assert_eq!(infer_error_code(&error), "Q-1-99");
    }

    #[test]
    fn test_suggest_missing_required() {
        let error = make_error("Missing required property 'author'");
        let hint = suggest_fix(&error);
        assert!(hint.is_some());
        assert!(hint.unwrap().contains("author"));
    }

    #[test]
    fn test_suggest_boolean() {
        let error = make_error("Expected boolean, got string");
        let hint = suggest_fix(&error);
        assert!(hint.is_some());
        assert!(hint.unwrap().contains("true"));
    }

    #[test]
    fn test_suggest_none() {
        let error = make_error("Something went wrong");
        let hint = suggest_fix(&error);
        assert!(hint.is_none());
    }
}
