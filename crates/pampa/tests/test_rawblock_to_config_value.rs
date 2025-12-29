/*
 * test_rawblock_to_config_value.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Tests for rawblock_to_config_value function.
 * These tests verify the new ConfigValue-based parsing path for document metadata
 * and ensure equivalence with the legacy rawblock_to_meta_with_source_info function.
 */

use pampa::pandoc::ast_context::ASTContext;
use pampa::pandoc::location::{Location, Range, SourceInfo};
use pampa::pandoc::{RawBlock, rawblock_to_config_value, rawblock_to_meta_with_source_info};
use pampa::template::config_value_to_meta;
use pampa::utils::diagnostic_collector::DiagnosticCollector;
use quarto_pandoc_types::{ConfigValueKind, Inline, MetaValueWithSourceInfo};

fn make_rawblock(content: &str) -> RawBlock {
    RawBlock {
        format: "quarto_minus_metadata".to_string(),
        text: content.to_string(),
        source_info: SourceInfo::with_range(Range {
            start: Location {
                offset: 0,
                row: 0,
                column: 0,
            },
            end: Location {
                offset: content.len(),
                row: 0,
                column: content.len(),
            },
        })
        .to_source_map_info(),
    }
}

fn make_ast_context() -> ASTContext {
    ASTContext::with_filename("<test>".to_string())
}

// =============================================================================
// Basic Parsing Tests
// =============================================================================

#[test]
fn test_rawblock_to_config_value_simple_string() {
    let content = r#"---
title: Hello World
---"#;
    let block = make_rawblock(content);
    let mut diagnostics = DiagnosticCollector::new();

    let config = rawblock_to_config_value(&block, &mut diagnostics);

    // Should be a Map with "title" key
    match &config.value {
        ConfigValueKind::Map(entries) => {
            assert_eq!(entries.len(), 1, "Expected 1 entry");
            assert_eq!(entries[0].key, "title");

            // In DocumentMetadata context, untagged strings become PandocInlines
            match &entries[0].value.value {
                ConfigValueKind::PandocInlines(inlines) => {
                    // "Hello World" should have some inlines
                    assert!(!inlines.is_empty());
                }
                other => panic!("Expected PandocInlines for title, got: {:?}", other),
            }
        }
        other => panic!("Expected Map, got: {:?}", other),
    }
}

#[test]
fn test_rawblock_to_config_value_boolean() {
    let content = r#"---
toc: true
---"#;
    let block = make_rawblock(content);
    let mut diagnostics = DiagnosticCollector::new();

    let config = rawblock_to_config_value(&block, &mut diagnostics);

    match &config.value {
        ConfigValueKind::Map(entries) => {
            let toc = entries
                .iter()
                .find(|e| e.key == "toc")
                .expect("toc not found");
            match &toc.value.value {
                ConfigValueKind::Scalar(yaml_rust2::Yaml::Boolean(b)) => {
                    assert!(*b);
                }
                other => panic!("Expected Boolean for toc, got: {:?}", other),
            }
        }
        other => panic!("Expected Map, got: {:?}", other),
    }
}

#[test]
fn test_rawblock_to_config_value_integer() {
    let content = r#"---
page-count: 42
---"#;
    let block = make_rawblock(content);
    let mut diagnostics = DiagnosticCollector::new();

    let config = rawblock_to_config_value(&block, &mut diagnostics);

    match &config.value {
        ConfigValueKind::Map(entries) => {
            let count = entries
                .iter()
                .find(|e| e.key == "page-count")
                .expect("page-count not found");
            match &count.value.value {
                ConfigValueKind::Scalar(yaml_rust2::Yaml::Integer(i)) => {
                    assert_eq!(*i, 42);
                }
                other => panic!("Expected Integer for page-count, got: {:?}", other),
            }
        }
        other => panic!("Expected Map, got: {:?}", other),
    }
}

