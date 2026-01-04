/*
 * normalize.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * AST normalization for property testing.
 *
 * This module handles known differences between pampa and comrak output
 * by normalizing both ASTs to a common form before comparison.
 */

use quarto_pandoc_types::{Block, Figure, Inline, Pandoc, Paragraph, attr::AttrSourceInfo};

/// Normalize a Pandoc AST for comparison.
///
/// This handles known differences between pampa and comrak:
/// 1. Strip heading IDs (pampa generates, comrak doesn't)
/// 2. Figure → Paragraph(Image) (pampa wraps standalone images)
/// 3. Strip autolink `uri` class from Link attrs
/// 4. Normalize code block attributes
pub fn normalize(ast: Pandoc) -> Pandoc {
    Pandoc {
        blocks: ast.blocks.into_iter().map(normalize_block).collect(),
        ..ast
    }
}

/// Normalize a single block.
fn normalize_block(block: Block) -> Block {
    match block {
        // Strip heading IDs and normalize content
        Block::Header(mut h) => {
            // Clear the ID (first element of attr tuple)
            h.attr.0 = String::new();
            h.content = normalize_inlines(h.content);
            // Strip leading/trailing spaces from header content
            // (pampa includes space after ATX markers as content, comrak doesn't)
            h.content = strip_leading_trailing_spaces(h.content);
            Block::Header(h)
        }

        // Figure → Paragraph(Image) for standalone images
        Block::Figure(fig) => normalize_figure(fig),

        // Normalize code block attributes (keep only language class) and text
        Block::CodeBlock(mut cb) => {
            // Keep only the first class (language) if present, clear everything else
            let lang_class = if cb.attr.1.is_empty() {
                vec![]
            } else {
                vec![cb.attr.1[0].clone()]
            };
            cb.attr = (String::new(), lang_class, Default::default());
            // Strip trailing newline from code block text
            // (comrak includes trailing newline, pampa doesn't)
            cb.text = cb.text.trim_end_matches('\n').to_string();
            Block::CodeBlock(cb)
        }

        // Recurse into container blocks
        Block::Paragraph(mut p) => {
            p.content = normalize_inlines(p.content);
            Block::Paragraph(p)
        }

        Block::Plain(mut p) => {
            p.content = normalize_inlines(p.content);
            Block::Plain(p)
        }

        Block::BlockQuote(mut bq) => {
            bq.content = bq.content.into_iter().map(normalize_block).collect();
            Block::BlockQuote(bq)
        }

        Block::BulletList(mut bl) => {
            bl.content = bl
                .content
                .into_iter()
                .map(|item| item.into_iter().map(normalize_block).collect())
                .collect();
            Block::BulletList(bl)
        }

        Block::OrderedList(mut ol) => {
            ol.content = ol
                .content
                .into_iter()
                .map(|item| item.into_iter().map(normalize_block).collect())
                .collect();
            Block::OrderedList(ol)
        }

        // Pass through other blocks unchanged
        other => other,
    }
}

/// Normalize a Figure block.
///
/// pampa wraps standalone images in Figure blocks; comrak keeps them
/// as Image inlines in Paragraph blocks. We normalize Figure(Plain(Image))
/// to Paragraph(Image).
fn normalize_figure(fig: Figure) -> Block {
    // Check if this is a standalone image figure:
    // Figure containing a single Plain/Para with a single Image
    if fig.content.len() == 1 {
        match &fig.content[0] {
            Block::Plain(plain) if is_single_image(&plain.content) => {
                // Unwrap to Paragraph(Image)
                return Block::Paragraph(Paragraph {
                    content: normalize_inlines(plain.content.clone()),
                    source_info: fig.source_info,
                });
            }
            Block::Paragraph(para) if is_single_image(&para.content) => {
                // Already Paragraph, just normalize inlines
                return Block::Paragraph(Paragraph {
                    content: normalize_inlines(para.content.clone()),
                    source_info: fig.source_info,
                });
            }
            _ => {}
        }
    }

    // Not a simple standalone image figure, recurse normally
    Block::Figure(Figure {
        content: fig.content.into_iter().map(normalize_block).collect(),
        caption: fig.caption, // TODO: normalize caption if needed
        attr: fig.attr,
        source_info: fig.source_info,
        attr_source: AttrSourceInfo::empty(),
    })
}

/// Check if inlines contain a single Image.
fn is_single_image(inlines: &[Inline]) -> bool {
    inlines.len() == 1 && matches!(inlines[0], Inline::Image(_))
}

