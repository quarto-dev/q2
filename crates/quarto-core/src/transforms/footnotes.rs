/*
 * footnotes.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Transform that extracts inline footnotes and creates a footnotes section.
 */

//! Footnotes transform for HTML rendering.
//!
//! This transform extracts inline footnotes from the AST and creates a consolidated
//! footnotes section at the end of the document. It runs in the **normalization phase**
//! of the pipeline, so user Lua filters see the normalized footnote structure.
//!
//! ## Input AST Elements
//!
//! - `Inline::Note` - Inline footnote with block content (e.g., `^[footnote text]`)
//! - `Inline::NoteReference` - Reference to a defined note (e.g., `[^1]`)
//! - `Block::NoteDefinitionPara` - Single-paragraph note definition
//! - `Block::NoteDefinitionFencedBlock` - Multi-paragraph note definition
//!
//! ## Output Structure
//!
//! For `reference-location: document` (default), produces:
//!
//! ```html
//! <p>Text<sup id="fnref1"><a href="#fn1" class="footnote-ref" role="doc-noteref">1</a></sup></p>
//!
//! <section id="footnotes" class="footnotes" role="doc-endnotes">
//!   <hr>
//!   <ol>
//!     <li id="fn1">
//!       <p>Footnote content.<a href="#fnref1" class="footnote-back" role="doc-backlink">↩︎</a></p>
//!     </li>
//!   </ol>
//! </section>
//! ```
//!
//! ## Configuration
//!
//! - `reference-location`: Controls footnote placement
//!   - `document` (default): Footnotes section at end of document
//!   - `margin`: Convert to margin notes (no section created)
//!   - `block`/`section`: Handled by Pandoc, transform is a no-op

use std::collections::HashMap;

use hashlink::LinkedHashMap;
use quarto_pandoc_types::attr::AttrSourceInfo;
use quarto_pandoc_types::block::{Block, Div, OrderedList, Paragraph};
use quarto_pandoc_types::inline::{Inline, Link, Span, Str, Superscript};
use quarto_pandoc_types::pandoc::Pandoc;
use quarto_pandoc_types::{Blocks, Inlines, ListNumberDelim, ListNumberStyle};
use quarto_source_map::SourceInfo;

use crate::Result;
use crate::render::RenderContext;
use crate::transform::AstTransform;
use crate::transforms::ReferenceLocation;

/// Transform that extracts footnotes and creates a footnotes section.
///
/// This transform is part of the **normalization phase** and runs early in the
/// pipeline. It converts Quarto's footnote syntax into a standard structure
/// that the HTML writer can render.
pub struct FootnotesTransform;

impl FootnotesTransform {
    /// Create a new footnotes transform.
    pub fn new() -> Self {
        Self
    }

    /// Get the reference-location configuration.
    ///
    /// Note: Assumes format normalization has lifted document-root options
    /// into format metadata.
    fn get_reference_location(&self, ctx: &RenderContext) -> ReferenceLocation {
        ctx.format_metadata("reference-location")
            .and_then(|v| v.as_str())
            .map(ReferenceLocation::from_str)
            .unwrap_or_default()
    }
}

impl Default for FootnotesTransform {
    fn default() -> Self {
        Self::new()
    }
}

impl AstTransform for FootnotesTransform {
    fn name(&self) -> &str {
        "footnotes"
    }

    fn transform(&self, ast: &mut Pandoc, ctx: &mut RenderContext) -> Result<()> {
        let reference_location = self.get_reference_location(ctx);

        // For block/section placement, Pandoc handles this - no-op
        if matches!(
            reference_location,
            ReferenceLocation::Block | ReferenceLocation::Section
        ) {
            return Ok(());
        }

        // Collect all note definitions first
        let mut note_definitions: HashMap<String, NoteContent> = HashMap::new();
        collect_note_definitions(&mut ast.blocks, &mut note_definitions);

        // Process the document, extracting inline notes and resolving references
        let mut footnote_collector = FootnoteCollector::new(
            note_definitions,
            reference_location == ReferenceLocation::Margin,
        );
        process_blocks(&mut ast.blocks, &mut footnote_collector);

        // Create footnotes section only for document location
        if reference_location == ReferenceLocation::Document
            && !footnote_collector.footnotes.is_empty()
        {
            let footnotes_section = create_footnotes_section(&footnote_collector.footnotes);
            ast.blocks.push(footnotes_section);
        }

        // For margin location:
        // - Footnote references are created with "margin-note" class
        // - No footnotes section is created
        // - Full margin content placement is handled by CSS/layout or future enhancement

        Ok(())
    }
}

