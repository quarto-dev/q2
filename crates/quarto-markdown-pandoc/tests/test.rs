/*
 * test.rs
 * Copyright (c) 2025 Posit, PBC
 */

use glob::glob;
use quarto_markdown_pandoc::errors::parse_is_good;
use quarto_markdown_pandoc::pandoc::{ASTContext, treesitter_to_pandoc};
use quarto_markdown_pandoc::utils::diagnostic_collector::DiagnosticCollector;
use quarto_markdown_pandoc::utils::output::VerboseOutput;
use quarto_markdown_pandoc::{readers, writers};
use std::io::{self, Write};
use std::process::{Command, Stdio};
use tree_sitter_qmd::MarkdownParser;

#[test]
fn unit_test_simple_qmd_parses() {
    let inputs = ["_hello_", "**bold**", "$e=mc^2$", "$$e=mc^2$$"];
    for input in inputs {
        let mut parser = MarkdownParser::default();
        let input_bytes = input.as_bytes();
        let tree = parser
            .parse(input_bytes, None)
            .expect("Failed to parse input");
        let mut buf = Vec::new();
        let mut error_collector = DiagnosticCollector::new();
        writers::native::write(
            &treesitter_to_pandoc(
                &mut std::io::sink(),
                &tree,
                &input_bytes,
                &ASTContext::anonymous(),
                &mut error_collector,
            )
            .unwrap(),
            &mut buf,
        )
        .unwrap();
        let ast = String::from_utf8(buf).expect("Invalid UTF-8 in output");
        println!("{}", &ast);
        assert!(true, "Parsed successfully");
    }
}

fn has_good_pandoc_version() -> bool {
    let output = Command::new("pandoc")
        .arg("--version")
        .output()
        .expect("Failed to execute pandoc command");
    let version_str = String::from_utf8_lossy(&output.stdout);
    version_str.contains("3.6") || version_str.contains("3.7") || version_str.contains("3.8")
}

fn canonicalize_pandoc_ast(ast: &str, from: &str, to: &str) -> String {
    let mut child = Command::new("pandoc")
        .arg("--from")
        .arg(from)
        .arg("--to")
        .arg(to)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("Failed to start pandoc process");
    let stdin = child.stdin.as_mut().unwrap();
    stdin
        .write_all(ast.as_bytes())
        .expect("Failed to write to stdin");
    let output = child.wait_with_output().expect("Failed to read stdout");
    String::from_utf8_lossy(&output.stdout).to_string()
}
fn matches_canonical_pandoc_format(
    markdown: &str,
    ast: &String,
    pandoc_reader: &str,
    output_format: &str,
) -> bool {
    if !has_good_pandoc_version() {
        return true; // Skip test if pandoc version is not suitable
    }
    let our_ast = canonicalize_pandoc_ast(ast, output_format, output_format);
    let pandoc_ast = canonicalize_pandoc_ast(markdown, pandoc_reader, output_format);
    if our_ast != pandoc_ast {
        eprintln!("Format: {} -> {}", pandoc_reader, output_format);
        eprintln!("Input:\n{}", markdown);
        eprintln!("Our AST:\n{}", our_ast);
        eprintln!("Pandoc AST:\n{}", pandoc_ast);
    }
    our_ast == pandoc_ast
}

fn matches_pandoc_markdown_reader(input: &str) -> bool {
    if !has_good_pandoc_version() {
        return true; // Skip test if pandoc version is not suitable
    }
    let mut buf1 = Vec::new();
    let mut buf2 = Vec::new();

    let (doc, context, _warnings) = readers::qmd::read(
        input.as_bytes(),
        false,
        "<input>",
        &mut std::io::sink(),
    )
    .unwrap();
    writers::native::write(&doc, &mut buf1).unwrap();
    let native_output = String::from_utf8(buf1).expect("Invalid UTF-8 in output");
    writers::json::write(&doc, &context, &mut buf2).unwrap();
    let json_output = String::from_utf8(buf2).expect("Invalid UTF-8 in output");

    let mut our_value: serde_json::Value =
        serde_json::from_str(&json_output).expect("Failed to parse our JSON");
    remove_location_fields(&mut our_value);
    let json_output = serde_json::to_string(&our_value).expect("Failed to serialize our JSON");

    matches_canonical_pandoc_format(input, &native_output, "markdown", "native")
        && matches_canonical_pandoc_format(input, &json_output, "markdown", "json")
}

