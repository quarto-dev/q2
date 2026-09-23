/*
 * footnotes_resolve.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Transform that resolves native Pandoc `Note` inlines into HTML footnote
 * chrome.
 */

//! Footnotes HTML-chrome transform (the "B2/B4" half).
//!
//! This is the presentation half of the footnotes split (see
//! `claude-notes/designs/pandoc-hybrid-architecture.md` §6 and
//! [`crate::transforms::FootnotesTransform`], its semantic sibling). It
//! consumes the native `Inline::Note`s [`crate::transforms::FootnotesTransform`]
//! produces (or that the parser produced directly for `^[...]` inline notes)
//! and converts them into the HTML-specific chrome: a `Span#fnrefN`
//! superscript link at each reference site, plus a trailing
//! `Div#footnotes` section with backlinks.
//!
//! It is registered immediately after `FootnotesTransform` and is excluded
//! for `Pandoc(_)` output profiles (docx, pptx, …) — a Pandoc writer has no
//! HTML chrome to consume; it numbers and places `Note`s itself.
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
use quarto_pandoc_types::{Blocks, ListNumberDelim, ListNumberStyle};
use quarto_source_map::{By, SourceInfo};

use quarto_pandoc_types::ConfigValue;

use crate::Result;
use crate::render::RenderContext;
use crate::transform::{AstTransform, TransformPhase};
use crate::transforms::{FOOTNOTE_REF_ID_MARKER_FORMAT, ReferenceLocation};

/// Transform that resolves native `Inline::Note`s into HTML footnote chrome.
///
/// This transform is part of the **normalization phase** (not
/// `Finalization`): it consumes `Inline::Note`, which its sibling
/// [`crate::transforms::FootnotesTransform`] produces one step earlier in
/// the same phase — not crossref-resolved structure (float, caption,
/// number, or resolved `@ref`), which is what would require
/// `Finalization`.
pub struct FootnotesResolveTransform;

impl FootnotesResolveTransform {
    /// Create a new footnotes-resolve transform.
    pub fn new() -> Self {
        Self
    }

    /// Get the reference-location configuration from merged metadata.
    fn get_reference_location(meta: &ConfigValue) -> ReferenceLocation {
        // Use `as_plain_text` (not `as_str`): in document-metadata context a
        // bare YAML string value is parsed as markdown and stored as
        // `ConfigValueKind::PandocInlines`, for which `as_str` returns `None`.
        // `as_plain_text` handles both the inline and scalar forms. (bd-9ez3ngt1)
        meta.get("reference-location")
            .and_then(|v| v.as_plain_text())
            .map(|s| ReferenceLocation::from_str(&s))
            .unwrap_or_default()
    }
}

impl Default for FootnotesResolveTransform {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait(?Send)]
impl AstTransform for FootnotesResolveTransform {
    fn name(&self) -> &str {
        "footnotes-resolve"
    }

    fn phase(&self) -> TransformPhase {
        TransformPhase::Normalization
    }

    async fn transform(&self, ast: &mut Pandoc, _ctx: &mut RenderContext) -> Result<()> {
        let reference_location = Self::get_reference_location(&ast.meta);

        // For block/section placement, Pandoc handles this - no-op. This
        // gate is deliberately duplicated from `FootnotesTransform` (H5c) —
        // see that transform's identical check for the rationale.
        if matches!(
            reference_location,
            ReferenceLocation::Block | ReferenceLocation::Section
        ) {
            return Ok(());
        }

        let mut collector = FootnoteCollector::new(reference_location == ReferenceLocation::Margin);
        process_blocks(&mut ast.blocks, &mut collector);

        // Create footnotes section only for document location
        if reference_location == ReferenceLocation::Document && !collector.footnotes.is_empty() {
            let footnotes_section = create_footnotes_section(&collector.footnotes);
            ast.blocks.push(footnotes_section);
        }

        // For margin location:
        // - Footnote references are created with "margin-note" class
        // - No footnotes section is created
        // - Full margin content placement is handled by CSS/layout or future enhancement

        Ok(())
    }
}

/// Collected footnote with its number and content, ready for chrome rendering.
#[derive(Debug, Clone)]
struct CollectedFootnote {
    /// The footnote number (1-based, for display)
    number: usize,
    /// The footnote's block content
    content: Blocks,
    /// Source info for the footnote (the `Inline::Note`'s own source_info)
    source_info: SourceInfo,
}

