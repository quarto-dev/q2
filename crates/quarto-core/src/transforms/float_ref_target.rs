/*
 * transforms/float_ref_target.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Sugar transform: canonicalize float crossref targets into
 * CustomNode("FloatRefTarget").
 */

//! Sugar transform that produces the canonical crossref float target shape.
//!
//! This transform runs in the **normalization** phase (see plan D3) and
//! rewrites anything that a user (or pre-engine sugaring, or an engine)
//! wrote in "Div-with-crossref-id" style into a single canonical custom
//! node type: `CustomNode("FloatRefTarget", ..)`. Doing this once up front
//! means every downstream transform — indexing, resolution, back-end
//! rendering — only has to match one shape.
//!
//! ## Recognized input shapes
//!
//! Per plan 1.2, four author-facing shapes collapse to the same canonical
//! node:
//!
//! 1. `Div(#<ref>-..)` containing arbitrary content plus an optional
//!    trailing paragraph that becomes the caption. The bare `Div` form is
//!    the most common author-written shape.
//! 2. `Figure(#<ref>-..)` — `![caption](img){#fig-..}` Markdown. Pandoc's
//!    native `Figure` already separates content from caption; we just lift
//!    them into the custom node.
//! 3. `Div(#<ref>-..) > Figure` — user wrote a Div but its only block is a
//!    Figure. The outer id wins; the inner Figure is flattened into the
//!    custom node's slots.
//! 4. `Div(#tbl-..) > Table` — standard table crossref. The Table's own
//!    caption becomes the target's caption; the Table stays as content.
//!
//! A fifth shape, engine-emitted figure divs, matches shape (3) or (1)
//! post-engine and is therefore handled by the same code paths.
//!
//! ## Output
//!
//! A `CustomNode("FloatRefTarget")` with:
//!
//! - `attr`: the original block's attributes, preserving the identifier.
//! - `plain_data`:
//!   ```json
//!   {
//!     "ref_type":   "<prefix>",
//!     "kind":       "<display name>",
//!     "identifier": "<full id>"
//!   }
//!   ```
//! - `slots`:
//!   - `"content"`: [`Slot::Blocks`] — the body blocks (image, table,
//!     whatever). Empty is allowed.
//!   - `"caption_long"`: [`Slot::Blocks`] — present iff the original shape
//!     had a caption. Contains the caption blocks verbatim.
//!   - `"caption_short"`: [`Slot::Inlines`] — present iff the original
//!     shape carried a short caption (e.g. Figure.caption.short or a
//!     `fig-scap` attribute). Not yet populated in Phase 1; a later task
//!     will wire `fig-scap` / `tbl-scap` extraction.
//!
//! `plain_data.order` is *not* set here — the [`CrossrefIndexTransform`]
//! fills it during the crossref phase.

use quarto_pandoc_types::attr::{Attr, AttrSourceInfo};
use quarto_pandoc_types::block::{Block, Blocks, Div, Figure, Paragraph};
use quarto_pandoc_types::custom::{CustomNode, Slot};
use quarto_pandoc_types::pandoc::Pandoc;
use serde_json::json;

use crate::Result;
use crate::crossref::{FLOAT_REF_TARGET, RefTypeRegistry};
use crate::render::RenderContext;
use crate::transform::AstTransform;

/// Transform that sugars float crossref targets into
/// `CustomNode("FloatRefTarget")`.
pub struct FloatRefTargetSugarTransform;

impl FloatRefTargetSugarTransform {
    pub fn new() -> Self {
        Self
    }
}

impl Default for FloatRefTargetSugarTransform {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait(?Send)]
impl AstTransform for FloatRefTargetSugarTransform {
    fn name(&self) -> &str {
        "float-ref-target-sugar"
    }

    async fn transform(&self, ast: &mut Pandoc, ctx: &mut RenderContext) -> Result<()> {
        // If no registry is set up (e.g. unit tests or a WASM path that
        // bypassed the pre-engine stage), we have nothing to match against.
        // A missing registry is not an error — just a no-op.
        let Some(registry) = ctx.ref_type_registry.as_ref() else {
            return Ok(());
        };
        transform_blocks(&mut ast.blocks, registry);
        Ok(())
    }
}

/// Walk a block list, sugaring float-ref targets in place.
fn transform_blocks(blocks: &mut Vec<Block>, reg: &RefTypeRegistry) {
    for block in blocks.iter_mut() {
        transform_block(block, reg);
    }
}

