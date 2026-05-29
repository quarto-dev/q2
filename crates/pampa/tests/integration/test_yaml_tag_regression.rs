/*
 * test_yaml_tag_regression.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Tests for k-62: YAML tag information lost in new API
 *
 * Updated to use ConfigValue API (Phase 5 migration).
 */

use pampa::pandoc::location::{Location, Range, SourceInfo};
use pampa::pandoc::{Inline, RawBlock, rawblock_to_config_value};
use pampa::utils::diagnostic_collector::DiagnosticCollector;
use quarto_pandoc_types::ConfigValueKind;

#[test]
fn test_yaml_tags_preserved_in_new_api() {
    // Test YAML with tagged strings
    let yaml_content = r#"---
tagged_path: !path images/*.png
tagged_glob: !glob posts/*/index.qmd
tagged_str: !str _foo_.py
regular: This has *emphasis*
---"#;

    let block = RawBlock {
        format: "quarto_minus_metadata".to_string(),
        text: yaml_content.to_string(),
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
        panic!("Expected Map, got {:?}", config.value);
    };

    // Check tagged_path - should be Path variant
    let tagged_path_entry = entries
        .iter()
        .find(|e| e.key == "tagged_path")
        .expect("tagged_path not found");

    if let ConfigValueKind::Path(path) = &tagged_path_entry.value.value {
        assert_eq!(path, "images/*.png");
    } else {
        panic!(
            "Expected Path for tagged_path, got: {:?}",
            tagged_path_entry.value.value
        );
    }

    // Check tagged_glob - should be Glob variant
    let tagged_glob_entry = entries
        .iter()
        .find(|e| e.key == "tagged_glob")
        .expect("tagged_glob not found");

    if let ConfigValueKind::Glob(glob) = &tagged_glob_entry.value.value {
        assert_eq!(glob, "posts/*/index.qmd");
    } else {
        panic!(
            "Expected Glob for tagged_glob, got: {:?}",
            tagged_glob_entry.value.value
        );
    }

    // Check tagged_str - should be Scalar(String), not parsed as markdown
    let tagged_str_entry = entries
        .iter()
        .find(|e| e.key == "tagged_str")
        .expect("tagged_str not found");

    if let ConfigValueKind::Scalar(yaml_rust2::Yaml::String(s)) = &tagged_str_entry.value.value {
        assert_eq!(s, "_foo_.py");
    } else {
        panic!(
            "Expected Scalar(String) for tagged_str, got: {:?}",
            tagged_str_entry.value.value
        );
    }

    // Check regular - should be PandocInlines with Emph element from *emphasis*
    let regular_entry = entries
        .iter()
        .find(|e| e.key == "regular")
        .expect("regular not found");

    if let ConfigValueKind::PandocInlines(inlines) = &regular_entry.value.value {
        let has_emph = inlines
            .iter()
            .any(|inline| matches!(inline, Inline::Emph(_)));
        assert!(has_emph, "regular should have Emph element from *emphasis*");
    } else {
        panic!(
            "Expected PandocInlines for regular, got: {:?}",
            regular_entry.value.value
        );
    }
}
