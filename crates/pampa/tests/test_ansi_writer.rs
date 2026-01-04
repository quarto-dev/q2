//! Tests for the ANSI writer.
//!
//! These tests exercise the ANSI writer's output for various block and inline types,
//! as well as color parsing and configuration options.

use hashlink::LinkedHashMap;
use pampa::pandoc::{
    AttrSourceInfo, Block, BlockQuote, BulletList, Code, DefinitionList, Div, Emph, Header,
    HorizontalRule, Image, Inline, LineBreak, Link, Math, Note, OrderedList, Pandoc, Paragraph,
    Plain, Quoted, RawBlock, RawInline, SmallCaps, SoftBreak, Space, Span, Str, Strikeout, Strong,
    Subscript, Superscript, TargetSourceInfo, Underline,
};
use pampa::writers;
use quarto_source_map::SourceInfo;

fn empty_source() -> SourceInfo {
    SourceInfo::default()
}

fn empty_attr() -> (String, Vec<String>, LinkedHashMap<String, String>) {
    (String::new(), vec![], LinkedHashMap::new())
}

fn empty_attr_source() -> AttrSourceInfo {
    AttrSourceInfo::empty()
}

/// Write a Pandoc document to ANSI and return the output as a string
fn write_ansi(pandoc: &Pandoc) -> String {
    let mut buf = Vec::new();
    writers::ansi::write(pandoc, &mut buf).expect("Failed to write ANSI");
    String::from_utf8(buf).expect("Invalid UTF-8")
}

// ============================================================================
// Basic Block Tests
// ============================================================================

#[test]
fn test_plain_block() {
    let pandoc = Pandoc {
        meta: Default::default(),
        blocks: vec![Block::Plain(Plain {
            content: vec![Inline::Str(Str {
                text: "Hello world".to_string(),
                source_info: empty_source(),
            })],
            source_info: empty_source(),
        })],
    };

    let output = write_ansi(&pandoc);
    assert!(output.contains("Hello world"));
    assert!(output.ends_with("\n"));
}

#[test]
fn test_paragraph_block() {
    let pandoc = Pandoc {
        meta: Default::default(),
        blocks: vec![Block::Paragraph(Paragraph {
            content: vec![Inline::Str(Str {
                text: "Paragraph content".to_string(),
                source_info: empty_source(),
            })],
            source_info: empty_source(),
        })],
    };

    let output = write_ansi(&pandoc);
    assert!(output.contains("Paragraph content"));
}

#[test]
fn test_consecutive_plain_blocks() {
    let pandoc = Pandoc {
        meta: Default::default(),
        blocks: vec![
            Block::Plain(Plain {
                content: vec![Inline::Str(Str {
                    text: "First".to_string(),
                    source_info: empty_source(),
                })],
                source_info: empty_source(),
            }),
            Block::Plain(Plain {
                content: vec![Inline::Str(Str {
                    text: "Second".to_string(),
                    source_info: empty_source(),
                })],
                source_info: empty_source(),
            }),
        ],
    };

    let output = write_ansi(&pandoc);
    // Consecutive Plains should have single newline between them (no blank line)
    assert!(output.contains("First\nSecond"));
}

#[test]
fn test_paragraph_spacing() {
    let pandoc = Pandoc {
        meta: Default::default(),
        blocks: vec![
            Block::Paragraph(Paragraph {
                content: vec![Inline::Str(Str {
                    text: "Para 1".to_string(),
                    source_info: empty_source(),
                })],
                source_info: empty_source(),
            }),
            Block::Paragraph(Paragraph {
                content: vec![Inline::Str(Str {
                    text: "Para 2".to_string(),
                    source_info: empty_source(),
                })],
                source_info: empty_source(),
            }),
        ],
    };

    let output = write_ansi(&pandoc);
    // Paragraphs should have blank line between them
    assert!(output.contains("Para 1\n\nPara 2"));
}

// ============================================================================
// Header Tests
// ============================================================================

#[test]
fn test_header_level_1() {
    let pandoc = Pandoc {
        meta: Default::default(),
        blocks: vec![Block::Header(Header {
            level: 1,
            attr: empty_attr(),
            content: vec![Inline::Str(Str {
                text: "Title".to_string(),
                source_info: empty_source(),
            })],
            source_info: empty_source(),
            attr_source: empty_attr_source(),
        })],
    };

    let output = write_ansi(&pandoc);
    assert!(output.contains("Title"));
}

