/*
 * meta.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * This module contains parsing and conversion functions for metadata.
 * Phase 5: Removed legacy MetaValueWithSourceInfo-based functions.
 * Now uses ConfigValue API exclusively.
 */

use crate::utils::output::VerboseOutput;
use hashlink::LinkedHashMap;
use quarto_pandoc_types::{AttrSourceInfo, Inline, RawBlock, Span, Str};
use std::{io, mem};

// =============================================================================
// yaml_to_config_value: Unified YAML → ConfigValue conversion
// =============================================================================

use quarto_config::{ConfigMapEntry, ConfigValue, ConfigValueKind, InterpretationContext, MergeOp};
use yaml_rust2::Yaml;

/// Parse a YAML string as markdown and return ConfigValue with PandocInlines/PandocBlocks.
///
/// - If `is_explicit_md` is true: This is a !md tagged value, ERROR on parse failure
/// - If `is_explicit_md` is false: This is an untagged value, WARN on parse failure
fn parse_yaml_string_as_markdown_to_config(
    value: &str,
    source_info: &quarto_source_map::SourceInfo,
    is_explicit_md: bool,
    diagnostics: &mut crate::utils::diagnostic_collector::DiagnosticCollector,
) -> ConfigValueKind {
    use crate::readers;
    use quarto_error_reporting::DiagnosticMessageBuilder;

    let mut output_stream = VerboseOutput::Sink(io::sink());
    let result = readers::qmd::read(
        value.as_bytes(),
        false,
        "<metadata>",
        &mut output_stream,
        true,
        Some(source_info.clone()),
    );

    match result {
        Ok((mut pandoc, _, warnings)) => {
            // Propagate warnings from recursive parse
            for warning in warnings {
                diagnostics.add(warning);
            }
            // Parse succeeded - return as PandocInlines or PandocBlocks
            if pandoc.blocks.len() == 1
                && let quarto_pandoc_types::Block::Paragraph(p) = &mut pandoc.blocks[0]
            {
                return ConfigValueKind::PandocInlines(mem::take(&mut p.content));
            }
            ConfigValueKind::PandocBlocks(pandoc.blocks)
        }
        Err(_parse_errors) => {
            if is_explicit_md {
                // !md tag: ERROR on parse failure
                let diagnostic =
                    DiagnosticMessageBuilder::error("Failed to parse !md tagged value")
                        .with_code("Q-1-20")
                        .with_location(source_info.clone())
                        .problem("The `!md` tag requires valid markdown syntax")
                        .add_detail(format!("Could not parse: {}", value))
                        .add_hint("Remove the `!md` tag or fix the markdown syntax")
                        .build();
                diagnostics.add(diagnostic);
            } else {
                // Untagged: WARN on parse failure
                let diagnostic = DiagnosticMessageBuilder::warning(
                    "Failed to parse metadata value as markdown",
                )
                .with_code("Q-1-20")
                .with_location(source_info.clone())
                .problem(format!("Could not parse '{}' as markdown", value))
                .add_hint(
                    "Add the `!str` tag to treat this as a plain string, or fix the markdown syntax",
                )
                .build();
                diagnostics.add(diagnostic);
            }

            // Return error recovery span. The bytes are the raw YAML
            // value; reuse the caller's source_info so attribution
            // points at the offending YAML range rather than nowhere.
            let span = Span {
                attr: (
                    String::new(),
                    vec!["yaml-markdown-syntax-error".to_string()],
                    LinkedHashMap::new(),
                ),
                content: vec![Inline::Str(Str {
                    text: value.to_string(),
                    source_info: source_info.clone(),
                })],
                source_info: source_info.clone(),
                attr_source: AttrSourceInfo::empty(),
            };
            ConfigValueKind::PandocInlines(vec![Inline::Span(span)])
        }
    }
}

