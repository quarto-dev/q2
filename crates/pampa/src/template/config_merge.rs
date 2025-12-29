/*
 * template/config_merge.rs
 * Copyright (c) 2025 Posit, PBC
 */

//! Configuration merging for template rendering.
//!
//! This module provides the integration between the quarto-config merging system
//! and template rendering. It handles:
//!
//! 1. Converting document metadata (`MetaValueWithSourceInfo`) to `ConfigValue`
//! 2. Computing template defaults (like `lang` and `pagetitle`)
//! 3. Merging defaults with document metadata
//! 4. Converting the merged result to `TemplateContext`
//!
//! # Example
//!
//! ```ignore
//! // Convert document metadata to ConfigValue
//! let doc_meta = meta_to_config_value(&pandoc.meta);
//!
//! // Compute template defaults (lang, pagetitle derived from title)
//! let template_meta = compute_template_defaults(&pandoc.meta);
//!
//! // Merge: template_meta (defaults) <> doc_meta (document values override)
//! let merged = MergedConfig::new(vec![&template_meta, &doc_meta]);
//! let materialized = merged.materialize().unwrap();
//!
//! // Convert to template context
//! let template_ctx = config_to_template_context(&materialized, MetaWriter::Html);
//! ```

use crate::pandoc::block::Block;
use crate::pandoc::inline::Inlines;
use crate::template::context::MetaWriter;
use crate::writers::plaintext;
use hashlink::LinkedHashMap;
use quarto_config::{ConfigMapEntry, ConfigValue, ConfigValueKind, MergeOp, MergedConfig};
use quarto_doctemplate::{TemplateContext, TemplateValue};
use quarto_error_reporting::DiagnosticMessage;
use quarto_pandoc_types::inline::{Inline, Span, Str};
use quarto_pandoc_types::meta::MetaValueWithSourceInfo;
use quarto_pandoc_types::AttrSourceInfo;
use quarto_source_map::SourceInfo;
use std::collections::HashMap;
use yaml_rust2::Yaml;

// =============================================================================
// MetaValueWithSourceInfo -> ConfigValue
// =============================================================================

/// Convert a `MetaValueWithSourceInfo` to a `ConfigValue`.
///
/// This preserves source information and maps Pandoc metadata types
/// to the corresponding ConfigValue types.
pub fn meta_to_config_value(meta: &MetaValueWithSourceInfo) -> ConfigValue {
    match meta {
        MetaValueWithSourceInfo::MetaString { value, source_info } => ConfigValue {
            value: ConfigValueKind::Scalar(Yaml::String(value.clone())),
            source_info: source_info.clone(),
            merge_op: MergeOp::Concat,
        },
        MetaValueWithSourceInfo::MetaBool { value, source_info } => ConfigValue {
            value: ConfigValueKind::Scalar(Yaml::Boolean(*value)),
            source_info: source_info.clone(),
            merge_op: MergeOp::Concat,
        },
        MetaValueWithSourceInfo::MetaInlines {
            content,
            source_info,
        } => ConfigValue {
            value: ConfigValueKind::PandocInlines(content.clone()),
            source_info: source_info.clone(),
            merge_op: MergeOp::Prefer, // Inlines default to prefer (last wins)
        },
        MetaValueWithSourceInfo::MetaBlocks {
            content,
            source_info,
        } => ConfigValue {
            value: ConfigValueKind::PandocBlocks(content.clone()),
            source_info: source_info.clone(),
            merge_op: MergeOp::Prefer, // Blocks default to prefer (last wins)
        },
        MetaValueWithSourceInfo::MetaList { items, source_info } => {
            let config_items: Vec<ConfigValue> = items.iter().map(meta_to_config_value).collect();
            ConfigValue {
                value: ConfigValueKind::Array(config_items),
                source_info: source_info.clone(),
                merge_op: MergeOp::Concat,
            }
        }
        MetaValueWithSourceInfo::MetaMap {
            entries,
            source_info,
        } => {
            let config_entries: Vec<ConfigMapEntry> = entries
                .iter()
                .map(|entry| ConfigMapEntry {
                    key: entry.key.clone(),
                    key_source: entry.key_source.clone(),
                    value: meta_to_config_value(&entry.value),
                })
                .collect();
            ConfigValue {
                value: ConfigValueKind::Map(config_entries),
                source_info: source_info.clone(),
                merge_op: MergeOp::Concat,
            }
        }
    }
}