#[test]
fn test_header_level_2() {
    let pandoc = Pandoc {
        meta: Default::default(),
        blocks: vec![Block::Header(Header {
            level: 2,
            attr: empty_attr(),
            content: vec![Inline::Str(Str {
                text: "Section".to_string(),
                source_info: empty_source(),
            })],
            source_info: empty_source(),
            attr_source: empty_attr_source(),
        })],
    };

    let output = write_ansi(&pandoc);
    assert!(output.contains("Section"));
}

#[test]
fn test_header_level_3_to_6() {
    for level in 3..=6 {
        let pandoc = Pandoc {
            meta: Default::default(),
            blocks: vec![Block::Header(Header {
                level,
                attr: empty_attr(),
                content: vec![Inline::Str(Str {
                    text: format!("Header {}", level),
                    source_info: empty_source(),
                })],
                source_info: empty_source(),
                attr_source: empty_attr_source(),
            })],
        };

        let output = write_ansi(&pandoc);
        assert!(output.contains(&format!("Header {}", level)));
    }
}

// ============================================================================
// Horizontal Rule Test
// ============================================================================

#[test]
fn test_horizontal_rule() {
    let pandoc = Pandoc {
        meta: Default::default(),
        blocks: vec![Block::HorizontalRule(HorizontalRule {
            source_info: empty_source(),
        })],
    };

    let output = write_ansi(&pandoc);
    // Should contain horizontal line characters
    assert!(output.contains("─") || output.contains("-"));
}

// ============================================================================
// List Tests
// ============================================================================

#[test]
fn test_bullet_list() {
    let pandoc = Pandoc {
        meta: Default::default(),
        blocks: vec![Block::BulletList(BulletList {
            content: vec![
                vec![Block::Plain(Plain {
                    content: vec![Inline::Str(Str {
                        text: "Item 1".to_string(),
                        source_info: empty_source(),
                    })],
                    source_info: empty_source(),
                })],
                vec![Block::Plain(Plain {
                    content: vec![Inline::Str(Str {
                        text: "Item 2".to_string(),
                        source_info: empty_source(),
                    })],
                    source_info: empty_source(),
                })],
            ],
            source_info: empty_source(),
        })],
    };

    let output = write_ansi(&pandoc);
    assert!(output.contains("Item 1"));
    assert!(output.contains("Item 2"));
}

#[test]
fn test_ordered_list() {
    let pandoc = Pandoc {
        meta: Default::default(),
        blocks: vec![Block::OrderedList(OrderedList {
            attr: (
                1,
                pampa::pandoc::ListNumberStyle::Decimal,
                pampa::pandoc::ListNumberDelim::Period,
            ),
            content: vec![
                vec![Block::Plain(Plain {
                    content: vec![Inline::Str(Str {
                        text: "First".to_string(),
                        source_info: empty_source(),
                    })],
                    source_info: empty_source(),
                })],
                vec![Block::Plain(Plain {
                    content: vec![Inline::Str(Str {
                        text: "Second".to_string(),
                        source_info: empty_source(),
                    })],
                    source_info: empty_source(),
                })],
            ],
            source_info: empty_source(),
        })],
    };

    let output = write_ansi(&pandoc);
    assert!(output.contains("First"));
    assert!(output.contains("Second"));
}

#[test]
fn test_definition_list() {
    let pandoc = Pandoc {
        meta: Default::default(),
        blocks: vec![Block::DefinitionList(DefinitionList {
            content: vec![(
                vec![Inline::Str(Str {
                    text: "Term".to_string(),
                    source_info: empty_source(),
                })],
                vec![vec![Block::Plain(Plain {
                    content: vec![Inline::Str(Str {
                        text: "Definition".to_string(),
                        source_info: empty_source(),
                    })],
                    source_info: empty_source(),
                })]],
            )],
            source_info: empty_source(),
        })],
    };

    let output = write_ansi(&pandoc);
    assert!(output.contains("Term"));
    assert!(output.contains("Definition"));
}

// ============================================================================
// BlockQuote Test
// ============================================================================