fn matches_pandoc_commonmark_reader(input: &str) -> bool {
    if !has_good_pandoc_version() {
        return true; // Skip test if pandoc version is not suitable
    }
    let mut buf1 = Vec::new();
    let mut buf2 = Vec::new();
    let mut error_collector1 = DiagnosticCollector::new();
    writers::native::write(
        &treesitter_to_pandoc(
            &mut std::io::sink(),
            &MarkdownParser::default()
                .parse(input.as_bytes(), None)
                .unwrap(),
            input.as_bytes(),
            &ASTContext::anonymous(),
            &mut error_collector1,
        )
        .unwrap(),
        &mut buf1,
    )
    .unwrap();
    let native_output = String::from_utf8(buf1).expect("Invalid UTF-8 in output");
    let context_for_json = ASTContext::anonymous();
    let mut error_collector2 = DiagnosticCollector::new();
    writers::json::write(
        &treesitter_to_pandoc(
            &mut std::io::sink(),
            &MarkdownParser::default()
                .parse(input.as_bytes(), None)
                .unwrap(),
            input.as_bytes(),
            &context_for_json,
            &mut error_collector2,
        )
        .unwrap(),
        &context_for_json,
        &mut buf2,
    )
    .unwrap();
    let json_output = String::from_utf8(buf2).expect("Invalid UTF-8 in output");
    matches_canonical_pandoc_format(
        input,
        &native_output,
        "commonmark+strikeout+subscript+superscript",
        "native",
    ) && matches_canonical_pandoc_format(
        input,
        &json_output,
        "commonmark+strikeout+subscript+superscript",
        "json",
    )
}

#[test]
fn unit_test_corpus_matches_pandoc_markdown() {
    assert!(
        has_good_pandoc_version(),
        "Pandoc version is not suitable for testing"
    );
    let mut file_count = 0;
    for entry in
        glob("tests/pandoc-match-corpus/markdown/*.qmd").expect("Failed to read glob pattern")
    {
        match entry {
            Ok(path) => {
                eprintln!("Opening file: {}", path.display());
                let input = std::fs::read_to_string(&path).expect("Failed to read file");
                assert!(
                    matches_pandoc_markdown_reader(&input),
                    "File {} does not match pandoc markdown reader",
                    path.display()
                );
                file_count += 1;
            }
            Err(e) => panic!("Error reading glob entry: {}", e),
        }
    }
    assert!(
        file_count > 0,
        "No files found in tests/pandoc-match-corpus/markdown directory"
    );
}

#[test]
fn unit_test_corpus_matches_pandoc_commonmark() {
    assert!(
        has_good_pandoc_version(),
        "Pandoc version is not suitable for testing"
    );
    let mut file_count = 0;
    for entry in
        glob("tests/pandoc-match-corpus/commonmark/*.qmd").expect("Failed to read glob pattern")
    {
        match entry {
            Ok(path) => {
                eprintln!("Opening file: {}", path.display());
                let input = std::fs::read_to_string(&path).expect("Failed to read file");
                assert!(
                    matches_pandoc_commonmark_reader(&input),
                    "File {} does not match pandoc commonmark reader",
                    path.display()
                );
                file_count += 1;
            }
            Err(e) => panic!("Error reading glob entry: {}", e),
        }
    }
    assert!(
        file_count > 0,
        "No files found in tests/pandoc-match-corpus/commonmark directory"
    );
}

#[test]
fn unit_test_snapshots_native() {
    test_snapshots_for_format("native", |pandoc, _context, buffer| {
        writers::native::write(pandoc, buffer).map_err(|e| e.into())
    });
}

#[test]
fn unit_test_snapshots_qmd() {
    test_snapshots_for_format("qmd", |pandoc, _context, buffer| {
        writers::qmd::write(pandoc, buffer).map_err(|e| e.into())
    });
}

#[test]
fn unit_test_snapshots_json() {
    test_snapshots_for_format("json", |pandoc, context, buffer| {
        writers::json::write(pandoc, context, buffer).map_err(|e| e.into())
    });
}

