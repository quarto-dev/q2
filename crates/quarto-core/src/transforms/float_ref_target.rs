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
//!     had a caption. Its **first block is always a `Plain`**: a caption
//!     authored as a trailing paragraph (div form, `#| fig-cap` cells)
//!     arrives as a `Paragraph` and is canonicalized here, while
//!     Pandoc-native `Figure` / `Table` captions are `Plain` already. One
//!     shape for every float form means every consumer (the crossref
//!     renderer's prefix step, the index, the HTML writer's `<figcaption>`)
//!     behaves identically regardless of how the float was written — and
//!     the writer emits bare inlines inside `<figcaption>`, as Q1 does.
//!     Any trailing blocks are kept as authored. Consumers must still accept
//!     a leading `Paragraph` defensively: Lua filters and the JSON reader can
//!     hand the pipeline either. (bd-n3sark9b; Q1's own IR is *not*
//!     canonical — its div-form caption stays a `Para` — so this is a
//!     deliberate, documented divergence for filter authors: bd-t0qt409i.)
//!   - `"caption_short"`: [`Slot::Inlines`] — present iff the original
//!     shape carried a short caption (e.g. Figure.caption.short or a
//!     `fig-scap` attribute). Not yet populated in Phase 1; a later task
//!     will wire `fig-scap` / `tbl-scap` extraction.
//!
//! `plain_data.order` is *not* set here — the [`CrossrefIndexTransform`]
//! fills it during the crossref phase.

use quarto_pandoc_types::attr::{Attr, AttrSourceInfo};
use quarto_pandoc_types::block::{Block, Blocks, Div, Figure, Plain};
use quarto_pandoc_types::custom::{CustomNode, Slot};
use quarto_pandoc_types::pandoc::Pandoc;
use serde_json::json;

