use quarto_markdown_pandoc::readers;
use quarto_markdown_pandoc::utils;

#[test]
fn test_json_error_format() {
    // Create input with a malformed code block to trigger an error
    let input = "```{python\n";

    // Test with JSON errors enabled using the formatter closure
    let json_formatter = readers::qmd_error_messages::produce_json_error_messages;
    let result = readers::qmd::read(
        input.as_bytes(),
        false,
        "test.md",
        &mut std::io::sink(),
        Some(json_formatter),
    );

    assert!(result.is_err());
    let error_messages = result.unwrap_err();
    assert_eq!(error_messages.len(), 1);
    
    // Verify the error is valid JSON
    let json_str = &error_messages[0];
    let parsed: serde_json::Value = serde_json::from_str(json_str).expect("Should be valid JSON");
    
    // Verify it's an array
    assert!(parsed.is_array());
    let errors = parsed.as_array().unwrap();
    assert!(errors.len() > 0);
    
    // Verify the structure of the first error
    let first_error = &errors[0];
    assert!(first_error.get("filename").is_some());
    assert!(first_error.get("title").is_some());
    assert!(first_error.get("message").is_some());
    assert!(first_error.get("location").is_some());
    
    let location = first_error.get("location").unwrap();
    assert!(location.get("row").is_some());
    assert!(location.get("column").is_some());
    assert!(location.get("byte_offset").is_some());
    assert!(location.get("size").is_some());
}

#[test]
fn test_regular_error_format() {
    // Create input with a malformed code block to trigger an error
    let input = "```{python\n";

    // Test with JSON errors disabled (None for formatter)
    let result = readers::qmd::read(
        input.as_bytes(),
        false,
        "test.md",
        &mut std::io::sink(),
        None::<fn(&[u8], &utils::tree_sitter_log_observer::TreeSitterLogObserver, &str) -> Vec<String>>,
    );

    assert!(result.is_err());
    let error_messages = result.unwrap_err();
    
    // Regular errors should be plain strings, not JSON
    for msg in &error_messages {
        // Verify it's NOT valid JSON (should be a formatted error message)
        if msg.starts_with("[") || msg.starts_with("{") {
            let parse_result: Result<serde_json::Value, _> = serde_json::from_str(msg);
            assert!(parse_result.is_err(), "Regular error messages should not be JSON");
        }
    }
}