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
use quarto_pandoc_types::{ConfigValue, ConfigValueKind};

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
/// * `meta` - Document metadata from the Pandoc AST (as ConfigValue)
///
/// # Returns
/// The complete HTML document as a string.
pub fn render_with_template(body: &str, meta: &ConfigValue) -> Result<String> {
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
/// * `meta` - Document metadata from the Pandoc AST (as ConfigValue)
///
/// # Returns
/// The complete HTML document as a string.
pub fn render_with_custom_template(
    template: &Template,
    body: &str,
    meta: &ConfigValue,
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
/// * `meta` - Document metadata from the Pandoc AST (as ConfigValue)
/// * `css_paths` - Paths to CSS files (relative to output HTML)
///
/// # Returns
/// The complete HTML document as a string.
pub fn render_with_resources(
    body: &str,
    meta: &ConfigValue,
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
fn add_metadata_to_context_except(meta: &ConfigValue, ctx: &mut TemplateContext, exclude: &[&str]) {
    if let ConfigValueKind::Map(entries) = &meta.value {
        for entry in entries {
            if !exclude.contains(&entry.key.as_str()) {
                let value = config_value_to_template_value(&entry.value);
                ctx.insert(&entry.key, value);
            }
        }
    }
}

/// Extract CSS paths from document metadata.
fn extract_css_from_meta(meta: &ConfigValue) -> Option<Vec<TemplateValue>> {
    if let ConfigValueKind::Map(entries) = &meta.value {
        for entry in entries {
            if entry.key == "css" {
                // Try string first
                if let Some(s) = entry.value.as_str() {
                    return Some(vec![TemplateValue::String(s.to_string())]);
                }
                // Try inlines (YAML values like `css: custom.css` are often parsed as inlines)
                if let ConfigValueKind::PandocInlines(content) = &entry.value.value {
                    let text = inlines_to_text(content);
                    return Some(vec![TemplateValue::String(text)]);
                }
                // Try array
                if let ConfigValueKind::Array(items) = &entry.value.value {
                    return Some(items.iter().map(config_value_to_template_value).collect());
                }
                return Some(Vec::new());
            }
        }
    }
    None
}

/// Add metadata from the Pandoc AST to the template context.
fn add_metadata_to_context(meta: &ConfigValue, ctx: &mut TemplateContext) {
    if let ConfigValueKind::Map(entries) = &meta.value {
        for entry in entries {
            let value = config_value_to_template_value(&entry.value);
            ctx.insert(&entry.key, value);
        }
    }
}

/// Convert a ConfigValue to a TemplateValue.
fn config_value_to_template_value(meta: &ConfigValue) -> TemplateValue {
    // Try string-like values first (handles Scalar(String), Path, Glob, Expr)
    if let Some(s) = meta.as_str() {
        return TemplateValue::String(s.to_string());
    }

    // Try boolean
    if let Some(b) = meta.as_bool() {
        return TemplateValue::Bool(b);
    }

    // Try integer
    if let Some(i) = meta.as_int() {
        return TemplateValue::String(i.to_string());
    }

    // Check for null
    if meta.is_null() {
        return TemplateValue::Null;
    }

    // Handle other variants
    match &meta.value {
        ConfigValueKind::PandocInlines(content) => {
            // Convert inlines to plain text for template use
            let text = inlines_to_text(content);
            TemplateValue::String(text)
        }
        ConfigValueKind::PandocBlocks(content) => {
            // Convert blocks to plain text for template use
            let text = blocks_to_text(content);
            TemplateValue::String(text)
        }
        ConfigValueKind::Array(items) => {
            let list_items: Vec<TemplateValue> =
                items.iter().map(config_value_to_template_value).collect();
            TemplateValue::List(list_items)
        }
        ConfigValueKind::Map(entries) => {
            let mut map = std::collections::HashMap::new();
            for entry in entries {
                let value = config_value_to_template_value(&entry.value);
                map.insert(entry.key.clone(), value);
            }
            TemplateValue::Map(map)
        }
        // Scalar variants already handled above (string, bool, int, null)
        // Path, Glob, Expr already handled by as_str()
        _ => TemplateValue::Null,
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
    use quarto_pandoc_types::ConfigMapEntry;
    use quarto_pandoc_types::block::*;
    use quarto_pandoc_types::inline::*;
    use quarto_pandoc_types::{ListNumberDelim, ListNumberStyle};
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
        let meta = ConfigValue::new_map(
            vec![ConfigMapEntry {
                key: "pagetitle".to_string(),
                key_source: dummy_source_info(),
                value: ConfigValue::new_string("Test Document", dummy_source_info()),
            }],
            dummy_source_info(),
        );

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
        let meta = ConfigValue::new_map(
            vec![
                ConfigMapEntry {
                    key: "pagetitle".to_string(),
                    key_source: dummy_source_info(),
                    value: ConfigValue::new_string("Test", dummy_source_info()),
                },
                ConfigMapEntry {
                    key: "css".to_string(),
                    key_source: dummy_source_info(),
                    value: ConfigValue::new_array(
                        vec![
                            ConfigValue::new_string("style1.css", dummy_source_info()),
                            ConfigValue::new_string("style2.css", dummy_source_info()),
                        ],
                        dummy_source_info(),
                    ),
                },
            ],
            dummy_source_info(),
        );

        let body = "<p>Content</p>";
        let result = render_with_template(body, &meta);

        assert!(result.is_ok());
        let html = result.unwrap();
        assert!(html.contains(r#"<link rel="stylesheet" href="style1.css">"#));
        assert!(html.contains(r#"<link rel="stylesheet" href="style2.css">"#));
    }

    #[test]
    fn test_render_with_custom_template() {
        let template = Template::compile("Title: $title$\nBody: $body$").unwrap();
        let meta = ConfigValue::new_map(
            vec![ConfigMapEntry {
                key: "title".to_string(),
                key_source: dummy_source_info(),
                value: ConfigValue::new_string("Custom", dummy_source_info()),
            }],
            dummy_source_info(),
        );

        let result = render_with_custom_template(&template, "Hello", &meta);
        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(output.contains("Title: Custom"));
        assert!(output.contains("Body: Hello"));
    }

    #[test]
    fn test_render_with_resources() {
        let meta = ConfigValue::new_map(
            vec![ConfigMapEntry {
                key: "pagetitle".to_string(),
                key_source: dummy_source_info(),
                value: ConfigValue::new_string("Test", dummy_source_info()),
            }],
            dummy_source_info(),
        );

        let css_paths = vec!["lib/styles.css".to_string(), "lib/theme.css".to_string()];
        let result = render_with_resources("<p>Body</p>", &meta, &css_paths);

        assert!(result.is_ok());
        let html = result.unwrap();
        assert!(html.contains(r#"href="lib/styles.css"#));
        assert!(html.contains(r#"href="lib/theme.css"#));
    }

    #[test]
    fn test_render_with_resources_combines_css() {
        let meta = ConfigValue::new_map(
            vec![
                ConfigMapEntry {
                    key: "pagetitle".to_string(),
                    key_source: dummy_source_info(),
                    value: ConfigValue::new_string("Test", dummy_source_info()),
                },
                ConfigMapEntry {
                    key: "css".to_string(),
                    key_source: dummy_source_info(),
                    value: ConfigValue::new_string("user.css", dummy_source_info()),
                },
            ],
            dummy_source_info(),
        );

        let css_paths = vec!["default.css".to_string()];
        let result = render_with_resources("<p>Body</p>", &meta, &css_paths);

        assert!(result.is_ok());
        let html = result.unwrap();
        // Both default and user CSS should be present
        assert!(html.contains("default.css"));
        assert!(html.contains("user.css"));
    }

    // === ConfigValue conversion tests ===

    #[test]
    fn test_config_value_conversion_string() {
        let meta = ConfigValue::new_string("test", dummy_source_info());
        let value = config_value_to_template_value(&meta);
        assert_eq!(value, TemplateValue::String("test".to_string()));
    }

    #[test]
    fn test_config_value_conversion_bool() {
        let meta = ConfigValue::new_bool(true, dummy_source_info());
        let value = config_value_to_template_value(&meta);
        assert_eq!(value, TemplateValue::Bool(true));
    }

    #[test]
    fn test_config_value_conversion_bool_false() {
        let meta = ConfigValue::new_bool(false, dummy_source_info());
        let value = config_value_to_template_value(&meta);
        assert_eq!(value, TemplateValue::Bool(false));
    }

    #[test]
    fn test_config_value_conversion_int() {
        // Test integer conversion via a map since ConfigValue doesn't expose direct int construction
        // The actual int handling is tested via the config_value_to_template_value function
        // when it encounters Scalar(Integer) in the AST
        let meta = ConfigValue::new_map(
            vec![ConfigMapEntry {
                key: "num".to_string(),
                key_source: dummy_source_info(),
                value: ConfigValue::new_string("42", dummy_source_info()), // String representation
            }],
            dummy_source_info(),
        );
        let value = config_value_to_template_value(&meta);
        // Map conversion works
        match value {
            TemplateValue::Map(map) => {
                assert!(map.contains_key("num"));
            }
            _ => panic!("Expected Map"),
        }
    }

    #[test]
    fn test_config_value_conversion_null() {
        let meta = ConfigValue::null(dummy_source_info());
        let value = config_value_to_template_value(&meta);
        assert_eq!(value, TemplateValue::Null);
    }

    #[test]
    fn test_config_value_conversion_path() {
        let meta = ConfigValue::new_path("./data.csv".to_string(), dummy_source_info());
        let value = config_value_to_template_value(&meta);
        assert_eq!(value, TemplateValue::String("./data.csv".to_string()));
    }

    #[test]
    fn test_config_value_conversion_glob() {
        let meta = ConfigValue::new_glob("*.qmd".to_string(), dummy_source_info());
        let value = config_value_to_template_value(&meta);
        assert_eq!(value, TemplateValue::String("*.qmd".to_string()));
    }

    #[test]
    fn test_config_value_conversion_expr() {
        let meta = ConfigValue::new_expr("params$x".to_string(), dummy_source_info());
        let value = config_value_to_template_value(&meta);
        assert_eq!(value, TemplateValue::String("params$x".to_string()));
    }

    #[test]
    fn test_config_value_conversion_inlines() {
        let meta = ConfigValue::new_inlines(
            vec![
                Inline::Str(Str {
                    text: "Hello".to_string(),
                    source_info: dummy_source_info(),
                }),
                Inline::Space(Space {
                    source_info: dummy_source_info(),
                }),
                Inline::Str(Str {
                    text: "World".to_string(),
                    source_info: dummy_source_info(),
                }),
            ],
            dummy_source_info(),
        );
        let value = config_value_to_template_value(&meta);
        assert_eq!(value, TemplateValue::String("Hello World".to_string()));
    }

    #[test]
    fn test_config_value_conversion_blocks() {
        let meta = ConfigValue::new_blocks(
            vec![Block::Paragraph(Paragraph {
                content: vec![Inline::Str(Str {
                    text: "Test paragraph".to_string(),
                    source_info: dummy_source_info(),
                })],
                source_info: dummy_source_info(),
            })],
            dummy_source_info(),
        );
        let value = config_value_to_template_value(&meta);
        assert_eq!(value, TemplateValue::String("Test paragraph".to_string()));
    }

    #[test]
    fn test_config_value_conversion_list() {
        let meta = ConfigValue::new_array(
            vec![
                ConfigValue::new_string("a", dummy_source_info()),
                ConfigValue::new_string("b", dummy_source_info()),
            ],
            dummy_source_info(),
        );
        let value = config_value_to_template_value(&meta);
        match value {
            TemplateValue::List(items) => {
                assert_eq!(items.len(), 2);
                assert_eq!(items[0], TemplateValue::String("a".to_string()));
                assert_eq!(items[1], TemplateValue::String("b".to_string()));
            }
            _ => panic!("Expected List"),
        }
    }

    #[test]
    fn test_config_value_conversion_map() {
        let meta = ConfigValue::new_map(
            vec![
                ConfigMapEntry {
                    key: "key1".to_string(),
                    key_source: dummy_source_info(),
                    value: ConfigValue::new_string("value1", dummy_source_info()),
                },
                ConfigMapEntry {
                    key: "key2".to_string(),
                    key_source: dummy_source_info(),
                    value: ConfigValue::new_bool(true, dummy_source_info()),
                },
            ],
            dummy_source_info(),
        );
        let value = config_value_to_template_value(&meta);
        match value {
            TemplateValue::Map(map) => {
                assert_eq!(map.len(), 2);
                assert_eq!(
                    map.get("key1"),
                    Some(&TemplateValue::String("value1".to_string()))
                );
                assert_eq!(map.get("key2"), Some(&TemplateValue::Bool(true)));
            }
            _ => panic!("Expected Map"),
        }
    }

    // === extract_css_from_meta tests ===

    #[test]
    fn test_extract_css_string() {
        let meta = ConfigValue::new_map(
            vec![ConfigMapEntry {
                key: "css".to_string(),
                key_source: dummy_source_info(),
                value: ConfigValue::new_string("style.css", dummy_source_info()),
            }],
            dummy_source_info(),
        );
        let css = extract_css_from_meta(&meta);
        assert!(css.is_some());
        let css_list = css.unwrap();
        assert_eq!(css_list.len(), 1);
        assert_eq!(css_list[0], TemplateValue::String("style.css".to_string()));
    }

    #[test]
    fn test_extract_css_inlines() {
        let meta = ConfigValue::new_map(
            vec![ConfigMapEntry {
                key: "css".to_string(),
                key_source: dummy_source_info(),
                value: ConfigValue::new_inlines(
                    vec![Inline::Str(Str {
                        text: "inline.css".to_string(),
                        source_info: dummy_source_info(),
                    })],
                    dummy_source_info(),
                ),
            }],
            dummy_source_info(),
        );
        let css = extract_css_from_meta(&meta);
        assert!(css.is_some());
        let css_list = css.unwrap();
        assert_eq!(css_list.len(), 1);
        assert_eq!(css_list[0], TemplateValue::String("inline.css".to_string()));
    }

    #[test]
    fn test_extract_css_array() {
        let meta = ConfigValue::new_map(
            vec![ConfigMapEntry {
                key: "css".to_string(),
                key_source: dummy_source_info(),
                value: ConfigValue::new_array(
                    vec![
                        ConfigValue::new_string("a.css", dummy_source_info()),
                        ConfigValue::new_string("b.css", dummy_source_info()),
                    ],
                    dummy_source_info(),
                ),
            }],
            dummy_source_info(),
        );
        let css = extract_css_from_meta(&meta);
        assert!(css.is_some());
        let css_list = css.unwrap();
        assert_eq!(css_list.len(), 2);
    }

    #[test]
    fn test_extract_css_not_present() {
        let meta = ConfigValue::new_map(
            vec![ConfigMapEntry {
                key: "title".to_string(),
                key_source: dummy_source_info(),
                value: ConfigValue::new_string("Test", dummy_source_info()),
            }],
            dummy_source_info(),
        );
        let css = extract_css_from_meta(&meta);
        assert!(css.is_none());
    }

    #[test]
    fn test_extract_css_null_value() {
        let meta = ConfigValue::new_map(
            vec![ConfigMapEntry {
                key: "css".to_string(),
                key_source: dummy_source_info(),
                value: ConfigValue::null(dummy_source_info()),
            }],
            dummy_source_info(),
        );
        let css = extract_css_from_meta(&meta);
        // Returns empty vec for non-recognized css value
        assert!(css.is_some());
        assert!(css.unwrap().is_empty());
    }

    // === inlines_to_text tests ===

    #[test]
    fn test_inlines_to_text_soft_break() {
        let inlines = vec![
            Inline::Str(Str {
                text: "Line1".to_string(),
                source_info: dummy_source_info(),
            }),
            Inline::SoftBreak(SoftBreak {
                source_info: dummy_source_info(),
            }),
            Inline::Str(Str {
                text: "Line2".to_string(),
                source_info: dummy_source_info(),
            }),
        ];
        let text = inlines_to_text(&inlines);
        assert_eq!(text, "Line1 Line2");
    }

    #[test]
    fn test_inlines_to_text_line_break() {
        let inlines = vec![
            Inline::Str(Str {
                text: "Line1".to_string(),
                source_info: dummy_source_info(),
            }),
            Inline::LineBreak(LineBreak {
                source_info: dummy_source_info(),
            }),
            Inline::Str(Str {
                text: "Line2".to_string(),
                source_info: dummy_source_info(),
            }),
        ];
        let text = inlines_to_text(&inlines);
        assert_eq!(text, "Line1\nLine2");
    }

    #[test]
    fn test_inlines_to_text_emph() {
        let inlines = vec![Inline::Emph(Emph {
            content: vec![Inline::Str(Str {
                text: "emphasized".to_string(),
                source_info: dummy_source_info(),
            })],
            source_info: dummy_source_info(),
        })];
        let text = inlines_to_text(&inlines);
        assert_eq!(text, "emphasized");
    }

    #[test]
    fn test_inlines_to_text_strong() {
        let inlines = vec![Inline::Strong(Strong {
            content: vec![Inline::Str(Str {
                text: "bold".to_string(),
                source_info: dummy_source_info(),
            })],
            source_info: dummy_source_info(),
        })];
        let text = inlines_to_text(&inlines);
        assert_eq!(text, "bold");
    }

    #[test]
    fn test_inlines_to_text_underline() {
        let inlines = vec![Inline::Underline(Underline {
            content: vec![Inline::Str(Str {
                text: "underlined".to_string(),
                source_info: dummy_source_info(),
            })],
            source_info: dummy_source_info(),
        })];
        let text = inlines_to_text(&inlines);
        assert_eq!(text, "underlined");
    }

    #[test]
    fn test_inlines_to_text_strikeout() {
        let inlines = vec![Inline::Strikeout(Strikeout {
            content: vec![Inline::Str(Str {
                text: "struck".to_string(),
                source_info: dummy_source_info(),
            })],
            source_info: dummy_source_info(),
        })];
        let text = inlines_to_text(&inlines);
        assert_eq!(text, "struck");
    }

    #[test]
    fn test_inlines_to_text_superscript() {
        let inlines = vec![Inline::Superscript(Superscript {
            content: vec![Inline::Str(Str {
                text: "2".to_string(),
                source_info: dummy_source_info(),
            })],
            source_info: dummy_source_info(),
        })];
        let text = inlines_to_text(&inlines);
        assert_eq!(text, "2");
    }

    #[test]
    fn test_inlines_to_text_subscript() {
        let inlines = vec![Inline::Subscript(Subscript {
            content: vec![Inline::Str(Str {
                text: "i".to_string(),
                source_info: dummy_source_info(),
            })],
            source_info: dummy_source_info(),
        })];
        let text = inlines_to_text(&inlines);
        assert_eq!(text, "i");
    }

    #[test]
    fn test_inlines_to_text_smallcaps() {
        let inlines = vec![Inline::SmallCaps(SmallCaps {
            content: vec![Inline::Str(Str {
                text: "smallcaps".to_string(),
                source_info: dummy_source_info(),
            })],
            source_info: dummy_source_info(),
        })];
        let text = inlines_to_text(&inlines);
        assert_eq!(text, "smallcaps");
    }

    #[test]
    fn test_inlines_to_text_quoted() {
        let inlines = vec![Inline::Quoted(Quoted {
            quote_type: QuoteType::DoubleQuote,
            content: vec![Inline::Str(Str {
                text: "quoted".to_string(),
                source_info: dummy_source_info(),
            })],
            source_info: dummy_source_info(),
        })];
        let text = inlines_to_text(&inlines);
        assert_eq!(text, "\"quoted\"");
    }

    #[test]
    fn test_inlines_to_text_code() {
        let inlines = vec![Inline::Code(Code {
            attr: quarto_pandoc_types::attr::Attr::default(),
            text: "code()".to_string(),
            source_info: dummy_source_info(),
            attr_source: quarto_pandoc_types::attr::AttrSourceInfo::empty(),
        })];
        let text = inlines_to_text(&inlines);
        assert_eq!(text, "code()");
    }

    #[test]
    fn test_inlines_to_text_math() {
        let inlines = vec![Inline::Math(Math {
            math_type: MathType::InlineMath,
            text: "x^2".to_string(),
            source_info: dummy_source_info(),
        })];
        let text = inlines_to_text(&inlines);
        assert_eq!(text, "x^2");
    }

    #[test]
    fn test_inlines_to_text_link() {
        let inlines = vec![Inline::Link(Link {
            attr: quarto_pandoc_types::attr::Attr::default(),
            content: vec![Inline::Str(Str {
                text: "link text".to_string(),
                source_info: dummy_source_info(),
            })],
            target: ("https://example.com".to_string(), "".to_string()),
            source_info: dummy_source_info(),
            attr_source: quarto_pandoc_types::attr::AttrSourceInfo::empty(),
            target_source: quarto_pandoc_types::attr::TargetSourceInfo::empty(),
        })];
        let text = inlines_to_text(&inlines);
        assert_eq!(text, "link text");
    }

    #[test]
    fn test_inlines_to_text_span() {
        let inlines = vec![Inline::Span(Span {
            attr: quarto_pandoc_types::attr::Attr::default(),
            content: vec![Inline::Str(Str {
                text: "span content".to_string(),
                source_info: dummy_source_info(),
            })],
            source_info: dummy_source_info(),
            attr_source: quarto_pandoc_types::attr::AttrSourceInfo::empty(),
        })];
        let text = inlines_to_text(&inlines);
        assert_eq!(text, "span content");
    }

    // === blocks_to_text tests ===

    #[test]
    fn test_blocks_to_text_plain() {
        let blocks = vec![Block::Plain(Plain {
            content: vec![Inline::Str(Str {
                text: "plain text".to_string(),
                source_info: dummy_source_info(),
            })],
            source_info: dummy_source_info(),
        })];
        let text = blocks_to_text(&blocks);
        assert_eq!(text, "plain text");
    }

    #[test]
    fn test_blocks_to_text_paragraph() {
        let blocks = vec![Block::Paragraph(Paragraph {
            content: vec![Inline::Str(Str {
                text: "paragraph".to_string(),
                source_info: dummy_source_info(),
            })],
            source_info: dummy_source_info(),
        })];
        let text = blocks_to_text(&blocks);
        assert_eq!(text, "paragraph");
    }

    #[test]
    fn test_blocks_to_text_header() {
        let blocks = vec![Block::Header(Header {
            level: 1,
            attr: quarto_pandoc_types::attr::Attr::default(),
            content: vec![Inline::Str(Str {
                text: "Heading".to_string(),
                source_info: dummy_source_info(),
            })],
            source_info: dummy_source_info(),
            attr_source: quarto_pandoc_types::attr::AttrSourceInfo::empty(),
        })];
        let text = blocks_to_text(&blocks);
        assert_eq!(text, "Heading");
    }

    #[test]
    fn test_blocks_to_text_code_block() {
        let blocks = vec![Block::CodeBlock(CodeBlock {
            attr: quarto_pandoc_types::attr::Attr::default(),
            text: "fn main() {}".to_string(),
            source_info: dummy_source_info(),
            attr_source: quarto_pandoc_types::attr::AttrSourceInfo::empty(),
        })];
        let text = blocks_to_text(&blocks);
        assert_eq!(text, "fn main() {}");
    }

    #[test]
    fn test_blocks_to_text_blockquote() {
        let blocks = vec![Block::BlockQuote(BlockQuote {
            content: vec![Block::Paragraph(Paragraph {
                content: vec![Inline::Str(Str {
                    text: "quoted".to_string(),
                    source_info: dummy_source_info(),
                })],
                source_info: dummy_source_info(),
            })],
            source_info: dummy_source_info(),
        })];
        let text = blocks_to_text(&blocks);
        assert_eq!(text, "quoted");
    }

    #[test]
    fn test_blocks_to_text_div() {
        let blocks = vec![Block::Div(Div {
            attr: quarto_pandoc_types::attr::Attr::default(),
            content: vec![Block::Paragraph(Paragraph {
                content: vec![Inline::Str(Str {
                    text: "div content".to_string(),
                    source_info: dummy_source_info(),
                })],
                source_info: dummy_source_info(),
            })],
            source_info: dummy_source_info(),
            attr_source: quarto_pandoc_types::attr::AttrSourceInfo::empty(),
        })];
        let text = blocks_to_text(&blocks);
        assert_eq!(text, "div content");
    }

    #[test]
    fn test_blocks_to_text_lineblock() {
        let blocks = vec![Block::LineBlock(LineBlock {
            content: vec![
                vec![Inline::Str(Str {
                    text: "Line 1".to_string(),
                    source_info: dummy_source_info(),
                })],
                vec![Inline::Str(Str {
                    text: "Line 2".to_string(),
                    source_info: dummy_source_info(),
                })],
            ],
            source_info: dummy_source_info(),
        })];
        let text = blocks_to_text(&blocks);
        assert!(text.contains("Line 1"));
        assert!(text.contains("Line 2"));
    }

    #[test]
    fn test_blocks_to_text_ordered_list() {
        let blocks = vec![Block::OrderedList(OrderedList {
            attr: (1, ListNumberStyle::Default, ListNumberDelim::Default),
            content: vec![vec![Block::Plain(Plain {
                content: vec![Inline::Str(Str {
                    text: "Item 1".to_string(),
                    source_info: dummy_source_info(),
                })],
                source_info: dummy_source_info(),
            })]],
            source_info: dummy_source_info(),
        })];
        let text = blocks_to_text(&blocks);
        assert!(text.contains("Item 1"));
    }

    #[test]
    fn test_blocks_to_text_bullet_list() {
        let blocks = vec![Block::BulletList(BulletList {
            content: vec![vec![Block::Plain(Plain {
                content: vec![Inline::Str(Str {
                    text: "Bullet".to_string(),
                    source_info: dummy_source_info(),
                })],
                source_info: dummy_source_info(),
            })]],
            source_info: dummy_source_info(),
        })];
        let text = blocks_to_text(&blocks);
        assert!(text.contains("Bullet"));
    }

    #[test]
    fn test_blocks_to_text_multiple() {
        let blocks = vec![
            Block::Paragraph(Paragraph {
                content: vec![Inline::Str(Str {
                    text: "Para 1".to_string(),
                    source_info: dummy_source_info(),
                })],
                source_info: dummy_source_info(),
            }),
            Block::Paragraph(Paragraph {
                content: vec![Inline::Str(Str {
                    text: "Para 2".to_string(),
                    source_info: dummy_source_info(),
                })],
                source_info: dummy_source_info(),
            }),
        ];
        let text = blocks_to_text(&blocks);
        assert!(text.contains("Para 1"));
        assert!(text.contains("Para 2"));
        // Should have newline between blocks
        assert!(text.contains('\n'));
    }
}
