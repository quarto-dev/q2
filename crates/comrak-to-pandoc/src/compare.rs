/*
 * compare.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * AST comparison functions that ignore source location information.
 */

use quarto_pandoc_types::{Attr, Block, Inline, Pandoc};

/// Compare two Pandoc documents, ignoring source location information.
pub fn ast_eq_ignore_source(a: &Pandoc, b: &Pandoc) -> bool {
    blocks_eq(&a.blocks, &b.blocks)
    // Note: We ignore meta comparison for now as it's not relevant for CommonMark subset
}

/// Compare two block lists, ignoring source locations.
pub fn blocks_eq(a: &[Block], b: &[Block]) -> bool {
    a.len() == b.len() && a.iter().zip(b.iter()).all(|(x, y)| block_eq(x, y))
}

/// Compare two blocks, ignoring source locations.
pub fn block_eq(a: &Block, b: &Block) -> bool {
    match (a, b) {
        (Block::Plain(x), Block::Plain(y)) => inlines_eq(&x.content, &y.content),
        (Block::Paragraph(x), Block::Paragraph(y)) => inlines_eq(&x.content, &y.content),
        (Block::Header(x), Block::Header(y)) => {
            x.level == y.level && attr_eq(&x.attr, &y.attr) && inlines_eq(&x.content, &y.content)
        }
        (Block::CodeBlock(x), Block::CodeBlock(y)) => attr_eq(&x.attr, &y.attr) && x.text == y.text,
        (Block::BlockQuote(x), Block::BlockQuote(y)) => blocks_eq(&x.content, &y.content),
        (Block::BulletList(x), Block::BulletList(y)) => {
            x.content.len() == y.content.len()
                && x.content
                    .iter()
                    .zip(y.content.iter())
                    .all(|(a, b)| blocks_eq(a, b))
        }
        (Block::OrderedList(x), Block::OrderedList(y)) => {
            x.attr == y.attr // ListAttributes has no source info
                && x.content.len() == y.content.len()
                && x.content
                    .iter()
                    .zip(y.content.iter())
                    .all(|(a, b)| blocks_eq(a, b))
        }
        (Block::HorizontalRule(_), Block::HorizontalRule(_)) => true,
        (Block::Div(x), Block::Div(y)) => {
            attr_eq(&x.attr, &y.attr) && blocks_eq(&x.content, &y.content)
        }
        _ => false,
    }
}

/// Compare two inline lists, ignoring source locations.
pub fn inlines_eq(a: &[Inline], b: &[Inline]) -> bool {
    a.len() == b.len() && a.iter().zip(b.iter()).all(|(x, y)| inline_eq(x, y))
}

/// Compare two inlines, ignoring source locations.
pub fn inline_eq(a: &Inline, b: &Inline) -> bool {
    match (a, b) {
        (Inline::Str(x), Inline::Str(y)) => x.text == y.text,
        (Inline::Space(_), Inline::Space(_)) => true,
        (Inline::SoftBreak(_), Inline::SoftBreak(_)) => true,
        (Inline::LineBreak(_), Inline::LineBreak(_)) => true,
        (Inline::Emph(x), Inline::Emph(y)) => inlines_eq(&x.content, &y.content),
        (Inline::Strong(x), Inline::Strong(y)) => inlines_eq(&x.content, &y.content),
        (Inline::Strikeout(x), Inline::Strikeout(y)) => inlines_eq(&x.content, &y.content),
        (Inline::Superscript(x), Inline::Superscript(y)) => inlines_eq(&x.content, &y.content),
        (Inline::Subscript(x), Inline::Subscript(y)) => inlines_eq(&x.content, &y.content),
        (Inline::Code(x), Inline::Code(y)) => attr_eq(&x.attr, &y.attr) && x.text == y.text,
        (Inline::Link(x), Inline::Link(y)) => {
            attr_eq(&x.attr, &y.attr) && inlines_eq(&x.content, &y.content) && x.target == y.target
        }
        (Inline::Image(x), Inline::Image(y)) => {
            attr_eq(&x.attr, &y.attr) && inlines_eq(&x.content, &y.content) && x.target == y.target
        }
        (Inline::Span(x), Inline::Span(y)) => {
            attr_eq(&x.attr, &y.attr) && inlines_eq(&x.content, &y.content)
        }
        (Inline::Quoted(x), Inline::Quoted(y)) => {
            x.quote_type == y.quote_type && inlines_eq(&x.content, &y.content)
        }
        (Inline::Math(x), Inline::Math(y)) => x.math_type == y.math_type && x.text == y.text,
        (Inline::RawInline(x), Inline::RawInline(y)) => x.format == y.format && x.text == y.text,
        _ => false,
    }
}

