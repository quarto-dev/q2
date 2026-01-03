//! Tests for shortcode parsing through the treesitter parser.
//!
//! These tests exercise the shortcode parsing functions in
//! treesitter_utils/shortcode.rs through the higher-level parsing API.
//!
//! Note: Shortcodes are parsed as Inline::Shortcode internally but are
//! converted to Span format in the output. These tests verify the parsing
//! by examining the output Span structure.

use pampa::pandoc::{Block, Inline, Span};
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

/// Find the first shortcode span (class "quarto-shortcode__") in the paragraph
fn get_first_shortcode_span(pandoc: &pampa::pandoc::Pandoc) -> &Span {
    let inlines = get_first_paragraph_inlines(pandoc);
    for inline in inlines {
        if let Inline::Span(span) = inline {
            if span.attr.1.contains(&"quarto-shortcode__".to_string()) {
                return span;
            }
        }
    }
    panic!("No shortcode span found in first paragraph")
}

/// Get the shortcode name from a shortcode span
fn get_shortcode_name(span: &Span) -> Option<&str> {
    if let Some(Inline::Span(param_span)) = span.content.first() {
        param_span.attr.2.get("data-value").map(|s| s.as_str())
    } else {
        None
    }
}

/// Get parameter spans (all content spans after the name)
fn get_param_spans(span: &Span) -> Vec<&Span> {
    span.content.iter().skip(1).filter_map(|inline| {
        if let Inline::Span(s) = inline {
            Some(s)
        } else {
            None
        }
    }).collect()
}

/// Check if a param span is a keyword param (has data-key)
fn is_keyword_param(span: &Span) -> bool {
    span.attr.2.contains_key("data-key")
}

// ============================================================================
// Basic shortcode parsing tests
// ============================================================================

#[test]
fn test_parse_shortcode_name_only() {
    let pandoc = parse_qmd("{{< myshortcode >}}");
    let span = get_first_shortcode_span(&pandoc);

    assert_eq!(get_shortcode_name(span), Some("myshortcode"));
    // Only the name, no additional params
    assert_eq!(span.content.len(), 1);
}

#[test]
fn test_parse_shortcode_with_positional_arg() {
    let pandoc = parse_qmd("{{< video test.mp4 >}}");
    let span = get_first_shortcode_span(&pandoc);

    assert_eq!(get_shortcode_name(span), Some("video"));
    let params = get_param_spans(span);
    assert_eq!(params.len(), 1);
    assert_eq!(params[0].attr.2.get("data-value").map(|s| s.as_str()), Some("test.mp4"));
}

#[test]
fn test_parse_shortcode_multiple_positional_args() {
    let pandoc = parse_qmd("{{< embed file.py cell1 cell2 >}}");
    let span = get_first_shortcode_span(&pandoc);

    assert_eq!(get_shortcode_name(span), Some("embed"));
    let params = get_param_spans(span);
    assert_eq!(params.len(), 3);
}

// ============================================================================
// Boolean argument parsing tests
// ============================================================================

#[test]
fn test_parse_boolean_true() {
    let pandoc = parse_qmd("{{< toggle true >}}");
    let span = get_first_shortcode_span(&pandoc);

    assert_eq!(get_shortcode_name(span), Some("toggle"));
    let params = get_param_spans(span);
    assert_eq!(params.len(), 1);
    assert_eq!(params[0].attr.2.get("data-value").map(|s| s.as_str()), Some("true"));
}

#[test]
fn test_parse_boolean_false() {
    let pandoc = parse_qmd("{{< toggle false >}}");
    let span = get_first_shortcode_span(&pandoc);

    let params = get_param_spans(span);
    assert_eq!(params.len(), 1);
    assert_eq!(params[0].attr.2.get("data-value").map(|s| s.as_str()), Some("false"));
}

// ============================================================================
// Numeric argument parsing tests
// ============================================================================

#[test]
fn test_parse_number_integer() {
    let pandoc = parse_qmd("{{< counter 42 >}}");
    let span = get_first_shortcode_span(&pandoc);

    assert_eq!(get_shortcode_name(span), Some("counter"));
    let params = get_param_spans(span);
    assert_eq!(params.len(), 1);
    assert_eq!(params[0].attr.2.get("data-value").map(|s| s.as_str()), Some("42"));
}

#[test]
fn test_parse_number_float() {
    let pandoc = parse_qmd("{{< scale 1.5 >}}");
    let span = get_first_shortcode_span(&pandoc);

    let params = get_param_spans(span);
    assert_eq!(params.len(), 1);
    assert_eq!(params[0].attr.2.get("data-value").map(|s| s.as_str()), Some("1.5"));
}

#[test]
fn test_parse_number_negative() {
    let pandoc = parse_qmd("{{< offset -10 >}}");
    let span = get_first_shortcode_span(&pandoc);

    let params = get_param_spans(span);
    assert_eq!(params.len(), 1);
    assert_eq!(params[0].attr.2.get("data-value").map(|s| s.as_str()), Some("-10"));
}

