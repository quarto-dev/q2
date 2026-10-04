//! The import transforms: pandoc's reading of Word's track changes, comments and highlights
//! into q2 editorial marks (I3, I4, I13), plus the pptx format default and list-style
//! detection (I6, I17).
//!
//! Each is a pure `Pandoc -> (Pandoc, counts)` walk; nothing here reads a clock or a
//! filesystem (I20). The class names are constants in this one place: P6's export transform
//! later moves them, and the I4 shape, into a shared module used by both directions.

use std::collections::HashSet;

use hashlink::LinkedHashMap;
use quarto_pandoc_types::attr::AttrSourceInfo;
use quarto_pandoc_types::block::{Block, Paragraph};
use quarto_pandoc_types::inline::{Inline, Inlines, Span};
use quarto_pandoc_types::list::{ListNumberDelim, ListNumberStyle};
use quarto_pandoc_types::pandoc::Pandoc;
use quarto_source_map::{By, SourceInfo};

use crate::ast_walk::{
    for_each_block_list_mut, for_each_inline_list_mut, for_each_meta_inline_list_mut,
};

// ---------------------------------------------------------------------------
// Class names
// ---------------------------------------------------------------------------

/// pandoc's docx reader spans (`--track-changes=all`).
pub const PANDOC_INSERTION: &str = "insertion";
pub const PANDOC_DELETION: &str = "deletion";
pub const PANDOC_PARAGRAPH_INSERTION: &str = "paragraph-insertion";
pub const PANDOC_PARAGRAPH_DELETION: &str = "paragraph-deletion";
pub const PANDOC_COMMENT_START: &str = "comment-start";
pub const PANDOC_COMMENT_END: &str = "comment-end";
/// A Word highlight.
pub const PANDOC_MARK: &str = "mark";

/// The q2 editorial marks (the classes the qmd reader gives `[++ ]`, `[-- ]`, `[!! ]`,
/// `[>> ]`).
pub const QUARTO_INSERT: &str = "quarto-insert";
pub const QUARTO_DELETE: &str = "quarto-delete";
pub const QUARTO_HIGHLIGHT: &str = "quarto-highlight";
pub const QUARTO_EDIT_COMMENT: &str = "quarto-edit-comment";

/// The attributes an editorial mark keeps from pandoc's span.
const KEPT_ATTRS: [&str; 2] = ["author", "date"];

/// What the transforms counted, for the report (Q-24-5, -6, -7, -14).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct TransformCounts {
    /// Comments attached to their block (I13), plus comments dropped from metadata (Q-24-5).
    pub block_comments: usize,
    /// Paragraph-mark tracked changes dropped (Q-24-6).
    pub paragraph_marks: usize,
    /// Ordered lists whose number style is lost (Q-24-7).
    pub list_styles: usize,
    /// Unmatched comment markers (Q-24-14).
    pub unmatched_markers: usize,
}

// ---------------------------------------------------------------------------
// Small helpers
// ---------------------------------------------------------------------------

fn generated() -> SourceInfo {
    SourceInfo::generated(By::unknown())
}

fn has_class(span: &Span, class: &str) -> bool {
    span.attr.1.iter().any(|c| c == class)
}

fn marker_id(span: &Span) -> &str {
    span.attr.2.get("id").map_or("", String::as_str)
}

fn as_span(inline: &Inline) -> Option<&Span> {
    match inline {
        Inline::Span(s) => Some(s),
        _ => None,
    }
}

fn is_start(inline: &Inline) -> bool {
    as_span(inline).is_some_and(|s| has_class(s, PANDOC_COMMENT_START))
}

fn is_end(inline: &Inline) -> bool {
    as_span(inline).is_some_and(|s| has_class(s, PANDOC_COMMENT_END))
}

fn id_of(inline: &Inline) -> &str {
    as_span(inline).map_or("", marker_id)
}

fn kept_attrs(span: &Span) -> LinkedHashMap<String, String> {
    span.attr
        .2
        .iter()
        .filter(|(k, _)| KEPT_ATTRS.contains(&k.as_str()))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect()
}

fn new_span(
    classes: Vec<String>,
    attrs: LinkedHashMap<String, String>,
    content: Inlines,
) -> Inline {
    Inline::Span(Span {
        attr: (String::new(), classes, attrs),
        content,
        source_info: generated(),
        attr_source: AttrSourceInfo::empty(),
    })
}