#[test]
fn test_blockquote() {
    let pandoc = Pandoc {
        meta: Default::default(),
        blocks: vec![Block::BlockQuote(BlockQuote {
            content: vec![Block::Paragraph(Paragraph {
                content: vec![Inline::Str(Str {
                    text: "Quoted text".to_string(),
                    source_info: empty_source(),
                })],
                source_info: empty_source(),
            })],
            source_info: empty_source(),
        })],
    };

    let output = write_ansi(&pandoc);
    assert!(output.contains("Quoted text"));
    // Should have some marker (either │ or >)
    assert!(output.contains("│") || output.contains(">"));
}

// ============================================================================
// Div Test
// ============================================================================

#[test]
fn test_div() {
    let pandoc = Pandoc {
        meta: Default::default(),
        blocks: vec![Block::Div(Div {
            attr: empty_attr(),
            content: vec![Block::Paragraph(Paragraph {
                content: vec![Inline::Str(Str {
                    text: "Div content".to_string(),
                    source_info: empty_source(),
                })],
                source_info: empty_source(),
            })],
            source_info: empty_source(),
            attr_source: empty_attr_source(),
        })],
    };

    let output = write_ansi(&pandoc);
    assert!(output.contains("Div content"));
}

#[test]
fn test_div_with_color() {
    let mut attrs = LinkedHashMap::new();
    attrs.insert("color".to_string(), "red".to_string());
    let attr = (String::new(), vec![], attrs);

    let pandoc = Pandoc {
        meta: Default::default(),
        blocks: vec![Block::Div(Div {
            attr,
            content: vec![Block::Paragraph(Paragraph {
                content: vec![Inline::Str(Str {
                    text: "Colored".to_string(),
                    source_info: empty_source(),
                })],
                source_info: empty_source(),
            })],
            source_info: empty_source(),
            attr_source: empty_attr_source(),
        })],
    };

    let output = write_ansi(&pandoc);
    assert!(output.contains("Colored"));
}

// ============================================================================
// RawBlock Test
// ============================================================================

#[test]
fn test_rawblock_ansi_format() {
    let pandoc = Pandoc {
        meta: Default::default(),
        blocks: vec![Block::RawBlock(RawBlock {
            format: "ansi".to_string(),
            text: "Raw ANSI content".to_string(),
            source_info: empty_source(),
        })],
    };

    let output = write_ansi(&pandoc);
    assert!(output.contains("Raw ANSI content"));
}

#[test]
fn test_rawblock_other_format_skipped() {
    let pandoc = Pandoc {
        meta: Default::default(),
        blocks: vec![Block::RawBlock(RawBlock {
            format: "html".to_string(),
            text: "<p>HTML content</p>".to_string(),
            source_info: empty_source(),
        })],
    };

    let output = write_ansi(&pandoc);
    // HTML format should be skipped
    assert!(!output.contains("HTML content"));
}

// ============================================================================
// Inline Tests
// ============================================================================

#[test]
fn test_inline_str() {
    let pandoc = Pandoc {
        meta: Default::default(),
        blocks: vec![Block::Plain(Plain {
            content: vec![Inline::Str(Str {
                text: "Simple text".to_string(),
                source_info: empty_source(),
            })],
            source_info: empty_source(),
        })],
    };

    let output = write_ansi(&pandoc);
    assert!(output.contains("Simple text"));
}

#[test]
fn test_inline_space() {
    let pandoc = Pandoc {
        meta: Default::default(),
        blocks: vec![Block::Plain(Plain {
            content: vec![
                Inline::Str(Str {
                    text: "Word1".to_string(),
                    source_info: empty_source(),
                }),
                Inline::Space(Space {
                    source_info: empty_source(),
                }),
                Inline::Str(Str {
                    text: "Word2".to_string(),
                    source_info: empty_source(),
                }),
            ],
            source_info: empty_source(),
        })],
    };

    let output = write_ansi(&pandoc);
    assert!(output.contains("Word1 Word2"));
}

