/*
 * qmd_prepass.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * AST pre-pass for the qmd writer.
 *
 * The writer emits one block or inline at a time, and two of the fixes for
 * pandoc-shaped ASTs (document import) need context it doesn't have:
 *
 * - a `Note` with more than one block can't be written inline (`^[…]` holds
 *   one paragraph). It becomes a `[^nK]` reference at the note's position and
 *   a fenced definition (`::: ^nK`) placed after the top-level block holding
 *   the reference;
 * - two adjacent `OrderedList`s whose written markers are the same (`1.`
 *   and `1.`) re-read as one list, so a `<!-- -->` raw block goes between
 *   them. Lists whose markers differ (`1.` then `1)`) need nothing: the
 *   reader starts a new list at a delimiter change, as CommonMark does.
 *   (Bullet lists are kept apart by alternating the marker, which the block
 *   writer does itself.) Inside a list item the block writer fences the
 *   separator, because a bare one written tight would be absorbed by the
 *   reader (bd-2bcre8cr).
 *
 * The pass is copy-on-trigger. The incremental writer runs on every hub edit,
 * and a qmd-read AST never contains either shape (qmd lists are decimal, and
 * two adjacent ones always differ in delimiter), so a read-only scan decides
 * first and the document is cloned only when something needs rewriting.
 */

use crate::pandoc::block::{NoteDefinitionFencedBlock, RawBlock};
use crate::pandoc::inline::{Note, NoteReference};
use crate::pandoc::{Block, Inline, Pandoc};
use std::borrow::Cow;
use std::collections::HashSet;

/// The text of the raw block that keeps two ordered lists apart.
const LIST_SEPARATOR: &str = "<!-- -->";

/// `pandoc` ready for the block writer: borrowed when nothing needs
/// rewriting, a rewritten copy otherwise.
pub(super) fn prepare(pandoc: &Pandoc) -> Cow<'_, Pandoc> {
    let mut scan = TriggerScan::default();
    walk_blocks(&pandoc.blocks, &mut scan);
    if !scan.found {
        return Cow::Borrowed(pandoc);
    }
    Cow::Owned(rewrite(pandoc))
}

/// Does this `Note` need the reference-and-definition form? A note holding
/// exactly one paragraph stays inline; an empty one keeps its inline form too.
fn is_multi_block(note: &Note) -> bool {
    !matches!(
        note.content.as_slice(),
        [] | [Block::Paragraph(_) | Block::Plain(_)]
    )
}

// ---------------------------------------------------------------------------
// Read-only walk
// ---------------------------------------------------------------------------

trait Visit {
    /// Called for every block sequence, before its blocks are visited.
    fn blocks(&mut self, _blocks: &[Block]) {}
    fn block(&mut self, _block: &Block) {}
    fn inline(&mut self, _inline: &Inline) {}
}

#[derive(Default)]
struct TriggerScan {
    found: bool,
}

impl Visit for TriggerScan {
    fn blocks(&mut self, blocks: &[Block]) {
        if blocks
            .windows(2)
            .any(|w| needs_list_separator(&w[0], &w[1]))
        {
            self.found = true;
        }
    }

    fn inline(&mut self, inline: &Inline) {
        if let Inline::Note(note) = inline
            && is_multi_block(note)
        {
            self.found = true;
        }
    }
}

/// Every id in the document: element ids, note references and note
/// definitions, and the `reference-id` of a `quarto-note-reference` span.
#[derive(Default)]
struct IdCollector {
    ids: HashSet<String>,
}

impl IdCollector {
    fn add_attr(&mut self, attr: &crate::pandoc::Attr) {
        if !attr.0.is_empty() {
            self.ids.insert(attr.0.clone());
        }
        if let Some(reference) = attr.2.get("reference-id") {
            self.ids.insert(reference.clone());
        }
    }
}

