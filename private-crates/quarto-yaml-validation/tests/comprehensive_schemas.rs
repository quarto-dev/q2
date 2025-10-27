//! Comprehensive tests parsing ALL schemas from quarto-cli schema files
//!
//! These tests attempt to parse every schema definition from the quarto-cli
//! schema files to ensure compatibility and identify any unsupported patterns.

use quarto_yaml_validation::Schema;
use std::collections::HashMap;

/// Helper to extract and parse schemas from a field-based YAML file
/// (document-execute.yml, document-text.yml, etc.)
fn parse_field_schemas(content: &str, file_name: &str) -> (usize, usize, Vec<(String, String)>) {
    let yaml = quarto_yaml::parse(content).expect("Failed to parse YAML");
    let items = yaml.as_array().expect("Expected array at root");

    let mut success_count = 0;
    let mut total_count = 0;
    let mut failures = Vec::new();

    for item in items {
        let name = item
            .get_hash_value("name")
            .and_then(|v| v.yaml.as_str())
            .unwrap_or("<unknown>");

        if let Some(schema_yaml) = item.get_hash_value("schema") {
            total_count += 1;
            match Schema::from_yaml(schema_yaml) {
                Ok(_) => success_count += 1,
                Err(e) => failures.push((name.to_string(), format!("{:?}", e))),
            }
        }
    }

    eprintln!(
        "{}: Successfully parsed {}/{} schemas",
        file_name, success_count, total_count
    );

    if !failures.is_empty() {
        eprintln!("  Failures:");
        for (name, error) in &failures {
            eprintln!("    - {}: {}", name, error);
        }
    }

    (success_count, total_count, failures)
}

#[test]
fn test_parse_all_document_execute_schemas() {
    let content = include_str!("../test-fixtures/schemas/document-execute.yml");
    let (success, total, failures) = parse_field_schemas(content, "document-execute.yml");

    // We expect high success rate now that P0/P1 features are implemented
    assert!(
        success > total * 9 / 10,
        "Too many failures in document-execute.yml: {}/{} succeeded. Failures: {:?}",
        success,
        total,
        failures
    );
}

#[test]
fn test_parse_all_document_text_schemas() {
    let content = include_str!("../test-fixtures/schemas/document-text.yml");
    let (success, total, failures) = parse_field_schemas(content, "document-text.yml");

    assert!(
        success > total * 9 / 10,
        "Too many failures in document-text.yml: {}/{} succeeded. Failures: {:?}",
        success,
        total,
        failures
    );
}

#[test]
fn test_parse_all_document_website_schemas() {
    let content = include_str!("../test-fixtures/schemas/document-website.yml");
    let (success, total, failures) = parse_field_schemas(content, "document-website.yml");

    assert!(
        success > total * 9 / 10,
        "Too many failures in document-website.yml: {}/{} succeeded. Failures: {:?}",
        success,
        total,
        failures
    );
}

#[test]
fn test_parse_key_definitions_schemas() {
    // Rather than trying to parse all 101 definitions generically,
    // test key patterns that use our P0/P1 features

    // Test arrayOf patterns
    let yaml1 = quarto_yaml::parse(r#"arrayOf: path"#).unwrap();
    assert!(Schema::from_yaml(&yaml1).is_ok(), "pandoc-shortcodes pattern");

    let yaml2 = quarto_yaml::parse(
        r#"
arrayOf:
  arrayOf:
    schema: string
    length: 2
"#,
    )
    .unwrap();
    assert!(
        Schema::from_yaml(&yaml2).is_ok(),
        "pandoc-format-request-headers pattern"
    );

    // Test maybeArrayOf pattern
    let yaml3 = quarto_yaml::parse(r#"maybeArrayOf: string"#).unwrap();
    assert!(Schema::from_yaml(&yaml3).is_ok(), "contents-auto pattern");

    // Test record pattern
    let yaml4 = quarto_yaml::parse(
        r#"
record:
  type:
    enum: [citeproc]
"#,
    )
    .unwrap();
    assert!(
        Schema::from_yaml(&yaml4).is_ok(),
        "pandoc-format-filters record pattern"
    );

    // Test complex anyOf with object
    let yaml5 = quarto_yaml::parse(
        r#"
anyOf:
  - string
  - object:
      properties:
        value: string
        format: string
      required: [value]
"#,
    )
    .unwrap();
    assert!(Schema::from_yaml(&yaml5).is_ok(), "date pattern");

    eprintln!("✓ All key definitions.yml patterns parsed successfully");
}

#[test]
fn test_comprehensive_statistics() {
    // Parse all field-based files and gather statistics
    let mut stats: HashMap<String, (usize, usize)> = HashMap::new();

    let files = vec![
        (
            "document-execute.yml",
            include_str!("../test-fixtures/schemas/document-execute.yml"),
        ),
        (
            "document-text.yml",
            include_str!("../test-fixtures/schemas/document-text.yml"),
        ),
        (
            "document-website.yml",
            include_str!("../test-fixtures/schemas/document-website.yml"),
        ),
    ];

    let mut total_success = 0;
    let mut total_schemas = 0;
    let mut all_failures = Vec::new();

    for (name, content) in files {
        let (success, total, mut failures) = parse_field_schemas(content, name);

        stats.insert(name.to_string(), (success, total));
        total_success += success;
        total_schemas += total;

        for (id, err) in failures.drain(..) {
            all_failures.push((name.to_string(), id, err));
        }
    }

    eprintln!("\n=== Comprehensive Test Statistics (Field-Based Files) ===");
    eprintln!("Total schemas parsed: {}/{}", total_success, total_schemas);
    eprintln!(
        "Overall success rate: {:.1}%",
        (total_success as f64 / total_schemas as f64) * 100.0
    );
    eprintln!("\nPer-file breakdown:");
    for (file, (success, total)) in &stats {
        eprintln!(
            "  {}: {}/{} ({:.1}%)",
            file,
            success,
            total,
            (*success as f64 / *total as f64) * 100.0
        );
    }

    if !all_failures.is_empty() {
        eprintln!("\n=== All Failures ({}) ===", all_failures.len());
        for (file, id, error) in &all_failures {
            eprintln!("  {}:{}: {}", file, id, error);
        }
    }

    // With P0/P1 features implemented, we expect >90% success on field-based files
    assert!(
        total_success > total_schemas * 9 / 10,
        "Overall success rate too low: {}/{} ({:.1}%)",
        total_success,
        total_schemas,
        (total_success as f64 / total_schemas as f64) * 100.0
    );
}
