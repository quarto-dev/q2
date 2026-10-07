/*
 * editorial_marks_ooxml.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! Editorial marks to Word and PowerPoint (document import P6, I23).
//!
//! A Rust port of Gordon's `quarto-ooxml-editorial-marks` extension (both filters: the mark
//! rewrite and the attribution stamping), plus the I4 commented-range shape that the import
//! side (P3) produces. It rewrites q2's editorial marks (`[++ ]`, `[-- ]`, `[!! ]`, `[>> ]`
//! and their `:::` block forms) into the vocabulary pandoc's own docx writer turns into
//! native OOXML (`insertion`, `deletion`, a bare `mark` span, `comment-start` /
//! `comment-end`), or, for pptx, into one raw `<a:r>` run per mark.
//!
//! The transform is always on for docx and pptx and a no-op for every other format. It runs
//! before pandoc, in the transform pipeline, so the native `q2 render --to docx` and the
//! hub's "Download as" (pandoc.wasm) both get it. It is a bucket-B3 service
//! (`pipeline::BUCKETS`): registered for every profile, excluded for none.
//!
//! ## Where this diverges from the extension (decided with Gordon, 2026-10-03)
//!
//! - Comment ids are numeric (`0`, `1`, ...), in the order the `comment-start` spans are
//!   emitted; the mark's own id is ignored. OOXML's `w:id` is an integer.
//! - A comment with no author gets `author="unknown"` (pandoc's own default for tracked
//!   changes); without one pandoc's docx reader drops the comment.
//! - Insert, delete and highlight marks inside a block comment's body are converted. Comment
//!   marks there fold into the message as text and stay unconverted spans.
//! - A pptx block mark's paragraphs are joined with a space, not with nothing.
//!
//! ## Traversal
//!
//! Top-down, written here rather than on `ast_walk`'s post-order list walks: a comment's
//! message is built from its *unconverted* content, so a parent must be decided before its
//! children. The walk does cover every place the list walks do, including `Block::Custom` and
//! `Inline::Custom` slots (for docx/pptx the renderer transforms are excluded, so callouts and
//! tabsets are still custom nodes here) and the metadata's inlines.

use std::sync::Arc;

use hashlink::LinkedHashMap;
use quarto_pandoc_types::attr::AttrSourceInfo;
use quarto_pandoc_types::block::{Block, Div, Paragraph};
use quarto_pandoc_types::inline::{Inline, Inlines, RawInline, Span};
use quarto_pandoc_types::pandoc::Pandoc;
use quarto_source_map::{By, SourceInfo};

use crate::Result;
use crate::ast_walk::{
    ListVisitor, block_children_mut, config_value_lists_mut, inline_children_mut,
};
use crate::attribution::AttributionData;
use crate::editorial_marks::{
    CommentedRange, MarkKind, PANDOC_COMMENT_END, PANDOC_COMMENT_START, PANDOC_DELETION,
    PANDOC_INSERTION, PANDOC_MARK, blocks_to_inlines, commented_range, new_span,
};
use crate::format::FormatIdentifier;
use crate::render::RenderContext;
use crate::transform::{AstTransform, TransformPhase};
use crate::transforms::attribution_render::query_attribution;
use crate::transforms::metadata_normalize::inlines_to_plain_text;

/// pandoc's default author for a tracked change that has none.
const UNKNOWN_AUTHOR: &str = "unknown";

/// Rewrites editorial marks for docx and pptx. See the module documentation.
#[derive(Debug, Default)]
pub struct EditorialMarksOoxmlTransform;

impl EditorialMarksOoxmlTransform {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait::async_trait(?Send)]
impl AstTransform for EditorialMarksOoxmlTransform {
    fn name(&self) -> &str {
        "editorial-marks-ooxml"
    }

    fn phase(&self) -> TransformPhase {
        TransformPhase::Finalization
    }

    async fn transform(&self, ast: &mut Pandoc, ctx: &mut RenderContext) -> Result<()> {
        let target = match ctx.format.identifier {
            FormatIdentifier::Docx => Target::Docx,
            FormatIdentifier::Pptx => Target::Pptx,
            _ => return Ok(()),
        };
        convert_document(ast, target, ctx.attribution_data.clone());
        Ok(())
    }
}

/// Rewrite every editorial mark in `ast` (body and metadata) for `target`.
fn convert_document(ast: &mut Pandoc, target: Target, attribution: Option<Arc<AttributionData>>) {
    let mut conv = Converter {
        target,
        attribution,
        next_comment_id: 0,
        fold_comments: 0,
    };
    // Metadata first: pandoc's writer puts the title block before the body, so a mark in the
    // title is the first in the document.
    config_value_lists_mut(&mut ast.meta, &mut conv);
    conv.block_list(&mut ast.blocks, None);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Target {
    Docx,
    Pptx,
}

fn generated() -> SourceInfo {
    SourceInfo::generated(By::unknown())
}

/// What a block insertion, deletion or highlight wraps each of its leaf paragraphs in.
struct WrapSpec {
    class: &'static str,
    attrs: LinkedHashMap<String, String>,
}

impl WrapSpec {
    fn wrap(&self, content: Inlines) -> Inlines {
        vec![new_span(
            vec![self.class.to_string()],
            self.attrs.clone(),
            content,
        )]
    }
}

struct Converter {
    target: Target,
    attribution: Option<Arc<AttributionData>>,
    next_comment_id: usize,
    /// Above zero while a block comment's body is converted: comment marks there fold into
    /// the message and are left as they are.
    fold_comments: usize,
}

impl ListVisitor for Converter {
    fn inlines(&mut self, list: &mut Inlines) {
        self.inline_list(list);
    }

    fn blocks(&mut self, list: &mut Vec<Block>) {
        self.block_list(list, None);
    }
}

/// The visitor for the blocks under a block insertion, deletion or highlight: every leaf
/// `Para`/`Plain`/`Header` at any depth gets wrapped.
struct Wrapper<'a> {
    conv: &'a mut Converter,
    spec: &'a WrapSpec,
}

impl ListVisitor for Wrapper<'_> {
    fn inlines(&mut self, list: &mut Inlines) {
        self.conv.inline_list(list);
    }

    fn blocks(&mut self, list: &mut Vec<Block>) {
        self.conv.block_list(list, Some(self.spec));
    }
}

/// `time` as `YYYY-MM-DDTHH:MM:SSZ`. Providers differ in the unit (git blame gives seconds);
/// anything above 1e11 would be past the year 5000 as seconds, so it is milliseconds.
fn iso_utc(time: i64) -> Option<String> {
    let secs = if time > 100_000_000_000 {
        time / 1000
    } else {
        time
    };
    let at = time::OffsetDateTime::from_unix_timestamp(secs).ok()?;
    at.format(&time::format_description::well_known::Rfc3339)
        .ok()
}

fn xml_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

impl Converter {
    // ----- attribution (ported from stamp-attribution.lua) ------------------------------

    /// Copy the blamed author and time onto an insert, delete or comment mark, where the
    /// source didn't set them. No data, or no hit: the mark is left alone.
    fn stamp(&self, attrs: &mut LinkedHashMap<String, String>, source_info: &SourceInfo) {
        let Some(data) = &self.attribution else {
            return;
        };
        let Some(record) = query_attribution(source_info, &data.runs, data.file_id) else {
            return;
        };
        if !attrs.contains_key("author")
            && let Some(identity) = data.identities.get(&record.actor)
        {
            attrs.insert("author".to_string(), identity.display_name.clone());
        }
        if !attrs.contains_key("date")
            && let Some(date) = iso_utc(record.time)
        {
            attrs.insert("date".to_string(), date);
        }
    }

    // ----- inline lists ------------------------------------------------------------------

    fn inline_list(&mut self, list: &mut Inlines) {
        let mut out: Inlines = Vec::with_capacity(list.len());
        for inline in std::mem::take(list) {
            match inline {
                Inline::Span(span) => self.span(span, &mut out),
                mut other => {
                    inline_children_mut(&mut other, self);
                    out.push(other);
                }
            }
        }
        *list = out;
    }

    fn span(&mut self, mut span: Span, out: &mut Inlines) {
        match MarkKind::from_first_class(&span.attr.1) {
            // Inside a block comment's body a comment mark is text of the message: it stays in
            // the body as an unconverted span, with its mark classes stripped so no later
            // filter (the extension's, say) mistakes it for a live mark.
            Some(MarkKind::Comment) if self.fold_comments > 0 => {
                strip_mark_classes(&mut span);
                Neutralizer.inlines(&mut span.content);
                out.push(Inline::Span(span));
            }
            Some(kind) => match self.target {
                Target::Docx => self.docx_span(kind, span, out),
                Target::Pptx => self.pptx_span(kind, span, out),
            },
            None => {
                if self.target == Target::Docx
                    && self.fold_comments == 0
                    && let Some(range) = commented_range(&span)
                {
                    self.commented_range(span, range, out);
                } else {
                    self.inline_list(&mut span.content);
                    out.push(Inline::Span(span));
                }
            }
        }
    }

    fn docx_span(&mut self, kind: MarkKind, mut span: Span, out: &mut Inlines) {
        match kind {
            MarkKind::Insert | MarkKind::Delete => {
                self.stamp(&mut span.attr.2, &span.source_info);
                span.attr.1[0] = if kind == MarkKind::Insert {
                    PANDOC_INSERTION
                } else {
                    PANDOC_DELETION
                }
                .to_string();
                span.attr_source = AttrSourceInfo::empty();
                self.inline_list(&mut span.content);
                out.push(Inline::Span(span));
            }
            MarkKind::Highlight => {
                // The writer applies a highlight only to a bare span: no id, classes or
                // attributes.
                self.inline_list(&mut span.content);
                out.push(new_span(
                    vec![PANDOC_MARK.to_string()],
                    LinkedHashMap::new(),
                    span.content,
                ));
            }
            MarkKind::Comment => {
                self.stamp(&mut span.attr.2, &span.source_info);
                let id = self.alloc_comment_id();
                out.push(comment_start(id, &span.attr.2, span.content));
                out.push(comment_end(id));
            }
        }
    }

    /// The I4 shape: `comment-start`s, the range, then the matching `comment-end`s in the
    /// same order. The first comment is the range's, the rest are its replies sharing it.
    fn commented_range(&mut self, span: Span, range: CommentedRange, out: &mut Inlines) {
        let mut content = span.content;
        let mut comments: Vec<Span> = Vec::with_capacity(range.comments.len());
        for &i in range.comments.iter().rev() {
            // Back to front keeps the earlier indices valid.
            let taken = std::mem::replace(
                &mut content[i],
                Inline::Space(quarto_pandoc_types::inline::Space {
                    source_info: generated(),
                }),
            );
            if let Inline::Span(s) = taken {
                comments.push(s);
            }
        }
        comments.reverse();
        content.truncate(range.range_len);

        // Ids go to the outer comments first, so they follow the order the `comment-start`
        // spans have in the final list, nested ranges included.
        let mut ids = Vec::with_capacity(comments.len());
        for comment in comments.iter_mut() {
            self.stamp(&mut comment.attr.2, &comment.source_info);
            ids.push(self.alloc_comment_id());
        }
        self.inline_list(&mut content);
        for (comment, &id) in comments.into_iter().zip(&ids) {
            out.push(comment_start(id, &comment.attr.2, comment.content));
        }
        out.extend(content);
        for &id in &ids {
            out.push(comment_end(id));
        }
    }

    fn pptx_span(&mut self, kind: MarkKind, mut span: Span, out: &mut Inlines) {
        if kind == MarkKind::Comment {
            self.stamp(&mut span.attr.2, &span.source_info);
        }
        let text = inlines_to_plain_text(&span.content);
        out.push(pptx_run(
            kind,
            span.attr.2.get("author").map(String::as_str),
            &text,
        ));
    }

    fn alloc_comment_id(&mut self) -> usize {
        let id = self.next_comment_id;
        self.next_comment_id += 1;
        id
    }

    // ----- block lists -------------------------------------------------------------------

    /// Convert a block list. With `wrap`, every leaf `Para`/`Plain`/`Header` at any depth
    /// has its inlines wrapped in the spec's span (the body of a block insertion, deletion or
    /// highlight); a nested mark Div is converted on its own and not wrapped again.
    fn block_list(&mut self, list: &mut Vec<Block>, wrap: Option<&WrapSpec>) {
        let mut out: Vec<Block> = Vec::with_capacity(list.len());
        for block in std::mem::take(list) {
            match block {
                Block::Div(div) if self.is_live_mark(&div.attr.1) => self.mark_div(div, &mut out),
                Block::Paragraph(mut p) if wrap.is_some() => {
                    self.inline_list(&mut p.content);
                    p.content = wrap.expect("checked").wrap(p.content);
                    out.push(Block::Paragraph(p));
                }
                Block::Plain(mut p) if wrap.is_some() => {
                    self.inline_list(&mut p.content);
                    p.content = wrap.expect("checked").wrap(p.content);
                    out.push(Block::Plain(p));
                }
                Block::Header(mut h) if wrap.is_some() => {
                    self.inline_list(&mut h.content);
                    h.content = wrap.expect("checked").wrap(h.content);
                    out.push(Block::Header(h));
                }
                // A comment Div inside a block comment's body folds: see `span`.
                Block::Div(mut div)
                    if MarkKind::from_first_class(&div.attr.1) == Some(MarkKind::Comment) =>
                {
                    div.attr.1.retain(|c| MarkKind::from_class(c).is_none());
                    div.attr_source = AttrSourceInfo::empty();
                    Neutralizer.blocks(&mut div.content);
                    out.push(Block::Div(div));
                }
                mut other => {
                    match wrap {
                        None => block_children_mut(&mut other, self),
                        Some(spec) => {
                            block_children_mut(&mut other, &mut Wrapper { conv: self, spec })
                        }
                    }
                    out.push(other);
                }
            }
        }
        *list = out;
    }

    /// A Div whose first class is an editorial mark, and which isn't a comment inside a
    /// block comment's body.
    fn is_live_mark(&self, classes: &[String]) -> bool {
        match MarkKind::from_first_class(classes) {
            Some(MarkKind::Comment) => self.fold_comments == 0,
            Some(_) => true,
            None => false,
        }
    }

    fn mark_div(&mut self, mut div: Div, out: &mut Vec<Block>) {
        let kind = MarkKind::from_first_class(&div.attr.1).expect("a mark div");
        if self.target == Target::Pptx {
            if kind == MarkKind::Comment {
                self.stamp(&mut div.attr.2, &div.source_info);
            }
            let text = div
                .content
                .iter()
                .map(|b| inlines_to_plain_text(&blocks_to_inlines(std::slice::from_ref(b))))
                .filter(|t| !t.is_empty())
                .collect::<Vec<_>>()
                .join(" ");
            out.push(Block::Paragraph(Paragraph {
                content: vec![pptx_run(
                    kind,
                    div.attr.2.get("author").map(String::as_str),
                    &text,
                )],
                source_info: generated(),
            }));
            return;
        }
        match kind {
            MarkKind::Insert | MarkKind::Delete => {
                self.stamp(&mut div.attr.2, &div.source_info);
                let mut attrs = LinkedHashMap::new();
                for key in ["author", "date"] {
                    if let Some(v) = div.attr.2.get(key) {
                        attrs.insert(key.to_string(), v.clone());
                    }
                }
                let spec = WrapSpec {
                    class: if kind == MarkKind::Insert {
                        PANDOC_INSERTION
                    } else {
                        PANDOC_DELETION
                    },
                    attrs,
                };
                let mut body = div.content;
                self.block_list(&mut body, Some(&spec));
                out.extend(body);
            }
            MarkKind::Highlight => {
                let spec = WrapSpec {
                    class: PANDOC_MARK,
                    attrs: LinkedHashMap::new(),
                };
                let mut body = div.content;
                self.block_list(&mut body, Some(&spec));
                out.extend(body);
            }
            MarkKind::Comment => self.comment_div(div, out),
        }
    }

    /// A block comment: the message is the body flattened to inlines; the body stays, with
    /// the range markers spliced onto its first and last paragraph.
    fn comment_div(&mut self, mut div: Div, out: &mut Vec<Block>) {
        self.stamp(&mut div.attr.2, &div.source_info);
        let id = self.alloc_comment_id();
        let message = blocks_to_inlines(&div.content);
        let start = comment_start(id, &div.attr.2, message);
        let end = comment_end(id);

        let mut body = div.content;
        self.fold_comments += 1;
        self.block_list(&mut body, None);
        self.fold_comments -= 1;

        if body.is_empty() {
            out.push(para(vec![start, end]));
            return;
        }
        attach(&mut body, 0, start, true);
        let last = body.len() - 1;
        attach(&mut body, last, end, false);
        out.extend(body);
    }
}

/// Strip the editorial-mark classes (`quarto-insert`, ...) from a span, leaving it a plain
/// span with its text.
fn strip_mark_classes(span: &mut Span) {
    span.attr.1.retain(|c| MarkKind::from_class(c).is_none());
    span.attr_source = AttrSourceInfo::empty();
}

/// Folds a subtree into text: strips the mark classes from every span and div under it.
struct Neutralizer;

impl ListVisitor for Neutralizer {
    fn inlines(&mut self, list: &mut Inlines) {
        for inline in list.iter_mut() {
            if let Inline::Span(span) = inline {
                strip_mark_classes(span);
            }
            inline_children_mut(inline, self);
        }
    }

    fn blocks(&mut self, list: &mut Vec<Block>) {
        for block in list.iter_mut() {
            if let Block::Div(div) = block {
                div.attr.1.retain(|c| MarkKind::from_class(c).is_none());
                div.attr_source = AttrSourceInfo::empty();
            }
            block_children_mut(block, self);
        }
    }
}

fn para(content: Inlines) -> Block {
    Block::Paragraph(Paragraph {
        content,
        source_info: generated(),
    })
}

/// Put `marker` at the start or end of the block's inlines, or in a paragraph of its own
/// when the block has none (a `CodeBlock`, a list).
fn attach(blocks: &mut Vec<Block>, idx: usize, marker: Inline, at_start: bool) {
    let list = match &mut blocks[idx] {
        Block::Paragraph(p) => Some(&mut p.content),
        Block::Plain(p) => Some(&mut p.content),
        Block::Header(h) => Some(&mut h.content),
        _ => None,
    };
    match list {
        Some(list) if at_start => list.insert(0, marker),
        Some(list) => list.push(marker),
        None if at_start => blocks.insert(0, para(vec![marker])),
        None => blocks.push(para(vec![marker])),
    }
}

/// `Span ("", ["comment-start"], [id, author, date?]) message`. Other attributes have no
/// OOXML home and are dropped.
fn comment_start(
    id: usize,
    source_attrs: &LinkedHashMap<String, String>,
    mut message: Inlines,
) -> Inline {
    // The message is text: any mark in it (a nested comment, an insertion) is folded in, left
    // unconverted, and its mark classes stripped so no later filter converts it again.
    Neutralizer.inlines(&mut message);
    let mut attrs = LinkedHashMap::new();
    attrs.insert("id".to_string(), id.to_string());
    let author = source_attrs
        .get("author")
        .filter(|a| !a.is_empty())
        .map_or(UNKNOWN_AUTHOR, String::as_str);
    attrs.insert("author".to_string(), author.to_string());
    if let Some(date) = source_attrs.get("date").filter(|d| !d.is_empty()) {
        attrs.insert("date".to_string(), date.clone());
    }
    new_span(vec![PANDOC_COMMENT_START.to_string()], attrs, message)
}

fn comment_end(id: usize) -> Inline {
    let mut attrs = LinkedHashMap::new();
    attrs.insert("id".to_string(), id.to_string());
    new_span(vec![PANDOC_COMMENT_END.to_string()], attrs, Vec::new())
}

/// One raw `<a:r>` run for a pptx mark; `xml:space="preserve"` keeps a comment's leading
/// space.
fn pptx_run(kind: MarkKind, author: Option<&str>, text: &str) -> Inline {
    let (rpr, text) = match kind {
        MarkKind::Insert => ("<a:rPr u=\"sng\"/>", text.to_string()),
        MarkKind::Delete => ("<a:rPr strike=\"sngStrike\"/>", text.to_string()),
        MarkKind::Highlight => (
            "<a:rPr><a:highlight><a:srgbClr val=\"FFFF00\"/></a:highlight></a:rPr>",
            text.to_string(),
        ),
        MarkKind::Comment => (
            "<a:rPr i=\"1\"><a:solidFill><a:srgbClr val=\"C00000\"/></a:solidFill></a:rPr>",
            format!(
                " [{}: {}]",
                author.filter(|a| !a.is_empty()).unwrap_or("comment"),
                text
            ),
        ),
    };
    Inline::RawInline(RawInline {
        format: "openxml".to_string(),
        text: format!(
            "<a:r>{rpr}<a:t xml:space=\"preserve\">{}</a:t></a:r>",
            xml_escape(&text)
        ),
        source_info: generated(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::attribution::{AttributionDataBuilder, Identity};
    use pampa::readers::qmd;
    use quarto_pandoc_types::config_value::ConfigValue;
    use quarto_pandoc_types::custom::{CustomNode, Slot};

    fn parse(qmd_text: &str) -> Pandoc {
        let mut sink = Vec::<u8>::new();
        let (ast, _ctx, _warnings) = qmd::read(
            qmd_text.as_bytes(),
            false,
            "test.qmd",
            &mut sink,
            true,
            None,
        )
        .expect("parse qmd");
        ast
    }

    fn convert(qmd_text: &str, target: Target) -> String {
        let mut ast = parse(qmd_text);
        convert_document(&mut ast, target, None);
        show_blocks(&ast.blocks)
    }

    fn docx(qmd_text: &str) -> String {
        convert(qmd_text, Target::Docx)
    }

    // ----- a terse structural printer: `Span(#id .class k=v)[children]` ------------------

    fn attr_text(attr: &quarto_pandoc_types::attr::Attr) -> String {
        let mut parts: Vec<String> = Vec::new();
        if !attr.0.is_empty() {
            parts.push(format!("#{}", attr.0));
        }
        parts.extend(attr.1.iter().map(|c| format!(".{c}")));
        parts.extend(attr.2.iter().map(|(k, v)| format!("{k}={v}")));
        if parts.is_empty() {
            String::new()
        } else {
            format!("({})", parts.join(" "))
        }
    }

    fn show_inlines(list: &[Inline]) -> String {
        list.iter().map(show_inline).collect::<String>()
    }

    fn show_inline(i: &Inline) -> String {
        match i {
            Inline::Str(s) => s.text.clone(),
            Inline::Space(_) => "_".into(),
            Inline::SoftBreak(_) => "~".into(),
            Inline::LineBreak(_) => "<br>".into(),
            Inline::Code(c) => format!("Code[{}]", c.text),
            Inline::Emph(e) => format!("Emph[{}]", show_inlines(&e.content)),
            Inline::Strong(e) => format!("Strong[{}]", show_inlines(&e.content)),
            Inline::Span(s) => format!("Span{}[{}]", attr_text(&s.attr), show_inlines(&s.content)),
            Inline::RawInline(r) => format!("Raw[{}:{}]", r.format, r.text),
            Inline::Note(n) => format!("Note[{}]", show_blocks(&n.content)),
            Inline::Custom(c) => format!("Custom:{}", c.type_name),
            _ => "?".into(),
        }
    }

    fn show_blocks(list: &[Block]) -> String {
        list.iter().map(show_block).collect::<Vec<_>>().join(" ")
    }

    fn show_block(b: &Block) -> String {
        match b {
            Block::Paragraph(p) => format!("Para[{}]", show_inlines(&p.content)),
            Block::Plain(p) => format!("Plain[{}]", show_inlines(&p.content)),
            Block::Header(h) => format!("Header[{}]", show_inlines(&h.content)),
            Block::CodeBlock(c) => format!("CodeBlock[{}]", c.text.trim_end()),
            Block::Div(d) => format!("Div{}[{}]", attr_text(&d.attr), show_blocks(&d.content)),
            Block::BulletList(l) => format!(
                "Bullets[{}]",
                l.content
                    .iter()
                    .map(|i| show_blocks(i))
                    .collect::<Vec<_>>()
                    .join(" | ")
            ),
            Block::BlockQuote(q) => format!("Quote[{}]", show_blocks(&q.content)),
            Block::Custom(c) => format!(
                "Custom:{}[{}]",
                c.type_name,
                c.slots
                    .iter()
                    .map(|(k, s)| match s {
                        Slot::Blocks(bs) => format!("{k}={}", show_blocks(bs)),
                        Slot::Block(b) => format!("{k}={}", show_block(b)),
                        Slot::Inlines(is) => format!("{k}={}", show_inlines(is)),
                        Slot::Inline(i) => format!("{k}={}", show_inline(i)),
                    })
                    .collect::<Vec<_>>()
                    .join(";")
            ),
            _ => "?".into(),
        }
    }

    // ----- T2a: insert, delete, highlight ---------------------------------------------

    #[test]
    fn inline_insert_and_delete_keep_id_other_classes_and_attributes() {
        assert_eq!(
            docx("[++ x]{#i .extra k=v author=\"A\"} and [-- y]{date=\"2026-01-01\"}\n"),
            "Para[Span(#i .insertion .extra k=v author=A)[x]_and_Span(.deletion date=2026-01-01)[y]]"
        );
    }

    #[test]
    fn inline_highlight_becomes_a_bare_mark_span() {
        assert_eq!(docx("[!! x]{#i .extra k=v}\n"), "Para[Span(.mark)[x]]");
    }

    #[test]
    fn block_insertion_wraps_every_leaf_paragraph_at_any_depth() {
        let out = docx(
            "::: ++ {author=\"Eve\" date=\"2026-05-05T05:05:05Z\"}\n\nOne.\n\n## Head\n\n- a\n- b\n\n> quoted\n\n```\ncode\n```\n\n:::\n",
        );
        let w = "Span(.insertion author=Eve date=2026-05-05T05:05:05Z)";
        assert_eq!(
            out,
            format!(
                "Para[{w}[One.]] Header[{w}[Head]] Bullets[Plain[{w}[a]] | Plain[{w}[b]]] \
                 Quote[Para[{w}[quoted]]] CodeBlock[code]"
            )
        );
    }

    #[test]
    fn block_deletion_and_highlight_use_their_own_wrapper() {
        assert_eq!(
            docx("::: --\n\nGone.\n\n:::\n"),
            "Para[Span(.deletion)[Gone.]]"
        );
        assert_eq!(
            docx("::: !! {#hl .extra}\n\nMarked.\n\n:::\n"),
            "Para[Span(.mark)[Marked.]]"
        );
    }

    #[test]
    fn inline_marks_inside_a_block_insertions_paragraph_are_still_converted() {
        assert_eq!(
            docx("::: ++\n\nkeep [!! this] and [-- that]\n\n:::\n"),
            "Para[Span(.insertion)[keep_Span(.mark)[this]_and_Span(.deletion)[that]]]"
        );
    }

    #[test]
    fn a_nested_mark_div_is_converted_on_its_own_and_not_wrapped_again() {
        // The extension's 03-block-marks nesting: a comment Div inside an insertion Div.
        let out = docx(":::: ++\n\nAdded.\n\n::: >>\n\nWhy?\n\n:::\n\n::::\n");
        assert_eq!(
            out,
            "Para[Span(.insertion)[Added.]] \
             Para[Span(.comment-start id=0 author=unknown)[Why?]Why?Span(.comment-end id=0)[]]"
        );
    }

    // ----- T2b: comments ---------------------------------------------------------------

    const START0: &str = "Span(.comment-start id=0 author=unknown)";

    #[test]
    fn inline_comments_get_numeric_ids_in_order_and_a_default_author() {
        assert_eq!(
            docx("a [>> one] b [>> two]{#comment-id author=\"Ann\" date=\"2026-01-01\" k=v} c\n"),
            "Para[a_Span(.comment-start id=0 author=unknown)[one]Span(.comment-end id=0)[]_b_\
             Span(.comment-start id=1 author=Ann date=2026-01-01)[two]Span(.comment-end id=1)[]_c]"
        );
    }

    #[test]
    fn a_block_comment_keeps_its_body_and_flattens_it_into_the_message() {
        assert_eq!(
            docx("::: >> {author=\"cs\" date=\"2026-09-24\"}\n\nThis is long.\n\nMany.\n\n:::\n"),
            "Para[Span(.comment-start id=0 author=cs date=2026-09-24)[This_is_long.<br>Many.]This_is_long.] \
             Para[Many.Span(.comment-end id=0)[]]"
        );
    }

    #[test]
    fn a_block_comment_on_a_code_block_gets_paragraphs_of_its_own() {
        assert_eq!(
            docx("::: >>\n\n```\nx=1\n```\n\n:::\n"),
            format!("Para[{START0}[Code[x=1]]] CodeBlock[x=1] Para[Span(.comment-end id=0)[]]")
        );
    }

    #[test]
    fn an_empty_block_comment_is_one_paragraph_holding_both_markers() {
        assert_eq!(
            docx("::: >>\n:::\n"),
            format!("Para[{START0}[]Span(.comment-end id=0)[]]")
        );
    }

    #[test]
    fn a_block_comments_body_converts_insertions_but_folds_comments() {
        let out = docx("::: >>\n\nOuter [>> inner] and [++ added] text.\n\n:::\n");
        assert_eq!(
            out,
            format!(
                "Para[{START0}[Outer_Span[inner]_and_Span[added]_text.]\
                 Outer_Span[inner]_and_Span(.insertion)[added]_text.Span(.comment-end id=0)[]]"
            )
        );
        assert_eq!(out.matches("comment-start").count(), 1, "one Word comment");
    }

    #[test]
    fn a_mark_inside_an_inline_comments_message_folds_into_its_text() {
        let out = docx("see [>> why [++ added] and [>> deeper]]\n");
        assert_eq!(out.matches("comment-start").count(), 1);
        assert!(!out.contains(".insertion"), "{out}");
        assert!(out.contains("Span[added]"), "{out}");
        assert!(
            !out.contains("quarto-"),
            "no live mark class is left behind: {out}"
        );
    }

    #[test]
    fn a_plain_container_div_with_a_trailing_comment_is_not_a_mark() {
        let out = docx(
            "::: {.quarto-edit-comment-container}\n\n```\nx = 1\n```\n\n[>> constant?]{author=\"Alice\"}\n\n:::\n",
        );
        assert_eq!(
            out,
            "Div(.quarto-edit-comment-container)[CodeBlock[x = 1] \
             Para[Span(.comment-start id=0 author=Alice)[constant?]Span(.comment-end id=0)[]]]"
        );
    }

    #[test]
    fn marks_in_the_title_and_in_custom_node_slots_are_converted() {
        let mut ast = parse("[++ new] title\n\n[-- gone]\n\n::: ++\n\nOne.\n\nTwo.\n\n:::\n");
        let title = match &ast.blocks[0] {
            Block::Paragraph(p) => p.content.clone(),
            _ => panic!("a paragraph"),
        };
        let inline_para = match ast.blocks[1].clone() {
            Block::Paragraph(p) => p,
            _ => panic!("a paragraph"),
        };
        let div = ast.blocks[2].clone();
        ast.meta.insert_path(
            &["title"],
            ConfigValue::new_inlines(title.clone(), SourceInfo::for_test()),
        );
        let node = CustomNode::new("Callout", Default::default(), SourceInfo::for_test())
            .with_slot("title", Slot::Inlines(title))
            .with_slot("content", Slot::Blocks(vec![Block::Paragraph(inline_para)]))
            .with_slot("one", Slot::Block(Box::new(div)));
        ast.blocks = vec![Block::Custom(node)];
        convert_document(&mut ast, Target::Docx, None);

        let shown = show_blocks(&ast.blocks);
        assert_eq!(
            shown,
            "Custom:Callout[title=Span(.insertion)[new]_title;content=Para[Span(.deletion)[gone]];\
             one=Div[Para[Span(.insertion)[One.]] Para[Span(.insertion)[Two.]]]]"
        );
        let meta_title = ast.meta.get("title").expect("title");
        match &meta_title.value {
            quarto_pandoc_types::config_value::ConfigValueKind::PandocInlines(i) => {
                assert_eq!(show_inlines(i), "Span(.insertion)[new]_title");
            }
            other => panic!("inlines, got {other:?}"),
        }
    }

    #[test]
    fn a_single_paragraph_block_mark_in_a_block_slot_stays_one_block() {
        let mut ast = parse("::: ++\n\nOne.\n\n:::\n");
        let div = ast.blocks[0].clone();
        let node = CustomNode::new("Callout", Default::default(), SourceInfo::for_test())
            .with_slot("one", Slot::Block(Box::new(div)));
        ast.blocks = vec![Block::Custom(node)];
        convert_document(&mut ast, Target::Docx, None);
        assert_eq!(
            show_blocks(&ast.blocks),
            "Custom:Callout[one=Para[Span(.insertion)[One.]]]"
        );
    }

    #[test]
    fn a_comment_div_inside_a_block_comment_folds_without_a_live_class() {
        let out = docx(":::: >>\n\nOuter.\n\n::: >>\n\nInner.\n\n:::\n\n::::\n");
        assert_eq!(out.matches("comment-start").count(), 1, "{out}");
        assert!(!out.contains("quarto-"), "{out}");
        assert!(out.contains("Div[Para[Inner.]]"), "{out}");
    }

    // ----- T3: I4 commented ranges -----------------------------------------------------

    fn end(id: usize) -> String {
        format!("Span(.comment-end id={id})[]")
    }

    #[test]
    fn a_commented_range_is_start_range_end() {
        assert_eq!(
            docx("[a commented range[>> Please.]{author=\"Ann\"}]\n"),
            format!(
                "Para[Span(.comment-start id=0 author=Ann)[Please.]a_commented_range{}]",
                end(0)
            )
        );
    }

    #[test]
    fn replies_share_the_range_with_flat_ends_in_start_order() {
        assert_eq!(
            docx("[range[>> a][>> b]]\n"),
            format!(
                "Para[Span(.comment-start id=0 author=unknown)[a]Span(.comment-start id=1 author=unknown)[b]range{}{}]",
                end(0),
                end(1)
            )
        );
        let three = docx("[range[>> a][>> b][>> c]]\n");
        assert_eq!(three.matches("comment-start").count(), 3);
        assert!(
            three.ends_with(&format!("range{}{}{}]", end(0), end(1), end(2))),
            "{three}"
        );
    }

    #[test]
    fn space_between_the_comments_is_dropped_and_before_them_stays_in_the_range() {
        assert_eq!(
            docx("[range [>> a] [>> b]]\n"),
            format!(
                "Para[Span(.comment-start id=0 author=unknown)[a]Span(.comment-start id=1 author=unknown)[b]range_{}{}]",
                end(0),
                end(1)
            )
        );
    }

    #[test]
    fn a_range_may_start_with_emphasis() {
        assert_eq!(
            docx("[*emph* tail[>> c]]\n"),
            format!("Para[{START0}[c]Emph[emph]_tail{}]", end(0))
        );
    }

    #[test]
    fn nested_ranges_number_the_outer_comment_first() {
        assert_eq!(
            docx("[outer [inner[>> in]] text[>> out]]\n"),
            format!(
                "Para[Span(.comment-start id=0 author=unknown)[out]outer_\
                 Span(.comment-start id=1 author=unknown)[in]inner{}_text{}]",
                end(1),
                end(0)
            )
        );
    }

    #[test]
    fn a_wrapper_with_attributes_or_a_comment_mid_span_is_not_a_range() {
        assert_eq!(
            docx("[range[>> c]]{.cls}\n"),
            format!("Para[Span(.cls)[range{START0}[c]{}]]", end(0))
        );
        assert_eq!(
            docx("[a [>> c] b]\n"),
            format!("Para[Span[a_{START0}[c]{}_b]]", end(0))
        );
    }

    #[test]
    fn a_commented_range_inside_an_insertion_is_converted() {
        assert_eq!(
            docx("[++ [range[>> c]] more]\n"),
            format!("Para[Span(.insertion)[{START0}[c]range{}_more]]", end(0))
        );
    }

    #[test]
    fn a_comment_inside_a_ranges_comment_folds() {
        let out = docx("[range[>> outer [>> inner]]]\n");
        assert_eq!(out.matches("comment-start").count(), 1, "{out}");
        assert!(out.contains("Span[inner]"), "{out}");
        assert!(
            !out.contains("quarto-"),
            "no live mark class is left behind: {out}"
        );
    }

    #[test]
    fn a_range_ending_in_a_point_comment_exports_as_a_reply_group() {
        // The known ambiguity: `[text [>> pt] [>> c]]` is `pt` with reply `c`.
        let out = docx("[text[>> pt][>> c]]\n");
        assert_eq!(
            out,
            format!(
                "Para[Span(.comment-start id=0 author=unknown)[pt]Span(.comment-start id=1 author=unknown)[c]text{}{}]",
                end(0),
                end(1)
            )
        );
    }

    // ----- T4: pptx --------------------------------------------------------------------

    fn run(rpr: &str, text: &str) -> String {
        format!("Raw[openxml:<a:r>{rpr}<a:t xml:space=\"preserve\">{text}</a:t></a:r>]")
    }

    const COMMENT_RPR: &str =
        "<a:rPr i=\"1\"><a:solidFill><a:srgbClr val=\"C00000\"/></a:solidFill></a:rPr>";

    #[test]
    fn pptx_marks_become_raw_runs() {
        let pptx = |q: &str| convert(q, Target::Pptx);
        assert_eq!(
            pptx("[++ x]\n"),
            format!("Para[{}]", run("<a:rPr u=\"sng\"/>", "x"))
        );
        assert_eq!(
            pptx("[-- x]\n"),
            format!("Para[{}]", run("<a:rPr strike=\"sngStrike\"/>", "x"))
        );
        assert_eq!(
            pptx("[!! x]\n"),
            format!(
                "Para[{}]",
                run(
                    "<a:rPr><a:highlight><a:srgbClr val=\"FFFF00\"/></a:highlight></a:rPr>",
                    "x"
                )
            )
        );
        assert_eq!(
            pptx("[>> hi]{author=\"Ann\"}\n"),
            format!("Para[{}]", run(COMMENT_RPR, " [Ann: hi]"))
        );
        assert_eq!(
            pptx("[>> hi]\n"),
            format!("Para[{}]", run(COMMENT_RPR, " [comment: hi]"))
        );
    }

    #[test]
    fn pptx_runs_escape_xml_and_flatten_rich_content() {
        assert_eq!(
            convert("[++ 1 < 2 & *three*]\n", Target::Pptx),
            format!(
                "Para[{}]",
                run("<a:rPr u=\"sng\"/>", "1 &lt; 2 &amp; three")
            )
        );
    }

    #[test]
    fn a_pptx_block_mark_joins_its_paragraphs_with_a_space() {
        assert_eq!(
            convert("::: ++\n\none\n\ntwo\n\n:::\n", Target::Pptx),
            format!("Para[{}]", run("<a:rPr u=\"sng\"/>", "one two"))
        );
    }

    #[test]
    fn a_pptx_commented_range_wrapper_stays_plain_and_its_comments_become_runs() {
        assert_eq!(
            convert("[range[>> c]]\n", Target::Pptx),
            format!("Para[Span[range{}]]", run(COMMENT_RPR, " [comment: c]"))
        );
    }

    // ----- T5: attribution stamping ----------------------------------------------------

    fn data(
        runs: &[(usize, usize, &str, i64)],
        identities: &[(&str, &str)],
    ) -> Arc<AttributionData> {
        let mut b = AttributionDataBuilder::new();
        for (actor, name) in identities {
            b.set_identity(
                actor,
                Identity {
                    display_name: (*name).to_string(),
                    color: "#000000".to_string(),
                },
            );
        }
        for (start, end, actor, time) in runs {
            b.push_run(*start, *end, actor, *time);
        }
        Arc::new(b.build())
    }

    fn stamped(qmd_text: &str, data: Arc<AttributionData>) -> String {
        let mut ast = parse(qmd_text);
        convert_document(&mut ast, Target::Docx, Some(data));
        show_blocks(&ast.blocks)
    }

    const ALICE: &[(&str, &str)] = &[("alice@example.com", "Alice")];
    const WHEN: &str = "2023-11-14T22:13:20Z";

    #[test]
    fn a_mark_gets_the_blamed_name_and_date() {
        let text = "[++ x]\n";
        let d = data(
            &[(0, text.len(), "alice@example.com", 1_700_000_000)],
            ALICE,
        );
        assert_eq!(
            stamped(text, d),
            format!("Para[Span(.insertion author=Alice date={WHEN})[x]]")
        );
    }

    #[test]
    fn a_millisecond_time_converts() {
        let text = "[-- x]\n";
        let d = data(
            &[(0, text.len(), "alice@example.com", 1_700_000_000_000)],
            ALICE,
        );
        assert_eq!(
            stamped(text, d),
            format!("Para[Span(.deletion author=Alice date={WHEN})[x]]")
        );
    }

    #[test]
    fn an_actor_without_an_identity_gets_a_date_only() {
        let text = "[++ x]\n";
        let d = data(
            &[(0, text.len(), "nobody@example.com", 1_700_000_000)],
            ALICE,
        );
        assert_eq!(
            stamped(text, d),
            format!("Para[Span(.insertion date={WHEN})[x]]")
        );
    }

    #[test]
    fn explicit_author_and_date_win() {
        let text = "[++ x]{author=\"Zed\"} [-- y]{date=\"2020-01-01\"}\n";
        let d = data(
            &[(0, text.len(), "alice@example.com", 1_700_000_000)],
            ALICE,
        );
        assert_eq!(
            stamped(text, d),
            format!(
                "Para[Span(.insertion author=Zed date={WHEN})[x]_Span(.deletion date=2020-01-01 author=Alice)[y]]"
            )
        );
    }

    #[test]
    fn a_highlight_is_not_stamped_and_a_comment_is() {
        let text = "[!! x] [>> c]\n";
        let d = data(
            &[(0, text.len(), "alice@example.com", 1_700_000_000)],
            ALICE,
        );
        assert_eq!(
            stamped(text, d),
            format!(
                "Para[Span(.mark)[x]_Span(.comment-start id=0 author=Alice date={WHEN})[c]{}]",
                end(0)
            )
        );
    }

    #[test]
    fn a_block_insertions_wrapping_spans_carry_the_stamped_author() {
        let text = "::: ++\n\none\n\ntwo\n\n:::\n";
        let d = data(
            &[(0, text.len(), "alice@example.com", 1_700_000_000)],
            ALICE,
        );
        let w = format!("Span(.insertion author=Alice date={WHEN})");
        assert_eq!(stamped(text, d), format!("Para[{w}[one]] Para[{w}[two]]"));
    }

    #[test]
    fn no_data_no_hit_or_another_file_leaves_the_mark_alone() {
        let text = "[>> c]\n";
        assert_eq!(docx(text), format!("Para[{START0}[c]{}]", end(0)));
        let elsewhere = data(&[(500, 600, "alice@example.com", 1_700_000_000)], ALICE);
        assert_eq!(
            stamped(text, elsewhere),
            format!("Para[{START0}[c]{}]", end(0))
        );

        let mut other_file = data(
            &[(0, text.len(), "alice@example.com", 1_700_000_000)],
            ALICE,
        );
        Arc::get_mut(&mut other_file).unwrap().file_id = quarto_source_map::FileId(7);
        assert_eq!(
            stamped(text, other_file),
            format!("Para[{START0}[c]{}]", end(0))
        );
    }

    // ----- gating through the transform ------------------------------------------------

    async fn run_transform(format: crate::format::Format, qmd_text: &str) -> String {
        use crate::project::{DocumentInfo, ProjectContext};
        use crate::render::BinaryDependencies;
        let mut ast = parse(qmd_text);
        let project = ProjectContext {
            dir: std::path::PathBuf::from("/project"),
            is_single_file: true,
            files: vec![DocumentInfo::from_path("/project/test.qmd")],
            output_dir: std::path::PathBuf::from("/project"),
            ..Default::default()
        };
        let doc = DocumentInfo::from_path("/project/test.qmd");
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);
        EditorialMarksOoxmlTransform::new()
            .transform(&mut ast, &mut ctx)
            .await
            .unwrap();
        show_blocks(&ast.blocks)
    }

    #[tokio::test]
    async fn the_transform_acts_for_docx_and_pptx_only() {
        use crate::format::{Format, FormatIdentifier};
        let q = "[++ x]\n";
        assert_eq!(
            run_transform(Format::docx(), q).await,
            "Para[Span(.insertion)[x]]"
        );
        let pptx = Format {
            identifier: FormatIdentifier::Pptx,
            target_format: "pptx".to_string(),
            display_name: "PPTX".to_string(),
            output_extension: "pptx".to_string(),
            ..Format::docx()
        };
        assert!(run_transform(pptx, q).await.contains("Raw[openxml:"));
        for other in [Format::html(), Format::pdf()] {
            assert_eq!(
                run_transform(other, q).await,
                "Para[Span(.quarto-insert)[x]]"
            );
        }
    }
}
