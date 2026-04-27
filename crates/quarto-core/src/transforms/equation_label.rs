/*
 * transforms/equation_label.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Sugaring transform for labelled display equations.
 */

//! Sugaring transform for display equations with crossref labels.
//!
//! pampa wraps `$$ ... $$ {#eq-xxx}` as:
//!
//! ```text
//! Span(id="eq-xxx", classes=["quarto-math-with-attribute"], [Math(DisplayMath, text)])
//! ```
//!
//! This transform converts such Spans into `Inline::Custom(CustomNode("Equation"))`
//! carrying the standard crossref triple (`ref_type`, `kind`, `identifier`) in
//! `plain_data`, so the shared crossref indexer and resolver handle equations
//! uniformly alongside block-level crossref targets.
//!
//! Runs in the **normalization phase**, after `FloatRefTargetSugarTransform` and
//! before `CrossrefIndexTransform`.

use quarto_pandoc_types::block::{Block, Blocks};
use quarto_pandoc_types::custom::{CustomNode, Slot};
use quarto_pandoc_types::inline::{Inline, Inlines, MathType, Span};
use quarto_pandoc_types::pandoc::Pandoc;
use serde_json::json;

use crate::Result;
use crate::crossref::EQUATION;
use crate::render::RenderContext;
use crate::transform::AstTransform;

/// The CSS class pampa adds to `Span` nodes wrapping math with a trailing
/// attribute specifier (e.g. `$$ ... $$ {#eq-foo}`).
const MATH_WITH_ATTR_CLASS: &str = "quarto-math-with-attribute";

/// Transform that converts `Span.quarto-math-with-attribute` wrapping
/// `DisplayMath` into `CustomNode("Equation")` for crossref indexing.
pub struct EquationLabelTransform;

impl EquationLabelTransform {
    pub fn new() -> Self {
        Self
    }
}

impl Default for EquationLabelTransform {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait(?Send)]
impl AstTransform for EquationLabelTransform {
    fn name(&self) -> &str {
        "equation-label"
    }

    async fn transform(&self, ast: &mut Pandoc, _ctx: &mut RenderContext) -> Result<()> {
        transform_blocks(&mut ast.blocks);
        Ok(())
    }
}

fn transform_blocks(blocks: &mut Blocks) {
    for block in blocks.iter_mut() {
        transform_block(block);
    }
}

fn transform_block(block: &mut Block) {
    match block {
        Block::Paragraph(p) => transform_inlines(&mut p.content),
        Block::Plain(p) => transform_inlines(&mut p.content),
        Block::Header(h) => transform_inlines(&mut h.content),
        Block::BlockQuote(bq) => transform_blocks(&mut bq.content),
        Block::Div(div) => transform_blocks(&mut div.content),
        Block::Figure(fig) => {
            transform_blocks(&mut fig.content);
            if let Some(long) = fig.caption.long.as_mut() {
                transform_blocks(long);
            }
            if let Some(short) = fig.caption.short.as_mut() {
                transform_inlines(short);
            }
        }
        Block::OrderedList(ol) => {
            for item in &mut ol.content {
                transform_blocks(item);
            }
        }
        Block::BulletList(bl) => {
            for item in &mut bl.content {
                transform_blocks(item);
            }
        }
        Block::DefinitionList(dl) => {
            for (_term, defs) in &mut dl.content {
                for def in defs {
                    transform_blocks(def);
                }
            }
        }
        Block::LineBlock(lb) => {
            for line in &mut lb.content {
                transform_inlines(line);
            }
        }
        Block::Custom(node) => {
            for (_k, slot) in node.slots.iter_mut() {
                match slot {
                    Slot::Block(b) => transform_block(b),
                    Slot::Blocks(bs) => transform_blocks(bs),
                    Slot::Inline(i) => transform_inline(i),
                    Slot::Inlines(is) => transform_inlines(is),
                }
            }
        }
        _ => {}
    }
}