/// Compare two attributes, ignoring source locations.
fn attr_eq(a: &Attr, b: &Attr) -> bool {
    a.0 == b.0 && a.1 == b.1 && a.2 == b.2
}

/// Pretty-print difference between two Pandoc documents for debugging.
#[allow(dead_code)]
pub fn diff_ast(a: &Pandoc, b: &Pandoc) -> String {
    let mut result = String::new();

    if a.blocks.len() != b.blocks.len() {
        result.push_str(&format!(
            "Block count differs: {} vs {}\n",
            a.blocks.len(),
            b.blocks.len()
        ));
    }

    for (i, (block_a, block_b)) in a.blocks.iter().zip(b.blocks.iter()).enumerate() {
        if !block_eq(block_a, block_b) {
            result.push_str(&format!("Block {} differs:\n", i));
            result.push_str(&format!("  A: {:?}\n", block_a));
            result.push_str(&format!("  B: {:?}\n", block_b));
        }
    }

    if result.is_empty() {
        "ASTs are equivalent".to_string()
    } else {
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_pandoc_types::{Emph, Header, Paragraph, Plain, Str};
    use quarto_source_map::{FileId, SourceInfo};

    fn dummy_source() -> SourceInfo {
        SourceInfo::original(FileId(0), 0, 0)
    }

    fn other_source() -> SourceInfo {
        SourceInfo::original(FileId(1), 100, 200)
    }

    #[test]
    fn test_str_eq_ignores_source() {
        let a = Inline::Str(Str {
            text: "hello".to_string(),
            source_info: dummy_source(),
        });
        let b = Inline::Str(Str {
            text: "hello".to_string(),
            source_info: other_source(),
        });
        assert!(inline_eq(&a, &b));
    }

    #[test]
    fn test_str_neq_different_text() {
        let a = Inline::Str(Str {
            text: "hello".to_string(),
            source_info: dummy_source(),
        });
        let b = Inline::Str(Str {
            text: "world".to_string(),
            source_info: dummy_source(),
        });
        assert!(!inline_eq(&a, &b));
    }

    #[test]
    fn test_emph_eq() {
        let a = Inline::Emph(Emph {
            content: vec![Inline::Str(Str {
                text: "hello".to_string(),
                source_info: dummy_source(),
            })],
            source_info: dummy_source(),
        });
        let b = Inline::Emph(Emph {
            content: vec![Inline::Str(Str {
                text: "hello".to_string(),
                source_info: other_source(),
            })],
            source_info: other_source(),
        });
        assert!(inline_eq(&a, &b));
    }

    #[test]
    fn test_paragraph_eq() {
        let a = Block::Paragraph(Paragraph {
            content: vec![Inline::Str(Str {
                text: "hello".to_string(),
                source_info: dummy_source(),
            })],
            source_info: dummy_source(),
        });
        let b = Block::Paragraph(Paragraph {
            content: vec![Inline::Str(Str {
                text: "hello".to_string(),
                source_info: other_source(),
            })],
            source_info: other_source(),
        });
        assert!(block_eq(&a, &b));
    }

    #[test]
    fn test_plain_vs_paragraph_not_equal() {
        let a = Block::Plain(Plain {
            content: vec![],
            source_info: dummy_source(),
        });
        let b = Block::Paragraph(Paragraph {
            content: vec![],
            source_info: dummy_source(),
        });
        assert!(!block_eq(&a, &b));
    }

    #[test]
    fn test_header_eq() {
        use hashlink::LinkedHashMap;

        let a = Block::Header(Header {
            level: 1,
            attr: (String::new(), vec![], LinkedHashMap::new()),
            content: vec![],
            source_info: dummy_source(),
            attr_source: quarto_pandoc_types::AttrSourceInfo::empty(),
        });
        let b = Block::Header(Header {
            level: 1,
            attr: (String::new(), vec![], LinkedHashMap::new()),
            content: vec![],
            source_info: other_source(),
            attr_source: quarto_pandoc_types::AttrSourceInfo::empty(),
        });
        assert!(block_eq(&a, &b));
    }

    #[test]
    fn test_header_neq_different_level() {
        use hashlink::LinkedHashMap;

        let a = Block::Header(Header {
            level: 1,
            attr: (String::new(), vec![], LinkedHashMap::new()),
            content: vec![],
            source_info: dummy_source(),
            attr_source: quarto_pandoc_types::AttrSourceInfo::empty(),
        });
        let b = Block::Header(Header {
            level: 2,
            attr: (String::new(), vec![], LinkedHashMap::new()),
            content: vec![],
            source_info: dummy_source(),
            attr_source: quarto_pandoc_types::AttrSourceInfo::empty(),
        });
        assert!(!block_eq(&a, &b));
    }

    #[test]
    fn test_space_eq() {
        let a = Inline::Space(quarto_pandoc_types::Space {
            source_info: dummy_source(),
        });
        let b = Inline::Space(quarto_pandoc_types::Space {
            source_info: other_source(),
        });
        assert!(inline_eq(&a, &b));
    }

    #[test]
    fn test_softbreak_eq() {
        let a = Inline::SoftBreak(quarto_pandoc_types::SoftBreak {
            source_info: dummy_source(),
        });
        let b = Inline::SoftBreak(quarto_pandoc_types::SoftBreak {
            source_info: other_source(),
        });
        assert!(inline_eq(&a, &b));
    }

    #[test]
    fn test_linebreak_eq() {
        let a = Inline::LineBreak(quarto_pandoc_types::LineBreak {
            source_info: dummy_source(),
        });
        let b = Inline::LineBreak(quarto_pandoc_types::LineBreak {
            source_info: other_source(),
        });
        assert!(inline_eq(&a, &b));
    }

    #[test]
    fn test_strong_eq() {
        let a = Inline::Strong(quarto_pandoc_types::Strong {
            content: vec![Inline::Str(Str {
                text: "bold".to_string(),
                source_info: dummy_source(),
            })],
            source_info: dummy_source(),
        });
        let b = Inline::Strong(quarto_pandoc_types::Strong {
            content: vec![Inline::Str(Str {
                text: "bold".to_string(),
                source_info: other_source(),
            })],
            source_info: other_source(),
        });
        assert!(inline_eq(&a, &b));
    }

    #[test]
    fn test_strikeout_eq() {
        let a = Inline::Strikeout(quarto_pandoc_types::Strikeout {
            content: vec![],
            source_info: dummy_source(),
        });
        let b = Inline::Strikeout(quarto_pandoc_types::Strikeout {
            content: vec![],
            source_info: other_source(),
        });
        assert!(inline_eq(&a, &b));
    }

    #[test]
    fn test_superscript_eq() {
        let a = Inline::Superscript(quarto_pandoc_types::Superscript {
            content: vec![],
            source_info: dummy_source(),
        });
        let b = Inline::Superscript(quarto_pandoc_types::Superscript {
            content: vec![],
            source_info: other_source(),
        });
        assert!(inline_eq(&a, &b));
    }

    #[test]
    fn test_subscript_eq() {
        let a = Inline::Subscript(quarto_pandoc_types::Subscript {
            content: vec![],
            source_info: dummy_source(),
        });
        let b = Inline::Subscript(quarto_pandoc_types::Subscript {
            content: vec![],
            source_info: other_source(),
        });
        assert!(inline_eq(&a, &b));
    }

    #[test]
    fn test_code_inline_eq() {
        use hashlink::LinkedHashMap;
        let a = Inline::Code(quarto_pandoc_types::Code {
            attr: (String::new(), vec![], LinkedHashMap::new()),
            text: "code".to_string(),
            source_info: dummy_source(),
            attr_source: quarto_pandoc_types::AttrSourceInfo::empty(),
        });
        let b = Inline::Code(quarto_pandoc_types::Code {
            attr: (String::new(), vec![], LinkedHashMap::new()),
            text: "code".to_string(),
            source_info: other_source(),
            attr_source: quarto_pandoc_types::AttrSourceInfo::empty(),
        });
        assert!(inline_eq(&a, &b));
    }

    #[test]
    fn test_link_eq() {
        use hashlink::LinkedHashMap;
        let a = Inline::Link(quarto_pandoc_types::Link {
            attr: (String::new(), vec![], LinkedHashMap::new()),
            content: vec![],
            target: ("url".to_string(), "title".to_string()),
            source_info: dummy_source(),
            attr_source: quarto_pandoc_types::AttrSourceInfo::empty(),
            target_source: quarto_pandoc_types::TargetSourceInfo::empty(),
        });
        let b = Inline::Link(quarto_pandoc_types::Link {
            attr: (String::new(), vec![], LinkedHashMap::new()),
            content: vec![],
            target: ("url".to_string(), "title".to_string()),
            source_info: other_source(),
            attr_source: quarto_pandoc_types::AttrSourceInfo::empty(),
            target_source: quarto_pandoc_types::TargetSourceInfo::empty(),
        });
        assert!(inline_eq(&a, &b));
    }

    #[test]
    fn test_image_eq() {
        use hashlink::LinkedHashMap;
        let a = Inline::Image(quarto_pandoc_types::Image {
            attr: (String::new(), vec![], LinkedHashMap::new()),
            content: vec![],
            target: ("img.png".to_string(), "alt".to_string()),
            source_info: dummy_source(),
            attr_source: quarto_pandoc_types::AttrSourceInfo::empty(),
            target_source: quarto_pandoc_types::TargetSourceInfo::empty(),
        });
        let b = Inline::Image(quarto_pandoc_types::Image {
            attr: (String::new(), vec![], LinkedHashMap::new()),
            content: vec![],
            target: ("img.png".to_string(), "alt".to_string()),
            source_info: other_source(),
            attr_source: quarto_pandoc_types::AttrSourceInfo::empty(),
            target_source: quarto_pandoc_types::TargetSourceInfo::empty(),
        });
        assert!(inline_eq(&a, &b));
    }

    #[test]
    fn test_span_eq() {
        use hashlink::LinkedHashMap;
        let a = Inline::Span(quarto_pandoc_types::Span {
            attr: (String::new(), vec![], LinkedHashMap::new()),
            content: vec![],
            source_info: dummy_source(),
            attr_source: quarto_pandoc_types::AttrSourceInfo::empty(),
        });
        let b = Inline::Span(quarto_pandoc_types::Span {
            attr: (String::new(), vec![], LinkedHashMap::new()),
            content: vec![],
            source_info: other_source(),
            attr_source: quarto_pandoc_types::AttrSourceInfo::empty(),
        });
        assert!(inline_eq(&a, &b));
    }

    #[test]
    fn test_quoted_eq() {
        let a = Inline::Quoted(quarto_pandoc_types::Quoted {
            quote_type: quarto_pandoc_types::QuoteType::DoubleQuote,
            content: vec![],
            source_info: dummy_source(),
        });
        let b = Inline::Quoted(quarto_pandoc_types::Quoted {
            quote_type: quarto_pandoc_types::QuoteType::DoubleQuote,
            content: vec![],
            source_info: other_source(),
        });
        assert!(inline_eq(&a, &b));
    }

    #[test]
    fn test_math_eq() {
        let a = Inline::Math(quarto_pandoc_types::Math {
            math_type: quarto_pandoc_types::MathType::InlineMath,
            text: "x^2".to_string(),
            source_info: dummy_source(),
        });
        let b = Inline::Math(quarto_pandoc_types::Math {
            math_type: quarto_pandoc_types::MathType::InlineMath,
            text: "x^2".to_string(),
            source_info: other_source(),
        });
        assert!(inline_eq(&a, &b));
    }

    #[test]
    fn test_rawinline_eq() {
        let a = Inline::RawInline(quarto_pandoc_types::RawInline {
            format: "html".to_string(),
            text: "<b>".to_string(),
            source_info: dummy_source(),
        });
        let b = Inline::RawInline(quarto_pandoc_types::RawInline {
            format: "html".to_string(),
            text: "<b>".to_string(),
            source_info: other_source(),
        });
        assert!(inline_eq(&a, &b));
    }

    #[test]
    fn test_inline_type_mismatch() {
        // Tests the catch-all `_ => false` case in inline_eq
        let a = Inline::Space(quarto_pandoc_types::Space {
            source_info: dummy_source(),
        });
        let b = Inline::Str(Str {
            text: "x".to_string(),
            source_info: dummy_source(),
        });
        assert!(!inline_eq(&a, &b));
    }

    #[test]
    fn test_codeblock_eq() {
        use hashlink::LinkedHashMap;
        let a = Block::CodeBlock(quarto_pandoc_types::CodeBlock {
            attr: (
                String::new(),
                vec!["python".to_string()],
                LinkedHashMap::new(),
            ),
            text: "print(1)".to_string(),
            source_info: dummy_source(),
            attr_source: quarto_pandoc_types::AttrSourceInfo::empty(),
        });
        let b = Block::CodeBlock(quarto_pandoc_types::CodeBlock {
            attr: (
                String::new(),
                vec!["python".to_string()],
                LinkedHashMap::new(),
            ),
            text: "print(1)".to_string(),
            source_info: other_source(),
            attr_source: quarto_pandoc_types::AttrSourceInfo::empty(),
        });
        assert!(block_eq(&a, &b));
    }

    #[test]
    fn test_blockquote_eq() {
        let a = Block::BlockQuote(quarto_pandoc_types::BlockQuote {
            content: vec![],
            source_info: dummy_source(),
        });
        let b = Block::BlockQuote(quarto_pandoc_types::BlockQuote {
            content: vec![],
            source_info: other_source(),
        });
        assert!(block_eq(&a, &b));
    }

    #[test]
    fn test_bulletlist_eq() {
        let a = Block::BulletList(quarto_pandoc_types::BulletList {
            content: vec![vec![]],
            source_info: dummy_source(),
        });
        let b = Block::BulletList(quarto_pandoc_types::BulletList {
            content: vec![vec![]],
            source_info: other_source(),
        });
        assert!(block_eq(&a, &b));
    }

    #[test]
    fn test_orderedlist_eq() {
        let a = Block::OrderedList(quarto_pandoc_types::OrderedList {
            attr: (
                1,
                quarto_pandoc_types::ListNumberStyle::Decimal,
                quarto_pandoc_types::ListNumberDelim::Period,
            ),
            content: vec![vec![]],
            source_info: dummy_source(),
        });
        let b = Block::OrderedList(quarto_pandoc_types::OrderedList {
            attr: (
                1,
                quarto_pandoc_types::ListNumberStyle::Decimal,
                quarto_pandoc_types::ListNumberDelim::Period,
            ),
            content: vec![vec![]],
            source_info: other_source(),
        });
        assert!(block_eq(&a, &b));
    }

    #[test]
    fn test_horizontalrule_eq() {
        let a = Block::HorizontalRule(quarto_pandoc_types::HorizontalRule {
            source_info: dummy_source(),
        });
        let b = Block::HorizontalRule(quarto_pandoc_types::HorizontalRule {
            source_info: other_source(),
        });
        assert!(block_eq(&a, &b));
    }

    #[test]
    fn test_div_eq() {
        use hashlink::LinkedHashMap;
        let a = Block::Div(quarto_pandoc_types::Div {
            attr: (String::new(), vec![], LinkedHashMap::new()),
            content: vec![],
            source_info: dummy_source(),
            attr_source: quarto_pandoc_types::AttrSourceInfo::empty(),
        });
        let b = Block::Div(quarto_pandoc_types::Div {
            attr: (String::new(), vec![], LinkedHashMap::new()),
            content: vec![],
            source_info: other_source(),
            attr_source: quarto_pandoc_types::AttrSourceInfo::empty(),
        });
        assert!(block_eq(&a, &b));
    }

    #[test]
    fn test_block_type_mismatch() {
        // Tests the catch-all `_ => false` case in block_eq
        let a = Block::HorizontalRule(quarto_pandoc_types::HorizontalRule {
            source_info: dummy_source(),
        });
        let b = Block::Paragraph(Paragraph {
            content: vec![],
            source_info: dummy_source(),
        });
        assert!(!block_eq(&a, &b));
    }

    #[test]
    fn test_ast_eq_ignore_source() {
        use quarto_pandoc_types::ConfigValue;
        let a = Pandoc {
            meta: ConfigValue::default(),
            blocks: vec![Block::Paragraph(Paragraph {
                content: vec![Inline::Str(Str {
                    text: "hello".to_string(),
                    source_info: dummy_source(),
                })],
                source_info: dummy_source(),
            })],
        };
        let b = Pandoc {
            meta: ConfigValue::default(),
            blocks: vec![Block::Paragraph(Paragraph {
                content: vec![Inline::Str(Str {
                    text: "hello".to_string(),
                    source_info: other_source(),
                })],
                source_info: other_source(),
            })],
        };
        assert!(ast_eq_ignore_source(&a, &b));
    }

    #[test]
    fn test_diff_ast_equal() {
        use quarto_pandoc_types::ConfigValue;
        let a = Pandoc {
            meta: ConfigValue::default(),
            blocks: vec![],
        };
        assert_eq!(diff_ast(&a, &a), "ASTs are equivalent");
    }

    #[test]
    fn test_diff_ast_block_count_differs() {
        use quarto_pandoc_types::ConfigValue;
        let a = Pandoc {
            meta: ConfigValue::default(),
            blocks: vec![Block::HorizontalRule(quarto_pandoc_types::HorizontalRule {
                source_info: dummy_source(),
            })],
        };
        let b = Pandoc {
            meta: ConfigValue::default(),
            blocks: vec![],
        };
        let diff = diff_ast(&a, &b);
        assert!(diff.contains("Block count differs: 1 vs 0"));
    }

    #[test]
    fn test_diff_ast_block_differs() {
        use quarto_pandoc_types::ConfigValue;
        let a = Pandoc {
            meta: ConfigValue::default(),
            blocks: vec![Block::Paragraph(Paragraph {
                content: vec![Inline::Str(Str {
                    text: "hello".to_string(),
                    source_info: dummy_source(),
                })],
                source_info: dummy_source(),
            })],
        };
        let b = Pandoc {
            meta: ConfigValue::default(),
            blocks: vec![Block::Paragraph(Paragraph {
                content: vec![Inline::Str(Str {
                    text: "world".to_string(),
                    source_info: dummy_source(),
                })],
                source_info: dummy_source(),
            })],
        };
        let diff = diff_ast(&a, &b);
        assert!(diff.contains("Block 0 differs"));
    }

    #[test]
    fn test_blocks_eq_length_mismatch() {
        let a = vec![Block::HorizontalRule(quarto_pandoc_types::HorizontalRule {
            source_info: dummy_source(),
        })];
        let b = vec![];
        assert!(!blocks_eq(&a, &b));
    }

    #[test]
    fn test_inlines_eq_length_mismatch() {
        let a = vec![Inline::Space(quarto_pandoc_types::Space {
            source_info: dummy_source(),
        })];
        let b = vec![];
        assert!(!inlines_eq(&a, &b));
    }
}
