/*
 * test_json_roundtrip.rs
 * Copyright (c) 2025 Posit, PBC
 */

use hashlink::LinkedHashMap;
use quarto_markdown_pandoc::pandoc::ast_context::ASTContext;
use quarto_markdown_pandoc::pandoc::location::SourceInfo;
use quarto_markdown_pandoc::pandoc::{Block, Inline, Pandoc, Paragraph, Str};
use quarto_markdown_pandoc::readers;
use quarto_markdown_pandoc::writers::json;
use std::collections::HashMap;

#[test]
fn test_json_roundtrip_simple_paragraph() {
    // Create a simple Pandoc document
    let original = Pandoc {
        meta: quarto_markdown_pandoc::pandoc::MetaValueWithSourceInfo::default(),
        blocks: vec![Block::Paragraph(Paragraph {
            content: vec![Inline::Str(Str {
                text: "Hello, world!".to_string(),
                source_info: SourceInfo::new(
                    None,
                    quarto_markdown_pandoc::pandoc::location::Range {
                        start: quarto_markdown_pandoc::pandoc::location::Location {
                            offset: 0,
                            row: 0,
                            column: 0,
                        },
                        end: quarto_markdown_pandoc::pandoc::location::Location {
                            offset: 13,
                            row: 0,
                            column: 13,
                        },
                    },
                ),
                source_info_qsm: None,
            })],
            source_info: SourceInfo::new(
                None,
                quarto_markdown_pandoc::pandoc::location::Range {
                    start: quarto_markdown_pandoc::pandoc::location::Location {
                        offset: 0,
                        row: 0,
                        column: 0,
                    },
                    end: quarto_markdown_pandoc::pandoc::location::Location {
                        offset: 13,
                        row: 0,
                        column: 13,
                        },
                },
            ),
            source_info_qsm: None,
        })],
    };

    // Write to JSON
    let context = ASTContext::new(); // Empty context since we have no filenames
    let mut json_output = Vec::new();
    json::write(&original, &context, &mut json_output).expect("Failed to write JSON");

    // Read back from JSON
    let mut json_reader = std::io::Cursor::new(json_output);
    let (parsed, _parsed_context) =
        readers::json::read(&mut json_reader).expect("Failed to read JSON");

    // Compare the documents
    assert_eq!(original.meta, parsed.meta);
    assert_eq!(original.blocks.len(), parsed.blocks.len());

    // For now, just check that we can parse back what we wrote
    // Full equality might be challenging due to location differences
    match (&original.blocks[0], &parsed.blocks[0]) {
        (Block::Paragraph(orig_para), Block::Paragraph(parsed_para)) => {
            assert_eq!(orig_para.content.len(), parsed_para.content.len());
            match (&orig_para.content[0], &parsed_para.content[0]) {
                (Inline::Str(orig_str), Inline::Str(parsed_str)) => {
                    assert_eq!(orig_str.text, parsed_str.text);
                }
                _ => panic!("Expected Str inline"),
            }
        }
        _ => panic!("Expected paragraph blocks"),
    }
}