/// Convert YamlWithSourceInfo to ConfigValue with context-aware interpretation.
///
/// This is the unified conversion function that handles both:
/// - Document metadata (frontmatter) - strings are parsed as markdown by default
/// - Project config (_quarto.yml) - strings are kept literal by default
///
/// # Interpretation Rules
///
/// ## Tag Handling (same for both contexts)
/// - `!prefer`: Sets merge_op to Prefer
/// - `!concat`: Sets merge_op to Concat
/// - `!path`: Creates Path(String) variant
/// - `!glob`: Creates Glob(String) variant
/// - `!expr`: Creates Expr(String) variant
/// - `!str`: Keeps string literal → Scalar(String)
/// - `!md`: Parses string as markdown → PandocInlines/PandocBlocks
///
/// ## Default for Untagged Strings (context-dependent)
/// - `DocumentMetadata`: Parse as markdown → PandocInlines/PandocBlocks
/// - `ProjectConfig`: Keep literal → Scalar(String)
///
/// # Example
///
/// ```rust,ignore
/// // Document metadata context (frontmatter)
/// let config = yaml_to_config_value(yaml, InterpretationContext::DocumentMetadata, &mut diags);
/// // Untagged strings are parsed as markdown
///
/// // Project config context (_quarto.yml)
/// let config = yaml_to_config_value(yaml, InterpretationContext::ProjectConfig, &mut diags);
/// // Untagged strings are kept literal
/// ```
pub fn yaml_to_config_value(
    yaml: quarto_yaml::YamlWithSourceInfo,
    context: InterpretationContext,
    diagnostics: &mut crate::utils::diagnostic_collector::DiagnosticCollector,
) -> ConfigValue {
    let mut path: Vec<String> = Vec::new();
    yaml_to_config_value_at(yaml, context, diagnostics, &mut path)
}