impl Visit for IdCollector {
    fn block(&mut self, block: &Block) {
        match block {
            Block::CodeBlock(b) => self.add_attr(&b.attr),
            Block::Header(b) => self.add_attr(&b.attr),
            Block::Table(b) => self.add_attr(&b.attr),
            Block::Figure(b) => self.add_attr(&b.attr),
            Block::Div(b) => self.add_attr(&b.attr),
            Block::NoteDefinitionPara(b) => {
                self.ids.insert(b.id.clone());
            }
            Block::NoteDefinitionFencedBlock(b) => {
                self.ids.insert(b.id.clone());
            }
            _ => {}
        }
    }

    fn inline(&mut self, inline: &Inline) {
        match inline {
            Inline::Code(i) => self.add_attr(&i.attr),
            Inline::Link(i) => self.add_attr(&i.attr),
            Inline::Image(i) => self.add_attr(&i.attr),
            Inline::Span(i) => self.add_attr(&i.attr),
            Inline::Insert(i) => self.add_attr(&i.attr),
            Inline::Delete(i) => self.add_attr(&i.attr),
            Inline::Highlight(i) => self.add_attr(&i.attr),
            Inline::EditComment(i) => self.add_attr(&i.attr),
            Inline::NoteReference(i) => {
                self.ids.insert(i.id.clone());
            }
            _ => {}
        }
    }
}

fn walk_blocks(blocks: &[Block], v: &mut impl Visit) {
    v.blocks(blocks);
    for block in blocks {
        v.block(block);
        match block {
            Block::Plain(b) => walk_inlines(&b.content, v),
            Block::Paragraph(b) => walk_inlines(&b.content, v),
            Block::LineBlock(b) => b.content.iter().for_each(|l| walk_inlines(l, v)),
            Block::BlockQuote(b) => walk_blocks(&b.content, v),
            Block::OrderedList(b) => b.content.iter().for_each(|i| walk_blocks(i, v)),
            Block::BulletList(b) => b.content.iter().for_each(|i| walk_blocks(i, v)),
            Block::DefinitionList(b) => {
                for (term, defs) in &b.content {
                    walk_inlines(term, v);
                    defs.iter().for_each(|d| walk_blocks(d, v));
                }
            }
            Block::Header(b) => walk_inlines(&b.content, v),
            Block::Table(b) => {
                walk_caption(&b.caption, v);
                let head_rows = b.head.rows.iter();
                let body_rows = b
                    .bodies
                    .iter()
                    .flat_map(|body| body.head.iter().chain(&body.body));
                let foot_rows = b.foot.rows.iter();
                for row in head_rows.chain(body_rows).chain(foot_rows) {
                    row.cells.iter().for_each(|c| walk_blocks(&c.content, v));
                }
            }
            Block::Figure(b) => {
                walk_caption(&b.caption, v);
                walk_blocks(&b.content, v);
            }
            Block::Div(b) => walk_blocks(&b.content, v),
            Block::NoteDefinitionPara(b) => walk_inlines(&b.content, v),
            Block::NoteDefinitionFencedBlock(b) => walk_blocks(&b.content, v),
            Block::CaptionBlock(b) => walk_inlines(&b.content, v),
            // Leaves, and custom nodes, which the qmd writer doesn't render.
            Block::CodeBlock(_)
            | Block::RawBlock(_)
            | Block::HorizontalRule(_)
            | Block::BlockMetadata(_)
            | Block::Custom(_) => {}
        }
    }
}

fn walk_caption(caption: &crate::pandoc::Caption, v: &mut impl Visit) {
    if let Some(short) = &caption.short {
        walk_inlines(short, v);
    }
    if let Some(long) = &caption.long {
        walk_blocks(long, v);
    }
}

