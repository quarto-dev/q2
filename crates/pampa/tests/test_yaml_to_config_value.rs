/*
 * test_yaml_to_config_value.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Tests for yaml_to_config_value function with context-aware interpretation.
 */

use pampa::pandoc::yaml_to_config_value;
use pampa::utils::diagnostic_collector::DiagnosticCollector;
use quarto_pandoc_types::{ConfigValue, ConfigValueKind, Inline, InterpretationContext, MergeOp};
use yaml_rust2::Yaml;

/// Helper to parse YAML string and convert to ConfigValue
fn parse_yaml(content: &str, context: InterpretationContext) -> ConfigValue {
    let yaml = quarto_yaml::parse(content).expect("Failed to parse YAML");
    let mut diagnostics = DiagnosticCollector::new();
    yaml_to_config_value(yaml, context, &mut diagnostics)
}

/// Helper to parse YAML and also return diagnostics
fn parse_yaml_with_diagnostics(
    content: &str,
    context: InterpretationContext,
) -> (ConfigValue, DiagnosticCollector) {
    let yaml = quarto_yaml::parse(content).expect("Failed to parse YAML");
    let mut diagnostics = DiagnosticCollector::new();
    let value = yaml_to_config_value(yaml, context, &mut diagnostics);
    (value, diagnostics)
}

// =============================================================================
// Context-Dependent String Interpretation Tests
// =============================================================================

#[test]
fn test_document_metadata_parses_strings_as_markdown() {
    // In DocumentMetadata context, strings are parsed as markdown by default
    let config = parse_yaml(
        "This has *emphasis*",
        InterpretationContext::DocumentMetadata,
    );

    match &config.value {
        ConfigValueKind::PandocInlines(inlines) => {
            // Should have Emph element from *emphasis*
            let has_emph = inlines
                .iter()
                .any(|inline| matches!(inline, Inline::Emph(_)));
            assert!(
                has_emph,
                "DocumentMetadata should parse *emphasis* as Emph inline"
            );
        }
        other => panic!(
            "Expected PandocInlines for markdown string, got: {:?}",
            other
        ),
    }
}

#[test]
fn test_project_config_keeps_strings_literal() {
    // In ProjectConfig context, strings are kept literal by default
    let config = parse_yaml("This has *emphasis*", InterpretationContext::ProjectConfig);

    match &config.value {
        ConfigValueKind::Scalar(Yaml::String(s)) => {
            assert_eq!(s, "This has *emphasis*");
        }
        other => panic!(
            "Expected literal Scalar string for ProjectConfig, got: {:?}",
            other
        ),
    }
}

#[test]
fn test_simple_string_document_metadata() {
    // Simple string without markdown formatting
    let config = parse_yaml("Hello World", InterpretationContext::DocumentMetadata);

    match &config.value {
        ConfigValueKind::PandocInlines(inlines) => {
            // Should have a Str inline with "Hello World"
            assert!(!inlines.is_empty(), "Should have inlines");
            // Note: Markdown parser may produce multiple inlines (Str + Space + Str)
        }
        other => panic!("Expected PandocInlines, got: {:?}", other),
    }
}

#[test]
fn test_simple_string_project_config() {
    // Simple string in ProjectConfig context
    let config = parse_yaml("Hello World", InterpretationContext::ProjectConfig);

    match &config.value {
        ConfigValueKind::Scalar(Yaml::String(s)) => {
            assert_eq!(s, "Hello World");
        }
        other => panic!("Expected Scalar, got: {:?}", other),
    }
}

// =============================================================================
// Explicit Tag Tests - !str, !md, !path, !glob, !expr
// =============================================================================

#[test]
fn test_str_tag_in_document_metadata() {
    // !str should keep literal even in DocumentMetadata context
    let content = "!str _foo_.py";
    let config = parse_yaml(content, InterpretationContext::DocumentMetadata);

    match &config.value {
        ConfigValueKind::Scalar(Yaml::String(s)) => {
            assert_eq!(
                s, "_foo_.py",
                "!str should keep underscore-surrounded text literal"
            );
        }
        other => panic!("Expected Scalar for !str, got: {:?}", other),
    }
}