/// Content of a footnote (either inline content or block content).
#[derive(Debug, Clone)]
enum NoteContent {
    /// Single paragraph of inline content
    Inlines(Inlines),
    /// Multiple blocks of content
    Blocks(Blocks),
}

/// Collected footnote with its ID and content.
#[derive(Debug, Clone)]
struct CollectedFootnote {
    /// The footnote ID (e.g., "1", "2", or user-defined like "fn-custom")
    id: String,
    /// The footnote number (1-based, for display)
    number: usize,
    /// The footnote content
    content: NoteContent,
    /// Source info for the footnote
    source_info: SourceInfo,
}

/// State for collecting footnotes during AST traversal.
struct FootnoteCollector {
    /// Pre-defined note definitions (from [^id]: syntax)
    definitions: HashMap<String, NoteContent>,
    /// Collected footnotes in order of appearance
    footnotes: Vec<CollectedFootnote>,
    /// Counter for auto-generated footnote IDs
    counter: usize,
    /// Whether we're in margin mode (affects ref class)
    is_margin: bool,
}

impl FootnoteCollector {
    fn new(definitions: HashMap<String, NoteContent>, is_margin: bool) -> Self {
        Self {
            definitions,
            footnotes: Vec::new(),
            counter: 0,
            is_margin,
        }
    }

    /// Add an inline note and return its assigned number.
    fn add_inline_note(&mut self, content: Blocks, source_info: SourceInfo) -> usize {
        self.counter += 1;
        let number = self.counter;
        let id = number.to_string();

        self.footnotes.push(CollectedFootnote {
            id,
            number,
            content: NoteContent::Blocks(content),
            source_info,
        });

        number
    }

    /// Resolve a note reference and return its number, or None if not found.
    fn resolve_reference(&mut self, ref_id: &str, source_info: SourceInfo) -> Option<usize> {
        // Check if we've already resolved this reference
        for footnote in &self.footnotes {
            if footnote.id == ref_id {
                return Some(footnote.number);
            }
        }

        // Look up in definitions
        if let Some(content) = self.definitions.remove(ref_id) {
            self.counter += 1;
            let number = self.counter;

            self.footnotes.push(CollectedFootnote {
                id: ref_id.to_string(),
                number,
                content,
                source_info,
            });

            Some(number)
        } else {
            // Reference to undefined note - leave as-is (will produce broken link)
            // TODO: Consider emitting a warning
            None
        }
    }
}

/// Collect note definitions from blocks, removing them from the AST.
fn collect_note_definitions(
    blocks: &mut Vec<Block>,
    definitions: &mut HashMap<String, NoteContent>,
) {
    blocks.retain_mut(|block| {
        match block {
            Block::NoteDefinitionPara(def) => {
                definitions.insert(
                    def.id.clone(),
                    NoteContent::Inlines(std::mem::take(&mut def.content)),
                );
                false // Remove from AST
            }
            Block::NoteDefinitionFencedBlock(def) => {
                definitions.insert(
                    def.id.clone(),
                    NoteContent::Blocks(std::mem::take(&mut def.content)),
                );
                false // Remove from AST
            }
            // Recursively process nested blocks
            Block::BlockQuote(bq) => {
                collect_note_definitions(&mut bq.content, definitions);
                true
            }
            Block::OrderedList(ol) => {
                for item in &mut ol.content {
                    collect_note_definitions(item, definitions);
                }
                true
            }
            Block::BulletList(bl) => {
                for item in &mut bl.content {
                    collect_note_definitions(item, definitions);
                }
                true
            }
            Block::DefinitionList(dl) => {
                for (_term, defs) in &mut dl.content {
                    for def in defs {
                        collect_note_definitions(def, definitions);
                    }
                }
                true
            }
            Block::Div(div) => {
                collect_note_definitions(&mut div.content, definitions);
                true
            }
            Block::Figure(fig) => {
                collect_note_definitions(&mut fig.content, definitions);
                true
            }
            _ => true,
        }
    });
}

/// Process blocks, extracting footnotes and replacing inline notes/references.
fn process_blocks(blocks: &mut Vec<Block>, collector: &mut FootnoteCollector) {
    for block in blocks.iter_mut() {
        process_block(block, collector);
    }
}

