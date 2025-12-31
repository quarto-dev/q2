/*
 * callout_resolve.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Transform that resolves Callout CustomNodes to standard Pandoc AST.
 */

//! Callout resolution transform.
//!
//! This transform converts Callout CustomNodes back to standard Pandoc AST
//! (Div blocks with appropriate structure). This separation allows:
//!
//! 1. The HTML writer to remain generic (no knowledge of callout semantics)
//! 2. Different resolve transforms to produce different HTML structures
//! 3. A single source of HTML-writing behavior in the codebase
//!
//! ## Pipeline Order
//!
//! This transform should run AFTER `CalloutTransform`:
//! 1. `CalloutTransform`: Div with `.callout-*` → CustomNode("Callout")
//! 2. `CalloutResolveTransform`: CustomNode("Callout") → Div with HTML structure
//!
//! ## Output Structure
//!
//! The transform produces this Div structure (which renders to the expected HTML):
//!
//! ```text
//! Div.callout.callout-{type}
//!   Div.callout-header
//!     Div.callout-icon-container
//!       Plain[RawInline(html, "<i class=\"callout-icon\"></i>")]
//!     Div.callout-title-container.flex-fill
//!       Plain[title inlines...]
//!   Div.callout-body-container.callout-body
//!     [content blocks...]
//! ```

use hashlink::LinkedHashMap;
use quarto_pandoc_types::attr::{Attr, AttrSourceInfo};
use quarto_pandoc_types::block::{Block, Div, Plain};
use quarto_pandoc_types::custom::{CustomNode, Slot};
use quarto_pandoc_types::inline::{Inline, RawInline, Str};
use quarto_pandoc_types::pandoc::Pandoc;
use quarto_source_map::SourceInfo;
use serde_json::Value;

use crate::Result;
use crate::render::RenderContext;
use crate::transform::AstTransform;

/// Transform that resolves Callout CustomNodes to standard Pandoc Div structure.
///
/// This enables the HTML writer to remain generic while still producing
/// the expected callout HTML structure.
pub struct CalloutResolveTransform;

impl CalloutResolveTransform {
    /// Create a new callout resolve transform.
    pub fn new() -> Self {
        Self
    }
}

impl Default for CalloutResolveTransform {
    fn default() -> Self {
        Self::new()
    }
}

impl AstTransform for CalloutResolveTransform {
    fn name(&self) -> &str {
        "callout-resolve"
    }

    fn transform(&self, ast: &mut Pandoc, _ctx: &mut RenderContext) -> Result<()> {
        resolve_blocks(&mut ast.blocks);
        Ok(())
    }
}

/// Resolve CustomNodes in a vector of blocks.
fn resolve_blocks(blocks: &mut Vec<Block>) {
    for block in blocks.iter_mut() {
        resolve_block(block);
    }
}

/// Resolve a single block, potentially converting CustomNode to Div.
fn resolve_block(block: &mut Block) {
    // First, recursively resolve any nested blocks
    match block {
        Block::BlockQuote(bq) => {
            resolve_blocks(&mut bq.content);
        }
        Block::OrderedList(ol) => {
            for item in &mut ol.content {
                resolve_blocks(item);
            }
        }
        Block::BulletList(bl) => {
            for item in &mut bl.content {
                resolve_blocks(item);
            }
        }
        Block::DefinitionList(dl) => {
            for (_term, defs) in &mut dl.content {
                for def in defs {
                    resolve_blocks(def);
                }
            }
        }
        Block::Figure(fig) => {
            resolve_blocks(&mut fig.content);
        }
        Block::Div(div) => {
            resolve_blocks(&mut div.content);
        }
        Block::Table(table) => {
            for body in &mut table.bodies {
                for row in &mut body.body {
                    for cell in &mut row.cells {
                        resolve_blocks(&mut cell.content);
                    }
                }
            }
            for row in &mut table.head.rows {
                for cell in &mut row.cells {
                    resolve_blocks(&mut cell.content);
                }
            }
            for row in &mut table.foot.rows {
                for cell in &mut row.cells {
                    resolve_blocks(&mut cell.content);
                }
            }
        }
        Block::Custom(custom) => {
            // First resolve any nested blocks in slots
            for (_name, slot) in &mut custom.slots {
                match slot {
                    Slot::Block(b) => resolve_block(b),
                    Slot::Blocks(bs) => resolve_blocks(bs),
                    _ => {}
                }
            }

            // Then check if this is a Callout that should be resolved
            if custom.type_name == "Callout" {
                let resolved_div = resolve_callout(custom);
                *block = Block::Div(resolved_div);
            }
        }
        // Other block types don't contain nested blocks
        _ => {}
    }
}