/// State for collecting footnotes during AST traversal.
struct FootnoteCollector {
    /// Collected footnotes in order of appearance
    footnotes: Vec<CollectedFootnote>,
    /// Counter for auto-generated footnote numbers
    counter: usize,
    /// Whether we're in margin mode (affects ref class)
    is_margin: bool,
    /// Maps a named reference id (from the marker `FootnotesTransform`
    /// prepends to resolved-reference content, see
    /// [`extract_and_strip_ref_id`]) to the footnote number already assigned
    /// to it — so a second `[^a]` reuses the first's number/entry instead of
    /// creating a duplicate, matching the pre-split transform's
    /// `resolve_reference` dedup scan.
    resolved_refs: HashMap<String, usize>,
}

impl FootnoteCollector {
    fn new(is_margin: bool) -> Self {
        Self {
            footnotes: Vec::new(),
            counter: 0,
            is_margin,
            resolved_refs: HashMap::new(),
        }
    }

    /// Add a resolved note and return its assigned number.
    fn add_note(&mut self, content: Blocks, source_info: SourceInfo) -> usize {
        self.counter += 1;
        let number = self.counter;

        self.footnotes.push(CollectedFootnote {
            number,
            content,
            source_info,
        });

        number
    }

    /// Resolve a `Note`'s assigned footnote number, given the named
    /// reference id extracted from its content's marker (if any). Repeated
    /// references to the same id reuse the first's number and do **not**
    /// add a new entry to `self.footnotes` — the content passed for a
    /// repeat is simply dropped, exactly as the pre-split transform dropped
    /// a repeat's content in `resolve_reference`.
    fn resolve_or_add(
        &mut self,
        ref_id: Option<String>,
        content: Blocks,
        source_info: SourceInfo,
    ) -> usize {
        match ref_id {
            Some(id) => {
                if let Some(&number) = self.resolved_refs.get(&id) {
                    number
                } else {
                    let number = self.add_note(content, source_info);
                    self.resolved_refs.insert(id, number);
                    number
                }
            }
            None => self.add_note(content, source_info),
        }
    }
}

/// Extract the marker `FootnotesTransform` prepends to a resolved named
/// reference's content (see `mark_with_ref_id` in `footnotes.rs`), returning
/// the original `[^id]` string and removing the marker block from `content`
/// in place. Returns `None` for an inline `^[...]` note, which carries no
/// marker and is never deduped.
fn extract_and_strip_ref_id(content: &mut Blocks) -> Option<String> {
    let is_marker = matches!(
        content.first(),
        Some(Block::RawBlock(rb)) if rb.format == FOOTNOTE_REF_ID_MARKER_FORMAT
    );
    if !is_marker {
        return None;
    }
    let Block::RawBlock(rb) = content.remove(0) else {
        unreachable!("checked above");
    };
    Some(rb.text)
}