#[test]
fn test_rawblock_to_config_value_array() {
    let content = r#"---
categories:
  - first
  - second
  - third
---"#;
    let block = make_rawblock(content);
    let mut diagnostics = DiagnosticCollector::new();

    let config = rawblock_to_config_value(&block, &mut diagnostics);

    match &config.value {
        ConfigValueKind::Map(entries) => {
            let categories = entries
                .iter()
                .find(|e| e.key == "categories")
                .expect("categories not found");
            match &categories.value.value {
                ConfigValueKind::Array(items) => {
                    assert_eq!(items.len(), 3);
                    // Each item should be PandocInlines (markdown parsed)
                    for item in items {
                        assert!(matches!(item.value, ConfigValueKind::PandocInlines(_)));
                    }
                }
                other => panic!("Expected Array for categories, got: {:?}", other),
            }
        }
        other => panic!("Expected Map, got: {:?}", other),
    }
}

#[test]
fn test_rawblock_to_config_value_nested_map() {
    let content = r#"---
format:
  html:
    theme: cosmo
---"#;
    let block = make_rawblock(content);
    let mut diagnostics = DiagnosticCollector::new();

    let config = rawblock_to_config_value(&block, &mut diagnostics);

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
                                ConfigValueKind::PandocInlines(_) => {
                                    // "cosmo" parsed as markdown
                                }
                                other => {
                                    panic!("Expected PandocInlines for theme, got: {:?}", other)
                                }
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

// =============================================================================
// Markdown Parsing Tests (DocumentMetadata context)
// =============================================================================

#[test]
fn test_rawblock_to_config_value_parses_markdown() {
    let content = r#"---
title: This has *emphasis* and **strong**
---"#;
    let block = make_rawblock(content);
    let mut diagnostics = DiagnosticCollector::new();

    let config = rawblock_to_config_value(&block, &mut diagnostics);

    match &config.value {
        ConfigValueKind::Map(entries) => {
            let title = entries
                .iter()
                .find(|e| e.key == "title")
                .expect("title not found");
            match &title.value.value {
                ConfigValueKind::PandocInlines(inlines) => {
                    // Should have Emph and Strong elements
                    let has_emph = inlines.iter().any(|i| matches!(i, Inline::Emph(_)));
                    let has_strong = inlines.iter().any(|i| matches!(i, Inline::Strong(_)));
                    assert!(has_emph, "Expected Emph inline from *emphasis*");
                    assert!(has_strong, "Expected Strong inline from **strong**");
                }
                other => panic!("Expected PandocInlines for title, got: {:?}", other),
            }
        }
        other => panic!("Expected Map, got: {:?}", other),
    }
}

// =============================================================================
// Explicit Tag Tests
// =============================================================================

#[test]
fn test_rawblock_to_config_value_str_tag() {
    let content = r#"---
filename: !str _foo_.py
---"#;
    let block = make_rawblock(content);
    let mut diagnostics = DiagnosticCollector::new();

    let config = rawblock_to_config_value(&block, &mut diagnostics);

    match &config.value {
        ConfigValueKind::Map(entries) => {
            let filename = entries
                .iter()
                .find(|e| e.key == "filename")
                .expect("filename not found");
            match &filename.value.value {
                ConfigValueKind::Scalar(yaml_rust2::Yaml::String(s)) => {
                    assert_eq!(
                        s, "_foo_.py",
                        "!str should keep underscore-surrounded text literal"
                    );
                }
                other => panic!("Expected Scalar string for !str, got: {:?}", other),
            }
        }
        other => panic!("Expected Map, got: {:?}", other),
    }
}

#[test]
fn test_rawblock_to_config_value_path_tag() {
    let content = r#"---
data-file: !path ./data/input.csv
---"#;
    let block = make_rawblock(content);
    let mut diagnostics = DiagnosticCollector::new();

    let config = rawblock_to_config_value(&block, &mut diagnostics);

    match &config.value {
        ConfigValueKind::Map(entries) => {
            let data_file = entries
                .iter()
                .find(|e| e.key == "data-file")
                .expect("data-file not found");
            match &data_file.value.value {
                ConfigValueKind::Path(p) => {
                    assert_eq!(p, "./data/input.csv");
                }
                other => panic!("Expected Path variant for !path, got: {:?}", other),
            }
        }
        other => panic!("Expected Map, got: {:?}", other),
    }
}

#[test]
fn test_rawblock_to_config_value_glob_tag() {
    let content = r#"---
sources: !glob "**/*.qmd"
---"#;
    let block = make_rawblock(content);
    let mut diagnostics = DiagnosticCollector::new();

    let config = rawblock_to_config_value(&block, &mut diagnostics);

    match &config.value {
        ConfigValueKind::Map(entries) => {
            let sources = entries
                .iter()
                .find(|e| e.key == "sources")
                .expect("sources not found");
            match &sources.value.value {
                ConfigValueKind::Glob(g) => {
                    assert_eq!(g, "**/*.qmd");
                }
                other => panic!("Expected Glob variant for !glob, got: {:?}", other),
            }
        }
        other => panic!("Expected Map, got: {:?}", other),
    }
}

#[test]
fn test_rawblock_to_config_value_expr_tag() {
    let content = r#"---
threshold: !expr params$value
---"#;
    let block = make_rawblock(content);
    let mut diagnostics = DiagnosticCollector::new();

    let config = rawblock_to_config_value(&block, &mut diagnostics);

    match &config.value {
        ConfigValueKind::Map(entries) => {
            let threshold = entries
                .iter()
                .find(|e| e.key == "threshold")
                .expect("threshold not found");
            match &threshold.value.value {
                ConfigValueKind::Expr(e) => {
                    assert_eq!(e, "params$value");
                }
                other => panic!("Expected Expr variant for !expr, got: {:?}", other),
            }
        }
        other => panic!("Expected Map, got: {:?}", other),
    }
}

// =============================================================================
// Merge Tag Tests
// =============================================================================

#[test]
fn test_rawblock_to_config_value_prefer_tag() {
    let content = r#"---
option: !prefer my-value
---"#;
    let block = make_rawblock(content);
    let mut diagnostics = DiagnosticCollector::new();

    let config = rawblock_to_config_value(&block, &mut diagnostics);

    match &config.value {
        ConfigValueKind::Map(entries) => {
            let option = entries
                .iter()
                .find(|e| e.key == "option")
                .expect("option not found");
            assert_eq!(
                option.value.merge_op,
                quarto_pandoc_types::MergeOp::Prefer,
                "!prefer should set merge_op to Prefer"
            );
        }
        other => panic!("Expected Map, got: {:?}", other),
    }
}

// =============================================================================
// Source Location Preservation Tests
// =============================================================================

#[test]
fn test_rawblock_to_config_value_preserves_source_info() {
    let content = r#"---
title: Hello
---"#;
    let block = make_rawblock(content);
    let mut diagnostics = DiagnosticCollector::new();

    let config = rawblock_to_config_value(&block, &mut diagnostics);

    // The config's source_info should not be the default
    let default_source = quarto_source_map::SourceInfo::default();
    assert!(
        config.source_info != default_source,
        "Config should have non-default source_info"
    );

    // Check that nested values also have source info
    if let ConfigValueKind::Map(entries) = &config.value {
        let title = entries.iter().find(|e| e.key == "title").unwrap();
        assert!(
            title.key_source != default_source,
            "Key source should be tracked"
        );
        assert!(
            title.value.source_info != default_source,
            "Value source should be tracked"
        );
    }
}

// =============================================================================
// Equivalence Tests: rawblock_to_config_value vs rawblock_to_meta_with_source_info
// =============================================================================

/// Helper to compare MetaValueWithSourceInfo structures recursively.
/// Returns a description of the first difference found, or None if equal.
fn compare_meta_values(
    a: &MetaValueWithSourceInfo,
    b: &MetaValueWithSourceInfo,
    path: &str,
) -> Option<String> {
    match (a, b) {
        (
            MetaValueWithSourceInfo::MetaString { value: va, .. },
            MetaValueWithSourceInfo::MetaString { value: vb, .. },
        ) => {
            if va != vb {
                Some(format!(
                    "{}: string values differ: {:?} vs {:?}",
                    path, va, vb
                ))
            } else {
                None
            }
        }
        (
            MetaValueWithSourceInfo::MetaBool { value: va, .. },
            MetaValueWithSourceInfo::MetaBool { value: vb, .. },
        ) => {
            if va != vb {
                Some(format!(
                    "{}: bool values differ: {:?} vs {:?}",
                    path, va, vb
                ))
            } else {
                None
            }
        }
        (
            MetaValueWithSourceInfo::MetaInlines { content: ca, .. },
            MetaValueWithSourceInfo::MetaInlines { content: cb, .. },
        ) => {
            // Compare inlines by debug representation (structural equality)
            let da = format!("{:?}", ca);
            let db = format!("{:?}", cb);
            if da != db {
                Some(format!(
                    "{}: MetaInlines differ:\n  A: {:?}\n  B: {:?}",
                    path, ca, cb
                ))
            } else {
                None
            }
        }
        (
            MetaValueWithSourceInfo::MetaBlocks { content: ca, .. },
            MetaValueWithSourceInfo::MetaBlocks { content: cb, .. },
        ) => {
            let da = format!("{:?}", ca);
            let db = format!("{:?}", cb);
            if da != db {
                Some(format!("{}: MetaBlocks differ", path))
            } else {
                None
            }
        }
        (
            MetaValueWithSourceInfo::MetaList { items: ia, .. },
            MetaValueWithSourceInfo::MetaList { items: ib, .. },
        ) => {
            if ia.len() != ib.len() {
                return Some(format!(
                    "{}: MetaList length differs: {} vs {}",
                    path,
                    ia.len(),
                    ib.len()
                ));
            }
            for (i, (item_a, item_b)) in ia.iter().zip(ib.iter()).enumerate() {
                if let Some(diff) = compare_meta_values(item_a, item_b, &format!("{}[{}]", path, i))
                {
                    return Some(diff);
                }
            }
            None
        }
        (
            MetaValueWithSourceInfo::MetaMap { entries: ea, .. },
            MetaValueWithSourceInfo::MetaMap { entries: eb, .. },
        ) => {
            if ea.len() != eb.len() {
                return Some(format!(
                    "{}: MetaMap length differs: {} vs {}",
                    path,
                    ea.len(),
                    eb.len()
                ));
            }
            for (entry_a, entry_b) in ea.iter().zip(eb.iter()) {
                if entry_a.key != entry_b.key {
                    return Some(format!(
                        "{}: key differs: {:?} vs {:?}",
                        path, entry_a.key, entry_b.key
                    ));
                }
                if let Some(diff) = compare_meta_values(
                    &entry_a.value,
                    &entry_b.value,
                    &format!("{}.{}", path, entry_a.key),
                ) {
                    return Some(diff);
                }
            }
            None
        }
        _ => Some(format!(
            "{}: variant mismatch: {:?} vs {:?}",
            path,
            std::mem::discriminant(a),
            std::mem::discriminant(b)
        )),
    }
}

#[test]
fn test_equivalence_simple_string() {
    let content = r#"---
title: Hello World
---"#;
    let block = make_rawblock(content);
    let context = make_ast_context();
    let mut diags1 = DiagnosticCollector::new();
    let mut diags2 = DiagnosticCollector::new();

    // New path: rawblock -> ConfigValue -> MetaValueWithSourceInfo
    let config = rawblock_to_config_value(&block, &mut diags1);
    let via_config = config_value_to_meta(&config);

    // Legacy path: rawblock -> MetaValueWithSourceInfo directly
    let direct = rawblock_to_meta_with_source_info(&block, &context, &mut diags2);

    // Compare
    if let Some(diff) = compare_meta_values(&via_config, &direct, "root") {
        panic!("Equivalence failed: {}", diff);
    }
}

#[test]
fn test_equivalence_boolean() {
    let content = r#"---
toc: true
draft: false
---"#;
    let block = make_rawblock(content);
    let context = make_ast_context();
    let mut diags1 = DiagnosticCollector::new();
    let mut diags2 = DiagnosticCollector::new();

    let config = rawblock_to_config_value(&block, &mut diags1);
    let via_config = config_value_to_meta(&config);
    let direct = rawblock_to_meta_with_source_info(&block, &context, &mut diags2);

    if let Some(diff) = compare_meta_values(&via_config, &direct, "root") {
        panic!("Equivalence failed: {}", diff);
    }
}

#[test]
fn test_equivalence_array() {
    let content = r#"---
authors:
  - Alice
  - Bob
---"#;
    let block = make_rawblock(content);
    let context = make_ast_context();
    let mut diags1 = DiagnosticCollector::new();
    let mut diags2 = DiagnosticCollector::new();

    let config = rawblock_to_config_value(&block, &mut diags1);
    let via_config = config_value_to_meta(&config);
    let direct = rawblock_to_meta_with_source_info(&block, &context, &mut diags2);

    if let Some(diff) = compare_meta_values(&via_config, &direct, "root") {
        panic!("Equivalence failed: {}", diff);
    }
}

#[test]
fn test_equivalence_nested_map() {
    let content = r#"---
format:
  html:
    toc: true
    theme: default
---"#;
    let block = make_rawblock(content);
    let context = make_ast_context();
    let mut diags1 = DiagnosticCollector::new();
    let mut diags2 = DiagnosticCollector::new();

    let config = rawblock_to_config_value(&block, &mut diags1);
    let via_config = config_value_to_meta(&config);
    let direct = rawblock_to_meta_with_source_info(&block, &context, &mut diags2);

    if let Some(diff) = compare_meta_values(&via_config, &direct, "root") {
        panic!("Equivalence failed: {}", diff);
    }
}

#[test]
fn test_equivalence_markdown_in_string() {
    let content = r#"---
title: This has *emphasis*
---"#;
    let block = make_rawblock(content);
    let context = make_ast_context();
    let mut diags1 = DiagnosticCollector::new();
    let mut diags2 = DiagnosticCollector::new();

    let config = rawblock_to_config_value(&block, &mut diags1);
    let via_config = config_value_to_meta(&config);
    let direct = rawblock_to_meta_with_source_info(&block, &context, &mut diags2);

    if let Some(diff) = compare_meta_values(&via_config, &direct, "root") {
        panic!("Equivalence failed: {}", diff);
    }
}

#[test]
fn test_equivalence_str_tag() {
    // Note: The !str tag behavior differs slightly between the two paths.
    // The legacy path wraps in Span with "yaml-tagged-string" class,
    // while the new path produces Scalar(String).
    // config_value_to_meta converts this to MetaString, which is different.
    // This test documents this known difference.
    let content = r#"---
filename: !str _foo_.py
---"#;
    let block = make_rawblock(content);
    let context = make_ast_context();
    let mut diags1 = DiagnosticCollector::new();
    let mut diags2 = DiagnosticCollector::new();

    let config = rawblock_to_config_value(&block, &mut diags1);
    let via_config = config_value_to_meta(&config);
    let direct = rawblock_to_meta_with_source_info(&block, &context, &mut diags2);

    // The legacy path produces MetaInlines with a Span wrapper,
    // while the new path produces MetaString.
    // Both preserve the literal value "_foo_.py", just in different structures.

    // Extract the underlying string value from both
    // Note: The config path produces MetaString for !str, while direct path produces MetaInlines
    let via_config_value = match &via_config {
        MetaValueWithSourceInfo::MetaMap { entries, .. } => {
            let entry = entries.iter().find(|e| e.key == "filename").unwrap();
            match &entry.value {
                // Config path: !str → Scalar(String) → MetaString
                MetaValueWithSourceInfo::MetaString { value, .. } => value.clone(),
                _ => panic!(
                    "Expected MetaString from config path, got: {:?}",
                    entry.value
                ),
            }
        }
        _ => panic!("Expected MetaMap"),
    };

    let direct_value = match &direct {
        MetaValueWithSourceInfo::MetaMap { entries, .. } => {
            let entry = entries.iter().find(|e| e.key == "filename").unwrap();
            match &entry.value {
                MetaValueWithSourceInfo::MetaInlines { content, .. } => {
                    // !str produces plain Str inline (no Span wrapper)
                    if let Some(Inline::Str(s)) = content.first() {
                        s.text.clone()
                    } else {
                        panic!("Expected Str in MetaInlines, got: {:?}", content)
                    }
                }
                _ => panic!("Expected MetaInlines from direct path"),
            }
        }
        _ => panic!("Expected MetaMap"),
    };

    assert_eq!(
        via_config_value, direct_value,
        "Both paths should preserve the literal string value"
    );
}

// =============================================================================
// No Diagnostics for Valid Input
// =============================================================================

#[test]
fn test_no_diagnostics_for_valid_input() {
    let content = r#"---
title: Hello World
toc: true
items:
  - first
  - second
format:
  html:
    theme: cosmo
---"#;
    let block = make_rawblock(content);
    let mut diagnostics = DiagnosticCollector::new();

    let _ = rawblock_to_config_value(&block, &mut diagnostics);

    assert!(
        diagnostics.diagnostics().is_empty(),
        "Expected no diagnostics for valid input, got: {:?}",
        diagnostics.into_diagnostics()
    );
}
