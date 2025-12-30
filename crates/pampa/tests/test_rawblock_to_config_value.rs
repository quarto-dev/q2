/*
 * test_rawblock_to_config_value.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Tests for rawblock_to_config_value function.
 * These tests verify the ConfigValue-based parsing path for document metadata.
 *
 * Updated for Phase 5: Removed equivalence tests with legacy MetaValueWithSourceInfo API.
 */

use pampa::pandoc::location::{Location, Range, SourceInfo};
use pampa::pandoc::{Inline, RawBlock, rawblock_to_config_value};
use pampa::utils::diagnostic_collector::DiagnosticCollector;
use quarto_pandoc_types::ConfigValueKind;

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
year: 2024
---"#;
    let block = make_rawblock(content);
    let mut diagnostics = DiagnosticCollector::new();

    let config = rawblock_to_config_value(&block, &mut diagnostics);

    match &config.value {
        ConfigValueKind::Map(entries) => {
            let year = entries
                .iter()
                .find(|e| e.key == "year")
                .expect("year not found");
            match &year.value.value {
                ConfigValueKind::Scalar(yaml_rust2::Yaml::Integer(n)) => {
                    assert_eq!(*n, 2024);
                }
                other => panic!("Expected Integer for year, got: {:?}", other),
            }
        }
        other => panic!("Expected Map, got: {:?}", other),
    }
}

#[test]
fn test_rawblock_to_config_value_array() {
    let content = r#"---
authors:
  - Alice
  - Bob
---"#;
    let block = make_rawblock(content);
    let mut diagnostics = DiagnosticCollector::new();

    let config = rawblock_to_config_value(&block, &mut diagnostics);

    match &config.value {
        ConfigValueKind::Map(entries) => {
            let authors = entries
                .iter()
                .find(|e| e.key == "authors")
                .expect("authors not found");
            match &authors.value.value {
                ConfigValueKind::Array(items) => {
                    assert_eq!(items.len(), 2, "Expected 2 authors");
                    // Array items in DocumentMetadata context are PandocInlines
                    assert!(matches!(items[0].value, ConfigValueKind::PandocInlines(_)));
                    assert!(matches!(items[1].value, ConfigValueKind::PandocInlines(_)));
                }
                other => panic!("Expected Array for authors, got: {:?}", other),
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
    toc: true
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
                ConfigValueKind::Map(inner) => {
                    let html = inner
                        .iter()
                        .find(|e| e.key == "html")
                        .expect("html not found");
                    match &html.value.value {
                        ConfigValueKind::Map(html_opts) => {
                            let toc = html_opts
                                .iter()
                                .find(|e| e.key == "toc")
                                .expect("toc not found");
                            assert!(matches!(
                                toc.value.value,
                                ConfigValueKind::Scalar(yaml_rust2::Yaml::Boolean(true))
                            ));
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
fn test_rawblock_to_config_value_parses_markdown() {
    let content = r#"---
title: This has *emphasis*
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
                    // Should contain Emph element for *emphasis*
                    let has_emph = inlines
                        .iter()
                        .any(|inline| matches!(inline, Inline::Emph(_)));
                    assert!(has_emph, "Expected Emph element in parsed markdown");
                }
                other => panic!("Expected PandocInlines for title, got: {:?}", other),
            }
        }
        other => panic!("Expected Map, got: {:?}", other),
    }
}

// =============================================================================
// Tag Tests
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
            // !str tag produces Scalar(String), not PandocInlines
            match &filename.value.value {
                ConfigValueKind::Scalar(yaml_rust2::Yaml::String(s)) => {
                    assert_eq!(s, "_foo_.py");
                }
                other => panic!("Expected Scalar(String) for !str tag, got: {:?}", other),
            }
        }
        other => panic!("Expected Map, got: {:?}", other),
    }
}

#[test]
fn test_rawblock_to_config_value_path_tag() {
    let content = r#"---
image: !path images/logo.png
---"#;
    let block = make_rawblock(content);
    let mut diagnostics = DiagnosticCollector::new();

    let config = rawblock_to_config_value(&block, &mut diagnostics);

    match &config.value {
        ConfigValueKind::Map(entries) => {
            let image = entries
                .iter()
                .find(|e| e.key == "image")
                .expect("image not found");
            match &image.value.value {
                ConfigValueKind::Path(p) => {
                    assert_eq!(p, "images/logo.png");
                }
                other => panic!("Expected Path for !path tag, got: {:?}", other),
            }
        }
        other => panic!("Expected Map, got: {:?}", other),
    }
}

#[test]
fn test_rawblock_to_config_value_glob_tag() {
    let content = r#"---
files: !glob posts/*/index.qmd
---"#;
    let block = make_rawblock(content);
    let mut diagnostics = DiagnosticCollector::new();

    let config = rawblock_to_config_value(&block, &mut diagnostics);

    match &config.value {
        ConfigValueKind::Map(entries) => {
            let files = entries
                .iter()
                .find(|e| e.key == "files")
                .expect("files not found");
            match &files.value.value {
                ConfigValueKind::Glob(g) => {
                    assert_eq!(g, "posts/*/index.qmd");
                }
                other => panic!("Expected Glob for !glob tag, got: {:?}", other),
            }
        }
        other => panic!("Expected Map, got: {:?}", other),
    }
}

#[test]
fn test_rawblock_to_config_value_expr_tag() {
    let content = r#"---
date: !expr Sys.Date()
---"#;
    let block = make_rawblock(content);
    let mut diagnostics = DiagnosticCollector::new();

    let config = rawblock_to_config_value(&block, &mut diagnostics);

    match &config.value {
        ConfigValueKind::Map(entries) => {
            let date = entries
                .iter()
                .find(|e| e.key == "date")
                .expect("date not found");
            match &date.value.value {
                ConfigValueKind::Expr(e) => {
                    assert_eq!(e, "Sys.Date()");
                }
                other => panic!("Expected Expr for !expr tag, got: {:?}", other),
            }
        }
        other => panic!("Expected Map, got: {:?}", other),
    }
}

#[test]
fn test_rawblock_to_config_value_prefer_tag() {
    let content = r#"---
title: !prefer My Title
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
            // !prefer sets the merge_op, but value should still be PandocInlines
            assert!(
                matches!(title.value.value, ConfigValueKind::PandocInlines(_)),
                "Expected PandocInlines for !prefer tagged value"
            );
            assert!(
                matches!(title.value.merge_op, quarto_pandoc_types::MergeOp::Prefer),
                "Expected Prefer merge_op"
            );
        }
        other => panic!("Expected Map, got: {:?}", other),
    }
}

// =============================================================================
// Source Tracking Tests
// =============================================================================

#[test]
fn test_rawblock_to_config_value_preserves_source_info() {
    let content = r#"---
title: Hello
---"#;
    let block = make_rawblock(content);
    let mut diagnostics = DiagnosticCollector::new();

    let config = rawblock_to_config_value(&block, &mut diagnostics);

    // The config itself should have source info
    let default_source = quarto_source_map::SourceInfo::default();
    assert!(
        config.source_info != default_source,
        "Config source should be tracked"
    );

    // The entries should also have source info
    if let ConfigValueKind::Map(entries) = &config.value {
        let title = entries
            .iter()
            .find(|e| e.key == "title")
            .expect("title not found");
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
