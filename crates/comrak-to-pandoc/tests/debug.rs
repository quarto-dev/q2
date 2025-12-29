fn main() {}

#[cfg(test)]
mod tests {
    use comrak::{Arena, Options, parse_document};
    use comrak_to_pandoc::{ast_eq_ignore_source, convert_document, normalize};

    fn debug_roundtrip(markdown: &str) -> bool {
        // Parse with comrak
        let arena = Arena::new();
        let options = Options::default();
        let root = parse_document(&arena, markdown, &options);
        let comrak_ast = convert_document(root);

        // Parse with pampa
        let mut output = Vec::new();
        let (pampa_ast, _ctx, _errors) = pampa::readers::qmd::read(
            markdown.as_bytes(),
            false,
            "test.md",
            &mut output,
            true,
            None,
        )
        .expect("pampa parse failed");

        eprintln!("=== MARKDOWN ===");
        eprintln!("{:?}", markdown);

        eprintln!("\n=== COMRAK AST (raw blocks) ===");
        eprintln!("{:#?}", comrak_ast.blocks);

        eprintln!("\n=== PAMPA AST (raw blocks) ===");
        eprintln!("{:#?}", pampa_ast.blocks);

        let comrak_normalized = normalize(comrak_ast.clone());
        let pampa_normalized = normalize(pampa_ast.clone());

        eprintln!("\n=== COMRAK AST (normalized) ===");
        eprintln!("{:#?}", comrak_normalized.blocks);

        eprintln!("\n=== PAMPA AST (normalized) ===");
        eprintln!("{:#?}", pampa_normalized.blocks);

        let result = ast_eq_ignore_source(&comrak_normalized, &pampa_normalized);
        eprintln!("\n=== EQUAL (ignoring source)? ===");
        eprintln!("{}", result);

        result
    }

    #[test]
    fn debug_emph() {
        assert!(debug_roundtrip("aA *aA*\n"));
    }

    #[test]
    fn debug_two_code_spans() {
        // This was the minimized failing case from the image test
        assert!(debug_roundtrip("`Aa` `Aa`\n"));
    }

    #[test]
    fn debug_code_space_code() {
        assert!(debug_roundtrip("`code1` `code2`\n"));
    }

    #[test]
    fn debug_simple_emph() {
        assert!(debug_roundtrip("*hello*\n"));
    }

    #[test]
    fn debug_emph_after_word() {
        assert!(debug_roundtrip("word *emph*\n"));
    }

    #[test]
    fn debug_strong() {
        assert!(debug_roundtrip("**strong**\n"));
    }

    #[test]
    fn debug_strong_after_word() {
        assert!(debug_roundtrip("word **strong**\n"));
    }

    #[test]
    fn debug_link() {
        assert!(debug_roundtrip("[text](https://example.com)\n"));
    }

    #[test]
    fn debug_header() {
        assert!(debug_roundtrip("# heading\n"));
    }

    #[test]
    fn debug_code_block() {
        assert!(debug_roundtrip("```python\ncode\n```\n"));
    }

    #[test]
    fn debug_code_block_no_lang() {
        assert!(debug_roundtrip("```\ncode\n```\n"));
    }

    #[test]
    fn debug_strong_with_code() {
        // Minimized failing case from test_image_roundtrip
        assert!(debug_roundtrip("**AA `AA`**\n"));
    }

    #[test]
    fn debug_emph_with_code() {
        assert!(debug_roundtrip("*word `code`*\n"));
    }

    // =========================================================================
    // KNOWN PARSER DIFFERENCES
    // These tests are commented out because they demonstrate known differences
    // between pampa and comrak that cannot be normalized.
    // =========================================================================

    // Nested emphasis: pampa produces [Emph, Emph, Emph], comrak produces
    // [Emph([..., Strong([...]), ...])]
    #[test]
    #[ignore = "known parser difference: nested emphasis"]
    fn debug_nested_strong_in_emph() {
        debug_roundtrip("*some **strong** text*\n");
    }

    // HR inside blockquote
    #[test]
    #[ignore = "known parser difference: HR in blockquote"]
    fn debug_hr_in_blockquote() {
        debug_roundtrip("> ---\n");
    }

    // List inside blockquote
    #[test]
    #[ignore = "known parser difference: list in blockquote"]
    fn debug_list_in_blockquote() {
        debug_roundtrip("> - item\n");
    }

    // Hard line break - testing if this actually works
    #[test]
    fn debug_hard_linebreak() {
        // Using raw string to avoid shell escaping issues
        let markdown = r"line one\
line two
";
        assert!(debug_roundtrip(markdown));
    }