/// The comment span `[>> text]{author= date=}` for a `comment-start` marker. The Word `id` is
/// dropped.
fn comment_span(start: &Inline) -> Inline {
    let span = as_span(start).expect("a comment-start span");
    new_span(
        vec![QUARTO_EDIT_COMMENT.to_string()],
        kept_attrs(span),
        span.content.clone(),
    )
}

// ---------------------------------------------------------------------------
// T4a: class renames, paragraph marks, flattening nested comment ends
// ---------------------------------------------------------------------------

/// `Span .insertion {…}` → `Span .quarto-insert {author date}` (and `.deletion`): the mark
/// class goes first (P2 T4's shorthand rule), every other pandoc attribute is dropped.
fn rename_tracked_change(span: &mut Span, from: &str, to: &str) {
    let kept = kept_attrs(span);
    let mut classes: Vec<String> = span.attr.1.iter().filter(|c| *c != from).cloned().collect();
    classes.insert(0, to.to_string());
    span.attr = (String::new(), classes, kept);
    span.attr_source = AttrSourceInfo::empty();
}

/// `Span .mark` → `Span .quarto-highlight`, other attributes kept.
fn rename_highlight(span: &mut Span) {
    let mut classes: Vec<String> = span
        .attr
        .1
        .iter()
        .filter(|c| *c != PANDOC_MARK)
        .cloned()
        .collect();
    classes.insert(0, QUARTO_HIGHLIGHT.to_string());
    span.attr.1 = classes;
    span.attr_source = AttrSourceInfo::empty();
}

/// One list's share of T4a. Children are already done (the walk is post-order), so a
/// `comment-end`'s content is already flat.
fn normalize_list(list: &mut Inlines, counts: &mut TransformCounts) {
    if !list.iter().any(|i| matches!(i, Inline::Span(_))) {
        return;
    }
    let mut out = Vec::with_capacity(list.len());
    for inline in std::mem::take(list) {
        let Inline::Span(mut span) = inline else {
            out.push(inline);
            continue;
        };
        if has_class(&span, PANDOC_PARAGRAPH_INSERTION)
            || has_class(&span, PANDOC_PARAGRAPH_DELETION)
        {
            counts.paragraph_marks += 1;
        } else if has_class(&span, PANDOC_INSERTION) {
            rename_tracked_change(&mut span, PANDOC_INSERTION, QUARTO_INSERT);
            out.push(Inline::Span(span));
        } else if has_class(&span, PANDOC_DELETION) {
            rename_tracked_change(&mut span, PANDOC_DELETION, QUARTO_DELETE);
            out.push(Inline::Span(span));
        } else if has_class(&span, PANDOC_MARK) {
            rename_highlight(&mut span);
            out.push(Inline::Span(span));
        } else if has_class(&span, PANDOC_COMMENT_END) && !span.content.is_empty() {
            // pandoc nests a reply's end inside its parent's: splice the content back in
            // place, so every end marker is a direct list member.
            let nested = std::mem::take(&mut span.content);
            out.push(Inline::Span(span));
            out.extend(nested);
        } else {
            out.push(Inline::Span(span));
        }
    }
    *list = out;
}

// ---------------------------------------------------------------------------
// T4b: same-list pairing
// ---------------------------------------------------------------------------

/// The maximal run of consecutive elements around `at` satisfying `pred`.
fn run_around(list: &[Inline], at: usize, pred: fn(&Inline) -> bool) -> (usize, usize) {
    let mut lo = at;
    while lo > 0 && pred(&list[lo - 1]) {
        lo -= 1;
    }
    let mut hi = at;
    while hi + 1 < list.len() && pred(&list[hi + 1]) {
        hi += 1;
    }
    (lo, hi)
}

fn is_leading_space(inline: &Inline) -> bool {
    matches!(inline, Inline::Space(_) | Inline::SoftBreak(_))
}

