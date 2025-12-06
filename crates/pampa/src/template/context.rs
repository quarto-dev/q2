/*
 * template/context.rs
 * Copyright (c) 2025 Posit, PBC
 */

//! Conversion from Pandoc metadata to template context.
//!
//! This module handles the conversion from `MetaValueWithSourceInfo` to
//! `TemplateValue`, including rendering `MetaInlines` and `MetaBlocks`
//! using the appropriate writer.

use crate::pandoc::block::Block;
use crate::pandoc::inline::Inlines;
use crate::writers::{html, plaintext};
use quarto_doctemplate::{TemplateContext, TemplateValue};
use quarto_error_reporting::DiagnosticMessage;
use quarto_pandoc_types::meta::MetaValueWithSourceInfo;
use std::collections::HashMap;

/// Strategy for rendering metadata inlines and blocks to strings.
///
/// When converting `MetaInlines` or `MetaBlocks` to `TemplateValue::String`,
/// we need to render them using a writer. This enum specifies which writer to use.
#[derive(Debug, Clone, Copy, Default)]
pub enum MetaWriter {
    /// Render as HTML.
    #[default]
    Html,
    /// Render as plain text (no markup).
    Plaintext,
}

impl MetaWriter {
    /// Render inlines to a string using this writer.
    pub fn render_inlines(&self, inlines: &Inlines) -> (String, Vec<DiagnosticMessage>) {
        match self {
            MetaWriter::Html => {
                let mut buf = Vec::new();
                let result = html::write_inlines(inlines, &mut buf);
                let diagnostics = if let Err(e) = result {
                    vec![DiagnosticMessage::error(format!(
                        "Failed to render inlines as HTML: {}",
                        e
                    ))]
                } else {
                    vec![]
                };
                (String::from_utf8_lossy(&buf).into_owned(), diagnostics)
            }
            MetaWriter::Plaintext => plaintext::inlines_to_string(inlines),
        }
    }

    /// Render blocks to a string using this writer.
    pub fn render_blocks(&self, blocks: &[Block]) -> (String, Vec<DiagnosticMessage>) {
        match self {
            MetaWriter::Html => {
                let mut buf = Vec::new();
                let result = html::write_blocks(blocks, &mut buf);
                let diagnostics = if let Err(e) = result {
                    vec![DiagnosticMessage::error(format!(
                        "Failed to render blocks as HTML: {}",
                        e
                    ))]
                } else {
                    vec![]
                };
                (String::from_utf8_lossy(&buf).into_owned(), diagnostics)
            }
            MetaWriter::Plaintext => plaintext::blocks_to_string(blocks),
        }
    }
}

/// Context for metadata conversion, collecting diagnostics.
pub struct ConversionContext {
    pub writer: MetaWriter,
    pub diagnostics: Vec<DiagnosticMessage>,
}