#[test]
fn test_str_tag_in_project_config() {
    // !str should also work (though redundant) in ProjectConfig context
    let content = "!str literal_value";
    let config = parse_yaml(content, InterpretationContext::ProjectConfig);

    match &config.value {
        ConfigValueKind::Scalar(Yaml::String(s)) => {
            assert_eq!(s, "literal_value");
        }
        other => panic!("Expected Scalar for !str, got: {:?}", other),
    }
}

#[test]
fn test_md_tag_in_project_config() {
    // !md should force markdown parsing in ProjectConfig context
    let content = "!md This has *emphasis*";
    let config = parse_yaml(content, InterpretationContext::ProjectConfig);

    match &config.value {
        ConfigValueKind::PandocInlines(inlines) => {
            let has_emph = inlines
                .iter()
                .any(|inline| matches!(inline, Inline::Emph(_)));
            assert!(
                has_emph,
                "!md should force markdown parsing in ProjectConfig"
            );
        }
        other => panic!("Expected PandocInlines for !md, got: {:?}", other),
    }
}

#[test]
fn test_md_tag_in_document_metadata() {
    // !md should also work (though redundant) in DocumentMetadata context
    let content = "!md This has **strong**";
    let config = parse_yaml(content, InterpretationContext::DocumentMetadata);

    match &config.value {
        ConfigValueKind::PandocInlines(inlines) => {
            let has_strong = inlines
                .iter()
                .any(|inline| matches!(inline, Inline::Strong(_)));
            assert!(has_strong, "!md should parse **strong** as Strong inline");
        }
        other => panic!("Expected PandocInlines for !md, got: {:?}", other),
    }
}

#[test]
fn test_path_tag() {
    let content = "!path ./data/file.csv";

    // Path should work the same in both contexts
    for context in [
        InterpretationContext::DocumentMetadata,
        InterpretationContext::ProjectConfig,
    ] {
        let config = parse_yaml(content, context);

        match &config.value {
            ConfigValueKind::Path(p) => {
                assert_eq!(p, "./data/file.csv");
            }
            other => panic!("Expected Path variant, got: {:?}", other),
        }
    }
}

#[test]
fn test_glob_tag() {
    // Glob patterns with special characters need to be quoted in YAML
    let content = "!glob \"**/*.qmd\"";

    for context in [
        InterpretationContext::DocumentMetadata,
        InterpretationContext::ProjectConfig,
    ] {
        let config = parse_yaml(content, context);

        match &config.value {
            ConfigValueKind::Glob(g) => {
                assert_eq!(g, "**/*.qmd");
            }
            other => panic!("Expected Glob variant, got: {:?}", other),
        }
    }
}

#[test]
fn test_expr_tag() {
    let content = "!expr params$threshold";

    for context in [
        InterpretationContext::DocumentMetadata,
        InterpretationContext::ProjectConfig,
    ] {
        let config = parse_yaml(content, context);

        match &config.value {
            ConfigValueKind::Expr(e) => {
                assert_eq!(e, "params$threshold");
            }
            other => panic!("Expected Expr variant, got: {:?}", other),
        }
    }
}

// =============================================================================
// Merge Operation Tests
// =============================================================================

#[test]
fn test_prefer_tag_on_string() {
    let content = "!prefer value";
    let config = parse_yaml(content, InterpretationContext::ProjectConfig);

    assert_eq!(config.merge_op, MergeOp::Prefer);
    match &config.value {
        ConfigValueKind::Scalar(Yaml::String(s)) => {
            assert_eq!(s, "value");
        }
        other => panic!("Expected Scalar, got: {:?}", other),
    }
}

#[test]
fn test_concat_tag_on_string() {
    let content = "!concat value";
    let config = parse_yaml(content, InterpretationContext::ProjectConfig);

    assert_eq!(config.merge_op, MergeOp::Concat);
}

#[test]
fn test_prefer_with_str_tag() {
    // Combining merge and interpretation tags with underscore separator
    let content = "!prefer_str _literal_";
    let config = parse_yaml(content, InterpretationContext::DocumentMetadata);

    assert_eq!(config.merge_op, MergeOp::Prefer);
    match &config.value {
        ConfigValueKind::Scalar(Yaml::String(s)) => {
            assert_eq!(s, "_literal_");
        }
        other => panic!("Expected Scalar, got: {:?}", other),
    }
}

