/*
 * footnotes.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Transform that resolves footnote references/definitions into native
 * Pandoc `Note` inlines.
 */

//! Footnotes resolution transform (the "B1" half).
//!
//! This is the semantic half of the footnotes split (see
//! `claude-notes/designs/pandoc-hybrid-architecture.md` §6). It collects
//! `[^id]: ...` definitions and resolves `Inline::NoteReference` (and the
//! pampa-lowered `Span.quarto-note-reference` form) into native
//! `Inline::Note` inlines — the same primitive pandoc's own markdown reader
//! produces for `^[...]` inline notes. It runs for **every** output format,
//! including `Pandoc(_)` (docx, pptx, …), so pandoc's own writers can number
//! and place footnotes themselves.
//!
//! The HTML-specific chrome (`Span#fnrefN` superscript links + the trailing
//! `Div#footnotes` section with backlinks) is a **separate** transform,
//! [`crate::transforms::FootnotesResolveTransform`] (`name() ==
//! "footnotes-resolve"`), registered immediately after this one and excluded
//! for `Pandoc(_)` profiles. The two communicate only through the
//! `Inline::Note` AST node: this transform produces it, that one consumes and
//! destroys it into HTML chrome. Under `HtmlRender`/`RevealjsRender`, running
//! both back-to-back is byte-identical to the pre-split single-transform
//! behavior.
//!
//! ## Input AST Elements
//!
//! - `Inline::Note` - Inline footnote with block content (e.g., `^[footnote text]`) — left untouched
//! - `Inline::NoteReference` - Reference to a defined note (e.g., `[^1]`) — resolved into `Inline::Note`
//! - `Block::NoteDefinitionPara` - Single-paragraph note definition — collected, removed from the AST
//! - `Block::NoteDefinitionFencedBlock` - Multi-paragraph note definition — collected, removed from the AST
//!
//! ## Configuration
//!
//! - `reference-location: block` / `section`: handled by Pandoc itself; this
//!   transform (and its `-resolve` sibling) is a no-op, under every profile.

use std::collections::HashMap;

use quarto_pandoc_types::block::{Block, Paragraph, RawBlock};
use quarto_pandoc_types::inline::{Inline, Note};
use quarto_pandoc_types::pandoc::Pandoc;
use quarto_pandoc_types::{Blocks, Inlines};
use quarto_source_map::SourceInfo;

use crate::transforms::FOOTNOTE_REF_ID_MARKER_FORMAT;

use quarto_pandoc_types::ConfigValue;

use crate::Result;
use crate::format::PipelineProfile;
use crate::render::RenderContext;
use crate::transform::{AstTransform, TransformPhase};
use crate::transforms::ReferenceLocation;

/// Transform that resolves footnote references/definitions into native
/// `Inline::Note` inlines.
///
/// This transform is part of the **normalization phase**. It runs for every
/// output format (including `Pandoc(_)`), unlike its HTML-chrome sibling
/// [`crate::transforms::FootnotesResolveTransform`].
pub struct FootnotesTransform;

impl FootnotesTransform {
    /// Create a new footnotes transform.
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

impl Default for FootnotesTransform {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait(?Send)]
impl AstTransform for FootnotesTransform {
    fn name(&self) -> &str {
        "footnotes"
    }

    fn phase(&self) -> TransformPhase {
        TransformPhase::Normalization
    }

