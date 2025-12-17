/*
 * block.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Convert comrak block nodes to Pandoc blocks.
 */

use crate::inline::convert_children_to_inlines;
use crate::{empty_attr, empty_source_info};
use comrak::arena_tree::Node;
use comrak::nodes::{Ast, ListDelimType, ListType, NodeCodeBlock, NodeHeading, NodeList, NodeValue};
use hashlink::LinkedHashMap;
use quarto_pandoc_types::{
    AttrSourceInfo, Block, BlockQuote, Blocks, BulletList, CodeBlock, Header, HorizontalRule,
    ListAttributes, ListNumberDelim, ListNumberStyle, MetaValueWithSourceInfo, OrderedList, Pandoc,
    Paragraph, Plain,
};
use std::cell::RefCell;

/// Convert a comrak document to a Pandoc document.
///
/// # Panics
/// Panics if the AST contains nodes outside the CommonMark subset
/// (tables, strikethrough, footnotes, etc.)
pub fn convert_document<'a>(root: &'a Node<'a, RefCell<Ast>>) -> Pandoc {
    let ast = root.data.borrow();
    match &ast.value {
        NodeValue::Document => {
            let blocks = convert_children_to_blocks(root);
            Pandoc {
                meta: MetaValueWithSourceInfo::default(),
                blocks,
            }
        }
        _ => panic!("Expected Document node at root"),
    }
}

/// Convert a comrak node's block children to Pandoc blocks.
fn convert_children_to_blocks<'a>(node: &'a Node<'a, RefCell<Ast>>) -> Blocks {
    node.children()
        .flat_map(|child| convert_block(child))
        .collect()
}

/// Convert a comrak block node to Pandoc blocks.
///
/// Returns a Vec because some nodes may expand to multiple blocks.
fn convert_block<'a>(node: &'a Node<'a, RefCell<Ast>>) -> Blocks {
    let ast = node.data.borrow();

    match &ast.value {
        NodeValue::Document => {
            // Document is handled at the top level
            convert_children_to_blocks(node)
        }

        NodeValue::Paragraph => {
            let inlines = convert_children_to_inlines(node);
            vec![Block::Paragraph(Paragraph {
                content: inlines,
                source_info: empty_source_info(),
            })]
        }

        NodeValue::Heading(heading) => {
            vec![convert_heading(node, heading)]
        }

        NodeValue::CodeBlock(code_block) => {
            vec![convert_code_block(code_block)]
        }

        NodeValue::BlockQuote => {
            let children = convert_children_to_blocks(node);
            vec![Block::BlockQuote(BlockQuote {
                content: children,
                source_info: empty_source_info(),
            })]
        }

        NodeValue::List(list) => {
            vec![convert_list(node, list)]
        }

        NodeValue::Item(_) => {
            // Items are handled within convert_list
            panic!("Item should not be converted directly; use convert_list_item")
        }

        NodeValue::ThematicBreak => {
            vec![Block::HorizontalRule(HorizontalRule {
                source_info: empty_source_info(),
            })]
        }

        NodeValue::FrontMatter(_) => {
            // Skip front matter for now (it goes in Meta, not blocks)
            vec![]
        }

        // Unsupported block types - panic as they're outside CommonMark subset
        NodeValue::HtmlBlock(_) => {
            panic!("HtmlBlock not supported in CommonMark subset")
        }
        NodeValue::Table(_) => {
            panic!("Table (GFM extension) not supported in CommonMark subset")
        }
        NodeValue::TableRow(_) => {
            panic!("TableRow (GFM extension) not supported in CommonMark subset")
        }
        NodeValue::TableCell => {
            panic!("TableCell (GFM extension) not supported in CommonMark subset")
        }
        NodeValue::FootnoteDefinition(_) => {
            panic!("FootnoteDefinition not supported in CommonMark subset")
        }
        NodeValue::DescriptionList => {
            panic!("DescriptionList not supported in CommonMark subset")
        }
        NodeValue::DescriptionItem(_) => {
            panic!("DescriptionItem not supported in CommonMark subset")
        }
        NodeValue::DescriptionTerm => {
            panic!("DescriptionTerm not supported in CommonMark subset")
        }
        NodeValue::DescriptionDetails => {
            panic!("DescriptionDetails not supported in CommonMark subset")
        }
        NodeValue::TaskItem(_) => {
            panic!("TaskItem (GFM extension) not supported in CommonMark subset")
        }
        NodeValue::MultilineBlockQuote(_) => {
            panic!("MultilineBlockQuote not supported in CommonMark subset")
        }
        NodeValue::Alert(_) => {
            panic!("Alert (GitHub extension) not supported in CommonMark subset")
        }
        NodeValue::Subtext => {
            panic!("Subtext not supported in CommonMark subset")
        }

        // Inline nodes shouldn't appear in block context
        _ => {
            panic!(
                "Unexpected node type in block context: {:?}",
                std::mem::discriminant(&ast.value)
            )
        }
    }
}

