//! Diagnostic test for yaml-tags.qmd nondeterminism investigation
//!
//! Run with: cargo nextest run -p pampa diagnostic_yaml_tags -- --nocapture
//!
//! This test outputs detailed information about the sourceInfoPool to help
//! diagnose platform-specific differences between Linux and macOS.

use pampa::readers;
use pampa::utils::output::VerboseOutput;
use pampa::writers;
use std::io;

#[test]
fn diagnostic_yaml_tags_source_info_pool() {
    eprintln!("\n============================================================");
    eprintln!("DIAGNOSTIC: yaml-tags.qmd sourceInfoPool analysis");
    eprintln!("============================================================\n");

    // Print platform info
    eprintln!("Platform: {}", std::env::consts::OS);
    eprintln!("Arch: {}", std::env::consts::ARCH);

    let test_file = "tests/snapshots/json/yaml-tags.qmd";
    let qmd_content = std::fs::read_to_string(test_file).expect("Failed to read test file");

    eprintln!("\n--- Input file content ---");
    eprintln!("{}", qmd_content);
    eprintln!("--- End input ---\n");

    // Parse QMD
    let mut output_stream = VerboseOutput::Sink(io::sink());
    let (pandoc, context, _warnings) = readers::qmd::read(
        qmd_content.as_bytes(),
        false,
        test_file,
        &mut output_stream,
        true,
        None,
    )
    .expect("Failed to parse QMD");

    // Write to JSON
    let mut buffer = Vec::new();
    writers::json::write(&pandoc, &context, &mut buffer).expect("Failed to write JSON");
    let json_output = String::from_utf8(buffer).expect("Invalid UTF-8");

    // Parse JSON to extract sourceInfoPool
    let json: serde_json::Value = serde_json::from_str(&json_output).expect("Failed to parse JSON");

    eprintln!("--- Full JSON output ---");
    eprintln!(
        "{}",
        serde_json::to_string_pretty(&json).unwrap_or_else(|_| json_output.clone())
    );
    eprintln!("--- End JSON ---\n");

    // Extract and analyze sourceInfoPool
    if let Some(ast_context) = json.get("astContext") {
        eprintln!("--- astContext analysis ---\n");

        // metaTopLevelKeySources
        if let Some(meta_sources) = ast_context.get("metaTopLevelKeySources") {
            eprintln!("metaTopLevelKeySources: {}", meta_sources);
        }

        // sourceInfoPool
        if let Some(pool) = ast_context.get("sourceInfoPool") {
            if let Some(pool_array) = pool.as_array() {
                eprintln!("\nsourceInfoPool size: {}", pool_array.len());
                eprintln!("\nsourceInfoPool entries:");

                for (i, entry) in pool_array.iter().enumerate() {
                    eprintln!("  [{}]: {}", i, entry);
                }

                // Check for duplicates by content
                eprintln!("\n--- Duplicate analysis ---");
                let mut seen: std::collections::HashMap<String, Vec<usize>> =
                    std::collections::HashMap::new();

                for (i, entry) in pool_array.iter().enumerate() {
                    let key = entry.to_string();
                    seen.entry(key).or_default().push(i);
                }

                let duplicates: Vec<_> = seen
                    .iter()
                    .filter(|(_, indices)| indices.len() > 1)
                    .collect();

                if duplicates.is_empty() {
                    eprintln!("No duplicate entries found in sourceInfoPool");
                } else {
                    eprintln!("Found {} entries with duplicates:", duplicates.len());
                    for (content, indices) in duplicates {
                        eprintln!("  Content: {} appears at indices: {:?}", content, indices);
                    }
                }
            }
        }
    }

    eprintln!("\n============================================================");
    eprintln!("END DIAGNOSTIC");
    eprintln!("============================================================\n");
}
