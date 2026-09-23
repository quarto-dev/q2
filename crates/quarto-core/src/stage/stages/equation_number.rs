/*
 * stage/stages/equation_number.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Encode equation numbers the way the document's math renderer needs.
 */

//! Encode each numbered equation's number for the selected math renderer.
//!
//! ## Why this is a stage after user post filters
//!
//! `CrossrefRenderTransform` (Finalization phase, inside
//! `AstTransformsStage`) numbers equations but does not decide how the
//! number is *typeset*: it leaves the `Math` text byte-identical to the
//! source and records the number as the reserved
//! [`EQ_NUMBER_ATTR`](crate::crossref::EQ_NUMBER_ATTR) attribute on the
//! equation span. That encoding is a presentation decision that depends on
//! the format and on `html-math-method` — `amsmath`'s `\tag{N}` is only
//! understood by MathJax and KaTeX; a converter that reads math alone
//! (MathML, Pandoc's, quarto-math's) rejects it — so it is made here, in a
//! stage that runs after `UserFiltersStage::post`. Running after the post
//! filters is the point: a Lua filter can read, rewrite or delete
//! `el.attributes["quarto-eq-number"]` and this stage honours the result.
//! (A Finalization *transform* would run before those filters and consume
//! the attribute first.) `CodeHighlightStage` occupies the same slot for
//! the same reason. Design: `claude-notes/designs/transform-pipeline-phases.md`;
//! plan: `claude-notes/plans/2026-09-21-equation-numbering-and-mathml.md`.
//!
//! ## Encodings
//!
//! | [`NumberEncoding`] | when | effect on `Span[Math(Display, t)]` |
//! |---|---|---|
//! | `TexTag` | HTML-based format, MathJax or KaTeX | `t` → `t\tag{N}` |
//! | `Qquad` | HTML-based, `plain` or an unknown method | `t` → `t \qquad(N)` (Quarto 1's non-JS encoding) |
//! | `Sibling` | HTML-based, `mathml` | `t` untouched; a `Span.quarto-eq-number` with `(N)` follows the math, and the equation span gains `quarto-eq-sibling-number` |
//! | `Writer` | every other format | `t` untouched; the format's writer numbers |
//!
//! In every case the attribute is removed, so it never reaches a writer.
//! A span whose first inline is not `Math(DisplayMath)` (a filter replaced
//! it) only loses the attribute; it is traced, not changed.

use async_trait::async_trait;

use quarto_pandoc_types::attr::AttrSourceInfo;
use quarto_pandoc_types::block::Block;
use quarto_pandoc_types::inline::{Inline, Math, MathType, Span, Str};
use quarto_source_map::SourceInfo;

use crate::ast_walk::for_each_inline_mut;
use crate::crossref::EQ_NUMBER_ATTR;
use crate::format::Format;
use crate::math_method::{MathMethod, MathMethodConfig};
use crate::stage::{
    EventLevel, PipelineData, PipelineDataKind, PipelineError, PipelineStage, StageContext,
};
use crate::trace_event;

/// Class of the sibling label span the `Sibling` encoding appends after
/// the math: `<span class="quarto-eq-number">(1)</span>`.
pub const EQ_NUMBER_LABEL_CLASS: &str = "quarto-eq-number";

/// Modifier class the `Sibling` encoding adds to the equation span so the
/// stylesheet can lay the math and its label out as one row.
pub const EQ_SIBLING_NUMBER_CLASS: &str = "quarto-eq-sibling-number";

/// How an equation number is written into the document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NumberEncoding {
    /// `\tag{N}` appended to the TeX (MathJax, KaTeX).
    TexTag,
    /// ` \qquad(N)` appended to the TeX (no engine reads `\tag`).
    Qquad,
    /// A label span after the math, outside it (MathML).
    Sibling,
    /// Nothing: the format's writer numbers equations itself.
    Writer,
}

impl NumberEncoding {
    /// The encoding for a document rendered to `format` with the given
    /// math method.
    pub fn for_document(format: &Format, method: &MathMethod) -> Self {
        if !format.identifier.is_html_based() {
            return Self::Writer;
        }
        match method {
            MathMethod::Mathjax | MathMethod::Katex => Self::TexTag,
            MathMethod::MathMl => Self::Sibling,
            MathMethod::Plain | MathMethod::Unknown(_) => Self::Qquad,
        }
    }
}

/// Encode every `quarto-eq-number` attribute for the document's renderer
/// and remove it.
pub struct EquationNumberStage;

impl EquationNumberStage {
    pub fn new() -> Self {
        Self
    }
}