/// Normalize a sequence of inlines, flattening empty Spans.
fn normalize_inlines(inlines: Vec<Inline>) -> Vec<Inline> {
    inlines
        .into_iter()
        .flat_map(normalize_inline_to_vec)
        .collect()
}

/// Normalize a single inline, potentially producing multiple inlines.
///
/// This handles Span unwrapping: pampa wraps certain content in Span elements
/// with empty attributes, while comrak doesn't. We unwrap such Spans.
fn normalize_inline_to_vec(inline: Inline) -> Vec<Inline> {
    match inline {
        // Unwrap empty-attribute Spans
        Inline::Span(span) if is_empty_attr(&span.attr) => {
            // Recursively normalize the span's content
            normalize_inlines(span.content)
        }

        // Strip uri class from autolinks
        Inline::Link(mut link) => {
            // Remove "uri" class if present
            link.attr.1.retain(|c| c != "uri");
            link.content = normalize_inlines(link.content);
            vec![Inline::Link(link)]
        }

        // Recurse into container inlines
        Inline::Emph(mut e) => {
            e.content = normalize_inlines(e.content);
            vec![Inline::Emph(e)]
        }

        Inline::Strong(mut s) => {
            s.content = normalize_inlines(s.content);
            vec![Inline::Strong(s)]
        }

        Inline::Image(mut img) => {
            img.content = normalize_inlines(img.content);
            vec![Inline::Image(img)]
        }

        // Pass through other inlines unchanged
        other => vec![other],
    }
}

/// Check if an attribute tuple is empty (no id, no classes, no attributes).
fn is_empty_attr(attr: &quarto_pandoc_types::Attr) -> bool {
    attr.0.is_empty() && attr.1.is_empty() && attr.2.is_empty()
}

/// Strip leading and trailing Space inlines from a sequence.
fn strip_leading_trailing_spaces(mut inlines: Vec<Inline>) -> Vec<Inline> {
    // Strip leading spaces
    while !inlines.is_empty() && matches!(inlines.first(), Some(Inline::Space(_))) {
        inlines.remove(0);
    }
    // Strip trailing spaces
    while !inlines.is_empty() && matches!(inlines.last(), Some(Inline::Space(_))) {
        inlines.pop();
    }
    inlines
}

#[cfg(test)]
mod tests {
    use super::*;
    use hashlink::LinkedHashMap;
    use quarto_pandoc_types::{ConfigValue, Header, Link, attr::TargetSourceInfo};
    use quarto_source_map::{FileId, SourceInfo};

    fn empty_source_info() -> SourceInfo {
        SourceInfo::original(FileId(0), 0, 0)
    }

    #[test]
    fn test_normalize_strips_heading_id() {
        let ast = Pandoc {
            meta: ConfigValue::default(),
            blocks: vec![Block::Header(Header {
                level: 1,
                content: vec![],
                attr: ("my-id".to_string(), vec![], LinkedHashMap::new()),
                source_info: empty_source_info(),
                attr_source: AttrSourceInfo::empty(),
            })],
        };

        let normalized = normalize(ast);
        match &normalized.blocks[0] {
            Block::Header(h) => assert_eq!(h.attr.0, ""),
            _ => panic!("Expected Header"),
        }
    }

    #[test]
    fn test_normalize_strips_uri_class() {
        let ast = Pandoc {
            meta: ConfigValue::default(),
            blocks: vec![Block::Paragraph(Paragraph {
                content: vec![Inline::Link(Link {
                    content: vec![],
                    target: ("http://example.com".to_string(), String::new()),
                    attr: (String::new(), vec!["uri".to_string()], LinkedHashMap::new()),
                    source_info: empty_source_info(),
                    attr_source: AttrSourceInfo::empty(),
                    target_source: TargetSourceInfo::empty(),
                })],
                source_info: empty_source_info(),
            })],
        };

        let normalized = normalize(ast);
        match &normalized.blocks[0] {
            Block::Paragraph(p) => match &p.content[0] {
                Inline::Link(link) => assert!(link.attr.1.is_empty()),
                _ => panic!("Expected Link"),
            },
            _ => panic!("Expected Paragraph"),
        }
    }

    #[test]
    fn test_normalize_codeblock_strips_trailing_newline() {
        use quarto_pandoc_types::CodeBlock;
        let ast = Pandoc {
            meta: ConfigValue::default(),
            blocks: vec![Block::CodeBlock(CodeBlock {
                attr: (
                    "id".to_string(),
                    vec!["python".to_string(), "extra".to_string()],
                    LinkedHashMap::new(),
                ),
                text: "code\n".to_string(),
                source_info: empty_source_info(),
                attr_source: AttrSourceInfo::empty(),
            })],
        };

        let normalized = normalize(ast);
        match &normalized.blocks[0] {
            Block::CodeBlock(cb) => {
                // ID should be cleared
                assert_eq!(cb.attr.0, "");
                // Only first class (language) should remain
                assert_eq!(cb.attr.1, vec!["python".to_string()]);
                // Trailing newline should be stripped
                assert_eq!(cb.text, "code");
            }
            _ => panic!("Expected CodeBlock"),
        }
    }

