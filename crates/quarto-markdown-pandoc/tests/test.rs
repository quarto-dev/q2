/*
 * test.rs
 * Copyright (c) 2025 Posit, PBC
 */

use glob::glob;
use quarto_markdown_pandoc::errors::parse_is_good;
use quarto_markdown_pandoc::pandoc::treesitter_to_pandoc;
use quarto_markdown_pandoc::{readers, writers};
use std::io::Write;
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
        writers::native::write(
            &treesitter_to_pandoc(&mut std::io::sink(), &tree, &input_bytes).unwrap(),
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

    let doc = readers::qmd::read(
        input.as_bytes(), 
        false, 
        "<input>", 
        &mut std::io::sink(), 
        None::<fn(&[u8], &quarto_markdown_pandoc::utils::tree_sitter_log_observer::TreeSitterLogObserver, &str) -> Vec<String>>
    ).unwrap();
    writers::native::write(&doc, &mut buf1).unwrap();
    let native_output = String::from_utf8(buf1).expect("Invalid UTF-8 in output");
    writers::json::write(&doc, &mut buf2).unwrap();
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
    writers::native::write(
        &treesitter_to_pandoc(
            &mut std::io::sink(),
            &MarkdownParser::default()
                .parse(input.as_bytes(), None)
                .unwrap(),
            input.as_bytes(),
        )
        .unwrap(),
        &mut buf1,
    )
    .unwrap();
    let native_output = String::from_utf8(buf1).expect("Invalid UTF-8 in output");
    writers::json::write(
        &treesitter_to_pandoc(
            &mut std::io::sink(),
            &MarkdownParser::default()
                .parse(input.as_bytes(), None)
                .unwrap(),
            input.as_bytes(),
        )
        .unwrap(),
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
fn unit_test_snapshots() {
    let mut file_count = 0;
    for entry in glob("tests/snapshots/*.qmd").expect("Failed to read glob pattern") {
        match entry {
            Ok(path) => {
                eprintln!("Opening file: {}", path.display());
                let input = std::fs::read_to_string(&path).expect("Failed to read file");
                let snapshot_path = path.with_extension("qmd.snapshot");
                let mut buffer = Vec::new();
                writers::native::write(
                    &treesitter_to_pandoc(
                        &mut std::io::sink(),
                        &MarkdownParser::default()
                            .parse(input.as_bytes(), None)
                            .unwrap(),
                        input.as_bytes(),
                    )
                    .unwrap(),
                    &mut buffer,
                )
                .unwrap();
                let ast = String::from_utf8(buffer).expect("Invalid UTF-8 in output");
                let snapshot = std::fs::read_to_string(&snapshot_path).unwrap_or_else(|_| {
                    panic!(
                        "Snapshot file {} does not exist, please create it",
                        snapshot_path.display()
                    )
                });
                assert_eq!(
                    ast.trim(),
                    snapshot.trim(),
                    "Snapshot mismatch for file {}",
                    path.display()
                );
                file_count += 1;
            }
            Err(e) => panic!("Error reading glob entry: {}", e),
        }
    }
    assert!(
        file_count > 0,
        "No files found in tests/snapshots directory"
    );
}

fn remove_location_fields(json: &mut serde_json::Value) {
    if let Some(obj) = json.as_object_mut() {
        obj.remove("l"); // Remove the "l" field
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
                let pandoc =
                    treesitter_to_pandoc(&mut std::io::sink(), &tree, input_bytes).unwrap();
                let mut buf = Vec::new();
                writers::json::write(&pandoc, &mut buf).unwrap();
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
                let _ = treesitter_to_pandoc(&mut std::io::sink(), &tree, input_bytes);
                file_count += 1;
            }
            Err(e) => panic!("Error reading glob entry: {}", e),
        }
    }
    assert!(file_count > 0, "No files found in tests/smoke directory");
}