/// Recursive worker for [`yaml_to_config_value`], threading the
/// map-key path from the metadata root so untagged scalars can
/// consult the key-path annotation table
/// ([`super::meta_annotations`]; bd-v7ixzsp5). `path` is maintained
/// by the map branch (push key / recurse / pop); arrays are
/// transparent (items share the array's path).
fn yaml_to_config_value_at(
    yaml: quarto_yaml::YamlWithSourceInfo,
    context: InterpretationContext,
    diagnostics: &mut crate::utils::diagnostic_collector::DiagnosticCollector,
    path: &mut Vec<String>,
) -> ConfigValue {
    // Parse tags using quarto-config's tag parser
    let parsed_tag = if let Some((tag_str, tag_source)) = &yaml.tag {
        let mut tag_diags = Vec::new();
        let result = quarto_config::parse_tag(tag_str, tag_source, &mut tag_diags);
        for diag in tag_diags {
            diagnostics.add(diag);
        }
        result
    } else {
        Default::default()
    };

    let merge_op = parsed_tag.merge_op.unwrap_or(MergeOp::Concat);
    let interpretation = parsed_tag.interpretation;
    let unknown_components = parsed_tag.unknown_components;

    // Handle compound types first (arrays and maps)
    if yaml.is_array() {
        let (items, source_info) = yaml.into_array().unwrap();
        // Arrays are transparent for the annotation path: items
        // share the array's key path.
        let config_items: Vec<ConfigValue> = items
            .into_iter()
            .map(|item| yaml_to_config_value_at(item, context, diagnostics, path))
            .collect();

        return ConfigValue {
            value: ConfigValueKind::Array(config_items),
            source_info,
            merge_op,
        };
    }

    if yaml.is_hash() {
        let (entries, source_info) = yaml.into_hash().unwrap();
        let config_entries: Vec<ConfigMapEntry> = entries
            .into_iter()
            .filter_map(|entry| {
                entry.key.yaml.as_str().map(|key_str| {
                    path.push(key_str.to_string());
                    let value = yaml_to_config_value_at(entry.value, context, diagnostics, path);
                    path.pop();
                    ConfigMapEntry {
                        key: key_str.to_string(),
                        key_source: entry.key_span,
                        value,
                    }
                })
            })
            .collect();

        return ConfigValue {
            value: ConfigValueKind::Map(config_entries),
            source_info,
            merge_op,
        };
    }

    // Handle scalar values
    let source_info = yaml.source_info.clone();
    let yaml_value = yaml.yaml;

    match yaml_value {
        Yaml::String(s) => {
            // Determine how to interpret the string based on tag and context
            let value = match interpretation {
                // Explicit tags always override context
                Some(quarto_config::Interpretation::Path) => ConfigValueKind::Path(s),
                Some(quarto_config::Interpretation::Glob) => ConfigValueKind::Glob(s),
                Some(quarto_config::Interpretation::Expr) => ConfigValueKind::Expr(s),
                Some(quarto_config::Interpretation::PlainString) => {
                    // !str: Keep as literal scalar
                    ConfigValueKind::Scalar(Yaml::String(s))
                }
                Some(quarto_config::Interpretation::Markdown) => {
                    // !md: Parse as markdown
                    parse_yaml_string_as_markdown_to_config(&s, &source_info, true, diagnostics)
                }
                None => {
                    // Check if there are unknown tag components to preserve
                    if !unknown_components.is_empty() {
                        // Create Span wrapper to preserve unknown tag information
                        // Use the first unknown component as the tag name (e.g., "date" from !date)
                        let tag_name = unknown_components.join("_");
                        let mut attributes = LinkedHashMap::new();
                        attributes.insert("tag".to_string(), tag_name);
                        let span = Span {
                            attr: (
                                String::new(),
                                vec!["yaml-tagged-string".to_string()],
                                attributes,
                            ),
                            content: vec![Inline::Str(Str {
                                text: s,
                                source_info: source_info.clone(),
                            })],
                            // Wrapper around the tagged scalar — reuse
                            // the value's source range so attribution
                            // points at the YAML.
                            source_info: source_info.clone(),
                            attr_source: AttrSourceInfo::empty(),
                        };
                        ConfigValueKind::PandocInlines(vec![Inline::Span(span)])
                    } else if let Some(annotated) =
                        super::meta_annotations::annotated_interpretation(path)
                    {
                        // No tag, but the key path carries an
                        // interpretation annotation (bd-v7ixzsp5;
                        // e.g. `listing.contents` entries are globs,
                        // never markdown). Explicit tags took the
                        // branches above, so annotations only replace
                        // the untagged default.
                        match annotated {
                            quarto_config::Interpretation::Path => ConfigValueKind::Path(s),
                            quarto_config::Interpretation::Glob => ConfigValueKind::Glob(s),
                            quarto_config::Interpretation::Expr => ConfigValueKind::Expr(s),
                            quarto_config::Interpretation::PlainString => {
                                ConfigValueKind::Scalar(Yaml::String(s))
                            }
                            quarto_config::Interpretation::Markdown => {
                                parse_yaml_string_as_markdown_to_config(
                                    &s,
                                    &source_info,
                                    true,
                                    diagnostics,
                                )
                            }
                        }
                    } else {
                        // No tag: Use context-dependent default
                        match context {
                            InterpretationContext::DocumentMetadata => {
                                // Document metadata: parse as markdown
                                parse_yaml_string_as_markdown_to_config(
                                    &s,
                                    &source_info,
                                    false,
                                    diagnostics,
                                )
                            }
                            InterpretationContext::ProjectConfig => {
                                // Project config: keep literal
                                ConfigValueKind::Scalar(Yaml::String(s))
                            }
                        }
                    }
                }
            };

            ConfigValue {
                value,
                source_info,
                merge_op,
            }
        }

        Yaml::Boolean(b) => ConfigValue {
            value: ConfigValueKind::Scalar(Yaml::Boolean(b)),
            source_info,
            merge_op,
        },

        Yaml::Integer(i) => ConfigValue {
            value: ConfigValueKind::Scalar(Yaml::Integer(i)),
            source_info,
            merge_op,
        },

        Yaml::Real(r) => ConfigValue {
            value: ConfigValueKind::Scalar(Yaml::Real(r)),
            source_info,
            merge_op,
        },

        Yaml::Null => ConfigValue {
            value: ConfigValueKind::Scalar(Yaml::Null),
            source_info,
            merge_op,
        },

        Yaml::BadValue => ConfigValue {
            value: ConfigValueKind::Scalar(Yaml::Null),
            source_info,
            merge_op,
        },

        Yaml::Alias(_) => {
            // YAML aliases are resolved by yaml-rust2, so this shouldn't happen
            ConfigValue {
                value: ConfigValueKind::Scalar(Yaml::Null),
                source_info,
                merge_op,
            }
        }

        // Array and Hash should have been handled above
        Yaml::Array(_) | Yaml::Hash(_) => {
            unreachable!("Array/Hash should be handled by is_array/is_hash checks")
        }
    }
}

