/*
 * callout.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Transform that converts callout Divs to CustomNodes.
 */

//! Callout conversion transform.
//!
//! This transform finds Div blocks with `.callout-*` classes and converts
//! them to CustomNode blocks with type "Callout". This enables the HTML
//! writer to render them with proper callout styling.
//!
//! ## Input Structure
//!
//! A callout in the source document looks like:
//!
//! ```markdown
//! ::: {.callout-warning}
//! ## Optional Title
//!
//! Body content here.
//! :::
//! ```
//!
//! This is parsed as a Div with class "callout-warning" containing a Header
//! and Paragraph blocks.
//!
//! ## Output Structure
//!
//! The transform converts this to a CustomNode with:
//! - `type_name`: "Callout"
//! - `slots`:
//!   - "title": Inlines from the first Header (if present)
//!   - "content": Blocks (remaining blocks after title extraction)
//! - `plain_data`: `{"type": "warning", "appearance": "default", ...}`
//! - `attr`: Original Div attributes

use quarto_pandoc_types::attr::Attr;
use quarto_pandoc_types::block::{Block, Div};
use quarto_pandoc_types::custom::{CustomNode, Slot};
use quarto_pandoc_types::pandoc::Pandoc;
use serde_json::json;

use crate::render::RenderContext;
use crate::transform::AstTransform;
use crate::Result;

/// Known callout types in Quarto.
const CALLOUT_TYPES: &[&str] = &["note", "warning", "tip", "caution", "important"];

/// Transform that converts callout Divs to CustomNodes.
///
/// This allows the HTML writer to render callouts with proper structure
/// (header, icon, title, body) rather than as plain divs.
pub struct CalloutTransform;

impl CalloutTransform {
    /// Create a new callout transform.
    pub fn new() -> Self {
        Self
    }
}

impl Default for CalloutTransform {
    fn default() -> Self {
        Self::new()
    }
}

impl AstTransform for CalloutTransform {
    fn name(&self) -> &str {
        "callout"
    }

    fn transform(&self, ast: &mut Pandoc, _ctx: &mut RenderContext) -> Result<()> {
        // Transform all blocks in the document
        transform_blocks(&mut ast.blocks);
        Ok(())
    }
}

/// Transform a vector of blocks, converting callout Divs to CustomNodes.
fn transform_blocks(blocks: &mut Vec<Block>) {
    for block in blocks.iter_mut() {
        transform_block(block);
    }
}

/// Transform a single block, potentially converting it to a CustomNode.
fn transform_block(block: &mut Block) {
    // First, recursively transform any nested blocks
    match block {
        Block::BlockQuote(bq) => {
            transform_blocks(&mut bq.content);
        }
        Block::OrderedList(ol) => {
            for item in &mut ol.content {
                transform_blocks(item);
            }
        }
        Block::BulletList(bl) => {
            for item in &mut bl.content {
                transform_blocks(item);
            }
        }
        Block::DefinitionList(dl) => {
            for (_term, defs) in &mut dl.content {
                for def in defs {
                    transform_blocks(def);
                }
            }
        }
        Block::Figure(fig) => {
            transform_blocks(&mut fig.content);
        }
        Block::Div(div) => {
            // First transform nested content
            transform_blocks(&mut div.content);

            // Then check if this div is a callout and convert it
            if let Some(callout_type) = extract_callout_type(&div.attr) {
                let custom = convert_div_to_callout(div, &callout_type);
                *block = Block::Custom(custom);
            }
        }
        Block::Table(table) => {
            // Transform table bodies
            for body in &mut table.bodies {
                for row in &mut body.body {
                    for cell in &mut row.cells {
                        transform_blocks(&mut cell.content);
                    }
                }
            }
            // Transform table head
            for row in &mut table.head.rows {
                for cell in &mut row.cells {
                    transform_blocks(&mut cell.content);
                }
            }
            // Transform table foot
            for row in &mut table.foot.rows {
                for cell in &mut row.cells {
                    transform_blocks(&mut cell.content);
                }
            }
        }
        Block::Custom(custom) => {
            // Transform blocks inside custom node slots
            for (_name, slot) in &mut custom.slots {
                match slot {
                    Slot::Block(b) => transform_block(b),
                    Slot::Blocks(bs) => transform_blocks(bs),
                    _ => {}
                }
            }
        }
        // Other block types don't contain nested blocks
        _ => {}
    }
}

