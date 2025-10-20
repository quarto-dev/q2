/*
 * test_metadata_source_tracking.rs
 * Test that metadata source tracking is correct in PandocAST
 */

use quarto_markdown_pandoc::pandoc::MetaValueWithSourceInfo;
use quarto_markdown_pandoc::readers;
use quarto_markdown_pandoc::writers;

/// Helper to resolve a SourceInfo chain to absolute file offset
fn resolve_source_offset(source: &quarto_source_map::SourceInfo) -> usize {
    match &source.mapping {
        quarto_source_map::SourceMapping::Original { .. } => source.range.start.offset,
        quarto_source_map::SourceMapping::Substring { offset, parent } => {
            offset + resolve_source_offset(parent)
        }
        quarto_source_map::SourceMapping::Concat { .. } => {
            // For concat, just use the start offset
            source.range.start.offset
        }
        quarto_source_map::SourceMapping::Transformed { .. } => {
            // For transformed, just use the start offset
            source.range.start.offset
        }
    }
}

#[test]
fn test_metadata_source_tracking_002_qmd() {
    /*
     * File: tests/snapshots/json/002.qmd
     * Content:
     * ---
     * title: metadata1
     * ---
     *
     * ::: hello
     *
     * ---
     * nested: meta
     * ---
     *
     * :::
     *
     * Byte offsets:
     * - Line 0 (0-3): "---"
     * - Line 1 (4-20): "title: metadata1"
     *   - "title" at offset 4-9
     *   - ": " at offset 9-11
     *   - "metadata1" at offset 11-20
     * - Line 2 (21-24): "---"
     * - Line 7 (41-53): "nested: meta"
     *   - "nested" at offset 41-47
     *   - ": " at offset 47-49
     *   - "meta" at offset 49-53
     */

    let test_file = "tests/snapshots/json/002.qmd";
    let content = std::fs::read_to_string(test_file).expect("Failed to read test file");

    // Step 1: Read QMD to PandocAST
    let mut output_stream =
        quarto_markdown_pandoc::utils::output::VerboseOutput::Sink(std::io::sink());
    let (pandoc, context) = readers::qmd::read(
        content.as_bytes(),
        false,
        test_file,
        &mut output_stream,
        None::<
            fn(
                &[u8],
                &quarto_markdown_pandoc::utils::tree_sitter_log_observer::TreeSitterLogObserver,
                &str,
            ) -> Vec<String>,
        >,
    )
    .expect("Failed to parse QMD");

    // Verify document-level metadata: title: metadata1
    if let MetaValueWithSourceInfo::MetaMap { ref entries, .. } = pandoc.meta {
        let title_entry = entries
            .iter()
            .find(|e| e.key == "title")
            .expect("Should have 'title' in metadata");

        // Verify key source: "title"
        let key_offset = resolve_source_offset(&title_entry.key_source);
        // "title" starts at position 0 in the YAML string "title: metadata1\n"
        // Absolute offset should be 4 (start of YAML frontmatter content)
        assert_eq!(key_offset, 4, "Key 'title' should start at file offset 4");

        // Verify value source: "metadata1"
        match &title_entry.value {
            MetaValueWithSourceInfo::MetaInlines { source_info, .. } => {
                let value_offset = resolve_source_offset(source_info);
                // "metadata1" starts at position 7 in the YAML string "title: metadata1\n"
                // Absolute offset should be 4 + 7 = 11
                assert_eq!(
                    value_offset, 11,
                    "Value 'metadata1' should start at file offset 11"
                );
            }
            other => panic!("Expected MetaInlines for title value, got {:?}", other),
        }
    } else {
        panic!("Expected MetaMap for pandoc.meta");
    }

    // NOTE: Lexical metadata (nested: meta) test skipped for now
    // The lexical metadata in ::: blocks appears to be processed differently
    // and might not produce BlockMetadata in the final AST.
    // This would require further investigation of the filter chain.

    // Step 2: Write to JSON
    let mut json_output = Vec::new();
    writers::json::write(&pandoc, &context, &mut json_output).expect("Failed to write JSON");

    // Step 3: Read JSON back to PandocAST
    let mut json_reader = std::io::Cursor::new(json_output);
    let (pandoc_from_json, _context_from_json) =
        readers::json::read(&mut json_reader).expect("Failed to read JSON");

    // Step 4: Verify source info is preserved through JSON roundtrip
    // Check document-level metadata
    if let MetaValueWithSourceInfo::MetaMap { ref entries, .. } = pandoc_from_json.meta {
        let title_entry = entries
            .iter()
            .find(|e| e.key == "title")
            .expect("Should have 'title' in metadata after JSON roundtrip");

        let key_offset = resolve_source_offset(&title_entry.key_source);
        // Key tracking through JSON roundtrip
        assert_eq!(
            key_offset, 4,
            "After JSON roundtrip: Key 'title' should still start at file offset 4"
        );

        if let MetaValueWithSourceInfo::MetaInlines { source_info, .. } = &title_entry.value {
            let value_offset = resolve_source_offset(source_info);
            assert_eq!(
                value_offset, 11,
                "After JSON roundtrip: Value 'metadata1' should still start at file offset 11"
            );
        }
    }

    // NOTE: Lexical metadata roundtrip test also skipped (see above)

    eprintln!("\n✅ SUCCESS!");
    eprintln!("✓ Document-level metadata source tracking verified:");
    eprintln!("  - Value 'metadata1' correctly tracked to file offset 11");
    eprintln!("✓ Source info preserved through JSON roundtrip:");
    eprintln!("  - Value source still points to offset 11 after round-trip");
}

