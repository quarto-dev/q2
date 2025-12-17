/*
 * differential.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Differential tests comparing comrak→pandoc conversion with pampa's output.
 *
 * These tests document known differences between CommonMark (comrak) and
 * qmd (pampa). Tests marked with #[ignore] have known differences that are
 * documented.
 *
 * ## Known Differences
 *
 * 1. **Heading IDs**: pampa auto-generates IDs for headings (e.g., "heading-1").
 *    CommonMark/comrak doesn't generate IDs - attr.id is empty.
 *
 * 2. **Standalone Images**: pampa wraps standalone images in Figure blocks.
 *    CommonMark/comrak keeps them as inline Image elements in a Paragraph.
 *
 * 3. **Code Block Attributes**: pampa may add additional attributes to code blocks.
 *
 * These differences represent qmd extensions beyond the CommonMark subset.
 * For the CommonMark-compatible subset, we may need to:
 * - Add options to pampa to disable these extensions
 * - Or normalize the ASTs before comparison
 */

use comrak::{parse_document, Arena, Options};
use comrak_to_pandoc::{ast_eq_ignore_source, convert_document};

/// Parse markdown with comrak and convert to Pandoc AST
fn parse_with_comrak(markdown: &str) -> quarto_pandoc_types::Pandoc {
    let arena = Arena::new();
    let options = Options::default();
    let root = parse_document(&arena, markdown, &options);
    convert_document(root)
}

/// Parse markdown with pampa and get Pandoc AST
fn parse_with_pampa(markdown: &str) -> quarto_pandoc_types::Pandoc {
    let mut output = Vec::new();
    let (pandoc, _ctx, _errors) = pampa::readers::qmd::read(
        markdown.as_bytes(),
        false,
        "test.md",
        &mut output,
        true,
        None,
    )
    .expect("pampa parse failed");
    pandoc
}

/// Compare two ASTs and print diff if they don't match
fn assert_asts_match(comrak_ast: &quarto_pandoc_types::Pandoc, pampa_ast: &quarto_pandoc_types::Pandoc, input: &str) {
    if !ast_eq_ignore_source(comrak_ast, pampa_ast) {
        eprintln!("AST mismatch for input: {:?}", input);
        eprintln!("Comrak blocks: {:#?}", comrak_ast.blocks);
        eprintln!("Pampa blocks: {:#?}", pampa_ast.blocks);
        panic!("ASTs do not match");
    }
}

// ============================================================================
// Simple element tests
// ============================================================================

#[test]
fn test_simple_paragraph() {
    let md = "Hello world.\n";
    let comrak = parse_with_comrak(md);
    let pampa = parse_with_pampa(md);
    assert_asts_match(&comrak, &pampa, md);
}

#[test]
fn test_two_paragraphs() {
    let md = "First paragraph.\n\nSecond paragraph.\n";
    let comrak = parse_with_comrak(md);
    let pampa = parse_with_pampa(md);
    assert_asts_match(&comrak, &pampa, md);
}

// KNOWN DIFFERENCE: pampa auto-generates heading IDs (e.g., "heading-1")
// CommonMark/comrak doesn't generate IDs
#[test]
#[ignore = "pampa auto-generates heading IDs, comrak doesn't"]
fn test_atx_heading_h1() {
    let md = "# Heading 1\n";
    let comrak = parse_with_comrak(md);
    let pampa = parse_with_pampa(md);
    assert_asts_match(&comrak, &pampa, md);
}

// KNOWN DIFFERENCE: pampa auto-generates heading IDs
#[test]
#[ignore = "pampa auto-generates heading IDs, comrak doesn't"]
fn test_atx_heading_h2() {
    let md = "## Heading 2\n";
    let comrak = parse_with_comrak(md);
    let pampa = parse_with_pampa(md);
    assert_asts_match(&comrak, &pampa, md);
}

// KNOWN DIFFERENCE: pampa auto-generates heading IDs
#[test]
#[ignore = "pampa auto-generates heading IDs, comrak doesn't"]
fn test_atx_heading_with_inline() {
    let md = "# Hello *world*\n";
    let comrak = parse_with_comrak(md);
    let pampa = parse_with_pampa(md);
    assert_asts_match(&comrak, &pampa, md);
}

// ============================================================================
// Inline formatting tests
// ============================================================================

#[test]
fn test_emphasis_asterisk() {
    let md = "*emphasized*\n";
    let comrak = parse_with_comrak(md);
    let pampa = parse_with_pampa(md);
    assert_asts_match(&comrak, &pampa, md);
}

#[test]
fn test_emphasis_underscore() {
    let md = "_emphasized_\n";
    let comrak = parse_with_comrak(md);
    let pampa = parse_with_pampa(md);
    assert_asts_match(&comrak, &pampa, md);
}