fn test_snapshots_for_format<F>(format: &str, writer: F)
where
    F: Fn(
        &quarto_markdown_pandoc::pandoc::Pandoc,
        &quarto_markdown_pandoc::pandoc::ast_context::ASTContext,
        &mut Vec<u8>,
    ) -> Result<(), Box<dyn std::error::Error + 'static>>,
{
    let pattern = format!("tests/snapshots/{}/*.qmd", format);
    let mut file_count = 0;
    let mut failures = Vec::new();
    let mut updated_count = 0;

    // Check if we should update snapshots instead of comparing
    let update_snapshots = std::env::var("UPDATE_SNAPSHOTS")
        .map(|v| v == "1" || v.to_lowercase() == "true")
        .unwrap_or(false);

    for entry in glob(&pattern).expect("Failed to read glob pattern") {
        match entry {
            Ok(path) => {
                eprintln!("Opening file: {}", path.display());
                let snapshot_path = path.with_extension("qmd.snapshot");
                let mut buffer = Vec::new();
                let mut input = std::fs::read_to_string(&path).expect("Failed to read file");
                if !input.ends_with("\n") {
                    input.push('\n'); // ensure the input ends with a newline
                }
                let mut output_stream = VerboseOutput::Sink(io::sink());
                let (pandoc, context, _warnings) = readers::qmd::read(
                    input.as_bytes(),
                    false,
                    &path.to_string_lossy(),
                    &mut output_stream,
                ).unwrap();

                writer(&pandoc, &context, &mut buffer).unwrap();
                let output = String::from_utf8(buffer).expect("Invalid UTF-8 in output");

                if update_snapshots {
                    // Update mode: write the output to the snapshot file
                    std::fs::write(&snapshot_path, &output).unwrap_or_else(|_| {
                        panic!("Failed to write snapshot file {}", snapshot_path.display())
                    });
                    eprintln!("  Updated snapshot: {}", snapshot_path.display());
                    updated_count += 1;
                } else {
                    // Normal mode: compare output with snapshot
                    let snapshot = std::fs::read_to_string(&snapshot_path).unwrap_or_else(|_| {
                        panic!(
                            "Snapshot file {} does not exist, please create it",
                            snapshot_path.display()
                        )
                    });

                    if output.trim() != snapshot.trim() {
                        failures.push(format!(
                            "Snapshot mismatch for file: {}\n  Snapshot path: {}",
                            path.display(),
                            snapshot_path.display()
                        ));
                    }
                }
                file_count += 1;
            }
            Err(e) => panic!("Error reading glob entry: {}", e),
        }
    }

    assert!(
        file_count > 0,
        "No files found in tests/snapshots/{} directory",
        format
    );

    if update_snapshots {
        eprintln!(
            "\n✓ Updated {} snapshot(s) for format '{}'",
            updated_count, format
        );
    } else if !failures.is_empty() {
        panic!(
            "\n\n{} snapshot(s) failed for format '{}':\n\n{}\n",
            failures.len(),
            format,
            failures.join("\n")
        );
    }
}

fn remove_location_fields(json: &mut serde_json::Value) {
    if let Some(obj) = json.as_object_mut() {
        obj.remove("l"); // Remove the "l" field (old SourceInfo)
        obj.remove("s"); // Remove the "s" field (new quarto_source_map::SourceInfo)
        obj.remove("astContext"); // Remove the astContext field (includes metaTopLevelKeySources)
        for value in obj.values_mut() {
            remove_location_fields(value);
        }
    } else if let Some(array) = json.as_array_mut() {
        for item in array {
            remove_location_fields(item);
        }
    }
}

#[test]
fn test_json_writer() {
    assert!(
        has_good_pandoc_version(),
        "Pandoc version is not suitable for testing"
    );
    let mut file_count = 0;
    for entry in glob("tests/writers/json/*.md").expect("Failed to read glob pattern") {
        match entry {
            Ok(path) => {
                eprintln!("Opening file: {}", path.display());
                let markdown = std::fs::read_to_string(&path).expect("Failed to read file");

                // Parse with our parser
                let mut parser = MarkdownParser::default();
                let input_bytes = markdown.as_bytes();
                let tree = parser
                    .parse(input_bytes, None)
                    .expect("Failed to parse input");
                let test_context = ASTContext::anonymous();
                let mut error_collector = DiagnosticCollector::new();
                let pandoc = treesitter_to_pandoc(
                    &mut std::io::sink(),
                    &tree,
                    input_bytes,
                    &test_context,
                    &mut error_collector,
                )
                .unwrap();
                let mut buf = Vec::new();
                writers::json::write(&pandoc, &test_context, &mut buf).unwrap();
                let our_json = String::from_utf8(buf).expect("Invalid UTF-8 in our JSON output");

                // Get Pandoc's output
                let output = Command::new("pandoc")
                    .arg("-t")
                    .arg("json")
                    .arg("-f")
                    .arg("markdown")
                    .arg(&path)
                    .output()
                    .expect("Failed to execute pandoc");

                let pandoc_json = String::from_utf8(output.stdout).expect("Invalid UTF-8");

                // Parse both JSON outputs to compare
                let mut our_value: serde_json::Value =
                    serde_json::from_str(&our_json).expect("Failed to parse our JSON");
                let pandoc_value: serde_json::Value =
                    serde_json::from_str(&pandoc_json).expect("Failed to parse Pandoc JSON");
                remove_location_fields(&mut our_value);

                assert_eq!(
                    our_value,
                    pandoc_value,
                    "JSON outputs don't match for file {}.\nOurs:\n{}\nPandoc's:\n{}",
                    path.display(),
                    serde_json::to_string_pretty(&our_value).unwrap(),
                    serde_json::to_string_pretty(&pandoc_value).unwrap()
                );
                file_count += 1;
            }
            Err(e) => panic!("Error reading glob entry: {}", e),
        }
    }
    assert!(
        file_count > 0,
        "No files found in tests/writers/json directory"
    );
}

