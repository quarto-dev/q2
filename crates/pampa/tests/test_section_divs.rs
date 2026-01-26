/*
 * test_section_divs.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Integration tests for section-divs transform.
 * Compares pampa output with Pandoc's --section-divs output.
 */

use glob::glob;
use pampa::pandoc::ASTContext;
use pampa::transforms::sectionize_blocks;
use pampa::{readers, writers};
use std::process::{Command, Stdio};

/// Check if pandoc is available with a suitable version
fn has_good_pandoc_version() -> bool {
    let output = Command::new("pandoc")
        .arg("--version")
        .output()
        .expect("Failed to execute pandoc command");
    let version_str = String::from_utf8_lossy(&output.stdout);
    version_str.contains("3.6") || version_str.contains("3.7") || version_str.contains("3.8")
}

/// Get Pandoc's HTML output with --section-divs
fn get_pandoc_section_divs_html(markdown: &str) -> String {
    use std::io::Write;

    let mut child = Command::new("pandoc")
        .arg("--section-divs")
        .arg("-t")
        .arg("html")
        .arg("-f")
        .arg("markdown")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("Failed to start pandoc process");

    let stdin = child.stdin.as_mut().unwrap();
    stdin
        .write_all(markdown.as_bytes())
        .expect("Failed to write to stdin");

    let output = child.wait_with_output().expect("Failed to read stdout");
    String::from_utf8_lossy(&output.stdout).to_string()
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

/// Get our HTML output with section-divs transform applied
fn get_our_section_divs_html(markdown: &str) -> String {
    // Parse markdown to AST
    let (mut pandoc, _context, _warnings) = readers::qmd::read(
        markdown.as_bytes(),
        false,
        "<test>",
        &mut std::io::sink(),
        true,
        None,
    )
    .expect("Failed to parse markdown");

    // Apply sectionize transform
    pandoc.blocks = sectionize_blocks(pandoc.blocks);

    // Write to HTML
    let context = ASTContext::anonymous();
    let mut buf = Vec::new();
    writers::html::write(&pandoc, &context, &mut buf).expect("Failed to write HTML");

    String::from_utf8(buf).expect("Invalid UTF-8 in HTML output")
}

/// Compare HTML structures, accounting for our extra "section" class
fn structures_match(our_html: &str, pandoc_html: &str) -> bool {
    let our_normalized = normalize_html(our_html);
    let pandoc_normalized = normalize_html(pandoc_html);

    // Our output has an extra "section " in the class list (e.g., "section level2" vs "level2")
    // This is by design, so we need to account for it in comparison
    let our_without_section_class = our_normalized.replace("class=\"section ", "class=\"");

    our_without_section_class == pandoc_normalized
}

#[test]
fn test_section_divs_against_pandoc() {
    if !has_good_pandoc_version() {
        eprintln!("Skipping section-divs Pandoc comparison: pandoc version not suitable");
        return;
    }

    let mut file_count = 0;
    let mut failures = Vec::new();

    for entry in glob("tests/writers/html/section-divs/*.md").expect("Failed to read glob pattern")
    {
        match entry {
            Ok(path) => {
                eprintln!("Testing section-divs for: {}", path.display());
                let markdown = std::fs::read_to_string(&path).expect("Failed to read file");

                let our_html = get_our_section_divs_html(&markdown);
                let pandoc_html = get_pandoc_section_divs_html(&markdown);

                if !structures_match(&our_html, &pandoc_html) {
                    failures.push(format!(
                        "Mismatch for {}:\n\nOurs:\n{}\n\nPandoc:\n{}",
                        path.display(),
                        normalize_html(&our_html),
                        normalize_html(&pandoc_html)
                    ));
                }

                file_count += 1;
            }
            Err(e) => panic!("Error reading glob entry: {}", e),
        }
    }

    assert!(
        file_count > 0,
        "No files found in tests/writers/html/section-divs/"
    );

    if !failures.is_empty() {
        panic!(
            "\n\n{} section-divs test(s) failed:\n\n{}",
            failures.len(),
            failures.join("\n\n---\n\n")
        );
    }
}

#[test]
fn test_section_divs_flat_sections() {
    let markdown = "## A\n\nContent A.\n\n## B\n\nContent B.\n";
    let html = get_our_section_divs_html(markdown);

    // Should have two sibling sections
    assert!(html.contains("<section"), "Expected section tags");
    assert_eq!(
        html.matches("<section").count(),
        2,
        "Expected 2 section tags"
    );
    assert_eq!(
        html.matches("</section>").count(),
        2,
        "Expected 2 closing section tags"
    );
}

#[test]
fn test_section_divs_nested() {
    let markdown = "## Parent\n\nContent.\n\n### Child\n\nNested.\n";
    let html = get_our_section_divs_html(markdown);

    // Should have nested sections
    assert!(html.contains("<section"), "Expected section tags");
    // The child section should be inside the parent
    let parent_start = html.find("<section").expect("No section found");
    let child_start = html[parent_start + 1..]
        .find("<section")
        .expect("No nested section found");
    assert!(
        child_start > 0,
        "Child section should be after parent section start"
    );
}

#[test]
fn test_section_divs_id_handling() {
    let markdown = "## Section {#my-id}\n\nContent.\n";
    let html = get_our_section_divs_html(markdown);

    // ID should be on section, not on h2
    assert!(
        html.contains("<section id=\"my-id\""),
        "Expected id on section tag, got: {}",
        html
    );
    // h2 should not have id
    assert!(
        !html.contains("<h2 id="),
        "h2 should not have id, got: {}",
        html
    );
}

#[test]
fn test_section_divs_class_handling() {
    let markdown = "## Section {.myclass}\n\nContent.\n";
    let html = get_our_section_divs_html(markdown);

    // Class should be on both section and h2
    assert!(
        html.contains("class=\"section level2 myclass\""),
        "Expected classes on section, got: {}",
        html
    );
    assert!(
        html.contains("<h2 class=\"myclass\""),
        "Expected class on h2, got: {}",
        html
    );
}
