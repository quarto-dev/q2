/*
 * test_error_corpus.rs
 *
 * Tests to verify error messages from the error corpus produce proper output
 */

use regex::Regex;
use std::fs;
use std::path::PathBuf;

/// Test that all files in resources/error-corpus/*.qmd produce ariadne-formatted errors
/// with file:line:column information and source code snippets.
#[test]
fn test_error_corpus_ariadne_output() {
    let corpus_dir = PathBuf::from("resources/error-corpus");
    assert!(
        corpus_dir.exists(),
        "Error corpus directory should exist: {}",
        corpus_dir.display()
    );

    // Find all .qmd files in the error corpus
    let mut qmd_files: Vec<PathBuf> = fs::read_dir(&corpus_dir)
        .expect("Failed to read error corpus directory")
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("qmd") {
                Some(path)
            } else {
                None
            }
        })
        .collect();

    qmd_files.sort();

    assert!(
        !qmd_files.is_empty(),
        "Error corpus should contain at least one .qmd file"
    );

    // Compile regex once outside the loop
    // Pattern matches: filename.qmd:123:456 (where 123 is line, 456 is column)
    let location_pattern = Regex::new(r"\.qmd:\d+:\d+").expect("Invalid regex pattern");

    for qmd_file in &qmd_files {
        println!("Testing error corpus file: {}", qmd_file.display());

        let content = fs::read_to_string(qmd_file)
            .unwrap_or_else(|e| panic!("Failed to read {}: {}", qmd_file.display(), e));

        // Parse the file - we expect it to fail with diagnostics
        let result = quarto_markdown_pandoc::readers::qmd::read(
            content.as_bytes(),
            false, // not loose mode
            &qmd_file.to_string_lossy(),
            &mut std::io::sink(),
        );

        match result {
            Ok(_) => {
                panic!(
                    "Expected {} to produce errors, but it parsed successfully",
                    qmd_file.display()
                );
            }
            Err(diagnostics) => {
                assert!(
                    !diagnostics.is_empty(),
                    "Expected diagnostics for {}",
                    qmd_file.display()
                );

                // Create a SourceContext for rendering
                let mut source_context = quarto_source_map::SourceContext::new();
                source_context.add_file(qmd_file.to_string_lossy().to_string(), Some(content));

                // Render each diagnostic to text
                // Track whether at least one diagnostic has ariadne output
                let mut has_any_ariadne = false;

                for diagnostic in &diagnostics {
                    let text_output = diagnostic.to_text(Some(&source_context));

                    // Check if this diagnostic has ariadne output
                    // Ariadne uses box drawing characters for pretty printing
                    let has_box_chars = text_output.contains("│")
                        || text_output.contains("─")
                        || text_output.contains("╭")
                        || text_output.contains("╯");

                    if has_box_chars {
                        has_any_ariadne = true;

                        // If it has ariadne output, it should have file:line:column notation
                        assert!(
                            location_pattern.is_match(&text_output),
                            "Ariadne output for {} should contain file:line:column notation (pattern: .qmd:NUMBER:NUMBER). Got:\n{}",
                            qmd_file.display(),
                            text_output
                        );
                    }
                }

                // At least one diagnostic should have had ariadne output
                assert!(
                    has_any_ariadne,
                    "At least one diagnostic for {} should have ariadne output",
                    qmd_file.display()
                );
            }
        }
    }
}

/// Test that all files in resources/error-corpus/*.qmd produce JSON errors
/// with proper source location information (file_id and offsets).
#[test]
fn test_error_corpus_json_locations() {
    let corpus_dir = PathBuf::from("resources/error-corpus");
    assert!(
        corpus_dir.exists(),
        "Error corpus directory should exist: {}",
        corpus_dir.display()
    );

    // Find all .qmd files in the error corpus
    let mut qmd_files: Vec<PathBuf> = fs::read_dir(&corpus_dir)
        .expect("Failed to read error corpus directory")
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("qmd") {
                Some(path)
            } else {
                None
            }
        })
        .collect();

    qmd_files.sort();

    assert!(
        !qmd_files.is_empty(),
        "Error corpus should contain at least one .qmd file"
    );

    for qmd_file in &qmd_files {
        println!("Testing JSON error locations for: {}", qmd_file.display());

        let content = fs::read_to_string(qmd_file)
            .unwrap_or_else(|e| panic!("Failed to read {}: {}", qmd_file.display(), e));

        // Parse the file - we expect it to fail with diagnostics
        let result = quarto_markdown_pandoc::readers::qmd::read(
            content.as_bytes(),
            false, // not loose mode
            &qmd_file.to_string_lossy(),
            &mut std::io::sink(),
        );

        match result {
            Ok(_) => {
                panic!(
                    "Expected {} to produce errors, but it parsed successfully",
                    qmd_file.display()
                );
            }
            Err(diagnostics) => {
                assert!(
                    !diagnostics.is_empty(),
                    "Expected diagnostics for {}",
                    qmd_file.display()
                );

                // Check each diagnostic has location information
                for diagnostic in &diagnostics {
                    let json_value = diagnostic.to_json();

                    // Check that the main error has a location field
                    if json_value.get("location").is_some() {
                        let location = json_value.get("location").unwrap();

                        // Should have an Original variant with file_id and offsets
                        let original = location.get("Original");
                        assert!(
                            original.is_some(),
                            "Error location for {} should have Original variant. Got:\n{}",
                            qmd_file.display(),
                            serde_json::to_string_pretty(&json_value).unwrap()
                        );

                        let original = original.unwrap();
                        assert!(
                            original.get("file_id").is_some(),
                            "Error location for {} should have file_id. Got:\n{}",
                            qmd_file.display(),
                            serde_json::to_string_pretty(&json_value).unwrap()
                        );
                        assert!(
                            original.get("start_offset").is_some(),
                            "Error location for {} should have start_offset. Got:\n{}",
                            qmd_file.display(),
                            serde_json::to_string_pretty(&json_value).unwrap()
                        );
                        assert!(
                            original.get("end_offset").is_some(),
                            "Error location for {} should have end_offset. Got:\n{}",
                            qmd_file.display(),
                            serde_json::to_string_pretty(&json_value).unwrap()
                        );
                    }

                    // Check details also have location information
                    if let Some(details) = json_value.get("details").and_then(|d| d.as_array()) {
                        for detail in details {
                            if let Some(detail_loc) = detail.get("location") {
                                let original = detail_loc.get("Original");
                                assert!(
                                    original.is_some(),
                                    "Detail location for {} should have Original variant. Got:\n{}",
                                    qmd_file.display(),
                                    serde_json::to_string_pretty(&json_value).unwrap()
                                );
                            }
                        }
                    }
                }
            }
        }
    }
}