fn convert_heading<'a>(node: &'a Node<'a, RefCell<Ast>>, heading: &NodeHeading) -> Block {
    if heading.setext {
        panic!("Setext headings not supported in CommonMark subset");
    }

    let inlines = convert_children_to_inlines(node);
    Block::Header(Header {
        level: heading.level as usize,
        attr: empty_attr(),
        content: inlines,
        source_info: empty_source_info(),
        attr_source: AttrSourceInfo::empty(),
    })
}

fn convert_code_block(code_block: &NodeCodeBlock) -> Block {
    if !code_block.fenced {
        panic!("Indented code blocks not supported in CommonMark subset");
    }

    let classes = if code_block.info.is_empty() {
        vec![]
    } else {
        // Info string is the language (may have additional metadata after space)
        // For CommonMark, just use the whole info string as language
        vec![code_block
            .info
            .split_whitespace()
            .next()
            .unwrap_or("")
            .to_string()]
    };

    Block::CodeBlock(CodeBlock {
        attr: (String::new(), classes, LinkedHashMap::new()),
        text: code_block.literal.clone(),
        source_info: empty_source_info(),
        attr_source: AttrSourceInfo::empty(),
    })
}

fn convert_list<'a>(node: &'a Node<'a, RefCell<Ast>>, list: &NodeList) -> Block {
    let items: Vec<Blocks> = node
        .children()
        .map(|child| convert_list_item(child, list.tight))
        .collect();

    match list.list_type {
        ListType::Bullet => Block::BulletList(BulletList {
            content: items,
            source_info: empty_source_info(),
        }),
        ListType::Ordered => {
            let attr: ListAttributes = (
                list.start,
                ListNumberStyle::Decimal,
                match list.delimiter {
                    ListDelimType::Period => ListNumberDelim::Period,
                    ListDelimType::Paren => ListNumberDelim::OneParen,
                },
            );
            Block::OrderedList(OrderedList {
                attr,
                content: items,
                source_info: empty_source_info(),
            })
        }
    }
}