/// Process a single block.
fn process_block(block: &mut Block, collector: &mut FootnoteCollector) {
    match block {
        Block::Paragraph(para) => {
            process_inlines(&mut para.content, collector);
        }
        Block::Plain(plain) => {
            process_inlines(&mut plain.content, collector);
        }
        Block::Header(header) => {
            process_inlines(&mut header.content, collector);
        }
        Block::BlockQuote(bq) => {
            process_blocks(&mut bq.content, collector);
        }
        Block::OrderedList(ol) => {
            for item in &mut ol.content {
                process_blocks(item, collector);
            }
        }
        Block::BulletList(bl) => {
            for item in &mut bl.content {
                process_blocks(item, collector);
            }
        }
        Block::DefinitionList(dl) => {
            for (term, defs) in &mut dl.content {
                process_inlines(term, collector);
                for def in defs {
                    process_blocks(def, collector);
                }
            }
        }
        Block::Div(div) => {
            process_blocks(&mut div.content, collector);
        }
        Block::Figure(fig) => {
            process_blocks(&mut fig.content, collector);
            // Caption has short: Option<Inlines> and long: Option<Blocks>
            if let Some(ref mut blocks) = fig.caption.long {
                process_blocks(blocks, collector);
            }
        }
        Block::Table(table) => {
            // Process table caption
            // Caption has short: Option<Inlines> and long: Option<Blocks>
            if let Some(ref mut blocks) = table.caption.long {
                process_blocks(blocks, collector);
            }
            // Process table cells
            for body in &mut table.bodies {
                for row in &mut body.body {
                    for cell in &mut row.cells {
                        process_blocks(&mut cell.content, collector);
                    }
                }
            }
            for row in &mut table.head.rows {
                for cell in &mut row.cells {
                    process_blocks(&mut cell.content, collector);
                }
            }
            for row in &mut table.foot.rows {
                for cell in &mut row.cells {
                    process_blocks(&mut cell.content, collector);
                }
            }
        }
        _ => {}
    }
}

/// Process inlines, replacing Note and NoteReference with superscript links.
fn process_inlines(inlines: &mut Vec<Inline>, collector: &mut FootnoteCollector) {
    for inline in inlines.iter_mut() {
        process_inline(inline, collector);
    }
}

/// Process a single inline, potentially replacing it.
fn process_inline(inline: &mut Inline, collector: &mut FootnoteCollector) {
    match inline {
        Inline::Note(note) => {
            let source_info = note.source_info.clone();
            let content = std::mem::take(&mut note.content);
            let number = collector.add_inline_note(content, source_info.clone());

            // Replace with superscript reference
            *inline = create_footnote_ref(number, &source_info, collector.is_margin);
        }
        Inline::NoteReference(note_ref) => {
            let source_info = note_ref.source_info.clone();
            if let Some(number) = collector.resolve_reference(&note_ref.id, source_info.clone()) {
                *inline = create_footnote_ref(number, &source_info, collector.is_margin);
            }
            // If not resolved, leave as-is (broken reference)
        }
        // Recursively process inlines that contain other inlines
        Inline::Emph(emph) => {
            process_inlines(&mut emph.content, collector);
        }
        Inline::Strong(strong) => {
            process_inlines(&mut strong.content, collector);
        }
        Inline::Strikeout(s) => {
            process_inlines(&mut s.content, collector);
        }
        Inline::Superscript(sup) => {
            process_inlines(&mut sup.content, collector);
        }
        Inline::Subscript(sub) => {
            process_inlines(&mut sub.content, collector);
        }
        Inline::SmallCaps(sc) => {
            process_inlines(&mut sc.content, collector);
        }
        Inline::Quoted(q) => {
            process_inlines(&mut q.content, collector);
        }
        Inline::Cite(cite) => {
            process_inlines(&mut cite.content, collector);
        }
        Inline::Link(link) => {
            process_inlines(&mut link.content, collector);
        }
        Inline::Span(span) => {
            process_inlines(&mut span.content, collector);
        }
        Inline::Underline(u) => {
            process_inlines(&mut u.content, collector);
        }
        Inline::Delete(d) => {
            process_inlines(&mut d.content, collector);
        }
        Inline::Insert(i) => {
            process_inlines(&mut i.content, collector);
        }
        Inline::Highlight(h) => {
            process_inlines(&mut h.content, collector);
        }
        _ => {}
    }
}

