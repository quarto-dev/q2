//! The editorial-marks vocabulary shared by both directions of the document-import epic.
//!
//! Import (`crate::import::transforms`) turns pandoc's reading of Word's tracked changes,
//! highlights and comments into q2 editorial marks (I3, I4); the export transform
//! (`crate::transforms::editorial_marks_ooxml`) turns them back (I23). The class names, the
//! mark kinds, the I4 commented-range shape and its builder live here, so the two directions
//! cannot drift apart.

use hashlink::LinkedHashMap;
use quarto_pandoc_types::attr::AttrSourceInfo;
use quarto_pandoc_types::block::Block;
use quarto_pandoc_types::inline::{Code, Inline, Inlines, LineBreak, RawInline, Span};
use quarto_source_map::{By, SourceInfo};

/// pandoc's docx reader spans (`--track-changes=all`), which are also the classes its docx
/// writer turns into native OOXML.
pub const PANDOC_INSERTION: &str = "insertion";
pub const PANDOC_DELETION: &str = "deletion";
/// Import-only: a paragraph-mark tracked change. Export never emits these.
pub const PANDOC_PARAGRAPH_INSERTION: &str = "paragraph-insertion";
pub const PANDOC_PARAGRAPH_DELETION: &str = "paragraph-deletion";
pub const PANDOC_COMMENT_START: &str = "comment-start";
pub const PANDOC_COMMENT_END: &str = "comment-end";
/// A Word highlight (the writer applies it only to a bare span).
pub const PANDOC_MARK: &str = "mark";

/// The q2 editorial marks (the classes the qmd reader gives `[++ ]`, `[-- ]`, `[!! ]`,
/// `[>> ]`).
pub const QUARTO_INSERT: &str = "quarto-insert";
pub const QUARTO_DELETE: &str = "quarto-delete";
pub const QUARTO_HIGHLIGHT: &str = "quarto-highlight";
pub const QUARTO_EDIT_COMMENT: &str = "quarto-edit-comment";

/// Which editorial mark a span or div is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkKind {
    Insert,
    Delete,
    Highlight,
    Comment,
}

impl MarkKind {
    pub fn from_class(class: &str) -> Option<Self> {
        match class {
            QUARTO_INSERT => Some(Self::Insert),
            QUARTO_DELETE => Some(Self::Delete),
            QUARTO_HIGHLIGHT => Some(Self::Highlight),
            QUARTO_EDIT_COMMENT => Some(Self::Comment),
            _ => None,
        }
    }

    /// A mark is identified by its **first** class.
    pub fn from_first_class(classes: &[String]) -> Option<Self> {
        classes.first().and_then(|c| Self::from_class(c))
    }

    pub fn quarto_class(self) -> &'static str {
        match self {
            Self::Insert => QUARTO_INSERT,
            Self::Delete => QUARTO_DELETE,
            Self::Highlight => QUARTO_HIGHLIGHT,
            Self::Comment => QUARTO_EDIT_COMMENT,
        }
    }
}

fn generated() -> SourceInfo {
    SourceInfo::generated(By::unknown())
}

/// A synthesized span (no source position, no attribute source).
pub fn new_span(
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

// ---------------------------------------------------------------------------
// I4: the commented range
// ---------------------------------------------------------------------------

/// A recognized I4 commented range: the span's content is `content[..range_len]` (the
/// range) followed by the comment spans at `comments` (indices into the content, in order;
/// the first is the range's comment, the rest are its replies). `Space`/`SoftBreak`
/// inlines between the comment spans are not part of the range and not in `comments`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommentedRange {
    pub range_len: usize,
    pub comments: Vec<usize>,
}

fn is_comment_span(inline: &Inline) -> bool {
    matches!(inline, Inline::Span(s)
        if MarkKind::from_first_class(&s.attr.1) == Some(MarkKind::Comment))
}

fn is_gap(inline: &Inline) -> bool {
    matches!(inline, Inline::Space(_) | Inline::SoftBreak(_))
}

