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
            if pandoc.blocks.len() == 1 {
                if let quarto_pandoc_types::Block::Paragraph(p) = &mut pandoc.blocks[0] {
                    return ConfigValueKind::PandocInlines(mem::take(&mut p.content));
                }
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

            // Return error recovery span
            let span = Span {
                attr: (
                    String::new(),
                    vec!["yaml-markdown-syntax-error".to_string()],
                    LinkedHashMap::new(),
                ),
                content: vec![Inline::Str(Str {
                    text: value.to_string(),
                    source_info: quarto_source_map::SourceInfo::default(),
                })],
                source_info: quarto_source_map::SourceInfo::default(),
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
        let config_items: Vec<ConfigValue> = items
            .into_iter()
            .map(|item| yaml_to_config_value(item, context, diagnostics))
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
                entry.key.yaml.as_str().map(|key_str| ConfigMapEntry {
                    key: key_str.to_string(),
                    key_source: entry.key_span,
                    value: yaml_to_config_value(entry.value, context, diagnostics),
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
                            source_info: quarto_source_map::SourceInfo::default(),
                            attr_source: AttrSourceInfo::empty(),
                        };
                        ConfigValueKind::PandocInlines(vec![Inline::Span(span)])
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
        Err(e) => panic!(
            "(unimplemented syntax error - this is a bug!) Failed to parse metadata block as YAML: {}",
            e
        ),
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
