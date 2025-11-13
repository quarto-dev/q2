/*
 * test_unicode_error_offsets.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Test that error diagnostics correctly handle multi-byte UTF-8 characters
 * when calculating error positions.
 */

use quarto_markdown_pandoc::readers;

#[test]
fn test_unicode_error_position() {
    // Test case: Multi-byte UTF-8 character (✓ = 3 bytes) before parse error
    // The '}' character is invalid in this context and should trigger an error
    // Error should be reported at the correct visual column position
    let input = "✓\n[}no]{.hello}";
    let input_bytes = input.as_bytes();

    let mut output = Vec::new();
    let result = readers::qmd::read(input_bytes, false, "test.qmd", &mut output, true);

    // Should have errors (the '}' is invalid)
    assert!(
        result.is_err(),
        "Expected parse errors for invalid '}}' character"
    );

    let diagnostics = result.unwrap_err();
    assert!(
        !diagnostics.is_empty(),
        "Should have at least one diagnostic"
    );

    // Create source context and register the file so ariadne can display it
    let mut source_context = quarto_source_map::SourceContext::new();
    source_context.add_file("test.qmd".to_string(), Some(input.to_string()));
    let error_text = diagnostics[0].to_text(Some(&source_context));

    // The error should reference line 2, column 2 (not column 4)
    // Line 2 is the second line: "[}no]{.hello}"
    // Column 2 is the '}' character (after the '[' at column 1)
    //
    // If the bug exists, it will show column 4 because:
    // - ✓ is 3 bytes but 1 character
    // - We pass byte offset 5 (4 bytes for "✓\n[" = 3+1+1)
    // - Ariadne in char mode treats this as char offset 5
    // - Which is: ✓(0), \n(1), [(2), }(3), n(4), o(5) <- points here instead

    println!("Error text:\n{}", error_text);

    // The error should mention line 2, column 2
    // In the current buggy version, it would show 2:4
    assert!(
        error_text.contains("2:2")
            || error_text.contains("line 2") && error_text.contains("column 2"),
        "Error should be at line 2, column 2, but got:\n{}",
        error_text
    );

    // The error should NOT mention column 4
    assert!(
        !error_text.contains("2:4"),
        "Error incorrectly reported at column 4 instead of column 2:\n{}",
        error_text
    );
}

#[test]
fn test_ascii_error_position_baseline() {
    // Baseline test: ASCII character before parse error
    // This should work correctly even before the fix
    let input = "x\n[}no]{.hello}";
    let input_bytes = input.as_bytes();

    let mut output = Vec::new();
    let result = readers::qmd::read(input_bytes, false, "test.qmd", &mut output, true);

    assert!(
        result.is_err(),
        "Expected parse errors for invalid '}}' character"
    );

    let diagnostics = result.unwrap_err();
    // Create source context and register the file so ariadne can display it
    let mut source_context = quarto_source_map::SourceContext::new();
    source_context.add_file("test.qmd".to_string(), Some(input.to_string()));
    let error_text = diagnostics[0].to_text(Some(&source_context));

    println!("ASCII baseline error text:\n{}", error_text);

    // Should correctly show line 2, column 2
    assert!(
        error_text.contains("2:2")
            || error_text.contains("line 2") && error_text.contains("column 2"),
        "ASCII baseline: Error should be at line 2, column 2, but got:\n{}",
        error_text
    );
}
