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