// =============================================================================
// ConfigValue -> MetaValueWithSourceInfo
// =============================================================================

/// Convert a `ConfigValue` back to `MetaValueWithSourceInfo`.
///
/// This is the inverse of `meta_to_config_value`, used during migration
/// to allow gradual adoption of ConfigValue while maintaining compatibility
/// with code that still expects MetaValueWithSourceInfo.
///
/// # Note on merge_op
///
/// The `merge_op` field in ConfigValue has no equivalent in MetaValueWithSourceInfo,
/// so it is discarded during this conversion. The merge semantics are only
/// relevant during config merging, not after materialization.
///
/// # Note on Path/Glob/Expr variants
///
/// These ConfigValueKind variants have no direct equivalent in MetaValueWithSourceInfo.
/// They are converted to MetaString, which loses the semantic information about
/// the intended interpretation. This is acceptable for backward compatibility
/// during migration, but code consuming these values should migrate to using
/// ConfigValue directly to preserve the full semantics.
pub fn config_value_to_meta(config: &ConfigValue) -> MetaValueWithSourceInfo {
    use quarto_pandoc_types::meta::MetaMapEntry;

    match &config.value {
        ConfigValueKind::Scalar(yaml) => match yaml {
            Yaml::String(s) => MetaValueWithSourceInfo::MetaString {
                value: s.clone(),
                source_info: config.source_info.clone(),
            },
            Yaml::Boolean(b) => MetaValueWithSourceInfo::MetaBool {
                value: *b,
                source_info: config.source_info.clone(),
            },
            Yaml::Integer(i) => MetaValueWithSourceInfo::MetaString {
                value: i.to_string(),
                source_info: config.source_info.clone(),
            },
            Yaml::Real(r) => MetaValueWithSourceInfo::MetaString {
                value: r.clone(),
                source_info: config.source_info.clone(),
            },
            Yaml::Null => MetaValueWithSourceInfo::MetaString {
                value: String::new(),
                source_info: config.source_info.clone(),
            },
            // Array and Hash in Yaml should not appear in Scalar variant
            _ => MetaValueWithSourceInfo::MetaString {
                value: String::new(),
                source_info: config.source_info.clone(),
            },
        },
        ConfigValueKind::PandocInlines(inlines) => MetaValueWithSourceInfo::MetaInlines {
            content: inlines.clone(),
            source_info: config.source_info.clone(),
        },
        ConfigValueKind::PandocBlocks(blocks) => MetaValueWithSourceInfo::MetaBlocks {
            content: blocks.clone(),
            source_info: config.source_info.clone(),
        },
        // Path: produces MetaInlines with plain Str (matches legacy !path behavior)
        ConfigValueKind::Path(s) => MetaValueWithSourceInfo::MetaInlines {
            content: vec![Inline::Str(Str {
                text: s.clone(),
                source_info: config.source_info.clone(),
            })],
            source_info: config.source_info.clone(),
        },
        // Glob, Expr: produces MetaInlines with Span wrapper (matches legacy !glob, !expr behavior)
        ConfigValueKind::Glob(s) => {
            let mut attributes = LinkedHashMap::new();
            attributes.insert("tag".to_string(), "glob".to_string());
            let span = Span {
                attr: (
                    String::new(),
                    vec!["yaml-tagged-string".to_string()],
                    attributes,
                ),
                content: vec![Inline::Str(Str {
                    text: s.clone(),
                    source_info: config.source_info.clone(),
                })],
                source_info: SourceInfo::default(),
                attr_source: AttrSourceInfo::empty(),
            };
            MetaValueWithSourceInfo::MetaInlines {
                content: vec![Inline::Span(span)],
                source_info: config.source_info.clone(),
            }
        }
        ConfigValueKind::Expr(s) => {
            let mut attributes = LinkedHashMap::new();
            attributes.insert("tag".to_string(), "expr".to_string());
            let span = Span {
                attr: (
                    String::new(),
                    vec!["yaml-tagged-string".to_string()],
                    attributes,
                ),
                content: vec![Inline::Str(Str {
                    text: s.clone(),
                    source_info: config.source_info.clone(),
                })],
                source_info: SourceInfo::default(),
                attr_source: AttrSourceInfo::empty(),
            };
            MetaValueWithSourceInfo::MetaInlines {
                content: vec![Inline::Span(span)],
                source_info: config.source_info.clone(),
            }
        }
        ConfigValueKind::Array(items) => MetaValueWithSourceInfo::MetaList {
            items: items.iter().map(config_value_to_meta).collect(),
            source_info: config.source_info.clone(),
        },
        ConfigValueKind::Map(entries) => MetaValueWithSourceInfo::MetaMap {
            entries: entries
                .iter()
                .map(|entry| MetaMapEntry {
                    key: entry.key.clone(),
                    key_source: entry.key_source.clone(),
                    value: config_value_to_meta(&entry.value),
                })
                .collect(),
            source_info: config.source_info.clone(),
        },
    }
}

