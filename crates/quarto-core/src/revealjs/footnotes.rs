/*
 * revealjs/footnotes.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Per-slide footnote coalescing for revealjs (Phase 2e-ii).
 */

//! Coalesce each slide's footnotes (and authored asides) into a single
//! bottom-of-slide `<aside>`, mirroring Quarto 1's `coalesceAsides` /
//! `handleSlideFootnotes` (`format-reveal.ts`).
//!
//! ## Where this runs
//!
//! This transform runs **after** [`FootnotesTransform`], consuming its
//! *resolved* output rather than the raw `Inline::Note` / `NoteReference` /
//! `NoteDefinition*` representation. By that point:
//!
//! - every footnote *reference* is a `Span#fnrefN` →
//!   `Superscript` → `Link(#fnN, role="doc-noteref")`, and
//! - every footnote *definition* is a `Div#fnN` inside the trailing
//!   `Div#footnotes` (class `section`), which the HTML writer would otherwise
//!   serialize as a final `<section role="doc-endnotes">` slide.
//!
//! Running post-resolution is deliberate: it is robust to **every** footnote
//! source — inline `^[…]`, `[^id]` refs, and citeproc note-style citations all
//! funnel into that one normalized structure (see the plan's Phase 2e-ii
//! decision, 2026-06-09).
//!
//! ## What it produces (per leaf slide)
//!
//! For each leaf slide that carries footnote refs and/or authored `.aside`
//! Divs, it appends one coalesced `Div.aside` (→ `<aside class="aside">`)
//! containing:
//!
//! - a plain `<div>` for each authored aside's content, then
//! - a `Div.aside-footnotes` wrapping an `OrderedList` of the referenced
//!   definitions (backlinks stripped), if any.
//!
//! Each in-text reference is replaced by a plain `Superscript([Str(N)])`
//! renumbered **per slide** (1, 2, …), and the trailing `Div#footnotes` is
//! deleted. `OrderedList` has no class slot, so `aside-footnotes` rides a
//! wrapping `Div` — the same trick [`FootnotesTransform`] uses for the
//! document-level list.
//!
//! ## Gating (Q1-faithful)
//!
//! Coalesce by default; opt out with `reference-location: document` (Q1:
//! `slideFootnotes = referenceLocation !== "document"`, and reveal never sets
//! `reference-location`, so coalescing is the default).
//!
//! WASM-safe (pure AST manipulation) — so it benefits `q2 render` and
//! `q2 preview` alike, with no previewRegistry changes (the coalesced output is
//! all standard `Div` / `OrderedList` / `Superscript` nodes).

use std::collections::HashMap;

use hashlink::LinkedHashMap;
use quarto_pandoc_types::attr::AttrSourceInfo;
use quarto_pandoc_types::block::{Block, Div, OrderedList};
use quarto_pandoc_types::inline::{Inline, Str, Superscript};
use quarto_pandoc_types::pandoc::Pandoc;
use quarto_pandoc_types::{Blocks, ListNumberDelim, ListNumberStyle};
use quarto_source_map::SourceInfo;

use crate::Result;
use crate::render::RenderContext;
use crate::transform::AstTransform;

/// Transform that coalesces per-slide footnotes/asides for `format: revealjs`.
pub struct RevealFootnotesTransform;

impl RevealFootnotesTransform {
    pub fn new() -> Self {
        Self
    }
}

impl Default for RevealFootnotesTransform {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait(?Send)]
impl AstTransform for RevealFootnotesTransform {
    fn name(&self) -> &str {
        "reveal-footnotes"
    }

    async fn transform(&self, ast: &mut Pandoc, _ctx: &mut RenderContext) -> Result<()> {
        // Q1: `slideFootnotes = referenceLocation !== "document"`. Reveal never
        // sets `reference-location`, so the default (unset) coalesces; an
        // explicit `document` opts back into the trailing footnotes slide.
        // Use `as_plain_text`, not `as_str`: real frontmatter parses YAML
        // scalars as markdown (`PandocInlines`), for which `as_str` returns
        // `None`. (The same quirk means `FootnotesTransform` always runs in its
        // default Document mode in real renders — conveniently building the
        // trailing section this transform redistributes, whatever the user set.)
        let ref_loc = ast
            .meta
            .get("reference-location")
            .and_then(|v| v.as_plain_text());
        let coalesce = !ref_loc
            .as_deref()
            .is_some_and(|s| s.trim().eq_ignore_ascii_case("document"));
        if !coalesce {
            return Ok(());
        }

        // Pull the resolved definitions out of the trailing `Div#footnotes`
        // (deleting that would-be final slide); empty when there are no
        // footnotes (we may still coalesce multiple asides per slide).
        let defs = extract_footnotes_section(&mut ast.blocks);
        coalesce_slides(&mut ast.blocks, &defs);
        Ok(())
    }
}

