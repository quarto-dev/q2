//! Integration tests using real quarto-cli schema files
//!
//! These tests verify that our YAML schema parser can successfully parse
//! actual schema files from quarto-cli without errors.

use quarto_yaml_validation::Schema;

/// Test specific schemas from definitions.yml that use our P0/P1 features
#[test]
fn test_parse_definitions_yml() {
    // Test pandoc-format-request-headers (nested arrayOf with length)
    let yaml1 = quarto_yaml::parse(
        r#"
arrayOf:
  arrayOf:
    schema: string
    length: 2
"#,
    )
    .unwrap();
    assert!(
        Schema::from_yaml(&yaml1).is_ok(),
        "Failed to parse pandoc-format-request-headers pattern"
    );

    // Test pandoc-shortcodes (simple arrayOf)
    let yaml2 = quarto_yaml::parse(
        r#"
arrayOf: path
"#,
    )
    .unwrap();
    assert!(
        Schema::from_yaml(&yaml2).is_ok(),
        "Failed to parse pandoc-shortcodes pattern"
    );

    // Test pandoc-format-filters (arrayOf with anyOf and record)
    let yaml3 = quarto_yaml::parse(
        r#"
arrayOf:
  anyOf:
    - path
    - object:
        properties:
          type: string
          path: path
        required: [path]
    - record:
        type:
          enum: [citeproc]
"#,
    )
    .unwrap();
    assert!(
        Schema::from_yaml(&yaml3).is_ok(),
        "Failed to parse pandoc-format-filters pattern"
    );

    // Test contents-auto (maybeArrayOf)
    let yaml4 = quarto_yaml::parse(
        r#"
maybeArrayOf: string
"#,
    )
    .unwrap();
    assert!(
        Schema::from_yaml(&yaml4).is_ok(),
        "Failed to parse contents-auto auto field pattern"
    );

    // Test date-format (schema wrapper)
    let yaml5 = quarto_yaml::parse(
        r#"
schema: string
"#,
    )
    .unwrap();
    assert!(
        Schema::from_yaml(&yaml5).is_ok(),
        "Failed to parse date-format pattern"
    );
}

/// Test parsing document-text.yml which contains many schema wrapper patterns
#[test]
fn test_parse_document_text_yml() {
    let yaml_content = include_str!("../test-fixtures/schemas/document-text.yml");
    let yaml = quarto_yaml::parse(yaml_content).expect("Failed to parse YAML");

    // The file is an array of field definitions
    let items = yaml.as_array().expect("Expected array at root");

    // Parse each field definition
    for item in items {
        let name = item
            .get_hash_value("name")
            .and_then(|v| v.yaml.as_str())
            .unwrap_or("<unknown>");

        // Each item should have a 'schema' field
        if let Some(schema_yaml) = item.get_hash_value("schema") {
            let schema_result = Schema::from_yaml(schema_yaml);
            assert!(
                schema_result.is_ok(),
                "Failed to parse schema for field '{}': {:?}",
                name,
                schema_result.err()
            );
        }
    }
}

/// Test specific patterns from definitions.yml
#[test]
fn test_definitions_arrayof_patterns() {
    // Test nested arrayOf with length constraint (pandoc-format-request-headers)
    let yaml = quarto_yaml::parse(
        r#"
arrayOf:
  arrayOf:
    schema: string
    length: 2
"#,
    )
    .unwrap();

    let schema = Schema::from_yaml(&yaml).unwrap();
    // Should parse as Array containing Array containing String
    match schema {
        Schema::Array(outer) => {
            assert!(outer.items.is_some());
            if let Some(inner_box) = outer.items {
                match *inner_box {
                    Schema::Array(inner) => {
                        assert_eq!(inner.min_items, Some(2));
                        assert_eq!(inner.max_items, Some(2));
                        assert!(inner.items.is_some());
                    }
                    _ => panic!("Expected inner Array schema"),
                }
            }
        }
        _ => panic!("Expected outer Array schema"),
    }
}

/// Test maybeArrayOf pattern from definitions.yml
#[test]
fn test_definitions_maybe_arrayof() {
    let yaml = quarto_yaml::parse(
        r#"
maybeArrayOf: string
"#,
    )
    .unwrap();

    let schema = Schema::from_yaml(&yaml).unwrap();
    // Should parse as anyOf: [string, array of string]
    match schema {
        Schema::AnyOf(s) => {
            assert_eq!(s.schemas.len(), 2);
            // First should be string, second should be array
            assert!(matches!(s.schemas[0], Schema::String(_)));
            assert!(matches!(s.schemas[1], Schema::Array(_)));
        }
        _ => panic!("Expected AnyOf schema"),
    }
}

/// Test record pattern from definitions.yml
#[test]
fn test_definitions_record() {
    let yaml = quarto_yaml::parse(
        r#"
record:
  type:
    enum: [citeproc]
"#,
    )
    .unwrap();

    let schema = Schema::from_yaml(&yaml).unwrap();
    // Should parse as closed object with all properties required
    match schema {
        Schema::Object(s) => {
            assert!(s.closed);
            assert_eq!(s.properties.len(), 1);
            assert_eq!(s.required.len(), 1);
            assert!(s.required.contains(&"type".to_string()));
            assert!(s.properties.contains_key("type"));
        }
        _ => panic!("Expected Object schema"),
    }
}

/// Test schema wrapper from document-text.yml
#[test]
fn test_document_text_schema_wrapper() {
    let yaml = quarto_yaml::parse(
        r#"
schema:
  enum: [lf, crlf, native]
"#,
    )
    .unwrap();

    let schema = Schema::from_yaml(&yaml).unwrap();
    // Should parse as enum
    match schema {
        Schema::Enum(s) => {
            assert_eq!(s.values.len(), 3);
        }
        _ => panic!("Expected Enum schema"),
    }
}
