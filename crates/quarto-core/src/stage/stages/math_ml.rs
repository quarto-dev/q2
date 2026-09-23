/*
 * stage/stages/math_ml.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Convert math to MathML at render time (`html-math-method: mathml`).
 */

//! Native MathML for `format: html` (bd-3evfzwal).
//!
//! When the document selects `html-math-method: mathml`, every
//! `Inline::Math` is converted with `quarto_math` (`Target::MathMl`) and
//! replaced by
//!
//! ```html
//! <span class="math inline|display"><math …>…</math></span>
//! ```
//!
//! (as a `Span` around a `RawInline`), the same wrapper the HTML writer
//! emits around TeX, so stylesheets and the preview-parity tooling keep
//! working. Every current browser renders MathML Core natively, so a
//! document whose math all converts ships no MathJax.
//!
//! An expression the converter cannot fully handle (an unknown command, an
//! unbalanced `\left`) is left as `Inline::Math`. `MathJsStage`, which runs
//! next, then sees leftover math and loads MathJax for the page, so the
//! reader still gets rendered math; the author gets the `Q-22-*`
//! diagnostic pointing at the offending characters in the `.qmd`,
//! downgraded to a warning because the page did render. The plan calls
//! this the hybrid fallback (decision 2 of
//! `claude-notes/plans/2026-09-21-equation-numbering-and-mathml.md`).
//!
//! Numbered equations reach this stage with their number already encoded
//! as a sibling label by `EquationNumberStage` (the `Sibling` encoding),
//! so the TeX handed to the converter is the author's, with no `\tag`.
//!
//! Excluded from the q2-preview pipeline, which renders `Inline::Math`
//! client-side with KaTeX.
//!
//! **HTML-based formats only.** `html-math-method` is an HTML option, and
//! it commonly sits in shared metadata of a project that also renders
//! docx or typst. On the Pandoc leg the stage is a no-op (and `math-ml`
//! is on `PANDOC_STAGE_EXCLUDED`), matching Quarto 1, which forwards the
//! key to pandoc, whose non-HTML writers ignore it, and matching
//! `EquationNumberStage`'s `Writer` encoding. Converting there would
//! hand a `RawInline` to the vendored `crossref/equations.lua`, which
//! crashes on it (pandoc exit 83, Q-20-3). No diagnostic is raised: the
//! `html-` prefix is exactly what makes the key safe to share.

use async_trait::async_trait;

use quarto_error_reporting::{DiagnosticKind, DiagnosticMessage};
use quarto_math::convert::{Target, convert};
use quarto_math::normalize::Mode;
use quarto_math::spec::Spec;
use quarto_pandoc_types::attr::AttrSourceInfo;
use quarto_pandoc_types::inline::{Inline, Math, MathType, RawInline, Span};

use crate::ast_walk::for_each_inline_mut;
use crate::format::Format;
use crate::math_method::{MathMethod, MathMethodConfig};
use crate::stage::{
    EventLevel, PipelineData, PipelineDataKind, PipelineError, PipelineStage, StageContext,
};
use crate::trace_event;

/// Convert every `Inline::Math` to MathML when `html-math-method: mathml`.
pub struct MathMlStage;

impl MathMlStage {
    pub fn new() -> Self {
        Self
    }
}

impl Default for MathMlStage {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait(?Send)]
impl PipelineStage for MathMlStage {
    fn name(&self) -> &str {
        "math-ml"
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
        if !applies_to(&ctx.format, &method) {
            return Ok(PipelineData::DocumentAst(doc));
        }

        let outcome = convert_document(&mut doc.ast.blocks);
        ctx.add_diagnostics(outcome.diagnostics);
        if outcome.converted > 0 || outcome.left > 0 {
            trace_event!(
                ctx,
                EventLevel::Debug,
                "math-ml: converted {} expression(s) to MathML, left {} as TeX for MathJax",
                outcome.converted,
                outcome.left
            );
        }

        Ok(PipelineData::DocumentAst(doc))
    }
}

/// Whether the stage converts anything for a document rendered to
/// `format` with `method`: only an HTML-based format that selected
/// `html-math-method: mathml`. Every other format ignores the option (see
/// the module docs).
pub fn applies_to(format: &Format, method: &MathMethod) -> bool {
    format.identifier.is_html_based() && *method == MathMethod::MathMl
}

/// What one pass over a document did.
#[derive(Default)]
pub struct Outcome {
    /// Expressions replaced by MathML.
    pub converted: usize,
    /// Expressions left as `Inline::Math` for the JS fallback.
    pub left: usize,
    /// Every diagnostic the converter raised, located in the `.qmd`;
    /// errors downgraded to warnings (see the module docs).
    pub diagnostics: Vec<DiagnosticMessage>,
}

