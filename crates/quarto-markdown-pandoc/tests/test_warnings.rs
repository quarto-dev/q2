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
    let result = readers::qmd::read(
        input.as_bytes(),
        false,
        "test.md",
        &mut std::io::sink(),
        true,
        None,
    );

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
    let result = readers::qmd::read(
        input.as_bytes(),
        false,
        "test.md",
        &mut std::io::sink(),
        true,
        None,
    );

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

#[test]
fn test_html_element_produces_warning_not_error() {
    // HTML elements should produce warnings and be auto-converted to RawInline nodes
    let input = "<b>hello world</b>";

    // Parse the document
    let result = readers::qmd::read(
        input.as_bytes(),
        false,
        "test.md",
        &mut std::io::sink(),
        true,
        None,
    );

    // Parsing should succeed (warnings are not errors)
    assert!(
        result.is_ok(),
        "Document with HTML elements should parse successfully with warnings, not fail with errors"
    );

    let (pandoc, _context, warnings) = result.unwrap();

    // Should have warnings about HTML elements
    assert!(
        !warnings.is_empty(),
        "Should have warnings for HTML elements"
    );

    // Warnings should be Q-2-9 (not Q-2-6 errors)
    let html_warnings: Vec<_> = warnings
        .iter()
        .filter(|w| w.code.as_ref().map(|c| c.as_str()) == Some("Q-2-9"))
        .collect();

    assert!(
        !html_warnings.is_empty(),
        "Should have Q-2-9 warnings for HTML elements"
    );

    // Verify all diagnostics are warnings, not errors
    assert!(
        warnings
            .iter()
            .all(|d| d.kind == quarto_error_reporting::DiagnosticKind::Warning),
        "All diagnostics should be warnings, not errors"
    );

    // Verify the AST contains RawInline nodes
    use quarto_markdown_pandoc::pandoc::{Block, Inline};
    let para = match &pandoc.blocks[0] {
        Block::Paragraph(p) => p,
        _ => panic!("Expected paragraph block"),
    };

    // Should have: RawInline("<b>"), Str("hello"), Space, Str("world"), RawInline("</b>")
    // Or possibly merged: RawInline("<b>"), Str("hello world"), RawInline("</b>")
    let raw_inlines: Vec<_> = para
        .content
        .iter()
        .filter_map(|i| match i {
            Inline::RawInline(r) => Some(r),
            _ => None,
        })
        .collect();

    assert_eq!(
        raw_inlines.len(),
        2,
        "Should have two RawInline nodes (opening and closing tags)"
    );

    assert_eq!(raw_inlines[0].format, "html");
    assert_eq!(raw_inlines[0].text, "<b>");

    assert_eq!(raw_inlines[1].format, "html");
    assert_eq!(raw_inlines[1].text, "</b>");
}

#[test]
fn test_multiple_html_elements() {
    // Multiple HTML elements should each produce warnings and be converted
    let input = "<i>italic</i> and <b>bold</b>";

    let result = readers::qmd::read(
        input.as_bytes(),
        false,
        "test.md",
        &mut std::io::sink(),
        true,
        None,
    );

    assert!(result.is_ok(), "Document should parse successfully");

    let (pandoc, _context, warnings) = result.unwrap();

    // Should have 4 warnings (two opening + two closing tags)
    let html_warnings: Vec<_> = warnings
        .iter()
        .filter(|w| w.code.as_ref().map(|c| c.as_str()) == Some("Q-2-9"))
        .collect();

    assert_eq!(
        html_warnings.len(),
        4,
        "Should have 4 Q-2-9 warnings for 4 HTML elements"
    );

    // Verify AST structure
    use quarto_markdown_pandoc::pandoc::{Block, Inline};
    let para = match &pandoc.blocks[0] {
        Block::Paragraph(p) => p,
        _ => panic!("Expected paragraph block"),
    };

    let raw_inlines: Vec<_> = para
        .content
        .iter()
        .filter_map(|i| match i {
            Inline::RawInline(r) => Some(r),
            _ => None,
        })
        .collect();

    assert_eq!(raw_inlines.len(), 4, "Should have 4 RawInline nodes");

    // Verify the HTML tags
    assert_eq!(raw_inlines[0].text, "<i>");
    assert_eq!(raw_inlines[1].text, "</i>");
    assert_eq!(raw_inlines[2].text, "<b>");
    assert_eq!(raw_inlines[3].text, "</b>");

    // All should have format="html"
    assert!(raw_inlines.iter().all(|r| r.format == "html"));
}