// ============================================================================
// Keyword argument parsing tests
// ============================================================================

#[test]
fn test_parse_keyword_string() {
    let pandoc = parse_qmd("{{< include file=test.qmd >}}");
    let span = get_first_shortcode_span(&pandoc);

    assert_eq!(get_shortcode_name(span), Some("include"));
    let params = get_param_spans(span);
    assert_eq!(params.len(), 1);
    assert!(is_keyword_param(params[0]));
    assert_eq!(params[0].attr.2.get("data-key").map(|s| s.as_str()), Some("file"));
    assert_eq!(params[0].attr.2.get("data-value").map(|s| s.as_str()), Some("test.qmd"));
}

#[test]
fn test_parse_keyword_number() {
    let pandoc = parse_qmd("{{< video width=800 >}}");
    let span = get_first_shortcode_span(&pandoc);

    let params = get_param_spans(span);
    assert_eq!(params.len(), 1);
    assert!(is_keyword_param(params[0]));
    assert_eq!(params[0].attr.2.get("data-key").map(|s| s.as_str()), Some("width"));
    assert_eq!(params[0].attr.2.get("data-value").map(|s| s.as_str()), Some("800"));
}

#[test]
fn test_parse_keyword_boolean() {
    let pandoc = parse_qmd("{{< widget enabled=true >}}");
    let span = get_first_shortcode_span(&pandoc);

    let params = get_param_spans(span);
    assert_eq!(params.len(), 1);
    assert!(is_keyword_param(params[0]));
    assert_eq!(params[0].attr.2.get("data-key").map(|s| s.as_str()), Some("enabled"));
    assert_eq!(params[0].attr.2.get("data-value").map(|s| s.as_str()), Some("true"));
}

#[test]
fn test_parse_multiple_keywords() {
    let pandoc = parse_qmd("{{< embed src=chart.py width=400 height=300 >}}");
    let span = get_first_shortcode_span(&pandoc);

    let params = get_param_spans(span);
    assert_eq!(params.len(), 3);
    // All should be keyword params
    for param in &params {
        assert!(is_keyword_param(param));
    }
}

#[test]
fn test_parse_mixed_positional_and_keyword() {
    let pandoc = parse_qmd("{{< video test.mp4 width=800 >}}");
    let span = get_first_shortcode_span(&pandoc);

    assert_eq!(get_shortcode_name(span), Some("video"));
    let params = get_param_spans(span);
    assert_eq!(params.len(), 2);
    // First is positional (no data-key)
    assert!(!is_keyword_param(params[0]));
    assert_eq!(params[0].attr.2.get("data-value").map(|s| s.as_str()), Some("test.mp4"));
    // Second is keyword
    assert!(is_keyword_param(params[1]));
}

// ============================================================================
// Escaped shortcode tests
// ============================================================================

#[test]
fn test_parse_escaped_shortcode() {
    let pandoc = parse_qmd("{{{< myshortcode >}}}");
    let span = get_first_shortcode_span(&pandoc);

    // Escaped shortcode should still parse
    assert_eq!(get_shortcode_name(span), Some("myshortcode"));
}

#[test]
fn test_parse_escaped_with_args() {
    let pandoc = parse_qmd("{{{< video test.mp4 >}}}");
    let span = get_first_shortcode_span(&pandoc);

    assert_eq!(get_shortcode_name(span), Some("video"));
    let params = get_param_spans(span);
    assert!(!params.is_empty());
}

// ============================================================================
// Context tests
// ============================================================================

#[test]
fn test_shortcode_in_paragraph_context() {
    let pandoc = parse_qmd("Before {{< myshortcode >}} after");
    let inlines = get_first_paragraph_inlines(&pandoc);

    // Count shortcode spans
    let shortcode_count = inlines
        .iter()
        .filter(|i| {
            if let Inline::Span(span) = i {
                span.attr.1.contains(&"quarto-shortcode__".to_string())
            } else {
                false
            }
        })
        .count();
    assert_eq!(shortcode_count, 1);
}

#[test]
fn test_multiple_shortcodes_in_paragraph() {
    let pandoc = parse_qmd("{{< first >}} and {{< second >}}");
    let inlines = get_first_paragraph_inlines(&pandoc);

    let shortcode_spans: Vec<_> = inlines
        .iter()
        .filter_map(|i| {
            if let Inline::Span(span) = i {
                if span.attr.1.contains(&"quarto-shortcode__".to_string()) {
                    return Some(span);
                }
            }
            None
        })
        .collect();

    assert_eq!(shortcode_spans.len(), 2);
    assert_eq!(get_shortcode_name(shortcode_spans[0]), Some("first"));
    assert_eq!(get_shortcode_name(shortcode_spans[1]), Some("second"));
}

// ============================================================================
// Quoted string tests
// ============================================================================
// Note: Quoted string parsing in shortcodes currently has limitations.
// The parser produces empty strings for quoted arguments.
// TODO: Fix parser to handle quoted strings in shortcodes properly.