fn walk_inlines(inlines: &[Inline], v: &mut impl Visit) {
    for inline in inlines {
        v.inline(inline);
        match inline {
            Inline::Emph(i) => walk_inlines(&i.content, v),
            Inline::Strong(i) => walk_inlines(&i.content, v),
            Inline::Underline(i) => walk_inlines(&i.content, v),
            Inline::Strikeout(i) => walk_inlines(&i.content, v),
            Inline::Superscript(i) => walk_inlines(&i.content, v),
            Inline::Subscript(i) => walk_inlines(&i.content, v),
            Inline::SmallCaps(i) => walk_inlines(&i.content, v),
            Inline::Quoted(i) => walk_inlines(&i.content, v),
            Inline::Link(i) => walk_inlines(&i.content, v),
            Inline::Image(i) => walk_inlines(&i.content, v),
            Inline::Span(i) => walk_inlines(&i.content, v),
            Inline::Insert(i) => walk_inlines(&i.content, v),
            Inline::Delete(i) => walk_inlines(&i.content, v),
            Inline::Highlight(i) => walk_inlines(&i.content, v),
            Inline::EditComment(i) => walk_inlines(&i.content, v),
            Inline::Cite(i) => {
                for citation in &i.citations {
                    walk_inlines(&citation.prefix, v);
                    walk_inlines(&citation.suffix, v);
                }
                walk_inlines(&i.content, v);
            }
            Inline::Note(i) => walk_blocks(&i.content, v),
            Inline::Str(_)
            | Inline::Code(_)
            | Inline::Space(_)
            | Inline::SoftBreak(_)
            | Inline::LineBreak(_)
            | Inline::Math(_)
            | Inline::RawInline(_)
            | Inline::Shortcode(_)
            | Inline::NoteReference(_)
            | Inline::Attr(_)
            | Inline::Custom(_) => {}
        }
    }
}

// ---------------------------------------------------------------------------
// Rewrite
// ---------------------------------------------------------------------------

enum Children<'a> {
    Blocks(&'a mut Vec<Block>),
    Inlines(&'a mut Vec<Inline>),
}

/// The block and inline sequences directly inside `block`.
fn block_children(block: &mut Block) -> Vec<Children<'_>> {
    use Children::{Blocks, Inlines};
    match block {
        Block::Plain(b) => vec![Inlines(&mut b.content)],
        Block::Paragraph(b) => vec![Inlines(&mut b.content)],
        Block::LineBlock(b) => b.content.iter_mut().map(Inlines).collect(),
        Block::BlockQuote(b) => vec![Blocks(&mut b.content)],
        Block::OrderedList(b) => b.content.iter_mut().map(Blocks).collect(),
        Block::BulletList(b) => b.content.iter_mut().map(Blocks).collect(),
        Block::DefinitionList(b) => {
            let mut out = Vec::new();
            for (term, defs) in &mut b.content {
                out.push(Inlines(term));
                out.extend(defs.iter_mut().map(Blocks));
            }
            out
        }
        Block::Header(b) => vec![Inlines(&mut b.content)],
        Block::Table(b) => {
            let mut out = caption_children(&mut b.caption);
            let body_rows = b
                .bodies
                .iter_mut()
                .flat_map(|body| body.head.iter_mut().chain(body.body.iter_mut()));
            for row in b
                .head
                .rows
                .iter_mut()
                .chain(body_rows)
                .chain(b.foot.rows.iter_mut())
            {
                out.extend(row.cells.iter_mut().map(|c| Blocks(&mut c.content)));
            }
            out
        }
        Block::Figure(b) => {
            let mut out = caption_children(&mut b.caption);
            out.push(Blocks(&mut b.content));
            out
        }
        Block::Div(b) => vec![Blocks(&mut b.content)],
        Block::NoteDefinitionPara(b) => vec![Inlines(&mut b.content)],
        Block::NoteDefinitionFencedBlock(b) => vec![Blocks(&mut b.content)],
        Block::CaptionBlock(b) => vec![Inlines(&mut b.content)],
        Block::CodeBlock(_)
        | Block::RawBlock(_)
        | Block::HorizontalRule(_)
        | Block::BlockMetadata(_)
        | Block::Custom(_) => Vec::new(),
    }
}