/// Walk one block: first recurse into children, then check this node itself
/// for crossref-target shape. Bottom-up order matters for nested crossref
/// targets (a figure inside a larger document region).
fn transform_block(block: &mut Block, reg: &RefTypeRegistry) {
    // Recurse into children first.
    match block {
        Block::BlockQuote(bq) => transform_blocks(&mut bq.content, reg),
        Block::OrderedList(ol) => {
            for item in &mut ol.content {
                transform_blocks(item, reg);
            }
        }
        Block::BulletList(bl) => {
            for item in &mut bl.content {
                transform_blocks(item, reg);
            }
        }
        Block::DefinitionList(dl) => {
            for (_term, defs) in &mut dl.content {
                for def in defs {
                    transform_blocks(def, reg);
                }
            }
        }
        Block::Figure(fig) => transform_blocks(&mut fig.content, reg),
        Block::Div(div) => transform_blocks(&mut div.content, reg),
        Block::Custom(custom) => {
            // Crossref targets can nest inside other custom nodes
            // (e.g. a figure inside a callout). Recurse through slots.
            for (_name, slot) in &mut custom.slots {
                match slot {
                    Slot::Block(b) => transform_block(b, reg),
                    Slot::Blocks(bs) => transform_blocks(bs, reg),
                    _ => {}
                }
            }
        }
        _ => {}
    }

    // Now check this block itself. Replace in place iff we recognize a
    // crossref target shape.
    let converted = match block {
        Block::Div(div) => classify_div(&div.attr, reg).map(|def| {
            convert_div(
                std::mem::replace(
                    div,
                    Div {
                        attr: empty_attr(),
                        content: Vec::new(),
                        source_info: div.source_info.clone(),
                        attr_source: AttrSourceInfo::empty(),
                    },
                ),
                def,
            )
        }),
        Block::Figure(fig) => classify_fig(&fig.attr, reg).map(|def| {
            convert_figure(
                std::mem::replace(
                    fig,
                    Figure {
                        attr: empty_attr(),
                        caption: quarto_pandoc_types::caption::Caption {
                            short: None,
                            long: None,
                            source_info: fig.caption.source_info.clone(),
                        },
                        content: Vec::new(),
                        source_info: fig.source_info.clone(),
                        attr_source: AttrSourceInfo::empty(),
                    },
                ),
                def,
            )
        }),
        _ => None,
    };

    if let Some(custom) = converted {
        *block = Block::Custom(custom);
    }
}

/// Classify a Div's attributes: if its id is a crossref target, return the
/// matching [`RefTypeDef`].
fn classify_div<'r>(
    attr: &Attr,
    reg: &'r RefTypeRegistry,
) -> Option<&'r crate::crossref::RefTypeDef> {
    let id = attr.0.as_str();
    reg.classify_cite_id(id)
}

/// Classify a Figure's attributes: if its id is a crossref target, return
/// the matching [`RefTypeDef`].
fn classify_fig<'r>(
    attr: &Attr,
    reg: &'r RefTypeRegistry,
) -> Option<&'r crate::crossref::RefTypeDef> {
    let id = attr.0.as_str();
    reg.classify_cite_id(id)
}

fn empty_attr() -> Attr {
    use hashlink::LinkedHashMap;
    (String::new(), Vec::new(), LinkedHashMap::new())
}

/// Convert a `Div` that we already know is a crossref target into a
/// FloatRefTarget custom node.
///
/// Shape handling:
/// - If the Div contains *exactly one* block and it's a `Figure`, flatten:
///   the Figure's content becomes the target's content slot, and the
///   Figure's caption becomes the target's caption. The Div's id wins
///   over the Figure's inner id (which is typically absent).
/// - If the Div contains *exactly one* block and it's a `Table`, keep the
///   Table as content and extract its caption as the target's caption.
/// - Otherwise, the last `Paragraph` becomes the caption (Q1 convention),
///   and the remaining blocks become content. Divs with no trailing para
///   still produce a target — just with no caption.
fn convert_div(div: Div, def: &crate::crossref::RefTypeDef) -> CustomNode {
    let source_info = div.source_info.clone();
    let attr = div.attr.clone();
    let identifier = attr.0.clone();

    let mut content_blocks = div.content;
    let (content, caption_long, caption_short) = match content_blocks.as_slice() {
        [Block::Figure(_)] => {
            // Flatten Div > Figure. Move Figure's content/caption up.
            let Block::Figure(fig) = content_blocks.remove(0) else {
                unreachable!()
            };
            let caption_long = fig.caption.long.unwrap_or_default();
            let caption_short = fig.caption.short;
            (fig.content, caption_long, caption_short)
        }
        [Block::Table(_)] => {
            // Table keeps its own caption for rendering convenience, but we
            // also surface the caption on the target so resolvers can use
            // it as link text.
            let Block::Table(table) = content_blocks.remove(0) else {
                unreachable!()
            };
            let caption_long = table.caption.long.clone().unwrap_or_default();
            let caption_short = table.caption.short.clone();
            (vec![Block::Table(table)], caption_long, caption_short)
        }
        _ => {
            // General case: last Paragraph becomes caption.
            let caption = match content_blocks.last() {
                Some(Block::Paragraph(_)) => {
                    let last = content_blocks.pop().unwrap();
                    let Block::Paragraph(para) = last else {
                        unreachable!()
                    };
                    Some(para)
                }
                _ => None,
            };
            let (content, long) = match caption {
                Some(para) => {
                    let para_block = Block::Paragraph(Paragraph {
                        content: para.content,
                        source_info: para.source_info,
                    });
                    (content_blocks, vec![para_block])
                }
                None => (content_blocks, Vec::new()),
            };
            (content, long, None)
        }
    };

    let mut node = CustomNode::new(FLOAT_REF_TARGET, attr, source_info);
    node.plain_data = json!({
        "ref_type":   def.ref_type,
        "kind":       def.kind,
        "identifier": identifier,
    });
    node.slots.insert("content".into(), Slot::Blocks(content));
    if !caption_long.is_empty() {
        node.slots
            .insert("caption_long".into(), Slot::Blocks(caption_long));
    }
    if let Some(short) = caption_short {
        if !short.is_empty() {
            node.slots
                .insert("caption_short".into(), Slot::Inlines(short));
        }
    }
    node
}

