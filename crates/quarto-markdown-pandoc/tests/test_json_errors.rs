use quarto_markdown_pandoc::readers;
use std::fs;
use std::process::Command;

#[test]
fn test_json_error_format() {
    // Create input with a malformed code block to trigger an error
    let input = "```{python\n";

    // Test with new API
    let result = readers::qmd::read(
        input.as_bytes(),
        false,
        "test.md",
        &mut std::io::sink(),
        true,
    );

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
    let result = readers::qmd::read(
        input.as_bytes(),
        false,
        "test.md",
        &mut std::io::sink(),
        true,
    );

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

    let result = readers::qmd::read(
        input.as_bytes(),
        false,
        "test.md",
        &mut std::io::sink(),
        true,
    );

    // Should succeed (the newline is added automatically)
    assert!(result.is_ok());

    // The newline warning is currently emitted in main.rs, not in the library
    // This test just verifies that the parse succeeds
}

#[test]
fn test_json_errors_flag_with_warning() {
    // Create a temporary file with invalid markdown in metadata to trigger Q-1-101 warning
    let temp_file = "/tmp/test_json_errors_warning.qmd";
    let input = r#"---
title: "Test Document"
description: "[incomplete link"
---

# Test
"#;

    fs::write(temp_file, input).expect("Failed to write temp file");

    // Run the binary with --json-errors flag
    let output = Command::new("cargo")
        .args(&[
            "run",
            "-p",
            "quarto-markdown-pandoc",
            "--",
            "-i",
            temp_file,
            "--json-errors",
        ])
        .output()
        .expect("Failed to execute command");

    // stderr should contain JSON warning
    let stderr = String::from_utf8_lossy(&output.stderr);

    // Parse the JSON from stderr
    let json_lines: Vec<&str> = stderr
        .lines()
        .filter(|line| line.starts_with("{"))
        .collect();

    assert!(!json_lines.is_empty(), "Expected JSON output on stderr");

    // Parse the first JSON line
    let json_value: serde_json::Value =
        serde_json::from_str(json_lines[0]).expect("Failed to parse JSON from stderr");

    // Verify the JSON structure
    assert_eq!(json_value["kind"], "warning", "Expected warning kind");
    assert_eq!(json_value["code"], "Q-1-101", "Expected Q-1-101 code");
    assert!(json_value["title"].is_string(), "Expected title field");
    assert!(json_value["problem"].is_object(), "Expected problem field");

    // Clean up
    let _ = fs::remove_file(temp_file);
}

#[test]
fn test_json_errors_flag_with_error() {
    // Create a temporary file with a parse error
    let temp_file = "/tmp/test_json_errors_error.qmd";
    let input = "```{python\n"; // Unclosed code fence

    fs::write(temp_file, input).expect("Failed to write temp file");

    // Run the binary with --json-errors flag
    let output = Command::new("cargo")
        .args(&[
            "run",
            "-p",
            "quarto-markdown-pandoc",
            "--",
            "-i",
            temp_file,
            "--json-errors",
        ])
        .output()
        .expect("Failed to execute command");

    // Should have non-zero exit code for errors
    assert!(!output.status.success(), "Expected command to fail");

    // stdout should contain JSON error(s)
    let stdout = String::from_utf8_lossy(&output.stdout);

    // Parse the JSON from stdout
    let json_lines: Vec<&str> = stdout
        .lines()
        .filter(|line| line.starts_with("{"))
        .collect();

    assert!(
        !json_lines.is_empty(),
        "Expected JSON output on stdout for errors"
    );

    // Parse the first JSON line
    let json_value: serde_json::Value =
        serde_json::from_str(json_lines[0]).expect("Failed to parse JSON from stdout");

    // Verify the JSON structure
    assert_eq!(json_value["kind"], "error", "Expected error kind");
    assert!(json_value["title"].is_string(), "Expected title field");

    // Clean up
    let _ = fs::remove_file(temp_file);
}