#[test]
fn test_inline_softbreak() {
    let pandoc = Pandoc {
        meta: Default::default(),
        blocks: vec![Block::Plain(Plain {
            content: vec![
                Inline::Str(Str {
                    text: "Line1".to_string(),
                    source_info: empty_source(),
                }),
                Inline::SoftBreak(SoftBreak {
                    source_info: empty_source(),
                }),
                Inline::Str(Str {
                    text: "Line2".to_string(),
                    source_info: empty_source(),
                }),
            ],
            source_info: empty_source(),
        })],
    };

    let output = write_ansi(&pandoc);
    assert!(output.contains("Line1"));
    assert!(output.contains("Line2"));
}

#[test]
fn test_inline_linebreak() {
    let pandoc = Pandoc {
        meta: Default::default(),
        blocks: vec![Block::Plain(Plain {
            content: vec![
                Inline::Str(Str {
                    text: "Before".to_string(),
                    source_info: empty_source(),
                }),
                Inline::LineBreak(LineBreak {
                    source_info: empty_source(),
                }),
                Inline::Str(Str {
                    text: "After".to_string(),
                    source_info: empty_source(),
                }),
            ],
            source_info: empty_source(),
        })],
    };

    let output = write_ansi(&pandoc);
    assert!(output.contains("Before"));
    assert!(output.contains("After"));
}

#[test]
fn test_inline_emph() {
    let pandoc = Pandoc {
        meta: Default::default(),
        blocks: vec![Block::Plain(Plain {
            content: vec![Inline::Emph(Emph {
                content: vec![Inline::Str(Str {
                    text: "Emphasis".to_string(),
                    source_info: empty_source(),
                })],
                source_info: empty_source(),
            })],
            source_info: empty_source(),
        })],
    };

    let output = write_ansi(&pandoc);
    assert!(output.contains("Emphasis"));
}

#[test]
fn test_inline_strong() {
    let pandoc = Pandoc {
        meta: Default::default(),
        blocks: vec![Block::Plain(Plain {
            content: vec![Inline::Strong(Strong {
                content: vec![Inline::Str(Str {
                    text: "Bold".to_string(),
                    source_info: empty_source(),
                })],
                source_info: empty_source(),
            })],
            source_info: empty_source(),
        })],
    };

    let output = write_ansi(&pandoc);
    assert!(output.contains("Bold"));
}

#[test]
fn test_inline_underline() {
    let pandoc = Pandoc {
        meta: Default::default(),
        blocks: vec![Block::Plain(Plain {
            content: vec![Inline::Underline(Underline {
                content: vec![Inline::Str(Str {
                    text: "Underlined".to_string(),
                    source_info: empty_source(),
                })],
                source_info: empty_source(),
            })],
            source_info: empty_source(),
        })],
    };

    let output = write_ansi(&pandoc);
    assert!(output.contains("Underlined"));
}

#[test]
fn test_inline_strikeout() {
    let pandoc = Pandoc {
        meta: Default::default(),
        blocks: vec![Block::Plain(Plain {
            content: vec![Inline::Strikeout(Strikeout {
                content: vec![Inline::Str(Str {
                    text: "Struck".to_string(),
                    source_info: empty_source(),
                })],
                source_info: empty_source(),
            })],
            source_info: empty_source(),
        })],
    };

    let output = write_ansi(&pandoc);
    assert!(output.contains("Struck"));
}

#[test]
fn test_inline_superscript() {
    let pandoc = Pandoc {
        meta: Default::default(),
        blocks: vec![Block::Plain(Plain {
            content: vec![Inline::Superscript(Superscript {
                content: vec![Inline::Str(Str {
                    text: "2".to_string(),
                    source_info: empty_source(),
                })],
                source_info: empty_source(),
            })],
            source_info: empty_source(),
        })],
    };

    let output = write_ansi(&pandoc);
    // Superscript is rendered as ^2
    assert!(output.contains("^2"));
}

#[test]
fn test_inline_subscript() {
    let pandoc = Pandoc {
        meta: Default::default(),
        blocks: vec![Block::Plain(Plain {
            content: vec![Inline::Subscript(Subscript {
                content: vec![Inline::Str(Str {
                    text: "2".to_string(),
                    source_info: empty_source(),
                })],
                source_info: empty_source(),
            })],
            source_info: empty_source(),
        })],
    };

    let output = write_ansi(&pandoc);
    // Subscript is rendered as _2
    assert!(output.contains("_2"));
}

