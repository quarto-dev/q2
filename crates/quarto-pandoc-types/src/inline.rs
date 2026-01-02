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

    let is_good_cite = content
        .split(is_semicolon)
        .all(|slice| slice.iter().any(|inline| matches!(inline, Inline::Cite(_))));

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
            for inline in inlines {
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

    // === is_empty_target tests ===

    #[test]
    fn test_is_empty_target_both_empty() {
        let target = (String::new(), String::new());
        assert!(is_empty_target(&target));
    }

    #[test]
    fn test_is_empty_target_url_only() {
        let target = ("https://example.com".to_string(), String::new());
        assert!(!is_empty_target(&target));
    }

    #[test]
    fn test_is_empty_target_title_only() {
        let target = (String::new(), "A title".to_string());
        assert!(!is_empty_target(&target));
    }

    #[test]
    fn test_is_empty_target_both_present() {
        let target = ("https://example.com".to_string(), "A title".to_string());
        assert!(!is_empty_target(&target));
    }

    // === make_span_inline tests ===

    #[test]
    fn test_make_span_inline_with_link_target() {
        let attr = (String::new(), vec![], LinkedHashMap::new());
        let target = ("https://example.com".to_string(), "Title".to_string());
        let content = vec![make_str("click here")];

        let result = make_span_inline(
            attr,
            target,
            content,
            dummy_source_info(),
            AttrSourceInfo::empty(),
            TargetSourceInfo::empty(),
        );

        match result {
            Inline::Link(link) => {
                assert_eq!(link.target.0, "https://example.com");
                assert_eq!(link.target.1, "Title");
                assert_eq!(link.content.len(), 1);
            }
            _ => panic!("Expected Link inline"),
        }
    }

    #[test]
    fn test_make_span_inline_smallcaps_empty_attr() {
        let attr = (
            String::new(),
            vec!["smallcaps".to_string()],
            LinkedHashMap::new(),
        );
        let target = (String::new(), String::new());
        let content = vec![make_str("text")];

        let result = make_span_inline(
            attr,
            target,
            content,
            dummy_source_info(),
            AttrSourceInfo::empty(),
            TargetSourceInfo::empty(),
        );

        match result {
            Inline::SmallCaps(sc) => {
                assert_eq!(sc.content.len(), 1);
                match &sc.content[0] {
                    Inline::Str(s) => assert_eq!(s.text, "text"),
                    _ => panic!("Expected Str"),
                }
            }
            _ => panic!("Expected SmallCaps inline, got {:?}", result),
        }
    }

    #[test]
    fn test_make_span_inline_smallcaps_with_remaining_attr() {
        let attr = (
            "myid".to_string(),
            vec!["smallcaps".to_string()],
            LinkedHashMap::new(),
        );
        let target = (String::new(), String::new());
        let content = vec![make_str("text")];

        let result = make_span_inline(
            attr,
            target,
            content,
            dummy_source_info(),
            AttrSourceInfo::empty(),
            TargetSourceInfo::empty(),
        );

        match result {
            Inline::SmallCaps(sc) => {
                // SmallCaps should wrap an inner Span with the remaining attributes
                assert_eq!(sc.content.len(), 1);
                match &sc.content[0] {
                    Inline::Span(span) => {
                        assert_eq!(span.attr.0, "myid");
                    }
                    _ => panic!("Expected inner Span"),
                }
            }
            _ => panic!("Expected SmallCaps inline"),
        }
    }

    #[test]
    fn test_make_span_inline_underline_ul_class() {
        let attr = (String::new(), vec!["ul".to_string()], LinkedHashMap::new());
        let target = (String::new(), String::new());
        let content = vec![make_str("underlined")];

        let result = make_span_inline(
            attr,
            target,
            content,
            dummy_source_info(),
            AttrSourceInfo::empty(),
            TargetSourceInfo::empty(),
        );

        match result {
            Inline::Underline(ul) => {
                assert_eq!(ul.content.len(), 1);
            }
            _ => panic!("Expected Underline inline"),
        }
    }

    #[test]
    fn test_make_span_inline_underline_class() {
        let attr = (
            String::new(),
            vec!["underline".to_string()],
            LinkedHashMap::new(),
        );
        let target = (String::new(), String::new());
        let content = vec![make_str("underlined")];

        let result = make_span_inline(
            attr,
            target,
            content,
            dummy_source_info(),
            AttrSourceInfo::empty(),
            TargetSourceInfo::empty(),
        );

        match result {
            Inline::Underline(ul) => {
                assert_eq!(ul.content.len(), 1);
            }
            _ => panic!("Expected Underline inline"),
        }
    }

    #[test]
    fn test_make_span_inline_underline_with_remaining_attr() {
        let attr = (
            "myid".to_string(),
            vec!["ul".to_string()],
            LinkedHashMap::new(),
        );
        let target = (String::new(), String::new());
        let content = vec![make_str("text")];

        let result = make_span_inline(
            attr,
            target,
            content,
            dummy_source_info(),
            AttrSourceInfo::empty(),
            TargetSourceInfo::empty(),
        );

        match result {
            Inline::Underline(ul) => {
                // Underline should wrap an inner Span with remaining attributes
                assert_eq!(ul.content.len(), 1);
                match &ul.content[0] {
                    Inline::Span(span) => {
                        assert_eq!(span.attr.0, "myid");
                    }
                    _ => panic!("Expected inner Span"),
                }
            }
            _ => panic!("Expected Underline inline"),
        }
    }

    #[test]
    fn test_make_span_inline_plain_span() {
        let attr = (
            "id".to_string(),
            vec!["class1".to_string()],
            LinkedHashMap::new(),
        );
        let target = (String::new(), String::new());
        let content = vec![make_str("text")];

        let result = make_span_inline(
            attr,
            target,
            content,
            dummy_source_info(),
            AttrSourceInfo::empty(),
            TargetSourceInfo::empty(),
        );

        match result {
            Inline::Span(span) => {
                assert_eq!(span.attr.0, "id");
                assert_eq!(span.attr.1, vec!["class1".to_string()]);
            }
            _ => panic!("Expected Span inline"),
        }
    }

    // === make_cite_inline with non-cite content tests ===

    #[test]
    fn test_make_cite_inline_fallback_to_span() {
        // Content that doesn't have a Cite should fall back to make_span_inline
        let content = vec![make_str("just text")];

        let result = make_cite_inline(
            (String::new(), vec![], LinkedHashMap::new()),
            (String::new(), String::new()),
            content,
            dummy_source_info(),
            AttrSourceInfo::empty(),
            TargetSourceInfo::empty(),
        );

        match result {
            Inline::Span(_) => {}
            _ => panic!("Expected Span inline from fallback"),
        }
    }

    #[test]
    fn test_make_cite_inline_with_semicolon_separator() {
        // Test multiple citations separated by semicolons
        let cite1 = Inline::Cite(Cite {
            citations: vec![make_citation("ref1", vec![], vec![])],
            content: vec![],
            source_info: dummy_source_info(),
        });
        let cite2 = Inline::Cite(Cite {
            citations: vec![make_citation("ref2", vec![], vec![])],
            content: vec![],
            source_info: dummy_source_info(),
        });

        let content = vec![cite1, make_str(";"), cite2];

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
                assert_eq!(cite.citations.len(), 2);
                assert_eq!(cite.citations[0].id, "ref1");
                assert_eq!(cite.citations[1].id, "ref2");
            }
            _ => panic!("Expected Cite inline"),
        }
    }

    #[test]
    fn test_make_cite_inline_author_in_text_converted() {
        // AuthorInText mode should be converted to NormalCitation
        let citation = Citation {
            id: "author2020".to_string(),
            prefix: vec![],
            suffix: vec![],
            mode: CitationMode::AuthorInText,
            note_num: 0,
            hash: 0,
            id_source: None,
        };
        let cite = Inline::Cite(Cite {
            citations: vec![citation],
            content: vec![],
            source_info: dummy_source_info(),
        });

        let content = vec![cite];

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
                assert_eq!(cite.citations[0].mode, CitationMode::NormalCitation);
            }
            _ => panic!("Expected Cite inline"),
        }
    }

    // === AsInline trait tests ===

    #[test]
    fn test_as_inline_str() {
        let s = Str {
            text: "hello".to_string(),
            source_info: dummy_source_info(),
        };
        let inline = s.as_inline();
        match inline {
            Inline::Str(Str { text, .. }) => assert_eq!(text, "hello"),
            _ => panic!("Expected Str"),
        }
    }

    #[test]
    fn test_as_inline_space() {
        let space = Space {
            source_info: dummy_source_info(),
        };
        let inline = space.as_inline();
        assert!(matches!(inline, Inline::Space(_)));
    }

    #[test]
    fn test_as_inline_linebreak() {
        let lb = LineBreak {
            source_info: dummy_source_info(),
        };
        let inline = lb.as_inline();
        assert!(matches!(inline, Inline::LineBreak(_)));
    }

    #[test]
    fn test_as_inline_softbreak() {
        let sb = SoftBreak {
            source_info: dummy_source_info(),
        };
        let inline = sb.as_inline();
        assert!(matches!(inline, Inline::SoftBreak(_)));
    }

    #[test]
    fn test_as_inline_emph() {
        let emph = Emph {
            content: vec![],
            source_info: dummy_source_info(),
        };
        let inline = emph.as_inline();
        assert!(matches!(inline, Inline::Emph(_)));
    }

    #[test]
    fn test_as_inline_strong() {
        let strong = Strong {
            content: vec![],
            source_info: dummy_source_info(),
        };
        let inline = strong.as_inline();
        assert!(matches!(inline, Inline::Strong(_)));
    }

    #[test]
    fn test_as_inline_identity() {
        // Inline already IS an inline, so as_inline should return self
        let original = make_str("test");
        let result = original.clone().as_inline();
        assert_eq!(original, result);
    }

    // === QuoteType tests ===

    #[test]
    fn test_quote_type_eq() {
        assert_eq!(QuoteType::SingleQuote, QuoteType::SingleQuote);
        assert_eq!(QuoteType::DoubleQuote, QuoteType::DoubleQuote);
        assert_ne!(QuoteType::SingleQuote, QuoteType::DoubleQuote);
    }

    #[test]
    fn test_quote_type_ord() {
        // Test ordering (SingleQuote < DoubleQuote based on enum order)
        assert!(QuoteType::SingleQuote < QuoteType::DoubleQuote);
    }

    #[test]
    fn test_quote_type_clone() {
        let qt = QuoteType::SingleQuote;
        let cloned = qt.clone();
        assert_eq!(qt, cloned);
    }

    // === MathType tests ===

    #[test]
    fn test_math_type_eq() {
        assert_eq!(MathType::InlineMath, MathType::InlineMath);
        assert_eq!(MathType::DisplayMath, MathType::DisplayMath);
        assert_ne!(MathType::InlineMath, MathType::DisplayMath);
    }

    #[test]
    fn test_math_type_ord() {
        assert!(MathType::InlineMath < MathType::DisplayMath);
    }

    #[test]
    fn test_math_type_clone() {
        let mt = MathType::DisplayMath;
        let cloned = mt.clone();
        assert_eq!(mt, cloned);
    }

    // === CitationMode tests ===

    #[test]
    fn test_citation_mode_eq() {
        assert_eq!(CitationMode::AuthorInText, CitationMode::AuthorInText);
        assert_eq!(CitationMode::SuppressAuthor, CitationMode::SuppressAuthor);
        assert_eq!(CitationMode::NormalCitation, CitationMode::NormalCitation);
        assert_ne!(CitationMode::AuthorInText, CitationMode::NormalCitation);
    }

    #[test]
    fn test_citation_mode_copy() {
        let mode = CitationMode::AuthorInText;
        let copied = mode; // Copy trait
        assert_eq!(mode, copied);
    }

    // === Struct construction and derive tests ===

    #[test]
    fn test_code_construction() {
        let code = Code {
            attr: (
                "id".to_string(),
                vec!["rust".to_string()],
                LinkedHashMap::new(),
            ),
            text: "fn main() {}".to_string(),
            source_info: dummy_source_info(),
            attr_source: AttrSourceInfo::empty(),
        };
        assert_eq!(code.text, "fn main() {}");
        assert_eq!(code.attr.0, "id");
    }

    #[test]
    fn test_math_construction() {
        let math = Math {
            math_type: MathType::DisplayMath,
            text: "E = mc^2".to_string(),
            source_info: dummy_source_info(),
        };
        assert_eq!(math.text, "E = mc^2");
        assert_eq!(math.math_type, MathType::DisplayMath);
    }

    #[test]
    fn test_raw_inline_construction() {
        let raw = RawInline {
            format: "html".to_string(),
            text: "<span>test</span>".to_string(),
            source_info: dummy_source_info(),
        };
        assert_eq!(raw.format, "html");
        assert_eq!(raw.text, "<span>test</span>");
    }

    #[test]
    fn test_quoted_construction() {
        let quoted = Quoted {
            quote_type: QuoteType::DoubleQuote,
            content: vec![make_str("hello")],
            source_info: dummy_source_info(),
        };
        assert_eq!(quoted.quote_type, QuoteType::DoubleQuote);
        assert_eq!(quoted.content.len(), 1);
    }

    #[test]
    fn test_note_reference_construction() {
        let note_ref = NoteReference {
            id: "fn1".to_string(),
            source_info: dummy_source_info(),
        };
        assert_eq!(note_ref.id, "fn1");
    }

    #[test]
    fn test_image_construction() {
        let img = Image {
            attr: (String::new(), vec![], LinkedHashMap::new()),
            content: vec![make_str("alt text")],
            target: ("image.png".to_string(), "Image title".to_string()),
            source_info: dummy_source_info(),
            attr_source: AttrSourceInfo::empty(),
            target_source: TargetSourceInfo::empty(),
        };
        assert_eq!(img.target.0, "image.png");
        assert_eq!(img.content.len(), 1);
    }

    #[test]
    fn test_insert_construction() {
        let insert = Insert {
            attr: (String::new(), vec![], LinkedHashMap::new()),
            content: vec![make_str("added")],
            source_info: dummy_source_info(),
            attr_source: AttrSourceInfo::empty(),
        };
        assert_eq!(insert.content.len(), 1);
    }

    #[test]
    fn test_delete_construction() {
        let delete = Delete {
            attr: (String::new(), vec![], LinkedHashMap::new()),
            content: vec![make_str("removed")],
            source_info: dummy_source_info(),
            attr_source: AttrSourceInfo::empty(),
        };
        assert_eq!(delete.content.len(), 1);
    }

    #[test]
    fn test_highlight_construction() {
        let highlight = Highlight {
            attr: (String::new(), vec![], LinkedHashMap::new()),
            content: vec![make_str("highlighted")],
            source_info: dummy_source_info(),
            attr_source: AttrSourceInfo::empty(),
        };
        assert_eq!(highlight.content.len(), 1);
    }

    #[test]
    fn test_edit_comment_construction() {
        let comment = EditComment {
            attr: (String::new(), vec![], LinkedHashMap::new()),
            content: vec![make_str("comment")],
            source_info: dummy_source_info(),
            attr_source: AttrSourceInfo::empty(),
        };
        assert_eq!(comment.content.len(), 1);
    }

    #[test]
    fn test_citation_construction() {
        let citation = Citation {
            id: "author2020".to_string(),
            prefix: vec![make_str("see")],
            suffix: vec![make_str("p. 1")],
            mode: CitationMode::SuppressAuthor,
            note_num: 5,
            hash: 12345,
            id_source: Some(dummy_source_info()),
        };
        assert_eq!(citation.id, "author2020");
        assert_eq!(citation.mode, CitationMode::SuppressAuthor);
        assert_eq!(citation.note_num, 5);
        assert!(citation.id_source.is_some());
    }

    // === Serialization tests ===

    #[test]
    fn test_inline_str_serialize() {
        let s = Str {
            text: "hello".to_string(),
            source_info: dummy_source_info(),
        };
        let inline = Inline::Str(s);
        let json = serde_json::to_string(&inline).unwrap();
        assert!(json.contains("hello"));
    }

    #[test]
    fn test_inline_roundtrip() {
        // Test serialize then deserialize
        let s = Str {
            text: "world".to_string(),
            source_info: dummy_source_info(),
        };
        let inline = Inline::Str(s);
        let json = serde_json::to_string(&inline).unwrap();
        let deserialized: Inline = serde_json::from_str(&json).unwrap();
        match deserialized {
            Inline::Str(s) => assert_eq!(s.text, "world"),
            _ => panic!("Expected Str"),
        }
    }

    // === Clone and Debug tests ===

    #[test]
    fn test_link_clone() {
        let link = Link {
            attr: ("id".to_string(), vec![], LinkedHashMap::new()),
            content: vec![make_str("link text")],
            target: ("url".to_string(), "title".to_string()),
            source_info: dummy_source_info(),
            attr_source: AttrSourceInfo::empty(),
            target_source: TargetSourceInfo::empty(),
        };
        let cloned = link.clone();
        assert_eq!(link.attr.0, cloned.attr.0);
        assert_eq!(link.target.0, cloned.target.0);
    }

    #[test]
    fn test_inline_debug() {
        let inline = make_str("test");
        let debug = format!("{:?}", inline);
        assert!(debug.contains("Str"));
        assert!(debug.contains("test"));
    }

    #[test]
    fn test_note_construction() {
        use crate::block::Plain;

        let note = Note {
            content: vec![crate::Block::Plain(Plain {
                content: vec![make_str("note content")],
                source_info: dummy_source_info(),
            })],
            source_info: dummy_source_info(),
        };
        assert_eq!(note.content.len(), 1);
    }
}