/// Remove the trailing `Div#footnotes` and index its definitions by `fnN` id,
/// with each definition's backlink stripped. Returns an empty map (and removes
/// nothing) when no such section exists.
fn extract_footnotes_section(blocks: &mut Vec<Block>) -> HashMap<String, Blocks> {
    let mut defs = HashMap::new();
    let Some(pos) = blocks
        .iter()
        .position(|b| matches!(b, Block::Div(d) if d.attr.0 == "footnotes"))
    else {
        return defs;
    };
    let Block::Div(div) = blocks.remove(pos) else {
        return defs;
    };
    // `Div#footnotes` content is `[HorizontalRule, OrderedList]`; each list item
    // is `[Div#fnN [ …content…, backlink ]]` (see `create_footnotes_section`).
    for block in div.content {
        if let Block::OrderedList(ol) = block {
            for item in ol.content {
                for b in item {
                    if let Block::Div(d) = b {
                        let mut content = d.content;
                        strip_backlinks(&mut content);
                        defs.insert(d.attr.0, content);
                    }
                }
            }
        }
    }
    defs
}

/// Walk the slide tree, coalescing footnotes/asides into each **leaf** slide. A
/// section Div whose children include section Divs is a *stack* (recurse); one
/// whose children are plain content is a leaf slide (process).
fn coalesce_slides(blocks: &mut [Block], defs: &HashMap<String, Blocks>) {
    for block in blocks {
        if let Block::Div(div) = block {
            if !is_section(div) {
                continue;
            }
            if has_child_section(div) {
                coalesce_slides(&mut div.content, defs);
            } else {
                coalesce_one_slide(div, defs);
            }
        }
    }
}

fn is_section(div: &Div) -> bool {
    div.attr.1.iter().any(|c| c == "section")
}

fn has_child_section(div: &Div) -> bool {
    div.content
        .iter()
        .any(|b| matches!(b, Block::Div(d) if is_section(d)))
}

/// Coalesce one leaf slide: gather `.aside` Divs + footnote refs, append a
/// single bottom `Div.aside`, and renumber refs per slide.
fn coalesce_one_slide(slide: &mut Div, defs: &HashMap<String, Blocks>) {
    // 1. Lift `.aside` Divs out of the slide content (keep their inner blocks).
    let mut asides: Vec<Blocks> = Vec::new();
    slide.content.retain_mut(|b| {
        if let Block::Div(d) = b {
            if d.attr.1.iter().any(|c| c == "aside") {
                asides.push(std::mem::take(&mut d.content));
                return false;
            }
        }
        true
    });

    // 2. Renumber footnote refs in the remaining content (per-slide 1, 2, …),
    //    recording the referenced `fnN` ids in first-appearance order.
    let mut order: Vec<String> = Vec::new();
    let mut numbering: HashMap<String, usize> = HashMap::new();
    renumber_refs_blocks(&mut slide.content, defs, &mut order, &mut numbering);

    if asides.is_empty() && order.is_empty() {
        return;
    }

    // 3. Build the single coalesced `<aside class="aside">`.
    let mut aside_content: Vec<Block> = Vec::new();
    for a in asides {
        aside_content.push(plain_div(a));
    }
    if !order.is_empty() {
        let items: Vec<Blocks> = order
            .iter()
            .map(|fn_id| defs.get(fn_id).cloned().unwrap_or_default())
            .collect();
        let ol = Block::OrderedList(OrderedList {
            attr: (1, ListNumberStyle::Decimal, ListNumberDelim::Period),
            content: items,
            source_info: SourceInfo::default(),
        });
        aside_content.push(classed_div("aside-footnotes", vec![ol]));
    }
    slide.content.push(classed_div("aside", aside_content));
}

/// A `<div>` with no attributes wrapping `content`.
fn plain_div(content: Blocks) -> Block {
    Block::Div(Div {
        attr: (String::new(), Vec::new(), LinkedHashMap::new()),
        content,
        source_info: SourceInfo::default(),
        attr_source: AttrSourceInfo::empty(),
    })
}

/// A `<div class="{class}">` wrapping `content`.
fn classed_div(class: &str, content: Blocks) -> Block {
    Block::Div(Div {
        attr: (String::new(), vec![class.to_string()], LinkedHashMap::new()),
        content,
        source_info: SourceInfo::default(),
        attr_source: AttrSourceInfo::empty(),
    })
}

/// Remove footnote backlinks (`<a class="footnote-back">`) from a definition's
/// blocks. The backlink is appended to the definition's last paragraph by
/// `FootnotesTransform`; per-slide footnotes drop it (the ref no longer links).
fn strip_backlinks(blocks: &mut Blocks) {
    for block in blocks.iter_mut() {
        match block {
            Block::Paragraph(p) => p.content.retain(|i| !is_backlink(i)),
            Block::Plain(p) => p.content.retain(|i| !is_backlink(i)),
            _ => {}
        }
    }
}

fn is_backlink(inline: &Inline) -> bool {
    matches!(inline, Inline::Link(l) if l.attr.1.iter().any(|c| c == "footnote-back"))
}

// ── reference renumbering ─────────────────────────────────────────────────

fn renumber_refs_blocks(
    blocks: &mut [Block],
    defs: &HashMap<String, Blocks>,
    order: &mut Vec<String>,
    numbering: &mut HashMap<String, usize>,
) {
    for block in blocks {
        renumber_refs_block(block, defs, order, numbering);
    }
}