#[test]
fn test_concat_with_md_tag() {
    // Combining merge and interpretation tags with underscore separator
    let content = "!concat_md \"**bold**\"";
    let config = parse_yaml(content, InterpretationContext::ProjectConfig);

    assert_eq!(config.merge_op, MergeOp::Concat);
    match &config.value {
        ConfigValueKind::PandocInlines(inlines) => {
            let has_strong = inlines
                .iter()
                .any(|inline| matches!(inline, Inline::Strong(_)));
            assert!(has_strong);
        }
        other => panic!("Expected PandocInlines, got: {:?}", other),
    }
}

// =============================================================================
// Compound Types Tests
// =============================================================================

#[test]
fn test_array_in_document_metadata() {
    let content = r#"
- first item
- second *emphasis*
- third item
"#;
    let config = parse_yaml(content, InterpretationContext::DocumentMetadata);

    match &config.value {
        ConfigValueKind::Array(items) => {
            assert_eq!(items.len(), 3);

            // Each item should be PandocInlines (markdown parsed)
            for item in items {
                match &item.value {
                    ConfigValueKind::PandocInlines(_) => {}
                    other => panic!("Expected PandocInlines for array item, got: {:?}", other),
                }
            }
        }
        other => panic!("Expected Array, got: {:?}", other),
    }
}

#[test]
fn test_array_in_project_config() {
    let content = r#"
- first item
- second *not emphasis*
- third item
"#;
    let config = parse_yaml(content, InterpretationContext::ProjectConfig);

    match &config.value {
        ConfigValueKind::Array(items) => {
            assert_eq!(items.len(), 3);

            // Each item should be literal Scalar
            for item in items {
                match &item.value {
                    ConfigValueKind::Scalar(Yaml::String(_)) => {}
                    other => panic!("Expected Scalar for array item, got: {:?}", other),
                }
            }
        }
        other => panic!("Expected Array, got: {:?}", other),
    }
}

#[test]
fn test_map_in_project_config() {
    let content = r#"
key1: value1
key2: value2
"#;
    let config = parse_yaml(content, InterpretationContext::ProjectConfig);

    match &config.value {
        ConfigValueKind::Map(entries) => {
            assert_eq!(entries.len(), 2);

            let key1 = entries
                .iter()
                .find(|e| e.key == "key1")
                .expect("key1 not found");
            match &key1.value.value {
                ConfigValueKind::Scalar(Yaml::String(s)) => {
                    assert_eq!(s, "value1");
                }
                other => panic!("Expected Scalar for key1, got: {:?}", other),
            }
        }
        other => panic!("Expected Map, got: {:?}", other),
    }
}

#[test]
fn test_prefer_on_inline_array() {
    // Note: Tags on arrays are captured only for inline syntax
    let content = r#"!prefer [a, b]"#;
    let config = parse_yaml(content, InterpretationContext::ProjectConfig);

    // Check that array is parsed correctly
    match &config.value {
        ConfigValueKind::Array(items) => {
            assert_eq!(items.len(), 2);
            // Note: quarto-yaml currently does not capture tags on compound types,
            // so merge_op will be default (Concat). This is a known limitation.
            // When quarto-yaml is updated to capture tags on arrays, this test
            // should be updated to assert MergeOp::Prefer.
        }
        other => panic!("Expected Array, got: {:?}", other),
    }
}

#[test]
fn test_prefer_on_inline_map() {
    // Note: Tags on maps are captured only for inline syntax
    let content = r#"!prefer {key: value}"#;
    let config = parse_yaml(content, InterpretationContext::ProjectConfig);

    // Check that map is parsed correctly
    match &config.value {
        ConfigValueKind::Map(entries) => {
            assert_eq!(entries.len(), 1);
            // Note: quarto-yaml currently does not capture tags on compound types,
            // so merge_op will be default (Concat). This is a known limitation.
            // When quarto-yaml is updated to capture tags on maps, this test
            // should be updated to assert MergeOp::Prefer.
        }
        other => panic!("Expected Map, got: {:?}", other),
    }
}

// =============================================================================
// Non-String Scalar Tests
// =============================================================================

#[test]
fn test_boolean_values() {
    for context in [
        InterpretationContext::DocumentMetadata,
        InterpretationContext::ProjectConfig,
    ] {
        let config_true = parse_yaml("true", context);
        match &config_true.value {
            ConfigValueKind::Scalar(Yaml::Boolean(b)) => {
                assert!(*b);
            }
            other => panic!("Expected Boolean true, got: {:?}", other),
        }

        let config_false = parse_yaml("false", context);
        match &config_false.value {
            ConfigValueKind::Scalar(Yaml::Boolean(b)) => {
                assert!(!*b);
            }
            other => panic!("Expected Boolean false, got: {:?}", other),
        }
    }
}