impl ConversionContext {
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

/// Convert a `MetaValueWithSourceInfo` to a `TemplateValue`.
///
/// This recursively converts the metadata structure, rendering any
/// `MetaInlines` or `MetaBlocks` to strings using the specified writer.
pub fn meta_to_template_value(
    meta: &MetaValueWithSourceInfo,
    ctx: &mut ConversionContext,
) -> TemplateValue {
    match meta {
        MetaValueWithSourceInfo::MetaString { value, .. } => TemplateValue::String(value.clone()),
        MetaValueWithSourceInfo::MetaBool { value, .. } => TemplateValue::Bool(*value),
        MetaValueWithSourceInfo::MetaInlines { content, .. } => {
            let (rendered, diags) = ctx.writer.render_inlines(content);
            ctx.add_diagnostics(diags);
            TemplateValue::String(rendered)
        }
        MetaValueWithSourceInfo::MetaBlocks { content, .. } => {
            let (rendered, diags) = ctx.writer.render_blocks(content);
            ctx.add_diagnostics(diags);
            TemplateValue::String(rendered)
        }
        MetaValueWithSourceInfo::MetaList { items, .. } => {
            let values: Vec<TemplateValue> = items
                .iter()
                .map(|item| meta_to_template_value(item, ctx))
                .collect();
            TemplateValue::List(values)
        }
        MetaValueWithSourceInfo::MetaMap { entries, .. } => {
            let map: HashMap<String, TemplateValue> = entries
                .iter()
                .map(|entry| {
                    let value = meta_to_template_value(&entry.value, ctx);
                    (entry.key.clone(), value)
                })
                .collect();
            TemplateValue::Map(map)
        }
    }
}

/// Build a template context from a Pandoc document's metadata.
///
/// This converts all metadata fields to template values and adds them
/// to a new `TemplateContext`. The `body` variable is NOT set by this
/// function - it should be added separately after rendering the document body.
///
/// # Arguments
///
/// * `meta` - The document metadata (typically `pandoc.meta`)
/// * `writer` - The writer to use for rendering `MetaInlines`/`MetaBlocks`
///
/// # Returns
///
/// A tuple of (context, diagnostics) where context contains all metadata
/// fields converted to template values.
pub fn metadata_to_context(
    meta: &MetaValueWithSourceInfo,
    writer: MetaWriter,
) -> (TemplateContext, Vec<DiagnosticMessage>) {
    let mut ctx = TemplateContext::new();
    let mut conv_ctx = ConversionContext::new(writer);

    // The root metadata should be a map
    if let MetaValueWithSourceInfo::MetaMap { entries, .. } = meta {
        for entry in entries {
            let value = meta_to_template_value(&entry.value, &mut conv_ctx);
            ctx.insert(entry.key.clone(), value);
        }
    }

    (ctx, conv_ctx.diagnostics)
}

/// Build a complete template context from a Pandoc document.
///
/// This converts metadata and adds the rendered body content.
///
/// # Arguments
///
/// * `meta` - The document metadata
/// * `body` - The pre-rendered body content (as a string)
/// * `writer` - The writer to use for rendering `MetaInlines`/`MetaBlocks`
///
/// # Returns
///
/// A tuple of (context, diagnostics).
pub fn pandoc_to_context(
    meta: &MetaValueWithSourceInfo,
    body: String,
    writer: MetaWriter,
) -> (TemplateContext, Vec<DiagnosticMessage>) {
    let (mut ctx, diagnostics) = metadata_to_context(meta, writer);

    // Add the body variable
    ctx.insert("body", TemplateValue::String(body));

    (ctx, diagnostics)
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_pandoc_types::meta::MetaMapEntry;
    use quarto_source_map::SourceInfo;

    fn dummy_source_info() -> SourceInfo {
        SourceInfo::default()
    }

    #[test]
    fn test_meta_string_to_template_value() {
        let meta = MetaValueWithSourceInfo::MetaString {
            value: "hello".to_string(),
            source_info: dummy_source_info(),
        };
        let mut ctx = ConversionContext::new(MetaWriter::Html);
        let result = meta_to_template_value(&meta, &mut ctx);
        assert_eq!(result, TemplateValue::String("hello".to_string()));
        assert!(ctx.diagnostics.is_empty());
    }

    #[test]
    fn test_meta_bool_to_template_value() {
        let meta = MetaValueWithSourceInfo::MetaBool {
            value: true,
            source_info: dummy_source_info(),
        };
        let mut ctx = ConversionContext::new(MetaWriter::Html);
        let result = meta_to_template_value(&meta, &mut ctx);
        assert_eq!(result, TemplateValue::Bool(true));
    }

    #[test]
    fn test_meta_list_to_template_value() {
        let meta = MetaValueWithSourceInfo::MetaList {
            items: vec![
                MetaValueWithSourceInfo::MetaString {
                    value: "a".to_string(),
                    source_info: dummy_source_info(),
                },
                MetaValueWithSourceInfo::MetaString {
                    value: "b".to_string(),
                    source_info: dummy_source_info(),
                },
            ],
            source_info: dummy_source_info(),
        };
        let mut ctx = ConversionContext::new(MetaWriter::Html);
        let result = meta_to_template_value(&meta, &mut ctx);
        assert_eq!(
            result,
            TemplateValue::List(vec![
                TemplateValue::String("a".to_string()),
                TemplateValue::String("b".to_string()),
            ])
        );
    }

    #[test]
    fn test_meta_map_to_template_value() {
        let meta = MetaValueWithSourceInfo::MetaMap {
            entries: vec![
                MetaMapEntry {
                    key: "title".to_string(),
                    key_source: dummy_source_info(),
                    value: MetaValueWithSourceInfo::MetaString {
                        value: "My Title".to_string(),
                        source_info: dummy_source_info(),
                    },
                },
                MetaMapEntry {
                    key: "draft".to_string(),
                    key_source: dummy_source_info(),
                    value: MetaValueWithSourceInfo::MetaBool {
                        value: false,
                        source_info: dummy_source_info(),
                    },
                },
            ],
            source_info: dummy_source_info(),
        };
        let mut ctx = ConversionContext::new(MetaWriter::Html);
        let result = meta_to_template_value(&meta, &mut ctx);

        if let TemplateValue::Map(map) = result {
            assert_eq!(
                map.get("title"),
                Some(&TemplateValue::String("My Title".to_string()))
            );
            assert_eq!(map.get("draft"), Some(&TemplateValue::Bool(false)));
        } else {
            panic!("Expected Map");
        }
    }

    #[test]
    fn test_metadata_to_context() {
        let meta = MetaValueWithSourceInfo::MetaMap {
            entries: vec![MetaMapEntry {
                key: "title".to_string(),
                key_source: dummy_source_info(),
                value: MetaValueWithSourceInfo::MetaString {
                    value: "Test".to_string(),
                    source_info: dummy_source_info(),
                },
            }],
            source_info: dummy_source_info(),
        };

        let (ctx, diags) = metadata_to_context(&meta, MetaWriter::Html);
        assert!(diags.is_empty());
        assert_eq!(
            ctx.get("title"),
            Some(&TemplateValue::String("Test".to_string()))
        );
    }

    #[test]
    fn test_pandoc_to_context() {
        let meta = MetaValueWithSourceInfo::MetaMap {
            entries: vec![MetaMapEntry {
                key: "title".to_string(),
                key_source: dummy_source_info(),
                value: MetaValueWithSourceInfo::MetaString {
                    value: "Test".to_string(),
                    source_info: dummy_source_info(),
                },
            }],
            source_info: dummy_source_info(),
        };

        let (ctx, diags) = pandoc_to_context(&meta, "<p>Hello</p>".to_string(), MetaWriter::Html);
        assert!(diags.is_empty());
        assert_eq!(
            ctx.get("title"),
            Some(&TemplateValue::String("Test".to_string()))
        );
        assert_eq!(
            ctx.get("body"),
            Some(&TemplateValue::String("<p>Hello</p>".to_string()))
        );
    }
}