impl Default for EquationNumberStage {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait(?Send)]
impl PipelineStage for EquationNumberStage {
    fn name(&self) -> &str {
        "equation-number"
    }

    fn input_kind(&self) -> PipelineDataKind {
        PipelineDataKind::DocumentAst
    }

    fn output_kind(&self) -> PipelineDataKind {
        PipelineDataKind::DocumentAst
    }

    async fn run(
        &self,
        input: PipelineData,
        ctx: &mut StageContext,
    ) -> Result<PipelineData, PipelineError> {
        let PipelineData::DocumentAst(mut doc) = input else {
            return Err(PipelineError::unexpected_input(
                self.name(),
                self.input_kind(),
                input.kind(),
            ));
        };

        let method = MathMethodConfig::from_meta(&doc.ast.meta).method;
        let encoding = NumberEncoding::for_document(&ctx.format, &method);
        let outcome = encode_document(&mut doc.ast.blocks, encoding);

        if outcome.encoded > 0 || outcome.non_canonical > 0 {
            trace_event!(
                ctx,
                EventLevel::Debug,
                "equation-number: {:?} for {} equation(s), {} left as-is (first inline is not display math)",
                encoding,
                outcome.encoded,
                outcome.non_canonical
            );
        }

        Ok(PipelineData::DocumentAst(doc))
    }
}

/// What one pass over a document did.
#[derive(Default)]
pub struct Outcome {
    /// Spans whose number was encoded.
    pub encoded: usize,
    /// Spans that carried the attribute but not `[Math(DisplayMath), …]`;
    /// only the attribute was removed.
    pub non_canonical: usize,
}

/// Apply `encoding` to one equation span that carried `number`. Returns
/// `false` when the span is not the canonical `[Math(DisplayMath), …]`
/// shape, in which case nothing but the attribute changes.
pub fn encode_number(span: &mut Span, number: &str, encoding: NumberEncoding) -> bool {
    let Some(Inline::Math(math)) = span.content.first_mut() else {
        return false;
    };
    if math.math_type != MathType::DisplayMath {
        return false;
    }
    match encoding {
        NumberEncoding::TexTag => append_to_tex(math, &format!("\\tag{{{number}}}")),
        NumberEncoding::Qquad => append_to_tex(math, &format!(" \\qquad({number})")),
        NumberEncoding::Sibling => {
            let source_info = span.source_info.clone();
            span.attr.1.push(EQ_SIBLING_NUMBER_CLASS.to_string());
            span.content.insert(
                1,
                Inline::Span(Span {
                    attr: (
                        String::new(),
                        vec![EQ_NUMBER_LABEL_CLASS.to_string()],
                        Default::default(),
                    ),
                    content: vec![Inline::Str(Str {
                        text: format!("({number})"),
                        source_info: source_info.clone(),
                    })],
                    source_info,
                    attr_source: AttrSourceInfo::empty(),
                }),
            );
        }
        NumberEncoding::Writer => {}
    }
    true
}

/// Append `suffix` to the math text without losing the reader's
/// byte-for-byte mapping of the original text (`Math.text_source`,
/// bd-ieldbghj): the original keeps its provenance and the suffix becomes
/// a synthesized, zero-source piece anchored at the end of the node, the
/// same shape `ProvenanceBuilder` uses for content with no source byte.
fn append_to_tex(math: &mut Math, suffix: &str) {
    math.text_source = math.text_source.take().map(|ts| {
        let node_len = math.source_info.length();
        let synthesized = SourceInfo::substring(math.source_info.clone(), node_len, node_len);
        SourceInfo::concat(vec![(ts, math.text.len()), (synthesized, suffix.len())])
    });
    math.text.push_str(suffix);
}

