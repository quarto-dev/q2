//! Tests for code span processing.
//!
//! These tests exercise the code_span.rs treesitter utility functions
//! through the higher-level parsing API.

use pampa::pandoc::{Block, Inline};
use pampa::readers;

fn parse_qmd(input: &str) -> pampa::pandoc::Pandoc {
    let result = readers::qmd::read(
        input.as_bytes(),
        false,
        "test.qmd",
        &mut std::io::sink(),
        true,
        None,
    );
    result.expect("Failed to parse QMD").0
}

fn get_first_paragraph_inlines(pandoc: &pampa::pandoc::Pandoc) -> &Vec<Inline> {
    match &pandoc.blocks[0] {
        Block::Paragraph(p) => &p.content,
        _ => panic!("Expected paragraph block, got {:?}", pandoc.blocks[0]),
    }
}

// ============================================================================
// Basic inline code tests
// ============================================================================

#[test]
fn test_simple_inline_code() {
    let pandoc = parse_qmd("`hello`");
    let inlines = get_first_paragraph_inlines(&pandoc);

    assert_eq!(inlines.len(), 1);
    match &inlines[0] {
        Inline::Code(code) => {
            assert_eq!(code.text, "hello");
            assert!(code.attr.0.is_empty()); // no id
            assert!(code.attr.1.is_empty()); // no classes
        }
        _ => panic!("Expected Code inline, got {:?}", inlines[0]),
    }
}

#[test]
fn test_inline_code_with_spaces() {
    let pandoc = parse_qmd("`hello world`");
    let inlines = get_first_paragraph_inlines(&pandoc);

    assert_eq!(inlines.len(), 1);
    match &inlines[0] {
        Inline::Code(code) => {
            assert_eq!(code.text, "hello world");
        }
        _ => panic!("Expected Code inline, got {:?}", inlines[0]),
    }
}

#[test]
fn test_inline_code_with_special_chars() {
    let pandoc = parse_qmd("`x = y + 1`");
    let inlines = get_first_paragraph_inlines(&pandoc);

    assert_eq!(inlines.len(), 1);
    match &inlines[0] {
        Inline::Code(code) => {
            assert_eq!(code.text, "x = y + 1");
        }
        _ => panic!("Expected Code inline, got {:?}", inlines[0]),
    }
}

#[test]
fn test_code_with_only_whitespace() {
    // Code with only whitespace should be trimmed to empty
    let pandoc = parse_qmd("`   `");
    let inlines = get_first_paragraph_inlines(&pandoc);

    assert_eq!(inlines.len(), 1);
    match &inlines[0] {
        Inline::Code(code) => {
            // Whitespace is trimmed
            assert_eq!(code.text, "");
        }
        _ => panic!("Expected Code inline, got {:?}", inlines[0]),
    }
}

#[test]
fn test_inline_code_in_context() {
    let pandoc = parse_qmd("Use `print()` to output");
    let inlines = get_first_paragraph_inlines(&pandoc);

    // Should have: Str("Use"), Space, Code("print()"), Space, Str("to"), Space, Str("output")
    let code_inlines: Vec<_> = inlines
        .iter()
        .filter_map(|i| match i {
            Inline::Code(c) => Some(c),
            _ => None,
        })
        .collect();

    assert_eq!(code_inlines.len(), 1);
    assert_eq!(code_inlines[0].text, "print()");
}

// ============================================================================
// Code with attributes tests
// ============================================================================

#[test]
fn test_inline_code_with_class() {
    let pandoc = parse_qmd("`code`{.python}");
    let inlines = get_first_paragraph_inlines(&pandoc);

    assert_eq!(inlines.len(), 1);
    match &inlines[0] {
        Inline::Code(code) => {
            assert_eq!(code.text, "code");
            assert!(code.attr.1.contains(&"python".to_string()));
        }
        _ => panic!("Expected Code inline, got {:?}", inlines[0]),
    }
}