fn extract_between_delimiters(input: &str) -> Option<&str> {
    let parts: Vec<&str> = input.split("---").collect();
    if parts.len() >= 3 {
        Some(parts[1].trim())
    } else {
        None
    }
}

/// Convert RawBlock to ConfigValue using unified conversion.
///
/// This function:
/// 1. Preserves source location information
/// 2. Returns ConfigValue (the unified metadata type)
/// 3. Uses InterpretationContext::DocumentMetadata (parse strings as markdown by default)
///
/// # Panics
///
/// Panics if the RawBlock format is not "quarto_minus_metadata" or if YAML parsing fails.
/// These should be replaced with proper error handling in production.
pub fn rawblock_to_config_value(
    block: &RawBlock,
    diagnostics: &mut crate::utils::diagnostic_collector::DiagnosticCollector,
) -> ConfigValue {
    if block.format != "quarto_minus_metadata" {
        panic!(
            "Expected RawBlock with format 'quarto_minus_metadata', got {}",
            block.format
        );
    }

    // Extract YAML content between --- delimiters
    let content = extract_between_delimiters(&block.text).unwrap();

    // Calculate offsets within RawBlock.text
    // Find the actual position of the trimmed content in the original text
    // extract_between_delimiters trims the content, so we need to find where it actually starts
    let yaml_start = block.text.find(content).unwrap();

    // block.source_info is already quarto_source_map::SourceInfo
    let parent = block.source_info.clone();

    // Create Substring SourceInfo for the YAML content within the RawBlock
    let yaml_parent =
        quarto_source_map::SourceInfo::substring(parent, yaml_start, yaml_start + content.len());

    // Parse YAML with source tracking
    let yaml = match quarto_yaml::parse_with_parent(content, yaml_parent.clone()) {
        Ok(y) => y,
        Err(e) => {
            // Report the YAML parse error as a diagnostic
            diagnostics.error_at(
                format!("Failed to parse YAML frontmatter: {}", e),
                yaml_parent,
            );
            // Return an empty map as the metadata
            return ConfigValue {
                value: ConfigValueKind::Map(Vec::new()),
                source_info: block.source_info.clone(),
                merge_op: MergeOp::default(),
            };
        }
    };

    // Transform YamlWithSourceInfo to ConfigValue using document metadata context
    // (strings are parsed as markdown by default)
    let mut result =
        yaml_to_config_value(yaml, InterpretationContext::DocumentMetadata, diagnostics);

    // For the top-level metadata, replace the source_info with yaml_parent
    // to ensure it spans the entire YAML content, not just where the mapping starts
    if let ConfigValueKind::Map(_) = &result.value {
        result.source_info = yaml_parent;
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_yaml::YamlWithSourceInfo;

    fn si() -> quarto_source_map::SourceInfo {
        quarto_source_map::SourceInfo::for_test()
    }

    #[test]
    fn test_extract_between_delimiters_valid() {
        let input = "---\ntitle: Test\n---";
        let result = extract_between_delimiters(input);
        assert_eq!(result, Some("title: Test"));
    }

    #[test]
    fn test_extract_between_delimiters_with_extra_content() {
        let input = "---\ntitle: Test\n---\nBody text";
        let result = extract_between_delimiters(input);
        assert_eq!(result, Some("title: Test"));
    }

    #[test]
    fn test_extract_between_delimiters_missing_delimiters() {
        // Only one delimiter
        let input = "---\ntitle: Test";
        let result = extract_between_delimiters(input);
        assert_eq!(result, None);

        // No delimiters
        let input = "title: Test";
        let result = extract_between_delimiters(input);
        assert_eq!(result, None);
    }

    #[test]
    fn test_yaml_to_config_value_string_document_metadata() {
        let yaml = YamlWithSourceInfo::new_scalar(Yaml::String("test *bold*".to_string()), si());
        let mut diagnostics = crate::utils::diagnostic_collector::DiagnosticCollector::new();
        let result = yaml_to_config_value(
            yaml,
            InterpretationContext::DocumentMetadata,
            &mut diagnostics,
        );
        // In document metadata context, strings are parsed as markdown
        assert!(matches!(
            result.value,
            ConfigValueKind::PandocInlines(_) | ConfigValueKind::PandocBlocks(_)
        ));
    }

    #[test]
    fn test_yaml_to_config_value_string_project_config() {
        let yaml = YamlWithSourceInfo::new_scalar(Yaml::String("test string".to_string()), si());
        let mut diagnostics = crate::utils::diagnostic_collector::DiagnosticCollector::new();
        let result =
            yaml_to_config_value(yaml, InterpretationContext::ProjectConfig, &mut diagnostics);
        // In project config context, strings stay as literals
        assert!(matches!(
            result.value,
            ConfigValueKind::Scalar(Yaml::String(_))
        ));
    }

    // ─────────────────────────────────────────────────────────────
    // Key-path interpretation annotations (bd-v7ixzsp5, GH #456).
    //
    // `listing.contents` entries are globs, not markdown. Without
    // the annotation, the DocumentMetadata markdown default either
    // warns (Q-1-20, `*.qmd` fails to parse) or silently corrupts
    // the pattern (`p*osts*.qmd` parses as emphasis and
    // `as_plain_text` reconstructs `posts.qmd`).
    // ─────────────────────────────────────────────────────────────

    fn convert_doc_meta(
        yaml_text: &str,
    ) -> (ConfigValue, Vec<quarto_error_reporting::DiagnosticMessage>) {
        let yaml = quarto_yaml::parse(yaml_text).expect("valid yaml");
        let mut diagnostics = crate::utils::diagnostic_collector::DiagnosticCollector::new();
        let result = yaml_to_config_value(
            yaml,
            InterpretationContext::DocumentMetadata,
            &mut diagnostics,
        );
        (result, diagnostics.diagnostics().to_vec())
    }

    #[test]
    fn listing_contents_array_items_are_globs_not_markdown() {
        let (result, diags) =
            convert_doc_meta("listing:\n  contents:\n    - \"p*osts*.qmd\"\n    - \"*.qmd\"\n");
        let contents = result
            .get("listing")
            .and_then(|l| l.get("contents"))
            .expect("listing.contents");
        let ConfigValueKind::Array(items) = &contents.value else {
            panic!("contents should be an array, got {:?}", contents.value);
        };
        assert!(
            matches!(&items[0].value, ConfigValueKind::Glob(s) if s == "p*osts*.qmd"),
            "asterisks must survive verbatim (no markdown emphasis parse); got {:?}",
            items[0].value
        );
        assert!(
            matches!(&items[1].value, ConfigValueKind::Glob(s) if s == "*.qmd"),
            "got {:?}",
            items[1].value
        );
        assert!(
            diags.is_empty(),
            "no Q-1-20 markdown-parse warning for glob strings; got {:?}",
            diags
        );
    }

    #[test]
    fn listing_contents_string_shorthand_is_glob() {
        let (result, diags) = convert_doc_meta("listing:\n  contents: \"*.qmd\"\n");
        let contents = result
            .get("listing")
            .and_then(|l| l.get("contents"))
            .expect("listing.contents");
        assert!(
            matches!(&contents.value, ConfigValueKind::Glob(s) if s == "*.qmd"),
            "got {:?}",
            contents.value
        );
        assert!(diags.is_empty(), "got {:?}", diags);
    }

    #[test]
    fn listing_contents_annotation_leaves_sibling_keys_as_markdown() {
        let (result, _) = convert_doc_meta("title: \"*bold*\"\nlisting:\n  contents: \"*.qmd\"\n");
        let title = result.get("title").expect("title");
        assert!(
            matches!(&title.value, ConfigValueKind::PandocInlines(_)),
            "title keeps the markdown default; got {:?}",
            title.value
        );
    }

    #[test]
    fn listing_contents_inline_record_fields_keep_markdown() {
        let (result, _) = convert_doc_meta(
            "listing:\n  contents:\n    - title: \"*bold*\"\n      path: x.html\n",
        );
        let contents = result
            .get("listing")
            .and_then(|l| l.get("contents"))
            .expect("listing.contents");
        let ConfigValueKind::Array(items) = &contents.value else {
            panic!("contents should be an array");
        };
        let title = items[0].get("title").expect("record title");
        assert!(
            matches!(&title.value, ConfigValueKind::PandocInlines(_)),
            "map entries under contents extend the key path, so record \
             fields keep the markdown default; got {:?}",
            title.value
        );
    }

    #[test]
    fn listing_contents_explicit_tag_overrides_annotation() {
        let (result, _) = convert_doc_meta("listing:\n  contents: !str \"*.qmd\"\n");
        let contents = result
            .get("listing")
            .and_then(|l| l.get("contents"))
            .expect("listing.contents");
        assert!(
            matches!(&contents.value, ConfigValueKind::Scalar(Yaml::String(s)) if s == "*.qmd"),
            "explicit tags always win over the annotation; got {:?}",
            contents.value
        );
    }

    #[test]
    fn listing_contents_under_format_key_is_glob() {
        let (result, diags) =
            convert_doc_meta("format:\n  html:\n    listing:\n      contents: \"*.qmd\"\n");
        let contents = result
            .get("format")
            .and_then(|f| f.get("html"))
            .and_then(|h| h.get("listing"))
            .and_then(|l| l.get("contents"))
            .expect("format.html.listing.contents");
        assert!(
            matches!(&contents.value, ConfigValueKind::Glob(s) if s == "*.qmd"),
            "got {:?}",
            contents.value
        );
        assert!(diags.is_empty(), "got {:?}", diags);
    }

    #[test]
    fn test_yaml_to_config_value_boolean() {
        let yaml = YamlWithSourceInfo::new_scalar(Yaml::Boolean(true), si());
        let mut diagnostics = crate::utils::diagnostic_collector::DiagnosticCollector::new();
        let result =
            yaml_to_config_value(yaml, InterpretationContext::ProjectConfig, &mut diagnostics);
        assert!(matches!(
            result.value,
            ConfigValueKind::Scalar(Yaml::Boolean(true))
        ));
    }

    #[test]
    fn test_yaml_to_config_value_integer() {
        let yaml = YamlWithSourceInfo::new_scalar(Yaml::Integer(42), si());
        let mut diagnostics = crate::utils::diagnostic_collector::DiagnosticCollector::new();
        let result =
            yaml_to_config_value(yaml, InterpretationContext::ProjectConfig, &mut diagnostics);
        assert!(matches!(
            result.value,
            ConfigValueKind::Scalar(Yaml::Integer(42))
        ));
    }

    #[test]
    fn test_yaml_to_config_value_real() {
        let yaml = YamlWithSourceInfo::new_scalar(Yaml::Real("3.14".to_string()), si());
        let mut diagnostics = crate::utils::diagnostic_collector::DiagnosticCollector::new();
        let result =
            yaml_to_config_value(yaml, InterpretationContext::ProjectConfig, &mut diagnostics);
        assert!(matches!(
            result.value,
            ConfigValueKind::Scalar(Yaml::Real(_))
        ));
    }

    #[test]
    fn test_yaml_to_config_value_null() {
        let yaml = YamlWithSourceInfo::new_scalar(Yaml::Null, si());
        let mut diagnostics = crate::utils::diagnostic_collector::DiagnosticCollector::new();
        let result =
            yaml_to_config_value(yaml, InterpretationContext::ProjectConfig, &mut diagnostics);
        assert!(matches!(result.value, ConfigValueKind::Scalar(Yaml::Null)));
    }

    #[test]
    fn test_yaml_to_config_value_bad_value() {
        let yaml = YamlWithSourceInfo::new_scalar(Yaml::BadValue, si());
        let mut diagnostics = crate::utils::diagnostic_collector::DiagnosticCollector::new();
        let result =
            yaml_to_config_value(yaml, InterpretationContext::ProjectConfig, &mut diagnostics);
        // BadValue becomes Null
        assert!(matches!(result.value, ConfigValueKind::Scalar(Yaml::Null)));
    }

    #[test]
    fn test_yaml_to_config_value_alias() {
        // Aliases should be resolved by yaml-rust2, but if they somehow appear,
        // we treat them as Null
        let yaml = YamlWithSourceInfo::new_scalar(Yaml::Alias(1), si());
        let mut diagnostics = crate::utils::diagnostic_collector::DiagnosticCollector::new();
        let result =
            yaml_to_config_value(yaml, InterpretationContext::ProjectConfig, &mut diagnostics);
        assert!(matches!(result.value, ConfigValueKind::Scalar(Yaml::Null)));
    }

    #[test]
    fn test_yaml_to_config_value_with_str_tag() {
        let yaml = YamlWithSourceInfo::new_scalar_with_tag(
            Yaml::String("plain text".to_string()),
            si(),
            Some(("str".to_string(), si())),
        );
        let mut diagnostics = crate::utils::diagnostic_collector::DiagnosticCollector::new();
        let result = yaml_to_config_value(
            yaml,
            InterpretationContext::DocumentMetadata,
            &mut diagnostics,
        );
        // !str tag keeps string literal even in document metadata context
        assert!(matches!(
            result.value,
            ConfigValueKind::Scalar(Yaml::String(_))
        ));
    }

    #[test]
    fn test_yaml_to_config_value_with_path_tag() {
        let yaml = YamlWithSourceInfo::new_scalar_with_tag(
            Yaml::String("/path/to/file".to_string()),
            si(),
            Some(("path".to_string(), si())),
        );
        let mut diagnostics = crate::utils::diagnostic_collector::DiagnosticCollector::new();
        let result =
            yaml_to_config_value(yaml, InterpretationContext::ProjectConfig, &mut diagnostics);
        assert!(matches!(result.value, ConfigValueKind::Path(_)));
    }

    #[test]
    fn test_yaml_to_config_value_with_glob_tag() {
        let yaml = YamlWithSourceInfo::new_scalar_with_tag(
            Yaml::String("*.qmd".to_string()),
            si(),
            Some(("glob".to_string(), si())),
        );
        let mut diagnostics = crate::utils::diagnostic_collector::DiagnosticCollector::new();
        let result =
            yaml_to_config_value(yaml, InterpretationContext::ProjectConfig, &mut diagnostics);
        assert!(matches!(result.value, ConfigValueKind::Glob(_)));
    }

    #[test]
    fn test_yaml_to_config_value_with_expr_tag() {
        let yaml = YamlWithSourceInfo::new_scalar_with_tag(
            Yaml::String("1 + 2".to_string()),
            si(),
            Some(("expr".to_string(), si())),
        );
        let mut diagnostics = crate::utils::diagnostic_collector::DiagnosticCollector::new();
        let result =
            yaml_to_config_value(yaml, InterpretationContext::ProjectConfig, &mut diagnostics);
        assert!(matches!(result.value, ConfigValueKind::Expr(_)));
    }
}
