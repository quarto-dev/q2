/*
 * template.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Template integration for HTML rendering.
 */

//! Template integration for document rendering.
//!
//! This module provides the integration layer between the quarto-doctemplate
//! engine and the Quarto render pipeline. It handles:
//!
//! - Default HTML template for standalone documents
//! - Conversion of Pandoc metadata to template values
//! - Rendering documents through the template engine
//!
//! ## Architecture
//!
//! The template system uses dependency injection: the rendered body content
//! is passed as a template variable, allowing the template to control the
//! overall document structure while the HTML writer controls content rendering.

use quarto_doctemplate::{Template, TemplateContext, TemplateValue};
use quarto_pandoc_types::meta::MetaValueWithSourceInfo;

use crate::Result;

/// Default HTML5 template for standalone documents.
///
/// This template is compatible with Pandoc's variable conventions:
/// - `$pagetitle$` / `$title$` - document title
/// - `$body$` - rendered body content
/// - `$css$` - CSS stylesheets (external files)
/// - `$header-includes$` - additional header content
const DEFAULT_HTML_TEMPLATE: &str = r#"<!DOCTYPE html>
<html$if(lang)$ lang="$lang$"$endif$>
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
$if(pagetitle)$
<title>$pagetitle$</title>
$endif$
$for(css)$
<link rel="stylesheet" href="$css$">
$endfor$
$if(header-includes)$
$header-includes$
$endif$
</head>
<body>
$body$
</body>
</html>
"#;

/// Compile the default HTML template.
pub fn default_html_template() -> Result<Template> {
    Template::compile(DEFAULT_HTML_TEMPLATE)
        .map_err(|e| crate::error::QuartoError::other(e.to_string()))
}

/// Render a document to HTML using the template engine.
///
/// # Arguments
/// * `body` - The rendered body content (HTML)
/// * `meta` - Document metadata from the Pandoc AST
///
/// # Returns
/// The complete HTML document as a string.
pub fn render_with_template(body: &str, meta: &MetaValueWithSourceInfo) -> Result<String> {
    let template = default_html_template()?;

    // Build template context from metadata
    let mut ctx = TemplateContext::new();

    // Add body content
    ctx.insert("body", TemplateValue::String(body.to_string()));

    // Convert and add metadata
    add_metadata_to_context(meta, &mut ctx);

    // Render the template
    template
        .render(&ctx)
        .map_err(|e| crate::error::QuartoError::other(e.to_string()))
}

/// Render a document using a custom template.
///
/// # Arguments
/// * `template` - A compiled template
/// * `body` - The rendered body content (HTML)
/// * `meta` - Document metadata from the Pandoc AST
///
/// # Returns
/// The complete HTML document as a string.
pub fn render_with_custom_template(
    template: &Template,
    body: &str,
    meta: &MetaValueWithSourceInfo,
) -> Result<String> {
    let mut ctx = TemplateContext::new();
    ctx.insert("body", TemplateValue::String(body.to_string()));
    add_metadata_to_context(meta, &mut ctx);

    template
        .render(&ctx)
        .map_err(|e| crate::error::QuartoError::other(e.to_string()))
}

/// Render a document with external resources.
///
/// This function renders the document with the default template and adds
/// CSS/JS resource paths to the template context. Resource paths from
/// the `css_paths` parameter are combined with any CSS paths from metadata.
///
/// # Arguments
/// * `body` - The rendered body content (HTML)
/// * `meta` - Document metadata from the Pandoc AST
/// * `css_paths` - Paths to CSS files (relative to output HTML)
///
/// # Returns
/// The complete HTML document as a string.
pub fn render_with_resources(
    body: &str,
    meta: &MetaValueWithSourceInfo,
    css_paths: &[String],
) -> Result<String> {
    let template = default_html_template()?;

    let mut ctx = TemplateContext::new();
    ctx.insert("body", TemplateValue::String(body.to_string()));

    // Add metadata, but we'll handle css specially
    add_metadata_to_context_except(meta, &mut ctx, &["css"]);

    // Build combined CSS list: default resources first, then user-specified
    let mut css_list: Vec<TemplateValue> = css_paths
        .iter()
        .map(|p| TemplateValue::String(p.clone()))
        .collect();

    // Add any user-specified CSS from metadata
    if let Some(user_css) = extract_css_from_meta(meta) {
        css_list.extend(user_css);
    }

    ctx.insert("css", TemplateValue::List(css_list));

    template
        .render(&ctx)
        .map_err(|e| crate::error::QuartoError::other(e.to_string()))
}

/// Add metadata from the Pandoc AST to the template context, excluding specific keys.
fn add_metadata_to_context_except(
    meta: &MetaValueWithSourceInfo,
    ctx: &mut TemplateContext,
    exclude: &[&str],
) {
    if let MetaValueWithSourceInfo::MetaMap { entries, .. } = meta {
        for entry in entries {
            if !exclude.contains(&entry.key.as_str()) {
                let value = meta_value_to_template_value(&entry.value);
                ctx.insert(&entry.key, value);
            }
        }
    }
}