#[test]
fn test_inline_smallcaps() {
    let pandoc = Pandoc {
        meta: Default::default(),
        blocks: vec![Block::Plain(Plain {
            content: vec![Inline::SmallCaps(SmallCaps {
                content: vec![Inline::Str(Str {
                    text: "SmallCaps".to_string(),
                    source_info: empty_source(),
                })],
                source_info: empty_source(),
            })],
            source_info: empty_source(),
        })],
    };

    let output = write_ansi(&pandoc);
    assert!(output.contains("SmallCaps"));
}

#[test]
fn test_inline_quoted_double() {
    let pandoc = Pandoc {
        meta: Default::default(),
        blocks: vec![Block::Plain(Plain {
            content: vec![Inline::Quoted(Quoted {
                quote_type: pampa::pandoc::QuoteType::DoubleQuote,
                content: vec![Inline::Str(Str {
                    text: "quoted".to_string(),
                    source_info: empty_source(),
                })],
                source_info: empty_source(),
            })],
            source_info: empty_source(),
        })],
    };

    let output = write_ansi(&pandoc);
    assert!(output.contains("\"quoted\""));
}

#[test]
fn test_inline_quoted_single() {
    let pandoc = Pandoc {
        meta: Default::default(),
        blocks: vec![Block::Plain(Plain {
            content: vec![Inline::Quoted(Quoted {
                quote_type: pampa::pandoc::QuoteType::SingleQuote,
                content: vec![Inline::Str(Str {
                    text: "quoted".to_string(),
                    source_info: empty_source(),
                })],
                source_info: empty_source(),
            })],
            source_info: empty_source(),
        })],
    };

    let output = write_ansi(&pandoc);
    assert!(output.contains("'quoted'"));
}

#[test]
fn test_inline_code() {
    let pandoc = Pandoc {
        meta: Default::default(),
        blocks: vec![Block::Plain(Plain {
            content: vec![Inline::Code(Code {
                attr: empty_attr(),
                text: "let x = 1".to_string(),
                source_info: empty_source(),
                attr_source: empty_attr_source(),
            })],
            source_info: empty_source(),
        })],
    };

    let output = write_ansi(&pandoc);
    assert!(output.contains("let x = 1"));
}

#[test]
fn test_inline_math() {
    let pandoc = Pandoc {
        meta: Default::default(),
        blocks: vec![Block::Plain(Plain {
            content: vec![Inline::Math(Math {
                math_type: pampa::pandoc::MathType::InlineMath,
                text: "E=mc^2".to_string(),
                source_info: empty_source(),
            })],
            source_info: empty_source(),
        })],
    };

    let output = write_ansi(&pandoc);
    assert!(output.contains("E=mc^2"));
}

#[test]
fn test_inline_rawinline_ansi() {
    let pandoc = Pandoc {
        meta: Default::default(),
        blocks: vec![Block::Plain(Plain {
            content: vec![Inline::RawInline(RawInline {
                format: "ansi".to_string(),
                text: "\x1b[31mRed\x1b[0m".to_string(),
                source_info: empty_source(),
            })],
            source_info: empty_source(),
        })],
    };

    let output = write_ansi(&pandoc);
    assert!(output.contains("\x1b[31mRed\x1b[0m"));
}

#[test]
fn test_inline_rawinline_other_format() {
    let pandoc = Pandoc {
        meta: Default::default(),
        blocks: vec![Block::Plain(Plain {
            content: vec![Inline::RawInline(RawInline {
                format: "html".to_string(),
                text: "<b>bold</b>".to_string(),
                source_info: empty_source(),
            })],
            source_info: empty_source(),
        })],
    };

    let output = write_ansi(&pandoc);
    // HTML format should be skipped
    assert!(!output.contains("<b>"));
}

#[test]
fn test_inline_link() {
    let pandoc = Pandoc {
        meta: Default::default(),
        blocks: vec![Block::Plain(Plain {
            content: vec![Inline::Link(Link {
                attr: empty_attr(),
                content: vec![Inline::Str(Str {
                    text: "Click me".to_string(),
                    source_info: empty_source(),
                })],
                target: ("https://example.com".to_string(), String::new()),
                target_source: TargetSourceInfo::empty(),
                source_info: empty_source(),
                attr_source: empty_attr_source(),
            })],
            source_info: empty_source(),
        })],
    };

    let output = write_ansi(&pandoc);
    assert!(output.contains("Click me"));
}

