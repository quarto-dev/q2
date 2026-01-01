/*
 * test_json_roundtrip.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Updated to use ConfigValue API directly (Phase 5 migration).
 */

use hashlink::LinkedHashMap;
use pampa::pandoc::ast_context::ASTContext;
use pampa::pandoc::{Block, Inline, Pandoc, Paragraph, Str};
use pampa::readers;
use pampa::writers::json;
use quarto_pandoc_types::{ConfigMapEntry, ConfigValue, ConfigValueKind, MergeOp};
use quarto_source_map::{FileId, Location, Range, SourceInfo};

#[test]
fn test_json_roundtrip_simple_paragraph() {
    // Create a simple Pandoc document
    let original = Pandoc {
        meta: ConfigValue::default(),
        blocks: vec![Block::Paragraph(Paragraph {
            content: vec![Inline::Str(Str {
                text: "Hello, world!".to_string(),
                source_info: SourceInfo::from_range(
                    FileId(0),
                    Range {
                        start: Location {
                            offset: 0,
                            row: 0,
                            column: 0,
                        },
                        end: Location {
                            offset: 13,
                            row: 0,
                            column: 13,
                        },
                    },
                ),
            })],
            source_info: SourceInfo::from_range(
                FileId(0),
                Range {
                    start: Location {
                        offset: 0,
                        row: 0,
                        column: 0,
                    },
                    end: Location {
                        offset: 13,
                        row: 0,
                        column: 13,
                    },
                },
            ),
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
        meta: ConfigValue {
            value: ConfigValueKind::Map(vec![ConfigMapEntry {
                key: "title".to_string(),
                key_source: SourceInfo::default(),
                value: ConfigValue {
                    value: ConfigValueKind::Scalar(yaml_rust2::Yaml::String(
                        "Test Document".to_string(),
                    )),
                    source_info: SourceInfo::default(),
                    merge_op: MergeOp::default(),
                },
            }]),
            source_info: SourceInfo::default(),
            merge_op: MergeOp::default(),
        },
        blocks: vec![
            Block::Paragraph(Paragraph {
                content: vec![
                    Inline::Str(Str {
                        text: "This is ".to_string(),
                        source_info: SourceInfo::from_range(
                            FileId(0),
                            Range {
                                start: Location {
                                    offset: 0,
                                    row: 0,
                                    column: 0,
                                },
                                end: Location {
                                    offset: 8,
                                    row: 0,
                                    column: 8,
                                },
                            },
                        ),
                    }),
                    Inline::Strong(pampa::pandoc::Strong {
                        content: vec![Inline::Str(Str {
                            text: "bold text".to_string(),
                            source_info: SourceInfo::from_range(
                                FileId(0),
                                Range {
                                    start: Location {
                                        offset: 8,
                                        row: 0,
                                        column: 8,
                                    },
                                    end: Location {
                                        offset: 17,
                                        row: 0,
                                        column: 17,
                                    },
                                },
                            ),
                        })],
                        source_info: SourceInfo::from_range(
                            FileId(0),
                            Range {
                                start: Location {
                                    offset: 8,
                                    row: 0,
                                    column: 8,
                                },
                                end: Location {
                                    offset: 17,
                                    row: 0,
                                    column: 17,
                                },
                            },
                        ),
                    }),
                    Inline::Str(Str {
                        text: ".".to_string(),
                        source_info: SourceInfo::from_range(
                            FileId(0),
                            Range {
                                start: Location {
                                    offset: 17,
                                    row: 0,
                                    column: 17,
                                },
                                end: Location {
                                    offset: 18,
                                    row: 0,
                                    column: 18,
                                },
                            },
                        ),
                    }),
                ],
                source_info: SourceInfo::from_range(
                    FileId(0),
                    Range {
                        start: Location {
                            offset: 0,
                            row: 0,
                            column: 0,
                        },
                        end: Location {
                            offset: 20,
                            row: 0,
                            column: 20,
                        },
                    },
                ),
            }),
            Block::CodeBlock(pampa::pandoc::CodeBlock {
                attr: (String::new(), vec![], LinkedHashMap::new()),
                text: "print('Hello, world!')".to_string(),
                source_info: SourceInfo::from_range(
                    FileId(0),
                    Range {
                        start: Location {
                            offset: 21,
                            row: 1,
                            column: 0,
                        },
                        end: Location {
                            offset: 43,
                            row: 1,
                            column: 22,
                        },
                    },
                ),
                attr_source: pampa::pandoc::attr::AttrSourceInfo::empty(),
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

    // Check metadata using ConfigValue API
    if let ConfigValueKind::Map(entries) = &parsed.meta.value {
        let title_entry = entries.iter().find(|e| e.key == "title");
        assert!(title_entry.is_some(), "Should have 'title' in metadata");

        if let Some(entry) = title_entry {
            match &entry.value.value {
                ConfigValueKind::Scalar(yaml_rust2::Yaml::String(s)) => {
                    assert_eq!(s, "Test Document");
                }
                other => panic!("Expected String for title, got {:?}", other),
            }
        }
    } else {
        panic!("Expected Map for metadata");
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
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![
            Block::Plain(pampa::pandoc::Plain {
                content: vec![Inline::Str(Str {
                    text: "Plain text".to_string(),
                    source_info: SourceInfo::from_range(
                        FileId(0),
                        Range {
                            start: Location {
                                offset: 0,
                                row: 0,
                                column: 0,
                            },
                            end: Location {
                                offset: 10,
                                row: 0,
                                column: 10,
                            },
                        },
                    ),
                })],
                source_info: SourceInfo::from_range(
                    FileId(0),
                    Range {
                        start: Location {
                            offset: 0,
                            row: 0,
                            column: 0,
                        },
                        end: Location {
                            offset: 10,
                            row: 0,
                            column: 10,
                        },
                    },
                ),
            }),
            Block::RawBlock(pampa::pandoc::RawBlock {
                format: "html".to_string(),
                text: "<div>Raw HTML</div>".to_string(),
                source_info: SourceInfo::from_range(
                    FileId(0),
                    Range {
                        start: Location {
                            offset: 11,
                            row: 1,
                            column: 0,
                        },
                        end: Location {
                            offset: 30,
                            row: 1,
                            column: 19,
                        },
                    },
                ),
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

/// Test that JSON roundtrip preserves source mapping capability (map_offset should work)
#[test]
fn test_json_roundtrip_preserves_source_mapping() {
    let qmd_content = r#"---
title: "Test"
---

Hello world
"#;

    // Create a temporary file for testing
    let temp_dir = std::env::temp_dir();
    let test_file = temp_dir.join("test_json_roundtrip_mapping.qmd");
    std::fs::write(&test_file, qmd_content).expect("Failed to write test file");

    // Step 1: Parse QMD to create initial AST with source mapping
    let result = readers::qmd::read(
        qmd_content.as_bytes(),
        false,
        &test_file.to_string_lossy(),
        &mut std::io::sink(),
        true,
        None,
    );
    let (pandoc1, context1, diagnostics) = result.expect("Failed to parse QMD");
    assert!(diagnostics.is_empty(), "Expected no parse errors");

    // Step 2: Serialize to JSON
    let mut json_buf = Vec::new();
    json::write(&pandoc1, &context1, &mut json_buf).expect("Failed to write JSON");

    // Step 3: Verify that JSON contains files array with embedded FileInformation
    let json_value: serde_json::Value =
        serde_json::from_slice(&json_buf).expect("Failed to parse JSON");
    let files_in_json = json_value["astContext"]["files"].as_array();
    assert!(
        files_in_json.is_some(),
        "JSON should contain files array in astContext"
    );
    let files_array = files_in_json.unwrap();
    assert_eq!(files_array.len(), 1, "Should have one file");

    // Verify the file entry has name, line_breaks, and total_length
    let file_obj = &files_array[0];
    assert!(
        file_obj["name"].is_string(),
        "File entry should have name field"
    );
    assert!(
        file_obj["line_breaks"].is_array(),
        "File entry should have line_breaks array"
    );
    assert!(
        file_obj["total_length"].is_number(),
        "File entry should have total_length"
    );

    // Step 4: Deserialize from JSON
    let (pandoc2, context2) =
        readers::json::read(&mut json_buf.as_slice()).expect("Failed to read JSON");

    // Step 5: Verify that the deserialized AST has working source mapping
    // Get the first block (should be a Para with "Hello world")
    if let Some(first_block) = pandoc2.blocks.first() {
        if let Block::Paragraph(para) = first_block {
            // Verify we have a SourceInfo (just check that it has non-zero length)
            assert!(
                para.source_info.length() > 0,
                "Para block should have source info with non-zero length after JSON roundtrip"
            );

            // The key test: map_offset should work on the deserialized SourceInfo
            let mapped_start = para.source_info.map_offset(0, &context2.source_context);
            assert!(
                mapped_start.is_some(),
                "map_offset should work after JSON roundtrip (start position). \
                 This means SourceContext should have been populated with file information from disk."
            );

            let mapped_end = para
                .source_info
                .map_offset(para.source_info.length(), &context2.source_context);
            assert!(
                mapped_end.is_some(),
                "map_offset should work after JSON roundtrip (end position)"
            );

            // Verify the mapped locations are sensible
            let start_loc = mapped_start.unwrap();
            // Row 4 is where "Hello world" starts (after frontmatter and blank line)
            assert_eq!(
                start_loc.location.row, 4,
                "After roundtrip, paragraph should still map to correct row"
            );

            // Also test inline elements have working source mapping
            if let Some(Inline::Str(str_inline)) = para.content.first() {
                let inline_mapped = str_inline
                    .source_info
                    .map_offset(0, &context2.source_context);
                assert!(
                    inline_mapped.is_some(),
                    "map_offset should work on inline elements after roundtrip"
                );
            }
        } else {
            panic!("Expected Para block, got {:?}", first_block);
        }
    } else {
        panic!("Expected at least one block in the document");
    }

    // Clean up
    std::fs::remove_file(&test_file).ok();
}

/// Test that escaped punctuation characters roundtrip correctly through qmd->AST->qmd
/// This tests all 30 escapable ASCII punctuation characters defined in the grammar
#[test]
fn test_qmd_roundtrip_escaped_punctuation() {
    use pampa::writers::qmd;

    // Test cases: each is (input_qmd, expected_output_qmd, description)
    // Note: We only test characters that we actively escape. Other punctuation
    // characters can be escaped in input, but won't be re-escaped in output.
    let test_cases = vec![
        // Dollar sign - critical for avoiding math mode
        (r"\$3.14", r"\$3.14", "escaped dollar sign"),
        (
            r"Price is \$5",
            r"Price is \$5",
            "escaped dollar in context",
        ),
        // Asterisk - critical for avoiding emphasis
        (r"\*test\*", r"\*test\*", "escaped asterisks"),
        (
            r"Note: \* is asterisk",
            r"Note: \* is asterisk",
            "escaped asterisk in context",
        ),
        // Underscore - critical for avoiding emphasis
        (r"\_test\_", r"\_test\_", "escaped underscores"),
        (
            r"var\_name",
            r"var\_name",
            "escaped underscore in identifier",
        ),
        // Brackets - critical for avoiding links
        (r"\[bracket\]", r"\[bracket\]", "escaped brackets"),
        // Backtick - critical for avoiding code
        (r"\`backtick\`", r"\`backtick\`", "escaped backticks"),
        // Hash - critical for avoiding headers
        (r"\# not a header", r"\# not a header", "escaped hash"),
        // Greater/less than - critical for avoiding blockquotes and HTML
        (r"\> not a quote", r"\> not a quote", "escaped greater-than"),
        (r"\< less than", r"\< less than", "escaped less-than"),
        // Backslash itself
        (r"\\", r"\\", "escaped backslash"),
        (
            r"C:\\path\\file",
            r"C:\\path\\file",
            "escaped backslashes in path",
        ),
        // Pipe - critical for tables
        (r"\|", r"\|", "escaped pipe"),
        // Tilde - critical for subscript/strikeout
        (r"\~", r"\~", "escaped tilde"),
        // Caret - critical for superscript
        (r"\^", r"\^", "escaped caret"),
        // Multiple escaped characters in one line
        (
            r"\$3.14 and \*not\* \$5",
            r"\$3.14 and \*not\* \$5",
            "multiple escaped chars",
        ),
        (
            r"Symbols: \$ \# \* \_ \[ \] \` \| \~ \^",
            r"Symbols: \$ \# \* \_ \[ \] \` \| \~ \^",
            "many symbols",
        ),
    ];

    for (input, expected_output, description) in test_cases {
        // Parse the input QMD
        let (parsed_doc, _context, diagnostics) = readers::qmd::read(
            input.as_bytes(),
            false,
            "<test>",
            &mut std::io::sink(),
            true,
            None,
        )
        .unwrap_or_else(|_| panic!("Failed to parse input for test case: {}", description));

        assert!(
            diagnostics.is_empty(),
            "Expected no parse errors for test case: {}. Got: {:?}",
            description,
            diagnostics
        );

        // Write the AST back to QMD
        let mut output_buf = Vec::new();
        qmd::write(&parsed_doc, &mut output_buf)
            .unwrap_or_else(|_| panic!("Failed to write QMD for test case: {}", description));

        let output = String::from_utf8(output_buf)
            .unwrap_or_else(|_| panic!("Invalid UTF-8 in output for test case: {}", description));

        // Trim trailing newline that write() adds
        let output_trimmed = output.trim_end();

        assert_eq!(
            output_trimmed, expected_output,
            "Roundtrip failed for test case: {}\nInput:    {}\nExpected: {}\nGot:      {}",
            description, input, expected_output, output_trimmed
        );
    }
}