/// Normalize HTML for comparison by removing extra whitespace
fn normalize_html(html: &str) -> String {
    // First, join all lines with spaces to handle attributes split across lines
    let single_line = html
        .lines()
        .map(|line| line.trim())
        .collect::<Vec<_>>()
        .join(" ");

    // Then split by > to preserve tag boundaries
    single_line
        .split('>')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(">\n")
}

#[test]
fn test_html_writer() {
    assert!(
        has_good_pandoc_version(),
        "Pandoc version is not suitable for testing"
    );
    let mut file_count = 0;
    for entry in glob("tests/writers/html/*.md").expect("Failed to read glob pattern") {
        match entry {
            Ok(path) => {
                eprintln!("Opening file: {}", path.display());
                let markdown = std::fs::read_to_string(&path).expect("Failed to read file");

                // Parse with our parser
                let mut parser = MarkdownParser::default();
                let input_bytes = markdown.as_bytes();
                let tree = parser
                    .parse(input_bytes, None)
                    .expect("Failed to parse input");
                let mut error_collector = DiagnosticCollector::new();
                let pandoc = treesitter_to_pandoc(
                    &mut std::io::sink(),
                    &tree,
                    input_bytes,
                    &ASTContext::anonymous(),
                    &mut error_collector,
                )
                .unwrap();
                let mut buf = Vec::new();
                writers::html::write(&pandoc, &mut buf).unwrap();
                let our_html = String::from_utf8(buf).expect("Invalid UTF-8 in our HTML output");

                // Get Pandoc's output
                let output = Command::new("pandoc")
                    .arg("-t")
                    .arg("html")
                    .arg("-f")
                    .arg("markdown")
                    .arg(&path)
                    .output()
                    .expect("Failed to execute pandoc");

                let pandoc_html = String::from_utf8(output.stdout).expect("Invalid UTF-8");

                // Normalize both HTML outputs for comparison
                let our_normalized = normalize_html(&our_html);
                let pandoc_normalized = normalize_html(&pandoc_html);

                assert_eq!(
                    our_normalized,
                    pandoc_normalized,
                    "HTML outputs don't match for file {}.\n\nOurs:\n{}\n\nPandoc's:\n{}",
                    path.display(),
                    our_html,
                    pandoc_html
                );
                file_count += 1;
            }
            Err(e) => panic!("Error reading glob entry: {}", e),
        }
    }
    assert!(
        file_count > 0,
        "No files found in tests/writers/html directory"
    );
}

fn ensure_file_does_not_parse(path: &std::path::Path) {
    let markdown = std::fs::read_to_string(path).expect("Failed to read file");
    let mut parser = MarkdownParser::default();
    let input_bytes = markdown.as_bytes();
    let tree = parser
        .parse(input_bytes, None)
        .expect("Failed to parse input");

    let errors = parse_is_good(&tree);
    if errors.is_empty() {
        panic!(
            "File {} should not parse but it did: {:?}",
            path.display(),
            errors
        );
    }
}

fn ensure_every_file_in_directory_does_not_parse(pattern: &str) {
    let mut file_count = 0;
    for entry in glob(pattern).expect("Failed to read glob pattern") {
        match entry {
            Ok(path) => {
                eprintln!("Opening file: {}", path.display());
                ensure_file_does_not_parse(&path);
                file_count += 1;
            }
            Err(e) => panic!("Error reading glob entry: {}", e),
        }
    }
    assert!(
        file_count > 0,
        "No files found in directory matching pattern: {}",
        pattern
    );
}