/// The first end marker (in list order) that has a start with the same id earlier in the
/// same list: `(start index, end index)`. Taking ends in position order processes nested
/// pairs innermost first and overlapping pairs in document order.
fn next_pair(list: &[Inline]) -> Option<(usize, usize)> {
    for (e, inline) in list.iter().enumerate() {
        if !is_end(inline) {
            continue;
        }
        let id = id_of(inline);
        if let Some(s) = list[..e].iter().position(|c| is_start(c) && id_of(c) == id) {
            return Some((s, e));
        }
    }
    None
}

/// Wrap one comment group: the pair `(s, e)` plus every comment whose start is in `s`'s start
/// run and whose end is in `e`'s end run (replies share their parent's range: I4).
fn wrap_group(list: &mut Inlines, s: usize, e: usize) {
    let (s_lo, s_hi) = run_around(list, s, is_start);
    let (e_lo, e_hi) = run_around(list, e, is_end);
    let mut starts: Vec<usize> = Vec::new();
    let mut ends: Vec<usize> = Vec::new();
    for i in s_lo..=s_hi {
        if let Some(j) = (e_lo..=e_hi).find(|&j| id_of(&list[j]) == id_of(&list[i])) {
            starts.push(i);
            ends.push(j);
        }
    }
    let first = *starts.iter().min().expect("the pair itself");
    let last = *ends.iter().max().expect("the pair itself");
    let comments: Vec<Inline> = starts.iter().map(|&i| comment_span(&list[i])).collect();

    let mut range: Vec<Inline> = Vec::new();
    for (idx, inline) in list[first..=last].iter().enumerate() {
        let abs = first + idx;
        if !starts.contains(&abs) && !ends.contains(&abs) {
            range.push(inline.clone());
        }
    }
    // The qmd reader trims a leading space inside `[…]` (I4): hoist it out in front.
    let hoist = range.iter().take_while(|i| is_leading_space(i)).count();
    let rest = range.split_off(hoist);
    let mut replacement = range; // the hoisted whitespace
    if rest.is_empty() {
        // An empty range: the point comment(s) alone, with no wrapper.
        replacement.extend(comments);
    } else {
        let mut content = rest;
        content.extend(comments);
        replacement.push(new_span(Vec::new(), LinkedHashMap::new(), content));
    }
    list.splice(first..=last, replacement);
}

fn pair_comments_in_list(list: &mut Inlines) {
    if !list.iter().any(is_end) {
        return;
    }
    while let Some((s, e)) = next_pair(list) {
        wrap_group(list, s, e);
    }
}

// ---------------------------------------------------------------------------
// T4c: the I13 fallback, orphans, metadata
// ---------------------------------------------------------------------------

/// Comment markers left after same-list pairing, by id.
struct Leftovers {
    starts: HashSet<String>,
    ends: HashSet<String>,
}

fn collect_leftovers(blocks: &mut Vec<Block>) -> Leftovers {
    let mut found = Leftovers {
        starts: HashSet::new(),
        ends: HashSet::new(),
    };
    for_each_inline_list_mut(blocks, &mut |list| {
        for inline in list.iter() {
            if is_start(inline) {
                found.starts.insert(id_of(inline).to_string());
            } else if is_end(inline) {
                found.ends.insert(id_of(inline).to_string());
            }
        }
    });
    found
}

struct Phase2<'a> {
    left: &'a Leftovers,
    counts: &'a mut TransformCounts,
}