/// Create a footnote reference inline (superscript link).
///
/// Produces: `<span id="fnref{N}"><sup><a href="#fn{N}" class="footnote-ref" role="doc-noteref">{N}</a></sup></span>`
///
/// When `is_margin` is true, adds "margin-note" class to the outer span.
fn create_footnote_ref(number: usize, source_info: &SourceInfo, is_margin: bool) -> Inline {
    let fn_id = format!("fn{}", number);
    let fnref_id = format!("fnref{}", number);

    // The link inside the superscript
    // Target is a tuple: (url, title)
    let link = Inline::Link(Link {
        attr: (
            String::new(),
            vec!["footnote-ref".to_string()],
            LinkedHashMap::from_iter([("role".to_string(), "doc-noteref".to_string())]),
        ),
        content: vec![Inline::Str(Str {
            text: number.to_string(),
            source_info: source_info.clone(),
        })],
        target: (format!("#{}", fn_id), String::new()),
        source_info: source_info.clone(),
        attr_source: AttrSourceInfo::empty(),
        target_source: quarto_pandoc_types::attr::TargetSourceInfo::empty(),
    });

    // Build the class list for the outer span
    let classes = if is_margin {
        vec!["margin-note".to_string()]
    } else {
        Vec::new()
    };

    // Wrap in a Span with the fnref ID, then in Superscript
    // Actually, Pandoc puts the ID on the superscript, but we don't have that field.
    // Let's use a Span wrapper.
    Inline::Span(Span {
        attr: (fnref_id, classes, LinkedHashMap::new()),
        content: vec![Inline::Superscript(Superscript {
            content: vec![link],
            source_info: source_info.clone(),
        })],
        source_info: source_info.clone(),
        attr_source: AttrSourceInfo::empty(),
    })
}

/// Create the footnotes section block.
///
/// Produces:
/// ```html
/// <section id="footnotes" class="footnotes" role="doc-endnotes">
///   <hr>
///   <ol>
///     <li id="fn1"><p>Content<a href="#fnref1" class="footnote-back" role="doc-backlink">↩︎</a></p></li>
///   </ol>
/// </section>
/// ```
fn create_footnotes_section(footnotes: &[CollectedFootnote]) -> Block {
    let source_info = SourceInfo::default();

    // Create list items for each footnote
    let list_items: Vec<Blocks> = footnotes
        .iter()
        .map(|footnote| create_footnote_item(footnote))
        .collect();

    // Create the ordered list
    let ordered_list = Block::OrderedList(OrderedList {
        attr: (1, ListNumberStyle::Decimal, ListNumberDelim::Period),
        content: list_items,
        source_info: source_info.clone(),
    });

    // Wrap in a section Div with appropriate attributes
    // Note: We use a Div with class "section" so the HTML writer emits <section>
    Block::Div(Div {
        attr: (
            "footnotes".to_string(),
            vec!["footnotes".to_string(), "section".to_string()],
            LinkedHashMap::from_iter([("role".to_string(), "doc-endnotes".to_string())]),
        ),
        content: vec![
            Block::HorizontalRule(quarto_pandoc_types::block::HorizontalRule {
                source_info: source_info.clone(),
            }),
            ordered_list,
        ],
        source_info,
        attr_source: AttrSourceInfo::empty(),
    })
}