/// Extract the callout type from a Div's attributes.
///
/// Returns Some("warning") for a div with class "callout-warning", etc.
/// Returns None if this is not a callout div.
fn extract_callout_type(attr: &Attr) -> Option<String> {
    let (_id, classes, _attrs) = attr;

    for class in classes {
        // Check for "callout-TYPE" pattern
        if let Some(suffix) = class.strip_prefix("callout-") {
            // Verify it's a known callout type
            if CALLOUT_TYPES.contains(&suffix) {
                return Some(suffix.to_string());
            }
        }
    }

    None
}

/// Convert a Div to a CustomNode with type "Callout".
fn convert_div_to_callout(div: &mut Div, callout_type: &str) -> CustomNode {
    let mut content_blocks = std::mem::take(&mut div.content);
    let mut title_inlines = Vec::new();

    // Check if the first block is a Header - if so, use it as the title
    if let Some(Block::Header(header)) = content_blocks.first() {
        // Only use H2 or lower as title (H1 would be document title)
        if header.level >= 2 {
            title_inlines = header.content.clone();
            content_blocks.remove(0);
        }
    }

    // Extract additional attributes from the div
    let appearance = extract_attr_value(&div.attr, "appearance").unwrap_or("default".to_string());
    let collapse = extract_attr_value(&div.attr, "collapse")
        .map(|v| v == "true")
        .unwrap_or(false);
    let icon = extract_attr_value(&div.attr, "icon")
        .map(|v| v != "false")
        .unwrap_or(true);

    // Build the plain_data JSON
    let plain_data = json!({
        "type": callout_type,
        "appearance": appearance,
        "collapse": collapse,
        "icon": icon
    });

    // Create the CustomNode
    let mut custom = CustomNode::new("Callout", div.attr.clone(), div.source_info.clone());
    custom.plain_data = plain_data;

    // Add title slot (may be empty if no header)
    custom.set_slot("title", Slot::Inlines(title_inlines));

    // Add content slot
    custom.set_slot("content", Slot::Blocks(content_blocks));

    custom
}

