/*
 * test_html_attr_handling.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Tests for HTML attribute handling in the HTML writer.
 * These tests verify that standard HTML5 attributes are NOT prefixed with data-,
 * while custom attributes ARE prefixed with data-.
 */

use pampa::pandoc::{treesitter_to_pandoc, ASTContext};
use pampa::utils::diagnostic_collector::DiagnosticCollector;
use pampa::writers::html::write_blocks_to;
use tree_sitter_qmd::MarkdownParser;

/// Helper to render QMD to HTML body
fn render_qmd_to_html(qmd: &str) -> String {
    let full_qmd = format!("---\ntitle: Test\n---\n\n{}", qmd);
    let input_bytes = full_qmd.as_bytes();

    let mut parser = MarkdownParser::default();
    let tree = parser.parse(input_bytes, None).expect("Failed to parse");
    let mut error_collector = DiagnosticCollector::new();
    let pandoc = treesitter_to_pandoc(
        &mut std::io::sink(),
        &tree,
        input_bytes,
        &ASTContext::anonymous(),
        &mut error_collector,
    )
    .unwrap();

    let mut output = Vec::new();
    write_blocks_to(&pandoc.blocks, &mut output).unwrap();
    String::from_utf8(output).unwrap()
}

// =============================================================================
// Tests for standard HTML5 attributes that should NOT be prefixed with data-
// =============================================================================

#[test]
fn test_style_attribute_not_prefixed() {
    let html = render_qmd_to_html(r#"[text]{style="color:red"}"#);
    assert!(
        html.contains(r#"style="color:red""#),
        "style attribute should not be prefixed with data-. Got: {}",
        html
    );
    assert!(
        !html.contains("data-style"),
        "style should not have data- prefix. Got: {}",
        html
    );
}

#[test]
fn test_title_attribute_not_prefixed() {
    let html = render_qmd_to_html(r#"[text]{title="tooltip"}"#);
    assert!(
        html.contains(r#"title="tooltip""#),
        "title attribute should not be prefixed with data-. Got: {}",
        html
    );
}

#[test]
fn test_dir_attribute_not_prefixed() {
    let html = render_qmd_to_html(r#"[text]{dir="ltr"}"#);
    assert!(
        html.contains(r#"dir="ltr""#),
        "dir attribute should not be prefixed with data-. Got: {}",
        html
    );
}

#[test]
fn test_lang_attribute_not_prefixed() {
    let html = render_qmd_to_html(r#"[text]{lang="en"}"#);
    assert!(
        html.contains(r#"lang="en""#),
        "lang attribute should not be prefixed with data-. Got: {}",
        html
    );
}

#[test]
fn test_width_attribute_not_prefixed() {
    let html = render_qmd_to_html(r#"[text]{width="100"}"#);
    assert!(
        html.contains(r#"width="100""#),
        "width attribute should not be prefixed with data-. Got: {}",
        html
    );
}

#[test]
fn test_height_attribute_not_prefixed() {
    let html = render_qmd_to_html(r#"[text]{height="50"}"#);
    assert!(
        html.contains(r#"height="50""#),
        "height attribute should not be prefixed with data-. Got: {}",
        html
    );
}

// =============================================================================
// Tests for data-* attributes that should be preserved as-is
// =============================================================================

#[test]
fn test_data_attribute_not_doubled() {
    let html = render_qmd_to_html(r#"[text]{data-foo="bar"}"#);
    assert!(
        html.contains(r#"data-foo="bar""#),
        "data-foo should be preserved as-is. Got: {}",
        html
    );
    assert!(
        !html.contains("data-data-"),
        "data- prefix should not be doubled. Got: {}",
        html
    );
}

#[test]
fn test_data_cites_attribute_preserved() {
    let html = render_qmd_to_html(r#"[text]{data-cites="smith2020"}"#);
    assert!(
        html.contains(r#"data-cites="smith2020""#),
        "data-cites should be preserved. Got: {}",
        html
    );
}

// =============================================================================
// Tests for aria-* attributes that should be preserved as-is
// =============================================================================

#[test]
fn test_aria_label_not_prefixed() {
    let html = render_qmd_to_html(r#"[text]{aria-label="description"}"#);
    assert!(
        html.contains(r#"aria-label="description""#),
        "aria-label should be preserved as-is. Got: {}",
        html
    );
    assert!(
        !html.contains("data-aria-"),
        "aria- prefix should not get data- prefix. Got: {}",
        html
    );
}

#[test]
fn test_aria_hidden_not_prefixed() {
    let html = render_qmd_to_html(r#"[text]{aria-hidden="true"}"#);
    assert!(
        html.contains(r#"aria-hidden="true""#),
        "aria-hidden should be preserved as-is. Got: {}",
        html
    );
}

// =============================================================================
// Tests for custom attributes that SHOULD be prefixed with data-
// =============================================================================

#[test]
fn test_custom_attribute_prefixed() {
    let html = render_qmd_to_html(r#"[text]{custom="value"}"#);
    assert!(
        html.contains(r#"data-custom="value""#),
        "custom attributes should be prefixed with data-. Got: {}",
        html
    );
}

#[test]
fn test_unknown_attribute_prefixed() {
    let html = render_qmd_to_html(r#"[text]{myattr="myvalue"}"#);
    assert!(
        html.contains(r#"data-myattr="myvalue""#),
        "unknown attributes should be prefixed with data-. Got: {}",
        html
    );
}

// =============================================================================
// Tests for multiple attributes combined
// =============================================================================

#[test]
fn test_mixed_attributes() {
    let html = render_qmd_to_html(r#"[text]{style="color:red" title="tip" custom="val"}"#);
    assert!(
        html.contains(r#"style="color:red""#),
        "style should not be prefixed. Got: {}",
        html
    );
    assert!(
        html.contains(r#"title="tip""#),
        "title should not be prefixed. Got: {}",
        html
    );
    assert!(
        html.contains(r#"data-custom="val""#),
        "custom should be prefixed. Got: {}",
        html
    );
}

// =============================================================================
// Tests for div attributes
// =============================================================================

#[test]
fn test_div_style_attribute() {
    let html = render_qmd_to_html(
        r#"::: {style="background:blue"}
content
:::"#,
    );
    assert!(
        html.contains(r#"style="background:blue""#),
        "div style should not be prefixed. Got: {}",
        html
    );
}