#[test]
fn test_json_roundtrip_complex_document() {
    // Create a more complex document with multiple block types
    let original = Pandoc {
        meta: quarto_markdown_pandoc::pandoc::MetaValueWithSourceInfo::MetaMap {
            entries: vec![quarto_markdown_pandoc::pandoc::meta::MetaMapEntry {
                key: "title".to_string(),
                key_source: quarto_source_map::SourceInfo::default(),
                value: quarto_markdown_pandoc::pandoc::MetaValueWithSourceInfo::MetaString {
                    value: "Test Document".to_string(),
                    source_info: quarto_source_map::SourceInfo::default(),
                },
            }],
            source_info: quarto_source_map::SourceInfo::default(),
        },
        blocks: vec![
            Block::Paragraph(Paragraph {
                content: vec![
                    Inline::Str(Str {
                        text: "This is ".to_string(),
                        source_info: SourceInfo::new(
                            None,
                            quarto_markdown_pandoc::pandoc::location::Range {
                                start: quarto_markdown_pandoc::pandoc::location::Location {
                                    offset: 0,
                                    row: 0,
                                    column: 0,
                                },
                                end: quarto_markdown_pandoc::pandoc::location::Location {
                                    offset: 8,
                                    row: 0,
                                    column: 8,
                                },
                            },
                        ),
                        source_info_qsm: None,
                    }),
                    Inline::Strong(quarto_markdown_pandoc::pandoc::Strong {
                        content: vec![Inline::Str(Str {
                            text: "bold text".to_string(),
                            source_info: SourceInfo::new(
                                None,
                                quarto_markdown_pandoc::pandoc::location::Range {
                                    start: quarto_markdown_pandoc::pandoc::location::Location {
                                        offset: 8,
                                        row: 0,
                                        column: 8,
                                    },
                                    end: quarto_markdown_pandoc::pandoc::location::Location {
                                        offset: 17,
                                        row: 0,
                                        column: 17,
                                    },
                                },
                            ),
                            source_info_qsm: None,
                        })],
                        source_info: SourceInfo::new(
                            None,
                            quarto_markdown_pandoc::pandoc::location::Range {
                                start: quarto_markdown_pandoc::pandoc::location::Location {
                                    offset: 8,
                                    row: 0,
                                    column: 8,
                                },
                                end: quarto_markdown_pandoc::pandoc::location::Location {
                                    offset: 17,
                                    row: 0,
                                    column: 17,
                                },
                            },
                        ),
                        source_info_qsm: None,
                    }),
                    Inline::Str(Str {
                        text: ".".to_string(),
                        source_info: SourceInfo::new(
                            None,
                            quarto_markdown_pandoc::pandoc::location::Range {
                                start: quarto_markdown_pandoc::pandoc::location::Location {
                                    offset: 17,
                                    row: 0,
                                    column: 17,
                                },
                                end: quarto_markdown_pandoc::pandoc::location::Location {
                                    offset: 18,
                                    row: 0,
                                    column: 18,
                                },
                            },
                        ),
                        source_info_qsm: None,
                    }),
                ],
                source_info: SourceInfo::new(
                    None,
                    quarto_markdown_pandoc::pandoc::location::Range {
                        start: quarto_markdown_pandoc::pandoc::location::Location {
                            offset: 0,
                            row: 0,
                            column: 0,
                        },
                        end: quarto_markdown_pandoc::pandoc::location::Location {
                            offset: 20,
                            row: 0,
                            column: 20,
                        },
                    },
                ),
                source_info_qsm: None,
            }),
            Block::CodeBlock(quarto_markdown_pandoc::pandoc::CodeBlock {
                attr: ("".to_string(), vec![], HashMap::new()),
                text: "print('Hello, world!')".to_string(),
                source_info: SourceInfo::new(
                    None,
                    quarto_markdown_pandoc::pandoc::location::Range {
                        start: quarto_markdown_pandoc::pandoc::location::Location {
                            offset: 21,
                            row: 1,
                            column: 0,
                        },
                        end: quarto_markdown_pandoc::pandoc::location::Location {
                            offset: 43,
                            row: 1,
                            column: 22,
                        },
                    },
                ),
                source_info_qsm: None,
            }),
        ],
    };

    // Write to JSON
    let context = ASTContext::new(); // Empty context since we have no filenames
    let mut json_output = Vec::new();
    json::write(&original, &context, &mut json_output).expect("Failed to write JSON");

    // Read back from JSON
    let mut json_reader = std::io::Cursor::new(json_output);
    let (parsed, _parsed_context) =
        readers::json::read(&mut json_reader).expect("Failed to read JSON");

    // Verify basic structure
    assert_eq!(parsed.blocks.len(), 2);
    assert!(parsed.meta.contains_key("title"));

    match parsed.meta.get("title") {
        Some(quarto_markdown_pandoc::pandoc::MetaValueWithSourceInfo::MetaString {
            value, ..
        }) => {
            assert_eq!(value, "Test Document");
        }
        _ => panic!("Expected MetaString for title"),
    }

    // Verify first block (paragraph)
    match &parsed.blocks[0] {
        Block::Paragraph(para) => {
            assert_eq!(para.content.len(), 3);
        }
        _ => panic!("Expected paragraph as first block"),
    }

    // Verify second block (code block)
    match &parsed.blocks[1] {
        Block::CodeBlock(code) => {
            assert_eq!(code.text, "print('Hello, world!')");
        }
        _ => panic!("Expected code block as second block"),
    }
}