    #[test]
    fn test_normalize_codeblock_empty_classes() {
        use quarto_pandoc_types::CodeBlock;
        let ast = Pandoc {
            meta: ConfigValue::default(),
            blocks: vec![Block::CodeBlock(CodeBlock {
                attr: (String::new(), vec![], LinkedHashMap::new()),
                text: "code".to_string(),
                source_info: empty_source_info(),
                attr_source: AttrSourceInfo::empty(),
            })],
        };

        let normalized = normalize(ast);
        match &normalized.blocks[0] {
            Block::CodeBlock(cb) => {
                assert!(cb.attr.1.is_empty());
            }
            _ => panic!("Expected CodeBlock"),
        }
    }

    #[test]
    fn test_normalize_plain_block() {
        use quarto_pandoc_types::{Plain, Str};
        let ast = Pandoc {
            meta: ConfigValue::default(),
            blocks: vec![Block::Plain(Plain {
                content: vec![Inline::Str(Str {
                    text: "hello".to_string(),
                    source_info: empty_source_info(),
                })],
                source_info: empty_source_info(),
            })],
        };

        let normalized = normalize(ast);
        assert!(matches!(&normalized.blocks[0], Block::Plain(_)));
    }

    #[test]
    fn test_normalize_blockquote() {
        use quarto_pandoc_types::BlockQuote;
        let ast = Pandoc {
            meta: ConfigValue::default(),
            blocks: vec![Block::BlockQuote(BlockQuote {
                content: vec![Block::Paragraph(Paragraph {
                    content: vec![],
                    source_info: empty_source_info(),
                })],
                source_info: empty_source_info(),
            })],
        };

        let normalized = normalize(ast);
        assert!(matches!(&normalized.blocks[0], Block::BlockQuote(_)));
    }

    #[test]
    fn test_normalize_bulletlist() {
        use quarto_pandoc_types::BulletList;
        let ast = Pandoc {
            meta: ConfigValue::default(),
            blocks: vec![Block::BulletList(BulletList {
                content: vec![vec![Block::Paragraph(Paragraph {
                    content: vec![],
                    source_info: empty_source_info(),
                })]],
                source_info: empty_source_info(),
            })],
        };

        let normalized = normalize(ast);
        assert!(matches!(&normalized.blocks[0], Block::BulletList(_)));
    }

    #[test]
    fn test_normalize_orderedlist() {
        use quarto_pandoc_types::{ListNumberDelim, ListNumberStyle, OrderedList};
        let ast = Pandoc {
            meta: ConfigValue::default(),
            blocks: vec![Block::OrderedList(OrderedList {
                attr: (1, ListNumberStyle::Decimal, ListNumberDelim::Period),
                content: vec![vec![Block::Paragraph(Paragraph {
                    content: vec![],
                    source_info: empty_source_info(),
                })]],
                source_info: empty_source_info(),
            })],
        };

        let normalized = normalize(ast);
        assert!(matches!(&normalized.blocks[0], Block::OrderedList(_)));
    }

    #[test]
    fn test_normalize_other_blocks_passthrough() {
        use quarto_pandoc_types::HorizontalRule;
        let ast = Pandoc {
            meta: ConfigValue::default(),
            blocks: vec![Block::HorizontalRule(HorizontalRule {
                source_info: empty_source_info(),
            })],
        };

        let normalized = normalize(ast);
        assert!(matches!(&normalized.blocks[0], Block::HorizontalRule(_)));
    }

    #[test]
    fn test_normalize_emph_inline() {
        use quarto_pandoc_types::{Emph, Str};
        let ast = Pandoc {
            meta: ConfigValue::default(),
            blocks: vec![Block::Paragraph(Paragraph {
                content: vec![Inline::Emph(Emph {
                    content: vec![Inline::Str(Str {
                        text: "hello".to_string(),
                        source_info: empty_source_info(),
                    })],
                    source_info: empty_source_info(),
                })],
                source_info: empty_source_info(),
            })],
        };

        let normalized = normalize(ast);
        match &normalized.blocks[0] {
            Block::Paragraph(p) => assert!(matches!(&p.content[0], Inline::Emph(_))),
            _ => panic!("Expected Paragraph"),
        }
    }

