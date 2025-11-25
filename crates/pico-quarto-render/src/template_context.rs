/*
 * template_context.rs
 * Copyright (c) 2025 Posit, PBC
 */

//! Template context building and metadata preparation.
//!
//! This module provides functions for:
//! - Preparing document metadata for template rendering (`prepare_template_metadata`)
//! - Converting Pandoc metadata to template values (`meta_to_template_value`)
//! - The main rendering function (`render_with_template`)

use std::collections::HashMap;
use std::path::Path;

use anyhow::Result;
use quarto_doctemplate::{Template, TemplateContext, TemplateValue};
use quarto_markdown_pandoc::pandoc::Pandoc;
use quarto_markdown_pandoc::pandoc::meta::{MetaMapEntry, MetaValueWithSourceInfo};

use crate::format_writers::FormatWriters;

/// Prepare document metadata for template rendering.
///
/// This mutates the document to add derived metadata fields:
/// - `pagetitle`: Plain-text version of `title` (for HTML `<title>` element)
///
/// More fields can be added in the future (author-meta, date-meta, etc.)
pub fn prepare_template_metadata(pandoc: &mut Pandoc) {
    // Only mutate if meta is a MetaMap
    let MetaValueWithSourceInfo::MetaMap {
        entries,
        source_info,
    } = &mut pandoc.meta
    else {
        return;
    };

    // Check if pagetitle already exists
    let has_pagetitle = entries.iter().any(|e| e.key == "pagetitle");
    if has_pagetitle {
        return;
    }

    // Look for title field
    let title_entry = entries.iter().find(|e| e.key == "title");
    if let Some(entry) = title_entry {
        let plain_text = match &entry.value {
            MetaValueWithSourceInfo::MetaString { value, .. } => value.clone(),
            MetaValueWithSourceInfo::MetaInlines { content, .. } => {
                let (text, _diagnostics) =
                    quarto_markdown_pandoc::writers::plaintext::inlines_to_string(content);
                text
            }
            MetaValueWithSourceInfo::MetaBlocks { content, .. } => {
                let (text, _diagnostics) =
                    quarto_markdown_pandoc::writers::plaintext::blocks_to_string(content);
                text
            }
            _ => return, // Other types: skip
        };

        // Add pagetitle entry
        entries.push(MetaMapEntry {
            key: "pagetitle".to_string(),
            key_source: source_info.clone(),
            value: MetaValueWithSourceInfo::MetaString {
                value: plain_text,
                source_info: source_info.clone(),
            },
        });
    }
}

/// Convert document metadata to template values.
///
/// This recursively converts the metadata structure:
/// - MetaString → TemplateValue::String (literal, no rendering)
/// - MetaBool → TemplateValue::Bool
/// - MetaInlines → TemplateValue::String (rendered via format writers)
/// - MetaBlocks → TemplateValue::String (rendered via format writers)
/// - MetaList → TemplateValue::List (recursive)
/// - MetaMap → TemplateValue::Map (recursive)
pub fn meta_to_template_value<W: FormatWriters>(
    meta: &MetaValueWithSourceInfo,
    writers: &W,
) -> Result<TemplateValue> {
    Ok(match meta {
        MetaValueWithSourceInfo::MetaString { value, .. } => {
            // MetaString is already a plain string - use as literal
            TemplateValue::String(value.clone())
        }
        MetaValueWithSourceInfo::MetaBool { value, .. } => TemplateValue::Bool(*value),
        MetaValueWithSourceInfo::MetaInlines { content, .. } => {
            // Render inlines using format-specific writer
            TemplateValue::String(writers.write_inlines(content)?)
        }
        MetaValueWithSourceInfo::MetaBlocks { content, .. } => {
            // Render blocks using format-specific writer
            TemplateValue::String(writers.write_blocks(content)?)
        }
        MetaValueWithSourceInfo::MetaList { items, .. } => {
            let values: Result<Vec<_>> = items
                .iter()
                .map(|item| meta_to_template_value(item, writers))
                .collect();
            TemplateValue::List(values?)
        }
        MetaValueWithSourceInfo::MetaMap { entries, .. } => {
            let mut map = HashMap::new();
            for entry in entries {
                map.insert(
                    entry.key.clone(),
                    meta_to_template_value(&entry.value, writers)?,
                );
            }
            TemplateValue::Map(map)
        }
    })
}