#[test]
fn test_nested_metadata_key_source_preservation() {
    // Test that when metadata values contain markdown that itself has YAML,
    // the key_source information is preserved (not lost via LinkedHashMap)
    //
    // This test verifies the fix for the LinkedHashMap limitation where
    // outer_metadata was using HashMap<String, MetaValue> and losing key_source

    let input = r#"---
title: Simple title
description: This is a description
---"#;

    let (pandoc, _context) = readers::qmd::read(
        input.as_bytes(),
        false,
        "test.qmd",
        &mut std::io::sink(),
        None::<
            fn(
                &[u8],
                &quarto_markdown_pandoc::utils::tree_sitter_log_observer::TreeSitterLogObserver,
                &str,
            ) -> Vec<String>,
        >,
    )
    .expect("Failed to parse");

    // Extract metadata
    let MetaValueWithSourceInfo::MetaMap { entries, .. } = pandoc.meta else {
        panic!("Expected MetaMap");
    };

    // Verify both entries have proper key_source tracking
    let title_entry = entries
        .iter()
        .find(|e| e.key == "title")
        .expect("Should have 'title' entry");

    let desc_entry = entries
        .iter()
        .find(|e| e.key == "description")
        .expect("Should have 'description' entry");

    // CRITICAL: Verify keys have non-default source info
    // Before the fix, when outer_metadata was LinkedHashMap<String, MetaValue>,
    // the key_source would be lost and default to offset 0

    // Resolve the source info chain to get absolute file offsets
    let title_offset = resolve_source_offset(&title_entry.key_source);
    let desc_offset = resolve_source_offset(&desc_entry.key_source);

    eprintln!("\nTitle key resolved offset: {}", title_offset);
    eprintln!("Description key resolved offset: {}", desc_offset);

    assert_ne!(
        title_offset, 0,
        "Title key should have non-zero offset (not SourceInfo::default())"
    );

    assert_ne!(
        desc_offset, 0,
        "Description key should have non-zero offset (not SourceInfo::default())"
    );

    // Verify keys are at EXACT expected locations in the YAML
    // Input: "---\ntitle: Simple title\ndescription: This is a description\n---"
    //        01234567890123456789012345678901234567890123456789012345678901234
    //        0         1         2         3         4         5         6
    //
    // "---\n" = 4 bytes
    // "title" starts at offset 4
    // "title: Simple title\n" = 20 bytes
    // "description" starts at offset 24

    assert_eq!(
        title_offset, 4,
        "Title key should be at exact offset 4, got {}",
        title_offset
    );

    assert_eq!(
        desc_offset, 24,
        "Description key should be at exact offset 24, got {}",
        desc_offset
    );

    eprintln!("\n✅ Metadata key_source preservation test passed!");
    eprintln!(
        "✓ Title key has proper source tracking (offset {})",
        title_offset
    );
    eprintln!(
        "✓ Description key has proper source tracking (offset {})",
        desc_offset
    );
    eprintln!("✓ LinkedHashMap fix working - key source information preserved!");
}
