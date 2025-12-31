/*
 * inline.rs
 * Copyright (c) 2025 Posit, PBC
 */

use crate::attr::{Attr, AttrSourceInfo, TargetSourceInfo, is_empty_attr};
use crate::block::Blocks;
use crate::custom::CustomNode;
use crate::shortcode::Shortcode;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
    Attr(Attr, AttrSourceInfo),

    // CriticMarkup-like extensions
    Insert(Insert),
    Delete(Delete),
    Highlight(Highlight),
    EditComment(EditComment),

    /// Custom node for Quarto inline extensions
    ///
    /// Parsed from Spans with special class names. When serialized to Pandoc JSON,
    /// these are converted to wrapper Spans with `__quarto_custom_node` class.
    Custom(CustomNode),
}

pub type Inlines = Vec<Inline>;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum QuoteType {
    SingleQuote,
    DoubleQuote,
}

pub type Target = (String, String);

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum MathType {
    InlineMath,
    DisplayMath,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Str {
    pub text: String,
    pub source_info: quarto_source_map::SourceInfo,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Emph {
    pub content: Inlines,
    pub source_info: quarto_source_map::SourceInfo,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Underline {
    pub content: Inlines,
    pub source_info: quarto_source_map::SourceInfo,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Strong {
    pub content: Inlines,
    pub source_info: quarto_source_map::SourceInfo,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Strikeout {
    pub content: Inlines,
    pub source_info: quarto_source_map::SourceInfo,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Superscript {
    pub content: Inlines,
    pub source_info: quarto_source_map::SourceInfo,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Subscript {
    pub content: Inlines,
    pub source_info: quarto_source_map::SourceInfo,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SmallCaps {
    pub content: Inlines,
    pub source_info: quarto_source_map::SourceInfo,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Quoted {
    pub quote_type: QuoteType,
    pub content: Inlines,
    pub source_info: quarto_source_map::SourceInfo,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Cite {
    pub citations: Vec<Citation>,
    pub content: Inlines,
    pub source_info: quarto_source_map::SourceInfo,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Code {
    pub attr: Attr,
    pub text: String,
    pub source_info: quarto_source_map::SourceInfo,
    pub attr_source: AttrSourceInfo,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Math {
    pub math_type: MathType,
    pub text: String,
    pub source_info: quarto_source_map::SourceInfo,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RawInline {
    pub format: String,
    pub text: String,
    pub source_info: quarto_source_map::SourceInfo,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Link {
    pub attr: Attr,
    pub content: Inlines,
    pub target: Target,
    pub source_info: quarto_source_map::SourceInfo,
    pub attr_source: AttrSourceInfo,
    pub target_source: TargetSourceInfo,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Image {
    pub attr: Attr,
    pub content: Inlines,
    pub target: Target,
    pub source_info: quarto_source_map::SourceInfo,
    pub attr_source: AttrSourceInfo,
    pub target_source: TargetSourceInfo,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Note {
    pub content: Blocks,
    pub source_info: quarto_source_map::SourceInfo,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Span {
    pub attr: Attr,
    pub content: Inlines,
    pub source_info: quarto_source_map::SourceInfo,
    pub attr_source: AttrSourceInfo,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Space {
    pub source_info: quarto_source_map::SourceInfo,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LineBreak {
    pub source_info: quarto_source_map::SourceInfo,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SoftBreak {
    pub source_info: quarto_source_map::SourceInfo,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NoteReference {
    pub id: String,
    pub source_info: quarto_source_map::SourceInfo,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Citation {
    pub id: String,
    pub prefix: Inlines,
    pub suffix: Inlines,
    pub mode: CitationMode,
    pub note_num: usize,
    pub hash: usize,
    pub id_source: Option<quarto_source_map::SourceInfo>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CitationMode {
    AuthorInText,
    SuppressAuthor,
    NormalCitation,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Insert {
    pub attr: Attr,
    pub content: Inlines,
    pub source_info: quarto_source_map::SourceInfo,
    pub attr_source: AttrSourceInfo,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Delete {
    pub attr: Attr,
    pub content: Inlines,
    pub source_info: quarto_source_map::SourceInfo,
    pub attr_source: AttrSourceInfo,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Highlight {
    pub attr: Attr,
    pub content: Inlines,
    pub source_info: quarto_source_map::SourceInfo,
    pub attr_source: AttrSourceInfo,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EditComment {
    pub attr: Attr,
    pub content: Inlines,
    pub source_info: quarto_source_map::SourceInfo,
    pub attr_source: AttrSourceInfo,
}

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
    Insert,
    Delete,
    Highlight,
    EditComment
);

// Note: Attr is omitted from the macro because it has two fields (Attr, AttrSourceInfo)
// and the macro doesn't support that pattern. Inline::Attr already IS an inline,
// so it doesn't need AsInline impl - the generic impl for Inline handles it.

pub fn is_empty_target(target: &Target) -> bool {
    target.0.is_empty() && target.1.is_empty()
}

pub fn make_span_inline(
    attr: Attr,
    target: Target,
    content: Inlines,
    source_info: quarto_source_map::SourceInfo,
    attr_source: AttrSourceInfo,
    target_source: TargetSourceInfo,
) -> Inline {
    // non-empty targets are never Underline or SmallCaps
    if !is_empty_target(&target) {
        return Inline::Link(Link {
            attr,
            content,
            target,
            source_info,
            attr_source,
            target_source,
        });
    }
    if attr.1.contains(&"smallcaps".to_string()) {
        let mut new_attr = attr.clone();
        new_attr.1.retain(|s| s != "smallcaps");
        if is_empty_attr(&new_attr) {
            return Inline::SmallCaps(SmallCaps {
                content,
                source_info,
            });
        }
        let inner_inline = make_span_inline(
            new_attr,
            target,
            content,
            source_info.clone(),
            attr_source.clone(),
            target_source.clone(),
        );
        return Inline::SmallCaps(SmallCaps {
            content: vec![inner_inline],
            source_info,
        });
    } else if attr.1.contains(&"ul".to_string()) {
        let mut new_attr = attr.clone();
        new_attr.1.retain(|s| s != "ul");
        if is_empty_attr(&new_attr) {
            return Inline::Underline(Underline {
                content,
                source_info,
            });
        }
        let inner_inline = make_span_inline(
            new_attr,
            target,
            content,
            source_info.clone(),
            attr_source.clone(),
            target_source.clone(),
        );
        return Inline::Underline(Underline {
            content: vec![inner_inline],
            source_info,
        });
    } else if attr.1.contains(&"underline".to_string()) {
        let mut new_attr = attr.clone();
        new_attr.1.retain(|s| s != "underline");
        if is_empty_attr(&new_attr) {
            return Inline::Underline(Underline {
                content,
                source_info,
            });
        }
        let inner_inline = make_span_inline(
            new_attr,
            target,
            content,
            source_info.clone(),
            attr_source.clone(),
            target_source.clone(),
        );
        return Inline::Underline(Underline {
            content: vec![inner_inline],
            source_info,
        });
    }

    Inline::Span(Span {
        attr,
        content,
        source_info,
        attr_source,
    })
}

pub fn make_cite_inline(
    attr: Attr,
    target: Target,
    content: Inlines,
    source_info: quarto_source_map::SourceInfo,
    attr_source: AttrSourceInfo,
    target_source: TargetSourceInfo,
) -> Inline {
    // the traversal here is slightly inefficient because we need
    // to non-destructively check for the goodness of the content
    // before deciding to destructively create a Cite

    let is_semicolon = |inline: &Inline| match &inline {
        Inline::Str(Str { text, .. }) => text == ";",
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
        return make_span_inline(
            attr,
            target,
            content,
            source_info,
            attr_source,
            target_source,
        );
    }

    // we can now destructively create a Cite inline
    // from the content.

    // first we split the content along semicolons
    let citations: Vec<Citation> = content
        .split(is_semicolon)
        .flat_map(|slice| {
            let inlines = slice.to_vec();
            let mut cite: Option<Cite> = None;
            let mut prefix: Inlines = vec![];
            let mut suffix: Inlines = vec![];

            // now we build prefix and suffix around a Cite. If there's none, we return None
            for inline in inlines.into_iter() {
                if cite.is_none() {
                    if let Inline::Cite(c) = inline {
                        cite = Some(c);
                    } else {
                        prefix.push(inline);
                    }
                } else {
                    suffix.push(inline);
                }
            }
            let Some(mut c) = cite else {
                panic!("Cite inline should have at least one citation, found none")
            };

            // Handle the case where a Cite already has multiple citations
            // This can happen when citation syntax appears in contexts like tables
            // where the parser creates a Cite with multiple citations
            if c.citations.len() == 1 {
                // Simple case: one citation, apply prefix and suffix directly
                let mut citation = c.citations.pop().unwrap();
                if citation.mode == CitationMode::AuthorInText {
                    // if the mode is AuthorInText, it becomes NormalCitation inside
                    // a compound cite
                    citation.mode = CitationMode::NormalCitation;
                }
                citation.prefix = prefix;
                citation.suffix = suffix;
                vec![citation]
            } else {
                // Complex case: multiple citations already present
                // Apply prefix to the first citation and suffix to the last
                let num_citations = c.citations.len();
                for (i, citation) in c.citations.iter_mut().enumerate() {
                    if citation.mode == CitationMode::AuthorInText {
                        citation.mode = CitationMode::NormalCitation;
                    }
                    if i == 0 {
                        // Prepend prefix to the first citation's prefix
                        let mut new_prefix = prefix.clone();
                        new_prefix.extend(citation.prefix.clone());
                        citation.prefix = new_prefix;
                    }
                    if i == num_citations - 1 {
                        // Append suffix to the last citation's suffix
                        citation.suffix.extend(suffix.clone());
                    }
                }
                // Return all citations from this slice
                c.citations
            }
        })
        .collect();
    Inline::Cite(Cite {
        citations,
        content: vec![],
        source_info,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use hashlink::LinkedHashMap;

    fn dummy_source_info() -> quarto_source_map::SourceInfo {
        quarto_source_map::SourceInfo::from_range(
            quarto_source_map::FileId(0),
            quarto_source_map::Range {
                start: quarto_source_map::Location {
                    offset: 0,
                    row: 0,
                    column: 0,
                },
                end: quarto_source_map::Location {
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

    fn make_space() -> Inline {
        Inline::Space(Space {
            source_info: dummy_source_info(),
        })
    }

    fn make_citation(id: &str, prefix: Inlines, suffix: Inlines) -> Citation {
        Citation {
            id: id.to_string(),
            prefix,
            suffix,
            mode: CitationMode::NormalCitation,
            note_num: 0,
            hash: 0,
            id_source: None,
        }
    }

    #[test]
    fn test_make_cite_inline_with_multiple_citations() {
        // Test case: a Cite inline that already contains multiple citations
        // This simulates what happens when the parser encounters citation syntax
        // in unsupported contexts (e.g., grid tables)

        // Create a Cite with two citations already in it
        let multi_cite = Inline::Cite(Cite {
            citations: vec![
                make_citation(
                    "knuth1984",
                    vec![],
                    vec![make_str(","), make_space(), make_str("pp. 33-35")],
                ),
                make_citation(
                    "wickham2015",
                    vec![make_space(), make_str("also"), make_space()],
                    vec![make_str(","), make_space(), make_str("chap. 1")],
                ),
            ],
            content: vec![],
            source_info: dummy_source_info(),
        });

        // Now call make_cite_inline with content that includes this multi-citation Cite
        // along with a prefix "see"
        let content = vec![make_str("see"), make_space(), multi_cite];

        let result = make_cite_inline(
            (String::new(), vec![], LinkedHashMap::new()),
            (String::new(), String::new()),
            content,
            dummy_source_info(),
            AttrSourceInfo::empty(),
            TargetSourceInfo::empty(),
        );

        // Verify the result is a Cite
        match result {
            Inline::Cite(cite) => {
                // Should have 2 citations
                assert_eq!(cite.citations.len(), 2);

                // First citation should have the prefix "see " prepended
                assert_eq!(cite.citations[0].id, "knuth1984");
                assert_eq!(cite.citations[0].prefix.len(), 2);
                match &cite.citations[0].prefix[0] {
                    Inline::Str(s) => assert_eq!(s.text, "see"),
                    _ => panic!("Expected Str"),
                }

                // Second citation should have its original prefix intact
                assert_eq!(cite.citations[1].id, "wickham2015");
                assert_eq!(cite.citations[1].prefix.len(), 3);
            }
            _ => panic!("Expected Cite inline, got: {:?}", result),
        }
    }

    #[test]
    fn test_make_cite_inline_with_single_citation_still_works() {
        // Test that the normal case (single citation) still works
        let single_cite = Inline::Cite(Cite {
            citations: vec![make_citation("knuth1984", vec![], vec![])],
            content: vec![],
            source_info: dummy_source_info(),
        });

        let content = vec![
            make_str("see"),
            make_space(),
            single_cite,
            make_str(","),
            make_space(),
            make_str("pp. 33"),
        ];

        let result = make_cite_inline(
            (String::new(), vec![], LinkedHashMap::new()),
            (String::new(), String::new()),
            content,
            dummy_source_info(),
            AttrSourceInfo::empty(),
            TargetSourceInfo::empty(),
        );

        match result {
            Inline::Cite(cite) => {
                assert_eq!(cite.citations.len(), 1);
                assert_eq!(cite.citations[0].id, "knuth1984");
                // Prefix should be "see "
                assert_eq!(cite.citations[0].prefix.len(), 2);
                // Suffix should be ", pp. 33"
                assert_eq!(cite.citations[0].suffix.len(), 3);
            }
            _ => panic!("Expected Cite inline"),
        }
    }
}