#[test]
fn test_json_write_then_read_matches_original_structure() {
    // This test ensures that anything we can write, we can also read back
    // with the same basic structure, even if exact equality is not possible

    let original = Pandoc {
        meta: quarto_markdown_pandoc::pandoc::MetaValueWithSourceInfo::default(),
        blocks: vec![
            Block::Plain(quarto_markdown_pandoc::pandoc::Plain {
                content: vec![Inline::Str(Str {
                    text: "Plain text".to_string(),
                    source_info: SourceInfo::new(
                        Some(0), // Index 0 will point to "test.md" in the context
                        quarto_markdown_pandoc::pandoc::location::Range {
                            start: quarto_markdown_pandoc::pandoc::location::Location {
                                offset: 0,
                                row: 0,
                                column: 0,
                            },
                            end: quarto_markdown_pandoc::pandoc::location::Location {
                                offset: 10,
                                row: 0,
                                column: 10,
                            },
                        },
                    ),
                    source_info_qsm: None,
                })],
                source_info: SourceInfo::new(
                    Some(0),
                    quarto_markdown_pandoc::pandoc::location::Range {
                        start: quarto_markdown_pandoc::pandoc::location::Location {
                            offset: 0,
                            row: 0,
                            column: 0,
                        },
                        end: quarto_markdown_pandoc::pandoc::location::Location {
                            offset: 10,
                            row: 0,
                            column: 10,
                        },
                    },
                ),
                source_info_qsm: None,
            }),
            Block::RawBlock(quarto_markdown_pandoc::pandoc::RawBlock {
                format: "html".to_string(),
                text: "<div>Raw HTML</div>".to_string(),
                source_info: SourceInfo::new(
                    Some(0),
                    quarto_markdown_pandoc::pandoc::location::Range {
                        start: quarto_markdown_pandoc::pandoc::location::Location {
                            offset: 11,
                            row: 1,
                            column: 0,
                        },
                        end: quarto_markdown_pandoc::pandoc::location::Location {
                            offset: 30,
                            row: 1,
                            column: 19,
                        },
                    },
                ),
                source_info_qsm: None,
            }),
        ],
    };

    // Write to JSON
    let context = ASTContext::with_filename("test.md"); // Create context with filename at index 0
    let mut json_output = Vec::new();
    json::write(&original, &context, &mut json_output).expect("Failed to write JSON");

    // Convert to string for debugging if needed
    let json_string = String::from_utf8(json_output.clone()).expect("Invalid UTF-8");
    println!("Generated JSON: {}", json_string);

    // Read back from JSON
    let mut json_reader = std::io::Cursor::new(json_output);
    let (parsed, parsed_context) =
        readers::json::read(&mut json_reader).expect("Failed to read JSON");

    // Verify context was preserved
    assert_eq!(parsed_context.filenames, vec!["test.md"]);

    // Verify we can parse back the same structure
    assert_eq!(original.blocks.len(), parsed.blocks.len());

    match (&original.blocks[0], &parsed.blocks[0]) {
        (Block::Plain(_), Block::Plain(_)) => {} // Structure matches
        _ => panic!("Block type mismatch for first block"),
    }

    match (&original.blocks[1], &parsed.blocks[1]) {
        (Block::RawBlock(orig), Block::RawBlock(parsed)) => {
            assert_eq!(orig.format, parsed.format);
            assert_eq!(orig.text, parsed.text);
        }
        _ => panic!("Block type mismatch for second block"),
    }
}