/// Extract CSS paths from document metadata.
fn extract_css_from_meta(meta: &MetaValueWithSourceInfo) -> Option<Vec<TemplateValue>> {
    if let MetaValueWithSourceInfo::MetaMap { entries, .. } = meta {
        for entry in entries {
            if entry.key == "css" {
                return Some(match &entry.value {
                    MetaValueWithSourceInfo::MetaString { value, .. } => {
                        vec![TemplateValue::String(value.clone())]
                    }
                    MetaValueWithSourceInfo::MetaInlines { content, .. } => {
                        // YAML values like `css: custom.css` are often parsed as inlines
                        let text = inlines_to_text(content);
                        vec![TemplateValue::String(text)]
                    }
                    MetaValueWithSourceInfo::MetaList { items, .. } => {
                        items.iter().map(meta_value_to_template_value).collect()
                    }
                    _ => Vec::new(),
                });
            }
        }
    }
    None
}

/// Add metadata from the Pandoc AST to the template context.
fn add_metadata_to_context(meta: &MetaValueWithSourceInfo, ctx: &mut TemplateContext) {
    if let MetaValueWithSourceInfo::MetaMap { entries, .. } = meta {
        for entry in entries {
            let value = meta_value_to_template_value(&entry.value);
            ctx.insert(&entry.key, value);
        }
    }
}

/// Convert a Pandoc MetaValue to a TemplateValue.
fn meta_value_to_template_value(meta: &MetaValueWithSourceInfo) -> TemplateValue {
    match meta {
        MetaValueWithSourceInfo::MetaString { value, .. } => TemplateValue::String(value.clone()),
        MetaValueWithSourceInfo::MetaBool { value, .. } => TemplateValue::Bool(*value),
        MetaValueWithSourceInfo::MetaInlines { content, .. } => {
            // Convert inlines to plain text for template use
            let text = inlines_to_text(content);
            TemplateValue::String(text)
        }
        MetaValueWithSourceInfo::MetaBlocks { content, .. } => {
            // Convert blocks to plain text for template use
            let text = blocks_to_text(content);
            TemplateValue::String(text)
        }
        MetaValueWithSourceInfo::MetaList { items, .. } => {
            let list_items: Vec<TemplateValue> =
                items.iter().map(meta_value_to_template_value).collect();
            TemplateValue::List(list_items)
        }
        MetaValueWithSourceInfo::MetaMap { entries, .. } => {
            let mut map = std::collections::HashMap::new();
            for entry in entries {
                let value = meta_value_to_template_value(&entry.value);
                map.insert(entry.key.clone(), value);
            }
            TemplateValue::Map(map)
        }
    }
}

/// Convert inlines to plain text.
fn inlines_to_text(inlines: &[quarto_pandoc_types::inline::Inline]) -> String {
    use quarto_pandoc_types::inline::Inline;

    let mut result = String::new();
    for inline in inlines {
        match inline {
            Inline::Str(s) => result.push_str(&s.text),
            Inline::Space(_) => result.push(' '),
            Inline::SoftBreak(_) => result.push(' '),
            Inline::LineBreak(_) => result.push('\n'),
            Inline::Emph(e) => result.push_str(&inlines_to_text(&e.content)),
            Inline::Strong(s) => result.push_str(&inlines_to_text(&s.content)),
            Inline::Underline(u) => result.push_str(&inlines_to_text(&u.content)),
            Inline::Strikeout(s) => result.push_str(&inlines_to_text(&s.content)),
            Inline::Superscript(s) => result.push_str(&inlines_to_text(&s.content)),
            Inline::Subscript(s) => result.push_str(&inlines_to_text(&s.content)),
            Inline::SmallCaps(s) => result.push_str(&inlines_to_text(&s.content)),
            Inline::Quoted(q) => {
                result.push('"');
                result.push_str(&inlines_to_text(&q.content));
                result.push('"');
            }
            Inline::Code(c) => result.push_str(&c.text),
            Inline::Math(m) => result.push_str(&m.text),
            Inline::Link(l) => result.push_str(&inlines_to_text(&l.content)),
            Inline::Image(i) => result.push_str(&inlines_to_text(&i.content)),
            Inline::Span(s) => result.push_str(&inlines_to_text(&s.content)),
            Inline::Cite(c) => result.push_str(&inlines_to_text(&c.content)),
            Inline::Note(n) => result.push_str(&blocks_to_text(&n.content)),
            _ => {}
        }
    }
    result
}