    async fn transform(&self, ast: &mut Pandoc, ctx: &mut RenderContext) -> Result<()> {
        let reference_location = Self::get_reference_location(&ast.meta);

        // For block/section placement, Pandoc handles this - no-op. This
        // gate is deliberately duplicated in `FootnotesResolveTransform`
        // (rather than left only there): if only the resolve half kept it,
        // this half would still rewrite `NoteReference` -> `Note` under
        // `reference-location: block`, corrupting the (now-skipped) chrome
        // step's input.
        if matches!(
            reference_location,
            ReferenceLocation::Block | ReferenceLocation::Section
        ) {
            return Ok(());
        }

        // Collect all note definitions first
        let mut note_definitions: HashMap<String, NoteContent> = HashMap::new();
        collect_note_definitions(&mut ast.blocks, &mut note_definitions);

        // Resolve references (and the pampa-lowered Span form) into native
        // `Inline::Note`s. An existing `Inline::Note` (from `^[...]`
        // syntax) is left untouched.
        process_blocks(&mut ast.blocks, &note_definitions);

        // Under `Pandoc(_)` profiles, `FootnotesResolveTransform` (which
        // owns `extract_and_strip_ref_id`) never runs, so nothing else
        // strips the ref-id marker `mark_with_ref_id` just prepended to
        // every resolved named reference's content. The marker's only
        // purpose is letting that transform dedupe repeated references —
        // for the Pandoc leg it has already served no purpose, and must
        // not reach pandoc's own wire format (see
        // `FOOTNOTE_REF_ID_MARKER_FORMAT`'s doc comment).
        if matches!(ctx.pipeline_profile, PipelineProfile::Pandoc(_)) {
            strip_ref_id_markers(&mut ast.blocks);
        }

        Ok(())
    }
}

/// Content of a footnote definition (either inline content or block content).
#[derive(Debug, Clone)]
enum NoteContent {
    /// Single paragraph of inline content
    Inlines(Inlines),
    /// Multiple blocks of content
    Blocks(Blocks),
}

impl NoteContent {
    /// Convert to `Note`'s `Blocks` shape. A single-paragraph definition
    /// (`NoteDefinitionPara`) is wrapped in a `Paragraph` using the
    /// reference occurrence's `source_info` — matching what the pre-split
    /// transform's `create_footnote_item` produced for the same content.
    fn into_blocks(self, source_info: &SourceInfo) -> Blocks {
        match self {
            NoteContent::Inlines(inlines) => vec![Block::Paragraph(Paragraph {
                content: inlines,
                source_info: source_info.clone(),
            })],
            NoteContent::Blocks(blocks) => blocks,
        }
    }
}

/// Prepend a hidden marker block carrying `ref_id` to a resolved named
/// reference's content, so [`crate::transforms::FootnotesResolveTransform`]
/// can dedupe repeated references to the same id into one shared footnote
/// entry — matching the pre-split transform's `resolve_reference` behavior —
/// without a side channel: the id travels inside the `Inline::Note`'s own
/// `content`, the one thing the two halves communicate through.
///
/// Applied uniformly to every resolved named reference (first occurrence and
/// repeats alike) so `FootnotesTransform` stays stateless — all the "have I
/// seen this id before" bookkeeping lives in the resolve half, which is the
/// side of the cut that actually needs to decide whether to share an entry.
///
/// A `RawBlock` with an unrecognized format is dropped silently by every
/// pandoc writer (the same convention `ExampleEmbedRenderTransform`'s
/// `RawBlock("html", ...)` iframe relies on for non-HTML profiles), so under
/// `Pandoc(_)` profiles — where nothing ever strips this marker — it is
/// inert: pandoc's own writer ignores it and renders the real content that
/// follows.
fn mark_with_ref_id(mut blocks: Blocks, ref_id: &str, source_info: &SourceInfo) -> Blocks {
    blocks.insert(
        0,
        Block::RawBlock(RawBlock {
            format: FOOTNOTE_REF_ID_MARKER_FORMAT.to_string(),
            text: ref_id.to_string(),
            source_info: source_info.clone(),
        }),
    );
    blocks
}

/// Strip the ref-id marker block [`mark_with_ref_id`] prepends, from every
/// `Inline::Note` reachable in `blocks`. Mirrors
/// [`crate::transforms::FootnotesResolveTransform`]'s
/// `extract_and_strip_ref_id` stripping logic; used only under `Pandoc(_)`
/// profiles, where that transform never runs to do this itself.
fn strip_ref_id_markers(blocks: &mut [Block]) {
    for block in blocks.iter_mut() {
        strip_ref_id_markers_in_block(block);
    }
}

/// Single-block counterpart of [`strip_ref_id_markers`], mirroring
/// `process_block`'s traversal shape.
fn strip_ref_id_markers_in_block(block: &mut Block) {
    match block {
        Block::Paragraph(para) => strip_ref_id_markers_in_inlines(&mut para.content),
        Block::Plain(plain) => strip_ref_id_markers_in_inlines(&mut plain.content),
        Block::Header(header) => strip_ref_id_markers_in_inlines(&mut header.content),
        Block::BlockQuote(bq) => strip_ref_id_markers(&mut bq.content),
        Block::OrderedList(ol) => {
            for item in &mut ol.content {
                strip_ref_id_markers(item);
            }
        }
        Block::BulletList(bl) => {
            for item in &mut bl.content {
                strip_ref_id_markers(item);
            }
        }
        Block::DefinitionList(dl) => {
            for (term, defs) in &mut dl.content {
                strip_ref_id_markers_in_inlines(term);
                for def in defs {
                    strip_ref_id_markers(def);
                }
            }
        }
        Block::Div(div) => strip_ref_id_markers(&mut div.content),
        Block::Figure(fig) => {
            strip_ref_id_markers(&mut fig.content);
            if let Some(ref mut blocks) = fig.caption.long {
                strip_ref_id_markers(blocks);
            }
        }
        Block::Table(table) => {
            if let Some(ref mut blocks) = table.caption.long {
                strip_ref_id_markers(blocks);
            }
            for body in &mut table.bodies {
                for row in &mut body.body {
                    for cell in &mut row.cells {
                        strip_ref_id_markers(&mut cell.content);
                    }
                }
            }
            for row in &mut table.head.rows {
                for cell in &mut row.cells {
                    strip_ref_id_markers(&mut cell.content);
                }
            }
            for row in &mut table.foot.rows {
                for cell in &mut row.cells {
                    strip_ref_id_markers(&mut cell.content);
                }
            }
        }
        _ => {}
    }
}

/// Inlines counterpart of [`strip_ref_id_markers`], mirroring
/// `process_inlines`'s traversal shape.
fn strip_ref_id_markers_in_inlines(inlines: &mut [Inline]) {
    for inline in inlines.iter_mut() {
        strip_ref_id_markers_in_inline(inline);
    }
}

/// Single-inline counterpart of [`strip_ref_id_markers`], mirroring
/// `process_inline`'s traversal shape. Does **not** recurse into an
/// `Inline::Note`'s own content — matching `process_inline`'s existing
/// behavior of never recursing into a `Note`'s content — since the marker,
/// when present, is always the note's own first content block.
fn strip_ref_id_markers_in_inline(inline: &mut Inline) {
    match inline {
        Inline::Note(note) => {
            let is_marker = matches!(
                note.content.first(),
                Some(Block::RawBlock(rb)) if rb.format == FOOTNOTE_REF_ID_MARKER_FORMAT
            );
            if is_marker {
                note.content.remove(0);
            }
        }
        Inline::Emph(emph) => strip_ref_id_markers_in_inlines(&mut emph.content),
        Inline::Strong(strong) => strip_ref_id_markers_in_inlines(&mut strong.content),
        Inline::Strikeout(s) => strip_ref_id_markers_in_inlines(&mut s.content),
        Inline::Superscript(sup) => strip_ref_id_markers_in_inlines(&mut sup.content),
        Inline::Subscript(sub) => strip_ref_id_markers_in_inlines(&mut sub.content),
        Inline::SmallCaps(sc) => strip_ref_id_markers_in_inlines(&mut sc.content),
        Inline::Quoted(q) => strip_ref_id_markers_in_inlines(&mut q.content),
        Inline::Cite(cite) => strip_ref_id_markers_in_inlines(&mut cite.content),
        Inline::Link(link) => strip_ref_id_markers_in_inlines(&mut link.content),
        Inline::Span(span) => strip_ref_id_markers_in_inlines(&mut span.content),
        Inline::Underline(u) => strip_ref_id_markers_in_inlines(&mut u.content),
        Inline::Delete(d) => strip_ref_id_markers_in_inlines(&mut d.content),
        Inline::Insert(i) => strip_ref_id_markers_in_inlines(&mut i.content),
        Inline::Highlight(h) => strip_ref_id_markers_in_inlines(&mut h.content),
        _ => {}
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

/// Process blocks, resolving note references into native `Inline::Note`s.
fn process_blocks(blocks: &mut [Block], definitions: &HashMap<String, NoteContent>) {
    for block in blocks.iter_mut() {
        process_block(block, definitions);
    }
}

/// Process a single block.
fn process_block(block: &mut Block, definitions: &HashMap<String, NoteContent>) {
    match block {
        Block::Paragraph(para) => {
            process_inlines(&mut para.content, definitions);
        }
        Block::Plain(plain) => {
            process_inlines(&mut plain.content, definitions);
        }
        Block::Header(header) => {
            process_inlines(&mut header.content, definitions);
        }
        Block::BlockQuote(bq) => {
            process_blocks(&mut bq.content, definitions);
        }
        Block::OrderedList(ol) => {
            for item in &mut ol.content {
                process_blocks(item, definitions);
            }
        }
        Block::BulletList(bl) => {
            for item in &mut bl.content {
                process_blocks(item, definitions);
            }
        }
        Block::DefinitionList(dl) => {
            for (term, defs) in &mut dl.content {
                process_inlines(term, definitions);
                for def in defs {
                    process_blocks(def, definitions);
                }
            }
        }
        Block::Div(div) => {
            process_blocks(&mut div.content, definitions);
        }
        Block::Figure(fig) => {
            process_blocks(&mut fig.content, definitions);
            // Caption has short: Option<Inlines> and long: Option<Blocks>
            if let Some(ref mut blocks) = fig.caption.long {
                process_blocks(blocks, definitions);
            }
        }
        Block::Table(table) => {
            // Process table caption
            // Caption has short: Option<Inlines> and long: Option<Blocks>
            if let Some(ref mut blocks) = table.caption.long {
                process_blocks(blocks, definitions);
            }
            // Process table cells
            for body in &mut table.bodies {
                for row in &mut body.body {
                    for cell in &mut row.cells {
                        process_blocks(&mut cell.content, definitions);
                    }
                }
            }
            for row in &mut table.head.rows {
                for cell in &mut row.cells {
                    process_blocks(&mut cell.content, definitions);
                }
            }
            for row in &mut table.foot.rows {
                for cell in &mut row.cells {
                    process_blocks(&mut cell.content, definitions);
                }
            }
        }
        _ => {}
    }
}

/// Process inlines, resolving `NoteReference`/note-reference `Span`s.
fn process_inlines(inlines: &mut [Inline], definitions: &HashMap<String, NoteContent>) {
    for inline in inlines.iter_mut() {
        process_inline(inline, definitions);
    }
}

/// Process a single inline, potentially replacing it with a native `Note`.
fn process_inline(inline: &mut Inline, definitions: &HashMap<String, NoteContent>) {
    match inline {
        Inline::Note(_) => {
            // Already the target shape (`^[...]` inline note) - leave as-is.
            // Note: nested notes/references inside its content are NOT
            // recursively resolved, matching the pre-split transform's
            // behavior (it never recursed into a Note's own content either).
        }
        Inline::NoteReference(note_ref) => {
            if let Some(content) = definitions.get(&note_ref.id).cloned() {
                let source_info = note_ref.source_info.clone();
                *inline = Inline::Note(Note {
                    content: mark_with_ref_id(
                        content.into_blocks(&source_info),
                        &note_ref.id,
                        &source_info,
                    ),
                    source_info,
                });
            }
            // If not resolved, leave as-is (broken reference).
        }
        // Recursively process inlines that contain other inlines
        Inline::Emph(emph) => {
            process_inlines(&mut emph.content, definitions);
        }
        Inline::Strong(strong) => {
            process_inlines(&mut strong.content, definitions);
        }
        Inline::Strikeout(s) => {
            process_inlines(&mut s.content, definitions);
        }
        Inline::Superscript(sup) => {
            process_inlines(&mut sup.content, definitions);
        }
        Inline::Subscript(sub) => {
            process_inlines(&mut sub.content, definitions);
        }
        Inline::SmallCaps(sc) => {
            process_inlines(&mut sc.content, definitions);
        }
        Inline::Quoted(q) => {
            process_inlines(&mut q.content, definitions);
        }
        Inline::Cite(cite) => {
            process_inlines(&mut cite.content, definitions);
        }
        Inline::Link(link) => {
            process_inlines(&mut link.content, definitions);
        }
        Inline::Span(span) => {
            // bd-po3gn41h: pampa's postprocess lowers a named footnote
            // reference `[^id]` into an *empty* Span with class
            // `quarto-note-reference` and a `reference-id` kv, before any
            // quarto-core transform runs (see
            // `crates/pampa/src/pandoc/treesitter_utils/postprocess.rs`,
            // the `.with_note_reference` filter). The typed
            // `Inline::NoteReference` is already gone by this point, so we
            // resolve the Span form here exactly like the NoteReference arm
            // above: look up the definition and replace with a native
            // `Inline::Note`, or leave the span untouched if the reference
            // is undefined (broken reference).
            let reference_id = if span.attr.1.iter().any(|c| c == "quarto-note-reference") {
                span.attr.2.get("reference-id").cloned()
            } else {
                None
            };
            if let Some(ref_id) = reference_id {
                if let Some(content) = definitions.get(&ref_id).cloned() {
                    let source_info = span.source_info.clone();
                    *inline = Inline::Note(Note {
                        content: mark_with_ref_id(
                            content.into_blocks(&source_info),
                            &ref_id,
                            &source_info,
                        ),
                        source_info,
                    });
                }
                // If not resolved, leave as-is (broken reference).
            } else {
                process_inlines(&mut span.content, definitions);
            }
        }
        Inline::Underline(u) => {
            process_inlines(&mut u.content, definitions);
        }
        Inline::Delete(d) => {
            process_inlines(&mut d.content, definitions);
        }
        Inline::Insert(i) => {
            process_inlines(&mut i.content, definitions);
        }
        Inline::Highlight(h) => {
            process_inlines(&mut h.content, definitions);
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_pandoc_types::ConfigMapEntry;
    use quarto_pandoc_types::NoteDefinitionPara;
    use quarto_pandoc_types::attr::AttrSourceInfo;
    use quarto_pandoc_types::block::Plain;
    use quarto_pandoc_types::inline::Span;
    use quarto_source_map::{FileId, Location, Range};

    use crate::format::Format;
    use crate::project::{DocumentInfo, ProjectConfig, ProjectContext};
    use crate::render::BinaryDependencies;
    use crate::transforms::FootnotesResolveTransform;

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
            config: ProjectConfig::default(),
            is_single_file: true,
            files: vec![DocumentInfo::from_path("/project/doc.qmd")],
            output_dir: std::path::PathBuf::from("/project"),

            ..Default::default()
        }
    }

    fn make_str(text: &str) -> Inline {
        Inline::Str(quarto_pandoc_types::inline::Str {
            text: text.to_string(),
            source_info: dummy_source_info(),
        })
    }

    fn make_meta(entries: Vec<ConfigMapEntry>) -> ConfigValue {
        ConfigValue::new_map(entries, dummy_source_info())
    }

    fn meta_entry(key: &str, value: ConfigValue) -> ConfigMapEntry {
        ConfigMapEntry {
            key: key.to_string(),
            key_source: dummy_source_info(),
            value,
        }
    }

    fn make_ctx_pair<'a>(
        project: &'a ProjectContext,
        doc: &'a DocumentInfo,
        format: &'a Format,
        binaries: &'a BinaryDependencies,
    ) -> RenderContext<'a> {
        RenderContext::new(project, doc, format, binaries)
    }

    /// Run both halves in sequence, mirroring how they're spliced into the
    /// real HTML/revealjs pipeline (`footnotes` immediately followed by
    /// `footnotes-resolve`).
    async fn run_both_halves(ast: &mut Pandoc, ctx: &mut RenderContext<'_>) {
        FootnotesTransform::new().transform(ast, ctx).await.unwrap();
        FootnotesResolveTransform::new()
            .transform(ast, ctx)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn test_transform_name() {
        let transform = FootnotesTransform::new();
        assert_eq!(transform.name(), "footnotes");
    }

    #[tokio::test]
    async fn test_single_inline_note() {
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
        let mut ctx = make_ctx_pair(&project, &doc, &format, &binaries);

        run_both_halves(&mut ast, &mut ctx).await;

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

    #[tokio::test]
    async fn test_multiple_notes() {
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
        let mut ctx = make_ctx_pair(&project, &doc, &format, &binaries);

        run_both_halves(&mut ast, &mut ctx).await;

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

    #[tokio::test]
    async fn test_note_definition_and_reference() {
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
        let mut ctx = make_ctx_pair(&project, &doc, &format, &binaries);

        run_both_halves(&mut ast, &mut ctx).await;

        // Note definition should be removed, leaving paragraph + footnotes section
        assert_eq!(ast.blocks.len(), 2);

        // First block should be the paragraph (not the definition)
        assert!(matches!(ast.blocks[0], Block::Paragraph(_)));

        // Footnotes section should exist
        assert!(matches!(ast.blocks[1], Block::Div(_)));
    }

    /// After just the B1 half, a `NoteReference` resolves into a native
    /// `Inline::Note` — the seam `FootnotesResolveTransform` consumes.
    #[tokio::test]
    async fn test_note_reference_resolves_to_native_note_after_b1_only() {
        let mut ast = Pandoc {
            meta: quarto_pandoc_types::ConfigValue::default(),
            blocks: vec![
                Block::NoteDefinitionPara(NoteDefinitionPara {
                    id: "myfoot".to_string(),
                    content: vec![make_str("Defined footnote content")],
                    source_info: dummy_source_info(),
                }),
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
        let mut ctx = make_ctx_pair(&project, &doc, &format, &binaries);

        FootnotesTransform::new()
            .transform(&mut ast, &mut ctx)
            .await
            .unwrap();

        // The definition block is gone; no chrome (Span#fnref / Div#footnotes)
        // has been produced yet — only B1 ran.
        assert_eq!(ast.blocks.len(), 1);
        if let Block::Paragraph(para) = &ast.blocks[0] {
            assert!(
                matches!(&para.content[1], Inline::Note(_)),
                "expected NoteReference resolved to native Inline::Note by B1 alone, got: {:?}",
                para.content[1]
            );
        } else {
            panic!("expected Paragraph");
        }
    }

    /// bd-po3gn41h: a named/reference-style footnote `[^id]` is lowered by
    /// pampa's postprocess into an *empty* `Inline::Span` with class
    /// `quarto-note-reference` and a `reference-id` kv (NOT an
    /// `Inline::NoteReference` — that variant is already gone by the time this
    /// transform runs). `FootnotesTransform` must resolve that Span form the
    /// same way it resolves `Inline::NoteReference`.
    #[tokio::test]
    async fn test_span_note_reference_resolves() {
        let mut reference_kv = hashlink::LinkedHashMap::new();
        reference_kv.insert("reference-id".to_string(), "bk".to_string());

        let mut ast = Pandoc {
            meta: quarto_pandoc_types::ConfigValue::default(),
            blocks: vec![
                // Paragraph with the lowered reference span (empty content).
                Block::Paragraph(Paragraph {
                    content: vec![
                        make_str("Ref."),
                        Inline::Span(Span {
                            attr: (
                                String::new(),
                                vec!["quarto-note-reference".to_string()],
                                reference_kv,
                            ),
                            content: vec![],
                            source_info: dummy_source_info(),
                            attr_source: AttrSourceInfo::empty(),
                        }),
                    ],
                    source_info: dummy_source_info(),
                }),
                // Block definition `::: ^bk … :::`.
                Block::NoteDefinitionFencedBlock(quarto_pandoc_types::NoteDefinitionFencedBlock {
                    id: "bk".to_string(),
                    content: vec![Block::Paragraph(Paragraph {
                        content: vec![make_str("Note.")],
                        source_info: dummy_source_info(),
                    })],
                    source_info: dummy_source_info(),
                }),
            ],
        };

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = make_ctx_pair(&project, &doc, &format, &binaries);

        run_both_halves(&mut ast, &mut ctx).await;

        // Definition block removed; paragraph + footnotes section remain.
        assert_eq!(ast.blocks.len(), 2, "blocks: {:#?}", ast.blocks);

        // The reference span must be replaced by the standard fnref superscript.
        if let Block::Paragraph(para) = &ast.blocks[0] {
            assert_eq!(para.content.len(), 2);
            match &para.content[1] {
                Inline::Span(span) => {
                    assert_eq!(span.attr.0, "fnref1", "expected fnref id on resolved ref");
                    match &span.content[0] {
                        Inline::Superscript(sup) => match &sup.content[0] {
                            Inline::Link(link) => {
                                assert_eq!(link.target.0, "#fn1");
                                assert!(link.attr.1.contains(&"footnote-ref".to_string()));
                            }
                            other => panic!("expected Link inside Superscript, got {other:?}"),
                        },
                        other => panic!("expected Superscript inside Span, got {other:?}"),
                    }
                }
                other => panic!("expected resolved footnote ref Span, got {other:?}"),
            }
        } else {
            panic!("expected Paragraph as first block");
        }

        // Footnotes section exists and carries the definition's text.
        if let Block::Div(div) = &ast.blocks[1] {
            assert_eq!(div.attr.0, "footnotes");
            let rendered = format!("{:?}", div);
            assert!(
                rendered.contains("Note."),
                "footnotes section should carry the definition content; got:\n{rendered}"
            );
        } else {
            panic!("expected footnotes Div as second block");
        }
    }

    #[tokio::test]
    async fn test_no_footnotes() {
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
        let mut ctx = make_ctx_pair(&project, &doc, &format, &binaries);

        run_both_halves(&mut ast, &mut ctx).await;

        // Should not add footnotes section
        assert_eq!(ast.blocks.len(), 1);
    }

    #[tokio::test]
    async fn test_reference_location_parsing() {
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

    #[tokio::test]
    async fn test_nested_note_in_emphasis() {
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
        let mut ctx = make_ctx_pair(&project, &doc, &format, &binaries);

        run_both_halves(&mut ast, &mut ctx).await;

        // Should have paragraph + footnotes section
        assert_eq!(ast.blocks.len(), 2);
    }

    #[tokio::test]
    async fn test_margin_mode_no_section() {
        let mut ast = Pandoc {
            meta: make_meta(vec![meta_entry(
                "reference-location",
                ConfigValue::new_string("margin", dummy_source_info()),
            )]),
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
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = make_ctx_pair(&project, &doc, &format, &binaries);

        run_both_halves(&mut ast, &mut ctx).await;

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

    #[tokio::test]
    async fn test_block_section_modes_are_noop() {
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
                meta: make_meta(vec![meta_entry(
                    "reference-location",
                    ConfigValue::new_string(mode, dummy_source_info()),
                )]),
                blocks: vec![Block::Paragraph(Paragraph {
                    content: vec![make_str("Text"), note.clone()],
                    source_info: dummy_source_info(),
                })],
            };

            let project = make_test_project();
            let doc = DocumentInfo::from_path("/project/doc.qmd");
            let format = Format::html();
            let binaries = BinaryDependencies::new();
            let mut ctx = make_ctx_pair(&project, &doc, &format, &binaries);

            run_both_halves(&mut ast, &mut ctx).await;

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

    /// T5.3: `reference-location: block`/`section` must remain a no-op under
    /// **every** profile, including `Pandoc("docx")` — both halves must keep
    /// the early-return gate independently (H5c). Revert either half's gate
    /// and this goes RED.
    ///
    /// Uses a `NoteReference` + `NoteDefinitionPara` fixture rather than a
    /// bare `Inline::Note`: a pre-existing `Note` is already B1's target
    /// shape, so B1 leaves it untouched regardless of its own gate — that
    /// fixture cannot discriminate "B1's gate is present" from "B1's gate
    /// was deleted, but there was nothing for it to do anyway". A
    /// `NoteReference` + definition pair does discriminate: without the
    /// gate, B1 would collect the definition (removing its block) and
    /// resolve the reference into a `Note`, both visible AST changes.
    #[tokio::test]
    async fn test_reference_location_block_section_noop_under_every_profile() {
        for mode in ["block", "section"] {
            for format in [Format::html(), Format::docx()] {
                let mut ast = Pandoc {
                    meta: make_meta(vec![meta_entry(
                        "reference-location",
                        ConfigValue::new_string(mode, dummy_source_info()),
                    )]),
                    blocks: vec![
                        Block::NoteDefinitionPara(NoteDefinitionPara {
                            id: "1".to_string(),
                            content: vec![make_str("note content")],
                            source_info: dummy_source_info(),
                        }),
                        Block::Paragraph(Paragraph {
                            content: vec![
                                make_str("Text"),
                                Inline::NoteReference(quarto_pandoc_types::inline::NoteReference {
                                    id: "1".to_string(),
                                    source_info: dummy_source_info(),
                                }),
                            ],
                            source_info: dummy_source_info(),
                        }),
                    ],
                };

                let project = make_test_project();
                let doc = DocumentInfo::from_path("/project/doc.qmd");
                let binaries = BinaryDependencies::new();
                let mut ctx = make_ctx_pair(&project, &doc, &format, &binaries);

                run_both_halves(&mut ast, &mut ctx).await;

                assert_eq!(
                    ast.blocks.len(),
                    2,
                    "mode {mode}, format {:?}: AST must be unchanged — the note definition \
                     block must survive (not collected)",
                    format.target_format
                );
                assert!(
                    matches!(ast.blocks[0], Block::NoteDefinitionPara(_)),
                    "mode {mode}, format {:?}: NoteDefinitionPara must survive untouched",
                    format.target_format
                );
                if let Block::Paragraph(para) = &ast.blocks[1] {
                    assert!(
                        matches!(&para.content[1], Inline::NoteReference(_)),
                        "mode {mode}, format {:?}: NoteReference must be left unresolved by \
                         both halves; got {:?}",
                        format.target_format,
                        para.content[1]
                    );
                } else {
                    panic!("expected Paragraph");
                }
            }
        }
    }

    #[tokio::test]
    async fn test_document_mode_creates_section() {
        let mut ast = Pandoc {
            meta: make_meta(vec![meta_entry(
                "reference-location",
                ConfigValue::new_string("document", dummy_source_info()),
            )]),
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
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = make_ctx_pair(&project, &doc, &format, &binaries);

        run_both_halves(&mut ast, &mut ctx).await;

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
