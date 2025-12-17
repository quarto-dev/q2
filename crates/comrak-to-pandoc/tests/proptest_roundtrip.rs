/*
 * proptest_roundtrip.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Property-based tests for CommonMark subset roundtripping.
 *
 * These tests verify that generated Pandoc ASTs produce identical results
 * when parsed by pampa (qmd reader) and comrak (CommonMark parser).
 *
 * The approach:
 * 1. Generate a random Pandoc AST (constrained to CommonMark subset)
 * 2. Write it to markdown using pampa's qmd writer
 * 3. Parse the markdown with both pampa and comrak
 * 4. Normalize both ASTs to handle known differences
 * 5. Compare the normalized ASTs
 */

use comrak::{parse_document, Arena, Options};
use comrak_to_pandoc::{ast_eq_ignore_source, convert_document, normalize};
use proptest::prelude::*;

// Import generators as a local module
mod generators;
use generators::*;

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

/// Write a Pandoc AST to markdown using pampa's qmd writer
fn write_to_markdown(pandoc: &quarto_pandoc_types::Pandoc) -> String {
    let mut buf = Vec::new();
    pampa::writers::qmd::write(pandoc, &mut buf).expect("qmd write failed");
    String::from_utf8(buf).expect("invalid utf8")
}

/// Compare two ASTs after normalization, ignoring source info
fn asts_equivalent(
    comrak_ast: &quarto_pandoc_types::Pandoc,
    pampa_ast: &quarto_pandoc_types::Pandoc,
) -> bool {
    let comrak_normalized = normalize(comrak_ast.clone());
    let pampa_normalized = normalize(pampa_ast.clone());
    ast_eq_ignore_source(&comrak_normalized, &pampa_normalized)
}

/// Print detailed diff when ASTs don't match
fn print_ast_diff(
    markdown: &str,
    comrak_ast: &quarto_pandoc_types::Pandoc,
    pampa_ast: &quarto_pandoc_types::Pandoc,
) {
    let comrak_normalized = normalize(comrak_ast.clone());
    let pampa_normalized = normalize(pampa_ast.clone());

    eprintln!("=== MARKDOWN ===");
    eprintln!("{}", markdown);
    eprintln!("=== COMRAK AST (normalized) ===");
    eprintln!("{:#?}", comrak_normalized.blocks);
    eprintln!("=== PAMPA AST (normalized) ===");
    eprintln!("{:#?}", pampa_normalized.blocks);
}

// ============================================================================
// Phase 2: Plain Text (L0, B0)
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// Test plain text roundtripping (no markdown features)
    #[test]
    fn test_plain_text_roundtrip(ast in gen_plain_text_doc()) {
        let markdown = write_to_markdown(&ast);
        let comrak_ast = parse_with_comrak(&markdown);
        let pampa_ast = parse_with_pampa(&markdown);

        if !asts_equivalent(&comrak_ast, &pampa_ast) {
            print_ast_diff(&markdown, &comrak_ast, &pampa_ast);
            panic!("ASTs do not match for plain text");
        }
    }
}

// ============================================================================
// Phase 3: Style Inlines (L1-L3)
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// Test emphasis roundtripping (L1)
    #[test]
    fn test_emph_roundtrip(ast in gen_with_emph_doc()) {
        let markdown = write_to_markdown(&ast);
        let comrak_ast = parse_with_comrak(&markdown);
        let pampa_ast = parse_with_pampa(&markdown);

        if !asts_equivalent(&comrak_ast, &pampa_ast) {
            print_ast_diff(&markdown, &comrak_ast, &pampa_ast);
            panic!("ASTs do not match for emphasis");
        }
    }

    /// Test strong emphasis roundtripping (L2)
    #[test]
    fn test_strong_roundtrip(ast in gen_with_strong_doc()) {
        let markdown = write_to_markdown(&ast);
        let comrak_ast = parse_with_comrak(&markdown);
        let pampa_ast = parse_with_pampa(&markdown);

        if !asts_equivalent(&comrak_ast, &pampa_ast) {
            print_ast_diff(&markdown, &comrak_ast, &pampa_ast);
            panic!("ASTs do not match for strong");
        }
    }

    /// Test inline code roundtripping (L3)
    #[test]
    fn test_code_roundtrip(ast in gen_with_code_doc()) {
        let markdown = write_to_markdown(&ast);
        let comrak_ast = parse_with_comrak(&markdown);
        let pampa_ast = parse_with_pampa(&markdown);

        if !asts_equivalent(&comrak_ast, &pampa_ast) {
            print_ast_diff(&markdown, &comrak_ast, &pampa_ast);
            panic!("ASTs do not match for inline code");
        }
    }
}