/// Convert a `Figure` that we already know is a crossref target (its id
/// matches a registered ref-type) into a FloatRefTarget custom node.
fn convert_figure(fig: Figure, def: &crate::crossref::RefTypeDef) -> CustomNode {
    let source_info = fig.source_info.clone();
    let attr = fig.attr.clone();
    let identifier = attr.0.clone();

    let content: Blocks = fig.content;
    let caption_long = fig.caption.long.unwrap_or_default();
    let caption_short = fig.caption.short;

    let mut node = CustomNode::new(FLOAT_REF_TARGET, attr, source_info);
    node.plain_data = json!({
        "ref_type":   def.ref_type,
        "kind":       def.kind,
        "identifier": identifier,
    });
    node.slots.insert("content".into(), Slot::Blocks(content));
    if !caption_long.is_empty() {
        node.slots
            .insert("caption_long".into(), Slot::Blocks(caption_long));
    }
    if let Some(short) = caption_short {
        if !short.is_empty() {
            node.slots
                .insert("caption_short".into(), Slot::Inlines(short));
        }
    }
    node
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crossref::crossref_target_view;
    use hashlink::LinkedHashMap;
    use quarto_pandoc_types::attr::AttrSourceInfo;
    use quarto_pandoc_types::block::{CodeBlock, Div, Figure, Paragraph};
    use quarto_pandoc_types::caption::Caption;
    use quarto_pandoc_types::inline::{Inline, Str};
    use quarto_source_map::{FileId, SourceInfo};

    fn si() -> SourceInfo {
        SourceInfo::original(FileId(0), 0, 0)
    }

    fn attr_id(id: &str) -> Attr {
        (id.to_string(), Vec::new(), LinkedHashMap::new())
    }

    fn str_inline(s: &str) -> Inline {
        Inline::Str(Str {
            text: s.to_string(),
            source_info: si(),
        })
    }

    fn para(text: &str) -> Block {
        Block::Paragraph(Paragraph {
            content: vec![str_inline(text)],
            source_info: si(),
        })
    }

    fn code(lang: &str, body: &str) -> Block {
        Block::CodeBlock(CodeBlock {
            attr: (String::new(), vec![lang.into()], LinkedHashMap::new()),
            text: body.to_string(),
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        })
    }

    fn run_transform(blocks: Vec<Block>, reg: &RefTypeRegistry) -> Vec<Block> {
        let mut blocks = blocks;
        transform_blocks(&mut blocks, reg);
        blocks
    }

    #[test]
    fn div_with_trailing_paragraph_becomes_float_ref_target() {
        let reg = RefTypeRegistry::builtin();
        let div = Block::Div(Div {
            attr: attr_id("fig-hello"),
            content: vec![code("python", "pyplot.show()"), para("Hello, world.")],
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        });
        let out = run_transform(vec![div], &reg);
        assert_eq!(out.len(), 1);

        let view = crossref_target_view(&out[0]).expect("is a float-ref target");
        assert_eq!(view.identifier, "fig-hello");
        assert_eq!(view.ref_type, "fig");
        assert_eq!(view.kind, "Figure");

        let Block::Custom(node) = &out[0] else {
            panic!("expected custom node");
        };
        // Content: the code block only (caption was stripped).
        let Slot::Blocks(content) = node.slots.get("content").unwrap() else {
            panic!("content slot not a Blocks");
        };
        assert_eq!(content.len(), 1);
        assert!(matches!(content[0], Block::CodeBlock(_)));

        // Caption long: the paragraph.
        let Slot::Blocks(cap) = node.slots.get("caption_long").unwrap() else {
            panic!("caption_long slot not a Blocks");
        };
        assert_eq!(cap.len(), 1);
        match &cap[0] {
            Block::Paragraph(p) => assert_eq!(p.content.len(), 1),
            other => panic!("caption first block should be Paragraph, got {:?}", other),
        }
    }

    #[test]
    fn div_without_trailing_paragraph_has_no_caption_slot() {
        let reg = RefTypeRegistry::builtin();
        let div = Block::Div(Div {
            attr: attr_id("fig-no-cap"),
            content: vec![code("python", "pyplot.show()")],
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        });
        let out = run_transform(vec![div], &reg);
        let Block::Custom(node) = &out[0] else {
            panic!("expected custom node");
        };
        assert!(node.slots.get("caption_long").is_none());
        assert!(node.slots.get("caption_short").is_none());
    }

    #[test]
    fn figure_with_id_becomes_float_ref_target() {
        let reg = RefTypeRegistry::builtin();
        let fig = Block::Figure(Figure {
            attr: attr_id("fig-plain"),
            caption: Caption {
                short: None,
                long: Some(vec![para("Caption from Figure.")]),
                source_info: si(),
            },
            content: vec![para("image placeholder")],
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        });
        let out = run_transform(vec![fig], &reg);
        let view = crossref_target_view(&out[0]).expect("is a target");
        assert_eq!(view.identifier, "fig-plain");
        let Block::Custom(node) = &out[0] else {
            panic!();
        };
        assert!(matches!(
            node.slots.get("caption_long"),
            Some(Slot::Blocks(b)) if b.len() == 1
        ));
    }

    #[test]
    fn div_over_figure_flattens_to_single_target() {
        let reg = RefTypeRegistry::builtin();
        let inner_fig = Block::Figure(Figure {
            attr: (String::new(), Vec::new(), LinkedHashMap::new()),
            caption: Caption {
                short: None,
                long: Some(vec![para("inner cap")]),
                source_info: si(),
            },
            content: vec![para("image")],
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        });
        let div = Block::Div(Div {
            attr: attr_id("fig-outer"),
            content: vec![inner_fig],
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        });
        let out = run_transform(vec![div], &reg);
        assert_eq!(out.len(), 1);
        let view = crossref_target_view(&out[0]).expect("is a target");
        assert_eq!(view.identifier, "fig-outer");
        let Block::Custom(node) = &out[0] else {
            panic!();
        };
        let Slot::Blocks(content) = node.slots.get("content").unwrap() else {
            panic!();
        };
        assert_eq!(content.len(), 1);
        assert!(matches!(&content[0], Block::Paragraph(p) if p.content.len() == 1));
        assert!(node.slots.get("caption_long").is_some());
    }

    #[test]
    fn div_without_crossref_id_left_alone() {
        let reg = RefTypeRegistry::builtin();
        let div = Block::Div(Div {
            attr: attr_id("just-a-div"),
            content: vec![para("content")],
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        });
        let out = run_transform(vec![div.clone()], &reg);
        assert_eq!(out, vec![div]);
    }

    #[test]
    fn citation_shaped_id_not_mistaken_for_crossref() {
        let reg = RefTypeRegistry::builtin();
        let div = Block::Div(Div {
            attr: attr_id("smithfoo-2020"),
            content: vec![para("biblio-looking div")],
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        });
        let out = run_transform(vec![div.clone()], &reg);
        assert_eq!(out, vec![div]);
    }

    #[test]
    fn nested_crossref_target_sugared() {
        let reg = RefTypeRegistry::builtin();
        let inner = Block::Div(Div {
            attr: attr_id("fig-nested"),
            content: vec![code("python", "x=1"), para("cap")],
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        });
        let outer = Block::Div(Div {
            attr: (
                String::new(),
                vec!["callout-note".into()],
                LinkedHashMap::new(),
            ),
            content: vec![inner],
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        });
        let out = run_transform(vec![outer], &reg);
        let Block::Div(outer_div) = &out[0] else {
            panic!("outer remains a Div");
        };
        let view = crossref_target_view(&outer_div.content[0]).expect("inner sugared");
        assert_eq!(view.identifier, "fig-nested");
    }

    #[test]
    fn custom_category_honored_if_registered() {
        let mut reg = RefTypeRegistry::builtin();
        reg.register_custom("dia", "Diagram", None).unwrap();
        let div = Block::Div(Div {
            attr: attr_id("dia-one"),
            content: vec![para("diagram caption")],
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        });
        let out = run_transform(vec![div], &reg);
        let view = crossref_target_view(&out[0]).expect("is a target");
        assert_eq!(view.ref_type, "dia");
        assert_eq!(view.kind, "Diagram");
    }
}