impl Phase2<'_> {
    /// Scan an inline list (and the inline containers under it) for leftover markers.
    /// Block comments go to `sink`; notes are their own blocks.
    fn scan(&mut self, list: &mut Inlines, sink: &mut Inlines) {
        let mut out: Inlines = Vec::with_capacity(list.len());
        // Set after a marker is taken out: the two spaces that flanked it become one.
        let mut gap = false;
        for mut inline in std::mem::take(list) {
            if is_start(&inline) {
                if self.left.ends.contains(id_of(&inline)) {
                    // Its end is somewhere the span can't reach: attach to the block (I13).
                    self.counts.block_comments += 1;
                    sink.push(comment_span(&inline));
                    gap = true;
                } else {
                    self.counts.unmatched_markers += 1;
                    out.push(comment_span(&inline));
                    gap = false;
                }
                continue;
            }
            if is_end(&inline) {
                if !self.left.starts.contains(id_of(&inline)) {
                    self.counts.unmatched_markers += 1;
                }
                gap = true;
                continue;
            }
            if gap
                && matches!(inline, Inline::Space(_))
                && matches!(out.last(), Some(Inline::Space(_)))
            {
                continue;
            }
            gap = false;
            self.descend(&mut inline, sink);
            out.push(inline);
        }
        *list = out;
    }

    fn descend(&mut self, inline: &mut Inline, sink: &mut Inlines) {
        match inline {
            Inline::Emph(x) => self.scan(&mut x.content, sink),
            Inline::Underline(x) => self.scan(&mut x.content, sink),
            Inline::Strong(x) => self.scan(&mut x.content, sink),
            Inline::Strikeout(x) => self.scan(&mut x.content, sink),
            Inline::Superscript(x) => self.scan(&mut x.content, sink),
            Inline::Subscript(x) => self.scan(&mut x.content, sink),
            Inline::SmallCaps(x) => self.scan(&mut x.content, sink),
            Inline::Quoted(x) => self.scan(&mut x.content, sink),
            Inline::Cite(x) => self.scan(&mut x.content, sink),
            Inline::Link(x) => self.scan(&mut x.content, sink),
            Inline::Image(x) => self.scan(&mut x.content, sink),
            Inline::Span(x) => self.scan(&mut x.content, sink),
            Inline::Insert(x) => self.scan(&mut x.content, sink),
            Inline::Delete(x) => self.scan(&mut x.content, sink),
            Inline::Highlight(x) => self.scan(&mut x.content, sink),
            Inline::EditComment(x) => self.scan(&mut x.content, sink),
            Inline::Note(n) => self.blocks(&mut n.content),
            _ => {}
        }
    }

    /// A paragraph-like block's own list: block comments are appended to its top level.
    fn own_list(&mut self, list: &mut Inlines) {
        let mut sink = Vec::new();
        self.scan(list, &mut sink);
        list.extend(sink);
    }

    fn blocks(&mut self, blocks: &mut Vec<Block>) {
        let mut out = Vec::with_capacity(blocks.len());
        for mut block in std::mem::take(blocks) {
            // Comments with no paragraph to take them go in a new `Para` after the block.
            let mut after: Inlines = Vec::new();
            let had_content = matches!(&block, Block::Plain(p) if !p.content.is_empty())
                || matches!(&block, Block::Paragraph(p) if !p.content.is_empty());
            self.block(&mut block, &mut after);
            // A paragraph that held nothing but a removed marker leaves nothing behind.
            let emptied = had_content
                && match &block {
                    Block::Plain(p) => p.content.is_empty(),
                    Block::Paragraph(p) => p.content.is_empty(),
                    _ => false,
                };
            if !emptied {
                out.push(block);
            }
            if !after.is_empty() {
                out.push(Block::Paragraph(Paragraph {
                    content: after,
                    source_info: generated(),
                }));
            }
        }
        *blocks = out;
    }

    fn caption(
        &mut self,
        caption: &mut quarto_pandoc_types::caption::Caption,
        after: &mut Inlines,
    ) {
        if let Some(short) = caption.short.as_mut() {
            self.scan(short, after);
        }
        if let Some(long) = caption.long.as_mut() {
            self.blocks(long);
        }
    }

    fn block(&mut self, block: &mut Block, after: &mut Inlines) {
        match block {
            Block::Plain(p) => self.own_list(&mut p.content),
            Block::Paragraph(p) => self.own_list(&mut p.content),
            Block::Header(h) => self.own_list(&mut h.content),
            Block::LineBlock(lb) => {
                for line in lb.content.iter_mut() {
                    self.scan(line, after);
                }
            }
            Block::BlockQuote(bq) => self.blocks(&mut bq.content),
            Block::OrderedList(ol) => {
                for item in ol.content.iter_mut() {
                    self.blocks(item);
                }
            }
            Block::BulletList(bl) => {
                for item in bl.content.iter_mut() {
                    self.blocks(item);
                }
            }
            Block::DefinitionList(dl) => {
                for (term, defs) in dl.content.iter_mut() {
                    self.scan(term, after);
                    for def in defs.iter_mut() {
                        self.blocks(def);
                    }
                }
            }
            Block::Div(d) => self.blocks(&mut d.content),
            Block::Figure(f) => {
                self.caption(&mut f.caption, after);
                self.blocks(&mut f.content);
            }
            Block::Table(t) => {
                self.caption(&mut t.caption, after);
                for row in t.head.rows.iter_mut().chain(t.foot.rows.iter_mut()) {
                    for cell in row.cells.iter_mut() {
                        self.blocks(&mut cell.content);
                    }
                }
                for body in t.bodies.iter_mut() {
                    for row in body.head.iter_mut().chain(body.body.iter_mut()) {
                        for cell in row.cells.iter_mut() {
                            self.blocks(&mut cell.content);
                        }
                    }
                }
            }
            Block::CaptionBlock(cb) => self.scan(&mut cb.content, after),
            Block::NoteDefinitionPara(n) => self.scan(&mut n.content, after),
            Block::NoteDefinitionFencedBlock(n) => self.blocks(&mut n.content),
            Block::CodeBlock(_)
            | Block::RawBlock(_)
            | Block::HorizontalRule(_)
            | Block::BlockMetadata(_)
            | Block::Custom(_) => {}
        }
    }
}

