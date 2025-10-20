/*
 * test_yaml_tag_regression.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Tests for k-62: YAML tag information lost in new API
 */

use quarto_markdown_pandoc::pandoc::ast_context::ASTContext;
use quarto_markdown_pandoc::pandoc::location::{Location, Range, SourceInfo};
use quarto_markdown_pandoc::pandoc::meta::{
    MetaValueWithSourceInfo, parse_metadata_strings_with_source_info,
    rawblock_to_meta_with_source_info,
};
use quarto_markdown_pandoc::pandoc::{Inline, RawBlock};

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

    let context = ASTContext::default();
    let meta = rawblock_to_meta_with_source_info(&block, &context);

    let mut outer_meta = Vec::new();
    let parsed_meta = parse_metadata_strings_with_source_info(meta, &mut outer_meta);

    // Extract entries
    let entries = if let MetaValueWithSourceInfo::MetaMap { entries, .. } = parsed_meta {
        entries
    } else {
        panic!("Expected MetaMap");
    };

    // Check tagged_path - should be MetaInlines with Span wrapper
    let tagged_path_entry = entries
        .iter()
        .find(|e| e.key == "tagged_path")
        .expect("tagged_path not found");

    if let MetaValueWithSourceInfo::MetaInlines {
        content: inlines, ..
    } = &tagged_path_entry.value
    {
        assert_eq!(inlines.len(), 1, "Expected exactly one inline");
        if let Inline::Span(span) = &inlines[0] {
            // Should have yaml-tagged-string class
            assert!(
                span.attr.1.contains(&"yaml-tagged-string".to_string()),
                "Expected yaml-tagged-string class, found: {:?}",
                span.attr.1
            );
            // Should have tag attribute
            assert_eq!(
                span.attr.2.get("tag"),
                Some(&"path".to_string()),
                "Expected tag=path attribute"
            );
            // Extract the string content
            if let Inline::Str(s) = &span.content[0] {
                assert_eq!(s.text, "images/*.png");
            } else {
                panic!("Expected Str inline inside Span");
            }
        } else {
            panic!("Expected Span inline, got: {:?}", inlines[0]);
        }
    } else {
        panic!(
            "Expected MetaInlines for tagged_path, got: {:?}",
            tagged_path_entry.value
        );
    }

    // Check regular - should parse markdown normally (Emph element)
    let regular_entry = entries
        .iter()
        .find(|e| e.key == "regular")
        .expect("regular not found");

    if let MetaValueWithSourceInfo::MetaInlines {
        content: inlines, ..
    } = &regular_entry.value
    {
        let has_emph = inlines
            .iter()
            .any(|inline| matches!(inline, Inline::Emph(_)));
        assert!(has_emph, "regular should have Emph element from *emphasis*");
    } else {
        panic!("Expected MetaInlines for regular");
    }
}
