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

/// Parse a config string as qmd markdown, yielding `PandocInlines` (for a
/// single-paragraph result) or `PandocBlocks`.
///
/// Public entry point for consumers that re-interpret specific
/// project-config strings as markdown after load — e.g. quarto-core's
/// `ConfigMarkdownTransform`, which applies markdown semantics to website
/// presentation keys (`website.title`, `page-footer` regions, …) so
/// shortcodes and inline markup behave as they do in document metadata.
///
/// Uses untagged-value semantics: a parse failure emits a Q-1-20 *warning*
/// into `diagnostics` and falls back to an error-recovery span carrying the
/// literal text.
pub fn parse_config_string_as_markdown(
    value: &str,
    source_info: &quarto_source_map::SourceInfo,
    diagnostics: &mut Vec<quarto_error_reporting::DiagnosticMessage>,
) -> ConfigValueKind {
    let mut collector = crate::utils::diagnostic_collector::DiagnosticCollector::new();
    let mut kind =
        parse_yaml_string_as_markdown_to_config(value, source_info, false, &mut collector);
    diagnostics.extend(collector.into_diagnostics());
    unwrap_lone_figure(&mut kind);
    kind
}

/// Undo the qmd reader's single-image-paragraph → `Figure` desugar for
/// config strings (bd-page-footer-image-items-stmpikgo).
///
/// Config strings are inline presentation contexts — footer/navbar
/// item text, titles, captions — where figure-with-caption semantics
/// is never wanted: consumers render inlines and would drop a Figure
/// block on the floor. A lone image with alt text is the one shape
/// the reader turns into a Figure, so unwrap it back to the image the
/// author wrote, reassembling the attr the desugar split (id on the
/// figure, classes/attributes on the image).
///
/// Deliberately *not* applied to `!md`-tagged values
/// ([`parse_yaml_string_as_markdown_to_config`] with
/// `is_explicit_md = true`): those are explicit block-context
/// markdown, where figures persist.
fn unwrap_lone_figure(kind: &mut ConfigValueKind) {
    use quarto_pandoc_types::Block;

    let ConfigValueKind::PandocBlocks(blocks) = kind else {
        return;
    };
    let [Block::Figure(figure)] = &mut blocks[..] else {
        return;
    };
    let [Block::Plain(plain)] = &mut figure.content[..] else {
        return;
    };
    let [Inline::Image(image)] = &mut plain.content[..] else {
        return;
    };
    image.attr.0 = figure.attr.0.clone();
    image.attr_source.id = figure.attr_source.id.clone();
    let image = image.clone();
    *kind = ConfigValueKind::PandocInlines(vec![Inline::Image(image)]);
}