#[test]
fn test_inline_code_with_id() {
    let pandoc = parse_qmd("`code`{#my-code}");
    let inlines = get_first_paragraph_inlines(&pandoc);

    assert_eq!(inlines.len(), 1);
    match &inlines[0] {
        Inline::Code(code) => {
            assert_eq!(code.text, "code");
            assert_eq!(code.attr.0, "my-code");
        }
        _ => panic!("Expected Code inline, got {:?}", inlines[0]),
    }
}

#[test]
fn test_inline_code_with_multiple_classes() {
    let pandoc = parse_qmd("`code`{.python .highlight}");
    let inlines = get_first_paragraph_inlines(&pandoc);

    assert_eq!(inlines.len(), 1);
    match &inlines[0] {
        Inline::Code(code) => {
            assert_eq!(code.text, "code");
            assert!(code.attr.1.contains(&"python".to_string()));
            assert!(code.attr.1.contains(&"highlight".to_string()));
        }
        _ => panic!("Expected Code inline, got {:?}", inlines[0]),
    }
}

#[test]
fn test_inline_code_with_key_value() {
    let pandoc = parse_qmd("`code`{key=value}");
    let inlines = get_first_paragraph_inlines(&pandoc);

    assert_eq!(inlines.len(), 1);
    match &inlines[0] {
        Inline::Code(code) => {
            assert_eq!(code.text, "code");
            assert_eq!(code.attr.2.get("key"), Some(&"value".to_string()));
        }
        _ => panic!("Expected Code inline, got {:?}", inlines[0]),
    }
}

#[test]
fn test_inline_code_with_complex_attrs() {
    let pandoc = parse_qmd("`code`{#id .class1 .class2 key=value}");
    let inlines = get_first_paragraph_inlines(&pandoc);

    assert_eq!(inlines.len(), 1);
    match &inlines[0] {
        Inline::Code(code) => {
            assert_eq!(code.text, "code");
            assert_eq!(code.attr.0, "id");
            assert!(code.attr.1.contains(&"class1".to_string()));
            assert!(code.attr.1.contains(&"class2".to_string()));
            assert_eq!(code.attr.2.get("key"), Some(&"value".to_string()));
        }
        _ => panic!("Expected Code inline, got {:?}", inlines[0]),
    }
}

// ============================================================================
// Raw inline tests (code with =format)
// ============================================================================

#[test]
fn test_raw_inline_html() {
    let pandoc = parse_qmd("`<b>`{=html}");
    let inlines = get_first_paragraph_inlines(&pandoc);

    assert_eq!(inlines.len(), 1);
    match &inlines[0] {
        Inline::RawInline(raw) => {
            assert_eq!(raw.format, "html");
            assert_eq!(raw.text, "<b>");
        }
        _ => panic!("Expected RawInline, got {:?}", inlines[0]),
    }
}

#[test]
fn test_raw_inline_latex() {
    let pandoc = parse_qmd(r"`\textbf{bold}`{=latex}");
    let inlines = get_first_paragraph_inlines(&pandoc);

    assert_eq!(inlines.len(), 1);
    match &inlines[0] {
        Inline::RawInline(raw) => {
            assert_eq!(raw.format, "latex");
            assert_eq!(raw.text, r"\textbf{bold}");
        }
        _ => panic!("Expected RawInline, got {:?}", inlines[0]),
    }
}

#[test]
fn test_raw_inline_custom_format() {
    let pandoc = parse_qmd("`content`{=custom}");
    let inlines = get_first_paragraph_inlines(&pandoc);

    assert_eq!(inlines.len(), 1);
    match &inlines[0] {
        Inline::RawInline(raw) => {
            assert_eq!(raw.format, "custom");
            assert_eq!(raw.text, "content");
        }
        _ => panic!("Expected RawInline, got {:?}", inlines[0]),
    }
}