fn caption_children(caption: &mut crate::pandoc::Caption) -> Vec<Children<'_>> {
    let mut out = Vec::new();
    if let Some(short) = &mut caption.short {
        out.push(Children::Inlines(short));
    }
    if let Some(long) = &mut caption.long {
        out.push(Children::Blocks(long));
    }
    out
}

/// The block and inline sequences directly inside `inline`.
fn inline_children(inline: &mut Inline) -> Vec<Children<'_>> {
    use Children::{Blocks, Inlines};
    match inline {
        Inline::Emph(i) => vec![Inlines(&mut i.content)],
        Inline::Strong(i) => vec![Inlines(&mut i.content)],
        Inline::Underline(i) => vec![Inlines(&mut i.content)],
        Inline::Strikeout(i) => vec![Inlines(&mut i.content)],
        Inline::Superscript(i) => vec![Inlines(&mut i.content)],
        Inline::Subscript(i) => vec![Inlines(&mut i.content)],
        Inline::SmallCaps(i) => vec![Inlines(&mut i.content)],
        Inline::Quoted(i) => vec![Inlines(&mut i.content)],
        Inline::Link(i) => vec![Inlines(&mut i.content)],
        Inline::Image(i) => vec![Inlines(&mut i.content)],
        Inline::Span(i) => vec![Inlines(&mut i.content)],
        Inline::Insert(i) => vec![Inlines(&mut i.content)],
        Inline::Delete(i) => vec![Inlines(&mut i.content)],
        Inline::Highlight(i) => vec![Inlines(&mut i.content)],
        Inline::EditComment(i) => vec![Inlines(&mut i.content)],
        Inline::Cite(i) => {
            let mut out = Vec::new();
            for citation in &mut i.citations {
                out.push(Inlines(&mut citation.prefix));
                out.push(Inlines(&mut citation.suffix));
            }
            out.push(Inlines(&mut i.content));
            out
        }
        Inline::Note(i) => vec![Blocks(&mut i.content)],
        Inline::Str(_)
        | Inline::Code(_)
        | Inline::Space(_)
        | Inline::SoftBreak(_)
        | Inline::LineBreak(_)
        | Inline::Math(_)
        | Inline::RawInline(_)
        | Inline::Shortcode(_)
        | Inline::NoteReference(_)
        | Inline::Attr(_)
        | Inline::Custom(_) => Vec::new(),
    }
}

struct Rewriter {
    /// Ids already in the document, plus the ones handed out so far.
    used: HashSet<String>,
    next: usize,
    /// Definitions created for the top-level block being rewritten, in id order.
    definitions: Vec<NoteDefinitionFencedBlock>,
}

impl Rewriter {
    fn fresh_id(&mut self) -> String {
        loop {
            let id = format!("n{}", self.next);
            self.next += 1;
            if self.used.insert(id.clone()) {
                return id;
            }
        }
    }

    fn blocks(&mut self, blocks: &mut Vec<Block>) {
        for block in blocks.iter_mut() {
            self.block(block);
        }
        separate_ordered_lists(blocks);
    }

    fn block(&mut self, block: &mut Block) {
        for child in block_children(block) {
            match child {
                Children::Blocks(blocks) => self.blocks(blocks),
                Children::Inlines(inlines) => self.inlines(inlines),
            }
        }
    }

    fn inlines(&mut self, inlines: &mut Vec<Inline>) {
        for inline in inlines.iter_mut() {
            if let Inline::Note(note) = inline
                && is_multi_block(note)
            {
                let id = self.fresh_id();
                let source_info = note.source_info.clone();
                let mut content = std::mem::take(&mut note.content);
                // Reserve the definition's place first so ids and definitions
                // stay in document order when the body holds further notes.
                let slot = self.definitions.len();
                self.definitions.push(NoteDefinitionFencedBlock {
                    id: id.clone(),
                    content: Vec::new(),
                    source_info: source_info.clone(),
                });
                self.blocks(&mut content);
                self.definitions[slot].content = content;
                *inline = Inline::NoteReference(NoteReference { id, source_info });
            } else {
                for child in inline_children(inline) {
                    match child {
                        Children::Blocks(blocks) => self.blocks(blocks),
                        Children::Inlines(inlines) => self.inlines(inlines),
                    }
                }
            }
        }
    }
}