// =============================================================================
// ConfigValue -> TemplateValue
// =============================================================================

/// Context for ConfigValue to TemplateValue conversion.
pub struct ConfigConversionContext {
    pub writer: MetaWriter,
    pub diagnostics: Vec<DiagnosticMessage>,
}

impl ConfigConversionContext {
    pub fn new(writer: MetaWriter) -> Self {
        Self {
            writer,
            diagnostics: Vec::new(),
        }
    }

    fn add_diagnostics(&mut self, diags: Vec<DiagnosticMessage>) {
        self.diagnostics.extend(diags);
    }
}

/// Convert a `ConfigValue` to a `TemplateValue`.
///
/// This handles rendering of PandocInlines/PandocBlocks using the specified writer.
pub fn config_to_template_value(
    config: &ConfigValue,
    ctx: &mut ConfigConversionContext,
) -> TemplateValue {
    match &config.value {
        ConfigValueKind::Scalar(yaml) => yaml_to_template_value(yaml),
        ConfigValueKind::Path(s) | ConfigValueKind::Glob(s) | ConfigValueKind::Expr(s) => {
            // Deferred interpretation variants - just return as string
            TemplateValue::String(s.clone())
        }
        ConfigValueKind::Array(items) => {
            let values: Vec<TemplateValue> = items
                .iter()
                .map(|item| config_to_template_value(item, ctx))
                .collect();
            TemplateValue::List(values)
        }
        ConfigValueKind::Map(entries) => {
            let map: HashMap<String, TemplateValue> = entries
                .iter()
                .map(|entry| {
                    (
                        entry.key.clone(),
                        config_to_template_value(&entry.value, ctx),
                    )
                })
                .collect();
            TemplateValue::Map(map)
        }
        ConfigValueKind::PandocInlines(inlines) => {
            let (rendered, diags) = ctx.writer.render_inlines(inlines);
            ctx.add_diagnostics(diags);
            TemplateValue::String(rendered)
        }
        ConfigValueKind::PandocBlocks(blocks) => {
            let (rendered, diags) = ctx.writer.render_blocks(blocks);
            ctx.add_diagnostics(diags);
            TemplateValue::String(rendered)
        }
    }
}

/// Convert a YAML value to a TemplateValue.
fn yaml_to_template_value(yaml: &Yaml) -> TemplateValue {
    match yaml {
        Yaml::String(s) => TemplateValue::String(s.clone()),
        Yaml::Boolean(b) => TemplateValue::Bool(*b),
        Yaml::Integer(i) => TemplateValue::String(i.to_string()),
        Yaml::Real(r) => TemplateValue::String(r.clone()),
        Yaml::Array(arr) => {
            let values: Vec<TemplateValue> = arr.iter().map(yaml_to_template_value).collect();
            TemplateValue::List(values)
        }
        Yaml::Hash(hash) => {
            let map: HashMap<String, TemplateValue> = hash
                .iter()
                .filter_map(|(k, v)| {
                    k.as_str()
                        .map(|key| (key.to_string(), yaml_to_template_value(v)))
                })
                .collect();
            TemplateValue::Map(map)
        }
        Yaml::Null => TemplateValue::String(String::new()),
        Yaml::BadValue => TemplateValue::String(String::new()),
        Yaml::Alias(_) => TemplateValue::String(String::new()),
    }
}