/// In metadata inlines a comment can't attach to anything: remove the markers and drop the
/// comments, counting each (Q-24-5).
fn drop_comments_from_meta(list: &mut Inlines, counts: &mut TransformCounts) {
    if !list.iter().any(|i| is_start(i) || is_end(i)) {
        return;
    }
    list.retain(|inline| {
        if is_start(inline) {
            counts.block_comments += 1;
            false
        } else {
            !is_end(inline)
        }
    });
}

// ---------------------------------------------------------------------------
// Entry points
// ---------------------------------------------------------------------------

/// T4: track changes, highlights, paragraph marks and comments (I3, I4, I13).
pub fn editorial_marks(pandoc: &mut Pandoc, counts: &mut TransformCounts) {
    // T4a. Renames, paragraph marks and flat comment ends, in the body and the metadata.
    for_each_inline_list_mut(&mut pandoc.blocks, &mut |list| normalize_list(list, counts));
    for_each_meta_inline_list_mut(&mut pandoc.meta, &mut |list| normalize_list(list, counts));

    // T4c (metadata). Nothing in metadata can carry a comment.
    for_each_meta_inline_list_mut(&mut pandoc.meta, &mut |list| {
        drop_comments_from_meta(list, counts)
    });

    // T4b. Pairs whose markers share an inline list.
    for_each_inline_list_mut(&mut pandoc.blocks, &mut pair_comments_in_list);

    // T4c. Pairs that don't (I13), and orphans.
    let left = collect_leftovers(&mut pandoc.blocks);
    if !left.starts.is_empty() || !left.ends.is_empty() {
        Phase2 {
            left: &left,
            counts,
        }
        .blocks(&mut pandoc.blocks);
    }
}

fn list_loses_style(style: &ListNumberStyle, delim: &ListNumberDelim) -> bool {
    match style {
        // `(@)` is the one marker the qmd grammar keeps.
        ListNumberStyle::Example => false,
        ListNumberStyle::Default | ListNumberStyle::Decimal => {
            matches!(delim, ListNumberDelim::TwoParens)
        }
        _ => true,
    }
}

/// T5: count ordered lists whose number style the qmd can't keep (I17), and give a pptx
/// import `format: revealjs` unless the metadata already sets `format` (I6).
pub fn other_transforms(pandoc: &mut Pandoc, format_id: &str, counts: &mut TransformCounts) {
    for_each_block_list_mut(&mut pandoc.blocks, &mut |blocks| {
        for block in blocks.iter() {
            if let Block::OrderedList(ol) = block {
                let (_, style, delim) = &ol.attr;
                if list_loses_style(style, delim) {
                    counts.list_styles += 1;
                }
            }
        }
    });
    if format_id == "pptx" && !pandoc.meta.contains_key("format") {
        pandoc.meta.insert_path(
            &["format"],
            quarto_pandoc_types::config_value::ConfigValue::new_string("revealjs", generated()),
        );
    }
}

/// Both transform groups, for a document read from `format_id`'s pandoc reader.
pub fn apply(mut pandoc: Pandoc, format_id: &str) -> (Pandoc, TransformCounts) {
    let mut counts = TransformCounts::default();
    editorial_marks(&mut pandoc, &mut counts);
    other_transforms(&mut pandoc, format_id, &mut counts);
    (pandoc, counts)
}