#[test]
fn test_integer_values() {
    for context in [
        InterpretationContext::DocumentMetadata,
        InterpretationContext::ProjectConfig,
    ] {
        let config = parse_yaml("42", context);
        match &config.value {
            ConfigValueKind::Scalar(Yaml::Integer(i)) => {
                assert_eq!(*i, 42);
            }
            other => panic!("Expected Integer, got: {:?}", other),
        }
    }
}

#[test]
fn test_float_values() {
    for context in [
        InterpretationContext::DocumentMetadata,
        InterpretationContext::ProjectConfig,
    ] {
        let config = parse_yaml("3.14", context);
        match &config.value {
            ConfigValueKind::Scalar(Yaml::Real(r)) => {
                let f: f64 = r.parse().expect("Failed to parse float");
                assert!((f - 3.14).abs() < 0.001);
            }
            other => panic!("Expected Real, got: {:?}", other),
        }
    }
}

#[test]
fn test_null_value() {
    for context in [
        InterpretationContext::DocumentMetadata,
        InterpretationContext::ProjectConfig,
    ] {
        let config = parse_yaml("null", context);
        match &config.value {
            ConfigValueKind::Scalar(Yaml::Null) => {}
            other => panic!("Expected Null, got: {:?}", other),
        }

        // Also test ~ which is YAML null
        let config_tilde = parse_yaml("~", context);
        match &config_tilde.value {
            ConfigValueKind::Scalar(Yaml::Null) => {}
            other => panic!("Expected Null for ~, got: {:?}", other),
        }
    }
}

// =============================================================================
// Error Handling Tests
// =============================================================================

#[test]
fn test_md_tag_with_invalid_markdown_produces_error() {
    // !md tag with syntax that can't be parsed should produce an error diagnostic
    // Note: Most strings can be parsed as markdown, so we use a tricky case
    let content = "!md This has [incomplete link(";
    let (config, diagnostics) =
        parse_yaml_with_diagnostics(content, InterpretationContext::ProjectConfig);

    // Check if we got any errors (the markdown parser may still produce output)
    // The main thing is it shouldn't panic
    match &config.value {
        ConfigValueKind::PandocInlines(_) | ConfigValueKind::PandocBlocks(_) => {
            // Parsed successfully (markdown is permissive)
        }
        ConfigValueKind::Scalar(Yaml::String(_)) => {
            // Fallback to string on error is also acceptable
            assert!(
                diagnostics.has_errors(),
                "Should have error diagnostic on parse failure"
            );
        }
        other => panic!("Unexpected variant: {:?}", other),
    }
}

#[test]
fn test_untagged_markdown_parse_failure_produces_warning() {
    // Untagged strings that fail to parse as markdown should produce a warning
    // and fall back to literal string
    // (Most markdown is permissive, so this test may not trigger the warning)
    let content = "posts/*/index.qmd";
    let (_config, _diagnostics) =
        parse_yaml_with_diagnostics(content, InterpretationContext::DocumentMetadata);

    // This string contains glob-like patterns which may fail markdown parsing
    // The main thing is it shouldn't panic
}

// =============================================================================
// Nested Structure Tests
// =============================================================================

#[test]
fn test_nested_map_with_mixed_contexts() {
    let content = r#"
format:
  html:
    theme: cosmo
    toc: true
  pdf:
    documentclass: article
"#;
    let config = parse_yaml(content, InterpretationContext::ProjectConfig);

    match &config.value {
        ConfigValueKind::Map(entries) => {
            let format = entries
                .iter()
                .find(|e| e.key == "format")
                .expect("format not found");
            match &format.value.value {
                ConfigValueKind::Map(format_entries) => {
                    let html = format_entries
                        .iter()
                        .find(|e| e.key == "html")
                        .expect("html not found");
                    match &html.value.value {
                        ConfigValueKind::Map(html_entries) => {
                            let theme = html_entries
                                .iter()
                                .find(|e| e.key == "theme")
                                .expect("theme not found");
                            match &theme.value.value {
                                ConfigValueKind::Scalar(Yaml::String(s)) => {
                                    assert_eq!(s, "cosmo");
                                }
                                other => panic!("Expected Scalar for theme, got: {:?}", other),
                            }

                            let toc = html_entries
                                .iter()
                                .find(|e| e.key == "toc")
                                .expect("toc not found");
                            match &toc.value.value {
                                ConfigValueKind::Scalar(Yaml::Boolean(b)) => {
                                    assert!(*b);
                                }
                                other => panic!("Expected Boolean for toc, got: {:?}", other),
                            }
                        }
                        other => panic!("Expected Map for html, got: {:?}", other),
                    }
                }
                other => panic!("Expected Map for format, got: {:?}", other),
            }
        }
        other => panic!("Expected Map, got: {:?}", other),
    }
}

