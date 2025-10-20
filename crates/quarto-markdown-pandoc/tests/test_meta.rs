/*
 * test_meta.rs
 * Copyright (c) 2025 Posit, PBC
 */

use hashlink::LinkedHashMap;
use quarto_markdown_pandoc::pandoc::location::{Location, Range, SourceInfo};
use quarto_markdown_pandoc::pandoc::meta::{MetaValue, rawblock_to_meta};
use quarto_markdown_pandoc::pandoc::{Inline, RawBlock, parse_metadata_strings};
use std::fs;

#[test]
fn test_metadata_parsing() {
    let content = fs::read_to_string("tests/features/metadata/metadata.qmd").unwrap();

    let block = RawBlock {
        format: "quarto_minus_metadata".to_string(),
        text: content,
        source_info: SourceInfo::with_range(Range {
            start: Location {
                offset: 0,
                row: 0,
                column: 0,
            },
            end: Location {
                offset: 0,
                row: 0,
                column: 0,
            },
        })
        .to_source_map_info(),
    };

    let meta = rawblock_to_meta(block);
    println!("Parsed metadata:");
    for (key, value) in &meta {
        println!("  {}: {:?}", key, value);
    }

    // Verify expected keys exist
    assert!(meta.contains_key("hello"));
    assert!(meta.contains_key("array"));
    assert!(meta.contains_key("array2"));
    assert!(meta.contains_key("complicated"));
    assert!(meta.contains_key("typed_values"));

    // Verify types
    assert!(matches!(meta.get("hello"), Some(MetaValue::MetaString(_))));
    assert!(matches!(meta.get("array"), Some(MetaValue::MetaList(_))));
    assert!(matches!(meta.get("array2"), Some(MetaValue::MetaList(_))));
    assert!(matches!(
        meta.get("complicated"),
        Some(MetaValue::MetaString(_))
    ));
    assert!(matches!(
        meta.get("typed_values"),
        Some(MetaValue::MetaList(_))
    ));
}

#[test]
fn test_yaml_tagged_strings() {
    // Test that YAML tags (!path, !glob, !str) prevent markdown parsing
    let content = fs::read_to_string("tests/yaml-tagged-strings.qmd").unwrap();

    let block = RawBlock {
        format: "quarto_minus_metadata".to_string(),
        text: content,
        source_info: SourceInfo::with_range(Range {
            start: Location {
                offset: 0,
                row: 0,
                column: 0,
            },
            end: Location {
                offset: 0,
                row: 0,
                column: 0,
            },
        })
        .to_source_map_info(),
    };

    let mut meta = rawblock_to_meta(block);
    let mut outer_meta = LinkedHashMap::new();

    // Parse metadata strings
    for (k, v) in meta.drain() {
        let parsed = parse_metadata_strings(v, &mut outer_meta);
        outer_meta.insert(k, parsed);
    }

    // Check plain_path - should be MetaInlines with Span wrapper
    let plain_path = outer_meta.get("plain_path").expect("plain_path not found");
    if let MetaValue::MetaInlines(inlines) = plain_path {
        assert_eq!(inlines.len(), 1, "Expected exactly one inline");
        if let Inline::Span(span) = &inlines[0] {
            assert!(span.attr.1.contains(&"yaml-tagged-string".to_string()));
            assert_eq!(span.attr.2.get("tag"), Some(&"path".to_string()));
            // Extract the string content
            if let Inline::Str(s) = &span.content[0] {
                assert_eq!(s.text, "images/neovim-*.png");
            } else {
                panic!("Expected Str inline inside Span");
            }
        } else {
            panic!("Expected Span inline, got: {:?}", inlines[0]);
        }
    } else {
        panic!("Expected MetaInlines for plain_path");
    }

    // Check glob_pattern
    let glob_pattern = outer_meta
        .get("glob_pattern")
        .expect("glob_pattern not found");
    if let MetaValue::MetaInlines(inlines) = glob_pattern {
        if let Inline::Span(span) = &inlines[0] {
            assert_eq!(span.attr.2.get("tag"), Some(&"glob".to_string()));
            if let Inline::Str(s) = &span.content[0] {
                assert_eq!(s.text, "posts/*/index.qmd");
            }
        }
    }

    // Check literal_string
    let literal_string = outer_meta
        .get("literal_string")
        .expect("literal_string not found");
    if let MetaValue::MetaInlines(inlines) = literal_string {
        if let Inline::Span(span) = &inlines[0] {
            assert_eq!(span.attr.2.get("tag"), Some(&"str".to_string()));
            if let Inline::Str(s) = &span.content[0] {
                assert_eq!(s.text, "_foo_.py");
            }
        }
    }

    // Check regular_markdown - should have parsed markdown (Emph element)
    let regular_markdown = outer_meta
        .get("regular_markdown")
        .expect("regular_markdown not found");
    if let MetaValue::MetaInlines(inlines) = regular_markdown {
        // Should contain Emph for *emphasis*
        let has_emph = inlines
            .iter()
            .any(|inline| matches!(inline, Inline::Emph(_)));
        assert!(
            has_emph,
            "regular_markdown should have Emph element from *emphasis*"
        );
    } else {
        panic!("Expected MetaInlines for regular_markdown");
    }
}

