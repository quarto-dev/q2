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
use indexmap::IndexMap;
use quarto_config::{ConfigValue, ConfigValueKind, MergeOp, MergedConfig};
use quarto_doctemplate::{TemplateContext, TemplateValue};
use quarto_error_reporting::DiagnosticMessage;
use quarto_pandoc_types::meta::MetaValueWithSourceInfo;
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
            interpretation: None,
        },
        MetaValueWithSourceInfo::MetaBool { value, source_info } => ConfigValue {
            value: ConfigValueKind::Scalar(Yaml::Boolean(*value)),
            source_info: source_info.clone(),
            merge_op: MergeOp::Concat,
            interpretation: None,
        },
        MetaValueWithSourceInfo::MetaInlines {
            content,
            source_info,
        } => ConfigValue {
            value: ConfigValueKind::PandocInlines(content.clone()),
            source_info: source_info.clone(),
            merge_op: MergeOp::Prefer, // Inlines default to prefer (last wins)
            interpretation: None,
        },
        MetaValueWithSourceInfo::MetaBlocks {
            content,
            source_info,
        } => ConfigValue {
            value: ConfigValueKind::PandocBlocks(content.clone()),
            source_info: source_info.clone(),
            merge_op: MergeOp::Prefer, // Blocks default to prefer (last wins)
            interpretation: None,
        },
        MetaValueWithSourceInfo::MetaList { items, source_info } => {
            let config_items: Vec<ConfigValue> =
                items.iter().map(meta_to_config_value).collect();
            ConfigValue {
                value: ConfigValueKind::Array(config_items),
                source_info: source_info.clone(),
                merge_op: MergeOp::Concat,
                interpretation: None,
            }
        }
        MetaValueWithSourceInfo::MetaMap { entries, source_info } => {
            let config_entries: IndexMap<String, ConfigValue> = entries
                .iter()
                .map(|entry| (entry.key.clone(), meta_to_config_value(&entry.value)))
                .collect();
            ConfigValue {
                value: ConfigValueKind::Map(config_entries),
                source_info: source_info.clone(),
                merge_op: MergeOp::Concat,
                interpretation: None,
            }
        }
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
                .map(|(key, value)| (key.clone(), config_to_template_value(value, ctx)))
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
                .filter_map(|(k, v)| k.as_str().map(|key| (key.to_string(), yaml_to_template_value(v))))
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
        for (key, value) in entries {
            let template_value = config_to_template_value(value, &mut conv_ctx);
            ctx.insert(key.clone(), template_value);
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
    let mut defaults: IndexMap<String, ConfigValue> = IndexMap::new();

    // Default language
    defaults.insert(
        "lang".to_string(),
        ConfigValue::new_scalar(Yaml::String("en".to_string()), SourceInfo::default()),
    );

    // Derive pagetitle from title
    if let Some(pagetitle) = derive_pagetitle(meta) {
        defaults.insert(
            "pagetitle".to_string(),
            ConfigValue::new_scalar(Yaml::String(pagetitle), SourceInfo::default()),
        );
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

        assert!(matches!(config.value, ConfigValueKind::Scalar(Yaml::String(ref s)) if s == "hello"));
    }

    #[test]
    fn test_meta_bool_to_config_value() {
        let meta = MetaValueWithSourceInfo::MetaBool {
            value: true,
            source_info: dummy_source_info(),
        };
        let config = meta_to_config_value(&meta);

        assert!(matches!(config.value, ConfigValueKind::Scalar(Yaml::Boolean(true))));
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

        if let ConfigValueKind::Map(entries) = &defaults.value {
            let lang = entries.get("lang").expect("lang should be present");
            assert!(matches!(&lang.value, ConfigValueKind::Scalar(Yaml::String(s)) if s == "en"));
        } else {
            panic!("Expected Map");
        }
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

        let (ctx, diags) = merged_metadata_to_context(&meta, "<p>Body</p>".to_string(), MetaWriter::Html);

        assert!(diags.is_empty(), "Expected no diagnostics: {:?}", diags);

        // Should have lang from defaults
        assert_eq!(ctx.get("lang"), Some(&TemplateValue::String("en".to_string())));

        // Should have pagetitle derived from title
        assert_eq!(ctx.get("pagetitle"), Some(&TemplateValue::String("Test Title".to_string())));

        // Should have title from document
        assert_eq!(ctx.get("title"), Some(&TemplateValue::String("Test Title".to_string())));

        // Should have body
        assert_eq!(ctx.get("body"), Some(&TemplateValue::String("<p>Body</p>".to_string())));
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
        assert_eq!(ctx.get("lang"), Some(&TemplateValue::String("de".to_string())));
    }
}