#[test]
fn test_disallowed_in_qmd_fails() {
    ensure_every_file_in_directory_does_not_parse(
        "tests/pandoc-differences/disallowed-in-qmd/*.qmd",
    );
    ensure_every_file_in_directory_does_not_parse("tests/invalid-syntax/*.qmd");
}

#[test]
fn test_do_not_smoke() {
    let mut file_count = 0;
    for entry in glob("tests/smoke/*.qmd").expect("Failed to read glob pattern") {
        match entry {
            Ok(path) => {
                eprintln!("Opening file: {}", path.display());
                let markdown = std::fs::read_to_string(&path).expect("Failed to read file");

                // Parse with our parser
                let mut parser = MarkdownParser::default();
                let input_bytes = markdown.as_bytes();
                let tree = parser
                    .parse(input_bytes, None)
                    .expect("Failed to parse input");
                let mut error_collector = DiagnosticCollector::new();
                let _ = treesitter_to_pandoc(
                    &mut std::io::sink(),
                    &tree,
                    input_bytes,
                    &ASTContext::anonymous(),
                    &mut error_collector,
                );
                file_count += 1;
            }
            Err(e) => panic!("Error reading glob entry: {}", e),
        }
    }
    assert!(file_count > 0, "No files found in tests/smoke directory");
}

#[test]
fn test_markdown_writer_smoke() {
    // Smoke test: read markdown, produce AST, write it back out
    // Just verifying that the code runs without panicking
    for pattern in &["tests/writers/markdown/*.qmd", "tests/smoke/*.qmd"] {
        let mut file_count = 0;
        for entry in glob(pattern).expect("Failed to read glob pattern") {
            match entry {
                Ok(path) => {
                    eprintln!("Testing markdown writer on: {}", path.display());
                    let markdown = std::fs::read_to_string(&path).expect("Failed to read file");

                    // Parse with our qmd reader to get AST
                    let doc_result = readers::qmd::read(
                        markdown.as_bytes(),
                        false,
                        path.to_str().unwrap(),
                        &mut std::io::sink(),
                    );

                    match doc_result {
                        Ok((doc, _context, _warnings)) => {
                            // Write it back out using the markdown writer
                            let mut buf = Vec::new();
                            writers::qmd::write(&doc, &mut buf).expect("Failed to write markdown");

                            // Convert to string to ensure it's valid UTF-8
                            let _output = String::from_utf8(buf)
                                .expect("Invalid UTF-8 in markdown writer output");
                        }
                        Err(_) => {
                            // Skip files that have parse errors - they may be testing error cases
                            eprintln!("Skipping {} due to parse error", path.display());
                        }
                    }

                    file_count += 1;
                }
                Err(e) => panic!("Error reading glob entry: {}", e),
            }
        }
        assert!(file_count > 0, "No files found in {} glob", pattern);
    }
}

#[test]
fn test_qmd_roundtrip_consistency() {
    // Test that QMD -> JSON -> QMD produces consistent results
    let test_files =
        glob("tests/roundtrip_tests/qmd-json-qmd/*.qmd").expect("Failed to read glob pattern");

    let mut file_count = 0;
    let mut failures = Vec::new();

    for entry in test_files {
        match entry {
            Ok(path) => {
                eprintln!("Testing roundtrip consistency for: {}", path.display());
                let original_qmd = std::fs::read_to_string(&path).expect("Failed to read file");

                // Step 1: QMD -> JSON
                let (doc1, context1, _warnings) = readers::qmd::read(
                    original_qmd.as_bytes(),
                    false,
                    path.to_str().unwrap(),
                    &mut std::io::sink(),
                ).expect("Failed to parse original QMD");

                let mut json_buf = Vec::new();
                writers::json::write(&doc1, &context1, &mut json_buf)
                    .expect("Failed to write JSON");
                let json_str = String::from_utf8(json_buf).expect("Invalid UTF-8 in JSON");

                // Step 2: JSON -> QMD
                let (doc2, _context2) = readers::json::read(&mut json_str.as_bytes())
                    .expect("Failed to read JSON back");

                let mut qmd_buf = Vec::new();
                writers::qmd::write(&doc2, &mut qmd_buf).expect("Failed to write QMD");
                let regenerated_qmd = String::from_utf8(qmd_buf).expect("Invalid UTF-8 in QMD");

                // Step 3: QMD -> JSON again
                let (doc3, context3, _warnings) = readers::qmd::read(
                    regenerated_qmd.as_bytes(),
                    false,
                    "<generated>",
                    &mut std::io::sink(),
                ).expect("Failed to parse regenerated QMD");

                // Compare JSON representations (without location fields)
                let mut json1_buf = Vec::new();
                writers::json::write(&doc1, &context1, &mut json1_buf)
                    .expect("Failed to write JSON1");
                let json1_str = String::from_utf8(json1_buf).expect("Invalid UTF-8 in JSON1");
                let mut json1_value: serde_json::Value =
                    serde_json::from_str(&json1_str).expect("Failed to parse JSON1");
                remove_location_fields(&mut json1_value);

                let mut json3_buf = Vec::new();
                writers::json::write(&doc3, &context3, &mut json3_buf)
                    .expect("Failed to write JSON3");
                let json3_str = String::from_utf8(json3_buf).expect("Invalid UTF-8 in JSON3");
                let mut json3_value: serde_json::Value =
                    serde_json::from_str(&json3_str).expect("Failed to parse JSON3");
                remove_location_fields(&mut json3_value);

                if json1_value != json3_value {
                    failures.push(format!("Roundtrip failed for: {}", path.display()));
                }

                file_count += 1;
            }
            Err(e) => panic!("Error reading glob entry: {}", e),
        }
    }

    assert!(
        file_count > 0,
        "No files found in tests/roundtrip_tests/qmd-json-qmd/"
    );

    if !failures.is_empty() {
        panic!(
            "\n\n{} roundtrip test(s) failed:\n\n{}\n",
            failures.len(),
            failures.join("\n")
        );
    }
}