#[test]
fn test_yaml_markdown_parse_failure() {
    // Test that untagged strings that fail markdown parsing are gracefully handled
    let content = fs::read_to_string("tests/yaml-markdown-parse-failure.qmd").unwrap();

    let block = RawBlock {
        format: "quarto_minus_metadata".to_string(),
        text: content,
        source_info: SourceInfo::with_range(Range {
            start: Location {
                offset: 0,
                row: 0,
                column: 0,
            },
            end: Location {
                offset: 0,
                row: 0,
                column: 0,
            },
        })
        .to_source_map_info(),
    };

    let mut meta = rawblock_to_meta(block);
    let mut outer_meta = LinkedHashMap::new();

    // Parse metadata strings - this should not panic
    for (k, v) in meta.drain() {
        let parsed = parse_metadata_strings(v, &mut outer_meta);
        outer_meta.insert(k, parsed);
    }

    // Check untagged_path - should be wrapped in error span
    let untagged_path = outer_meta
        .get("untagged_path")
        .expect("untagged_path not found");
    if let MetaValue::MetaInlines(inlines) = untagged_path {
        if let Inline::Span(span) = &inlines[0] {
            assert!(
                span.attr
                    .1
                    .contains(&"yaml-markdown-syntax-error".to_string())
            );
            if let Inline::Str(s) = &span.content[0] {
                assert_eq!(s.text, "posts/*/index.qmd");
            }
        } else {
            panic!("Expected Span inline for failed parse");
        }
    } else {
        panic!("Expected MetaInlines for untagged_path");
    }

    // Check another_glob - should also be wrapped in error span
    let another_glob = outer_meta
        .get("another_glob")
        .expect("another_glob not found");
    if let MetaValue::MetaInlines(inlines) = another_glob {
        if let Inline::Span(span) = &inlines[0] {
            assert!(
                span.attr
                    .1
                    .contains(&"yaml-markdown-syntax-error".to_string())
            );
            if let Inline::Str(s) = &span.content[0] {
                assert_eq!(s.text, "images/*.png");
            }
        }
    }

    // Check underscore_file - this one should successfully parse as markdown with Emph
    let underscore_file = outer_meta
        .get("underscore_file")
        .expect("underscore_file not found");
    if let MetaValue::MetaInlines(inlines) = underscore_file {
        // _foo_ should become Emph element
        let has_emph = inlines
            .iter()
            .any(|inline| matches!(inline, Inline::Emph(_)));
        assert!(
            has_emph,
            "underscore_file should have Emph element from _foo_"
        );
    }
}