#[cfg(test)]
fn is_list_separator(block: &Block) -> bool {
    matches!(block, Block::RawBlock(r) if r.format == "html" && r.text == LIST_SEPARATOR)
}

/// The marker class an ordered list is written with: `(@)` for example
/// lists, otherwise the delimiter after the number (the block writer's
/// mapping in `write_orderedlist`). `None` for example lists: the reader's
/// handling of adjacent `(@)` lists is unreliable (bd-bumesax9), so they are
/// always separated.
fn written_delimiter(list: &crate::pandoc::block::OrderedList) -> Option<char> {
    use crate::pandoc::list::{ListNumberDelim, ListNumberStyle};
    let (_, style, delim) = &list.attr;
    if matches!(style, ListNumberStyle::Example) {
        return None;
    }
    Some(match delim {
        ListNumberDelim::OneParen | ListNumberDelim::TwoParens => ')',
        ListNumberDelim::Period | ListNumberDelim::Default => '.',
    })
}

/// Would `prev` and `next`, written back to back, re-read as one list? Only
/// two ordered lists with the same written delimiter would; a delimiter
/// change starts a new list on the next read.
fn needs_list_separator(prev: &Block, next: &Block) -> bool {
    match (prev, next) {
        (Block::OrderedList(a), Block::OrderedList(b)) => {
            match (written_delimiter(a), written_delimiter(b)) {
                (Some(x), Some(y)) => x == y,
                _ => true,
            }
        }
        _ => false,
    }
}

/// Put a raw `<!-- -->` between every two adjacent ordered lists that would
/// otherwise re-read as one ([`needs_list_separator`]).
fn separate_ordered_lists(blocks: &mut Vec<Block>) {
    if !blocks
        .windows(2)
        .any(|w| needs_list_separator(&w[0], &w[1]))
    {
        return;
    }
    let mut out: Vec<Block> = Vec::with_capacity(blocks.len() + 1);
    for block in blocks.drain(..) {
        if let Some(prev @ Block::OrderedList(prev_list)) = out.last()
            && needs_list_separator(prev, &block)
        {
            let source_info = prev_list.source_info.clone();
            out.push(Block::RawBlock(Box::new(RawBlock {
                format: "html".to_string(),
                text: LIST_SEPARATOR.to_string(),
                source_info,
            })));
        }
        out.push(block);
    }
    *blocks = out;
}