#[test]
fn test_empty_blockquote_roundtrip() {
    // Specific test for empty blockquote roundtrip consistency
    let test_file = "tests/roundtrip_tests/qmd-json-qmd/blockquote_with_elements.qmd";

    eprintln!("Testing nested blockquote roundtrip for: {}", test_file);
    let original_qmd = std::fs::read_to_string(test_file).expect("Failed to read file");

    // Step 1: QMD -> JSON
    let (doc1, context1, _warnings) = readers::qmd::read(
        original_qmd.as_bytes(),
        false,
        test_file,
        &mut std::io::sink(),
    )
    .expect("Failed to parse original QMD");

    let mut json_buf = Vec::new();
    writers::json::write(&doc1, &context1, &mut json_buf).expect("Failed to write JSON");
    let json_str = String::from_utf8(json_buf).expect("Invalid UTF-8 in JSON");

    // Step 2: JSON -> QMD
    let (doc2, _context2) =
        readers::json::read(&mut json_str.as_bytes()).expect("Failed to read JSON back");

    let mut qmd_buf = Vec::new();
    writers::qmd::write(&doc2, &mut qmd_buf).expect("Failed to write QMD");
    let regenerated_qmd = String::from_utf8(qmd_buf).expect("Invalid UTF-8 in QMD");

    // Step 3: QMD -> JSON again
    let (doc3, context3, _warnings) = readers::qmd::read(
        regenerated_qmd.as_bytes(),
        false,
        "<generated>",
        &mut std::io::sink(),
    )
    .expect("Failed to parse regenerated QMD");

    // Compare JSON representations (without location fields)
    let mut json1_buf = Vec::new();
    writers::json::write(&doc1, &context1, &mut json1_buf).expect("Failed to write JSON1");
    let json1_str = String::from_utf8(json1_buf).expect("Invalid UTF-8 in JSON1");
    let mut json1_value: serde_json::Value =
        serde_json::from_str(&json1_str).expect("Failed to parse JSON1");
    remove_location_fields(&mut json1_value);

    let mut json3_buf = Vec::new();
    writers::json::write(&doc3, &context3, &mut json3_buf).expect("Failed to write JSON3");
    let json3_str = String::from_utf8(json3_buf).expect("Invalid UTF-8 in JSON3");
    let mut json3_value: serde_json::Value =
        serde_json::from_str(&json3_str).expect("Failed to parse JSON3");
    remove_location_fields(&mut json3_value);

    if json1_value != json3_value {
        eprintln!("Empty blockquote roundtrip failed for: {}", test_file);
        eprintln!("Original QMD:\n{}", original_qmd);
        eprintln!("Regenerated QMD:\n{}", regenerated_qmd);
        eprintln!(
            "Original JSON (normalized):\n{}",
            serde_json::to_string_pretty(&json1_value).unwrap()
        );
        eprintln!(
            "Final JSON (normalized):\n{}",
            serde_json::to_string_pretty(&json3_value).unwrap()
        );
        panic!("Empty blockquote roundtrip consistency test failed");
    }
}