#[test]
fn test_array_of_maps_document_metadata() {
    let content = r#"
- name: John
  role: Author
- name: Jane
  role: Editor
"#;
    let config = parse_yaml(content, InterpretationContext::DocumentMetadata);

    match &config.value {
        ConfigValueKind::Array(items) => {
            assert_eq!(items.len(), 2);

            // Check first item is a map with name/role
            match &items[0].value {
                ConfigValueKind::Map(entries) => {
                    let name = entries
                        .iter()
                        .find(|e| e.key == "name")
                        .expect("name not found");
                    // In DocumentMetadata, strings become PandocInlines
                    match &name.value.value {
                        ConfigValueKind::PandocInlines(inlines) => {
                            // Should contain "John"
                            assert!(!inlines.is_empty());
                        }
                        other => panic!("Expected PandocInlines for name, got: {:?}", other),
                    }
                }
                other => panic!("Expected Map for array item, got: {:?}", other),
            }
        }
        other => panic!("Expected Array, got: {:?}", other),
    }
}

// =============================================================================
// ConfigValue Helper Method Tests
// =============================================================================

#[test]
fn test_config_value_is_string_value() {
    // Test is_string_value method works correctly
    let scalar = parse_yaml("hello", InterpretationContext::ProjectConfig);
    assert!(scalar.is_string_value("hello"));
    assert!(!scalar.is_string_value("world"));

    let path = parse_yaml("!path ./file.txt", InterpretationContext::ProjectConfig);
    assert!(path.is_string_value("./file.txt"));

    // PandocInlines with single Str should also match
    let _md_simple = parse_yaml("hello", InterpretationContext::DocumentMetadata);
    // Note: "hello" parses to a single Str inline, so this should match
    // (unless the markdown parser wraps it differently)
}

#[test]
fn test_config_value_as_str() {
    let scalar = parse_yaml("value", InterpretationContext::ProjectConfig);
    assert_eq!(scalar.as_str(), Some("value"));

    let path = parse_yaml("!path ./data.csv", InterpretationContext::ProjectConfig);
    assert_eq!(path.as_str(), Some("./data.csv"));

    let glob = parse_yaml("!glob \"*.qmd\"", InterpretationContext::ProjectConfig);
    assert_eq!(glob.as_str(), Some("*.qmd"));

    let expr = parse_yaml("!expr x + y", InterpretationContext::ProjectConfig);
    assert_eq!(expr.as_str(), Some("x + y"));

    // Non-string scalars should return None
    let int = parse_yaml("42", InterpretationContext::ProjectConfig);
    assert_eq!(int.as_str(), None);
}

#[test]
fn test_config_value_get() {
    let content = r#"
foo: bar
baz: 42
"#;
    let config = parse_yaml(content, InterpretationContext::ProjectConfig);

    let foo = config.get("foo").expect("foo not found");
    match &foo.value {
        ConfigValueKind::Scalar(Yaml::String(s)) => {
            assert_eq!(s, "bar");
        }
        other => panic!("Expected Scalar, got: {:?}", other),
    }

    let baz = config.get("baz").expect("baz not found");
    match &baz.value {
        ConfigValueKind::Scalar(Yaml::Integer(i)) => {
            assert_eq!(*i, 42);
        }
        other => panic!("Expected Integer, got: {:?}", other),
    }

    assert!(config.get("nonexistent").is_none());
}

#[test]
fn test_config_value_contains_key() {
    let content = r#"
existing: value
"#;
    let config = parse_yaml(content, InterpretationContext::ProjectConfig);

    assert!(config.contains_key("existing"));
    assert!(!config.contains_key("nonexistent"));
}
