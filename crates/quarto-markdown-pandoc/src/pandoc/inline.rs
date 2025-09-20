/*
 * inline.rs
 * Copyright (c) 2025 Posit, PBC
 */

use crate::impl_source_location;
use crate::pandoc::attr::{Attr, is_empty_attr};
use crate::pandoc::block::Blocks;
use crate::pandoc::location::Range;
use crate::pandoc::location::SourceLocation;
use crate::pandoc::location::SourceInfo;
use crate::pandoc::shortcode::Shortcode;

#[derive(Debug, Clone, PartialEq)]
pub enum Inline {
    Str(Str),
    Emph(Emph),
    Underline(Underline),
    Strong(Strong),
    Strikeout(Strikeout),
    Superscript(Superscript),
    Subscript(Subscript),
    SmallCaps(SmallCaps),
    Quoted(Quoted),
    Cite(Cite),
    Code(Code),
    Space(Space),
    SoftBreak(SoftBreak),
    LineBreak(LineBreak),
    Math(Math),
    RawInline(RawInline),
    Link(Link),
    Image(Image),
    Note(Note),
    Span(Span),

    // quarto extensions
    // after desugaring, these nodes should not appear in a document
    Shortcode(Shortcode),
    NoteReference(NoteReference),
    // this is used to represent commonmark attributes in the document in places
    // where they are not directly attached to a block, like in headings and tables
    Attr(Attr),

    // CriticMarkup-like extensions
    Insert(Insert),
    Delete(Delete),
    Highlight(Highlight),
    EditComment(EditComment),
}

pub type Inlines = Vec<Inline>;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum QuoteType {
    SingleQuote,
    DoubleQuote,
}