fn renumber_refs_block(
    block: &mut Block,
    defs: &HashMap<String, Blocks>,
    order: &mut Vec<String>,
    numbering: &mut HashMap<String, usize>,
) {
    match block {
        Block::Paragraph(p) => renumber_refs_inlines(&mut p.content, defs, order, numbering),
        Block::Plain(p) => renumber_refs_inlines(&mut p.content, defs, order, numbering),
        Block::Header(h) => renumber_refs_inlines(&mut h.content, defs, order, numbering),
        Block::BlockQuote(bq) => renumber_refs_blocks(&mut bq.content, defs, order, numbering),
        Block::OrderedList(ol) => {
            for item in &mut ol.content {
                renumber_refs_blocks(item, defs, order, numbering);
            }
        }
        Block::BulletList(bl) => {
            for item in &mut bl.content {
                renumber_refs_blocks(item, defs, order, numbering);
            }
        }
        Block::DefinitionList(dl) => {
            for (term, items) in &mut dl.content {
                renumber_refs_inlines(term, defs, order, numbering);
                for item in items {
                    renumber_refs_blocks(item, defs, order, numbering);
                }
            }
        }
        Block::Div(div) => renumber_refs_blocks(&mut div.content, defs, order, numbering),
        Block::Figure(fig) => {
            renumber_refs_blocks(&mut fig.content, defs, order, numbering);
            if let Some(ref mut long) = fig.caption.long {
                renumber_refs_blocks(long, defs, order, numbering);
            }
        }
        Block::Table(table) => {
            if let Some(ref mut long) = table.caption.long {
                renumber_refs_blocks(long, defs, order, numbering);
            }
            for body in &mut table.bodies {
                for row in &mut body.body {
                    for cell in &mut row.cells {
                        renumber_refs_blocks(&mut cell.content, defs, order, numbering);
                    }
                }
            }
            for row in &mut table.head.rows {
                for cell in &mut row.cells {
                    renumber_refs_blocks(&mut cell.content, defs, order, numbering);
                }
            }
            for row in &mut table.foot.rows {
                for cell in &mut row.cells {
                    renumber_refs_blocks(&mut cell.content, defs, order, numbering);
                }
            }
        }
        _ => {}
    }
}

fn renumber_refs_inlines(
    inlines: &mut Vec<Inline>,
    defs: &HashMap<String, Blocks>,
    order: &mut Vec<String>,
    numbering: &mut HashMap<String, usize>,
) {
    for inline in inlines.iter_mut() {
        // A footnote ref is a `Span#fnrefN` (FootnotesTransform output). Only
        // treat it as one if the matching definition exists — leaves broken
        // refs (and margin-mode spans with no section) untouched.
        if let Inline::Span(span) = inline {
            if let Some(suffix) = span.attr.0.strip_prefix("fnref") {
                let fn_id = format!("fn{suffix}");
                if defs.contains_key(&fn_id) {
                    let n = match numbering.get(&fn_id) {
                        Some(n) => *n,
                        None => {
                            let n = order.len() + 1;
                            order.push(fn_id.clone());
                            numbering.insert(fn_id, n);
                            n
                        }
                    };
                    let si = span.source_info.clone();
                    *inline = Inline::Superscript(Superscript {
                        content: vec![Inline::Str(Str {
                            text: n.to_string(),
                            source_info: si.clone(),
                        })],
                        source_info: si,
                    });
                    continue;
                }
            }
        }
        // Otherwise recurse into container inlines (a ref may sit inside emph,
        // a link, a user span, …).
        match inline {
            Inline::Emph(x) => renumber_refs_inlines(&mut x.content, defs, order, numbering),
            Inline::Strong(x) => renumber_refs_inlines(&mut x.content, defs, order, numbering),
            Inline::Strikeout(x) => renumber_refs_inlines(&mut x.content, defs, order, numbering),
            Inline::Superscript(x) => renumber_refs_inlines(&mut x.content, defs, order, numbering),
            Inline::Subscript(x) => renumber_refs_inlines(&mut x.content, defs, order, numbering),
            Inline::SmallCaps(x) => renumber_refs_inlines(&mut x.content, defs, order, numbering),
            Inline::Quoted(x) => renumber_refs_inlines(&mut x.content, defs, order, numbering),
            Inline::Cite(x) => renumber_refs_inlines(&mut x.content, defs, order, numbering),
            Inline::Link(x) => renumber_refs_inlines(&mut x.content, defs, order, numbering),
            Inline::Span(x) => renumber_refs_inlines(&mut x.content, defs, order, numbering),
            Inline::Underline(x) => renumber_refs_inlines(&mut x.content, defs, order, numbering),
            Inline::Delete(x) => renumber_refs_inlines(&mut x.content, defs, order, numbering),
            Inline::Insert(x) => renumber_refs_inlines(&mut x.content, defs, order, numbering),
            Inline::Highlight(x) => renumber_refs_inlines(&mut x.content, defs, order, numbering),
            _ => {}
        }
    }
}
