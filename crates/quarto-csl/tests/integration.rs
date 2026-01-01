//! Integration tests for quarto-csl.
//!
//! These tests verify that the parser can handle real-world CSL files.
//! Test CSL files are stored in test-data/ within this crate.

use quarto_csl::parse_csl;
use std::fs;
use std::path::PathBuf;

/// Get the test-data directory path.
fn test_data_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test-data")
}

/// Test parsing the APA style.
#[test]
fn test_parse_apa_style() {
    let csl_path = test_data_dir().join("apa.csl");
    let content = fs::read_to_string(&csl_path).expect("Failed to read APA CSL");
    let style = parse_csl(&content).expect("Failed to parse APA CSL");

    assert_eq!(style.version, "1.0");
    assert_eq!(style.class, quarto_csl::StyleClass::InText);
    assert!(style.info.is_some());

    let info = style.info.as_ref().unwrap();
    assert!(
        info.title
            .as_ref()
            .unwrap()
            .contains("American Psychological Association")
    );

    // APA has many macros
    assert!(!style.macros.is_empty());
    assert!(style.macros.contains_key("author"));
    assert!(style.macros.contains_key("author-short"));
    assert!(style.macros.contains_key("title"));
}

/// Test parsing the IEEE style.
#[test]
fn test_parse_ieee_style() {
    let csl_path = test_data_dir().join("ieee.csl");
    let content = fs::read_to_string(&csl_path).expect("Failed to read IEEE CSL");
    let style = parse_csl(&content).expect("Failed to parse IEEE CSL");

    assert_eq!(style.version, "1.0");
}

/// Test parsing the Chicago note-bibliography style.
#[test]
fn test_parse_chicago_note_style() {
    let csl_path = test_data_dir().join("chicago-note-bibliography.csl");
    let content = fs::read_to_string(&csl_path).expect("Failed to read Chicago CSL");
    let style = parse_csl(&content).expect("Failed to parse Chicago CSL");

    // Chicago note style uses note class
    assert_eq!(style.class, quarto_csl::StyleClass::Note);
}

/// Test parsing the default CSL style.
#[test]
fn test_parse_default_style() {
    let csl_path = test_data_dir().join("default.csl");
    let content = fs::read_to_string(&csl_path).expect("Failed to read default CSL");
    let style = parse_csl(&content).expect("Failed to parse default CSL");

    assert_eq!(style.version, "1.0");
}

/// Test that all CSL files in test-data parse successfully.
#[test]
fn test_parse_all_test_data_csl_files() {
    let test_data = test_data_dir();

    let csl_files: Vec<PathBuf> = fs::read_dir(&test_data)
        .expect("Failed to read test-data directory")
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|ext| ext == "csl"))
        .collect();

    assert!(
        !csl_files.is_empty(),
        "No CSL files found in test-data directory"
    );

    let mut failures = Vec::new();

    for csl_path in &csl_files {
        match fs::read_to_string(csl_path) {
            Ok(content) => match parse_csl(&content) {
                Ok(_) => {}
                Err(e) => {
                    failures.push((csl_path.clone(), format!("{}", e)));
                }
            },
            Err(e) => {
                failures.push((csl_path.clone(), format!("Read error: {}", e)));
            }
        }
    }

    if !failures.is_empty() {
        let mut msg = format!(
            "Failed to parse {} of {} CSL files:\n",
            failures.len(),
            csl_files.len()
        );
        for (path, err) in &failures {
            msg.push_str(&format!("  {:?}: {}\n", path, err));
        }
        panic!("{}", msg);
    }

    println!(
        "Successfully parsed all {} CSL files in test-data",
        csl_files.len()
    );
}