/// Fold the recursive parse's own diagnostics into the Q-1-20 message as
/// located details, so the author learns *what* is wrong with the markdown
/// rather than just that it failed
/// (bd-q120-masks-config-md-diagnostic-a039r80t). Child spans arrive
/// already rerooted into the config file by `readers::qmd::read`'s Err
/// path; Q-1-20's severity (warning untagged, error under `!md`) is the
/// severity of the whole folded message.
fn fold_child_diagnostics(
    mut builder: quarto_error_reporting::DiagnosticMessageBuilder,
    children: &[quarto_error_reporting::DiagnosticMessage],
) -> quarto_error_reporting::DiagnosticMessageBuilder {
    use quarto_error_reporting::DetailKind;

    let mut seen_hints = std::collections::HashSet::new();
    for child in children {
        // The child's `problem` is its anchored explanation (the ariadne
        // renderer draws it as the label on the child's main span), so
        // fold it in as the located label — the config rendering then
        // mirrors what the same markdown produces in a document body.
        // The code + title identify the underlying error in a footer
        // line, where the author can search for it.
        let identity = match &child.code {
            Some(code) => format!("[{}] {}", code, child.title),
            None => child.title.clone(),
        };
        builder = match (&child.location, &child.problem) {
            (Some(location), Some(problem)) => builder
                .add_info_at(problem.as_str().to_string(), location.clone())
                .add_info(identity),
            (Some(location), None) => builder.add_info_at(identity, location.clone()),
            (None, Some(problem)) => builder
                .add_info(identity)
                .add_info(problem.as_str().to_string()),
            (None, None) => builder.add_info(identity),
        };
        for detail in &child.details {
            let text = detail.content.as_str().to_string();
            builder = match (&detail.location, &detail.kind) {
                (Some(location), DetailKind::Error) => {
                    builder.add_detail_at(text, location.clone())
                }
                (Some(location), DetailKind::Info) => builder.add_info_at(text, location.clone()),
                (Some(location), _) => builder.add_note_at(text, location.clone()),
                (None, DetailKind::Error) => builder.add_detail(text),
                (None, DetailKind::Info) => builder.add_info(text),
                (None, _) => builder.add_note(text),
            };
        }
        for hint in &child.hints {
            if seen_hints.insert(hint.as_str().to_string()) {
                builder = builder.add_hint(hint.as_str().to_string());
            }
        }
    }
    builder
}

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
        Err(parse_errors) => {
            let diagnostic = if is_explicit_md {
                // !md tag: ERROR on parse failure
                fold_child_diagnostics(
                    DiagnosticMessageBuilder::error("Failed to parse !md tagged value")
                        .with_code("Q-1-20")
                        .with_location(source_info.clone())
                        .problem("The `!md` tag requires valid markdown syntax")
                        .add_detail(format!("Could not parse: {}", value)),
                    &parse_errors,
                )
                .add_hint("Remove the `!md` tag or fix the markdown syntax")
                .build()
            } else {
                // Untagged: WARN on parse failure
                fold_child_diagnostics(
                    DiagnosticMessageBuilder::warning(
                        "Failed to parse metadata value as markdown",
                    )
                    .with_code("Q-1-20")
                    .with_location(source_info.clone())
                    .problem(format!("Could not parse '{}' as markdown", value)),
                    &parse_errors,
                )
                .add_hint(
                    "Add the `!str` tag to treat this as a plain string, or fix the markdown syntax",
                )
                .build()
            };
            diagnostics.add(diagnostic);

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
    // Provenance of the *decoded* content, when derivation ran (see
    // `YamlWithSourceInfo::content_source_info`'s contract). `source_info`
    // above describes the raw source text — quote delimiters, per-line
    // block-scalar indentation — so pairing a decoded string with it and
    // doing offset arithmetic drifts by whatever decoding stripped. This is
    // the base to use instead, everywhere a decoded string is re-parsed or
    // stored for later re-parsing.
    let content_source_info = yaml.content_source_info().cloned();
    let yaml_value = yaml.yaml;
    // Fallback to `source_info` when no content provenance is available
    // (non-scalar — unreachable here since compound types returned above —
    // hand-built node, unresolved alias, or a `quarto-yaml` desync).
    let markdown_base = content_source_info.as_ref().unwrap_or(&source_info);

    match yaml_value {
        Yaml::String(s) => {
            // q2 has already established this node is a string scalar, so a
            // `None` here is a desync report, not a silent fallback (see
            // `content_provenance_desync_warning`'s doc comment). Mirrors
            // `quarto_config::convert::content_provenance_desync_warning` —
            // the two must stay in lockstep for the same reason
            // `scalar_string_with_content_provenance` does.
            if content_source_info.is_none() {
                diagnostics.add(content_provenance_desync_warning(&source_info));
            }

            // Determine how to interpret the string based on tag and context
            let value = match interpretation {
                // Explicit tags always override context
                Some(quarto_config::Interpretation::Path) => ConfigValueKind::Path(s),
                Some(quarto_config::Interpretation::Glob) => ConfigValueKind::Glob(s),
                Some(quarto_config::Interpretation::Expr) => ConfigValueKind::Expr(s),
                Some(quarto_config::Interpretation::PlainString) => {
                    // !str: Keep as literal scalar
                    scalar_string_with_content_provenance(s, &content_source_info)
                }
                Some(quarto_config::Interpretation::Markdown) => {
                    // !md: Parse as markdown
                    parse_yaml_string_as_markdown_to_config(&s, markdown_base, true, diagnostics)
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
                                scalar_string_with_content_provenance(s, &content_source_info)
                            }
                            quarto_config::Interpretation::Markdown => {
                                parse_yaml_string_as_markdown_to_config(
                                    &s,
                                    markdown_base,
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
                                    markdown_base,
                                    false,
                                    diagnostics,
                                )
                            }
                            InterpretationContext::ProjectConfig => {
                                // Project config: keep literal
                                scalar_string_with_content_provenance(s, &content_source_info)
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
            value: ConfigValueKind::scalar(Yaml::Boolean(b)),
            source_info,
            merge_op,
        },

        Yaml::Integer(i) => ConfigValue {
            value: ConfigValueKind::scalar(Yaml::Integer(i)),
            source_info,
            merge_op,
        },

        Yaml::Real(r) => ConfigValue {
            value: ConfigValueKind::scalar(Yaml::Real(r)),
            source_info,
            merge_op,
        },

        Yaml::Null => ConfigValue {
            value: ConfigValueKind::scalar(Yaml::Null),
            source_info,
            merge_op,
        },

        Yaml::BadValue => ConfigValue {
            value: ConfigValueKind::scalar(Yaml::Null),
            source_info,
            merge_op,
        },

        Yaml::Alias(_) => {
            // YAML aliases are resolved by yaml-rust2, so this shouldn't happen
            ConfigValue {
                value: ConfigValueKind::scalar(Yaml::Null),
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

/// Build a `Scalar(String)` for a literal (non-markdown) string value,
/// carrying the decoded-content provenance derived by the YAML reader when
/// available. This is the deferred-project-config path's entry point:
/// `ConfigMarkdownTransform` (`quarto-core/src/transforms/config_markdown.rs`)
/// reads `content_source_info` back off the stored value when it later
/// re-parses a blessed key (e.g. `website.title`) as markdown.
///
/// Deliberately scoped to `Yaml::String`: q2 has no consumer that does
/// sub-offset arithmetic into a non-string scalar (a later provenance-desync
/// warning's rule is "`None` on a *string* scalar is a bug"), so every other
/// scalar kind continues to construct via `ConfigValueKind::scalar` with no
/// provenance.
fn scalar_string_with_content_provenance(
    s: String,
    content_source_info: &Option<quarto_source_map::SourceInfo>,
) -> ConfigValueKind {
    match content_source_info {
        Some(csi) => ConfigValueKind::scalar_with_provenance(Yaml::String(s), csi.clone()),
        None => ConfigValueKind::scalar(Yaml::String(s)),
    }
}

/// Build the consistency warning for a `Yaml::String` scalar whose
/// `content_source_info` came back `None`. Per
/// `YamlWithSourceInfo::content_source_info`'s doc comment, `None` on a node
/// already known to be a scalar means one of: no derivation ran (the node
/// was hand-built, e.g. in a test, or is an unresolved alias — `Yaml::Alias`
/// is excluded by the `Yaml::String` scoping here, so that half needs no
/// special case), or the lockstep derivation desynced (a `quarto-yaml` bug).
/// Either way this is worth reporting, but never worth failing a render
/// over — warning-level and non-fatal, a wrong caret beats no output.
///
/// Deliberately carries **no `Q-` code**: this is an internal consistency
/// signal a user cannot act on, not a user-facing/documented error (adding
/// one would require a `docs/errors/` page per `cargo xtask lint`'s
/// `error-docs-page-missing`/`error-docs-sidebar-unlisted` rules).
///
/// Mirrors `quarto_config::convert::content_provenance_desync_warning` — the
/// two must stay in lockstep for the same reason
/// `scalar_string_with_content_provenance` does (see its doc comment).
fn content_provenance_desync_warning(
    source_info: &quarto_source_map::SourceInfo,
) -> quarto_error_reporting::DiagnosticMessage {
    quarto_error_reporting::DiagnosticMessageBuilder::warning(
        "YAML string scalar has no content provenance",
    )
    .with_location(source_info.clone())
    .problem(
        "`YamlWithSourceInfo::content_source_info()` returned `None` for a node already \
         established to be a `Yaml::String` scalar",
    )
    .add_detail(
        "Expected only for a hand-built test fixture (no derivation ran) or an unresolved \
         alias; otherwise this is a `quarto-yaml` provenance-derivation desync",
    )
    .add_hint(
        "Falling back to the raw node span, which may misalign a caret pointed at decoded content",
    )
    .build()
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
        // .with_content_provenance(si()): a hand-built node derives no
        // content provenance by construction; attach a synthetic one so
        // this fixture doesn't trip the desync warning tested separately
        // below (this test is about markdown interpretation, not that
        // warning).
        let yaml = YamlWithSourceInfo::new_scalar(Yaml::String("test *bold*".to_string()), si())
            .with_content_provenance(si());
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
        // See the sibling document-metadata test above for why this
        // attaches synthetic content provenance.
        let yaml = YamlWithSourceInfo::new_scalar(Yaml::String("test string".to_string()), si())
            .with_content_provenance(si());
        let mut diagnostics = crate::utils::diagnostic_collector::DiagnosticCollector::new();
        let result =
            yaml_to_config_value(yaml, InterpretationContext::ProjectConfig, &mut diagnostics);
        // In project config context, strings stay as literals
        assert!(matches!(
            result.value,
            ConfigValueKind::Scalar {
                yaml: Yaml::String(_),
                ..
            }
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

    // ─────────────────────────────────────────────────────────────
    // Lone-figure unwrap in config strings
    // (bd-page-footer-image-items-stmpikgo).
    //
    // Config strings are inline presentation contexts (footer/navbar
    // item text, titles, captions); figure-with-caption semantics is
    // never wanted there. The qmd reader's postprocess desugars a
    // single-image paragraph into a Figure, which — without the
    // unwrap — leaves the value as PandocBlocks([Figure]) that inline
    // renderers and rewriters drop on the floor.
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn config_string_lone_image_unwraps_to_inline_image() {
        let mut diagnostics = Vec::new();
        let kind = parse_config_string_as_markdown(
            "![lone image](images/logo.svg)",
            &si(),
            &mut diagnostics,
        );
        let ConfigValueKind::PandocInlines(inlines) = kind else {
            panic!("lone image must unwrap to PandocInlines, got {:?}", kind);
        };
        assert_eq!(inlines.len(), 1, "exactly the image, got {:?}", inlines);
        let Inline::Image(img) = &inlines[0] else {
            panic!("expected Image inline, got {:?}", inlines[0]);
        };
        assert_eq!(img.target.0, "images/logo.svg");
        assert!(
            !img.content.is_empty(),
            "alt text must survive the round-trip through the figure desugar"
        );
    }

    #[test]
    fn config_string_lone_image_without_alt_is_inline_image() {
        // No alt text → the postprocess figure desugar never fires
        // (it requires a non-empty caption); pin the behavior so both
        // variants land in the same shape.
        let mut diagnostics = Vec::new();
        let kind = parse_config_string_as_markdown("![](images/logo.svg)", &si(), &mut diagnostics);
        let ConfigValueKind::PandocInlines(inlines) = kind else {
            panic!("expected PandocInlines, got {:?}", kind);
        };
        assert!(
            matches!(&inlines[..], [Inline::Image(_)]),
            "got {:?}",
            inlines
        );
    }

    #[test]
    fn config_string_lone_image_unwrap_restores_attr() {
        // The figure desugar splits the attr: the id moves to the
        // figure, classes/attributes stay on the image. The unwrap
        // must reassemble the image the author wrote.
        let mut diagnostics = Vec::new();
        let kind = parse_config_string_as_markdown(
            "![x](logo.svg){#the-id .the-class}",
            &si(),
            &mut diagnostics,
        );
        let ConfigValueKind::PandocInlines(inlines) = kind else {
            panic!("expected PandocInlines, got {:?}", kind);
        };
        let Inline::Image(img) = &inlines[0] else {
            panic!("expected Image inline, got {:?}", inlines[0]);
        };
        assert_eq!(img.attr.0, "the-id", "id must move back from the figure");
        assert_eq!(img.attr.1, vec!["the-class".to_string()]);
    }

    #[test]
    fn config_string_image_with_sibling_inline_stays_inlines() {
        let mut diagnostics = Vec::new();
        let kind =
            parse_config_string_as_markdown("![x](logo.svg) beside text", &si(), &mut diagnostics);
        assert!(
            matches!(kind, ConfigValueKind::PandocInlines(_)),
            "got {:?}",
            kind
        );
    }

    /// Sub-spans (an image target's URL span) must reroot through the
    /// parent `SourceInfo` exactly like node spans do
    /// (bd-page-footer-image-items-stmpikgo, Phase 4): a consumer that
    /// anchors a diagnostic at `target_source.url` must land inside
    /// the config file the scalar was authored in, not at raw
    /// offsets-into-the-scalar against `FileId(0)`.
    #[test]
    fn config_string_image_target_source_reroots_through_parent() {
        use quarto_source_map::FileId;
        let parent = quarto_source_map::SourceInfo::original(FileId(7), 100, 160);
        let mut diagnostics = Vec::new();
        let kind =
            parse_config_string_as_markdown("![x](images/logo.svg)", &parent, &mut diagnostics);
        let ConfigValueKind::PandocInlines(inlines) = kind else {
            panic!("expected PandocInlines");
        };
        let Inline::Image(img) = &inlines[0] else {
            panic!("expected Image inline");
        };
        let url_si = img
            .target_source
            .url
            .as_ref()
            .expect("URL span must be tracked");
        let (fid, start, end) = url_si
            .resolve_byte_range()
            .expect("URL span must resolve to a byte range");
        assert_eq!(fid, 7, "URL span must resolve into the parent's file");
        // "![x](" is 5 bytes into the scalar, which starts at parent
        // offset 100.
        assert_eq!(start, 105, "URL span must shift by the parent's start");
        assert_eq!(end, 105 + "images/logo.svg".len());
    }

    #[test]
    fn explicit_md_lone_image_keeps_figure_semantics() {
        // `!md`-tagged values are explicit block-context markdown:
        // a lone image there keeps its Figure (decision 3 of the
        // 2026-08-18 plan — figures persist in block settings).
        let mut collector = crate::utils::diagnostic_collector::DiagnosticCollector::new();
        let kind = parse_yaml_string_as_markdown_to_config(
            "![lone image](images/logo.svg)",
            &si(),
            true,
            &mut collector,
        );
        let ConfigValueKind::PandocBlocks(blocks) = &kind else {
            panic!("expected PandocBlocks, got {:?}", kind);
        };
        assert!(
            matches!(&blocks[..], [quarto_pandoc_types::Block::Figure(_)]),
            "got {:?}",
            blocks
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
            matches!(&contents.value, ConfigValueKind::Scalar { yaml: Yaml::String(s), .. } if s == "*.qmd"),
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
            ConfigValueKind::Scalar {
                yaml: Yaml::Boolean(true),
                ..
            }
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
            ConfigValueKind::Scalar {
                yaml: Yaml::Integer(42),
                ..
            }
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
            ConfigValueKind::Scalar {
                yaml: Yaml::Real(_),
                ..
            }
        ));
    }

    #[test]
    fn test_yaml_to_config_value_null() {
        let yaml = YamlWithSourceInfo::new_scalar(Yaml::Null, si());
        let mut diagnostics = crate::utils::diagnostic_collector::DiagnosticCollector::new();
        let result =
            yaml_to_config_value(yaml, InterpretationContext::ProjectConfig, &mut diagnostics);
        assert!(matches!(
            result.value,
            ConfigValueKind::Scalar {
                yaml: Yaml::Null,
                ..
            }
        ));
    }

    #[test]
    fn test_yaml_to_config_value_bad_value() {
        let yaml = YamlWithSourceInfo::new_scalar(Yaml::BadValue, si());
        let mut diagnostics = crate::utils::diagnostic_collector::DiagnosticCollector::new();
        let result =
            yaml_to_config_value(yaml, InterpretationContext::ProjectConfig, &mut diagnostics);
        // BadValue becomes Null
        assert!(matches!(
            result.value,
            ConfigValueKind::Scalar {
                yaml: Yaml::Null,
                ..
            }
        ));
    }

    #[test]
    fn test_yaml_to_config_value_alias() {
        // Aliases should be resolved by yaml-rust2, but if they somehow appear,
        // we treat them as Null
        let yaml = YamlWithSourceInfo::new_scalar(Yaml::Alias(1), si());
        let mut diagnostics = crate::utils::diagnostic_collector::DiagnosticCollector::new();
        let result =
            yaml_to_config_value(yaml, InterpretationContext::ProjectConfig, &mut diagnostics);
        assert!(matches!(
            result.value,
            ConfigValueKind::Scalar {
                yaml: Yaml::Null,
                ..
            }
        ));
    }

    #[test]
    fn test_yaml_to_config_value_with_str_tag() {
        // .with_content_provenance(si()): see the document-metadata test
        // above for why hand-built fixtures attach a synthetic span.
        let yaml = YamlWithSourceInfo::new_scalar_with_tag(
            Yaml::String("plain text".to_string()),
            si(),
            Some(("str".to_string(), si())),
        )
        .with_content_provenance(si());
        let mut diagnostics = crate::utils::diagnostic_collector::DiagnosticCollector::new();
        let result = yaml_to_config_value(
            yaml,
            InterpretationContext::DocumentMetadata,
            &mut diagnostics,
        );
        // !str tag keeps string literal even in document metadata context
        assert!(matches!(
            result.value,
            ConfigValueKind::Scalar {
                yaml: Yaml::String(_),
                ..
            }
        ));
    }

    #[test]
    fn test_yaml_to_config_value_with_path_tag() {
        let yaml = YamlWithSourceInfo::new_scalar_with_tag(
            Yaml::String("/path/to/file".to_string()),
            si(),
            Some(("path".to_string(), si())),
        )
        .with_content_provenance(si());
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
        )
        .with_content_provenance(si());
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
        )
        .with_content_provenance(si());
        let mut diagnostics = crate::utils::diagnostic_collector::DiagnosticCollector::new();
        let result =
            yaml_to_config_value(yaml, InterpretationContext::ProjectConfig, &mut diagnostics);
        assert!(matches!(result.value, ConfigValueKind::Expr(_)));
    }

    // ─────────────────────────────────────────────────────────────
    // Forwarding the underlying parser diagnostic on config-string
    // parse failure (bd-q120-masks-config-md-diagnostic-a039r80t).
    //
    // A config value that fails to parse as markdown must not reduce
    // to the generic Q-1-20 "could not parse" — the underlying parser
    // diagnostic (e.g. Q-2-3, kv-pair before class specifier) is
    // folded into the Q-1-20 message as located details, with spans
    // rerooted into the file the scalar was authored in.
    // ─────────────────────────────────────────────────────────────

    /// Markdown that fails q2's attribute grammar with Q-2-3
    /// (key-value pair before class specifier) — accepted by
    /// Pandoc/Q1, so common in ported configs.
    const KV_BEFORE_CLASS: &str = r#"![logo](images/logo.svg){width="65px" .light-content}"#;

    #[test]
    fn config_string_parse_failure_forwards_underlying_diagnostic() {
        use quarto_source_map::FileId;
        let parent =
            quarto_source_map::SourceInfo::original(FileId(7), 100, 100 + KV_BEFORE_CLASS.len());
        let mut diagnostics = Vec::new();
        let _ = parse_config_string_as_markdown(KV_BEFORE_CLASS, &parent, &mut diagnostics);

        assert_eq!(
            diagnostics.len(),
            1,
            "children fold into the single Q-1-20, got {:#?}",
            diagnostics
        );
        let diag = &diagnostics[0];
        assert_eq!(diag.code.as_deref(), Some("Q-1-20"));
        assert!(
            matches!(diag.kind, quarto_error_reporting::DiagnosticKind::Warning),
            "untagged branch stays a warning"
        );

        let detail_texts: Vec<&str> = diag.details.iter().map(|d| d.content.as_str()).collect();
        assert!(
            detail_texts
                .iter()
                .any(|t| t.contains("Key-value Pair Before Class Specifier")),
            "the child diagnostic's title must be folded in, got {:?}",
            detail_texts
        );
        assert!(
            detail_texts
                .iter()
                .any(|t| t.contains("cannot appear before the class specifier")),
            "the child's located notes must be folded in, got {:?}",
            detail_texts
        );

        let mut located = 0;
        for det in &diag.details {
            if let Some(loc) = &det.location {
                let (fid, start, end) = loc
                    .resolve_byte_range()
                    .expect("forwarded detail spans must resolve");
                assert_eq!(fid, 7, "detail spans must reroot into the parent's file");
                assert!(
                    start >= 100 && end <= 100 + KV_BEFORE_CLASS.len(),
                    "detail span {}..{} must land inside the scalar's range in the parent",
                    start,
                    end
                );
                located += 1;
            }
        }
        assert!(
            located >= 2,
            "the child's two-part span must survive forwarding, got {:#?}",
            diag.details
        );
    }

    #[test]
    fn explicit_md_parse_failure_forwards_underlying_diagnostic() {
        use quarto_source_map::FileId;
        let parent =
            quarto_source_map::SourceInfo::original(FileId(7), 100, 100 + KV_BEFORE_CLASS.len());
        let mut collector = crate::utils::diagnostic_collector::DiagnosticCollector::new();
        let _ =
            parse_yaml_string_as_markdown_to_config(KV_BEFORE_CLASS, &parent, true, &mut collector);
        let diagnostics = collector.into_diagnostics();

        assert_eq!(diagnostics.len(), 1, "got {:#?}", diagnostics);
        let diag = &diagnostics[0];
        assert_eq!(diag.code.as_deref(), Some("Q-1-20"));
        assert!(
            matches!(diag.kind, quarto_error_reporting::DiagnosticKind::Error),
            "!md branch stays an error"
        );
        assert!(
            diag.details.iter().any(|d| d
                .content
                .as_str()
                .contains("Key-value Pair Before Class Specifier")),
            "the child diagnostic must be folded in, got {:#?}",
            diag.details
        );
        for det in &diag.details {
            if let Some(loc) = &det.location {
                let (fid, _, _) = loc.resolve_byte_range().expect("must resolve");
                assert_eq!(fid, 7, "detail spans must reroot into the parent's file");
            }
        }
    }

    // C6: the desync/no-derivation warning (bd-yaml-provenance).
    //
    // `YamlWithSourceInfo::new_scalar` yields `content_source_info: None`
    // by construction (no derivation ran), so this hand-built node is
    // exactly the injection seam the warning exists to report on. This
    // pair is what makes the warning revertible: reverting the call to
    // `content_provenance_desync_warning` in `yaml_to_config_value_at`
    // turns the positive test red, and widening the rule beyond
    // `Yaml::String` would turn the negative test red.

    #[test]
    fn test_yaml_to_config_value_string_without_content_provenance_warns() {
        let yaml = YamlWithSourceInfo::new_scalar(Yaml::String("no provenance".to_string()), si());
        let mut diagnostics = crate::utils::diagnostic_collector::DiagnosticCollector::new();
        let result =
            yaml_to_config_value(yaml, InterpretationContext::ProjectConfig, &mut diagnostics);

        let diags = diagnostics.diagnostics();
        assert_eq!(
            diags.len(),
            1,
            "expected exactly one warning, got {diags:?}"
        );
        assert_eq!(
            diags[0].kind,
            quarto_error_reporting::DiagnosticKind::Warning,
            "must be a warning, not an error"
        );
        assert!(
            diags[0].code.is_none(),
            "no Q- code: this is an internal consistency signal, not user-actionable"
        );
        assert!(
            !diagnostics.has_errors(),
            "non-fatal: a walker bug must not turn a working render into a hard failure"
        );
        // The returned ConfigValue is still usable — non-fatal means the
        // conversion completes, not that it's discarded.
        assert!(matches!(
            result.value,
            ConfigValueKind::Scalar {
                yaml: Yaml::String(_),
                ..
            }
        ));
    }

    #[test]
    fn test_yaml_to_config_value_non_string_scalar_without_content_provenance_does_not_warn() {
        // The rule is scoped to `Yaml::String`: a non-string scalar's
        // `None` is correct (production never derives content provenance
        // for a non-string scalar either — see
        // `scalar_string_with_content_provenance`'s doc comment), so it
        // must not warn. Without this test, a later widening of the rule
        // beyond `Yaml::String` would go unnoticed.
        let yaml = YamlWithSourceInfo::new_scalar(Yaml::Integer(7), si());
        let mut diagnostics = crate::utils::diagnostic_collector::DiagnosticCollector::new();
        let result =
            yaml_to_config_value(yaml, InterpretationContext::ProjectConfig, &mut diagnostics);

        assert!(
            diagnostics.diagnostics().is_empty(),
            "non-string scalar's None must not warn, got {:?}",
            diagnostics.diagnostics()
        );
        assert!(matches!(
            result.value,
            ConfigValueKind::Scalar {
                yaml: Yaml::Integer(7),
                ..
            }
        ));
    }

    // ─────────────────────────────────────────────────────────────
    // C4a: content provenance in the re-parse base (bd-yaml-provenance).
    //
    // A block scalar's decoded content is not the same string as its
    // raw source text (decoding strips per-line indentation), so
    // pairing the decoded text with the raw span and doing offset
    // arithmetic into it drifts. This is a text-path assertion, not
    // just a JSON-column assertion (see the CLI tests in
    // `crates/quarto/tests/integration/json_errors.rs` for those): it
    // proves the diagnostic's resolved span underlines the *exact
    // source bytes* of each HTML tag, using
    // `quarto_config::span_assert::assert_diagnostic_underlines`
    // (task C3's piecewise `resolve_span`, which resolves the `Concat`
    // of per-line pieces a multi-line block scalar's content
    // provenance produces).
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn q_2_9_underlines_exact_html_tag_bytes_in_a_block_scalar() {
        const FIXTURE_FILE: &str = "fixture.yml";
        // Same shape as the canonical multi-line block-scalar fixture in
        // task-C4a-brief.md / the CLI test, reduced to just the scalar:
        // this test exercises `yaml_to_config_value` directly rather
        // than a full render.
        let yaml_text = "center: |\n  line one\n  line two\n  <span id=\"y\">Footer</span>\n";
        let parsed = quarto_yaml::parse_file(yaml_text, FIXTURE_FILE).expect("valid yaml");
        let mut diagnostics = crate::utils::diagnostic_collector::DiagnosticCollector::new();
        let _ = yaml_to_config_value(
            parsed,
            InterpretationContext::DocumentMetadata,
            &mut diagnostics,
        );

        let diags = diagnostics.diagnostics();
        let q29: Vec<_> = diags
            .iter()
            .filter(|d| d.code.as_deref() == Some("Q-2-9"))
            .collect();
        assert_eq!(
            q29.len(),
            2,
            "expected two Q-2-9 warnings (open + close <span> tags); got {diags:?}"
        );

        let ctx = quarto_config::span_assert::context_for(FIXTURE_FILE, yaml_text);
        quarto_config::span_assert::assert_diagnostic_underlines(q29[0], &ctx, "<span id=\"y\">");
        quarto_config::span_assert::assert_diagnostic_underlines(q29[1], &ctx, "</span>");
    }
}