    #[test]
    fn test_normalize_strong_inline() {
        use quarto_pandoc_types::{Str, Strong};
        let ast = Pandoc {
            meta: ConfigValue::default(),
            blocks: vec![Block::Paragraph(Paragraph {
                content: vec![Inline::Strong(Strong {
                    content: vec![Inline::Str(Str {
                        text: "hello".to_string(),
                        source_info: empty_source_info(),
                    })],
                    source_info: empty_source_info(),
                })],
                source_info: empty_source_info(),
            })],
        };

        let normalized = normalize(ast);
        match &normalized.blocks[0] {
            Block::Paragraph(p) => assert!(matches!(&p.content[0], Inline::Strong(_))),
            _ => panic!("Expected Paragraph"),
        }
    }

    #[test]
    fn test_normalize_image_inline() {
        use quarto_pandoc_types::Image;
        let ast = Pandoc {
            meta: ConfigValue::default(),
            blocks: vec![Block::Paragraph(Paragraph {
                content: vec![Inline::Image(Image {
                    content: vec![],
                    target: ("img.png".to_string(), String::new()),
                    attr: (String::new(), vec![], LinkedHashMap::new()),
                    source_info: empty_source_info(),
                    attr_source: AttrSourceInfo::empty(),
                    target_source: TargetSourceInfo::empty(),
                })],
                source_info: empty_source_info(),
            })],
        };

        let normalized = normalize(ast);
        match &normalized.blocks[0] {
            Block::Paragraph(p) => assert!(matches!(&p.content[0], Inline::Image(_))),
            _ => panic!("Expected Paragraph"),
        }
    }

    #[test]
    fn test_normalize_span_unwrap() {
        use quarto_pandoc_types::{Span, Str};
        // Empty-attr Span should be unwrapped
        let ast = Pandoc {
            meta: ConfigValue::default(),
            blocks: vec![Block::Paragraph(Paragraph {
                content: vec![Inline::Span(Span {
                    content: vec![Inline::Str(Str {
                        text: "hello".to_string(),
                        source_info: empty_source_info(),
                    })],
                    attr: (String::new(), vec![], LinkedHashMap::new()),
                    source_info: empty_source_info(),
                    attr_source: AttrSourceInfo::empty(),
                })],
                source_info: empty_source_info(),
            })],
        };

        let normalized = normalize(ast);
        match &normalized.blocks[0] {
            Block::Paragraph(p) => {
                // Span should have been unwrapped to just Str
                assert!(matches!(&p.content[0], Inline::Str(_)));
            }
            _ => panic!("Expected Paragraph"),
        }
    }

    #[test]
    fn test_normalize_other_inlines_passthrough() {
        use quarto_pandoc_types::Space;
        let ast = Pandoc {
            meta: ConfigValue::default(),
            blocks: vec![Block::Paragraph(Paragraph {
                content: vec![Inline::Space(Space {
                    source_info: empty_source_info(),
                })],
                source_info: empty_source_info(),
            })],
        };

        let normalized = normalize(ast);
        match &normalized.blocks[0] {
            Block::Paragraph(p) => assert!(matches!(&p.content[0], Inline::Space(_))),
            _ => panic!("Expected Paragraph"),
        }
    }

    #[test]
    fn test_strip_leading_trailing_spaces() {
        use quarto_pandoc_types::{Space, Str};
        let inlines = vec![
            Inline::Space(Space {
                source_info: empty_source_info(),
            }),
            Inline::Str(Str {
                text: "hello".to_string(),
                source_info: empty_source_info(),
            }),
            Inline::Space(Space {
                source_info: empty_source_info(),
            }),
        ];

        let result = strip_leading_trailing_spaces(inlines);
        assert_eq!(result.len(), 1);
        assert!(matches!(&result[0], Inline::Str(_)));
    }

    #[test]
    fn test_figure_plain_image_normalization() {
        use quarto_pandoc_types::{Caption, Image, Plain};
        // Figure containing Plain(Image) should become Paragraph(Image)
        let ast = Pandoc {
            meta: ConfigValue::default(),
            blocks: vec![Block::Figure(Figure {
                content: vec![Block::Plain(Plain {
                    content: vec![Inline::Image(Image {
                        content: vec![],
                        target: ("img.png".to_string(), String::new()),
                        attr: (String::new(), vec![], LinkedHashMap::new()),
                        source_info: empty_source_info(),
                        attr_source: AttrSourceInfo::empty(),
                        target_source: TargetSourceInfo::empty(),
                    })],
                    source_info: empty_source_info(),
                })],
                caption: Caption {
                    short: None,
                    long: None,
                    source_info: empty_source_info(),
                },
                attr: (String::new(), vec![], LinkedHashMap::new()),
                source_info: empty_source_info(),
                attr_source: AttrSourceInfo::empty(),
            })],
        };

        let normalized = normalize(ast);
        // Should be converted to Paragraph
        assert!(matches!(&normalized.blocks[0], Block::Paragraph(_)));
    }

