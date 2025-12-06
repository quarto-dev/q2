//! Integration tests for the citeproc filter.
//!
//! These tests verify that the citeproc filter correctly processes citations
//! and generates bibliography entries with proper formatting.
//!
//! The tests use the binary's command-line interface to ensure end-to-end
//! correctness of the citeproc processing.

use std::fs;
use std::process::Command;

/// Get the path to the quarto-markdown-pandoc binary.
/// This assumes the tests are run via cargo test, which builds the binary.
fn get_binary_path() -> String {
    // The binary is in the target directory
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let target_dir = format!("{}/../..", manifest_dir);

    // Try debug first, then release
    let debug_path = format!("{}/target/debug/quarto-markdown-pandoc", target_dir);
    let release_path = format!("{}/target/release/quarto-markdown-pandoc", target_dir);

    if std::path::Path::new(&debug_path).exists() {
        debug_path
    } else if std::path::Path::new(&release_path).exists() {
        release_path
    } else {
        // Fall back to using cargo run
        panic!("Binary not found at {} or {}", debug_path, release_path);
    }
}

/// Test that bibliography entries have proper delimiters between elements.
///
/// This tests the fix for a bug where `<group delimiter=". ">` wrapping
/// a `<choose>` element would not apply delimiters between the choose
/// branch's children (author, date, title, etc.).
#[test]
fn test_bibliography_delimiters() {
    // Create a temporary file with our test content
    let test_dir = tempfile::tempdir().expect("Failed to create temp dir");
    let test_file = test_dir.path().join("test.qmd");

    let qmd_content = r#"---
title: Test Document
references:
- id: jones2019
  type: book
  author:
    - family: Jones
      given: Alice
  title: An Important Book
  publisher: Academic Press
  issued:
    date-parts:
      - [2019]
- id: smith2020
  type: article-journal
  author:
    - family: Smith
      given: John
  title: A Great Paper
  container-title: Journal of Examples
  issued:
    date-parts:
      - [2020]
  volume: 1
  page: 1-10
---

Here is a citation [@jones2019].
"#;

    fs::write(&test_file, qmd_content).expect("Failed to write test file");

    // Run the binary with citeproc filter
    let binary = get_binary_path();
    let output = Command::new(&binary)
        .args(["-F", "citeproc", "-t", "html", "-i"])
        .arg(&test_file)
        .output()
        .expect("Failed to execute binary");

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        panic!("Binary failed with: {}", stderr);
    }

    let html_output = String::from_utf8_lossy(&output.stdout);

    // Verify delimiters are present in bibliography
    // For book: "Jones, Alice. 2019. An Important Book. Academic Press."
    assert!(
        html_output.contains(". 2019"),
        "Missing delimiter before year in bibliography. Got: {}",
        html_output
    );
    assert!(
        html_output.contains("2019. "),
        "Missing delimiter after year in bibliography. Got: {}",
        html_output
    );

    // For article: delimiters between author, year, title, journal
    assert!(
        html_output.contains(". 2020"),
        "Missing delimiter before year in article entry. Got: {}",
        html_output
    );

    // Verify the bug is fixed: should NOT have run-together text
    assert!(
        !html_output.contains("Alice2019"),
        "Bug detected: author and year run together without delimiter. Got: {}",
        html_output
    );
    assert!(
        !html_output.contains("John2020"),
        "Bug detected: author and year run together without delimiter. Got: {}",
        html_output
    );
}

/// Test that bibliography works with the default Chicago author-date style.
#[test]
fn test_chicago_author_date_style() {
    let test_dir = tempfile::tempdir().expect("Failed to create temp dir");
    let test_file = test_dir.path().join("test.qmd");

    let qmd_content = r#"---
title: Chicago Style Test
references:
- id: test2023
  type: book
  author:
    - family: TestAuthor
      given: FirstName
  title: A Test Book Title
  publisher: Test Publisher
  issued:
    date-parts:
      - [2023]
---

[@test2023]
"#;

    fs::write(&test_file, qmd_content).expect("Failed to write test file");

    let binary = get_binary_path();
    let output = Command::new(&binary)
        .args(["-F", "citeproc", "-t", "html", "-i"])
        .arg(&test_file)
        .output()
        .expect("Failed to execute binary");

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        panic!("Binary failed with: {}", stderr);
    }

    let html_output = String::from_utf8_lossy(&output.stdout);

    // Chicago author-date format for books:
    // Author, Given. Year. Title. Publisher.
    assert!(
        html_output.contains("TestAuthor, FirstName. 2023. "),
        "Chicago style bibliography format incorrect. Got: {}",
        html_output
    );
}