/// Convert blocks to plain text.
fn blocks_to_text(blocks: &[quarto_pandoc_types::block::Block]) -> String {
    use quarto_pandoc_types::block::Block;

    let mut result = String::new();
    for (i, block) in blocks.iter().enumerate() {
        if i > 0 {
            result.push('\n');
        }
        match block {
            Block::Plain(p) => result.push_str(&inlines_to_text(&p.content)),
            Block::Paragraph(p) => result.push_str(&inlines_to_text(&p.content)),
            Block::Header(h) => result.push_str(&inlines_to_text(&h.content)),
            Block::CodeBlock(c) => result.push_str(&c.text),
            Block::BlockQuote(b) => result.push_str(&blocks_to_text(&b.content)),
            Block::Div(d) => result.push_str(&blocks_to_text(&d.content)),
            Block::LineBlock(l) => {
                for line in &l.content {
                    result.push_str(&inlines_to_text(line));
                    result.push('\n');
                }
            }
            Block::OrderedList(o) => {
                for item in &o.content {
                    result.push_str(&blocks_to_text(item));
                    result.push('\n');
                }
            }
            Block::BulletList(b) => {
                for item in &b.content {
                    result.push_str(&blocks_to_text(item));
                    result.push('\n');
                }
            }
            _ => {}
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_pandoc_types::inline::Str;
    use quarto_pandoc_types::meta::MetaMapEntry;
    use quarto_source_map::{FileId, Location, Range, SourceInfo};

    fn dummy_source_info() -> SourceInfo {
        SourceInfo::from_range(
            FileId(0),
            Range {
                start: Location {
                    offset: 0,
                    row: 0,
                    column: 0,
                },
                end: Location {
                    offset: 0,
                    row: 0,
                    column: 0,
                },
            },
        )
    }

    #[test]
    fn test_default_template_compiles() {
        let result = default_html_template();
        assert!(result.is_ok());
    }

    #[test]
    fn test_render_simple_document() {
        let meta = MetaValueWithSourceInfo::MetaMap {
            entries: vec![MetaMapEntry {
                key: "pagetitle".to_string(),
                key_source: dummy_source_info(),
                value: MetaValueWithSourceInfo::MetaString {
                    value: "Test Document".to_string(),
                    source_info: dummy_source_info(),
                },
            }],
            source_info: dummy_source_info(),
        };

        let body = "<p>Hello, World!</p>";
        let result = render_with_template(body, &meta);

        assert!(result.is_ok());
        let html = result.unwrap();
        assert!(html.contains("<title>Test Document</title>"));
        assert!(html.contains("<p>Hello, World!</p>"));
        assert!(html.contains("<!DOCTYPE html>"));
    }

    #[test]
    fn test_render_with_css() {
        let meta = MetaValueWithSourceInfo::MetaMap {
            entries: vec![
                MetaMapEntry {
                    key: "pagetitle".to_string(),
                    key_source: dummy_source_info(),
                    value: MetaValueWithSourceInfo::MetaString {
                        value: "Test".to_string(),
                        source_info: dummy_source_info(),
                    },
                },
                MetaMapEntry {
                    key: "css".to_string(),
                    key_source: dummy_source_info(),
                    value: MetaValueWithSourceInfo::MetaList {
                        items: vec![
                            MetaValueWithSourceInfo::MetaString {
                                value: "style1.css".to_string(),
                                source_info: dummy_source_info(),
                            },
                            MetaValueWithSourceInfo::MetaString {
                                value: "style2.css".to_string(),
                                source_info: dummy_source_info(),
                            },
                        ],
                        source_info: dummy_source_info(),
                    },
                },
            ],
            source_info: dummy_source_info(),
        };

        let body = "<p>Content</p>";
        let result = render_with_template(body, &meta);

        assert!(result.is_ok());
        let html = result.unwrap();
        assert!(html.contains(r#"<link rel="stylesheet" href="style1.css">"#));
        assert!(html.contains(r#"<link rel="stylesheet" href="style2.css">"#));
    }

    #[test]
    fn test_meta_value_conversion_string() {
        let meta = MetaValueWithSourceInfo::MetaString {
            value: "test".to_string(),
            source_info: dummy_source_info(),
        };
        let value = meta_value_to_template_value(&meta);
        assert_eq!(value, TemplateValue::String("test".to_string()));
    }

    #[test]
    fn test_meta_value_conversion_bool() {
        let meta = MetaValueWithSourceInfo::MetaBool {
            value: true,
            source_info: dummy_source_info(),
        };
        let value = meta_value_to_template_value(&meta);
        assert_eq!(value, TemplateValue::Bool(true));
    }

    #[test]
    fn test_meta_value_conversion_inlines() {
        let meta = MetaValueWithSourceInfo::MetaInlines {
            content: vec![
                quarto_pandoc_types::inline::Inline::Str(Str {
                    text: "Hello".to_string(),
                    source_info: dummy_source_info(),
                }),
                quarto_pandoc_types::inline::Inline::Space(quarto_pandoc_types::inline::Space {
                    source_info: dummy_source_info(),
                }),
                quarto_pandoc_types::inline::Inline::Str(Str {
                    text: "World".to_string(),
                    source_info: dummy_source_info(),
                }),
            ],
            source_info: dummy_source_info(),
        };
        let value = meta_value_to_template_value(&meta);
        assert_eq!(value, TemplateValue::String("Hello World".to_string()));
    }

    #[test]
    fn test_meta_value_conversion_list() {
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
        let value = meta_value_to_template_value(&meta);
        match value {
            TemplateValue::List(items) => {
                assert_eq!(items.len(), 2);
                assert_eq!(items[0], TemplateValue::String("a".to_string()));
                assert_eq!(items[1], TemplateValue::String("b".to_string()));
            }
            _ => panic!("Expected List"),
        }
    }
}