/// Process blocks, replacing native notes with superscript links.
fn process_blocks(blocks: &mut [Block], collector: &mut FootnoteCollector) {
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

/// Process inlines, replacing `Inline::Note` with superscript links.
fn process_inlines(inlines: &mut [Inline], collector: &mut FootnoteCollector) {
    for inline in inlines.iter_mut() {
        process_inline(inline, collector);
    }
}

/// Process a single inline, potentially replacing it.
fn process_inline(inline: &mut Inline, collector: &mut FootnoteCollector) {
    match inline {
        Inline::Note(note) => {
            let source_info = note.source_info.clone();
            let mut content = std::mem::take(&mut note.content);
            let ref_id = extract_and_strip_ref_id(&mut content);
            let number = collector.resolve_or_add(ref_id, content, source_info.clone());

            // Replace with superscript reference. Note: a repeat reference
            // still gets its own span here (with the SAME number/fnref id
            // as the first occurrence) — byte-identical to the pre-split
            // transform, which also called `create_footnote_ref` at every
            // reference site regardless of dedup.
            *inline = create_footnote_ref(number, &source_info, collector.is_margin);
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
    // The synthesized container chrome (section Div, embedded <hr>, and the
    // OrderedList wrapping the footnote items) is pure synthesis: it
    // corresponds to no source bytes. The footnote content inside (created
    // by `create_footnote_item`) retains the original Note's source_info.
    let source_info = SourceInfo::generated(By::footnotes());

    // Create list items for each footnote
    let list_items: Vec<Blocks> = footnotes.iter().map(create_footnote_item).collect();

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
    let mut content_blocks = footnote.content.clone();

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
    use quarto_pandoc_types::inline::Note;
    use quarto_source_map::{FileId, Location, Range};

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

    fn make_str(text: &str) -> Inline {
        Inline::Str(Str {
            text: text.to_string(),
            source_info: dummy_source_info(),
        })
    }

    #[tokio::test]
    async fn test_transform_name() {
        let transform = FootnotesResolveTransform::new();
        assert_eq!(transform.name(), "footnotes-resolve");
    }

    /// T5.2: exact fnref shape from a bare `Inline::Note` input, i.e. this
    /// half's own responsibility in isolation (no B1 involved).
    #[tokio::test]
    async fn test_note_resolves_to_exact_fnref_shape() {
        use crate::format::Format;
        use crate::project::{DocumentInfo, ProjectConfig, ProjectContext};
        use crate::render::BinaryDependencies;

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
                ],
                source_info: dummy_source_info(),
            })],
        };

        let project = ProjectContext {
            dir: std::path::PathBuf::from("/project"),
            config: ProjectConfig::default(),
            is_single_file: true,
            files: vec![DocumentInfo::from_path("/project/doc.qmd")],
            output_dir: std::path::PathBuf::from("/project"),
            ..Default::default()
        };
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

        FootnotesResolveTransform::new()
            .transform(&mut ast, &mut ctx)
            .await
            .unwrap();

        assert_eq!(ast.blocks.len(), 2);

        let Block::Paragraph(para) = &ast.blocks[0] else {
            panic!("expected Paragraph");
        };
        let Inline::Span(span) = &para.content[1] else {
            panic!(
                "expected Span for footnote reference, got {:?}",
                para.content[1]
            );
        };
        assert_eq!(span.attr.0, "fnref1");
        let Inline::Superscript(sup) = &span.content[0] else {
            panic!("expected Superscript inside Span");
        };
        let Inline::Link(link) = &sup.content[0] else {
            panic!("expected Link inside Superscript");
        };
        assert_eq!(link.target.0, "#fn1");
        assert!(link.attr.1.contains(&"footnote-ref".to_string()));

        let Block::Div(div) = &ast.blocks[1] else {
            panic!("expected footnotes Div");
        };
        assert_eq!(div.attr.0, "footnotes");
        assert_eq!(
            div.attr.1,
            vec!["footnotes".to_string(), "section".to_string()]
        );

        // The footnote item carries a footnote-back backlink.
        let Block::OrderedList(ol) = &div.content[1] else {
            panic!("expected OrderedList in footnotes section");
        };
        let Block::Div(item) = &ol.content[0][0] else {
            panic!("expected footnote item Div");
        };
        let rendered = format!("{:?}", item);
        assert!(
            rendered.contains("footnote-back"),
            "footnote item must carry a footnote-back backlink; got:\n{rendered}"
        );
    }

    #[test]
    fn test_create_footnotes_section_has_generated_provenance() {
        // Plan 6: the synthesized footnotes container Div (and its embedded
        // chrome — HorizontalRule, OrderedList) carry
        // Generated { by: footnotes(), from: [] }. The footnote *items*
        // inside retain the original Note's source_info via
        // create_footnote_item.
        let block = create_footnotes_section(&[]);
        let Block::Div(div) = &block else {
            panic!("Expected Div");
        };
        match &div.source_info {
            SourceInfo::Generated(g) => {
                let quarto_source_map::Generated { by, from } = &**g;
                assert_eq!(by.kind, "footnotes");
                assert!(from.is_empty());
            }
            other => panic!("Expected Generated, got {:?}", other),
        }
        // The embedded HorizontalRule chrome carries the same shape.
        let Block::HorizontalRule(hr) = &div.content[0] else {
            panic!("Expected HorizontalRule");
        };
        assert!(matches!(&hr.source_info, SourceInfo::Generated(g) if g.by.kind == "footnotes"));
    }

    #[tokio::test]
    async fn test_no_notes_no_section() {
        use crate::format::Format;
        use crate::project::{DocumentInfo, ProjectConfig, ProjectContext};
        use crate::render::BinaryDependencies;

        let mut ast = Pandoc {
            meta: quarto_pandoc_types::ConfigValue::default(),
            blocks: vec![Block::Paragraph(Paragraph {
                content: vec![make_str("Just plain text.")],
                source_info: dummy_source_info(),
            })],
        };

        let project = ProjectContext {
            dir: std::path::PathBuf::from("/project"),
            config: ProjectConfig::default(),
            is_single_file: true,
            files: vec![DocumentInfo::from_path("/project/doc.qmd")],
            output_dir: std::path::PathBuf::from("/project"),
            ..Default::default()
        };
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

        FootnotesResolveTransform::new()
            .transform(&mut ast, &mut ctx)
            .await
            .unwrap();

        assert_eq!(ast.blocks.len(), 1);
    }
}