/// Resolve a Callout CustomNode to a Div with the expected HTML structure.
fn resolve_callout(custom: &mut CustomNode) -> Div {
    // Extract callout properties from plain_data
    let callout_type = extract_string(&custom.plain_data, "type").unwrap_or("note");
    let appearance = extract_string(&custom.plain_data, "appearance").unwrap_or("default");
    let collapse = extract_bool(&custom.plain_data, "collapse").unwrap_or(false);
    let icon = extract_bool(&custom.plain_data, "icon").unwrap_or(true);

    let source_info = custom.source_info.clone();

    // Build class list for outer div
    let mut classes = vec!["callout".to_string(), format!("callout-{}", callout_type)];
    if appearance != "default" {
        classes.push(format!("callout-appearance-{}", appearance));
    }
    if collapse {
        classes.push("callout-collapse".to_string());
    }

    // Include original non-callout classes from attr
    let (orig_id, orig_classes, orig_attrs) = &custom.attr;
    for cls in orig_classes {
        if !cls.starts_with("callout") {
            classes.push(cls.clone());
        }
    }

    // Build outer div attr
    let outer_attr: Attr = (orig_id.clone(), classes, orig_attrs.clone());

    // Extract title and content from slots
    let title_inlines = extract_title_inlines(custom, callout_type);
    let content_blocks = extract_content_blocks(custom);

    // Build the inner structure
    let mut header_content = Vec::new();

    // Icon container (if enabled)
    if icon {
        header_content.push(Block::Div(Div {
            attr: make_attr(&["callout-icon-container"]),
            content: vec![Block::Plain(Plain {
                content: vec![Inline::RawInline(RawInline {
                    format: "html".to_string(),
                    text: "<i class=\"callout-icon\"></i>".to_string(),
                    source_info: source_info.clone(),
                })],
                source_info: source_info.clone(),
            })],
            source_info: source_info.clone(),
            attr_source: AttrSourceInfo::empty(),
        }));
    }

    // Title container
    header_content.push(Block::Div(Div {
        attr: make_attr(&["callout-title-container", "flex-fill"]),
        content: vec![Block::Plain(Plain {
            content: title_inlines,
            source_info: source_info.clone(),
        })],
        source_info: source_info.clone(),
        attr_source: AttrSourceInfo::empty(),
    }));

    // Header div
    let header_div = Block::Div(Div {
        attr: make_attr(&["callout-header"]),
        content: header_content,
        source_info: source_info.clone(),
        attr_source: AttrSourceInfo::empty(),
    });

    // Body div
    let body_div = Block::Div(Div {
        attr: make_attr(&["callout-body-container", "callout-body"]),
        content: content_blocks,
        source_info: source_info.clone(),
        attr_source: AttrSourceInfo::empty(),
    });

    // Outer callout div
    Div {
        attr: outer_attr,
        content: vec![header_div, body_div],
        source_info,
        attr_source: AttrSourceInfo::empty(),
    }
}

/// Extract title inlines from the CustomNode, with fallback to default.
fn extract_title_inlines(custom: &CustomNode, callout_type: &str) -> Vec<Inline> {
    if let Some(title_slot) = custom.get_slot("title") {
        match title_slot {
            Slot::Inlines(inlines) if !inlines.is_empty() => {
                return inlines.clone();
            }
            Slot::Inline(inline) => {
                return vec![inline.as_ref().clone()];
            }
            _ => {}
        }
    }

    // Default title based on callout type
    let default_title = capitalize(callout_type);
    vec![Inline::Str(Str {
        text: default_title,
        source_info: SourceInfo::default(),
    })]
}