pub type Target = (String, String);

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum MathType {
    InlineMath,
    DisplayMath,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Str {
    pub text: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Emph {
    pub content: Inlines,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Underline {
    pub content: Inlines,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Strong {
    pub content: Inlines,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Strikeout {
    pub content: Inlines,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Superscript {
    pub content: Inlines,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Subscript {
    pub content: Inlines,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SmallCaps {
    pub content: Inlines,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Quoted {
    pub quote_type: QuoteType,
    pub content: Inlines,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Cite {
    pub citations: Vec<Citation>,
    pub content: Inlines,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Code {
    pub attr: Attr,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Math {
    pub math_type: MathType,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RawInline {
    pub format: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Link {
    pub attr: Attr,
    pub content: Inlines,
    pub target: Target,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Image {
    pub attr: Attr,
    pub content: Inlines,
    pub target: Target,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Note {
    pub content: Blocks,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Span {
    pub attr: Attr,
    pub content: Inlines,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Space {
    pub source_info: SourceInfo,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LineBreak {
    pub source_info: SourceInfo,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SoftBreak {
    pub source_info: SourceInfo,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NoteReference {
    pub id: String,
    pub range: Range,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Citation {
    pub id: String,
    pub prefix: Inlines,
    pub suffix: Inlines,
    pub mode: CitationMode,
    pub note_num: usize,
    pub hash: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CitationMode {
    AuthorInText,
    SuppressAuthor,
    NormalCitation,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Insert {
    pub content: Inlines,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Delete {
    pub content: Inlines,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Highlight {
    pub content: Inlines,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EditComment {
    pub content: Inlines,
}

impl_source_location!(Space, LineBreak, SoftBreak);

pub trait AsInline {
    fn as_inline(self) -> Inline;
}

macro_rules! impl_as_inline {
    ($($type:ident),*) => {
        $(
            impl AsInline for $type {
                fn as_inline(self) -> Inline {
                    Inline::$type(self)
                }
            }
        )*
    };
}

impl AsInline for Inline {
    fn as_inline(self) -> Inline {
        self
    }
}

impl_as_inline!(
    Str,
    Emph,
    Underline,
    Strong,
    Strikeout,
    Superscript,
    Subscript,
    SmallCaps,
    Quoted,
    Cite,
    Code,
    Space,
    SoftBreak,
    LineBreak,
    Math,
    RawInline,
    Link,
    Image,
    Note,
    Span,
    Shortcode,
    NoteReference,
    Attr,
    Insert,
    Delete,
    Highlight,
    EditComment
);

pub fn is_empty_target(target: &Target) -> bool {
    target.0.is_empty() && target.1.is_empty()
}

pub fn make_span_inline(attr: Attr, target: Target, content: Inlines) -> Inline {
    // non-empty targets are never Underline or SmallCaps
    if !is_empty_target(&target) {
        return Inline::Link(Link {
            attr,
            content,
            target,
        });
    }
    if attr.1.contains(&"smallcaps".to_string()) {
        let mut new_attr = attr.clone();
        new_attr.1 = new_attr
            .1
            .into_iter()
            .filter(|s| s != "smallcaps")
            .collect();
        if is_empty_attr(&new_attr) {
            return Inline::SmallCaps(SmallCaps { content });
        }
        let inner_inline = make_span_inline(new_attr, target, content);
        return Inline::SmallCaps(SmallCaps {
            content: vec![inner_inline],
        });
    } else if attr.1.contains(&"ul".to_string()) {
        let mut new_attr = attr.clone();
        new_attr.1 = new_attr.1.into_iter().filter(|s| s != "ul").collect();
        if is_empty_attr(&new_attr) {
            return Inline::Underline(Underline { content });
        }
        let inner_inline = make_span_inline(new_attr, target, content);
        return Inline::Underline(Underline {
            content: vec![inner_inline],
        });
    } else if attr.1.contains(&"underline".to_string()) {
        let mut new_attr = attr.clone();
        new_attr.1 = new_attr
            .1
            .into_iter()
            .filter(|s| s != "underline")
            .collect();
        if is_empty_attr(&new_attr) {
            return Inline::Underline(Underline { content });
        }
        let inner_inline = make_span_inline(new_attr, target, content);
        return Inline::Underline(Underline {
            content: vec![inner_inline],
        });
    }

    return Inline::Span(Span { attr, content });
}

pub fn make_cite_inline(attr: Attr, target: Target, content: Inlines) -> Inline {
    // the traversal here is slightly inefficient because we need
    // to non-destructively check for the goodness of the content
    // before deciding to destructively create a Cite

    let is_semicolon = |inline: &Inline| match &inline {
        Inline::Str(Str { text }) => text == ";",
        _ => false,
    };

    let is_good_cite = content.split(is_semicolon).all(|slice| {
        slice.iter().any(|inline| match inline {
            Inline::Cite(_) => true,
            _ => false,
        })
    });

    if !is_good_cite {
        // if the content is not a good Cite, we backtrack and return a Span
        return make_span_inline(attr, target, content);
    }

    // we can now destructively create a Cite inline
    // from the content.

    // first we split the content along semicolons
    let citations: Vec<Citation> = content
        .split(is_semicolon)
        .map(|slice| {
            let inlines = slice.to_vec();
            let mut cite: Option<Cite> = None;
            let mut prefix: Inlines = vec![];
            let mut suffix: Inlines = vec![];

            // now we build prefix and suffix around a Cite. If there's none, we return None
            inlines.into_iter().for_each(|inline| {
                if cite == None {
                    if let Inline::Cite(c) = inline {
                        cite = Some(c);
                    } else {
                        prefix.push(inline);
                    }
                } else {
                    suffix.push(inline);
                }
            });
            let Some(mut c) = cite else {
                panic!("Cite inline should have at least one citation, found none")
            };
            if c.citations.len() != 1 {
                panic!(
                    "Cite inline should have exactly one citation, found: {:?}",
                    c.citations
                );
            }
            let mut citation = c.citations.pop().unwrap();
            if citation.mode == CitationMode::AuthorInText {
                // if the mode is AuthorInText, it becomes NormalCitation inside
                // a compound cite
                citation.mode = CitationMode::NormalCitation;
            }
            citation.prefix = prefix;
            citation.suffix = suffix;
            citation
        })
        .collect();
    return Inline::Cite(Cite {
        citations,
        content: vec![],
    });
}

fn make_inline_leftover(node: &tree_sitter::Node, input_bytes: &[u8]) -> Inline {
    let text = node.utf8_text(input_bytes).unwrap().to_string();
    Inline::RawInline(RawInline {
        format: "quarto-internal-leftover".to_string(),
        text,
    })
}