#[test]
fn test_strong_asterisk() {
    let md = "**strong**\n";
    let comrak = parse_with_comrak(md);
    let pampa = parse_with_pampa(md);
    assert_asts_match(&comrak, &pampa, md);
}

#[test]
fn test_strong_underscore() {
    let md = "__strong__\n";
    let comrak = parse_with_comrak(md);
    let pampa = parse_with_pampa(md);
    assert_asts_match(&comrak, &pampa, md);
}

#[test]
fn test_inline_code() {
    let md = "`code`\n";
    let comrak = parse_with_comrak(md);
    let pampa = parse_with_pampa(md);
    assert_asts_match(&comrak, &pampa, md);
}

// ============================================================================
// Link and image tests
// ============================================================================

#[test]
fn test_inline_link() {
    let md = "[link text](http://example.com)\n";
    let comrak = parse_with_comrak(md);
    let pampa = parse_with_pampa(md);
    assert_asts_match(&comrak, &pampa, md);
}

#[test]
fn test_inline_link_with_title() {
    let md = "[link](http://example.com \"title\")\n";
    let comrak = parse_with_comrak(md);
    let pampa = parse_with_pampa(md);
    assert_asts_match(&comrak, &pampa, md);
}

#[test]
fn test_autolink() {
    let md = "<http://example.com>\n";
    let comrak = parse_with_comrak(md);
    let pampa = parse_with_pampa(md);
    assert_asts_match(&comrak, &pampa, md);
}

// KNOWN DIFFERENCE: pampa wraps standalone images in Figure blocks
// CommonMark/comrak keeps them as inline Image elements in a Paragraph
#[test]
#[ignore = "pampa wraps standalone images in Figure, comrak keeps them in Paragraph"]
fn test_image() {
    let md = "![alt text](image.png)\n";
    let comrak = parse_with_comrak(md);
    let pampa = parse_with_pampa(md);
    assert_asts_match(&comrak, &pampa, md);
}

// ============================================================================
// List tests
// ============================================================================

#[test]
fn test_bullet_list_tight() {
    let md = "- one\n- two\n- three\n";
    let comrak = parse_with_comrak(md);
    let pampa = parse_with_pampa(md);
    assert_asts_match(&comrak, &pampa, md);
}

#[test]
fn test_bullet_list_loose() {
    let md = "- one\n\n- two\n\n- three\n";
    let comrak = parse_with_comrak(md);
    let pampa = parse_with_pampa(md);
    assert_asts_match(&comrak, &pampa, md);
}

#[test]
fn test_ordered_list() {
    let md = "1. one\n2. two\n3. three\n";
    let comrak = parse_with_comrak(md);
    let pampa = parse_with_pampa(md);
    assert_asts_match(&comrak, &pampa, md);
}

// ============================================================================
// Code block tests
// ============================================================================

// KNOWN DIFFERENCE: pampa may add different attributes to code blocks
#[test]
#[ignore = "investigating code block attribute differences"]
fn test_fenced_code_block() {
    let md = "```\ncode\n```\n";
    let comrak = parse_with_comrak(md);
    let pampa = parse_with_pampa(md);
    assert_asts_match(&comrak, &pampa, md);
}

// KNOWN DIFFERENCE: pampa may add different attributes to code blocks
#[test]
#[ignore = "investigating code block attribute differences"]
fn test_fenced_code_block_with_lang() {
    let md = "```python\nprint('hello')\n```\n";
    let comrak = parse_with_comrak(md);
    let pampa = parse_with_pampa(md);
    assert_asts_match(&comrak, &pampa, md);
}

// ============================================================================
// Block quote tests
// ============================================================================

#[test]
fn test_blockquote() {
    let md = "> quoted text\n";
    let comrak = parse_with_comrak(md);
    let pampa = parse_with_pampa(md);
    assert_asts_match(&comrak, &pampa, md);
}

#[test]
fn test_nested_blockquote() {
    let md = "> outer\n> > inner\n";
    let comrak = parse_with_comrak(md);
    let pampa = parse_with_pampa(md);
    assert_asts_match(&comrak, &pampa, md);
}

// ============================================================================
// Other block tests
// ============================================================================

#[test]
fn test_horizontal_rule() {
    let md = "---\n";
    let comrak = parse_with_comrak(md);
    let pampa = parse_with_pampa(md);
    assert_asts_match(&comrak, &pampa, md);
}

// ============================================================================
// Line break tests
// ============================================================================

#[test]
fn test_soft_break() {
    let md = "line one\nline two\n";
    let comrak = parse_with_comrak(md);
    let pampa = parse_with_pampa(md);
    assert_asts_match(&comrak, &pampa, md);
}

#[test]
fn test_hard_break() {
    let md = "line one\\\nline two\n";
    let comrak = parse_with_comrak(md);
    let pampa = parse_with_pampa(md);
    assert_asts_match(&comrak, &pampa, md);
}