/// Encode every `quarto-eq-number` attribute under `blocks` and remove it.
pub fn encode_document(blocks: &mut [Block], encoding: NumberEncoding) -> Outcome {
    let mut out = Outcome::default();
    for_each_inline_mut(blocks, &mut |inline| {
        let Inline::Span(span) = inline else {
            return;
        };
        let Some(number) = span.attr.2.remove(EQ_NUMBER_ATTR) else {
            return;
        };
        if encode_number(span, &number, encoding) {
            out.encoded += 1;
        } else {
            out.non_canonical += 1;
        }
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::FormatIdentifier;
    use hashlink::LinkedHashMap;
    use quarto_source_map::FileId;

    fn si() -> SourceInfo {
        SourceInfo::for_test()
    }

    fn math(math_type: MathType, text: &str) -> Inline {
        Inline::Math(Math {
            math_type,
            text: text.to_string(),
            source_info: si(),
            text_source: None,
        })
    }

    /// The span `CrossrefRenderTransform` emits for a numbered equation.
    fn numbered_span(number: &str, content: Vec<Inline>) -> Span {
        let mut kvs = LinkedHashMap::new();
        kvs.insert(EQ_NUMBER_ATTR.to_string(), number.to_string());
        Span {
            attr: (
                "eq-x".to_string(),
                vec!["quarto-math-with-attribute".to_string()],
                kvs,
            ),
            content,
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        }
    }

    fn math_text(span: &Span) -> &str {
        let Inline::Math(m) = &span.content[0] else {
            panic!("first inline is not Math: {:?}", span.content[0]);
        };
        &m.text
    }

    // ── encode_number ──────────────────────────────────────────────

    #[test]
    fn tex_tag_appends_amsmath_tag() {
        let mut span = numbered_span("3", vec![math(MathType::DisplayMath, "e = mc^2")]);
        assert!(encode_number(&mut span, "3", NumberEncoding::TexTag));
        assert_eq!(math_text(&span), "e = mc^2\\tag{3}");
        assert_eq!(span.content.len(), 1);
        assert_eq!(span.attr.1, vec!["quarto-math-with-attribute"]);
    }

    #[test]
    fn qquad_appends_spaced_parenthesized_number() {
        let mut span = numbered_span("3", vec![math(MathType::DisplayMath, "e = mc^2")]);
        assert!(encode_number(&mut span, "3", NumberEncoding::Qquad));
        assert_eq!(math_text(&span), "e = mc^2 \\qquad(3)");
        assert_eq!(span.content.len(), 1);
    }

    #[test]
    fn sibling_appends_label_span_and_modifier_class() {
        let mut span = numbered_span("3", vec![math(MathType::DisplayMath, "e = mc^2")]);
        assert!(encode_number(&mut span, "3", NumberEncoding::Sibling));
        assert_eq!(math_text(&span), "e = mc^2", "the TeX is untouched");
        assert_eq!(
            span.attr.1,
            vec!["quarto-math-with-attribute", EQ_SIBLING_NUMBER_CLASS]
        );
        assert_eq!(span.content.len(), 2);
        let Inline::Span(label) = &span.content[1] else {
            panic!("expected the label span, got {:?}", span.content[1]);
        };
        assert_eq!(label.attr.0, "");
        assert_eq!(label.attr.1, vec![EQ_NUMBER_LABEL_CLASS]);
        assert!(label.attr.2.is_empty());
        let [Inline::Str(s)] = label.content.as_slice() else {
            panic!("label content: {:?}", label.content);
        };
        assert_eq!(s.text, "(3)");
    }

    #[test]
    fn sibling_label_goes_right_after_the_math() {
        let mut span = numbered_span(
            "1",
            vec![
                math(MathType::DisplayMath, "x"),
                Inline::Str(Str {
                    text: "trailing".to_string(),
                    source_info: si(),
                }),
            ],
        );
        assert!(encode_number(&mut span, "1", NumberEncoding::Sibling));
        assert!(matches!(&span.content[0], Inline::Math(_)));
        assert!(matches!(&span.content[1], Inline::Span(l) if l.attr.1 == [EQ_NUMBER_LABEL_CLASS]));
        assert!(matches!(&span.content[2], Inline::Str(s) if s.text == "trailing"));
    }

    #[test]
    fn writer_leaves_the_span_alone() {
        let mut span = numbered_span("3", vec![math(MathType::DisplayMath, "e = mc^2")]);
        let before = span.clone();
        assert!(encode_number(&mut span, "3", NumberEncoding::Writer));
        assert_eq!(span, before);
    }

    /// The appended encoding must not throw away the reader's byte-for-byte
    /// mapping of the math text (bd-ieldbghj): the original keeps its
    /// provenance and the suffix is a synthesized, zero-source piece.
    #[test]
    fn tex_tag_extends_text_source_instead_of_dropping_it() {
        // Source: `$$e = mc^2$$` at file offsets 0..12; text at 2..10.
        let node = SourceInfo::original(FileId(0), 0, 12);
        let text = SourceInfo::original(FileId(0), 2, 10);
        let mut span = numbered_span(
            "1",
            vec![Inline::Math(Math {
                math_type: MathType::DisplayMath,
                text: "e = mc^2".to_string(),
                source_info: node,
                text_source: Some(text),
            })],
        );
        assert!(encode_number(&mut span, "1", NumberEncoding::TexTag));
        let Inline::Math(math) = &span.content[0] else {
            panic!()
        };
        assert_eq!(math.text, "e = mc^2\\tag{1}");
        let ts = math
            .text_source
            .as_ref()
            .expect("tagging keeps the mapping");
        assert_eq!(ts.length(), math.text.len());
        let SourceInfo::Concat { pieces } = ts else {
            panic!("expected a Concat of [original text, synthesized tag], got {ts:?}");
        };
        assert_eq!(pieces.len(), 2);
        assert_eq!(pieces[0].length, "e = mc^2".len());
        assert_eq!(pieces[0].source_info.preimage_in(FileId(0)), Some(2..10));
        assert_eq!(pieces[1].offset_in_concat, "e = mc^2".len());
        assert_eq!(pieces[1].length, "\\tag{1}".len());
        // The tag has no source bytes: a zero-width piece at the node's end.
        assert_eq!(pieces[1].source_info.preimage_in(FileId(0)), Some(12..12));
    }

    #[test]
    fn qquad_extends_text_source_too() {
        let node = SourceInfo::original(FileId(0), 0, 5);
        let text = SourceInfo::original(FileId(0), 2, 3);
        let mut span = numbered_span(
            "2",
            vec![Inline::Math(Math {
                math_type: MathType::DisplayMath,
                text: "x".to_string(),
                source_info: node,
                text_source: Some(text),
            })],
        );
        assert!(encode_number(&mut span, "2", NumberEncoding::Qquad));
        let Inline::Math(math) = &span.content[0] else {
            panic!()
        };
        let ts = math.text_source.as_ref().unwrap();
        assert_eq!(ts.length(), "x \\qquad(2)".len());
        let SourceInfo::Concat { pieces } = ts else {
            panic!()
        };
        assert_eq!(pieces[1].length, " \\qquad(2)".len());
    }

    /// Without a mapping there is nothing to extend, and nothing to invent.
    #[test]
    fn no_text_source_stays_none() {
        let mut span = numbered_span("1", vec![math(MathType::DisplayMath, "x")]);
        assert!(encode_number(&mut span, "1", NumberEncoding::TexTag));
        let Inline::Math(m) = &span.content[0] else {
            panic!()
        };
        assert!(m.text_source.is_none());
    }

    #[test]
    fn labels_are_used_verbatim() {
        // A post filter may have rewritten the number to any text.
        let mut span = numbered_span("A.2", vec![math(MathType::DisplayMath, "x")]);
        assert!(encode_number(&mut span, "A.2", NumberEncoding::TexTag));
        assert_eq!(math_text(&span), "x\\tag{A.2}");
    }

    #[test]
    fn non_canonical_first_inline_is_not_touched() {
        for content in [
            vec![math(MathType::InlineMath, "x")],
            vec![Inline::Str(Str {
                text: "replaced".to_string(),
                source_info: si(),
            })],
            vec![],
        ] {
            for encoding in [
                NumberEncoding::TexTag,
                NumberEncoding::Qquad,
                NumberEncoding::Sibling,
            ] {
                let mut span = numbered_span("1", content.clone());
                let before = span.clone();
                assert!(
                    !encode_number(&mut span, "1", encoding),
                    "{encoding:?} on {content:?}"
                );
                assert_eq!(span, before, "{encoding:?} must not change {content:?}");
            }
        }
    }

    // ── NumberEncoding::for_document ───────────────────────────────

    fn format(identifier: FormatIdentifier) -> Format {
        Format {
            identifier,
            ..Format::html()
        }
    }

    #[test]
    fn html_and_revealjs_pick_by_method() {
        for id in [FormatIdentifier::Html, FormatIdentifier::Revealjs] {
            let f = format(id);
            assert_eq!(
                NumberEncoding::for_document(&f, &MathMethod::Mathjax),
                NumberEncoding::TexTag
            );
            assert_eq!(
                NumberEncoding::for_document(&f, &MathMethod::Katex),
                NumberEncoding::TexTag
            );
            assert_eq!(
                NumberEncoding::for_document(&f, &MathMethod::MathMl),
                NumberEncoding::Sibling
            );
            assert_eq!(
                NumberEncoding::for_document(&f, &MathMethod::Plain),
                NumberEncoding::Qquad
            );
            assert_eq!(
                NumberEncoding::for_document(&f, &MathMethod::Unknown("webtex".into())),
                NumberEncoding::Qquad
            );
        }
    }

    #[test]
    fn non_html_formats_defer_to_the_writer() {
        for id in [
            FormatIdentifier::Pdf,
            FormatIdentifier::Docx,
            FormatIdentifier::Typst,
        ] {
            for method in [MathMethod::Mathjax, MathMethod::MathMl, MathMethod::Plain] {
                assert_eq!(
                    NumberEncoding::for_document(&format(id), &method),
                    NumberEncoding::Writer,
                    "{id:?} {method:?}"
                );
            }
        }
    }

    // ── the walker ─────────────────────────────────────────────────

    fn walk(blocks: &mut [Block], encoding: NumberEncoding) -> Outcome {
        encode_document(blocks, encoding)
    }

    fn para(content: Vec<Inline>) -> Block {
        Block::Paragraph(quarto_pandoc_types::block::Paragraph {
            content,
            source_info: si(),
        })
    }

    #[test]
    fn walker_reaches_spans_nested_in_blocks_inlines_and_custom_slots() {
        use quarto_pandoc_types::block::{BulletList, Div};
        use quarto_pandoc_types::custom::{CustomNode, Slot};
        use quarto_pandoc_types::inline::Emph;

        let eq = || Inline::Span(numbered_span("1", vec![math(MathType::DisplayMath, "x")]));
        let mut custom = CustomNode::new("Anything", quarto_pandoc_types::attr::empty_attr(), si());
        custom
            .slots
            .insert("content".to_string(), Slot::Inlines(vec![eq()]));
        let mut blocks = vec![
            para(vec![eq()]),
            Block::Div(Div {
                attr: quarto_pandoc_types::attr::empty_attr(),
                content: vec![para(vec![Inline::Emph(Emph {
                    content: vec![eq()],
                    source_info: si(),
                })])],
                source_info: si(),
                attr_source: AttrSourceInfo::empty(),
            }),
            Block::BulletList(BulletList {
                content: vec![vec![para(vec![eq()])]],
                source_info: si(),
            }),
            Block::Custom(custom),
        ];
        let out = walk(&mut blocks, NumberEncoding::TexTag);
        assert_eq!(out.encoded, 4);
        assert_eq!(out.non_canonical, 0);

        // Every span lost its attribute and gained the tag.
        let mut seen = 0;
        let mut check = |inline: &Inline| {
            if let Inline::Span(s) = inline {
                assert!(!s.attr.2.contains_key(EQ_NUMBER_ATTR));
                assert_eq!(math_text(s), "x\\tag{1}");
                seen += 1;
            }
        };
        for block in &blocks {
            match block {
                Block::Paragraph(p) => p.content.iter().for_each(&mut check),
                Block::Div(d) => {
                    let Block::Paragraph(p) = &d.content[0] else {
                        panic!()
                    };
                    let Inline::Emph(e) = &p.content[0] else {
                        panic!()
                    };
                    e.content.iter().for_each(&mut check);
                }
                Block::BulletList(bl) => {
                    let Block::Paragraph(p) = &bl.content[0][0] else {
                        panic!()
                    };
                    p.content.iter().for_each(&mut check);
                }
                Block::Custom(c) => {
                    let Some(Slot::Inlines(is)) = c.slots.get("content") else {
                        panic!()
                    };
                    is.iter().for_each(&mut check);
                }
                other => panic!("{other:?}"),
            }
        }
        assert_eq!(seen, 4);
    }

    #[test]
    fn walker_removes_the_attribute_even_when_it_cannot_encode() {
        let mut blocks = vec![para(vec![Inline::Span(numbered_span(
            "1",
            vec![math(MathType::InlineMath, "x")],
        ))])];
        let out = walk(&mut blocks, NumberEncoding::TexTag);
        assert_eq!((out.encoded, out.non_canonical), (0, 1));
        let Block::Paragraph(p) = &blocks[0] else {
            panic!()
        };
        let Inline::Span(s) = &p.content[0] else {
            panic!()
        };
        assert!(!s.attr.2.contains_key(EQ_NUMBER_ATTR));
        assert_eq!(math_text(s), "x");
    }

    #[test]
    fn walker_ignores_spans_without_the_attribute() {
        let mut span = numbered_span("1", vec![math(MathType::DisplayMath, "x")]);
        span.attr.2.clear();
        let mut blocks = vec![para(vec![Inline::Span(span.clone())])];
        let out = walk(&mut blocks, NumberEncoding::TexTag);
        assert_eq!((out.encoded, out.non_canonical), (0, 0));
        let Block::Paragraph(p) = &blocks[0] else {
            panic!()
        };
        assert_eq!(p.content[0], Inline::Span(span));
    }
}