fn transform_inlines(inlines: &mut Inlines) {
    for inline in inlines.iter_mut() {
        transform_inline(inline);
    }
}

fn transform_inline(inline: &mut Inline) {
    // Recurse into container inlines first.
    match inline {
        Inline::Emph(e) => transform_inlines(&mut e.content),
        Inline::Underline(u) => transform_inlines(&mut u.content),
        Inline::Strong(s) => transform_inlines(&mut s.content),
        Inline::Strikeout(s) => transform_inlines(&mut s.content),
        Inline::Superscript(s) => transform_inlines(&mut s.content),
        Inline::Subscript(s) => transform_inlines(&mut s.content),
        Inline::SmallCaps(s) => transform_inlines(&mut s.content),
        Inline::Quoted(q) => transform_inlines(&mut q.content),
        Inline::Link(l) => transform_inlines(&mut l.content),
        Inline::Image(i) => transform_inlines(&mut i.content),
        Inline::Note(n) => transform_blocks(&mut n.content),
        Inline::Span(s) => transform_inlines(&mut s.content),
        Inline::Insert(i) => transform_inlines(&mut i.content),
        Inline::Delete(d) => transform_inlines(&mut d.content),
        Inline::Highlight(h) => transform_inlines(&mut h.content),
        Inline::Custom(node) => {
            for (_k, slot) in node.slots.iter_mut() {
                match slot {
                    Slot::Block(b) => transform_block(b),
                    Slot::Blocks(bs) => transform_blocks(bs),
                    Slot::Inline(i) => transform_inline(i),
                    Slot::Inlines(is) => transform_inlines(is),
                }
            }
        }
        _ => {}
    }

    // Check if this Span is a labelled display equation.
    if let Inline::Span(span) = inline {
        if let Some(node) = try_convert_equation(span) {
            *inline = Inline::Custom(node);
        }
    }
}

/// If `span` is a `Span.quarto-math-with-attribute` containing a single
/// `DisplayMath` inline and whose id starts with `eq-`, convert it to
/// the canonical `CustomNode("Equation")`.
fn try_convert_equation(span: &mut Span) -> Option<CustomNode> {
    let id = &span.attr.0;
    let classes = &span.attr.1;

    // Must have the marker class.
    if !classes.iter().any(|c| c == MATH_WITH_ATTR_CLASS) {
        return None;
    }

    // Must have an id starting with "eq-".
    if !id.starts_with("eq-") || id.len() <= 3 {
        return None;
    }

    // Must contain exactly one DisplayMath inline.
    if span.content.len() != 1 {
        return None;
    }
    if !matches!(
        &span.content[0],
        Inline::Math(m) if m.math_type == MathType::DisplayMath
    ) {
        return None;
    }

    // Take ownership of the math inline.
    let math_inline = std::mem::replace(
        &mut span.content[0],
        Inline::Str(quarto_pandoc_types::inline::Str {
            text: String::new(),
            source_info: span.source_info.clone(),
        }),
    );

    let identifier = id.clone();
    let source_info = span.source_info.clone();
    let attr = span.attr.clone();

    let mut node = CustomNode::new(EQUATION, attr, source_info);

    // Standard crossref triple.
    let mut data = serde_json::Map::new();
    data.insert("ref_type".into(), json!("eq"));
    data.insert("kind".into(), json!("Equation"));
    data.insert("identifier".into(), json!(identifier));
    node.plain_data = serde_json::Value::Object(data);

    // Store the math content in a slot.
    node.slots
        .insert("content".into(), Slot::Inlines(vec![math_inline]));

    Some(node)
}

#[cfg(test)]
mod tests {
    use super::*;
    use hashlink::LinkedHashMap;
    use quarto_pandoc_types::attr::AttrSourceInfo;
    use quarto_pandoc_types::block::Paragraph;
    use quarto_pandoc_types::inline::{Math, Str};
    use quarto_source_map::{FileId, SourceInfo};