/// Convert a `ConfigValue` to a `TemplateContext`.
///
/// The config value should be a Map at the root level.
pub fn config_to_template_context(
    config: &ConfigValue,
    writer: MetaWriter,
) -> (TemplateContext, Vec<DiagnosticMessage>) {
    let mut ctx = TemplateContext::new();
    let mut conv_ctx = ConfigConversionContext::new(writer);

    if let ConfigValueKind::Map(entries) = &config.value {
        for entry in entries {
            let template_value = config_to_template_value(&entry.value, &mut conv_ctx);
            ctx.insert(entry.key.clone(), template_value);
        }
    }

    (ctx, conv_ctx.diagnostics)
}

// =============================================================================
// Template Defaults Computation
// =============================================================================

/// Compute template default values from document metadata.
///
/// This creates a `ConfigValue` map containing:
/// - `lang`: Default language (currently "en")
/// - `pagetitle`: Plain text version of the title (for HTML `<title>` tag)
///
/// These defaults are meant to be merged with document metadata using
/// `MergedConfig::new(vec![&defaults, &doc_meta])` so document values
/// can override them.
pub fn compute_template_defaults(meta: &MetaValueWithSourceInfo) -> ConfigValue {
    let mut defaults: Vec<ConfigMapEntry> = Vec::new();

    // Default language
    defaults.push(ConfigMapEntry {
        key: "lang".to_string(),
        key_source: SourceInfo::default(),
        value: ConfigValue::new_scalar(Yaml::String("en".to_string()), SourceInfo::default()),
    });

    // Derive pagetitle from title
    if let Some(pagetitle) = derive_pagetitle(meta) {
        defaults.push(ConfigMapEntry {
            key: "pagetitle".to_string(),
            key_source: SourceInfo::default(),
            value: ConfigValue::new_scalar(Yaml::String(pagetitle), SourceInfo::default()),
        });
    }

    ConfigValue::new_map(defaults, SourceInfo::default())
}

/// Derive a plain-text pagetitle from document metadata.
///
/// Pandoc derives `pagetitle` by rendering the `title` field to plain text.
/// This is used for the HTML `<title>` element, which cannot contain markup.
fn derive_pagetitle(meta: &MetaValueWithSourceInfo) -> Option<String> {
    // meta should be a MetaMap at the root
    if let MetaValueWithSourceInfo::MetaMap { entries, .. } = meta {
        // Find the "title" entry
        for entry in entries {
            if entry.key == "title" {
                return match &entry.value {
                    MetaValueWithSourceInfo::MetaString { value, .. } => Some(value.clone()),
                    MetaValueWithSourceInfo::MetaInlines { content, .. } => {
                        let (plain_text, _diags) = plaintext::inlines_to_string(content);
                        Some(plain_text)
                    }
                    _ => None,
                };
            }
        }
    }
    None
}

// =============================================================================
// High-Level Integration
// =============================================================================