#[test]
fn test_inline_image() {
    let pandoc = Pandoc {
        meta: Default::default(),
        blocks: vec![Block::Plain(Plain {
            content: vec![Inline::Image(Image {
                attr: empty_attr(),
                content: vec![Inline::Str(Str {
                    text: "Alt text".to_string(),
                    source_info: empty_source(),
                })],
                target: ("image.png".to_string(), String::new()),
                target_source: TargetSourceInfo::empty(),
                source_info: empty_source(),
                attr_source: empty_attr_source(),
            })],
            source_info: empty_source(),
        })],
    };

    let output = write_ansi(&pandoc);
    assert!(output.contains("[Image: Alt text]"));
}

#[test]
fn test_inline_note() {
    let pandoc = Pandoc {
        meta: Default::default(),
        blocks: vec![Block::Plain(Plain {
            content: vec![Inline::Note(Note {
                content: vec![Block::Plain(Plain {
                    content: vec![Inline::Str(Str {
                        text: "Note content".to_string(),
                        source_info: empty_source(),
                    })],
                    source_info: empty_source(),
                })],
                source_info: empty_source(),
            })],
            source_info: empty_source(),
        })],
    };

    let output = write_ansi(&pandoc);
    // Note is rendered as superscript marker
    assert!(output.contains("^["));
}

#[test]
fn test_inline_span() {
    let pandoc = Pandoc {
        meta: Default::default(),
        blocks: vec![Block::Plain(Plain {
            content: vec![Inline::Span(Span {
                attr: empty_attr(),
                content: vec![Inline::Str(Str {
                    text: "Span text".to_string(),
                    source_info: empty_source(),
                })],
                source_info: empty_source(),
                attr_source: empty_attr_source(),
            })],
            source_info: empty_source(),
        })],
    };

    let output = write_ansi(&pandoc);
    assert!(output.contains("Span text"));
}

#[test]
fn test_inline_span_with_color() {
    let mut attrs = LinkedHashMap::new();
    attrs.insert("color".to_string(), "blue".to_string());
    let attr = (String::new(), vec![], attrs);

    let pandoc = Pandoc {
        meta: Default::default(),
        blocks: vec![Block::Plain(Plain {
            content: vec![Inline::Span(Span {
                attr,
                content: vec![Inline::Str(Str {
                    text: "Blue text".to_string(),
                    source_info: empty_source(),
                })],
                source_info: empty_source(),
                attr_source: empty_attr_source(),
            })],
            source_info: empty_source(),
        })],
    };

    let output = write_ansi(&pandoc);
    assert!(output.contains("Blue text"));
    // Should contain ANSI color codes
    assert!(output.contains("\x1b["));
}

#[test]
fn test_inline_span_with_background_color() {
    let mut attrs = LinkedHashMap::new();
    attrs.insert("background-color".to_string(), "yellow".to_string());
    let attr = (String::new(), vec![], attrs);

    let pandoc = Pandoc {
        meta: Default::default(),
        blocks: vec![Block::Plain(Plain {
            content: vec![Inline::Span(Span {
                attr,
                content: vec![Inline::Str(Str {
                    text: "Highlighted".to_string(),
                    source_info: empty_source(),
                })],
                source_info: empty_source(),
                attr_source: empty_attr_source(),
            })],
            source_info: empty_source(),
        })],
    };

    let output = write_ansi(&pandoc);
    assert!(output.contains("Highlighted"));
}

// ============================================================================
// Color Parsing Tests (via Spans)
// ============================================================================

#[test]
fn test_color_parsing_basic_colors() {
    let colors = [
        "black", "red", "green", "yellow", "blue", "magenta", "cyan", "white", "grey", "gray",
    ];

    for color in colors {
        let mut attrs = LinkedHashMap::new();
        attrs.insert("color".to_string(), color.to_string());
        let attr = (String::new(), vec![], attrs);

        let pandoc = Pandoc {
            meta: Default::default(),
            blocks: vec![Block::Plain(Plain {
                content: vec![Inline::Span(Span {
                    attr,
                    content: vec![Inline::Str(Str {
                        text: format!("Color {}", color),
                        source_info: empty_source(),
                    })],
                    source_info: empty_source(),
                    attr_source: empty_attr_source(),
                })],
                source_info: empty_source(),
            })],
        };

        let output = write_ansi(&pandoc);
        assert!(
            output.contains(&format!("Color {}", color)),
            "Failed for color: {}",
            color
        );
    }
}

