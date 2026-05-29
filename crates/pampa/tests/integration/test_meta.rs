/*
 * test_meta.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Updated to use ConfigValue API (Phase 5 migration).
 */

use pampa::pandoc::location::{Location, Range, SourceInfo};
use pampa::pandoc::{Inline, RawBlock, rawblock_to_config_value};
use pampa::utils::diagnostic_collector::DiagnosticCollector;
use quarto_pandoc_types::ConfigValueKind;
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

    let mut diagnostics = DiagnosticCollector::new();
    let config = rawblock_to_config_value(&block, &mut diagnostics);

    // Extract entries from ConfigValue
    let entries = if let ConfigValueKind::Map(entries) = &config.value {
        entries
    } else {
        panic!("Expected Map");
    };

    println!("Parsed metadata:");
    for entry in entries {
        println!("  {}: {:?}", entry.key, entry.value.value);
    }

    // Verify expected keys exist
    let has_key = |key: &str| entries.iter().any(|e| e.key == key);
    assert!(has_key("hello"), "Expected 'hello' key");
    assert!(has_key("array"), "Expected 'array' key");
    assert!(has_key("array2"), "Expected 'array2' key");
    assert!(has_key("complicated"), "Expected 'complicated' key");
    assert!(has_key("typed_values"), "Expected 'typed_values' key");

    // Verify types
    let get_entry = |key: &str| entries.iter().find(|e| e.key == key);

    // hello should be PandocInlines (parsed from string)
    let hello = get_entry("hello").expect("hello not found");
    assert!(
        matches!(hello.value.value, ConfigValueKind::PandocInlines(_)),
        "hello should be PandocInlines, got {:?}",
        hello.value.value
    );

    // array should be Array
    let array = get_entry("array").expect("array not found");
    assert!(
        matches!(array.value.value, ConfigValueKind::Array(_)),
        "array should be Array, got {:?}",
        array.value.value
    );

    // array2 should be Array
    let array2 = get_entry("array2").expect("array2 not found");
    assert!(
        matches!(array2.value.value, ConfigValueKind::Array(_)),
        "array2 should be Array, got {:?}",
        array2.value.value
    );

    // complicated should be PandocInlines (parsed from string)
    let complicated = get_entry("complicated").expect("complicated not found");
    assert!(
        matches!(complicated.value.value, ConfigValueKind::PandocInlines(_)),
        "complicated should be PandocInlines, got {:?}",
        complicated.value.value
    );

    // typed_values should be Array
    let typed_values = get_entry("typed_values").expect("typed_values not found");
    assert!(
        matches!(typed_values.value.value, ConfigValueKind::Array(_)),
        "typed_values should be Array, got {:?}",
        typed_values.value.value
    );
}

#[test]
fn test_yaml_tagged_strings() {
    // Test that YAML tags (!path, !glob, !str) produce correct ConfigValue variants
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

    let mut diagnostics = DiagnosticCollector::new();
    let config = rawblock_to_config_value(&block, &mut diagnostics);

    // Extract entries from ConfigValue
    let entries = if let ConfigValueKind::Map(entries) = &config.value {
        entries
    } else {
        panic!("Expected Map");
    };

    let get_entry = |key: &str| entries.iter().find(|e| e.key == key);

    // Check plain_path - should be Path variant
    let plain_path = get_entry("plain_path").expect("plain_path not found");
    if let ConfigValueKind::Path(path) = &plain_path.value.value {
        assert_eq!(path, "images/neovim-*.png");
    } else {
        panic!(
            "Expected Path for plain_path, got: {:?}",
            plain_path.value.value
        );
    }

    // Check glob_pattern - should be Glob variant
    let glob_pattern = get_entry("glob_pattern").expect("glob_pattern not found");
    if let ConfigValueKind::Glob(glob) = &glob_pattern.value.value {
        assert_eq!(glob, "posts/*/index.qmd");
    } else {
        panic!(
            "Expected Glob for glob_pattern, got: {:?}",
            glob_pattern.value.value
        );
    }

    // Check literal_string - should be Scalar(String), not parsed as markdown
    let literal_string = get_entry("literal_string").expect("literal_string not found");
    if let ConfigValueKind::Scalar(yaml_rust2::Yaml::String(s)) = &literal_string.value.value {
        assert_eq!(s, "_foo_.py");
    } else {
        panic!(
            "Expected Scalar(String) for literal_string, got: {:?}",
            literal_string.value.value
        );
    }

    // Check regular_markdown - should be PandocInlines with Emph element
    let regular_markdown = get_entry("regular_markdown").expect("regular_markdown not found");
    if let ConfigValueKind::PandocInlines(inlines) = &regular_markdown.value.value {
        let has_emph = inlines
            .iter()
            .any(|inline| matches!(inline, Inline::Emph(_)));
        assert!(
            has_emph,
            "regular_markdown should have Emph element from *emphasis*"
        );
    } else {
        panic!(
            "Expected PandocInlines for regular_markdown, got: {:?}",
            regular_markdown.value.value
        );
    }
}

#[test]
fn test_yaml_markdown_parse_behavior() {
    // Test how untagged strings that contain special characters are handled
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

    let mut diagnostics = DiagnosticCollector::new();
    let config = rawblock_to_config_value(&block, &mut diagnostics);

    // Extract entries from ConfigValue
    let entries = if let ConfigValueKind::Map(entries) = &config.value {
        entries
    } else {
        panic!("Expected Map");
    };

    let get_entry = |key: &str| entries.iter().find(|e| e.key == key);

    // untagged_path: posts/*/index.qmd
    // This should either:
    // 1. Parse successfully as markdown (PandocInlines with text content)
    // 2. Fail and be wrapped in error span (PandocInlines with Span containing yaml-markdown-syntax-error)
    let untagged_path = get_entry("untagged_path").expect("untagged_path not found");
    assert!(
        matches!(untagged_path.value.value, ConfigValueKind::PandocInlines(_)),
        "untagged_path should be PandocInlines, got: {:?}",
        untagged_path.value.value
    );

    // another_glob: images/*.png
    let another_glob = get_entry("another_glob").expect("another_glob not found");
    assert!(
        matches!(another_glob.value.value, ConfigValueKind::PandocInlines(_)),
        "another_glob should be PandocInlines, got: {:?}",
        another_glob.value.value
    );

    // underscore_file: _foo_.py - should parse as markdown with Emph from _foo_
    let underscore_file = get_entry("underscore_file").expect("underscore_file not found");
    if let ConfigValueKind::PandocInlines(inlines) = &underscore_file.value.value {
        let has_emph = inlines
            .iter()
            .any(|inline| matches!(inline, Inline::Emph(_)));
        assert!(
            has_emph,
            "underscore_file should have Emph element from _foo_"
        );
    } else {
        panic!(
            "Expected PandocInlines for underscore_file, got: {:?}",
            underscore_file.value.value
        );
    }
}