    fn si() -> SourceInfo {
        SourceInfo::original(FileId(0), 0, 0)
    }

    fn display_math_span(id: &str, text: &str) -> Inline {
        Inline::Span(Span {
            attr: (
                id.to_string(),
                vec![MATH_WITH_ATTR_CLASS.to_string()],
                LinkedHashMap::new(),
            ),
            content: vec![Inline::Math(Math {
                math_type: MathType::DisplayMath,
                text: text.to_string(),
                source_info: si(),
            })],
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        })
    }

    #[test]
    fn converts_labelled_display_math() {
        let mut inline = display_math_span("eq-einstein", "\ne = mc^2\n");
        transform_inline(&mut inline);

        let Inline::Custom(node) = &inline else {
            panic!("expected Custom, got {:?}", inline);
        };
        assert_eq!(node.type_name, EQUATION);
        assert_eq!(node.attr.0, "eq-einstein");
        assert_eq!(node.plain_data["ref_type"], "eq");
        assert_eq!(node.plain_data["kind"], "Equation");
        assert_eq!(node.plain_data["identifier"], "eq-einstein");

        // Content slot has the original DisplayMath.
        let Slot::Inlines(content) = node.slots.get("content").unwrap() else {
            panic!("expected Inlines slot");
        };
        assert_eq!(content.len(), 1);
        assert!(matches!(
            &content[0],
            Inline::Math(m) if m.math_type == MathType::DisplayMath
        ));
    }

    #[test]
    fn leaves_non_equation_span_alone() {
        // Span with quarto-math-with-attribute but id doesn't start with eq-.
        let mut inline = Inline::Span(Span {
            attr: (
                "fig-plot".to_string(),
                vec![MATH_WITH_ATTR_CLASS.to_string()],
                LinkedHashMap::new(),
            ),
            content: vec![Inline::Math(Math {
                math_type: MathType::DisplayMath,
                text: "x^2".to_string(),
                source_info: si(),
            })],
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        });
        transform_inline(&mut inline);
        assert!(matches!(inline, Inline::Span(_)));
    }

    #[test]
    fn leaves_inline_math_alone() {
        // InlineMath (not DisplayMath) should not be converted.
        let mut inline = Inline::Span(Span {
            attr: (
                "eq-inline".to_string(),
                vec![MATH_WITH_ATTR_CLASS.to_string()],
                LinkedHashMap::new(),
            ),
            content: vec![Inline::Math(Math {
                math_type: MathType::InlineMath,
                text: "x^2".to_string(),
                source_info: si(),
            })],
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        });
        transform_inline(&mut inline);
        assert!(matches!(inline, Inline::Span(_)));
    }

    #[test]
    fn leaves_span_without_marker_class() {
        let mut inline = Inline::Span(Span {
            attr: (
                "eq-foo".to_string(),
                vec!["other-class".to_string()],
                LinkedHashMap::new(),
            ),
            content: vec![Inline::Math(Math {
                math_type: MathType::DisplayMath,
                text: "x".to_string(),
                source_info: si(),
            })],
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        });
        transform_inline(&mut inline);
        assert!(matches!(inline, Inline::Span(_)));
    }

    #[test]
    fn walks_paragraph() {
        let mut block = Block::Paragraph(Paragraph {
            content: vec![
                Inline::Str(Str {
                    text: "before".to_string(),
                    source_info: si(),
                }),
                display_math_span("eq-test", "a + b"),
            ],
            source_info: si(),
        });
        transform_block(&mut block);
        let Block::Paragraph(p) = &block else {
            panic!();
        };
        assert!(matches!(&p.content[0], Inline::Str(_)));
        assert!(matches!(&p.content[1], Inline::Custom(_)));
    }

    #[test]
    fn empty_eq_id_not_converted() {
        // "eq-" alone (empty suffix) should not be converted.
        let mut inline = display_math_span("eq-", "x");
        transform_inline(&mut inline);
        assert!(matches!(inline, Inline::Span(_)));
    }
}
