use quarto_markdown_pandoc::readers;
use quarto_markdown_pandoc::utils;

#[test]
fn test_json_error_format() {
    // Create input with a malformed code block to trigger an error
    let input = "```{python\n";

    // Test with new API
    let result = readers::qmd::read(input.as_bytes(), false, "test.md", &mut std::io::sink());

    assert!(result.is_err());
    let diagnostics = result.unwrap_err();
    assert!(diagnostics.len() > 0, "Should have at least one diagnostic");

    // Verify the first diagnostic can be serialized to JSON
    let json_value = diagnostics[0].to_json();

    // Verify the structure - DiagnosticMessage has a different structure than the old format
    assert!(json_value.get("kind").is_some());
    assert!(json_value.get("title").is_some());
}

#[test]
fn test_regular_error_format() {
    // Create input with a malformed code block to trigger an error
    let input = "```{python\n";

    // Test with new API
    let result = readers::qmd::read(input.as_bytes(), false, "test.md", &mut std::io::sink());

    assert!(result.is_err());
    let diagnostics = result.unwrap_err();

    // Diagnostics can be formatted as text
    for diag in &diagnostics {
        let text = diag.to_text(None);
        // Verify it's a non-empty formatted error message
        assert!(!text.is_empty());
    }
}

#[test]
fn test_newline_warning() {
    // Test file without trailing newline
    let input = "# Hello World";

    let result = readers::qmd::read(input.as_bytes(), false, "test.md", &mut std::io::sink());

    // Should succeed (the newline is added automatically)
    assert!(result.is_ok());

    // The newline warning is currently emitted in main.rs, not in the library
    // This test just verifies that the parse succeeds
}