/// Create a single footnote list item.
fn create_footnote_item(footnote: &CollectedFootnote) -> Blocks {
    let source_info = &footnote.source_info;
    let fn_id = format!("fn{}", footnote.number);
    let fnref_id = format!("fnref{}", footnote.number);

    // Create the backlink
    // Target is a tuple: (url, title)
    let backlink = Inline::Link(Link {
        attr: (
            String::new(),
            vec!["footnote-back".to_string()],
            LinkedHashMap::from_iter([("role".to_string(), "doc-backlink".to_string())]),
        ),
        content: vec![Inline::Str(Str {
            text: "↩︎".to_string(),
            source_info: source_info.clone(),
        })],
        target: (format!("#{}", fnref_id), String::new()),
        source_info: source_info.clone(),
        attr_source: AttrSourceInfo::empty(),
        target_source: quarto_pandoc_types::attr::TargetSourceInfo::empty(),
    });

    // Convert content to blocks and append backlink to last paragraph
    let mut content_blocks = match &footnote.content {
        NoteContent::Inlines(inlines) => {
            vec![Block::Paragraph(Paragraph {
                content: inlines.clone(),
                source_info: source_info.clone(),
            })]
        }
        NoteContent::Blocks(blocks) => blocks.clone(),
    };

    // Append backlink to the last paragraph (or create one)
    if let Some(last_block) = content_blocks.last_mut() {
        match last_block {
            Block::Paragraph(para) => {
                para.content.push(backlink);
            }
            Block::Plain(plain) => {
                plain.content.push(backlink);
            }
            _ => {
                // Append a new paragraph with just the backlink
                content_blocks.push(Block::Paragraph(Paragraph {
                    content: vec![backlink],
                    source_info: source_info.clone(),
                }));
            }
        }
    } else {
        // Empty content, create paragraph with just backlink
        content_blocks.push(Block::Paragraph(Paragraph {
            content: vec![backlink],
            source_info: source_info.clone(),
        }));
    }

    // Wrap in a Div with the footnote ID
    // Note: In Pandoc's output, each <li> has the ID directly, but we can't do that
    // with OrderedList. So we wrap content in a Div with ID.
    // Actually, looking at Pandoc output more carefully, the ID is on the <li>.
    // Our OrderedList doesn't support per-item IDs, so we'll wrap in a Div.
    vec![Block::Div(Div {
        attr: (fn_id, Vec::new(), LinkedHashMap::new()),
        content: content_blocks,
        source_info: source_info.clone(),
        attr_source: AttrSourceInfo::empty(),
    })]
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_pandoc_types::NoteDefinitionPara;
    use quarto_pandoc_types::block::Plain;
    use quarto_pandoc_types::inline::Note;
    use quarto_source_map::{FileId, Location, Range};

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

    fn make_str(text: &str) -> Inline {
        Inline::Str(Str {
            text: text.to_string(),
            source_info: dummy_source_info(),
        })
    }

    #[test]
    fn test_transform_name() {
        let transform = FootnotesTransform::new();
        assert_eq!(transform.name(), "footnotes");
    }

    #[test]
    fn test_single_inline_note() {
        let mut ast = Pandoc {
            meta: quarto_pandoc_types::ConfigValue::default(),
            blocks: vec![Block::Paragraph(Paragraph {
                content: vec![
                    make_str("Text with"),
                    Inline::Note(Note {
                        content: vec![Block::Paragraph(Paragraph {
                            content: vec![make_str("footnote content")],
                            source_info: dummy_source_info(),
                        })],
                        source_info: dummy_source_info(),
                    }),
                    make_str(" more text."),
                ],
                source_info: dummy_source_info(),
            })],
        };

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

        let transform = FootnotesTransform::new();
        transform.transform(&mut ast, &mut ctx).unwrap();

        // Should have original paragraph + footnotes section
        assert_eq!(ast.blocks.len(), 2);

        // Check the inline note was replaced with a reference
        if let Block::Paragraph(para) = &ast.blocks[0] {
            // Should have: "Text with" + span(sup(link)) + " more text."
            assert_eq!(para.content.len(), 3);

            // The middle element should be a Span containing Superscript
            match &para.content[1] {
                Inline::Span(span) => {
                    assert_eq!(span.attr.0, "fnref1");
                    assert_eq!(span.content.len(), 1);
                    match &span.content[0] {
                        Inline::Superscript(sup) => {
                            assert_eq!(sup.content.len(), 1);
                            match &sup.content[0] {
                                Inline::Link(link) => {
                                    // Target is a tuple: (url, title)
                                    assert_eq!(link.target.0, "#fn1");
                                    assert!(link.attr.1.contains(&"footnote-ref".to_string()));
                                }
                                _ => panic!("Expected Link inside Superscript"),
                            }
                        }
                        _ => panic!("Expected Superscript inside Span"),
                    }
                }
                _ => panic!("Expected Span for footnote reference"),
            }
        } else {
            panic!("Expected Paragraph");
        }

        // Check footnotes section
        if let Block::Div(div) = &ast.blocks[1] {
            assert_eq!(div.attr.0, "footnotes");
            assert!(div.attr.1.contains(&"footnotes".to_string()));
            assert!(div.attr.1.contains(&"section".to_string()));
        } else {
            panic!("Expected footnotes Div");
        }
    }

    #[test]
    fn test_multiple_notes() {
        let mut ast = Pandoc {
            meta: quarto_pandoc_types::ConfigValue::default(),
            blocks: vec![Block::Paragraph(Paragraph {
                content: vec![
                    make_str("First"),
                    Inline::Note(Note {
                        content: vec![Block::Plain(Plain {
                            content: vec![make_str("note 1")],
                            source_info: dummy_source_info(),
                        })],
                        source_info: dummy_source_info(),
                    }),
                    make_str(" and second"),
                    Inline::Note(Note {
                        content: vec![Block::Plain(Plain {
                            content: vec![make_str("note 2")],
                            source_info: dummy_source_info(),
                        })],
                        source_info: dummy_source_info(),
                    }),
                ],
                source_info: dummy_source_info(),
            })],
        };

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

        let transform = FootnotesTransform::new();
        transform.transform(&mut ast, &mut ctx).unwrap();

        // Should have 2 footnotes in the section
        if let Block::Div(div) = &ast.blocks[1] {
            // div.content should be [HorizontalRule, OrderedList]
            if let Block::OrderedList(ol) = &div.content[1] {
                assert_eq!(ol.content.len(), 2);
            } else {
                panic!("Expected OrderedList in footnotes section");
            }
        }
    }

    #[test]
    fn test_note_definition_and_reference() {
        let mut ast = Pandoc {
            meta: quarto_pandoc_types::ConfigValue::default(),
            blocks: vec![
                // Note definition
                Block::NoteDefinitionPara(NoteDefinitionPara {
                    id: "myfoot".to_string(),
                    content: vec![make_str("Defined footnote content")],
                    source_info: dummy_source_info(),
                }),
                // Paragraph with reference
                Block::Paragraph(Paragraph {
                    content: vec![
                        make_str("See note"),
                        Inline::NoteReference(quarto_pandoc_types::inline::NoteReference {
                            id: "myfoot".to_string(),
                            source_info: dummy_source_info(),
                        }),
                    ],
                    source_info: dummy_source_info(),
                }),
            ],
        };

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

        let transform = FootnotesTransform::new();
        transform.transform(&mut ast, &mut ctx).unwrap();

        // Note definition should be removed, leaving paragraph + footnotes section
        assert_eq!(ast.blocks.len(), 2);

        // First block should be the paragraph (not the definition)
        assert!(matches!(ast.blocks[0], Block::Paragraph(_)));

        // Footnotes section should exist
        assert!(matches!(ast.blocks[1], Block::Div(_)));
    }

    #[test]
    fn test_no_footnotes() {
        let mut ast = Pandoc {
            meta: quarto_pandoc_types::ConfigValue::default(),
            blocks: vec![Block::Paragraph(Paragraph {
                content: vec![make_str("Just plain text.")],
                source_info: dummy_source_info(),
            })],
        };

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

        let transform = FootnotesTransform::new();
        transform.transform(&mut ast, &mut ctx).unwrap();

        // Should not add footnotes section
        assert_eq!(ast.blocks.len(), 1);
    }

    #[test]
    fn test_reference_location_parsing() {
        assert_eq!(
            ReferenceLocation::from_str("document"),
            ReferenceLocation::Document
        );
        assert_eq!(
            ReferenceLocation::from_str("Document"),
            ReferenceLocation::Document
        );
        assert_eq!(
            ReferenceLocation::from_str("section"),
            ReferenceLocation::Section
        );
        assert_eq!(
            ReferenceLocation::from_str("block"),
            ReferenceLocation::Block
        );
        assert_eq!(
            ReferenceLocation::from_str("margin"),
            ReferenceLocation::Margin
        );
        assert_eq!(
            ReferenceLocation::from_str("unknown"),
            ReferenceLocation::Document
        );
    }

    #[test]
    fn test_nested_note_in_emphasis() {
        let mut ast = Pandoc {
            meta: quarto_pandoc_types::ConfigValue::default(),
            blocks: vec![Block::Paragraph(Paragraph {
                content: vec![Inline::Emph(quarto_pandoc_types::inline::Emph {
                    content: vec![
                        make_str("emphasized"),
                        Inline::Note(Note {
                            content: vec![Block::Plain(Plain {
                                content: vec![make_str("nested note")],
                                source_info: dummy_source_info(),
                            })],
                            source_info: dummy_source_info(),
                        }),
                    ],
                    source_info: dummy_source_info(),
                })],
                source_info: dummy_source_info(),
            })],
        };

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

        let transform = FootnotesTransform::new();
        transform.transform(&mut ast, &mut ctx).unwrap();

        // Should have paragraph + footnotes section
        assert_eq!(ast.blocks.len(), 2);
    }

    #[test]
    fn test_margin_mode_no_section() {
        let mut ast = Pandoc {
            meta: quarto_pandoc_types::ConfigValue::default(),
            blocks: vec![Block::Paragraph(Paragraph {
                content: vec![
                    make_str("Text with"),
                    Inline::Note(Note {
                        content: vec![Block::Plain(Plain {
                            content: vec![make_str("margin note content")],
                            source_info: dummy_source_info(),
                        })],
                        source_info: dummy_source_info(),
                    }),
                ],
                source_info: dummy_source_info(),
            })],
        };

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        // Set reference-location: margin in format metadata
        let format = Format::html().with_metadata(serde_json::json!({
            "reference-location": "margin"
        }));
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

        let transform = FootnotesTransform::new();
        transform.transform(&mut ast, &mut ctx).unwrap();

        // Should only have the paragraph - NO footnotes section
        assert_eq!(ast.blocks.len(), 1);

        // Check that the footnote ref has "margin-note" class
        if let Block::Paragraph(para) = &ast.blocks[0] {
            match &para.content[1] {
                Inline::Span(span) => {
                    assert!(
                        span.attr.1.contains(&"margin-note".to_string()),
                        "Expected margin-note class on footnote ref"
                    );
                }
                _ => panic!("Expected Span for footnote reference"),
            }
        } else {
            panic!("Expected Paragraph");
        }
    }

    #[test]
    fn test_block_section_modes_are_noop() {
        // For block and section modes, the transform should be a no-op
        // (Pandoc handles these during rendering)

        let note = Inline::Note(Note {
            content: vec![Block::Plain(Plain {
                content: vec![make_str("note content")],
                source_info: dummy_source_info(),
            })],
            source_info: dummy_source_info(),
        });

        for mode in ["block", "section"] {
            let mut ast = Pandoc {
                meta: quarto_pandoc_types::ConfigValue::default(),
                blocks: vec![Block::Paragraph(Paragraph {
                    content: vec![make_str("Text"), note.clone()],
                    source_info: dummy_source_info(),
                })],
            };

            let project = make_test_project();
            let doc = DocumentInfo::from_path("/project/doc.qmd");
            let format = Format::html().with_metadata(serde_json::json!({
                "reference-location": mode
            }));
            let binaries = BinaryDependencies::new();
            let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

            let transform = FootnotesTransform::new();
            transform.transform(&mut ast, &mut ctx).unwrap();

            // Should be unchanged - still have the Note inline
            assert_eq!(ast.blocks.len(), 1);
            if let Block::Paragraph(para) = &ast.blocks[0] {
                assert!(
                    matches!(&para.content[1], Inline::Note(_)),
                    "Note should be unchanged for mode: {}",
                    mode
                );
            }
        }
    }

    #[test]
    fn test_document_mode_creates_section() {
        let mut ast = Pandoc {
            meta: quarto_pandoc_types::ConfigValue::default(),
            blocks: vec![Block::Paragraph(Paragraph {
                content: vec![
                    make_str("Text"),
                    Inline::Note(Note {
                        content: vec![Block::Plain(Plain {
                            content: vec![make_str("note content")],
                            source_info: dummy_source_info(),
                        })],
                        source_info: dummy_source_info(),
                    }),
                ],
                source_info: dummy_source_info(),
            })],
        };

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        // Explicitly set document mode (default)
        let format = Format::html().with_metadata(serde_json::json!({
            "reference-location": "document"
        }));
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

        let transform = FootnotesTransform::new();
        transform.transform(&mut ast, &mut ctx).unwrap();

        // Should have paragraph + footnotes section
        assert_eq!(ast.blocks.len(), 2);

        // Check footnote ref does NOT have margin-note class
        if let Block::Paragraph(para) = &ast.blocks[0] {
            match &para.content[1] {
                Inline::Span(span) => {
                    assert!(
                        !span.attr.1.contains(&"margin-note".to_string()),
                        "Document mode should not have margin-note class"
                    );
                }
                _ => panic!("Expected Span for footnote reference"),
            }
        }

        // Check footnotes section exists
        assert!(matches!(ast.blocks[1], Block::Div(_)));
    }
}
