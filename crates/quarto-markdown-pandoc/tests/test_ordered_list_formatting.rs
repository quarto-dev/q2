/*
 * test_ordered_list_formatting.rs
 * Copyright (c) 2025 Posit, PBC
 */

use quarto_markdown_pandoc::{readers, writers};

#[test]
fn test_ordered_list_10plus_formatting() {
    // Test that ordered lists with 10+ items have correct spacing
    let input = r#"1. First item
2. Second item
3. hi
4. hi
5. hi
6. hi
7. hi
8. hi
9. hi
10. Tenth item
11. Eleventh item"#;

    // Parse the markdown
    let (doc, _context, _warnings) = readers::qmd::read(
        input.as_bytes(),
        false,
        "<test>",
        &mut std::io::sink(),
        true,
    )
    .unwrap();

    // Write it back out
    let mut buf = Vec::new();
    writers::qmd::write(&doc, &mut buf).unwrap();
    let output = String::from_utf8(buf).unwrap();

    // Check that items 1-9 have two spaces after the period
    // and items 10+ have one space
    let lines: Vec<&str> = output.lines().collect();

    // Find the lines with list items
    for line in &lines {
        if line.starts_with("1. ") || line.starts_with("2. ") {
            // Should have two spaces after period for single digits
            assert!(
                line.starts_with("1.  ") || line.starts_with("2.  "),
                "Single digit items should have two spaces after period: '{}'",
                line
            );
        } else if line.starts_with("9. ") {
            assert!(
                line.starts_with("9.  "),
                "Item 9 should have two spaces after period: '{}'",
                line
            );
        } else if line.starts_with("10.") || line.starts_with("11.") {
            // Should have one space after period for double digits
            assert!(
                line.starts_with("10. ") || line.starts_with("11. "),
                "Double digit items should have one space after period: '{}'",
                line
            );
            // Make sure it's not two spaces
            assert!(
                !line.starts_with("10.  ") && !line.starts_with("11.  "),
                "Double digit items should not have two spaces: '{}'",
                line
            );
        }
    }
}

#[test]
fn test_ordered_list_continuation_indentation() {
    // Test that continuation lines in ordered lists use 4 spaces
    let input = r#"1. First item
   with continuation

10. Tenth item
    with continuation"#;

    // Parse the markdown
    let (doc, _context, _warnings) = readers::qmd::read(
        input.as_bytes(),
        false,
        "<test>",
        &mut std::io::sink(),
        true,
    )
    .unwrap();

    // Write it back out
    let mut buf = Vec::new();
    writers::qmd::write(&doc, &mut buf).unwrap();
    let output = String::from_utf8(buf).unwrap();

    // Check that all continuation lines use 4 spaces for indentation
    let lines: Vec<&str> = output.lines().collect();
    for (i, line) in lines.iter().enumerate() {
        if i > 0 && !line.is_empty() && !line.starts_with(char::is_numeric) {
            // This is a continuation line
            if line.starts_with("    ") {
                // Good - has 4-space indent
            } else if line.trim_start() != *line {
                // Has some indent but not 4 spaces - this would be wrong
                panic!(
                    "Continuation line should have 4-space indent, found: '{}'",
                    line
                );
            }
        }
    }
}
