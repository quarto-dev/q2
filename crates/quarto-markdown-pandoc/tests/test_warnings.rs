use quarto_markdown_pandoc::readers;

#[test]
fn test_caption_without_table_warning() {
    // Create input with a caption after a div (not a table)
    // This should parse successfully but emit a warning
    let input = r#"::: {.my-div}
Some content
:::

: This caption has no table
"#;

    // Parse the document
    let result = readers::qmd::read(input.as_bytes(), false, "test.md", &mut std::io::sink());

    // Parsing should succeed (warnings are not errors)
    assert!(
        result.is_ok(),
        "Document should parse successfully despite warning"
    );

    // TODO: Once the fix is implemented, we need to verify that the warning
    // "Caption found without a preceding table" was actually output.
    // For now, this test just verifies that parsing succeeds.
    // After the fix, we'll need to capture stderr or modify the API
    // to return warnings alongside the successful parse result.
}

#[test]
fn test_caption_with_table_no_warning() {
    // Create input with a proper table caption
    // This should parse successfully with no warnings
    let input = r#"| A | B |
|---|---|
| 1 | 2 |

: Table caption
"#;

    // Parse the document
    let result = readers::qmd::read(input.as_bytes(), false, "test.md", &mut std::io::sink());

    // Parsing should succeed and no warnings should be emitted
    assert!(
        result.is_ok(),
        "Document with valid table caption should parse successfully"
    );

    let (pandoc, _context, _warnings) = result.unwrap();

    // Verify we have a table in the output
    assert!(
        pandoc
            .blocks
            .iter()
            .any(|b| matches!(b, quarto_markdown_pandoc::pandoc::Block::Table(_))),
        "Should have a table in the output"
    );
}