    // Line break inside image alt text - from full_roundtrip failure
    #[test]
    #[ignore = "testing linebreak in image alt"]
    fn debug_linebreak_in_image_alt() {
        let markdown = r"![*aA*\
`aA`](https://aaa.example.com)
";
        debug_roundtrip(markdown);
    }

    #[test]
    fn debug_header_with_code() {
        // From failing code_block_roundtrip test
        assert!(debug_roundtrip("#### `code` text more\n"));
    }

    // =========================================================================
    // DISCOVERED PARSER DIFFERENCES (from property testing with all features)
    // =========================================================================

    #[test]
    #[ignore = "pampa bug: HR in blockquote produces empty content"]
    fn debug_hr_then_text_in_blockquote() {
        // Minimal failing case from test_blockquote_roundtrip
        // AST: BlockQuote([HorizontalRule, Paragraph("aA")]), HorizontalRule
        // The trailing HR outside the blockquote might matter
        debug_roundtrip("> ---\n> \n> aA\n\n---\n");
    }

    #[test]
    fn debug_hr_in_blockquote_simple() {
        // Just HR followed by text in blockquote (without trailing HR)
        assert!(debug_roundtrip("> ---\n> \n> aA\n"));
    }

    #[test]
    fn debug_single_list_in_blockquote() {
        // Single list in blockquote - should work
        assert!(debug_roundtrip("> - Aa\n> - Aa\n"));
    }

    #[test]
    #[ignore = "studying list-in-blockquote issue"]
    fn debug_two_lists_in_blockquote() {
        // Two lists in blockquote - from minimal failing case
        // BlockQuote([BulletList([...]), BulletList([...])])
        debug_roundtrip("> - Aa\n> - Aa\n>\n> - Aa\n> - Aa\n");
    }

    #[test]
    #[ignore = "studying qmd_writer output for two lists"]
    fn debug_qmd_writer_two_lists_in_blockquote() {
        use quarto_pandoc_types::*;
        use quarto_source_map::{FileId, SourceInfo};

        fn empty_source_info() -> SourceInfo {
            SourceInfo::original(FileId(0), 0, 0)
        }

        // Create the AST that the proptest minimized to
        let ast = Pandoc {
            meta: ConfigValue::default(),
            blocks: vec![Block::BlockQuote(BlockQuote {
                content: vec![
                    Block::BulletList(BulletList {
                        content: vec![
                            vec![Block::Plain(Plain {
                                content: vec![Inline::Str(Str {
                                    text: "Aa".to_string(),
                                    source_info: empty_source_info(),
                                })],
                                source_info: empty_source_info(),
                            })],
                            vec![Block::Plain(Plain {
                                content: vec![Inline::Str(Str {
                                    text: "Aa".to_string(),
                                    source_info: empty_source_info(),
                                })],
                                source_info: empty_source_info(),
                            })],
                        ],
                        source_info: empty_source_info(),
                    }),
                    Block::BulletList(BulletList {
                        content: vec![
                            vec![Block::Plain(Plain {
                                content: vec![Inline::Str(Str {
                                    text: "Aa".to_string(),
                                    source_info: empty_source_info(),
                                })],
                                source_info: empty_source_info(),
                            })],
                            vec![Block::Plain(Plain {
                                content: vec![Inline::Str(Str {
                                    text: "Aa".to_string(),
                                    source_info: empty_source_info(),
                                })],
                                source_info: empty_source_info(),
                            })],
                        ],
                        source_info: empty_source_info(),
                    }),
                ],
                source_info: empty_source_info(),
            })],
        };

        // Write to markdown
        let mut buf = Vec::new();
        pampa::writers::qmd::write(&ast, &mut buf).expect("qmd write failed");
        let markdown = String::from_utf8(buf).expect("invalid utf8");

        eprintln!("=== GENERATED AST ===");
        eprintln!("{:#?}", ast.blocks);
        eprintln!("\n=== QMD WRITER OUTPUT ===");
        eprintln!("{:?}", markdown);
        eprintln!("\n=== MARKDOWN (formatted) ===");
        eprintln!("{}", markdown);

        // Now test roundtrip
        debug_roundtrip(&markdown);
    }

    #[test]
    #[ignore = "complex nested autolinks"]
    fn debug_nested_autolinks_in_image() {
        // From test_full_roundtrip - triple nested links
        debug_roundtrip(
            "![[[[https://aaa.example.com](https://aaa.example.com)](https://aaa.example.com)](https://aaa.example.com)]\n",
        );
    }

    #[test]
    fn debug_bullet_list() {
        assert!(debug_roundtrip("- item one\n- item two\n"));
    }

    #[test]
    fn debug_blockquote() {
        assert!(debug_roundtrip("> quoted text\n"));
    }

    #[test]
    fn debug_hr() {
        assert!(debug_roundtrip("---\n"));
    }
}