use crate::Result;
use crate::crossref::{FLOAT_REF_TARGET, RefTypeRegistry};
use crate::render::RenderContext;
use crate::transform::{AstTransform, TransformPhase};

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

    fn phase(&self) -> TransformPhase {
        TransformPhase::Normalization
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
    // Desugar the bare-table caption form into the canonical float Div first,
    // so the uniform `Block::Div` classifier below handles it exactly like the
    // `::: {#tbl-…}` authoring form (bd-4ly7ne01).
    maybe_wrap_bare_table_into_div(block, reg);

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

/// Desugar the `: caption {#tbl-…}` pipe-table syntax into the canonical float
/// Div shape `Div(#tbl-…) > Table`.
///
/// That syntax parses to a *bare* `Block::Table` carrying the crossref id on
/// its own `attr` (the table also owns its caption). There is no wrapping Div,
/// so the uniform `classify_div`/`convert_div` path never sees it. We wrap it:
/// the id (and any classes/kvs) move onto a new `Div`, leaving the `Table`
/// anonymous — exactly the shape the `::: {#tbl-…}` authoring form produces, and
/// what `convert_div`'s `[Block::Table(_)]` arm already handles (it keeps the
/// table as content and lifts its caption). Moving the id off the table avoids
/// emitting a duplicate `id` in the output.
///
/// No-op unless `block` is a `Table` whose non-empty id classifies as a
/// crossref ref-type (so plain captioned tables without a `#…` id are
/// untouched). bd-4ly7ne01.
fn maybe_wrap_bare_table_into_div(block: &mut Block, reg: &RefTypeRegistry) {
    // Extract the Div attr while the `table` borrow is live; the match arm
    // returns (ending the borrow) before we move the whole table.
    let (div_attr, div_attr_source, source_info) = match block {
        Block::Table(table)
            if !table.attr.0.is_empty() && reg.classify_cite_id(&table.attr.0).is_some() =>
        {
            (
                std::mem::replace(&mut table.attr, empty_attr()),
                std::mem::replace(&mut table.attr_source, AttrSourceInfo::empty()),
                table.source_info.clone(),
            )
        }
        _ => return,
    };
    // Replace the block with a Div (consuming the now-anonymous Table), then
    // move the Table into the Div's content.
    let table_block = std::mem::replace(
        block,
        Block::Div(Div {
            attr: div_attr,
            content: Vec::new(),
            source_info,
            attr_source: div_attr_source,
        }),
    );
    if let Block::Div(div) = block {
        div.content.push(table_block);
    }
}

/// Canonicalize a float caption to the slot contract documented in the
/// module doc: a leading `Paragraph` becomes a `Plain` with the same inlines
/// and source info; any other leading block, and every trailing block, is
/// left untouched.
fn canonicalize_caption(mut caption: Blocks) -> Blocks {
    if matches!(caption.first(), Some(Block::Paragraph(_))) {
        let Block::Paragraph(para) = caption.remove(0) else {
            unreachable!("matched Paragraph above");
        };
        caption.insert(
            0,
            Block::Plain(Plain {
                content: para.content,
                source_info: para.source_info,
            }),
        );
    }
    caption
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
            // Surface the table's caption onto the target (numbered
            // rendering, resolver link text), then clear the Table's own
            // copy — matching Q1, which does the same at parse time
            // (`quarto-pre/parsefiguredivs.lua`: `table.caption =
            // pandoc.Caption{}` at L280, `el.caption.long =
            // pandoc.Blocks({})` at L544).
            //
            // This must happen here, at construction time, not only in
            // `crossref_render.rs`'s later HTML-DOM elision: the
            // `Bucket::B4` classification means `crossref-render` does
            // not survive to the Pandoc cut at all, so a docx/pptx
            // render's FloatRefTarget content reaches the vendored Lua
            // filters with whatever caption state it had *here* —
            // uncleared, it duplicated the caption in the rendered docx
            // (once via the Table's own native caption, once via the
            // Lua filter's numbered rendering). Measured via `cargo run
            // --bin q2 -- render <table-with-caption> --to docx`, which
            // produced two "My Caption" paragraphs in the output
            // `word/document.xml`.
            let Block::Table(mut table) = content_blocks.remove(0) else {
                unreachable!()
            };
            let caption_long = table.caption.long.clone().unwrap_or_default();
            let caption_short = table.caption.short.clone();
            table.caption.long = None;
            table.caption.short = None;
            (vec![Block::Table(table)], caption_long, caption_short)
        }
        _ => {
            // General case: last Paragraph becomes caption (Q1's
            // `refCaptionFromDiv`). It is canonicalized to Plain when the
            // slot is filled below.
            let long = match content_blocks.last() {
                Some(Block::Paragraph(_)) => vec![content_blocks.pop().unwrap()],
                _ => Vec::new(),
            };
            (content_blocks, long, None)
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
        node.slots.insert(
            "caption_long".into(),
            Slot::Blocks(canonicalize_caption(caption_long)),
        );
    }
    if let Some(short) = caption_short
        && !short.is_empty()
    {
        node.slots
            .insert("caption_short".into(), Slot::Inlines(short));
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
        node.slots.insert(
            "caption_long".into(),
            Slot::Blocks(canonicalize_caption(caption_long)),
        );
    }
    if let Some(short) = caption_short
        && !short.is_empty()
    {
        node.slots
            .insert("caption_short".into(), Slot::Inlines(short));
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

        // Caption long: the trailing paragraph, canonicalized to `Plain`
        // (bd-n3sark9b — every float form presents the same caption shape).
        let Slot::Blocks(cap) = node.slots.get("caption_long").unwrap() else {
            panic!("caption_long slot not a Blocks");
        };
        assert_eq!(cap.len(), 1);
        match &cap[0] {
            Block::Plain(p) => {
                assert_eq!(p.content.len(), 1);
                assert!(matches!(&p.content[0], Inline::Str(s) if s.text == "Hello, world."));
            }
            other => panic!("caption first block should be Plain, got {:?}", other),
        }
    }

    /// Assert the target's `caption_long` starts with a `Plain` whose first
    /// inline is `Str(text)`.
    fn assert_plain_caption(node: &CustomNode, text: &str) {
        let Slot::Blocks(cap) = node.slots.get("caption_long").expect("caption_long slot") else {
            panic!("caption_long slot not a Blocks");
        };
        match cap.first() {
            Some(Block::Plain(p)) => {
                assert!(
                    matches!(&p.content[0], Inline::Str(s) if s.text == text),
                    "caption inlines: {:?}",
                    p.content
                );
            }
            other => panic!("caption first block should be Plain, got {:?}", other),
        }
    }

    #[test]
    fn div_over_table_with_paragraph_caption_canonicalizes_to_plain() {
        // A Table whose caption.long starts with a Paragraph (possible from
        // Lua filters or the JSON reader; Pandoc itself emits Plain).
        use quarto_pandoc_types::table::{Table, TableFoot, TableHead};
        let reg = RefTypeRegistry::builtin();
        let table = Block::Table(Table {
            attr: (String::new(), Vec::new(), LinkedHashMap::new()),
            caption: Caption {
                short: None,
                long: Some(vec![para("Numbers")]),
                source_info: si(),
            },
            colspec: vec![],
            head: TableHead {
                attr: (String::new(), Vec::new(), LinkedHashMap::new()),
                rows: vec![],
                source_info: si(),
                attr_source: AttrSourceInfo::empty(),
            },
            bodies: vec![],
            foot: TableFoot {
                attr: (String::new(), Vec::new(), LinkedHashMap::new()),
                rows: vec![],
                source_info: si(),
                attr_source: AttrSourceInfo::empty(),
            },
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        });
        let div = Block::Div(Div {
            attr: attr_id("tbl-nums"),
            content: vec![table],
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        });
        let out = run_transform(vec![div], &reg);
        let Block::Custom(node) = &out[0] else {
            panic!("expected custom node");
        };
        assert_plain_caption(node, "Numbers");
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
        // A Paragraph caption on a native Figure is canonicalized to Plain too.
        assert_plain_caption(node, "Caption from Figure.");
    }

    /// A `Div(#tbl-..) > Table` float must surface the Table's caption
    /// onto the target *and* clear the Table's own copy — otherwise a
    /// docx/pptx render duplicates it, since `crossref-render`
    /// (`Bucket::B4`) never runs for the Pandoc-tail and can't elide it
    /// downstream the way it does for the HTML float DOM. Measured via
    /// `cargo run --bin q2 -- render <table-with-caption> --to docx`,
    /// which produced two "My Caption" paragraphs in the output
    /// `word/document.xml` before this fix.
    #[test]
    fn div_over_table_clears_the_tables_own_caption() {
        use quarto_pandoc_types::table::{Table, TableBody, TableFoot, TableHead};
        let table = Block::Table(Table {
            attr: (String::new(), Vec::new(), LinkedHashMap::new()),
            caption: Caption {
                short: None,
                long: Some(vec![para("My Caption")]),
                source_info: si(),
            },
            colspec: vec![],
            head: TableHead {
                attr: (String::new(), Vec::new(), LinkedHashMap::new()),
                rows: vec![],
                source_info: si(),
                attr_source: AttrSourceInfo::empty(),
            },
            bodies: vec![TableBody {
                attr: (String::new(), Vec::new(), LinkedHashMap::new()),
                rowhead_columns: 0,
                head: vec![],
                body: vec![],
                source_info: si(),
                attr_source: AttrSourceInfo::empty(),
            }],
            foot: TableFoot {
                attr: (String::new(), Vec::new(), LinkedHashMap::new()),
                rows: vec![],
                source_info: si(),
                attr_source: AttrSourceInfo::empty(),
            },
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        });
        let reg = RefTypeRegistry::builtin();
        let div = Block::Div(Div {
            attr: attr_id("tbl-one"),
            content: vec![table],
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        });
        let out = run_transform(vec![div], &reg);
        let Block::Custom(node) = &out[0] else {
            panic!("expected custom node");
        };

        // The target's own caption still carries the text (unchanged
        // behavior — resolvers/numbering still need it).
        let Slot::Blocks(cap) = node.slots.get("caption_long").unwrap() else {
            panic!("caption_long slot not a Blocks");
        };
        assert!(
            format!("{cap:?}").contains("My Caption"),
            "target's caption_long should carry the text: {cap:?}"
        );

        // ...but the Table inside "content" must no longer carry it.
        let Slot::Blocks(content) = node.slots.get("content").unwrap() else {
            panic!("content slot not a Blocks");
        };
        let Block::Table(t) = &content[0] else {
            panic!("expected the table in content");
        };
        assert!(
            t.caption.long.as_ref().is_none_or(|b| b.is_empty()),
            "the table's own caption must be cleared once surfaced onto the target, \
             else a docx/pptx render duplicates it: {:?}",
            t.caption.long
        );
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
        // The inner Figure's Paragraph caption is canonicalized to Plain.
        assert_plain_caption(node, "inner cap");
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