    #[test]
    fn test_figure_para_image_normalization() {
        use quarto_pandoc_types::{Caption, Image};
        // Figure containing Paragraph(Image) should stay Paragraph(Image)
        let ast = Pandoc {
            meta: ConfigValue::default(),
            blocks: vec![Block::Figure(Figure {
                content: vec![Block::Paragraph(Paragraph {
                    content: vec![Inline::Image(Image {
                        content: vec![],
                        target: ("img.png".to_string(), String::new()),
                        attr: (String::new(), vec![], LinkedHashMap::new()),
                        source_info: empty_source_info(),
                        attr_source: AttrSourceInfo::empty(),
                        target_source: TargetSourceInfo::empty(),
                    })],
                    source_info: empty_source_info(),
                })],
                caption: Caption {
                    short: None,
                    long: None,
                    source_info: empty_source_info(),
                },
                attr: (String::new(), vec![], LinkedHashMap::new()),
                source_info: empty_source_info(),
                attr_source: AttrSourceInfo::empty(),
            })],
        };

        let normalized = normalize(ast);
        assert!(matches!(&normalized.blocks[0], Block::Paragraph(_)));
    }

    #[test]
    fn test_figure_complex_stays_figure() {
        use quarto_pandoc_types::{Caption, Str};
        // Figure with multiple blocks should stay as Figure
        let ast = Pandoc {
            meta: ConfigValue::default(),
            blocks: vec![Block::Figure(Figure {
                content: vec![
                    Block::Paragraph(Paragraph {
                        content: vec![Inline::Str(Str {
                            text: "hello".to_string(),
                            source_info: empty_source_info(),
                        })],
                        source_info: empty_source_info(),
                    }),
                    Block::Paragraph(Paragraph {
                        content: vec![],
                        source_info: empty_source_info(),
                    }),
                ],
                caption: Caption {
                    short: None,
                    long: None,
                    source_info: empty_source_info(),
                },
                attr: (String::new(), vec![], LinkedHashMap::new()),
                source_info: empty_source_info(),
                attr_source: AttrSourceInfo::empty(),
            })],
        };

        let normalized = normalize(ast);
        assert!(matches!(&normalized.blocks[0], Block::Figure(_)));
    }

    #[test]
    fn test_is_empty_attr() {
        // Empty attr
        let empty = (String::new(), vec![], LinkedHashMap::new());
        assert!(is_empty_attr(&empty));

        // Non-empty id
        let with_id: quarto_pandoc_types::Attr = ("id".to_string(), vec![], LinkedHashMap::new());
        assert!(!is_empty_attr(&with_id));

        // Non-empty class
        let with_class: quarto_pandoc_types::Attr = (
            String::new(),
            vec!["class".to_string()],
            LinkedHashMap::new(),
        );
        assert!(!is_empty_attr(&with_class));
    }

    #[test]
    fn test_is_single_image() {
        use quarto_pandoc_types::{Image, Str};

        // Single image
        let single_image = vec![Inline::Image(Image {
            content: vec![],
            target: ("img.png".to_string(), String::new()),
            attr: (String::new(), vec![], LinkedHashMap::new()),
            source_info: empty_source_info(),
            attr_source: AttrSourceInfo::empty(),
            target_source: TargetSourceInfo::empty(),
        })];
        assert!(is_single_image(&single_image));

        // Multiple inlines
        let multiple = vec![
            Inline::Str(Str {
                text: "hello".to_string(),
                source_info: empty_source_info(),
            }),
            Inline::Image(Image {
                content: vec![],
                target: ("img.png".to_string(), String::new()),
                attr: (String::new(), vec![], LinkedHashMap::new()),
                source_info: empty_source_info(),
                attr_source: AttrSourceInfo::empty(),
                target_source: TargetSourceInfo::empty(),
            }),
        ];
        assert!(!is_single_image(&multiple));

        // Not an image
        let not_image = vec![Inline::Str(Str {
            text: "hello".to_string(),
            source_info: empty_source_info(),
        })];
        assert!(!is_single_image(&not_image));
    }
}