#[test]
fn test_color_parsing_dark_colors() {
    let colors = [
        "dark-red",
        "darkred",
        "dark-green",
        "darkgreen",
        "dark-blue",
        "darkblue",
        "dark-grey",
        "darkgrey",
        "dark-gray",
        "darkgray",
    ];

    for color in colors {
        let mut attrs = LinkedHashMap::new();
        attrs.insert("color".to_string(), color.to_string());
        let attr = (String::new(), vec![], attrs);

        let pandoc = Pandoc {
            meta: Default::default(),
            blocks: vec![Block::Plain(Plain {
                content: vec![Inline::Span(Span {
                    attr,
                    content: vec![Inline::Str(Str {
                        text: "Text".to_string(),
                        source_info: empty_source(),
                    })],
                    source_info: empty_source(),
                    attr_source: empty_attr_source(),
                })],
                source_info: empty_source(),
            })],
        };

        let output = write_ansi(&pandoc);
        assert!(output.contains("Text"), "Failed for color: {}", color);
    }
}

#[test]
fn test_color_parsing_hex() {
    // Test #RRGGBB format
    let mut attrs = LinkedHashMap::new();
    attrs.insert("color".to_string(), "#FF5500".to_string());
    let attr = (String::new(), vec![], attrs);

    let pandoc = Pandoc {
        meta: Default::default(),
        blocks: vec![Block::Plain(Plain {
            content: vec![Inline::Span(Span {
                attr,
                content: vec![Inline::Str(Str {
                    text: "Hex color".to_string(),
                    source_info: empty_source(),
                })],
                source_info: empty_source(),
                attr_source: empty_attr_source(),
            })],
            source_info: empty_source(),
        })],
    };

    let output = write_ansi(&pandoc);
    assert!(output.contains("Hex color"));
}

#[test]
fn test_color_parsing_hex_short() {
    // Test #RGB format
    let mut attrs = LinkedHashMap::new();
    attrs.insert("color".to_string(), "#F80".to_string());
    let attr = (String::new(), vec![], attrs);

    let pandoc = Pandoc {
        meta: Default::default(),
        blocks: vec![Block::Plain(Plain {
            content: vec![Inline::Span(Span {
                attr,
                content: vec![Inline::Str(Str {
                    text: "Short hex".to_string(),
                    source_info: empty_source(),
                })],
                source_info: empty_source(),
                attr_source: empty_attr_source(),
            })],
            source_info: empty_source(),
        })],
    };

    let output = write_ansi(&pandoc);
    assert!(output.contains("Short hex"));
}

#[test]
fn test_color_parsing_rgb_function() {
    let mut attrs = LinkedHashMap::new();
    attrs.insert("color".to_string(), "rgb(255, 128, 0)".to_string());
    let attr = (String::new(), vec![], attrs);

    let pandoc = Pandoc {
        meta: Default::default(),
        blocks: vec![Block::Plain(Plain {
            content: vec![Inline::Span(Span {
                attr,
                content: vec![Inline::Str(Str {
                    text: "RGB func".to_string(),
                    source_info: empty_source(),
                })],
                source_info: empty_source(),
                attr_source: empty_attr_source(),
            })],
            source_info: empty_source(),
        })],
    };

    let output = write_ansi(&pandoc);
    assert!(output.contains("RGB func"));
}

#[test]
fn test_color_parsing_ansi_palette() {
    let mut attrs = LinkedHashMap::new();
    attrs.insert("color".to_string(), "ansi(42)".to_string());
    let attr = (String::new(), vec![], attrs);

    let pandoc = Pandoc {
        meta: Default::default(),
        blocks: vec![Block::Plain(Plain {
            content: vec![Inline::Span(Span {
                attr,
                content: vec![Inline::Str(Str {
                    text: "ANSI palette".to_string(),
                    source_info: empty_source(),
                })],
                source_info: empty_source(),
                attr_source: empty_attr_source(),
            })],
            source_info: empty_source(),
        })],
    };

    let output = write_ansi(&pandoc);
    assert!(output.contains("ANSI palette"));
}