fn convert_list_item<'a>(node: &'a Node<'a, RefCell<Ast>>, tight: bool) -> Blocks {
    let children = convert_children_to_blocks(node);

    if tight {
        // For tight lists, convert single Paragraph to Plain
        children
            .into_iter()
            .map(|block| match block {
                Block::Paragraph(Paragraph {
                    content,
                    source_info,
                }) => Block::Plain(Plain {
                    content,
                    source_info,
                }),
                other => other,
            })
            .collect()
    } else {
        children
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use comrak::{parse_document, Arena, Options};

    fn parse(markdown: &str) -> Pandoc {
        let arena = Arena::new();
        let root = parse_document(&arena, markdown, &Options::default());
        convert_document(root)
    }

    #[test]
    fn test_paragraph() {
        let pandoc = parse("Hello world.\n");
        assert_eq!(pandoc.blocks.len(), 1);
        assert!(matches!(pandoc.blocks[0], Block::Paragraph(_)));
    }

    #[test]
    fn test_multiple_paragraphs() {
        let pandoc = parse("First.\n\nSecond.\n");
        assert_eq!(pandoc.blocks.len(), 2);
        assert!(matches!(pandoc.blocks[0], Block::Paragraph(_)));
        assert!(matches!(pandoc.blocks[1], Block::Paragraph(_)));
    }

    #[test]
    fn test_heading_levels() {
        for level in 1..=6 {
            let md = format!("{} Heading\n", "#".repeat(level));
            let pandoc = parse(&md);
            assert_eq!(pandoc.blocks.len(), 1);
            match &pandoc.blocks[0] {
                Block::Header(h) => assert_eq!(h.level, level),
                _ => panic!("Expected Header"),
            }
        }
    }

    #[test]
    fn test_fenced_code_block() {
        let pandoc = parse("```python\nprint('hello')\n```\n");
        assert_eq!(pandoc.blocks.len(), 1);
        match &pandoc.blocks[0] {
            Block::CodeBlock(cb) => {
                assert_eq!(cb.attr.1, vec!["python".to_string()]);
                assert_eq!(cb.text, "print('hello')\n");
            }
            _ => panic!("Expected CodeBlock"),
        }
    }

    #[test]
    fn test_fenced_code_block_no_lang() {
        let pandoc = parse("```\ncode\n```\n");
        assert_eq!(pandoc.blocks.len(), 1);
        match &pandoc.blocks[0] {
            Block::CodeBlock(cb) => {
                assert!(cb.attr.1.is_empty());
            }
            _ => panic!("Expected CodeBlock"),
        }
    }

    #[test]
    fn test_blockquote() {
        let pandoc = parse("> Quote\n");
        assert_eq!(pandoc.blocks.len(), 1);
        match &pandoc.blocks[0] {
            Block::BlockQuote(bq) => {
                assert_eq!(bq.content.len(), 1);
            }
            _ => panic!("Expected BlockQuote"),
        }
    }

    #[test]
    fn test_bullet_list_tight() {
        let pandoc = parse("- one\n- two\n- three\n");
        assert_eq!(pandoc.blocks.len(), 1);
        match &pandoc.blocks[0] {
            Block::BulletList(bl) => {
                assert_eq!(bl.content.len(), 3);
                // Tight list items should be Plain, not Paragraph
                for item in &bl.content {
                    assert_eq!(item.len(), 1);
                    assert!(matches!(item[0], Block::Plain(_)));
                }
            }
            _ => panic!("Expected BulletList"),
        }
    }

    #[test]
    fn test_bullet_list_loose() {
        let pandoc = parse("- one\n\n- two\n\n- three\n");
        assert_eq!(pandoc.blocks.len(), 1);
        match &pandoc.blocks[0] {
            Block::BulletList(bl) => {
                assert_eq!(bl.content.len(), 3);
                // Loose list items should be Paragraph
                for item in &bl.content {
                    assert_eq!(item.len(), 1);
                    assert!(matches!(item[0], Block::Paragraph(_)));
                }
            }
            _ => panic!("Expected BulletList"),
        }
    }

    #[test]
    fn test_ordered_list() {
        let pandoc = parse("1. one\n2. two\n3. three\n");
        assert_eq!(pandoc.blocks.len(), 1);
        match &pandoc.blocks[0] {
            Block::OrderedList(ol) => {
                assert_eq!(ol.content.len(), 3);
                assert_eq!(ol.attr.0, 1); // Start at 1
            }
            _ => panic!("Expected OrderedList"),
        }
    }

    #[test]
    fn test_ordered_list_start() {
        let pandoc = parse("5. five\n6. six\n");
        assert_eq!(pandoc.blocks.len(), 1);
        match &pandoc.blocks[0] {
            Block::OrderedList(ol) => {
                assert_eq!(ol.attr.0, 5); // Start at 5
            }
            _ => panic!("Expected OrderedList"),
        }
    }

    #[test]
    fn test_horizontal_rule() {
        let pandoc = parse("---\n");
        assert_eq!(pandoc.blocks.len(), 1);
        assert!(matches!(pandoc.blocks[0], Block::HorizontalRule(_)));
    }

    #[test]
    fn test_nested_blockquote() {
        let pandoc = parse("> outer\n>\n> > inner\n");
        assert_eq!(pandoc.blocks.len(), 1);
        match &pandoc.blocks[0] {
            Block::BlockQuote(outer) => {
                // Should contain paragraph and nested blockquote
                assert!(outer.content.len() >= 2);
            }
            _ => panic!("Expected BlockQuote"),
        }
    }
}