#[test]
fn test_multiple_raw_inlines() {
    let pandoc = parse_qmd("`<b>`{=html}text`</b>`{=html}");
    let inlines = get_first_paragraph_inlines(&pandoc);

    let raw_inlines: Vec<_> = inlines
        .iter()
        .filter_map(|i| match i {
            Inline::RawInline(r) => Some(r),
            _ => None,
        })
        .collect();

    assert_eq!(raw_inlines.len(), 2);
    assert_eq!(raw_inlines[0].format, "html");
    assert_eq!(raw_inlines[0].text, "<b>");
    assert_eq!(raw_inlines[1].format, "html");
    assert_eq!(raw_inlines[1].text, "</b>");
}

// ============================================================================
// Multiple code spans tests
// ============================================================================

#[test]
fn test_multiple_code_spans() {
    let pandoc = parse_qmd("`first` and `second`");
    let inlines = get_first_paragraph_inlines(&pandoc);

    let code_inlines: Vec<_> = inlines
        .iter()
        .filter_map(|i| match i {
            Inline::Code(c) => Some(c),
            _ => None,
        })
        .collect();

    assert_eq!(code_inlines.len(), 2);
    assert_eq!(code_inlines[0].text, "first");
    assert_eq!(code_inlines[1].text, "second");
}

#[test]
fn test_code_span_with_backtick_inside() {
    // Use double backticks to include a backtick in code
    let pandoc = parse_qmd("`` `code` ``");
    let inlines = get_first_paragraph_inlines(&pandoc);

    assert_eq!(inlines.len(), 1);
    match &inlines[0] {
        Inline::Code(code) => {
            assert!(code.text.contains("`"));
        }
        _ => panic!("Expected Code inline, got {:?}", inlines[0]),
    }
}

// ============================================================================
// Edge cases
// ============================================================================

#[test]
fn test_code_span_unicode() {
    let pandoc = parse_qmd("`héllo wörld`");
    let inlines = get_first_paragraph_inlines(&pandoc);

    assert_eq!(inlines.len(), 1);
    match &inlines[0] {
        Inline::Code(code) => {
            assert_eq!(code.text, "héllo wörld");
        }
        _ => panic!("Expected Code inline, got {:?}", inlines[0]),
    }
}

#[test]
fn test_code_span_emoji() {
    let pandoc = parse_qmd("`print(\"🎉\")`");
    let inlines = get_first_paragraph_inlines(&pandoc);

    assert_eq!(inlines.len(), 1);
    match &inlines[0] {
        Inline::Code(code) => {
            assert!(code.text.contains("🎉"));
        }
        _ => panic!("Expected Code inline, got {:?}", inlines[0]),
    }
}

#[test]
fn test_code_span_in_emphasis() {
    let pandoc = parse_qmd("*text `code` more*");
    let inlines = get_first_paragraph_inlines(&pandoc);

    // Should have Emph containing the code
    assert_eq!(inlines.len(), 1);
    match &inlines[0] {
        Inline::Emph(emph) => {
            let code_inlines: Vec<_> = emph
                .content
                .iter()
                .filter_map(|i| match i {
                    Inline::Code(c) => Some(c),
                    _ => None,
                })
                .collect();
            assert_eq!(code_inlines.len(), 1);
            assert_eq!(code_inlines[0].text, "code");
        }
        _ => panic!("Expected Emph inline, got {:?}", inlines[0]),
    }
}

#[test]
fn test_code_span_source_info() {
    let pandoc = parse_qmd("`code`");
    let inlines = get_first_paragraph_inlines(&pandoc);

    match &inlines[0] {
        Inline::Code(code) => {
            // Verify source info is present and reasonable
            let start = code.source_info.start_offset();
            let end = code.source_info.end_offset();
            assert!(end > start, "Source info should have positive length");
        }
        _ => panic!("Expected Code inline"),
    }
}