/// Extract a key-value attribute from the Div's attr.
fn extract_attr_value(attr: &Attr, key: &str) -> Option<String> {
    let (_id, _classes, attrs) = attr;
    attrs.get(key).cloned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_pandoc_types::attr::{empty_attr, AttrSourceInfo};
    use quarto_pandoc_types::block::{Header, Paragraph};
    use quarto_pandoc_types::inline::Str;
    use quarto_source_map::{FileId, Location, Range, SourceInfo};

    use crate::format::Format;
    use crate::project::{DocumentInfo, ProjectContext};
    use crate::render::{BinaryDependencies, RenderContext};

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

    fn callout_attr(callout_type: &str) -> Attr {
        (
            String::new(),
            vec![format!("callout-{}", callout_type)],
            hashlink::LinkedHashMap::new(),
        )
    }

    #[test]
    fn test_extract_callout_type_warning() {
        let attr = callout_attr("warning");
        assert_eq!(extract_callout_type(&attr), Some("warning".to_string()));
    }

    #[test]
    fn test_extract_callout_type_note() {
        let attr = callout_attr("note");
        assert_eq!(extract_callout_type(&attr), Some("note".to_string()));
    }

    #[test]
    fn test_extract_callout_type_unknown() {
        let attr = (
            String::new(),
            vec!["callout-unknown".to_string()],
            hashlink::LinkedHashMap::new(),
        );
        // Unknown callout types are not converted
        assert_eq!(extract_callout_type(&attr), None);
    }

    #[test]
    fn test_extract_callout_type_not_callout() {
        let attr = (
            String::new(),
            vec!["panel-tabset".to_string()],
            hashlink::LinkedHashMap::new(),
        );
        assert_eq!(extract_callout_type(&attr), None);
    }

    #[test]
    fn test_convert_simple_callout() {
        let mut ast = Pandoc {
            meta: quarto_pandoc_types::meta::MetaValueWithSourceInfo::MetaMap {
                entries: vec![],
                source_info: dummy_source_info(),
            },
            blocks: vec![Block::Div(Div {
                attr: callout_attr("warning"),
                content: vec![Block::Paragraph(Paragraph {
                    content: vec![quarto_pandoc_types::inline::Inline::Str(Str {
                        text: "Warning content".to_string(),
                        source_info: dummy_source_info(),
                    })],
                    source_info: dummy_source_info(),
                })],
                source_info: dummy_source_info(),
                attr_source: AttrSourceInfo::empty(),
            })],
        };

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

        let transform = CalloutTransform::new();
        transform.transform(&mut ast, &mut ctx).unwrap();

        // Verify the Div was converted to a Custom node
        assert_eq!(ast.blocks.len(), 1);
        match &ast.blocks[0] {
            Block::Custom(custom) => {
                assert_eq!(custom.type_name, "Callout");
                assert_eq!(custom.plain_data["type"], "warning");
                assert!(custom.get_slot("content").is_some());
            }
            _ => panic!("Expected Custom block"),
        }
    }

    #[test]
    fn test_convert_callout_with_title() {
        let mut ast = Pandoc {
            meta: quarto_pandoc_types::meta::MetaValueWithSourceInfo::MetaMap {
                entries: vec![],
                source_info: dummy_source_info(),
            },
            blocks: vec![Block::Div(Div {
                attr: callout_attr("tip"),
                content: vec![
                    Block::Header(Header {
                        level: 2,
                        attr: empty_attr(),
                        content: vec![quarto_pandoc_types::inline::Inline::Str(Str {
                            text: "Pro Tip".to_string(),
                            source_info: dummy_source_info(),
                        })],
                        source_info: dummy_source_info(),
                        attr_source: AttrSourceInfo::empty(),
                    }),
                    Block::Paragraph(Paragraph {
                        content: vec![quarto_pandoc_types::inline::Inline::Str(Str {
                            text: "Tip content".to_string(),
                            source_info: dummy_source_info(),
                        })],
                        source_info: dummy_source_info(),
                    }),
                ],
                source_info: dummy_source_info(),
                attr_source: AttrSourceInfo::empty(),
            })],
        };

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

        let transform = CalloutTransform::new();
        transform.transform(&mut ast, &mut ctx).unwrap();

        // Verify the Custom node has title and content
        match &ast.blocks[0] {
            Block::Custom(custom) => {
                assert_eq!(custom.type_name, "Callout");
                assert_eq!(custom.plain_data["type"], "tip");

                // Check title slot
                match custom.get_slot("title") {
                    Some(Slot::Inlines(inlines)) => {
                        assert_eq!(inlines.len(), 1);
                    }
                    _ => panic!("Expected title slot with Inlines"),
                }

                // Check content slot
                match custom.get_slot("content") {
                    Some(Slot::Blocks(blocks)) => {
                        assert_eq!(blocks.len(), 1); // Just the paragraph, header removed
                    }
                    _ => panic!("Expected content slot with Blocks"),
                }
            }
            _ => panic!("Expected Custom block"),
        }
    }

    #[test]
    fn test_nested_callout_in_blockquote() {
        let mut ast = Pandoc {
            meta: quarto_pandoc_types::meta::MetaValueWithSourceInfo::MetaMap {
                entries: vec![],
                source_info: dummy_source_info(),
            },
            blocks: vec![Block::BlockQuote(quarto_pandoc_types::block::BlockQuote {
                content: vec![Block::Div(Div {
                    attr: callout_attr("note"),
                    content: vec![Block::Paragraph(Paragraph {
                        content: vec![quarto_pandoc_types::inline::Inline::Str(Str {
                            text: "Nested note".to_string(),
                            source_info: dummy_source_info(),
                        })],
                        source_info: dummy_source_info(),
                    })],
                    source_info: dummy_source_info(),
                    attr_source: AttrSourceInfo::empty(),
                })],
                source_info: dummy_source_info(),
            })],
        };

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

        let transform = CalloutTransform::new();
        transform.transform(&mut ast, &mut ctx).unwrap();

        // Verify the nested Div was converted
        match &ast.blocks[0] {
            Block::BlockQuote(bq) => match &bq.content[0] {
                Block::Custom(custom) => {
                    assert_eq!(custom.type_name, "Callout");
                    assert_eq!(custom.plain_data["type"], "note");
                }
                _ => panic!("Expected Custom block inside BlockQuote"),
            },
            _ => panic!("Expected BlockQuote"),
        }
    }

    #[test]
    fn test_transform_name() {
        let transform = CalloutTransform::new();
        assert_eq!(transform.name(), "callout");
    }
}