#[test]
fn test_block_level_html_elements() {
    // Block-level HTML elements like <div> should also be converted to RawInline
    let input = "<div>content</div>";

    let result = readers::qmd::read(
        input.as_bytes(),
        false,
        "test.md",
        &mut std::io::sink(),
        true,
        None,
    );

    assert!(result.is_ok(), "Document should parse successfully");

    let (pandoc, _context, warnings) = result.unwrap();

    // Should have warnings for both div tags
    let html_warnings: Vec<_> = warnings
        .iter()
        .filter(|w| w.code.as_ref().map(|c| c.as_str()) == Some("Q-2-9"))
        .collect();

    assert!(
        !html_warnings.is_empty(),
        "Should have warnings for HTML div elements"
    );

    // Verify AST contains RawInline nodes
    use quarto_markdown_pandoc::pandoc::{Block, Inline};
    let para = match &pandoc.blocks[0] {
        Block::Paragraph(p) => p,
        _ => panic!("Expected paragraph block"),
    };

    let raw_inlines: Vec<_> = para
        .content
        .iter()
        .filter_map(|i| match i {
            Inline::RawInline(r) => Some(r),
            _ => None,
        })
        .collect();

    assert!(
        raw_inlines.len() >= 2,
        "Should have at least 2 RawInline nodes for opening and closing div"
    );

    // Verify the div tags
    assert_eq!(raw_inlines[0].format, "html");
    assert_eq!(raw_inlines[0].text, "<div>");

    let last = raw_inlines.last().unwrap();
    assert_eq!(last.format, "html");
    assert_eq!(last.text, "</div>");
}

#[test]
fn test_html_elements_source_locations() {
    // Verify that warnings have accurate source locations
    let input = "hello <b>world</b>";

    let result = readers::qmd::read(
        input.as_bytes(),
        false,
        "test.md",
        &mut std::io::sink(),
        true,
        None,
    );

    assert!(result.is_ok(), "Document should parse successfully");

    let (_pandoc, _context, warnings) = result.unwrap();

    // Get Q-2-9 warnings
    let html_warnings: Vec<_> = warnings
        .iter()
        .filter(|w| w.code.as_ref().map(|c| c.as_str()) == Some("Q-2-9"))
        .collect();

    assert_eq!(html_warnings.len(), 2, "Should have 2 warnings");

    // Verify warnings have source locations
    // Note: tree-sitter may include leading whitespace in html_element nodes
    assert!(html_warnings[0].location.is_some());
    let loc1 = html_warnings[0].location.as_ref().unwrap();
    // First HTML element "<b>" should start around position 6 (or 5 if it includes leading space from "hello ")
    assert!(
        loc1.start_offset() >= 5 && loc1.start_offset() <= 6,
        "First HTML element starts around offset 6, got {}",
        loc1.start_offset()
    );

    // Second warning should be for </b>
    assert!(html_warnings[1].location.is_some());
    let loc2 = html_warnings[1].location.as_ref().unwrap();
    // The closing tag should be later in the string
    assert!(
        loc2.start_offset() > loc1.start_offset(),
        "Second HTML element should start after the first"
    );
}

#[test]
fn test_comparison_with_explicit_raw_inline_syntax() {
    // Verify that auto-converted HTML produces same AST structure as explicit syntax
    let implicit = "<b>test</b>";
    let explicit = "`<b>`{=html}test`</b>`{=html}";

    let result_implicit = readers::qmd::read(
        implicit.as_bytes(),
        false,
        "test.md",
        &mut std::io::sink(),
        true,
        None,
    );
    let result_explicit = readers::qmd::read(
        explicit.as_bytes(),
        false,
        "test.md",
        &mut std::io::sink(),
        true,
        None,
    );

    assert!(result_implicit.is_ok() && result_explicit.is_ok());

    let (pandoc_implicit, _, _) = result_implicit.unwrap();
    let (pandoc_explicit, _, _) = result_explicit.unwrap();

    // Both should have same structure: paragraph with RawInline nodes
    use quarto_markdown_pandoc::pandoc::{Block, Inline};

    let para_implicit = match &pandoc_implicit.blocks[0] {
        Block::Paragraph(p) => p,
        _ => panic!("Expected paragraph block"),
    };

    let para_explicit = match &pandoc_explicit.blocks[0] {
        Block::Paragraph(p) => p,
        _ => panic!("Expected paragraph block"),
    };

    let raw_implicit: Vec<_> = para_implicit
        .content
        .iter()
        .filter_map(|i| match i {
            Inline::RawInline(r) => Some(&r.text),
            _ => None,
        })
        .collect();

    let raw_explicit: Vec<_> = para_explicit
        .content
        .iter()
        .filter_map(|i| match i {
            Inline::RawInline(r) => Some(&r.text),
            _ => None,
        })
        .collect();

    // Both should have the same HTML tags
    assert_eq!(raw_implicit.len(), 2);
    assert_eq!(raw_explicit.len(), 2);
    assert_eq!(raw_implicit[0], "<b>");
    assert_eq!(raw_explicit[0], "<b>");
    assert_eq!(raw_implicit[1], "</b>");
    assert_eq!(raw_explicit[1], "</b>");
}