#[test]
fn test_color_parsing_ansi_dash() {
    let mut attrs = LinkedHashMap::new();
    attrs.insert("color".to_string(), "ansi-200".to_string());
    let attr = (String::new(), vec![], attrs);

    let pandoc = Pandoc {
        meta: Default::default(),
        blocks: vec![Block::Plain(Plain {
            content: vec![Inline::Span(Span {
                attr,
                content: vec![Inline::Str(Str {
                    text: "ANSI dash".to_string(),
                    source_info: empty_source(),
                })],
                source_info: empty_source(),
                attr_source: empty_attr_source(),
            })],
            source_info: empty_source(),
        })],
    };

    let output = write_ansi(&pandoc);
    assert!(output.contains("ANSI dash"));
}

// ============================================================================
// Error Case Tests (Unsupported Blocks)
// ============================================================================

#[test]
fn test_unsupported_codeblock() {
    use pampa::pandoc::CodeBlock;

    let pandoc = Pandoc {
        meta: Default::default(),
        blocks: vec![Block::CodeBlock(CodeBlock {
            attr: empty_attr(),
            text: "code".to_string(),
            source_info: empty_source(),
            attr_source: empty_attr_source(),
        })],
    };

    let mut buf = Vec::new();
    let result = writers::ansi::write(&pandoc, &mut buf);

    // Should return an error for unsupported block
    assert!(result.is_err());
}

// Note: Table test removed due to complex struct initialization

// ============================================================================
// Mixed Content Tests
// ============================================================================

#[test]
fn test_mixed_blocks_and_inlines() {
    let pandoc = Pandoc {
        meta: Default::default(),
        blocks: vec![
            Block::Header(Header {
                level: 1,
                attr: empty_attr(),
                content: vec![Inline::Str(Str {
                    text: "Title".to_string(),
                    source_info: empty_source(),
                })],
                source_info: empty_source(),
                attr_source: empty_attr_source(),
            }),
            Block::Paragraph(Paragraph {
                content: vec![
                    Inline::Str(Str {
                        text: "This is".to_string(),
                        source_info: empty_source(),
                    }),
                    Inline::Space(Space {
                        source_info: empty_source(),
                    }),
                    Inline::Strong(Strong {
                        content: vec![Inline::Str(Str {
                            text: "important".to_string(),
                            source_info: empty_source(),
                        })],
                        source_info: empty_source(),
                    }),
                    Inline::Space(Space {
                        source_info: empty_source(),
                    }),
                    Inline::Str(Str {
                        text: "text.".to_string(),
                        source_info: empty_source(),
                    }),
                ],
                source_info: empty_source(),
            }),
            Block::BulletList(BulletList {
                content: vec![
                    vec![Block::Plain(Plain {
                        content: vec![Inline::Str(Str {
                            text: "Item one".to_string(),
                            source_info: empty_source(),
                        })],
                        source_info: empty_source(),
                    })],
                    vec![Block::Plain(Plain {
                        content: vec![Inline::Str(Str {
                            text: "Item two".to_string(),
                            source_info: empty_source(),
                        })],
                        source_info: empty_source(),
                    })],
                ],
                source_info: empty_source(),
            }),
        ],
    };

    let output = write_ansi(&pandoc);
    assert!(output.contains("Title"));
    assert!(output.contains("important"));
    assert!(output.contains("Item one"));
    assert!(output.contains("Item two"));
}

#[test]
fn test_nested_formatting() {
    let pandoc = Pandoc {
        meta: Default::default(),
        blocks: vec![Block::Paragraph(Paragraph {
            content: vec![Inline::Strong(Strong {
                content: vec![
                    Inline::Str(Str {
                        text: "Bold with ".to_string(),
                        source_info: empty_source(),
                    }),
                    Inline::Emph(Emph {
                        content: vec![Inline::Str(Str {
                            text: "italic".to_string(),
                            source_info: empty_source(),
                        })],
                        source_info: empty_source(),
                    }),
                ],
                source_info: empty_source(),
            })],
            source_info: empty_source(),
        })],
    };

    let output = write_ansi(&pandoc);
    assert!(output.contains("Bold with"));
    assert!(output.contains("italic"));
}

// Note: Citation test removed due to complex struct initialization