/// Extract content blocks from the CustomNode.
fn extract_content_blocks(custom: &mut CustomNode) -> Vec<Block> {
    if let Some(content_slot) = custom.slots.remove("content") {
        match content_slot {
            Slot::Blocks(blocks) => blocks,
            Slot::Block(block) => vec![*block],
            _ => Vec::new(),
        }
    } else {
        Vec::new()
    }
}

/// Create an Attr with the given classes.
fn make_attr(classes: &[&str]) -> Attr {
    (
        String::new(),
        classes.iter().map(|s| (*s).to_string()).collect(),
        LinkedHashMap::new(),
    )
}

/// Extract a string value from JSON.
fn extract_string<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(|v| v.as_str())
}

/// Extract a bool value from JSON.
fn extract_bool(value: &Value, key: &str) -> Option<bool> {
    value.get(key).and_then(|v| v.as_bool())
}

/// Capitalize the first letter of a string.
fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_pandoc_types::attr::empty_attr;
    use quarto_pandoc_types::block::Paragraph;
    use quarto_source_map::{FileId, Location, Range};
    use serde_json::json;

    use crate::format::Format;
    use crate::project::{DocumentInfo, ProjectContext};
    use crate::render::BinaryDependencies;

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

    fn make_test_project() -> ProjectContext {
        ProjectContext {
            dir: std::path::PathBuf::from("/project"),
            config: None,
            is_single_file: true,
            files: vec![DocumentInfo::from_path("/project/doc.qmd")],
            output_dir: std::path::PathBuf::from("/project"),
        }
    }

    #[test]
    fn test_transform_name() {
        let transform = CalloutResolveTransform::new();
        assert_eq!(transform.name(), "callout-resolve");
    }

    #[test]
    fn test_resolve_simple_callout() {
        let mut custom = CustomNode::new("Callout", empty_attr(), dummy_source_info());
        custom.plain_data = json!({"type": "warning"});
        custom.set_slot(
            "content",
            Slot::Blocks(vec![Block::Paragraph(Paragraph {
                content: vec![Inline::Str(Str {
                    text: "Warning content".to_string(),
                    source_info: dummy_source_info(),
                })],
                source_info: dummy_source_info(),
            })]),
        );

        let mut ast = Pandoc {
            meta: quarto_pandoc_types::ConfigValue::default(),
            blocks: vec![Block::Custom(custom)],
        };

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

        let transform = CalloutResolveTransform::new();
        transform.transform(&mut ast, &mut ctx).unwrap();

        // Verify the CustomNode was converted to a Div
        assert_eq!(ast.blocks.len(), 1);
        match &ast.blocks[0] {
            Block::Div(div) => {
                let (_, classes, _) = &div.attr;
                assert!(classes.contains(&"callout".to_string()));
                assert!(classes.contains(&"callout-warning".to_string()));

                // Should have header and body divs
                assert_eq!(div.content.len(), 2);

                // Check header structure
                match &div.content[0] {
                    Block::Div(header) => {
                        let (_, classes, _) = &header.attr;
                        assert!(classes.contains(&"callout-header".to_string()));
                    }
                    _ => panic!("Expected header Div"),
                }

                // Check body structure
                match &div.content[1] {
                    Block::Div(body) => {
                        let (_, classes, _) = &body.attr;
                        assert!(classes.contains(&"callout-body-container".to_string()));
                        assert!(classes.contains(&"callout-body".to_string()));
                    }
                    _ => panic!("Expected body Div"),
                }
            }
            _ => panic!("Expected Div block"),
        }
    }

    #[test]
    fn test_resolve_callout_with_title() {
        let mut custom = CustomNode::new("Callout", empty_attr(), dummy_source_info());
        custom.plain_data = json!({"type": "tip"});
        custom.set_slot(
            "title",
            Slot::Inlines(vec![Inline::Str(Str {
                text: "Pro Tip".to_string(),
                source_info: dummy_source_info(),
            })]),
        );
        custom.set_slot(
            "content",
            Slot::Blocks(vec![Block::Paragraph(Paragraph {
                content: vec![Inline::Str(Str {
                    text: "Tip content".to_string(),
                    source_info: dummy_source_info(),
                })],
                source_info: dummy_source_info(),
            })]),
        );

        let mut ast = Pandoc {
            meta: quarto_pandoc_types::ConfigValue::default(),
            blocks: vec![Block::Custom(custom)],
        };

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

        let transform = CalloutResolveTransform::new();
        transform.transform(&mut ast, &mut ctx).unwrap();

        // Verify structure
        match &ast.blocks[0] {
            Block::Div(div) => {
                let (_, classes, _) = &div.attr;
                assert!(classes.contains(&"callout-tip".to_string()));
            }
            _ => panic!("Expected Div"),
        }
    }

    #[test]
    fn test_resolve_callout_default_title() {
        let mut custom = CustomNode::new("Callout", empty_attr(), dummy_source_info());
        custom.plain_data = json!({"type": "note"});
        // No title slot - should use default

        let resolved = resolve_callout(&mut custom);

        // Find the title container and check it has "Note"
        let header = &resolved.content[0];
        if let Block::Div(header_div) = header {
            // Find title container (second child if icon is present)
            for block in &header_div.content {
                if let Block::Div(div) = block {
                    let (_, classes, _) = &div.attr;
                    if classes.contains(&"callout-title-container".to_string()) {
                        if let Block::Plain(plain) = &div.content[0] {
                            if let Inline::Str(s) = &plain.content[0] {
                                assert_eq!(s.text, "Note");
                                return;
                            }
                        }
                    }
                }
            }
        }
        panic!("Could not find default title");
    }

    #[test]
    fn test_resolve_callout_no_icon() {
        let mut custom = CustomNode::new("Callout", empty_attr(), dummy_source_info());
        custom.plain_data = json!({"type": "warning", "icon": false});

        let resolved = resolve_callout(&mut custom);

        // Header should only have title container, no icon
        if let Block::Div(header_div) = &resolved.content[0] {
            // With icon=false, header should have only 1 child (title container)
            assert_eq!(header_div.content.len(), 1);
            if let Block::Div(title_div) = &header_div.content[0] {
                let (_, classes, _) = &title_div.attr;
                assert!(classes.contains(&"callout-title-container".to_string()));
            }
        }
    }

    #[test]
    fn test_resolve_nested_callout() {
        // Callout inside a blockquote
        let mut custom = CustomNode::new("Callout", empty_attr(), dummy_source_info());
        custom.plain_data = json!({"type": "note"});

        let mut ast = Pandoc {
            meta: quarto_pandoc_types::ConfigValue::default(),
            blocks: vec![Block::BlockQuote(quarto_pandoc_types::block::BlockQuote {
                content: vec![Block::Custom(custom)],
                source_info: dummy_source_info(),
            })],
        };

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

        let transform = CalloutResolveTransform::new();
        transform.transform(&mut ast, &mut ctx).unwrap();

        // Verify the nested callout was resolved
        match &ast.blocks[0] {
            Block::BlockQuote(bq) => match &bq.content[0] {
                Block::Div(div) => {
                    let (_, classes, _) = &div.attr;
                    assert!(classes.contains(&"callout-note".to_string()));
                }
                _ => panic!("Expected Div inside BlockQuote"),
            },
            _ => panic!("Expected BlockQuote"),
        }
    }

    #[test]
    fn test_capitalize() {
        assert_eq!(capitalize("note"), "Note");
        assert_eq!(capitalize("warning"), "Warning");
        assert_eq!(capitalize(""), "");
        assert_eq!(capitalize("TIP"), "TIP");
    }
}