fn rewrite(pandoc: &Pandoc) -> Pandoc {
    let mut ids = IdCollector::default();
    walk_blocks(&pandoc.blocks, &mut ids);
    let mut rewriter = Rewriter {
        used: ids.ids,
        next: 1,
        definitions: Vec::new(),
    };

    let mut out = Vec::with_capacity(pandoc.blocks.len());
    for block in &pandoc.blocks {
        let mut block = block.clone();
        rewriter.block(&mut block);
        // Inserted blocks take the source info of the block they follow, so
        // the incremental writer's tiling of the output still holds.
        let source_info = block.source_info().clone();
        out.push(block);
        for mut definition in rewriter.definitions.drain(..) {
            definition.source_info = source_info.clone();
            out.push(Block::NoteDefinitionFencedBlock(Box::new(definition)));
        }
    }
    separate_ordered_lists(&mut out);

    Pandoc {
        meta: pandoc.meta.clone(),
        blocks: out,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(json: &str) -> Pandoc {
        crate::readers::json::read_completing_source_info(
            &mut json.as_bytes(),
            quarto_source_map::By::unknown(),
        )
        .expect("JSON should read")
        .0
    }

    fn doc(blocks: &str) -> Pandoc {
        read(&format!(
            r#"{{"pandoc-api-version":[1,23,1],"meta":{{}},"blocks":{blocks}}}"#
        ))
    }

    const ORDERED: &str = r#"{"t":"OrderedList","c":[[1,{"t":"Decimal"},{"t":"Period"}],[[{"t":"Plain","c":[{"t":"Str","c":"a"}]}]]]}"#;
    const PARA_A: &str = r#"{"t":"Para","c":[{"t":"Str","c":"a"}]}"#;

    #[test]
    fn a_document_with_neither_trigger_is_borrowed() {
        let single_note = format!(r#"{{"t":"Para","c":[{{"t":"Note","c":[{PARA_A}]}}]}}"#);
        let pandoc = doc(&format!(
            "[{PARA_A},{ORDERED},{PARA_A},{ORDERED},{single_note}]"
        ));
        assert!(matches!(prepare(&pandoc), Cow::Borrowed(_)));
    }

    #[test]
    fn adjacent_ordered_lists_trigger_a_rewrite() {
        let pandoc = doc(&format!("[{ORDERED},{ORDERED}]"));
        let prepared = prepare(&pandoc);
        assert!(matches!(prepared, Cow::Owned(_)));
        assert_eq!(prepared.blocks.len(), 3);
        assert!(is_list_separator(&prepared.blocks[1]));
    }

    fn ordered(delim: &str, style: &str) -> String {
        format!(
            r#"{{"t":"OrderedList","c":[[1,{{"t":"{style}"}},{{"t":"{delim}"}}],[[{{"t":"Plain","c":[{{"t":"Str","c":"a"}}]}}]]]}}"#
        )
    }

    /// bd-2bcre8cr: `1.` then `1)` re-read as two lists without help, and
    /// it's the only adjacent pair a qmd read produces, so it must not
    /// trigger the copy (or insert a separator the writer then has to place).
    #[test]
    fn ordered_lists_with_differing_delimiters_are_borrowed() {
        let pandoc = doc(&format!(
            "[{},{}]",
            ordered("Period", "Decimal"),
            ordered("OneParen", "Decimal")
        ));
        assert!(matches!(prepare(&pandoc), Cow::Borrowed(_)));
    }

    /// The written marker decides, not the AST field: `Default` and `Period`
    /// both write as `1.`, `OneParen` and `TwoParens` both as `1)`. Example
    /// lists are always separated (bd-bumesax9).
    #[test]
    fn lists_with_the_same_written_marker_are_separated() {
        for (a, b) in [
            (
                ordered("DefaultDelim", "Decimal"),
                ordered("Period", "Decimal"),
            ),
            (
                ordered("OneParen", "Decimal"),
                ordered("TwoParens", "Decimal"),
            ),
            (ordered("Period", "Example"), ordered("OneParen", "Decimal")),
        ] {
            let pandoc = doc(&format!("[{a},{b}]"));
            let prepared = prepare(&pandoc);
            assert_eq!(prepared.blocks.len(), 3, "{a} / {b}");
            assert!(is_list_separator(&prepared.blocks[1]), "{a} / {b}");
        }
    }

    #[test]
    fn a_multi_block_note_triggers_a_rewrite() {
        let two = format!(r#"{{"t":"Para","c":[{{"t":"Note","c":[{PARA_A},{PARA_A}]}}]}}"#);
        let pandoc = doc(&format!("[{two}]"));
        let prepared = prepare(&pandoc);
        assert!(matches!(prepared, Cow::Owned(_)));
        // The paragraph, then the definition.
        assert_eq!(prepared.blocks.len(), 2);
        assert!(matches!(&prepared.blocks[1], Block::NoteDefinitionFencedBlock(d) if d.id == "n1"));
    }
}