/// Recognize an I4 commented range: a plain span (no id, classes or attributes) whose
/// content **ends in** a run of one or more `quarto-edit-comment` spans. A span with
/// attributes, or whose comment isn't trailing, isn't one.
pub fn commented_range(span: &Span) -> Option<CommentedRange> {
    let (id, classes, attrs) = &span.attr;
    if !id.is_empty() || !classes.is_empty() || !attrs.is_empty() {
        return None;
    }
    let content = &span.content;
    let mut comments: Vec<usize> = Vec::new();
    let mut end = content.len();
    loop {
        if end > 0 && is_comment_span(&content[end - 1]) {
            comments.push(end - 1);
            end -= 1;
            continue;
        }
        // Whitespace between two comment spans of the run is dropped; whitespace in front of
        // the run's first comment belongs to the range.
        let mut gap_start = end;
        while gap_start > 0 && is_gap(&content[gap_start - 1]) {
            gap_start -= 1;
        }
        if !comments.is_empty()
            && gap_start < end
            && gap_start > 0
            && is_comment_span(&content[gap_start - 1])
        {
            end = gap_start;
            continue;
        }
        break;
    }
    if comments.is_empty() {
        return None;
    }
    comments.reverse();
    Some(CommentedRange {
        range_len: end,
        comments,
    })
}

/// Build the I4 shape: a plain span holding `range` followed by `comments` (the range's
/// comment, then its replies).
pub fn commented_range_span(mut range: Inlines, comments: Vec<Inline>) -> Inline {
    range.extend(comments);
    new_span(Vec::new(), LinkedHashMap::new(), range)
}

// ---------------------------------------------------------------------------
// pandoc.utils.blocks_to_inlines
// ---------------------------------------------------------------------------

/// `pandoc.utils.blocks_to_inlines(blocks)` with its default separator (a `LineBreak`):
/// the blocks flattened to inlines, one block after another, joined by `LineBreak`.
/// `Plain`, `Para` and `Header` give their inlines; a `LineBlock` its lines joined by
/// `LineBreak`; a `CodeBlock` a `Code`; a `RawBlock` a `RawInline`; a `Div`, `BlockQuote`,
/// `Figure` and note body are flattened recursively; the items of a list and the terms and
/// definitions of a definition list are flattened and concatenated with no separator; a
/// table gives its caption and its cells. A rule, a custom node or metadata gives nothing.
pub fn blocks_to_inlines(blocks: &[Block]) -> Inlines {
    blocks_to_inlines_with_sep(
        blocks,
        &[Inline::LineBreak(LineBreak {
            source_info: generated(),
        })],
    )
}

/// `pandoc.utils.blocks_to_inlines(blocks, sep)`: [`blocks_to_inlines`] with an explicit
/// separator between top-level blocks. Nested structure (list items, definitions, table
/// cells) flattens exactly as in [`blocks_to_inlines`]; only the top-level joints change.
pub fn blocks_to_inlines_with_sep(blocks: &[Block], sep: &[Inline]) -> Inlines {
    let mut out: Inlines = Vec::new();
    for (i, block) in blocks.iter().enumerate() {
        if i > 0 {
            out.extend(sep.iter().cloned());
        }
        out.extend(block_to_inlines(block));
    }
    out
}