/// Convert every `Inline::Math` under `blocks` in place.
pub fn convert_document(blocks: &mut [quarto_pandoc_types::block::Block]) -> Outcome {
    let mut outcome = Outcome::default();
    for_each_inline_mut(blocks, &mut |inline| {
        let Inline::Math(math) = inline else {
            return;
        };
        match convert_math(math) {
            (Some(replacement), diagnostics) => {
                outcome.converted += 1;
                outcome.diagnostics.extend(diagnostics);
                *inline = replacement;
            }
            (None, diagnostics) => {
                outcome.left += 1;
                outcome
                    .diagnostics
                    .extend(diagnostics.into_iter().map(downgrade_to_warning));
            }
        }
    });
    outcome
}

/// Convert one expression. `Some` is the replacement inline; `None` means
/// the converter withheld output and the TeX must stay.
fn convert_math(math: &Math) -> (Option<Inline>, Vec<DiagnosticMessage>) {
    let mode = match math.math_type {
        MathType::InlineMath => Mode::Inline,
        MathType::DisplayMath => Mode::Display,
    };
    // The text's own mapping when the reader recorded one (bd-ieldbghj),
    // else the node: diagnostics then still land on the `$…$` block.
    let text_source = math
        .text_source
        .clone()
        .unwrap_or_else(|| math.source_info.clone());
    let conversion = convert(
        &math.text,
        mode,
        Target::MathMl,
        &text_source,
        Spec::builtin(),
    );
    let Some(mathml) = conversion.output else {
        return (None, conversion.diagnostics);
    };
    let class = match math.math_type {
        MathType::InlineMath => "inline",
        MathType::DisplayMath => "display",
    };
    let replacement = Inline::Span(Span {
        attr: (
            String::new(),
            vec!["math".to_string(), class.to_string()],
            Default::default(),
        ),
        content: vec![Inline::RawInline(RawInline {
            format: "html".to_string(),
            text: mathml,
            source_info: math.source_info.clone(),
        })],
        source_info: math.source_info.clone(),
        attr_source: AttrSourceInfo::empty(),
    });
    (Some(replacement), conversion.diagnostics)
}