/// Merge template defaults with document metadata and convert to TemplateContext.
///
/// This is the main entry point for template metadata preparation. It:
/// 1. Converts document metadata to ConfigValue
/// 2. Computes template defaults (lang, pagetitle)
/// 3. Merges defaults with document metadata (doc values override defaults)
/// 4. Materializes the merged result
/// 5. Converts to TemplateContext
///
/// # Arguments
///
/// * `meta` - The document metadata
/// * `body` - The pre-rendered body content
/// * `writer` - The writer to use for rendering inlines/blocks
///
/// # Returns
///
/// A tuple of (context, diagnostics).
pub fn merged_metadata_to_context(
    meta: &MetaValueWithSourceInfo,
    body: String,
    writer: MetaWriter,
) -> (TemplateContext, Vec<DiagnosticMessage>) {
    let mut all_diagnostics = Vec::new();

    // Step 1: Convert document metadata to ConfigValue
    let doc_meta = meta_to_config_value(meta);

    // Step 2: Compute template defaults
    let template_meta = compute_template_defaults(meta);

    // Step 3: Merge (template_meta has lower priority, doc_meta has higher priority)
    let merged = MergedConfig::new(vec![&template_meta, &doc_meta]);

    // Step 4: Materialize
    let materialized = match merged.materialize() {
        Ok(config) => config,
        Err(e) => {
            all_diagnostics.push(
                quarto_error_reporting::DiagnosticMessageBuilder::error("Config merge failed")
                    .with_code("Q-1-30")
                    .problem(format!("Failed to materialize merged config: {:?}", e))
                    .build(),
            );
            // Fall back to just document metadata
            doc_meta.clone()
        }
    };

    // Step 5: Convert to TemplateContext
    let (mut template_ctx, conv_diags) = config_to_template_context(&materialized, writer);
    all_diagnostics.extend(conv_diags);

    // Add the body variable
    template_ctx.insert("body", TemplateValue::String(body));

    (template_ctx, all_diagnostics)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pandoc::inline::{Emph, Inline, Space, Str};
    use quarto_pandoc_types::meta::MetaMapEntry;

    fn dummy_source_info() -> SourceInfo {
        SourceInfo::default()
    }

    fn make_str(text: &str) -> Inline {
        Inline::Str(Str {
            text: text.to_string(),
            source_info: dummy_source_info(),
        })
    }

    fn make_space() -> Inline {
        Inline::Space(Space {
            source_info: dummy_source_info(),
        })
    }

    #[test]
    fn test_meta_string_to_config_value() {
        let meta = MetaValueWithSourceInfo::MetaString {
            value: "hello".to_string(),
            source_info: dummy_source_info(),
        };
        let config = meta_to_config_value(&meta);

        assert!(
            matches!(config.value, ConfigValueKind::Scalar(Yaml::String(ref s)) if s == "hello")
        );
    }

    #[test]
    fn test_meta_bool_to_config_value() {
        let meta = MetaValueWithSourceInfo::MetaBool {
            value: true,
            source_info: dummy_source_info(),
        };
        let config = meta_to_config_value(&meta);

        assert!(matches!(
            config.value,
            ConfigValueKind::Scalar(Yaml::Boolean(true))
        ));
    }

    #[test]
    fn test_meta_inlines_to_config_value() {
        let inlines = vec![make_str("hello"), make_space(), make_str("world")];
        let meta = MetaValueWithSourceInfo::MetaInlines {
            content: inlines,
            source_info: dummy_source_info(),
        };
        let config = meta_to_config_value(&meta);

        assert!(matches!(config.value, ConfigValueKind::PandocInlines(_)));
        assert_eq!(config.merge_op, MergeOp::Prefer);
    }

    #[test]
    fn test_config_to_template_value_scalar() {
        let config = ConfigValue::new_scalar(Yaml::String("test".to_string()), dummy_source_info());
        let mut ctx = ConfigConversionContext::new(MetaWriter::Html);
        let result = config_to_template_value(&config, &mut ctx);

        assert_eq!(result, TemplateValue::String("test".to_string()));
    }

    #[test]
    fn test_derive_pagetitle_from_string() {
        let meta = MetaValueWithSourceInfo::MetaMap {
            entries: vec![MetaMapEntry {
                key: "title".to_string(),
                key_source: dummy_source_info(),
                value: MetaValueWithSourceInfo::MetaString {
                    value: "My Title".to_string(),
                    source_info: dummy_source_info(),
                },
            }],
            source_info: dummy_source_info(),
        };

        let pagetitle = derive_pagetitle(&meta);
        assert_eq!(pagetitle, Some("My Title".to_string()));
    }

    #[test]
    fn test_derive_pagetitle_from_inlines() {
        // Title with emphasis: "Hello _world_" -> plain text "Hello world"
        let inlines = vec![
            make_str("Hello"),
            make_space(),
            Inline::Emph(Emph {
                content: vec![make_str("world")],
                source_info: dummy_source_info(),
            }),
        ];

        let meta = MetaValueWithSourceInfo::MetaMap {
            entries: vec![MetaMapEntry {
                key: "title".to_string(),
                key_source: dummy_source_info(),
                value: MetaValueWithSourceInfo::MetaInlines {
                    content: inlines,
                    source_info: dummy_source_info(),
                },
            }],
            source_info: dummy_source_info(),
        };

        let pagetitle = derive_pagetitle(&meta);
        assert_eq!(pagetitle, Some("Hello world".to_string()));
    }

    #[test]
    fn test_compute_template_defaults_has_lang() {
        let meta = MetaValueWithSourceInfo::MetaMap {
            entries: vec![],
            source_info: dummy_source_info(),
        };

        let defaults = compute_template_defaults(&meta);

        // Use ConfigValue's get() method instead of entries.get()
        let lang = defaults.get("lang").expect("lang should be present");
        assert!(matches!(&lang.value, ConfigValueKind::Scalar(Yaml::String(s)) if s == "en"));
    }

    #[test]
    fn test_merged_metadata_includes_defaults() {
        let meta = MetaValueWithSourceInfo::MetaMap {
            entries: vec![MetaMapEntry {
                key: "title".to_string(),
                key_source: dummy_source_info(),
                value: MetaValueWithSourceInfo::MetaString {
                    value: "Test Title".to_string(),
                    source_info: dummy_source_info(),
                },
            }],
            source_info: dummy_source_info(),
        };

        let (ctx, diags) =
            merged_metadata_to_context(&meta, "<p>Body</p>".to_string(), MetaWriter::Html);

        assert!(diags.is_empty(), "Expected no diagnostics: {:?}", diags);

        // Should have lang from defaults
        assert_eq!(
            ctx.get("lang"),
            Some(&TemplateValue::String("en".to_string()))
        );

        // Should have pagetitle derived from title
        assert_eq!(
            ctx.get("pagetitle"),
            Some(&TemplateValue::String("Test Title".to_string()))
        );

        // Should have title from document
        assert_eq!(
            ctx.get("title"),
            Some(&TemplateValue::String("Test Title".to_string()))
        );

        // Should have body
        assert_eq!(
            ctx.get("body"),
            Some(&TemplateValue::String("<p>Body</p>".to_string()))
        );
    }

    #[test]
    fn test_document_values_override_defaults() {
        // Document explicitly sets lang
        let meta = MetaValueWithSourceInfo::MetaMap {
            entries: vec![
                MetaMapEntry {
                    key: "title".to_string(),
                    key_source: dummy_source_info(),
                    value: MetaValueWithSourceInfo::MetaString {
                        value: "Test".to_string(),
                        source_info: dummy_source_info(),
                    },
                },
                MetaMapEntry {
                    key: "lang".to_string(),
                    key_source: dummy_source_info(),
                    value: MetaValueWithSourceInfo::MetaString {
                        value: "de".to_string(),
                        source_info: dummy_source_info(),
                    },
                },
            ],
            source_info: dummy_source_info(),
        };

        let (ctx, _diags) = merged_metadata_to_context(&meta, "".to_string(), MetaWriter::Html);

        // Document's lang should override default
        assert_eq!(
            ctx.get("lang"),
            Some(&TemplateValue::String("de".to_string()))
        );
    }

    // =========================================================================
    // Bidirectional conversion tests (ConfigValue <-> MetaValueWithSourceInfo)
    // =========================================================================

    #[test]
    fn test_config_string_to_meta() {
        let config =
            ConfigValue::new_scalar(Yaml::String("hello".to_string()), dummy_source_info());
        let meta = config_value_to_meta(&config);

        assert!(matches!(
            meta,
            MetaValueWithSourceInfo::MetaString { ref value, .. } if value == "hello"
        ));
    }

    #[test]
    fn test_config_bool_to_meta() {
        let config = ConfigValue::new_scalar(Yaml::Boolean(true), dummy_source_info());
        let meta = config_value_to_meta(&config);

        assert!(matches!(
            meta,
            MetaValueWithSourceInfo::MetaBool { value: true, .. }
        ));
    }

    #[test]
    fn test_config_inlines_to_meta() {
        let inlines = vec![make_str("hello")];
        let config = ConfigValue::new_inlines(inlines.clone(), dummy_source_info());
        let meta = config_value_to_meta(&config);

        match meta {
            MetaValueWithSourceInfo::MetaInlines { content, .. } => {
                assert_eq!(content.len(), 1);
            }
            _ => panic!("Expected MetaInlines"),
        }
    }

    #[test]
    fn test_config_array_to_meta() {
        let items = vec![
            ConfigValue::new_scalar(Yaml::String("a".to_string()), dummy_source_info()),
            ConfigValue::new_scalar(Yaml::String("b".to_string()), dummy_source_info()),
        ];
        let config = ConfigValue::new_array(items, dummy_source_info());
        let meta = config_value_to_meta(&config);

        match meta {
            MetaValueWithSourceInfo::MetaList { items, .. } => {
                assert_eq!(items.len(), 2);
            }
            _ => panic!("Expected MetaList"),
        }
    }

    #[test]
    fn test_config_map_to_meta() {
        let entries = vec![quarto_config::ConfigMapEntry {
            key: "key".to_string(),
            key_source: dummy_source_info(),
            value: ConfigValue::new_scalar(Yaml::String("value".to_string()), dummy_source_info()),
        }];
        let config = ConfigValue::new_map(entries, dummy_source_info());
        let meta = config_value_to_meta(&config);

        match meta {
            MetaValueWithSourceInfo::MetaMap { entries, .. } => {
                assert_eq!(entries.len(), 1);
                assert_eq!(entries[0].key, "key");
            }
            _ => panic!("Expected MetaMap"),
        }
    }

    #[test]
    fn test_config_path_to_meta_inlines() {
        // Path variant should convert to MetaInlines with plain Str (matches legacy behavior)
        let config = ConfigValue::new_path("./data.csv".to_string(), dummy_source_info());
        let meta = config_value_to_meta(&config);

        match meta {
            MetaValueWithSourceInfo::MetaInlines { content, .. } => {
                assert_eq!(content.len(), 1);
                if let Inline::Str(s) = &content[0] {
                    assert_eq!(s.text, "./data.csv");
                } else {
                    panic!("Expected Str inline");
                }
            }
            _ => panic!("Expected MetaInlines for Path"),
        }
    }

    #[test]
    fn test_roundtrip_meta_to_config_to_meta() {
        // Test that meta -> config -> meta preserves the essential structure
        let original_meta = MetaValueWithSourceInfo::MetaMap {
            entries: vec![
                MetaMapEntry {
                    key: "title".to_string(),
                    key_source: dummy_source_info(),
                    value: MetaValueWithSourceInfo::MetaString {
                        value: "Hello".to_string(),
                        source_info: dummy_source_info(),
                    },
                },
                MetaMapEntry {
                    key: "enabled".to_string(),
                    key_source: dummy_source_info(),
                    value: MetaValueWithSourceInfo::MetaBool {
                        value: true,
                        source_info: dummy_source_info(),
                    },
                },
            ],
            source_info: dummy_source_info(),
        };

        // Convert to config and back
        let config = meta_to_config_value(&original_meta);
        let roundtrip_meta = config_value_to_meta(&config);

        // Check structure is preserved
        match &roundtrip_meta {
            MetaValueWithSourceInfo::MetaMap { entries, .. } => {
                assert_eq!(entries.len(), 2);

                // Check title
                let title = entries.iter().find(|e| e.key == "title").unwrap();
                assert!(matches!(
                    &title.value,
                    MetaValueWithSourceInfo::MetaString { value, .. } if value == "Hello"
                ));

                // Check enabled
                let enabled = entries.iter().find(|e| e.key == "enabled").unwrap();
                assert!(matches!(
                    &enabled.value,
                    MetaValueWithSourceInfo::MetaBool { value: true, .. }
                ));
            }
            _ => panic!("Expected MetaMap after roundtrip"),
        }
    }
}