fn block_to_inlines(block: &Block) -> Inlines {
    match block {
        Block::Plain(p) => p.content.clone(),
        Block::Paragraph(p) => p.content.clone(),
        Block::Header(h) => h.content.clone(),
        Block::LineBlock(lb) => {
            let mut out = Vec::new();
            for (i, line) in lb.content.iter().enumerate() {
                if i > 0 {
                    out.push(Inline::LineBreak(LineBreak {
                        source_info: generated(),
                    }));
                }
                out.extend(line.iter().cloned());
            }
            out
        }
        Block::CodeBlock(cb) => vec![Inline::Code(Code {
            attr: cb.attr.clone(),
            text: cb.text.clone(),
            source_info: generated(),
            attr_source: AttrSourceInfo::empty(),
        })],
        Block::RawBlock(rb) => vec![Inline::RawInline(RawInline {
            format: rb.format.clone(),
            text: rb.text.clone(),
            source_info: generated(),
        })],
        Block::Div(d) => blocks_to_inlines(&d.content),
        Block::BlockQuote(bq) => blocks_to_inlines(&bq.content),
        Block::Figure(f) => blocks_to_inlines(&f.content),
        Block::NoteDefinitionPara(n) => n.content.clone(),
        Block::NoteDefinitionFencedBlock(n) => blocks_to_inlines(&n.content),
        Block::OrderedList(ol) => ol
            .content
            .iter()
            .flat_map(|i| blocks_to_inlines(i))
            .collect(),
        Block::BulletList(bl) => bl
            .content
            .iter()
            .flat_map(|i| blocks_to_inlines(i))
            .collect(),
        Block::DefinitionList(dl) => {
            let mut out = Vec::new();
            for (term, defs) in &dl.content {
                out.extend(term.iter().cloned());
                for def in defs {
                    out.extend(blocks_to_inlines(def));
                }
            }
            out
        }
        Block::Table(t) => {
            let mut out = Vec::new();
            if let Some(long) = &t.caption.long {
                out.extend(blocks_to_inlines(long));
            }
            let head_foot = t.head.rows.iter().chain(t.foot.rows.iter());
            for row in head_foot {
                for cell in &row.cells {
                    out.extend(blocks_to_inlines(&cell.content));
                }
            }
            for body in &t.bodies {
                for row in body.head.iter().chain(body.body.iter()) {
                    for cell in &row.cells {
                        out.extend(blocks_to_inlines(&cell.content));
                    }
                }
            }
            out
        }
        Block::CaptionBlock(cb) => cb.content.clone(),
        Block::HorizontalRule(_) | Block::BlockMetadata(_) | Block::Custom(_) => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_pandoc_types::block::{BulletList, CodeBlock, Paragraph};
    use quarto_pandoc_types::inline::{Space, Str};

    fn s(t: &str) -> Inline {
        Inline::Str(Str {
            text: t.into(),
            source_info: generated(),
        })
    }

    fn sp() -> Inline {
        Inline::Space(Space {
            source_info: generated(),
        })
    }

    fn para(content: Inlines) -> Block {
        Block::Paragraph(Paragraph {
            content,
            source_info: generated(),
        })
    }

    fn comment(text: &str) -> Inline {
        new_span(
            vec![QUARTO_EDIT_COMMENT.into()],
            LinkedHashMap::new(),
            vec![s(text)],
        )
    }

    fn plain_span(content: Inlines) -> Span {
        match new_span(Vec::new(), LinkedHashMap::new(), content) {
            Inline::Span(s) => s,
            _ => unreachable!(),
        }
    }

    fn describe(inlines: &[Inline]) -> Vec<String> {
        inlines
            .iter()
            .map(|i| match i {
                Inline::Str(x) => x.text.clone(),
                Inline::Space(_) => "_".into(),
                Inline::LineBreak(_) => "LB".into(),
                Inline::Code(c) => format!("Code({})", c.text),
                Inline::Span(_) => "Span".into(),
                _ => "?".into(),
            })
            .collect()
    }

    #[test]
    fn mark_kind_uses_the_first_class_only() {
        let classes = |c: &[&str]| c.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(
            MarkKind::from_first_class(&classes(&["quarto-insert", "x"])),
            Some(MarkKind::Insert)
        );
        assert_eq!(
            MarkKind::from_first_class(&classes(&["x", "quarto-insert"])),
            None
        );
        assert_eq!(MarkKind::from_first_class(&[]), None);
        for kind in [
            MarkKind::Insert,
            MarkKind::Delete,
            MarkKind::Highlight,
            MarkKind::Comment,
        ] {
            assert_eq!(MarkKind::from_class(kind.quarto_class()), Some(kind));
        }
    }

    #[test]
    fn blocks_to_inlines_matches_pandoc_utils() {
        // The verified case: Para, Para, CodeBlock, BulletList -> one LB two LB Code LB li.
        let code = Block::CodeBlock(CodeBlock {
            attr: Default::default(),
            text: "x=1".into(),
            source_info: generated(),
            attr_source: AttrSourceInfo::empty(),
        });
        let list = Block::BulletList(BulletList {
            content: vec![vec![para(vec![s("li")])]],
            source_info: generated(),
        });
        let out = blocks_to_inlines(&[para(vec![s("one")]), para(vec![s("two")]), code, list]);
        assert_eq!(
            describe(&out),
            ["one", "LB", "two", "LB", "Code(x=1)", "LB", "li"]
        );
    }

    #[test]
    fn blocks_to_inlines_concatenates_list_items_and_recurses_into_divs() {
        let list = Block::BulletList(BulletList {
            content: vec![vec![para(vec![s("a")])], vec![para(vec![s("b")])]],
            source_info: generated(),
        });
        let div = Block::Div(quarto_pandoc_types::block::Div {
            attr: Default::default(),
            content: vec![para(vec![s("x")]), para(vec![s("y")])],
            source_info: generated(),
            attr_source: AttrSourceInfo::empty(),
        });
        assert_eq!(describe(&blocks_to_inlines(&[list])), ["a", "b"]);
        assert_eq!(describe(&blocks_to_inlines(&[div])), ["x", "LB", "y"]);
        assert!(blocks_to_inlines(&[]).is_empty());
    }

    #[test]
    fn recognizes_one_comment_after_the_range() {
        let span = plain_span(vec![s("range"), comment("c")]);
        assert_eq!(
            commented_range(&span),
            Some(CommentedRange {
                range_len: 1,
                comments: vec![1]
            })
        );
    }

    #[test]
    fn recognizes_replies_and_drops_space_between_comments() {
        // `[range [>> a] [>> b]]`: the space before the run stays in the range, the one
        // between the comments goes.
        let span = plain_span(vec![s("range"), sp(), comment("a"), sp(), comment("b")]);
        assert_eq!(
            commented_range(&span),
            Some(CommentedRange {
                range_len: 2,
                comments: vec![2, 4]
            })
        );
        let tight = plain_span(vec![s("r"), comment("a"), comment("b"), comment("c")]);
        assert_eq!(
            commented_range(&tight),
            Some(CommentedRange {
                range_len: 1,
                comments: vec![1, 2, 3]
            })
        );
    }

    #[test]
    fn rejects_attributes_a_mid_span_comment_and_no_comment() {
        let mut with_attr = plain_span(vec![s("r"), comment("c")]);
        with_attr.attr.2.insert("k".into(), "v".into());
        assert_eq!(commented_range(&with_attr), None);
        let mut with_class = plain_span(vec![s("r"), comment("c")]);
        with_class.attr.1.push("x".into());
        assert_eq!(commented_range(&with_class), None);
        let mut with_id = plain_span(vec![s("r"), comment("c")]);
        with_id.attr.0 = "i".into();
        assert_eq!(commented_range(&with_id), None);
        // The comment isn't trailing.
        assert_eq!(
            commented_range(&plain_span(vec![comment("c"), s("after")])),
            None
        );
        assert_eq!(
            commented_range(&plain_span(vec![s("a"), comment("c"), sp()])),
            None
        );
        assert_eq!(commented_range(&plain_span(vec![s("just text")])), None);
        assert_eq!(commented_range(&plain_span(Vec::new())), None);
    }

    #[test]
    fn a_range_may_be_empty_and_the_builder_inverts_the_recognizer() {
        let only = plain_span(vec![comment("c")]);
        assert_eq!(
            commented_range(&only),
            Some(CommentedRange {
                range_len: 0,
                comments: vec![0]
            })
        );
        let built = commented_range_span(vec![s("r"), sp()], vec![comment("a"), comment("b")]);
        let Inline::Span(span) = built else {
            panic!("a span")
        };
        assert_eq!(
            commented_range(&span),
            Some(CommentedRange {
                range_len: 2,
                comments: vec![2, 3]
            })
        );
    }
}