// ============================================================================
// Phase 4: Links and Images (L4-L6)
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// Test link roundtripping (L4)
    #[test]
    fn test_link_roundtrip(ast in gen_with_link_doc()) {
        let markdown = write_to_markdown(&ast);
        let comrak_ast = parse_with_comrak(&markdown);
        let pampa_ast = parse_with_pampa(&markdown);

        if !asts_equivalent(&comrak_ast, &pampa_ast) {
            print_ast_diff(&markdown, &comrak_ast, &pampa_ast);
            panic!("ASTs do not match for links");
        }
    }

    /// Test image roundtripping (L5)
    #[test]
    fn test_image_roundtrip(ast in gen_with_image_doc()) {
        let markdown = write_to_markdown(&ast);
        let comrak_ast = parse_with_comrak(&markdown);
        let pampa_ast = parse_with_pampa(&markdown);

        if !asts_equivalent(&comrak_ast, &pampa_ast) {
            print_ast_diff(&markdown, &comrak_ast, &pampa_ast);
            panic!("ASTs do not match for images");
        }
    }
}

// ============================================================================
// Phase 5: Simple Blocks (B1-B3)
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// Test header roundtripping (B1)
    #[test]
    fn test_header_roundtrip(ast in gen_with_header_doc()) {
        let markdown = write_to_markdown(&ast);
        let comrak_ast = parse_with_comrak(&markdown);
        let pampa_ast = parse_with_pampa(&markdown);

        if !asts_equivalent(&comrak_ast, &pampa_ast) {
            print_ast_diff(&markdown, &comrak_ast, &pampa_ast);
            panic!("ASTs do not match for headers");
        }
    }

    /// Test code block roundtripping (B2)
    #[test]
    fn test_code_block_roundtrip(ast in gen_with_code_block_doc()) {
        let markdown = write_to_markdown(&ast);
        let comrak_ast = parse_with_comrak(&markdown);
        let pampa_ast = parse_with_pampa(&markdown);

        if !asts_equivalent(&comrak_ast, &pampa_ast) {
            print_ast_diff(&markdown, &comrak_ast, &pampa_ast);
            panic!("ASTs do not match for code blocks");
        }
    }

    /// Test horizontal rule roundtripping (B3)
    #[test]
    fn test_hr_roundtrip(ast in gen_with_hr_doc()) {
        let markdown = write_to_markdown(&ast);
        let comrak_ast = parse_with_comrak(&markdown);
        let pampa_ast = parse_with_pampa(&markdown);

        if !asts_equivalent(&comrak_ast, &pampa_ast) {
            print_ast_diff(&markdown, &comrak_ast, &pampa_ast);
            panic!("ASTs do not match for horizontal rules");
        }
    }
}

// ============================================================================
// Phase 6: Recursive Blocks (B4-B6)
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// Test blockquote roundtripping (B4)
    #[test]
    fn test_blockquote_roundtrip(ast in gen_with_blockquote_doc()) {
        let markdown = write_to_markdown(&ast);
        let comrak_ast = parse_with_comrak(&markdown);
        let pampa_ast = parse_with_pampa(&markdown);

        if !asts_equivalent(&comrak_ast, &pampa_ast) {
            print_ast_diff(&markdown, &comrak_ast, &pampa_ast);
            panic!("ASTs do not match for blockquotes");
        }
    }

    /// Test bullet list roundtripping (B5)
    #[test]
    fn test_bullet_list_roundtrip(ast in gen_with_bullet_list_doc()) {
        let markdown = write_to_markdown(&ast);
        let comrak_ast = parse_with_comrak(&markdown);
        let pampa_ast = parse_with_pampa(&markdown);

        if !asts_equivalent(&comrak_ast, &pampa_ast) {
            print_ast_diff(&markdown, &comrak_ast, &pampa_ast);
            panic!("ASTs do not match for bullet lists");
        }
    }
}

// ============================================================================
// Phase 7: Full Integration
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]

    /// Test full document roundtripping (all features)
    #[test]
    fn test_full_roundtrip(ast in gen_full_doc()) {
        let markdown = write_to_markdown(&ast);
        let comrak_ast = parse_with_comrak(&markdown);
        let pampa_ast = parse_with_pampa(&markdown);

        if !asts_equivalent(&comrak_ast, &pampa_ast) {
            print_ast_diff(&markdown, &comrak_ast, &pampa_ast);
            panic!("ASTs do not match for full document");
        }
    }
}