/// Render a document using a template.
///
/// # Arguments
/// - `pandoc` - The document (should have been through prepare_template_metadata)
/// - `template` - A compiled template with partials resolved
/// - `writers` - Format-specific writers for metadata conversion
///
/// # Returns
/// The rendered document as a string, or an error.
pub fn render_with_template<W: FormatWriters>(
    pandoc: &Pandoc,
    template: &Template,
    writers: &W,
) -> Result<String> {
    // 1. Convert metadata to TemplateValue::Map
    let meta_value = meta_to_template_value(&pandoc.meta, writers)?;

    // 2. Build TemplateContext from metadata
    let mut context = TemplateContext::new();
    if let TemplateValue::Map(map) = meta_value {
        for (key, value) in map {
            context.insert(key, value);
        }
    }

    // 3. Render document body and add to context
    let body = writers.write_blocks(&pandoc.blocks)?;
    context.insert("body", TemplateValue::String(body));

    // 4. Evaluate template
    let output = template
        .render(&context)
        .map_err(|e| anyhow::anyhow!("Template error: {:?}", e))?;

    Ok(output)
}

/// Compile a template from the embedded resources.
///
/// # Arguments
/// - `template_source` - The main template source
/// - `resolver` - Partial resolver for loading includes
///
/// # Returns
/// The compiled template, or an error.
pub fn compile_template<R: quarto_doctemplate::PartialResolver>(
    template_source: &str,
    resolver: &R,
) -> Result<Template> {
    Template::compile_with_resolver(template_source, Path::new("template.html"), resolver, 0)
        .map_err(|e| anyhow::anyhow!("Template compilation error: {:?}", e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_markdown_pandoc::pandoc::Inline;
    use quarto_markdown_pandoc::pandoc::inline::Str;

    fn dummy_source_info() -> quarto_source_map::SourceInfo {
        quarto_source_map::SourceInfo::from_range(
            quarto_source_map::FileId(0),
            quarto_source_map::Range {
                start: quarto_source_map::Location {
                    offset: 0,
                    row: 0,
                    column: 0,
                },
                end: quarto_source_map::Location {
                    offset: 0,
                    row: 0,
                    column: 0,
                },
            },
        )
    }

    #[test]
    fn test_prepare_template_metadata_adds_pagetitle() {
        let mut pandoc = Pandoc {
            meta: MetaValueWithSourceInfo::MetaMap {
                entries: vec![MetaMapEntry {
                    key: "title".to_string(),
                    key_source: dummy_source_info(),
                    value: MetaValueWithSourceInfo::MetaInlines {
                        content: vec![Inline::Str(Str {
                            text: "My Document".to_string(),
                            source_info: dummy_source_info(),
                        })],
                        source_info: dummy_source_info(),
                    },
                }],
                source_info: dummy_source_info(),
            },
            blocks: vec![],
        };

        prepare_template_metadata(&mut pandoc);

        // Check that pagetitle was added
        if let MetaValueWithSourceInfo::MetaMap { entries, .. } = &pandoc.meta {
            let pagetitle = entries.iter().find(|e| e.key == "pagetitle");
            assert!(pagetitle.is_some());
            if let Some(entry) = pagetitle {
                if let MetaValueWithSourceInfo::MetaString { value, .. } = &entry.value {
                    assert_eq!(value, "My Document");
                } else {
                    panic!("Expected MetaString for pagetitle");
                }
            }
        } else {
            panic!("Expected MetaMap");
        }
    }

    #[test]
    fn test_prepare_template_metadata_preserves_existing_pagetitle() {
        let mut pandoc = Pandoc {
            meta: MetaValueWithSourceInfo::MetaMap {
                entries: vec![
                    MetaMapEntry {
                        key: "title".to_string(),
                        key_source: dummy_source_info(),
                        value: MetaValueWithSourceInfo::MetaInlines {
                            content: vec![Inline::Str(Str {
                                text: "My Document".to_string(),
                                source_info: dummy_source_info(),
                            })],
                            source_info: dummy_source_info(),
                        },
                    },
                    MetaMapEntry {
                        key: "pagetitle".to_string(),
                        key_source: dummy_source_info(),
                        value: MetaValueWithSourceInfo::MetaString {
                            value: "Custom Page Title".to_string(),
                            source_info: dummy_source_info(),
                        },
                    },
                ],
                source_info: dummy_source_info(),
            },
            blocks: vec![],
        };

        prepare_template_metadata(&mut pandoc);

        // Check that pagetitle was NOT overwritten
        if let MetaValueWithSourceInfo::MetaMap { entries, .. } = &pandoc.meta {
            let pagetitle_entries: Vec<_> =
                entries.iter().filter(|e| e.key == "pagetitle").collect();
            assert_eq!(pagetitle_entries.len(), 1);
            if let MetaValueWithSourceInfo::MetaString { value, .. } = &pagetitle_entries[0].value {
                assert_eq!(value, "Custom Page Title");
            }
        }
    }

    #[test]
    fn test_meta_to_template_value_string() {
        use crate::format_writers::HtmlWriters;
        let writers = HtmlWriters;

        let meta = MetaValueWithSourceInfo::MetaString {
            value: "hello".to_string(),
            source_info: dummy_source_info(),
        };

        let result = meta_to_template_value(&meta, &writers).unwrap();
        assert_eq!(result, TemplateValue::String("hello".to_string()));
    }

    #[test]
    fn test_meta_to_template_value_bool() {
        use crate::format_writers::HtmlWriters;
        let writers = HtmlWriters;

        let meta = MetaValueWithSourceInfo::MetaBool {
            value: true,
            source_info: dummy_source_info(),
        };

        let result = meta_to_template_value(&meta, &writers).unwrap();
        assert_eq!(result, TemplateValue::Bool(true));
    }

    #[test]
    fn test_meta_to_template_value_inlines() {
        use crate::format_writers::HtmlWriters;
        let writers = HtmlWriters;

        let meta = MetaValueWithSourceInfo::MetaInlines {
            content: vec![Inline::Str(Str {
                text: "hello".to_string(),
                source_info: dummy_source_info(),
            })],
            source_info: dummy_source_info(),
        };

        let result = meta_to_template_value(&meta, &writers).unwrap();
        // HTML writer outputs plain text for Str
        assert_eq!(result, TemplateValue::String("hello".to_string()));
    }

    #[test]
    fn test_meta_to_template_value_map() {
        use crate::format_writers::HtmlWriters;
        let writers = HtmlWriters;

        let meta = MetaValueWithSourceInfo::MetaMap {
            entries: vec![
                MetaMapEntry {
                    key: "key1".to_string(),
                    key_source: dummy_source_info(),
                    value: MetaValueWithSourceInfo::MetaString {
                        value: "value1".to_string(),
                        source_info: dummy_source_info(),
                    },
                },
                MetaMapEntry {
                    key: "key2".to_string(),
                    key_source: dummy_source_info(),
                    value: MetaValueWithSourceInfo::MetaBool {
                        value: false,
                        source_info: dummy_source_info(),
                    },
                },
            ],
            source_info: dummy_source_info(),
        };

        let result = meta_to_template_value(&meta, &writers).unwrap();
        if let TemplateValue::Map(map) = result {
            assert_eq!(
                map.get("key1"),
                Some(&TemplateValue::String("value1".to_string()))
            );
            assert_eq!(map.get("key2"), Some(&TemplateValue::Bool(false)));
        } else {
            panic!("Expected TemplateValue::Map");
        }
    }
}