/// The page still renders (MathJax takes the expression), so a conversion
/// error is a warning to the author, not a failed render.
fn downgrade_to_warning(mut d: DiagnosticMessage) -> DiagnosticMessage {
    if d.kind == DiagnosticKind::Error {
        d.kind = DiagnosticKind::Warning;
    }
    d
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_pandoc_types::block::{Block, Header, Paragraph};
    use quarto_pandoc_types::custom::{CustomNode, Slot};
    use quarto_pandoc_types::inline::Str;
    use quarto_source_map::{FileId, SourceInfo};

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

    fn para(content: Vec<Inline>) -> Block {
        Block::Paragraph(Paragraph {
            content,
            source_info: si(),
        })
    }

    fn raw_html(inline: &Inline) -> &str {
        let Inline::Span(span) = inline else {
            panic!("expected the math span, got {inline:?}");
        };
        assert_eq!(span.attr.0, "");
        assert!(span.attr.2.is_empty());
        let [Inline::RawInline(raw)] = span.content.as_slice() else {
            panic!("expected one RawInline, got {:?}", span.content);
        };
        assert_eq!(raw.format, "html");
        &raw.text
    }

    #[test]
    fn inline_and_display_math_become_math_spans() {
        let mut blocks = vec![para(vec![
            math(MathType::InlineMath, "x^2"),
            Inline::Str(Str {
                text: "and".to_string(),
                source_info: si(),
            }),
            math(MathType::DisplayMath, "\\frac{a}{b}"),
        ])];
        let out = convert_document(&mut blocks);
        assert_eq!((out.converted, out.left), (2, 0));
        assert!(out.diagnostics.is_empty());
        let Block::Paragraph(p) = &blocks[0] else {
            panic!()
        };
        let Inline::Span(inline_span) = &p.content[0] else {
            panic!()
        };
        assert_eq!(inline_span.attr.1, vec!["math", "inline"]);
        let inline_html = raw_html(&p.content[0]);
        assert!(
            inline_html
                .starts_with(r#"<math xmlns="http://www.w3.org/1998/Math/MathML"><semantics>"#)
        );
        assert!(inline_html.contains("<msup><mi>x</mi><mn>2</mn></msup>"));
        assert!(matches!(&p.content[1], Inline::Str(s) if s.text == "and"));
        let Inline::Span(display_span) = &p.content[2] else {
            panic!()
        };
        assert_eq!(display_span.attr.1, vec!["math", "display"]);
        let display_html = raw_html(&p.content[2]);
        assert!(display_html.contains(r#"display="block""#));
        assert!(display_html.contains("<mfrac><mi>a</mi><mi>b</mi></mfrac>"));
    }

    #[test]
    fn an_unconvertible_expression_stays_math_with_a_warning() {
        let mut blocks = vec![para(vec![math(MathType::InlineMath, "x + \\bogus y")])];
        let out = convert_document(&mut blocks);
        assert_eq!((out.converted, out.left), (0, 1));
        let Block::Paragraph(p) = &blocks[0] else {
            panic!()
        };
        assert!(
            matches!(&p.content[0], Inline::Math(m) if m.text == "x + \\bogus y"),
            "the TeX must be untouched for MathJax, got {:?}",
            p.content[0]
        );
        assert_eq!(out.diagnostics.len(), 1);
        assert_eq!(out.diagnostics[0].code.as_deref(), Some("Q-22-1"));
        assert_eq!(out.diagnostics[0].kind, DiagnosticKind::Warning);
    }

    /// A conversion that succeeds with a caveat keeps its warning as-is.
    #[test]
    fn a_warning_on_a_converted_expression_is_kept() {
        // `\not` on a non-symbol is unsupported and produces an Error node...
        // so pick a real warning: ragged rows in an environment.
        let mut blocks = vec![para(vec![math(
            MathType::DisplayMath,
            "\\begin{matrix} a & b \\\\ c \\end{matrix}",
        )])];
        let out = convert_document(&mut blocks);
        assert_eq!((out.converted, out.left), (1, 0));
        assert_eq!(out.diagnostics.len(), 1, "{:?}", out.diagnostics);
        assert_eq!(out.diagnostics[0].kind, DiagnosticKind::Warning);
    }

    #[test]
    fn diagnostics_point_into_the_text_source_when_present() {
        // `$x + \bogus y$` at file bytes 0..14; text at 1..13.
        let node = SourceInfo::original(FileId(0), 0, 14);
        let text = SourceInfo::original(FileId(0), 1, 13);
        let mut blocks = vec![para(vec![Inline::Math(Math {
            math_type: MathType::InlineMath,
            text: "x + \\bogus y".to_string(),
            source_info: node,
            text_source: Some(text),
        })])];
        let out = convert_document(&mut blocks);
        let loc = out.diagnostics[0].location.as_ref().expect("located");
        // `\bogus` is at text offsets 4..10, so file bytes 5..11.
        assert_eq!(loc.preimage_in(FileId(0)), Some(5..11));
    }

    #[test]
    fn math_inside_headers_notes_and_custom_slots_is_converted() {
        let mut custom = CustomNode::new("Anything", quarto_pandoc_types::attr::empty_attr(), si());
        custom.slots.insert(
            "content".to_string(),
            Slot::Inlines(vec![math(MathType::InlineMath, "c^2")]),
        );
        let mut blocks = vec![
            Block::Header(Header {
                level: 1,
                attr: quarto_pandoc_types::attr::empty_attr(),
                content: vec![math(MathType::InlineMath, "h^2")],
                source_info: si(),
                attr_source: AttrSourceInfo::empty(),
            }),
            para(vec![Inline::Note(quarto_pandoc_types::inline::Note {
                content: vec![para(vec![math(MathType::InlineMath, "n^2")])],
                source_info: si(),
            })]),
            Block::Custom(custom),
        ];
        let out = convert_document(&mut blocks);
        assert_eq!((out.converted, out.left), (3, 0));
        let mut seen = Vec::new();
        for_each_inline_mut(&mut blocks, &mut |inline| {
            if let Inline::RawInline(raw) = inline {
                for var in ["h", "n", "c"] {
                    if raw
                        .text
                        .contains(&format!("<msup><mi>{var}</mi><mn>2</mn></msup>"))
                    {
                        seen.push(var);
                    }
                }
            }
        });
        seen.sort_unstable();
        assert_eq!(seen, vec!["c", "h", "n"]);
    }

    /// `html-math-method` is an HTML option: the stage runs for html and
    /// revealjs with `mathml`, and for nothing else.
    #[test]
    fn applies_only_to_html_based_formats_with_mathml() {
        use crate::format::FormatIdentifier;
        let format = |identifier| Format {
            identifier,
            ..Format::html()
        };
        for id in [FormatIdentifier::Html, FormatIdentifier::Revealjs] {
            assert!(applies_to(&format(id), &MathMethod::MathMl), "{id:?}");
            assert!(!applies_to(&format(id), &MathMethod::Mathjax), "{id:?}");
        }
        for id in [
            FormatIdentifier::Docx,
            FormatIdentifier::Pptx,
            FormatIdentifier::Typst,
            FormatIdentifier::Pdf,
        ] {
            assert!(
                !applies_to(&format(id), &MathMethod::MathMl),
                "{id:?} must ignore html-math-method"
            );
        }
    }
}
