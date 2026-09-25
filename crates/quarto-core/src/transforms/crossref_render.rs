/*
 * transforms/crossref_render.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Finalization-phase rendering of crossref custom nodes.
 */

//! Finalization-phase transform for crossref custom nodes.
//!
//! Converts the two front-end crossref custom node types into shapes the
//! writer knows how to emit:
//!
//! - [`CustomNode("FloatRefTarget")`](crate::crossref::FLOAT_REF_TARGET)
//!   → Pandoc's native `Figure` for figure-kind targets (so the HTML
//!   writer emits `<figure><figcaption>...</figcaption></figure>`), or a
//!   `Div` wrapping the content with the caption as a trailing paragraph
//!   for table- and listing-kind targets (where Pandoc's native `Figure`
//!   isn't the right enclosing element).
//! - [`CustomNode("CrossrefResolvedRef")`](crate::crossref::CROSSREF_RESOLVED_REF)
//!   → `Link` inline pointing at `#<identifier>` with text like
//!   `"Figure\u{a0}1"` (rendered from `kind` + `order.order`).
//!
//! ## Caption numbering
//!
//! A caption like "An overview of the pipeline" becomes "Figure 1: An
//! overview of the pipeline" — the `kind` + `order` prefix is prepended.
//! Unnumbered targets (no `order` in plain_data) simply keep the caption
//! as-is. The separator, sequence format, and localization live in a
//! later task (Q1 supports `crossref.fig-prefix`, `title-delim`, etc.);
//! Phase 1 hard-codes the English defaults: `"<Kind> <N>: "`.
//!
//! ## Format scope
//!
//! For Phase 1 we only target HTML via Pandoc's native Figure shape,
//! which is the right structure for all HTML-family formats. LaTeX /
//! Typst back-ends will need their own rendering transforms that emit
//! `\ref` / `@label` into raw blocks; those land later and are wired in
//! a format-specific pipeline.

use quarto_pandoc_types::attr::{Attr, AttrSourceInfo, TargetSourceInfo};
use quarto_pandoc_types::block::{Block, Blocks, Div, Figure, Header};
use quarto_pandoc_types::caption::Caption;
use quarto_pandoc_types::custom::{CustomNode, Slot};
use quarto_pandoc_types::inline::{Inline, Inlines, Link, Math, Space, Span, Str};
use quarto_pandoc_types::pandoc::Pandoc;
use quarto_source_map::SourceInfo;

use crate::Result;
use crate::crossref::{
    CROSSREF_RESOLVED_REF, EQ_NUMBER_ATTR, EQUATION, FLOAT_REF_TARGET, PROOF, THEOREM,
    format_chapter_index, format_section_number,
};
use crate::language::LanguageTerms;
use crate::render::RenderContext;
use crate::transform::{AstTransform, TransformPhase};

/// Transform that converts FloatRefTarget / CrossrefResolvedRef custom
/// nodes into writer-visible shapes.
pub struct CrossrefRenderTransform;

impl CrossrefRenderTransform {
    pub fn new() -> Self {
        Self
    }
}

impl Default for CrossrefRenderTransform {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait(?Send)]
impl AstTransform for CrossrefRenderTransform {
    fn name(&self) -> &str {
        "crossref-render"
    }

    fn phase(&self) -> TransformPhase {
        TransformPhase::Finalization
    }

    async fn transform(&self, ast: &mut Pandoc, ctx: &mut RenderContext) -> Result<()> {
        // Localized terms (bd-llhlzd7p): reference text prefers the
        // `crossref-<type>-prefix` term (Q1 semantics; prefix falls back to
        // title), and proof labels use `environment-proof-title`. `None`
        // when the LanguageResolveStage hasn't run (direct unit tests) —
        // the node's `kind` / English defaults apply then.
        let terms = LanguageTerms::from_meta(&ast.meta);
        let chapters = ast
            .meta
            .get("crossref")
            .and_then(|c| c.get("chapters"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let max_heading = ctx.crossref_index.as_ref().map_or(7, |i| i.max_heading);
        let mut fs = FloatState {
            html_float_dom: ctx.format.identifier.is_html_based(),
            used_ids: collect_document_ids(&ast.blocks),
            chapters,
            max_heading,
            chapter_seed: ctx.chapter_seed,
        };
        render_blocks(&mut ast.blocks, terms.as_ref(), &mut fs);
        Ok(())
    }
}

fn render_blocks(blocks: &mut Blocks, terms: Option<&LanguageTerms>, fs: &mut FloatState) {
    for block in blocks.iter_mut() {
        render_block(block, terms, fs);
    }
}

fn render_block(block: &mut Block, terms: Option<&LanguageTerms>, fs: &mut FloatState) {
    // Recurse into children.
    match block {
        Block::BlockQuote(bq) => render_blocks(&mut bq.content, terms, fs),
        Block::OrderedList(ol) => {
            for item in &mut ol.content {
                render_blocks(item, terms, fs);
            }
        }
        Block::BulletList(bl) => {
            for item in &mut bl.content {
                render_blocks(item, terms, fs);
            }
        }
        Block::DefinitionList(dl) => {
            for (term, defs) in &mut dl.content {
                render_inlines(term, terms, fs);
                for def in defs {
                    render_blocks(def, terms, fs);
                }
            }
        }
        Block::Figure(fig) => {
            render_blocks(&mut fig.content, terms, fs);
            if let Some(long) = fig.caption.long.as_mut() {
                render_blocks(long, terms, fs);
            }
            if let Some(short) = fig.caption.short.as_mut() {
                render_inlines(short, terms, fs);
            }
        }
        Block::Div(div) => render_blocks(&mut div.content, terms, fs),
        Block::Paragraph(p) => render_inlines(&mut p.content, terms, fs),
        Block::Plain(p) => render_inlines(&mut p.content, terms, fs),
        Block::LineBlock(lb) => {
            for line in &mut lb.content {
                render_inlines(line, terms, fs);
            }
        }
        Block::Header(h) => {
            render_inlines(&mut h.content, terms, fs);
            inject_header_number(h, terms, fs);
        }
        Block::Custom(node) => {
            // Recurse into slots first so nested resolved refs are rendered.
            for (_k, slot) in node.slots.iter_mut() {
                match slot {
                    Slot::Block(b) => render_block(b, terms, fs),
                    Slot::Blocks(bs) => render_blocks(bs, terms, fs),
                    Slot::Inline(i) => render_inline(i, terms, fs),
                    Slot::Inlines(is) => render_inlines(is, terms, fs),
                }
            }
        }
        _ => {}
    }

    // Convert this node if it's a recognized crossref block custom type.
    if let Block::Custom(node) = block {
        if node.type_name == FLOAT_REF_TARGET {
            let replacement = render_float_ref_target(take_custom_node(node), fs);
            *block = replacement;
        } else if node.type_name == THEOREM {
            let replacement = render_theorem(take_custom_node(node), fs);
            *block = replacement;
        } else if node.type_name == PROOF {
            let replacement = render_proof(take_custom_node(node), terms);
            *block = replacement;
        }
    }

    // Shape 2 (bd-hcp8m3ve): a standalone (non-crossref) `Figure` on an
    // HTML-family format gets Q1's `renderHtmlFigure` wrapper —
    // `Div(.quarto-figure .quarto-figure-<align>)` with the figure's id
    // moved onto the wrapper. Float figures are excluded by their
    // `quarto-float` class (they were just built with their own wrapper).
    if fs.html_float_dom {
        let needs_wrap = matches!(
            block,
            Block::Figure(f) if !f.attr.1.iter().any(|c| c == "quarto-float")
        );
        if needs_wrap {
            let Block::Figure(f) = std::mem::replace(
                block,
                Block::Div(Div {
                    attr: (String::new(), Vec::new(), hashlink::LinkedHashMap::new()),
                    content: Vec::new(),
                    source_info: SourceInfo::generated(quarto_source_map::By::unknown()),
                    attr_source: AttrSourceInfo::empty(),
                }),
            ) else {
                unreachable!("guarded by needs_wrap");
            };
            *block = wrap_standalone_figure(f);
        }
    }
}

/// Book-projects P0 / sections.lua's content-prepend: a header carrying
/// the index transform's `number` kv (stashed only under
/// `number-sections`/`number-depth`) gets that number prepended as
/// `Span(Str(number), class="header-section-number")` + Space — inert
/// wherever the kv wasn't stashed. A level-1 heading in an appendix
/// chapter gets Q1's "Appendix A —" shape instead of a bare number
/// (`appendix-title`/`appendix-delim` crossref options; hardcoded English
/// defaults, no crossref-option indirection per the plan).
fn inject_header_number(h: &mut Header, terms: Option<&LanguageTerms>, fs: &FloatState) {
    let Some(number) = h.attr.2.get("number").cloned() else {
        return;
    };
    let source_info = h.source_info.clone();
    let number_span = Inline::Span(Span {
        attr: (
            String::new(),
            vec!["header-section-number".to_string()],
            hashlink::LinkedHashMap::new(),
        ),
        content: vec![Inline::Str(Str {
            text: number,
            source_info: source_info.clone(),
        })],
        source_info: source_info.clone(),
        attr_source: AttrSourceInfo::empty(),
    });
    let mut prefix = if h.level == 1 && fs.chapter_seed.as_ref().is_some_and(|s| s.is_appendix) {
        let title = terms
            .and_then(|t| t.crossref_prefix("apx"))
            .map_or_else(|| "Appendix".to_string(), str::to_string);
        vec![
            Inline::Str(Str {
                text: title,
                source_info: source_info.clone(),
            }),
            Inline::Space(Space {
                source_info: source_info.clone(),
            }),
            number_span,
            Inline::Str(Str {
                text: " —".to_string(),
                source_info: source_info.clone(),
            }),
            Inline::Space(Space { source_info }),
        ]
    } else {
        vec![number_span, Inline::Space(Space { source_info })]
    };
    prefix.append(&mut h.content);
    h.content = prefix;
}

/// Q1 `renderHtmlFigure` for a non-crossref figure: move the id to a
/// `Div(.quarto-figure .quarto-figure-<align>)` wrapper; alignment comes
/// from the contained image's `fig-align` (default `center`, stripped).
fn wrap_standalone_figure(mut f: Figure) -> Block {
    let harvested = harvest_figure_attrs(&mut f.content);
    let (align, style, forwarded_classes) = match harvested {
        Some(h) => (
            h.align.unwrap_or_else(|| "center".to_string()),
            h.style,
            h.forwarded_classes,
        ),
        None => ("center".to_string(), None, Vec::new()),
    };
    let id = std::mem::take(&mut f.attr.0);
    let mut classes = vec![
        "quarto-figure".to_string(),
        format!("quarto-figure-{align}"),
    ];
    for c in forwarded_classes {
        if !classes.contains(&c) {
            classes.push(c);
        }
    }
    let mut kvs: hashlink::LinkedHashMap<String, String> = hashlink::LinkedHashMap::new();
    if let Some(style) = style {
        kvs.insert("style".to_string(), style);
    }
    let source_info = f.source_info.clone();
    Block::Div(Div {
        attr: (id, classes, kvs),
        content: vec![Block::Figure(f)],
        source_info,
        attr_source: AttrSourceInfo::empty(),
    })
}

/// Swap out a `CustomNode` in place with a placeholder, returning the
/// original so rendering can take ownership without cloning the whole
/// subtree. The placeholder is immediately replaced by the caller; it
/// never reaches downstream code.
fn take_custom_node(node: &mut CustomNode) -> CustomNode {
    std::mem::replace(
        node,
        CustomNode::new(
            "_placeholder",
            (String::new(), Vec::new(), hashlink::LinkedHashMap::new()),
            node.source_info.clone(),
        ),
    )
}

fn render_inlines(inlines: &mut Inlines, terms: Option<&LanguageTerms>, fs: &mut FloatState) {
    for inline in inlines.iter_mut() {
        render_inline(inline, terms, fs);
    }
}

fn render_inline(inline: &mut Inline, terms: Option<&LanguageTerms>, fs: &mut FloatState) {
    match inline {
        Inline::Emph(e) => render_inlines(&mut e.content, terms, fs),
        Inline::Underline(u) => render_inlines(&mut u.content, terms, fs),
        Inline::Strong(s) => render_inlines(&mut s.content, terms, fs),
        Inline::Strikeout(s) => render_inlines(&mut s.content, terms, fs),
        Inline::Superscript(s) => render_inlines(&mut s.content, terms, fs),
        Inline::Subscript(s) => render_inlines(&mut s.content, terms, fs),
        Inline::SmallCaps(s) => render_inlines(&mut s.content, terms, fs),
        Inline::Quoted(q) => render_inlines(&mut q.content, terms, fs),
        Inline::Link(l) => render_inlines(&mut l.content, terms, fs),
        Inline::Image(i) => render_inlines(&mut i.content, terms, fs),
        Inline::Note(n) => render_blocks(&mut n.content, terms, fs),
        Inline::Span(s) => render_inlines(&mut s.content, terms, fs),
        Inline::Insert(i) => render_inlines(&mut i.content, terms, fs),
        Inline::Delete(d) => render_inlines(&mut d.content, terms, fs),
        Inline::Highlight(h) => render_inlines(&mut h.content, terms, fs),
        Inline::Custom(node) => {
            for (_k, slot) in node.slots.iter_mut() {
                match slot {
                    Slot::Block(b) => render_block(b, terms, fs),
                    Slot::Blocks(bs) => render_blocks(bs, terms, fs),
                    Slot::Inline(i) => render_inline(i, terms, fs),
                    Slot::Inlines(is) => render_inlines(is, terms, fs),
                }
            }
        }
        _ => {}
    }

    if let Inline::Custom(node) = inline {
        if node.type_name == CROSSREF_RESOLVED_REF {
            *inline = render_resolved_ref(take_custom_node(node), terms, fs);
        } else if node.type_name == EQUATION {
            *inline = render_equation(take_custom_node(node), fs);
        }
    }
}

/// Traversal state for float rendering (bd-hcp8m3ve).
struct FloatState {
    /// HTML-family output → emit the Q1-verbatim float DOM shape
    /// (see `claude-notes/designs/float-layout-class-taxonomy.md`).
    html_float_dom: bool,
    /// Every id in the document, used to pick collision-free figcaption
    /// ids (`<float-id>-caption`, disambiguated only on real collision —
    /// replaces Q1's uuid suffix). Generated ids are inserted as chosen.
    used_ids: std::collections::HashSet<String>,
    /// `crossref.chapters` from document metadata — config, not state; it
    /// rides along here because it's consulted by `render_resolved_ref`
    /// deep in the walk (book-projects P0).
    chapters: bool,
    /// The document's `CrossrefIndex::max_heading` (Q1's maxHeading);
    /// config for the same reason as `chapters`. Defaults to 7 when no
    /// index ran (direct unit tests).
    max_heading: u32,
    /// The per-file chapter seed (book-projects P0/P4) — rides along for
    /// the same reason as `chapters`/`max_heading`: the header number
    /// injection reads its `is_appendix` ("Appendix A —" shape for a
    /// level-1 heading) and the float/equation/ref display numbers read
    /// its `chapter_number` (`format_crossref_number`).
    chapter_seed: Option<crate::render::ChapterSeed>,
}

/// Compose a crossref *display* number from the flat per-type counter and
/// the per-file chapter seed (book-projects P4).
///
/// `None` → today's exact bare `"{order}"` (non-book renders, byte-for-byte);
/// `Some(seed)` → `"{chapter}.{order}"`, with the chapter component a letter
/// when the seed is an appendix seed — the same letter conversion the
/// `@sec-` presentation path uses
/// ([`format_chapter_index`], Q1's `formatChapterIndex`).
///
/// This is the display-side half of chapter-local numbering: the counter
/// (`order.order`) keeps counting flat across the whole chapter file; only
/// the *presented* number is chapter-scoped. Matches Q1, where the Lua
/// `order` counter is flat and `file.bookItemNumber` supplies the chapter
/// prefix at display time.
pub(crate) fn format_crossref_number(
    order: u32,
    chapter_seed: Option<&crate::render::ChapterSeed>,
) -> String {
    match chapter_seed {
        None => order.to_string(),
        Some(seed) => format!(
            "{}.{order}",
            format_chapter_index(seed.chapter_number, seed.is_appendix)
        ),
    }
}

/// Collect every element id in the document (block and inline attrs).
fn collect_document_ids(blocks: &Blocks) -> std::collections::HashSet<String> {
    fn add(id: &str, out: &mut std::collections::HashSet<String>) {
        if !id.is_empty() {
            out.insert(id.to_string());
        }
    }
    fn walk_inlines(inlines: &[Inline], out: &mut std::collections::HashSet<String>) {
        for inline in inlines {
            match inline {
                Inline::Span(s) => {
                    add(&s.attr.0, out);
                    walk_inlines(&s.content, out);
                }
                Inline::Link(l) => {
                    add(&l.attr.0, out);
                    walk_inlines(&l.content, out);
                }
                Inline::Image(i) => {
                    add(&i.attr.0, out);
                    walk_inlines(&i.content, out);
                }
                Inline::Emph(e) => walk_inlines(&e.content, out),
                Inline::Underline(u) => walk_inlines(&u.content, out),
                Inline::Strong(s) => walk_inlines(&s.content, out),
                Inline::Strikeout(s) => walk_inlines(&s.content, out),
                Inline::Superscript(s) => walk_inlines(&s.content, out),
                Inline::Subscript(s) => walk_inlines(&s.content, out),
                Inline::SmallCaps(s) => walk_inlines(&s.content, out),
                Inline::Quoted(q) => walk_inlines(&q.content, out),
                Inline::Note(n) => walk(&n.content, out),
                Inline::Custom(c) => {
                    add(&c.attr.0, out);
                    for (_k, slot) in c.slots.iter() {
                        walk_slot(slot, out);
                    }
                }
                _ => {}
            }
        }
    }
    fn walk_slot(slot: &Slot, out: &mut std::collections::HashSet<String>) {
        match slot {
            Slot::Block(b) => walk(std::slice::from_ref(&**b), out),
            Slot::Blocks(bs) => walk(bs, out),
            Slot::Inline(i) => walk_inlines(std::slice::from_ref(&**i), out),
            Slot::Inlines(is) => walk_inlines(is, out),
        }
    }
    fn walk(blocks: &[Block], out: &mut std::collections::HashSet<String>) {
        for block in blocks {
            match block {
                Block::Div(d) => {
                    add(&d.attr.0, out);
                    walk(&d.content, out);
                }
                Block::Header(h) => {
                    add(&h.attr.0, out);
                    walk_inlines(&h.content, out);
                }
                Block::CodeBlock(cb) => add(&cb.attr.0, out),
                Block::Figure(f) => {
                    add(&f.attr.0, out);
                    walk(&f.content, out);
                    if let Some(long) = &f.caption.long {
                        walk(long, out);
                    }
                }
                Block::Table(t) => add(&t.attr.0, out),
                Block::BlockQuote(bq) => walk(&bq.content, out),
                Block::OrderedList(ol) => {
                    for item in &ol.content {
                        walk(item, out);
                    }
                }
                Block::BulletList(bl) => {
                    for item in &bl.content {
                        walk(item, out);
                    }
                }
                Block::DefinitionList(dl) => {
                    for (term, defs) in &dl.content {
                        walk_inlines(term, out);
                        for def in defs {
                            walk(def, out);
                        }
                    }
                }
                Block::Paragraph(p) => walk_inlines(&p.content, out),
                Block::Plain(p) => walk_inlines(&p.content, out),
                Block::LineBlock(lb) => {
                    for line in &lb.content {
                        walk_inlines(line, out);
                    }
                }
                Block::Custom(c) => {
                    add(&c.attr.0, out);
                    for (_k, slot) in c.slots.iter() {
                        walk_slot(slot, out);
                    }
                }
                _ => {}
            }
        }
    }
    let mut out = std::collections::HashSet::new();
    walk(blocks, &mut out);
    out
}

/// Figure attributes harvested (and stripped) from the float's first
/// contained image, mirroring Q1's `get_figure_attributes`:
/// `fig-align` drives the `quarto-figure-<align>` class, `style` is
/// forwarded to the outer div, and `column-*` / `margin-caption` classes
/// are forwarded so page-layout CSS keeps working.
struct HarvestedFigureAttrs {
    align: Option<String>,
    style: Option<String>,
    forwarded_classes: Vec<String>,
}

/// Find the float's first image (not descending into tables — Q1 #7727)
/// and harvest alignment/style/forwardable classes from it, stripping
/// `fig-align` and `style` from the image itself.
fn harvest_figure_attrs(blocks: &mut Blocks) -> Option<HarvestedFigureAttrs> {
    fn from_image(img: &mut quarto_pandoc_types::inline::Image) -> HarvestedFigureAttrs {
        let align = img.attr.2.remove("fig-align");
        let style = img.attr.2.remove("style");
        let forwarded_classes = img
            .attr
            .1
            .iter()
            .filter(|c| c.starts_with("column-") || c.as_str() == "margin-caption")
            .cloned()
            .collect();
        HarvestedFigureAttrs {
            align,
            style,
            forwarded_classes,
        }
    }
    fn scan_inlines(inlines: &mut Inlines) -> Option<HarvestedFigureAttrs> {
        for inline in inlines {
            match inline {
                Inline::Image(img) => return Some(from_image(img)),
                Inline::Link(l) => {
                    if let Some(h) = scan_inlines(&mut l.content) {
                        return Some(h);
                    }
                }
                _ => {}
            }
        }
        None
    }
    for block in blocks {
        match block {
            Block::Table(_) => continue,
            Block::Paragraph(p) => {
                if let Some(h) = scan_inlines(&mut p.content) {
                    return Some(h);
                }
            }
            Block::Plain(p) => {
                if let Some(h) = scan_inlines(&mut p.content) {
                    return Some(h);
                }
            }
            Block::Div(d) => {
                if let Some(h) = harvest_figure_attrs(&mut d.content) {
                    return Some(h);
                }
            }
            _ => {}
        }
    }
    None
}

/// Pick a collision-free figcaption id: `<float-id>-caption`, appending
/// `-1`, `-2`, … only when a real collision exists. Replaces Q1's uuid
/// suffix (see the design doc's figcaption-uuid finding). The chosen id
/// is recorded so later floats can't collide with it either.
fn allocate_caption_id(
    identifier: &str,
    used_ids: &mut std::collections::HashSet<String>,
) -> String {
    let base = format!("{identifier}-caption");
    let chosen = if !used_ids.contains(&base) {
        base
    } else {
        let mut n = 1usize;
        loop {
            let candidate = format!("{base}-{n}");
            if !used_ids.contains(&candidate) {
                break candidate;
            }
            n += 1;
        }
    };
    used_ids.insert(chosen.clone());
    chosen
}

/// Convert a FloatRefTarget custom node into the writer-visible shape.
///
/// For HTML-family formats this is the Q1-verbatim float DOM
/// (`claude-notes/designs/float-layout-class-taxonomy.md`):
///
/// ```text
/// Div(id, [.quarto-float .quarto-figure .quarto-figure-<align> …])
///   └ Figure("", [.quarto-float .quarto-float-<ref>], data-qf-* kvs)
///       └ Div("", [], aria-describedby=<caption-id>) [content]
///       + caption (the writers synthesize <figcaption> from the kvs)
/// ```
///
/// Non-HTML formats keep the earlier shapes: native `Figure` for
/// figure-kind targets, `Div` + trailing caption paragraph otherwise.
fn render_float_ref_target(node: CustomNode, fs: &mut FloatState) -> Block {
    let identifier = node.attr.0.clone();
    let ref_type = node
        .plain_data
        .get("ref_type")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let kind = node
        .plain_data
        .get("kind")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let number = node
        .plain_data
        .get("order")
        .and_then(|v| v.get("order"))
        .and_then(|v| v.as_u64())
        .map(|n| n as u32);

    let source_info = node.source_info.clone();

    // Extract slots
    let mut slots = node.slots;
    let content: Blocks = match slots.remove("content") {
        Some(Slot::Blocks(bs)) => bs,
        _ => Vec::new(),
    };
    let caption_long: Blocks = match slots.remove("caption_long") {
        Some(Slot::Blocks(bs)) => bs,
        _ => Vec::new(),
    };
    let caption_short: Option<Inlines> = match slots.remove("caption_short") {
        Some(Slot::Inlines(is)) => Some(is),
        _ => None,
    };

    let is_uncaptioned = caption_long.is_empty();
    let numbered_caption = prefix_caption(
        caption_long.clone(),
        &kind,
        number.map(|n| format_crossref_number(n, fs.chapter_seed.as_ref())),
    );

    // Only genuine float kinds get the Q1 float DOM. FloatRefTarget nodes
    // also exist for non-float registered prefixes (`sec` sections, `demo`
    // embeds, custom kinds) — those keep the legacy pass-through shapes
    // below, matching Q1 where only float categories reach
    // `float_reftarget_render_html_figure`. Custom float kinds join this
    // set when the crossref.custom float category lands.
    let is_float_kind = matches!(ref_type.as_str(), "fig" | "tbl" | "lst");

    if fs.html_float_dom && is_float_kind {
        let mut content = content;

        // bd-4m2n6qf1: a table float's caption is hoisted into the
        // synthesized `<figcaption>`, but the Table node keeps its own
        // `caption` — so both writers would emit the text twice, as
        // `<table><caption>` *and* as `<figcaption>`. Elide the Table's copy,
        // matching Q1, which does the same at float-parse time
        // (`quarto-pre/parsefiguredivs.lua`: `table.caption =
        // pandoc.Caption{}` at L280 for the div-wrapped form,
        // `el.caption.long = pandoc.Blocks({})` at L544 for the
        // caption-attr form). Q2 builds the float DOM in this
        // Finalization-phase transform, so the elision happens here.
        //
        // Scoped to top-level Tables in the float content — the ones whose
        // caption became the float caption. Skipped when the float is
        // uncaptioned, since then nothing was hoisted and the Table's
        // caption is the only copy of that text.
        if !is_uncaptioned {
            for block in content.iter_mut() {
                if let Block::Table(t) = block {
                    t.caption.long = None;
                    t.caption.short = None;
                }
            }
        }

        // Q1 `get_figure_attributes`: alignment/style/forwardable classes
        // come from the first contained image (never inside a table).
        let harvested = if !matches!(content.first(), Some(Block::Table(_))) {
            harvest_figure_attrs(&mut content)
        } else {
            None
        };
        let (mut align, style, forwarded_classes) = match harvested {
            Some(h) => (
                h.align.unwrap_or_else(|| "center".to_string()),
                h.style,
                h.forwarded_classes,
            ),
            None => ("center".to_string(), None, Vec::new()),
        };

        // Caption location: attr-level `cap-location` / `<ref>-cap-location`
        // (metadata-level configuration lands with the cap-location feature).
        let (_id0, mut user_classes, mut user_kvs) = node.attr;
        let caption_location = user_kvs
            .remove("cap-location")
            .or_else(|| user_kvs.remove(&format!("{ref_type}-cap-location")))
            .unwrap_or_else(|| "bottom".to_string());

        // Listings hard-code left alignment and a `listing` class (Q1 #9724).
        let is_listing = ref_type == "lst";
        if is_listing {
            align = "left".to_string();
            user_classes.push("listing".to_string());
        }

        let caption_id = allocate_caption_id(&identifier, &mut fs.used_ids);

        // Uncaptioned floats still get a label-only caption ("Figure 1")
        // plus the `quarto-uncaptioned` marker, matching Q1. A `Plain`, like
        // every other float caption (the sugar transform's slot contract),
        // so the writer emits bare inlines inside <figcaption>.
        let final_caption = if is_uncaptioned {
            let label = match number {
                // `\u{a0}` between kind and number, matching Q1's
                // titlePrefix (`nbspString()` before the number).
                Some(n) => format!("{kind}\u{a0}{n}"),
                None => kind.clone(),
            };
            vec![Block::Plain(quarto_pandoc_types::block::Plain {
                content: vec![Inline::Str(Str {
                    text: label,
                    source_info: source_info.clone(),
                })],
                source_info: source_info.clone(),
            })]
        } else {
            numbered_caption
        };

        // Content wrapper div carrying aria-describedby (Q1 verbatim).
        let mut wrapper_kvs: hashlink::LinkedHashMap<String, String> =
            hashlink::LinkedHashMap::new();
        wrapper_kvs.insert("aria-describedby".to_string(), caption_id.clone());
        let content_wrapper = Block::Div(Div {
            attr: (String::new(), Vec::new(), wrapper_kvs),
            content,
            source_info: source_info.clone(),
            attr_source: AttrSourceInfo::empty(),
        });

        // Inner <figure>: quarto-float + quarto-float-<ref>, and the
        // data-qf-* kvs both writers use to synthesize the <figcaption>.
        let mut fig_kvs: hashlink::LinkedHashMap<String, String> = hashlink::LinkedHashMap::new();
        fig_kvs.insert("data-qf-ref-type".to_string(), ref_type.clone());
        fig_kvs.insert(
            "data-qf-caption-location".to_string(),
            caption_location.clone(),
        );
        fig_kvs.insert("data-qf-caption-id".to_string(), caption_id.clone());
        if is_uncaptioned {
            fig_kvs.insert("data-qf-uncaptioned".to_string(), "1".to_string());
        }
        let figure = Block::Figure(Figure {
            attr: (
                String::new(),
                vec![
                    "quarto-float".to_string(),
                    format!("quarto-float-{ref_type}"),
                ],
                fig_kvs,
            ),
            caption: Caption {
                short: caption_short,
                long: Some(final_caption),
                source_info: source_info.clone(),
            },
            content: vec![content_wrapper],
            source_info: source_info.clone(),
            attr_source: AttrSourceInfo::empty(),
        });

        // Outer div: user classes + the Q1 taxonomy + forwarded classes.
        user_classes.extend([
            "quarto-float".to_string(),
            "quarto-figure".to_string(),
            format!("quarto-figure-{align}"),
        ]);
        for c in forwarded_classes {
            if !user_classes.contains(&c) {
                user_classes.push(c);
            }
        }
        if let Some(style) = style {
            user_kvs.insert("style".to_string(), style);
        }
        return Block::Div(Div {
            attr: (identifier, user_classes, user_kvs),
            content: vec![figure],
            source_info,
            attr_source: AttrSourceInfo::empty(),
        });
    }

    if ref_type == "fig" {
        // Prefer Pandoc's native Figure so the HTML writer emits
        // `<figure><figcaption>...</figcaption></figure>` with the id.
        Block::Figure(Figure {
            attr: node.attr,
            caption: Caption {
                short: caption_short,
                long: Some(numbered_caption),
                source_info: source_info.clone(),
            },
            content,
            source_info,
            attr_source: AttrSourceInfo::empty(),
        })
    } else {
        // Div wrapper: content + numbered-caption paragraph.
        let mut body = content;
        if !numbered_caption.is_empty() {
            body.extend(numbered_caption);
        }
        let _ = identifier; // id is on node.attr already
        Block::Div(Div {
            attr: node.attr,
            content: body,
            source_info,
            attr_source: AttrSourceInfo::empty(),
        })
    }
}

/// Convert a Theorem custom node into a `Div` structure the HTML writer
/// can serialize.
///
/// Output shape matches Q1 (`theorem.lua::add_renderer` for HTML):
///
/// ```html
/// <div id="thm-x" class="theorem">
///   <p><span class="theorem-title"><strong>Theorem&nbsp;1 (Optional Title)</strong></span> First body text...</p>
///   <p>Second body paragraph...</p>
/// </div>
/// ```
///
/// Key details:
/// - Class list: `theorem` plus the flavor env name when env ≠ `theorem`
///   (so a lemma gets `theorem lemma`; a plain theorem gets just `theorem`).
/// - Label lives inside a `Span(class=theorem-title)` that wraps the
///   `Strong`, so CSS can address the full label block. Pandoc writes
///   this as `<span class="theorem-title"><strong>…</strong></span>`.
/// - Non-breaking space (`\u{a0}`) between the kind and number —
///   prevents line-break between them in the rendered output. Q1 uses
///   the same nbsp via `nbspString()`.
/// - No trailing period after the label; Q1 does not emit one.
/// - If the body is empty or does not start with a Paragraph, an empty
///   placeholder Paragraph containing a single `\u{a0}` is inserted first
///   (Q1's `tprepend(el.content, {pandoc.Para({pandoc.Str '\u{a0}'})})`).
///   The label is then prepended into that first Paragraph. This keeps
///   the label in the normal paragraph flow instead of stranded as its
///   own block.
fn render_theorem(node: CustomNode, fs: &FloatState) -> Block {
    let kind = node
        .plain_data
        .get("kind")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let ref_type = node
        .plain_data
        .get("ref_type")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let number = node
        .plain_data
        .get("order")
        .and_then(|v| v.get("order"))
        .and_then(|v| v.as_u64())
        .map(|n| n as u32);

    let source_info = node.source_info.clone();
    let mut attr = node.attr;

    // Q1 class logic: always `theorem`; also `<env>` when env ≠ theorem.
    // No `ref_type` class — that was a Q2 leak of the internal prefix.
    if !attr.1.iter().any(|c| c == "theorem") {
        attr.1.push("theorem".to_string());
    }
    let env = theorem_env_for(&ref_type);
    if !env.is_empty() && env != "theorem" && !attr.1.iter().any(|c| c == env) {
        attr.1.push(env.to_string());
    }

    let mut slots = node.slots;
    let content: Blocks = match slots.remove("content") {
        Some(Slot::Blocks(bs)) => bs,
        _ => Vec::new(),
    };
    let title: Option<Inlines> = match slots.remove("title") {
        Some(Slot::Inlines(is)) if !is.is_empty() => Some(is),
        _ => None,
    };

    let label = theorem_label_inlines(
        &kind,
        number.map(|n| format_crossref_number(n, fs.chapter_seed.as_ref())),
        title.as_deref(),
        source_info.clone(),
    );

    // Ensure the first block is a Paragraph so the label can be prepended
    // into inline context (not stranded as a standalone block). See doc
    // comment on this function.
    let content = ensure_leading_paragraph_nbsp(content, source_info.clone());
    let body = prepend_theorem_label(content, label, source_info.clone());

    Block::Div(Div {
        attr,
        content: body,
        source_info,
        attr_source: AttrSourceInfo::empty(),
    })
}

/// Map a theorem ref-type prefix to its Q1 env name (which doubles as
/// the flavor CSS class when different from `"theorem"`).
///
/// `"thm"` → `"theorem"` (class list is just `["theorem"]`).
/// `"lem"` → `"lemma"` (class list is `["theorem", "lemma"]`).
///
/// Empty string for unknown / non-theorem prefixes. Kept in sync with
/// `THEOREM_CLASSES` in `transforms::theorem`.
fn theorem_env_for(ref_type: &str) -> &'static str {
    match ref_type {
        "thm" => "theorem",
        "lem" => "lemma",
        "cor" => "corollary",
        "prp" => "proposition",
        "cnj" => "conjecture",
        "def" => "definition",
        "exm" => "example",
        "exr" => "exercise",
        _ => "",
    }
}

/// If `content` is empty or its first block is not a Paragraph, insert a
/// placeholder `Paragraph(Str("\u{a0}"))` at the front. The placeholder
/// gives the label somewhere to live in inline context when the theorem
/// body starts with a display-math or code block. Matches Q1's
/// `tprepend(el.content, {pandoc.Para({pandoc.Str '\u{a0}'})})`.
fn ensure_leading_paragraph_nbsp(mut content: Blocks, source_info: SourceInfo) -> Blocks {
    if matches!(content.first(), Some(Block::Paragraph(_))) {
        return content;
    }
    let nbsp_para = Block::Paragraph(quarto_pandoc_types::block::Paragraph {
        content: vec![Inline::Str(Str {
            text: "\u{a0}".to_string(),
            source_info: source_info.clone(),
        })],
        source_info,
    });
    let mut out = Vec::with_capacity(content.len() + 1);
    out.push(nbsp_para);
    out.append(&mut content);
    out
}

/// Build the label inlines: a `Span(class=theorem-title)` wrapping a
/// `Strong` of `"<Kind>\u{a0}<N>"` plus an optional parenthesized title,
/// followed by a plain space. Matches Q1's `captionPrefix` +
/// `pandoc.Span(pandoc.Strong(...), {"theorem-title"})` shape.
///
/// Components are omitted individually: no number if `number` is None,
/// no parenthesized title if `title` is None. **No trailing period** —
/// Q1 doesn't emit one and some CSS rules assume its absence.
fn theorem_label_inlines(
    kind: &str,
    number: Option<String>,
    title: Option<&[Inline]>,
    source_info: SourceInfo,
) -> Inlines {
    // The kind and the number are joined with `\u{a0}` (non-breaking
    // space) so the label doesn't line-wrap between them. This matches
    // Q1's `ref:extend({nbspString()})` in refs.lua and `captionPrefix`
    // in theorems.lua (which uses `pandoc.Space()` there because Q1's
    // HTML writer emits the space via a later filter pass; for us we
    // produce the nbsp directly).
    let mut head_text = String::new();
    if !kind.is_empty() {
        head_text.push_str(kind);
    }
    if let Some(n) = number {
        if !head_text.is_empty() {
            head_text.push('\u{a0}');
        }
        head_text.push_str(&n);
    }

    let mut strong_content: Inlines = Vec::new();
    if !head_text.is_empty() {
        strong_content.push(Inline::Str(Str {
            text: head_text,
            source_info: source_info.clone(),
        }));
    }
    if let Some(title_inlines) = title {
        // "Theorem 1 (Title)": space + "(" + title + ")".
        strong_content.push(Inline::Str(Str {
            text: " (".to_string(),
            source_info: source_info.clone(),
        }));
        strong_content.extend(title_inlines.iter().cloned());
        strong_content.push(Inline::Str(Str {
            text: ")".to_string(),
            source_info: source_info.clone(),
        }));
    }

    let strong = Inline::Strong(quarto_pandoc_types::inline::Strong {
        content: strong_content,
        source_info: source_info.clone(),
    });
    let span = Inline::Span(Span {
        attr: (
            String::new(),
            vec!["theorem-title".to_string()],
            hashlink::LinkedHashMap::new(),
        ),
        content: vec![strong],
        source_info: source_info.clone(),
        attr_source: AttrSourceInfo::empty(),
    });

    vec![
        span,
        Inline::Str(Str {
            text: " ".to_string(),
            source_info,
        }),
    ]
}

/// Prepend `label` inlines to the first Paragraph of `content`. If
/// `content` is empty or its first block isn't a Paragraph, insert a
/// new label-only Paragraph at the front.
fn prepend_theorem_label(mut content: Blocks, label: Inlines, source_info: SourceInfo) -> Blocks {
    if let Some(Block::Paragraph(first)) = content.first_mut() {
        let mut new_content = label;
        new_content.extend(std::mem::take(&mut first.content));
        first.content = new_content;
        content
    } else {
        let label_para = Block::Paragraph(quarto_pandoc_types::block::Paragraph {
            content: label,
            source_info,
        });
        let mut out = Vec::with_capacity(content.len() + 1);
        out.push(label_para);
        out.extend(content);
        out
    }
}

/// Convert a Proof custom node into a `Div` with an italicized
/// "Proof." prefix (or the user's title).
///
/// Shape:
///
/// ```html
/// <div class="proof">
///   <p><em>Proof.</em> First body...</p>
///   <p>Second body...</p>
/// </div>
/// ```
///
/// Proofs never carry a number. The id (if any) flows through on the
/// Div's `id` attribute so anchor links still work.
fn render_proof(node: CustomNode, terms: Option<&LanguageTerms>) -> Block {
    let source_info = node.source_info.clone();
    let mut attr = node.attr;
    if !attr.1.iter().any(|c| c == "proof") {
        attr.1.push("proof".to_string());
    }

    let mut slots = node.slots;
    let content: Blocks = match slots.remove("content") {
        Some(Slot::Blocks(bs)) => bs,
        _ => Vec::new(),
    };
    let title: Option<Inlines> = match slots.remove("title") {
        Some(Slot::Inlines(is)) if !is.is_empty() => Some(is),
        _ => None,
    };

    // Build the italic label: "*Proof.* " or "*Custom title.* ".
    let mut em_content: Inlines = match title {
        Some(t) => {
            let mut inlines = t;
            inlines.push(Inline::Str(Str {
                text: ".".to_string(),
                source_info: source_info.clone(),
            }));
            inlines
        }
        None => {
            // `environment-proof-title` term ("Proof" → "Demostración", …);
            // the trailing period matches Q1's proof label shape.
            let proof_title = terms
                .and_then(|t| t.get("environment-proof-title"))
                .unwrap_or("Proof");
            vec![Inline::Str(Str {
                text: format!("{proof_title}."),
                source_info: source_info.clone(),
            })]
        }
    };
    // Make the label italic via Emph.
    let label: Inlines = vec![
        Inline::Emph(quarto_pandoc_types::inline::Emph {
            content: std::mem::take(&mut em_content),
            source_info: source_info.clone(),
        }),
        Inline::Str(Str {
            text: " ".to_string(),
            source_info: source_info.clone(),
        }),
    ];

    let body = prepend_theorem_label(content, label, source_info.clone());

    Block::Div(Div {
        attr,
        content: body,
        source_info,
        attr_source: AttrSourceInfo::empty(),
    })
}

/// Convert an Equation custom node into a `Span(id=...)` containing the
/// original `Math(DisplayMath, ...)`, byte-identical to the source, with
/// the equation's number on the reserved [`EQ_NUMBER_ATTR`]
/// (`quarto-eq-number`) attribute.
///
/// Output shape:
///
/// ```html
/// <span id="eq-einstein" quarto-eq-number="1">$$e = mc^2$$</span>
/// ```
///
/// How the number is *typeset* is not decided here: `\tag{N}` is an
/// `amsmath` command only MathJax and KaTeX understand, while a converter
/// that reads math alone needs ` \qquad(N)` or a label outside the math.
/// That choice depends on the format and `html-math-method`, so it is made
/// by `EquationNumberStage`, which runs after user post filters (so a Lua
/// filter can rewrite or delete the attribute) and removes the attribute.
/// The Span wrapper carries the id for anchor linking from `@eq-xxx`
/// references.
fn render_equation(node: CustomNode, fs: &FloatState) -> Inline {
    let number = node
        .plain_data
        .get("order")
        .and_then(|v| v.get("order"))
        .and_then(|v| v.as_u64())
        .map(|n| n as u32);

    let source_info = node.source_info.clone();
    let mut attr = node.attr.clone();
    if let Some(n) = number {
        attr.2.insert(
            EQ_NUMBER_ATTR.to_string(),
            format_crossref_number(n, fs.chapter_seed.as_ref()),
        );
    }

    // Extract the math inline from the content slot. The math text is
    // passed through untouched (see the doc comment).
    let mut slots = node.slots;
    let content = match slots.remove("content") {
        Some(Slot::Inlines(mut is)) if !is.is_empty() => vec![is.remove(0)],
        _ => vec![],
    };

    Inline::Span(Span {
        attr,
        content,
        source_info,
        attr_source: AttrSourceInfo::empty(),
    })
}

/// Convert a CrossrefResolvedRef custom node into a `Link` inline.
///
/// Link text is `"<Kind> <N>"` when the ref is resolved, or the literal
/// `"?id?"` (wrapped visibly) for unresolved refs so the failure is
/// obvious in the rendered document.
fn render_resolved_ref(node: CustomNode, terms: Option<&LanguageTerms>, fs: &FloatState) -> Inline {
    let identifier = node
        .plain_data
        .get("identifier")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    // Reference text uses the `crossref-<type>-prefix` term (which falls
    // back to `crossref-<type>-title`) when the language table defines one;
    // otherwise the node's `kind` (registry display name — already
    // localized for built-ins, or `crossref.custom`'s reference-prefix).
    let ref_type = node
        .plain_data
        .get("ref_type")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let kind = terms.and_then(|t| t.crossref_prefix(ref_type)).map_or_else(
        || {
            node.plain_data
                .get("kind")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string()
        },
        |s| s.to_string(),
    );
    let resolved = node
        .plain_data
        .get("resolved")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let number = node
        .plain_data
        .get("order")
        .and_then(|v| v.get("order"))
        .and_then(|v| v.as_u64())
        .map(|n| n as u32);
    // Cross-chapter patch (book-projects P5): `resolved_number` was already
    // composed at aggregation time from the *owning* chapter's raw order +
    // seed; `format_crossref_number` here would wrongly apply the
    // *rendering* chapter's seed and renumber the target. `target_href` is
    // the owning chapter's page-relative link target; absent for
    // locally-resolved refs, which keep the `#id` anchor.
    let resolved_number = node
        .plain_data
        .get("resolved_number")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let target_href = node
        .plain_data
        .get("target_href")
        .and_then(|v| v.as_str())
        .map(str::to_string);

    let source_info = node.source_info.clone();

    // Non-breaking space between the kind and the number so the rendered
    // link text doesn't break across lines — "Figure\u{a0}1" should always
    // stay together. Matches Q1's `ref:extend({nbspString()})` in
    // `refs.lua`. Applies uniformly to all crossref categories (Theorem,
    // Figure, Table, Equation, …) — Q1 does the same.
    let text = if resolved {
        if ref_type == "sec" {
            // `sec` numbers are the target's *section path*, not the
            // per-type counter (Q1 refs.lua + format.lua; book-projects P0).
            // The cross-chapter patch rewrites `order`/`in_appendix` to the
            // owning chapter's values, so this path needs no seed and works
            // unchanged for cross-chapter targets.
            sec_ref_text(&node, &kind, terms, fs)
        } else if let Some(num) = resolved_number {
            format!("{kind}\u{a0}{num}")
        } else {
            match number {
                Some(n) => {
                    format!(
                        "{kind}\u{a0}{}",
                        format_crossref_number(n, fs.chapter_seed.as_ref())
                    )
                }
                None => kind.clone(),
            }
        }
    } else {
        format!("?{identifier}?")
    };

    let content: Inlines = vec![Inline::Str(Str {
        text,
        source_info: source_info.clone(),
    })];
    let target = (
        target_href.unwrap_or_else(|| format!("#{identifier}")),
        String::new(),
    );

    // Every crossref link carries `quarto-xref`; unresolved refs additionally
    // carry `quarto-unresolved-ref` so downstream extensions can loudly style a
    // missing reference (matching TS Quarto's crossref/refs.lua:94, which uses
    // the class as its failure signal). Additive per Carlos, 2026-07-21
    // (bd-28iqotrt, audit row 17): we keep Q2's louder `?id?` Link + the dangling
    // `#id` target rather than switching to TS's plain Span.
    let mut classes = vec!["quarto-xref".to_string()];
    if !resolved {
        classes.push("quarto-unresolved-ref".to_string());
    }

    Inline::Link(Link {
        attr: (String::new(), classes, hashlink::LinkedHashMap::new()),
        content,
        target,
        source_info,
        attr_source: AttrSourceInfo::empty(),
        target_source: TargetSourceInfo::empty(),
    })
}

/// Build the link text for a resolved `sec` ref: "Section 1.2" by default,
/// "Chapter N" / "Appendix A" for a chapter-level heading under
/// `crossref.chapters` (Q1 refs.lua's `isChapterRef` prefix swap; the
/// number itself is Q1's `sectionNumber`, ported as
/// [`format_section_number`]).
///
/// `kind` is the already-resolved `sec` prefix (language-term or registry
/// display name); it is overridden only for the ch/apx swap, whose fallback
/// when no language table is loaded (unit tests) is hardcoded English —
/// there is no registry entry for "ch"/"apx" to fall back to.
fn sec_ref_text(
    node: &CustomNode,
    kind: &str,
    terms: Option<&LanguageTerms>,
    fs: &FloatState,
) -> String {
    let section: Vec<u32> = node
        .plain_data
        .get("order")
        .and_then(|v| v.get("section"))
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_u64().map(|n| n as u32))
                .collect()
        })
        .unwrap_or_default();
    if section.is_empty() {
        return kind.to_string();
    }
    let in_appendix = node
        .plain_data
        .get("in_appendix")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    // Q1's isChapterRef: no nonzero components below the top. For header
    // targets (whose own path always ends >= 1) this is exactly
    // `section.len() == 1`, but we port the literal predicate.
    let is_chapter_ref = fs.chapters && section[1..].iter().all(|&c| c == 0);
    let kind = if is_chapter_ref {
        let (prefix_type, fallback) = if in_appendix {
            ("apx", "Appendix")
        } else {
            ("ch", "Chapter")
        };
        terms
            .and_then(|t| t.crossref_prefix(prefix_type))
            .map_or_else(|| fallback.to_string(), str::to_string)
    } else {
        kind.to_string()
    };
    let num = format_section_number(&section, fs.max_heading, in_appendix);
    if num.is_empty() {
        kind
    } else {
        format!("{kind}\u{a0}{num}")
    }
}

/// Prepend a numbered prefix onto the first block of a caption block list,
/// returning a fresh Blocks. No-op if the kind is empty or the caption is
/// empty.
///
/// The first caption block may be a `Paragraph` *or* a `Plain`: the
/// div-form trailing paragraph arrives as `Paragraph`, while Pandoc-native
/// `Figure` / `Table` captions (`![cap](img){#fig-x}`, `: cap {#tbl-x}`)
/// arrive as `Plain`. The prefix lands in either and the block type is
/// preserved, mirroring Q1's `decorate_caption_with_crossref`, which
/// prepends into `caption_long.content` regardless of block type. If the
/// first block carries no inlines at all (a code block, a nested div), a
/// label-only `Plain` is inserted in front rather than dropping the prefix
/// (bd-n3sark9b — the Paragraph-only match used to silently skip Plain).
fn prefix_caption(caption: Blocks, kind: &str, number: Option<String>) -> Blocks {
    if kind.is_empty() || caption.is_empty() {
        return caption;
    }
    // Q1's titlePrefix joins kind and number with a non-breaking space
    // (`nbspString()`, format.lua), then title-delim + a regular space:
    // "Figure\u{a0}1: ". The docx writer (via the vendored Lua filters)
    // already emits that shape; matching it keeps the native HTML and
    // pandoc-hybrid legs byte-identical (pandoc_goldens).
    let prefix_text = match number {
        Some(n) => format!("{kind}\u{a0}{n}: "),
        None => format!("{kind}: "),
    };
    let mut out = caption;
    // Prepend a single Str carrying the trailing space, so we don't have
    // to synthesize Space inlines.
    let prefix_into = |content: &mut Inlines, src: SourceInfo| {
        let mut new_content: Inlines = vec![Inline::Str(Str {
            text: prefix_text.clone(),
            source_info: src,
        })];
        new_content.append(content);
        *content = new_content;
    };
    match out.first_mut() {
        Some(Block::Paragraph(first)) => {
            let src = first.source_info.clone();
            prefix_into(&mut first.content, src);
        }
        Some(Block::Plain(first)) => {
            let src = first.source_info.clone();
            prefix_into(&mut first.content, src);
        }
        Some(other) => {
            let src = other.source_info().clone();
            out.insert(
                0,
                Block::Plain(quarto_pandoc_types::block::Plain {
                    content: vec![Inline::Str(Str {
                        text: prefix_text,
                        source_info: src.clone(),
                    })],
                    source_info: src,
                }),
            );
        }
        None => unreachable!("caption checked non-empty above"),
    }
    out
}

/// Placeholder to silence potentially-unused helper warnings in narrow
/// test builds.
#[allow(dead_code)]
fn _dummy(a: Attr) -> Attr {
    a
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crossref::RefTypeRegistry;
    use crate::transforms::{
        CrossrefIndexTransform, CrossrefResolveTransform, EquationLabelTransform,
        FloatRefTargetSugarTransform, ProofSugarTransform, TheoremSugarTransform,
    };
    use hashlink::LinkedHashMap;
    use quarto_pandoc_types::block::{Block, CodeBlock, Div, Paragraph};
    use quarto_pandoc_types::inline::{Citation, CitationMode, Cite};
    use quarto_source_map::{FileId, SourceInfo};

    fn si() -> SourceInfo {
        SourceInfo::original(FileId(0), 0, 0)
    }

    fn attr_id(id: &str) -> Attr {
        (id.to_string(), Vec::new(), LinkedHashMap::new())
    }

    /// A default `FloatState` for direct unit tests of the walk's leaf
    /// renderers (`render_equation` etc.) — no seed, non-chapter config.
    fn float_state_for_tests() -> FloatState {
        FloatState {
            html_float_dom: true,
            used_ids: Default::default(),
            chapters: false,
            max_heading: 7,
            chapter_seed: None,
        }
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

    fn fig_div(id: &str, cap: &str) -> Block {
        Block::Div(Div {
            attr: attr_id(id),
            content: vec![
                Block::CodeBlock(CodeBlock {
                    attr: (String::new(), vec!["python".into()], LinkedHashMap::new()),
                    text: "x=1".into(),
                    source_info: si(),
                    attr_source: AttrSourceInfo::empty(),
                }),
                para(cap),
            ],
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        })
    }

    async fn run_full(blocks: Vec<Block>) -> Pandoc {
        use crate::format::Format;
        use crate::project::{DocumentInfo, ProjectConfig, ProjectContext};
        use crate::render::{BinaryDependencies, RenderContext};
        use std::path::PathBuf;
        let project = ProjectContext {
            dir: PathBuf::from("/p"),
            config: ProjectConfig::default(),
            is_single_file: true,
            files: vec![],
            output_dir: PathBuf::from("/p"),

            ..Default::default()
        };
        let doc = DocumentInfo::from_path("/p/t.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);
        ctx.ref_type_registry = Some(RefTypeRegistry::builtin());

        let mut ast = Pandoc {
            meta: quarto_pandoc_types::ConfigValue::default(),
            blocks,
        };
        TheoremSugarTransform::new()
            .transform(&mut ast, &mut ctx)
            .await
            .unwrap();
        ProofSugarTransform::new()
            .transform(&mut ast, &mut ctx)
            .await
            .unwrap();
        FloatRefTargetSugarTransform::new()
            .transform(&mut ast, &mut ctx)
            .await
            .unwrap();
        EquationLabelTransform::new()
            .transform(&mut ast, &mut ctx)
            .await
            .unwrap();
        CrossrefIndexTransform::new()
            .transform(&mut ast, &mut ctx)
            .await
            .unwrap();
        CrossrefResolveTransform::new()
            .transform(&mut ast, &mut ctx)
            .await
            .unwrap();
        CrossrefRenderTransform::new()
            .transform(&mut ast, &mut ctx)
            .await
            .unwrap();
        ast
    }

    fn cite(id: &str) -> Inline {
        Inline::Cite(Cite {
            citations: vec![Citation {
                id: id.to_string(),
                prefix: vec![],
                suffix: vec![],
                mode: CitationMode::NormalCitation,
                note_num: 0,
                hash: 0,
                id_source: None,
            }],
            content: vec![str_inline(&format!("@{}", id))],
            source_info: si(),
        })
    }

    fn header(level: usize, id: &str, text: &str) -> Block {
        Block::Header(quarto_pandoc_types::block::Header {
            level,
            attr: attr_id(id),
            content: vec![str_inline(text)],
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        })
    }

    /// `crossref: {chapters: <v>}` document metadata.
    fn meta_with_chapters(v: bool) -> quarto_pandoc_types::ConfigValue {
        use quarto_pandoc_types::{ConfigMapEntry, ConfigValue};
        let entry = |key: &str, value: ConfigValue| ConfigMapEntry {
            key: key.to_string(),
            key_source: si(),
            value,
        };
        ConfigValue::new_map(
            vec![entry(
                "crossref",
                ConfigValue::new_map(
                    vec![entry("chapters", ConfigValue::new_bool(v, si()))],
                    si(),
                ),
            )],
            si(),
        )
    }

    /// Same pipeline as [`run_full`], with document metadata and an optional
    /// chapter seed (book-projects P0: `crossref.chapters` and appendix
    /// state both arrive this way).
    async fn run_full_opts(
        blocks: Vec<Block>,
        meta: quarto_pandoc_types::ConfigValue,
        seed: Option<crate::render::ChapterSeed>,
    ) -> Pandoc {
        use crate::format::Format;
        use crate::project::{DocumentInfo, ProjectConfig, ProjectContext};
        use crate::render::{BinaryDependencies, RenderContext};
        use std::path::PathBuf;
        let project = ProjectContext {
            dir: PathBuf::from("/p"),
            config: ProjectConfig::default(),
            is_single_file: true,
            files: vec![],
            output_dir: PathBuf::from("/p"),

            ..Default::default()
        };
        let doc = DocumentInfo::from_path("/p/t.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);
        ctx.ref_type_registry = Some(RefTypeRegistry::builtin());
        ctx.chapter_seed = seed;

        let mut ast = Pandoc { meta, blocks };
        TheoremSugarTransform::new()
            .transform(&mut ast, &mut ctx)
            .await
            .unwrap();
        ProofSugarTransform::new()
            .transform(&mut ast, &mut ctx)
            .await
            .unwrap();
        FloatRefTargetSugarTransform::new()
            .transform(&mut ast, &mut ctx)
            .await
            .unwrap();
        EquationLabelTransform::new()
            .transform(&mut ast, &mut ctx)
            .await
            .unwrap();
        CrossrefIndexTransform::new()
            .transform(&mut ast, &mut ctx)
            .await
            .unwrap();
        CrossrefResolveTransform::new()
            .transform(&mut ast, &mut ctx)
            .await
            .unwrap();
        CrossrefRenderTransform::new()
            .transform(&mut ast, &mut ctx)
            .await
            .unwrap();
        ast
    }

    /// Extract the link text of the rendered ref at `content[idx]`.
    fn ref_link_text(content: &[Inline], idx: usize) -> String {
        let Inline::Link(link) = &content[idx] else {
            panic!("expected Link, got {:?}", content[idx]);
        };
        let Inline::Str(s) = &link.content[0] else {
            panic!("expected Str link text, got {:?}", link.content[0]);
        };
        s.text.clone()
    }

    #[tokio::test]
    async fn figure_target_renders_to_pandoc_figure() {
        // bd-hcp8m3ve: the Figure now sits inside the Q1-shape outer div;
        // the id lives on the div, and the caption prefix is unchanged.
        let ast = run_full(vec![fig_div("fig-1", "Caption A")]).await;
        let (outer, f) = float_shape(&ast.blocks[0]);
        assert_eq!(outer.attr.0, "fig-1");
        let long = f.caption.long.as_ref().unwrap();
        let Block::Plain(p) = &long[0] else {
            panic!();
        };
        // First inline should be the "Figure 1: " prefix.
        let Inline::Str(s) = &p.content[0] else {
            panic!();
        };
        assert_eq!(s.text, "Figure\u{a0}1: ");
        // Followed by the original caption inline.
        let Inline::Str(s) = &p.content[1] else {
            panic!();
        };
        assert_eq!(s.text, "Caption A");
    }

    #[tokio::test]
    async fn table_target_renders_to_div_with_prefixed_caption() {
        use quarto_pandoc_types::table::{Table, TableBody, TableFoot, TableHead};
        let table = Block::Table(Table {
            attr: (String::new(), Vec::new(), LinkedHashMap::new()),
            caption: Caption {
                short: None,
                long: None,
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
        let blocks = vec![Block::Div(Div {
            attr: attr_id("tbl-one"),
            content: vec![table, para("Table caption")],
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        })];
        let ast = run_full(blocks).await;
        // bd-hcp8m3ve: table floats are figure-wrapped now; the table lives
        // in the aria wrapper and the caption is a real Figure caption with
        // the "Table 1: " prefix (see table_target_renders_q1_float_shape
        // for the full shape assertions).
        let (outer, fig) = float_shape(&ast.blocks[0]);
        assert_eq!(outer.attr.0, "tbl-one");
        let Block::Div(content_div) = &fig.content[0] else {
            panic!()
        };
        assert!(matches!(content_div.content[0], Block::Table(_)));
        let long = fig.caption.long.as_ref().unwrap();
        let Block::Plain(p) = &long[0] else { panic!() };
        let Inline::Str(s) = &p.content[0] else {
            panic!()
        };
        assert_eq!(s.text, "Table\u{a0}1: ");
    }

    // ─── Q1-verbatim float DOM shape (bd-hcp8m3ve) ──────────────────────────
    //
    // HTML-based formats render floats as:
    //   Div(id, [quarto-float quarto-figure quarto-figure-<align>])
    //     └ Figure("", [quarto-float quarto-float-<ref>], data-qf-* kvs)
    //         └ Div("", [], aria-describedby=<caption-id>) [content]
    //         + caption (figcaption synthesized by the writers from the kvs)
    // Contract: claude-notes/designs/float-layout-class-taxonomy.md

    /// Dig `(outer Div, inner Figure)` out of a rendered float block.
    fn float_shape(block: &Block) -> (&Div, &Figure) {
        let Block::Div(outer) = block else {
            panic!("expected outer float Div, got {:?}", block);
        };
        let Block::Figure(fig) = &outer.content[0] else {
            panic!("expected inner Figure, got {:?}", outer.content[0]);
        };
        (outer, fig)
    }

    fn assert_classes(attr: &Attr, expected: &[&str], what: &str) {
        for c in expected {
            assert!(
                attr.1.contains(&c.to_string()),
                "{what} missing class {c}: {:?}",
                attr.1
            );
        }
    }

    #[tokio::test]
    async fn figure_target_renders_q1_float_shape() {
        let ast = run_full(vec![fig_div("fig-1", "Caption A")]).await;
        let (outer, fig) = float_shape(&ast.blocks[0]);
        assert_eq!(outer.attr.0, "fig-1");
        assert_classes(
            &outer.attr,
            &["quarto-float", "quarto-figure", "quarto-figure-center"],
            "outer div",
        );
        assert_eq!(fig.attr.0, "");
        assert_classes(&fig.attr, &["quarto-float", "quarto-float-fig"], "figure");
        assert_eq!(
            fig.attr.2.get("data-qf-ref-type").map(String::as_str),
            Some("fig")
        );
        assert_eq!(
            fig.attr
                .2
                .get("data-qf-caption-location")
                .map(String::as_str),
            Some("bottom")
        );
        assert_eq!(
            fig.attr.2.get("data-qf-caption-id").map(String::as_str),
            Some("fig-1-caption")
        );
        // Content is wrapped in an aria-describedby div pointing at the caption id.
        let Block::Div(content_div) = &fig.content[0] else {
            panic!("expected content wrapper Div, got {:?}", fig.content[0]);
        };
        assert_eq!(
            content_div
                .attr
                .2
                .get("aria-describedby")
                .map(String::as_str),
            Some("fig-1-caption")
        );
        // Caption still carries the "Figure 1: " prefix.
        let long = fig.caption.long.as_ref().unwrap();
        let Block::Plain(p) = &long[0] else { panic!() };
        let Inline::Str(s) = &p.content[0] else {
            panic!()
        };
        assert_eq!(s.text, "Figure\u{a0}1: ");
    }

    #[tokio::test]
    async fn fig_align_attribute_drives_alignment_class() {
        // Q1 reads `fig-align` from the contained Image and strips it.
        let mut img_attr: LinkedHashMap<String, String> = LinkedHashMap::new();
        img_attr.insert("fig-align".to_string(), "left".to_string());
        let img = Inline::Image(quarto_pandoc_types::inline::Image {
            attr: (String::new(), Vec::new(), img_attr),
            content: vec![],
            target: ("img.png".to_string(), String::new()),
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
            target_source: TargetSourceInfo::empty(),
        });
        let blocks = vec![Block::Div(Div {
            attr: attr_id("fig-a"),
            content: vec![
                Block::Paragraph(Paragraph {
                    content: vec![img],
                    source_info: si(),
                }),
                para("Cap"),
            ],
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        })];
        let ast = run_full(blocks).await;
        let (outer, fig) = float_shape(&ast.blocks[0]);
        assert_classes(&outer.attr, &["quarto-figure-left"], "outer div");
        // The fig-align attribute is consumed, not emitted on the image.
        fn find_image(blocks: &Blocks) -> Option<&quarto_pandoc_types::inline::Image> {
            for b in blocks {
                match b {
                    Block::Div(d) => {
                        if let Some(i) = find_image(&d.content) {
                            return Some(i);
                        }
                    }
                    Block::Paragraph(Paragraph { content, .. })
                    | Block::Plain(quarto_pandoc_types::block::Plain { content, .. }) => {
                        for inl in content {
                            if let Inline::Image(i) = inl {
                                return Some(i);
                            }
                        }
                    }
                    _ => {}
                }
            }
            None
        }
        let img = find_image(&fig.content).expect("image survives in content");
        assert!(
            !img.attr.2.contains_key("fig-align"),
            "fig-align must be stripped from the image: {:?}",
            img.attr.2
        );
    }

    #[tokio::test]
    async fn table_target_renders_q1_float_shape() {
        use quarto_pandoc_types::table::{Table, TableBody, TableFoot, TableHead};
        let table = Block::Table(Table {
            attr: (String::new(), Vec::new(), LinkedHashMap::new()),
            caption: Caption {
                short: None,
                long: None,
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
        let blocks = vec![Block::Div(Div {
            attr: attr_id("tbl-one"),
            content: vec![table, para("Table caption")],
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        })];
        let ast = run_full(blocks).await;
        let (outer, fig) = float_shape(&ast.blocks[0]);
        assert_eq!(outer.attr.0, "tbl-one");
        assert_classes(&outer.attr, &["quarto-float", "quarto-figure"], "outer div");
        assert_classes(&fig.attr, &["quarto-float", "quarto-float-tbl"], "figure");
        assert_eq!(
            fig.attr.2.get("data-qf-ref-type").map(String::as_str),
            Some("tbl")
        );
        // The table itself lives inside the aria wrapper.
        let Block::Div(content_div) = &fig.content[0] else {
            panic!()
        };
        assert!(matches!(content_div.content[0], Block::Table(_)));
        // Caption prefixed "Table 1: ".
        let long = fig.caption.long.as_ref().unwrap();
        let Block::Plain(p) = &long[0] else { panic!() };
        let Inline::Str(s) = &p.content[0] else {
            panic!()
        };
        assert_eq!(s.text, "Table\u{a0}1: ");
    }

    /// bd-4m2n6qf1: when a table float's caption comes from the Table's own
    /// `caption.long`, that caption must be cleared once it has been hoisted
    /// into the synthesized `<figcaption>` — otherwise the writers emit the
    /// text twice, as `<table><caption>` *and* as `<figcaption>`.
    ///
    /// Note `table_target_renders_q1_float_shape` above supplies the caption
    /// as a sibling paragraph, so it never exercised this path — which is why
    /// the workspace suite missed the duplication.
    ///
    /// Q1 performs the same elision at float-parse time
    /// (`quarto-pre/parsefiguredivs.lua`: `table.caption = pandoc.Caption{}`
    /// at L280, `el.caption.long = pandoc.Blocks({})` at L544). Q2 builds the
    /// float DOM in the Finalization-phase transform, so it elides here, at
    /// figcaption-synthesis time.
    #[tokio::test]
    async fn table_float_clears_the_tables_own_caption() {
        use quarto_pandoc_types::table::{Table, TableBody, TableFoot, TableHead};
        let table = Block::Table(Table {
            attr: (String::new(), Vec::new(), LinkedHashMap::new()),
            caption: Caption {
                short: None,
                long: Some(vec![para("Cap")]),
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
        let blocks = vec![Block::Div(Div {
            attr: attr_id("tbl-one"),
            content: vec![table],
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        })];
        let ast = run_full(blocks).await;
        let (_outer, fig) = float_shape(&ast.blocks[0]);

        // The figcaption still carries the caption text.
        let long = fig.caption.long.as_ref().expect("figcaption caption");
        assert!(
            format!("{long:?}").contains("Cap"),
            "figcaption should carry the caption text: {long:?}"
        );

        // ...and the Table inside the aria wrapper must no longer carry it.
        let Block::Div(content_div) = &fig.content[0] else {
            panic!("expected the aria content wrapper")
        };
        let Block::Table(t) = &content_div.content[0] else {
            panic!("expected the table inside the wrapper")
        };
        assert!(
            t.caption.long.as_ref().is_none_or(|b| b.is_empty()),
            "the table's own caption must be cleared once hoisted into the \
             figcaption, else it renders twice: {:?}",
            t.caption.long
        );
    }

    #[tokio::test]
    async fn standalone_captioned_figure_gets_quarto_figure_wrapper() {
        // Shape 2 (design doc): a non-crossref `![caption](img)` figure —
        // a native Figure with no id — is wrapped in
        // `Div(.quarto-figure .quarto-figure-<align>)`, with the figure's id
        // (when present) moving to the wrapper, Q1's `renderHtmlFigure`.
        let img = Inline::Image(quarto_pandoc_types::inline::Image {
            attr: (String::new(), Vec::new(), LinkedHashMap::new()),
            content: vec![],
            target: ("img.png".to_string(), String::new()),
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
            target_source: TargetSourceInfo::empty(),
        });
        let figure = Block::Figure(Figure {
            attr: (String::new(), Vec::new(), LinkedHashMap::new()),
            caption: Caption {
                short: None,
                long: Some(vec![para("A caption")]),
                source_info: si(),
            },
            content: vec![Block::Plain(quarto_pandoc_types::block::Plain {
                content: vec![img],
                source_info: si(),
            })],
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        });
        let ast = run_full(vec![figure]).await;
        let Block::Div(outer) = &ast.blocks[0] else {
            panic!("expected wrapper Div, got {:?}", ast.blocks[0]);
        };
        assert_classes(
            &outer.attr,
            &["quarto-figure", "quarto-figure-center"],
            "standalone wrapper",
        );
        assert!(
            matches!(outer.content.first(), Some(Block::Figure(_))),
            "figure inside wrapper"
        );
        // Standalone figures carry no float classes or data-qf kvs.
        let Some(Block::Figure(f)) = outer.content.first() else {
            unreachable!()
        };
        assert!(
            !f.attr.1.iter().any(|c| c == "quarto-float"),
            "standalone figure is not a float: {:?}",
            f.attr.1
        );
        assert!(
            !f.attr.2.keys().any(|k| k.starts_with("data-qf-")),
            "no float kvs on a standalone figure"
        );
    }

    #[tokio::test]
    async fn section_ref_target_is_not_float_wrapped() {
        // Section divs never become FloatRefTarget nodes at all:
        // `classify_div` excludes any div carrying the `section` class from
        // float sugaring (book-projects P0, Amendment A), so a section
        // passes through the whole pipeline as a plain section Div — never
        // grows a figure wrapper or a figcaption. (The assertions predate
        // the exclusion: they once held via the render-side non-float-kind
        // pass-through, after the kitchen-sink fixture caught sections being
        // swallowed into `quarto-float-sec` figures. They now hold for the
        // stronger reason that the misclassification never happens.)
        let blocks = vec![Block::Div(Div {
            attr: (
                "sec-x".to_string(),
                vec!["section".to_string()],
                LinkedHashMap::new(),
            ),
            content: vec![para("Section body text")],
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        })];
        let ast = run_full(blocks).await;
        let Block::Div(d) = &ast.blocks[0] else {
            panic!("expected section Div, got {:?}", ast.blocks[0]);
        };
        assert_eq!(d.attr.0, "sec-x");
        assert!(
            d.attr.1.contains(&"section".to_string()),
            "section class preserved: {:?}",
            d.attr.1
        );
        assert!(
            !d.attr.1.iter().any(|c| c.starts_with("quarto-float")),
            "sections must not be float-wrapped: {:?}",
            d.attr.1
        );
        assert!(
            !matches!(d.content.first(), Some(Block::Figure(_))),
            "no figure wrapper inside a section"
        );
    }

    #[tokio::test]
    async fn caption_id_collides_with_user_id_and_disambiguates() {
        // A user-authored element already owns "fig-1-caption": the generated
        // figcaption id must disambiguate (this replaces Q1's uuid suffix).
        let user_div = Block::Div(Div {
            attr: attr_id("fig-1-caption"),
            content: vec![para("mine")],
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        });
        let ast = run_full(vec![fig_div("fig-1", "Cap"), user_div]).await;
        let (_outer, fig) = float_shape(&ast.blocks[0]);
        assert_eq!(
            fig.attr.2.get("data-qf-caption-id").map(String::as_str),
            Some("fig-1-caption-1")
        );
        let Block::Div(content_div) = &fig.content[0] else {
            panic!()
        };
        assert_eq!(
            content_div
                .attr
                .2
                .get("aria-describedby")
                .map(String::as_str),
            Some("fig-1-caption-1")
        );
    }

    #[tokio::test]
    async fn uncaptioned_float_gets_uncaptioned_kv_and_label_caption() {
        // Q1: an uncaptioned float's figcaption holds just the label and the
        // figcaption gains `quarto-uncaptioned` (via the kv here).
        let blocks = vec![Block::Div(Div {
            attr: attr_id("fig-bare"),
            content: vec![Block::CodeBlock(CodeBlock {
                attr: (String::new(), vec!["python".into()], LinkedHashMap::new()),
                text: "x=1".into(),
                source_info: si(),
                attr_source: AttrSourceInfo::empty(),
            })],
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        })];
        let ast = run_full(blocks).await;
        let (_outer, fig) = float_shape(&ast.blocks[0]);
        assert_eq!(
            fig.attr.2.get("data-qf-uncaptioned").map(String::as_str),
            Some("1")
        );
        let long = fig.caption.long.as_ref().unwrap();
        let Block::Plain(p) = &long[0] else { panic!() };
        let Inline::Str(s) = &p.content[0] else {
            panic!()
        };
        assert_eq!(s.text, "Figure\u{a0}1");
    }

    #[tokio::test]
    async fn resolved_ref_renders_to_link() {
        let blocks = vec![
            fig_div("fig-a", "Cap"),
            Block::Paragraph(Paragraph {
                content: vec![str_inline("see "), cite("fig-a")],
                source_info: si(),
            }),
        ];
        let ast = run_full(blocks).await;
        // First block is the rendered figure, second is the paragraph.
        let Block::Paragraph(p) = &ast.blocks[1] else {
            panic!();
        };
        let Inline::Link(link) = &p.content[1] else {
            panic!("expected Link, got {:?}", p.content[1]);
        };
        assert_eq!(link.target.0, "#fig-a");
        let Inline::Str(s) = &link.content[0] else {
            panic!();
        };
        assert_eq!(s.text, "Figure\u{a0}1");
        assert!(link.attr.1.contains(&"quarto-xref".to_string()));
        // Resolved refs must NOT carry the unresolved marker (bd-28iqotrt).
        assert!(
            !link.attr.1.contains(&"quarto-unresolved-ref".to_string()),
            "resolved ref should not carry quarto-unresolved-ref"
        );
    }

    #[tokio::test]
    async fn unresolved_ref_renders_with_question_marks() {
        let blocks = vec![Block::Paragraph(Paragraph {
            content: vec![cite("fig-nope")],
            source_info: si(),
        })];
        let ast = run_full(blocks).await;
        let Block::Paragraph(p) = &ast.blocks[0] else {
            panic!();
        };
        let Inline::Link(link) = &p.content[0] else {
            panic!();
        };
        let Inline::Str(s) = &link.content[0] else {
            panic!();
        };
        assert_eq!(s.text, "?fig-nope?");
        // Unresolved refs additionally carry `quarto-unresolved-ref` (alongside
        // the base `quarto-xref`) so downstream extensions can loudly style a
        // missing reference — matching TS Quarto (crossref/refs.lua:94). Additive
        // per Carlos, 2026-07-21 (bd-28iqotrt, audit row 17): Q2 keeps its louder
        // `?id?` Link rather than TS's Span.
        assert!(
            link.attr.1.contains(&"quarto-xref".to_string()),
            "unresolved ref should keep the base quarto-xref class"
        );
        assert!(
            link.attr.1.contains(&"quarto-unresolved-ref".to_string()),
            "unresolved ref should carry quarto-unresolved-ref"
        );
    }

    // ── P5: cross-chapter patched refs ─────────────────────────────
    //
    // `CrossChapterCrossrefResolveTransform` patches still-unresolved
    // nodes with `resolved_number` / `target_href` (plus the owning
    // chapter's raw `order` / `in_appendix`). The display side must prefer
    // those: the generic non-sec path runs `format_crossref_number` with
    // the *rendering* chapter's seed, which would renumber another
    // chapter's figure; the link target would stay a dangling local `#id`.

    fn crossref_entry(
        id: &str,
        ref_type: &str,
        section: Vec<u32>,
        order: u32,
        in_appendix: bool,
    ) -> crate::crossref::index::CrossrefEntry {
        crate::crossref::index::CrossrefEntry {
            identifier: id.to_string(),
            ref_type: ref_type.to_string(),
            parent: None,
            order: crate::crossref::index::Order { section, order },
            caption: None,
            in_appendix,
            source_info: si(),
        }
    }

    /// An unresolved `CrossrefResolvedRef` node exactly as
    /// `CrossrefResolveTransform` leaves it for a foreign id.
    fn unresolved_ref_node(identifier: &str, ref_type: &str, kind: &str) -> Inline {
        let mut node = CustomNode::new(CROSSREF_RESOLVED_REF, Attr::default(), si());
        node.plain_data = serde_json::json!({
            "identifier": identifier,
            "ref_type": ref_type,
            "kind": kind,
            "resolved": false,
            "label_upper": false,
        });
        Inline::Custom(node)
    }

    /// The real P5 unit path for one cross-chapter ref: aggregate the
    /// given chapter inventories into a registry, run the resolve
    /// transform over an AST carrying one unresolved ref to `target_id`,
    /// then the render transform — under `rendering_seed` (the seed of the
    /// chapter being rendered, distinct from every owning chapter's).
    /// Returns the rendered Link.
    async fn render_cross_chapter(
        inventories: Vec<crate::crossref::project_index::ChapterCrossrefInventory>,
        target_id: &str,
        ref_type: &str,
        kind: &str,
        rendering_seed: crate::render::ChapterSeed,
        doc_max_heading: u32,
    ) -> Link {
        use crate::format::Format;
        use crate::project::{DocumentInfo, ProjectConfig, ProjectContext};
        use crate::render::BinaryDependencies;
        use crate::transforms::cross_chapter_crossref_resolve::CrossChapterCrossrefResolveTransform;
        use std::path::PathBuf;
        let (registry, _) =
            crate::crossref::project_index::aggregate_chapter_inventories(&inventories);
        let project = ProjectContext {
            dir: PathBuf::from("/p"),
            config: ProjectConfig::default(),
            is_single_file: true,
            files: vec![],
            output_dir: PathBuf::from("/p"),
            ..Default::default()
        };
        let doc = DocumentInfo::from_path("/p/ch3.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);
        ctx.ref_type_registry = Some(RefTypeRegistry::builtin());
        ctx.chapter_seed = Some(rendering_seed);
        let mut index = crate::crossref::CrossrefIndex::new(FileId(0));
        index.max_heading = doc_max_heading;
        ctx.crossref_index = Some(index);
        ctx.cross_chapter_crossref_registry = Some(std::sync::Arc::new(registry));

        let mut ast = Pandoc {
            meta: quarto_pandoc_types::ConfigValue::default(),
            blocks: vec![Block::Paragraph(Paragraph {
                content: vec![unresolved_ref_node(target_id, ref_type, kind)],
                source_info: si(),
            })],
        };
        CrossChapterCrossrefResolveTransform::new()
            .transform(&mut ast, &mut ctx)
            .await
            .unwrap();
        CrossrefRenderTransform::new()
            .transform(&mut ast, &mut ctx)
            .await
            .unwrap();
        let Block::Paragraph(p) = &ast.blocks[0] else {
            panic!();
        };
        let Inline::Link(link) = &p.content[0] else {
            panic!("expected Link, got {:?}", p.content[0]);
        };
        link.clone()
    }

    #[tokio::test]
    async fn cross_chapter_ref_renders_owning_number_and_target() {
        // ch2 owns fig-two; ch3 is being rendered (seed 3). The patched
        // node must show ch2's "2.1" — not the rendering chapter's "3.1"
        // — and link into ch2's file, not a dangling local anchor.
        let ch2 = crate::crossref::project_index::ChapterCrossrefInventory {
            index: {
                let mut index = crate::crossref::CrossrefIndex::new(FileId(0));
                index.max_heading = 1;
                index.insert(crossref_entry("fig-two", "fig", vec![2], 1, false));
                index
            },
            chapter_seed: Some(crate::render::ChapterSeed {
                chapter_number: 2,
                is_appendix: false,
            }),
            output_href: "ch2.html".to_string(),
        };
        let link = render_cross_chapter(
            vec![ch2],
            "fig-two",
            "fig",
            "Figure",
            crate::render::ChapterSeed {
                chapter_number: 3,
                is_appendix: false,
            },
            1,
        )
        .await;
        let Inline::Str(s) = &link.content[0] else {
            panic!();
        };
        assert_eq!(
            s.text, "Figure\u{a0}2.1",
            "the owning chapter's composed number must win over the rendering chapter's seed"
        );
        assert_eq!(
            link.target.0, "ch2.html#fig-two",
            "the link must target the owning chapter's file"
        );
        assert!(link.attr.1.contains(&"quarto-xref".to_string()));
        assert!(
            !link.attr.1.contains(&"quarto-unresolved-ref".to_string()),
            "a registry-resolved ref is resolved, not unresolved"
        );
    }

    #[tokio::test]
    async fn cross_chapter_sec_ref_renders_owning_appendix_shape() {
        // Appendix A owns sec-methods at §[1,2] (seed-offset path; the
        // appendix flag lives on the inventory). The display side re-derives
        // the number from the patched `order` + `in_appendix` — the same
        // local code path, no seed involved — and takes the patched
        // `target_href` for the link target.
        let app = crate::crossref::project_index::ChapterCrossrefInventory {
            index: {
                let mut index = crate::crossref::CrossrefIndex::new(FileId(0));
                index.max_heading = 1;
                index.insert(crossref_entry("sec-methods", "sec", vec![1, 2], 2, true));
                index
            },
            chapter_seed: Some(crate::render::ChapterSeed {
                chapter_number: 1,
                is_appendix: true,
            }),
            output_href: "app-a.html".to_string(),
        };
        let link = render_cross_chapter(
            vec![app],
            "sec-methods",
            "sec",
            "Section",
            crate::render::ChapterSeed {
                chapter_number: 3,
                is_appendix: false,
            },
            1,
        )
        .await;
        let Inline::Str(s) = &link.content[0] else {
            panic!();
        };
        assert_eq!(
            s.text, "Section\u{a0}A.2",
            "the owning chapter's appendix shape must come through the patched order/in_appendix"
        );
        assert_eq!(
            link.target.0, "app-a.html#sec-methods",
            "the link must target the owning chapter's file"
        );
    }

    // ── P0: `@sec-` reference presentation ─────────────────────────
    //
    // Q1 ports (crossref/refs.lua + format.lua): a `sec` ref renders its
    // *section path* (not a per-type counter) as the number; a chapter-level
    // heading under `crossref.chapters: true` takes the ch/apx prefix.

    #[tokio::test]
    async fn sec_ref_on_level_2_heading_resolves_to_section_number() {
        // H1-led doc, no `chapters` key: the top component is included
        // (maxHeading semantics — pinned empirically against Q1 2026-09-23).
        let ast = run_full(vec![
            header(1, "sec-intro", "Intro"),
            header(2, "sec-a", "A"),
            header(2, "sec-b", "B"),
            Block::Paragraph(Paragraph {
                content: vec![str_inline("see "), cite("sec-b")],
                source_info: si(),
            }),
        ])
        .await;
        let Block::Paragraph(p) = &ast.blocks[3] else {
            panic!();
        };
        assert_eq!(ref_link_text(&p.content, 1), "Section\u{a0}1.2");
    }

    #[tokio::test]
    async fn chapter_ref_resolves_when_chapters_enabled() {
        // `crossref.chapters: true` + level-1 heading target → "Chapter N",
        // no numeric section address (Q1's isChapterRef prefix swap).
        let ast = run_full_opts(
            vec![
                header(1, "sec-one", "One"),
                header(1, "sec-two", "Two"),
                Block::Paragraph(Paragraph {
                    content: vec![str_inline("see "), cite("sec-two")],
                    source_info: si(),
                }),
            ],
            meta_with_chapters(true),
            None,
        )
        .await;
        let Block::Paragraph(p) = &ast.blocks[2] else {
            panic!();
        };
        assert_eq!(ref_link_text(&p.content, 1), "Chapter\u{a0}2");
    }

    #[tokio::test]
    async fn appendix_chapter_ref_resolves_to_letter() {
        // An appendix chapter (per-file seed: appendix-local number 1 → "A")
        // resolves to "Appendix A", letter instead of numeral (Q1's
        // formatChapterIndex via file.bookItemNumber).
        let ast = run_full_opts(
            vec![
                header(1, "sec-app", "Appendix"),
                Block::Paragraph(Paragraph {
                    content: vec![str_inline("see "), cite("sec-app")],
                    source_info: si(),
                }),
            ],
            meta_with_chapters(true),
            Some(crate::render::ChapterSeed {
                chapter_number: 1,
                is_appendix: true,
            }),
        )
        .await;
        let Block::Paragraph(p) = &ast.blocks[1] else {
            panic!();
        };
        assert_eq!(ref_link_text(&p.content, 1), "Appendix\u{a0}A");
    }

    #[tokio::test]
    async fn h2_led_document_numbers_sections_relatively() {
        // Port of Q1's maxHeading (normalize/flags.lua): with no H1 and no
        // `chapters`, sections number relative to the shallowest heading —
        // first H2 is "1", its first H3 child "1.1".
        let ast = run_full(vec![
            header(2, "sec-a", "A"),
            header(3, "sec-b", "B"),
            Block::Paragraph(Paragraph {
                content: vec![
                    str_inline("a: "),
                    cite("sec-a"),
                    str_inline(" b: "),
                    cite("sec-b"),
                ],
                source_info: si(),
            }),
        ])
        .await;
        let Block::Paragraph(p) = &ast.blocks[2] else {
            panic!();
        };
        assert_eq!(ref_link_text(&p.content, 1), "Section\u{a0}1");
        assert_eq!(ref_link_text(&p.content, 3), "Section\u{a0}1.1");
    }

    // ── P0: visible number injection (sections.lua port) ───────────
    //
    // The index transform stashes a `number` kv on numbered headers (gated
    // on `number-sections`); this transform prepends
    // `Span(Str(number), class="header-section-number")` + Space wherever
    // the kv is present — so injection is automatically inert wherever the
    // stash didn't run.

    fn number_sections_meta() -> quarto_pandoc_types::ConfigValue {
        use quarto_pandoc_types::{ConfigMapEntry, ConfigValue};
        ConfigValue::new_map(
            vec![ConfigMapEntry {
                key: "number-sections".to_string(),
                key_source: si(),
                value: ConfigValue::new_bool(true, si()),
            }],
            si(),
        )
    }

    /// The header-section-number span the injection prepends.
    fn assert_number_span(inline: &Inline, number: &str) {
        let Inline::Span(span) = inline else {
            panic!("expected Span, got {:?}", inline);
        };
        assert!(
            span.attr.1.contains(&"header-section-number".to_string()),
            "span class: {:?}",
            span.attr.1
        );
        let Inline::Str(s) = &span.content[0] else {
            panic!()
        };
        assert_eq!(s.text, number);
    }

    #[tokio::test]
    async fn header_section_number_injected_when_stashed() {
        let ast = run_full_opts(
            vec![header(1, "sec-a", "Alpha"), header(2, "sec-b", "Beta")],
            number_sections_meta(),
            None,
        )
        .await;
        let Block::Header(h1) = &ast.blocks[0] else {
            panic!()
        };
        assert_number_span(&h1.content[0], "1");
        assert!(matches!(h1.content[1], Inline::Space(_)));
        let Inline::Str(s) = &h1.content[2] else {
            panic!()
        };
        assert_eq!(s.text, "Alpha");
        let Block::Header(h2) = &ast.blocks[1] else {
            panic!()
        };
        assert_number_span(&h2.content[0], "1.1");
    }

    #[tokio::test]
    async fn appendix_level1_header_gets_appendix_title_shape() {
        // sections.lua: a level-1 appendix heading's content ends as
        // [Str("Appendix"), Space, Span(number), Str(" —"), Space, ...].
        let ast = run_full_opts(
            vec![header(1, "sec-app", "Extra bits")],
            number_sections_meta(),
            Some(crate::render::ChapterSeed {
                chapter_number: 1,
                is_appendix: true,
            }),
        )
        .await;
        let Block::Header(h) = &ast.blocks[0] else {
            panic!()
        };
        let Inline::Str(title) = &h.content[0] else {
            panic!("appendix-title first: {:?}", h.content)
        };
        assert_eq!(title.text, "Appendix");
        assert!(matches!(h.content[1], Inline::Space(_)));
        assert_number_span(&h.content[2], "A");
        let Inline::Str(delim) = &h.content[3] else {
            panic!("appendix-delim: {:?}", h.content)
        };
        assert_eq!(delim.text, " —");
        assert!(matches!(h.content[4], Inline::Space(_)));
        let Inline::Str(rest) = &h.content[5] else {
            panic!()
        };
        assert_eq!(rest.text, "Extra bits");
    }

    #[tokio::test]
    async fn no_injection_without_number_sections() {
        // Regression: no number kv stashed → no injection; the header
        // content is byte-identical to the pre-feature pipeline.
        let ast = run_full(vec![header(1, "sec-a", "Alpha")]).await;
        let Block::Header(h) = &ast.blocks[0] else {
            panic!()
        };
        assert!(h.attr.2.get("number").is_none());
        assert_eq!(h.content.len(), 1);
        let Inline::Str(s) = &h.content[0] else {
            panic!()
        };
        assert_eq!(s.text, "Alpha");
    }

    // ── P4: chapter title decoration (`withChapterMetadata` port) ───
    //
    // Q1 decorates a book chapter's page title by prepending its chapter
    // number ("Appendix A — Title" for appendix chapters; plain for
    // `.unnumbered`). q2 re-expresses that as the header-number injection
    // above, seeded per chapter. These tests pin the two decoration cases
    // the P0 tests don't: the seeded *numbered* chapter, and the
    // deliberate `.unlisted` non-suppression.

    #[tokio::test]
    async fn seeded_numbered_chapter_h1_decorates_with_chapter_number() {
        // A book chapter's H1 carries the chapter-scoped number (seed 2 →
        // span "2"), with no appendix prefix — Q1's numbered-chapter
        // `formatChapterTitle` arm.
        let ast = run_full_opts(
            vec![header(1, "sec-a", "Alpha")],
            number_sections_meta(),
            Some(crate::render::ChapterSeed {
                chapter_number: 2,
                is_appendix: false,
            }),
        )
        .await;
        let Block::Header(h) = &ast.blocks[0] else {
            panic!()
        };
        assert_number_span(&h.content[0], "2");
        assert!(matches!(h.content[1], Inline::Space(_)));
        let Inline::Str(s) = &h.content[2] else {
            panic!()
        };
        assert_eq!(s.text, "Alpha");
        let joined: String = h
            .content
            .iter()
            .map(|i| match i {
                Inline::Str(s) => s.text.clone(),
                Inline::Space(_) => " ".to_string(),
                _ => String::new(),
            })
            .collect();
        assert!(
            !joined.contains("Appendix") && !joined.contains("—"),
            "a numbered (non-appendix) chapter must not get the appendix prefix: {joined:?}"
        );
    }

    #[tokio::test]
    async fn unlisted_class_does_not_suppress_decoration() {
        // Q1's contract: `.unlisted` gates only the toc default
        // (`isListedChapter`); `chapterInfoForInput` never consults it, so
        // a listed-but-unlisted chapter still gets its number decoration.
        // q2 keeps that split: the number stash skips only `.unnumbered`
        // (crossref_index), and the injection never consults classes —
        // listing is P1's sidebar translation's concern.
        let mut h = header(1, "sec-u", "Unlisted but numbered");
        if let Block::Header(inner) = &mut h {
            inner.attr.1.push("unlisted".to_string());
        }
        let ast = run_full_opts(
            vec![h],
            number_sections_meta(),
            Some(crate::render::ChapterSeed {
                chapter_number: 3,
                is_appendix: false,
            }),
        )
        .await;
        let Block::Header(h) = &ast.blocks[0] else {
            panic!()
        };
        assert_number_span(&h.content[0], "3");
    }

    // ── P4: chapter-scoped display numbers (book-projects) ──────────
    //
    // The chapter seed (book-projects P0) already seeds the section
    // counter; these tests pin the *display* half — float/equation/ref
    // numbers compose the seed's chapter number with the flat counter
    // ("Figure 2.1", "Figure C.1") instead of showing the bare counter.

    #[tokio::test]
    async fn chapter_two_seed_shows_h1_two_and_figure_two_one() {
        // A chapter-2 seed seeds the counter at [1] before the walk, so
        // the chapter's own H1 numbers "2"; a figure under it displays
        // the chapter-scoped "Figure 2.1", not the flat "Figure 1"
        // (observed against a real Q1 render: scratchpad/minibook).
        let ast = run_full_opts(
            vec![
                header(1, "sec-ch2", "Two"),
                fig_div("fig-ch2", "A figure in chapter two"),
            ],
            number_sections_meta(),
            Some(crate::render::ChapterSeed {
                chapter_number: 2,
                is_appendix: false,
            }),
        )
        .await;
        // H1 becomes section "2" (the seeding half — landed in P0).
        let Block::Header(h1) = &ast.blocks[0] else {
            panic!()
        };
        assert_number_span(&h1.content[0], "2");
        // The figure under it displays chapter-scoped "Figure 2.1: "
        // (the display half — the fix this test drives).
        let (outer, f) = float_shape(&ast.blocks[1]);
        assert_eq!(outer.attr.0, "fig-ch2");
        let long = f.caption.long.as_ref().unwrap();
        let Block::Plain(p) = &long[0] else { panic!() };
        let Inline::Str(s) = &p.content[0] else {
            panic!()
        };
        assert_eq!(s.text, "Figure\u{a0}2.1: ");
    }

    #[tokio::test]
    async fn appendix_seed_numbers_floats_letter_scoped() {
        // An appendix chapter's seed carries its *appendix-local* number
        // (P1's BookRenderItem numbering — the third appendix is 3), and
        // display numbers use the letter "C.1", independent of how many
        // main chapters precede it (Q1: a separate letter sequence, not
        // a continuation of the numeric chapters).
        let ast = run_full_opts(
            vec![
                fig_div("fig-app", "An appendix figure"),
                Block::Paragraph(Paragraph {
                    content: vec![str_inline("see "), cite("fig-app")],
                    source_info: si(),
                }),
            ],
            quarto_pandoc_types::ConfigValue::default(),
            Some(crate::render::ChapterSeed {
                chapter_number: 3,
                is_appendix: true,
            }),
        )
        .await;
        let (outer, f) = float_shape(&ast.blocks[0]);
        assert_eq!(outer.attr.0, "fig-app");
        let long = f.caption.long.as_ref().unwrap();
        let Block::Plain(p) = &long[0] else { panic!() };
        let Inline::Str(s) = &p.content[0] else {
            panic!()
        };
        assert_eq!(s.text, "Figure\u{a0}C.1: ");
        // The in-text ref shows the same letter-scoped number.
        let Block::Paragraph(rp) = &ast.blocks[1] else {
            panic!()
        };
        assert_eq!(ref_link_text(&rp.content, 1), "Figure\u{a0}C.1");
    }

    #[tokio::test]
    async fn float_ref_target_with_no_caption_renders_figure_with_empty_caption() {
        let blocks = vec![Block::Div(Div {
            attr: attr_id("fig-bare"),
            content: vec![Block::CodeBlock(CodeBlock {
                attr: (String::new(), vec!["python".into()], LinkedHashMap::new()),
                text: "x=1".into(),
                source_info: si(),
                attr_source: AttrSourceInfo::empty(),
            })],
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        })];
        let ast = run_full(blocks).await;
        // HTML float shape (bd-hcp8m3ve): outer div carries the id; the
        // uncaptioned float still gets a label-only caption + the
        // data-qf-uncaptioned marker (see
        // uncaptioned_float_gets_uncaptioned_kv_and_label_caption).
        let (outer, _fig) = float_shape(&ast.blocks[0]);
        assert_eq!(outer.attr.0, "fig-bare");
    }

    #[test]
    fn prefix_caption_prepends_kind_and_number() {
        let cap = vec![Block::Paragraph(Paragraph {
            content: vec![str_inline("Hello")],
            source_info: si(),
        })];
        let out = prefix_caption(cap, "Figure", Some("3".to_string()));
        let Block::Paragraph(p) = &out[0] else {
            panic!();
        };
        let Inline::Str(s) = &p.content[0] else {
            panic!();
        };
        assert_eq!(s.text, "Figure\u{a0}3: ");
    }

    #[test]
    fn prefix_caption_no_number_still_adds_prefix() {
        let cap = vec![Block::Paragraph(Paragraph {
            content: vec![str_inline("Hello")],
            source_info: si(),
        })];
        let out = prefix_caption(cap, "Figure", None);
        let Block::Paragraph(p) = &out[0] else {
            panic!();
        };
        let Inline::Str(s) = &p.content[0] else {
            panic!();
        };
        assert_eq!(s.text, "Figure: ");
    }

    #[test]
    fn prefix_caption_prepends_into_plain_first_block() {
        // bd-n3sark9b: Pandoc-native `Figure` and `Table` captions are
        // `Plain`, not `Paragraph`. The prefix must land in either, and the
        // block type must be preserved (a Plain caption stays Plain so the
        // HTML writer emits bare inlines inside <figcaption>).
        let cap = vec![Block::Plain(quarto_pandoc_types::block::Plain {
            content: vec![str_inline("Hello")],
            source_info: si(),
        })];
        let out = prefix_caption(cap, "Figure", Some("3".to_string()));
        assert_eq!(out.len(), 1);
        let Block::Plain(p) = &out[0] else {
            panic!("expected Plain to be preserved, got {:?}", out[0]);
        };
        let Inline::Str(s) = &p.content[0] else {
            panic!();
        };
        assert_eq!(s.text, "Figure\u{a0}3: ");
        let Inline::Str(s) = &p.content[1] else {
            panic!();
        };
        assert_eq!(s.text, "Hello");
    }

    #[test]
    fn prefix_caption_inserts_leading_plain_when_first_block_is_container() {
        // bd-n3sark9b: a caption whose first block carries no inlines
        // (e.g. a CodeBlock) must not silently lose its prefix. Insert a
        // label-only Plain in front, mirroring `prepend_theorem_label`.
        let cap = vec![Block::CodeBlock(CodeBlock {
            attr: (String::new(), Vec::new(), LinkedHashMap::new()),
            text: "x".into(),
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        })];
        let out = prefix_caption(cap, "Figure", Some("3".to_string()));
        assert_eq!(out.len(), 2);
        let Block::Plain(p) = &out[0] else {
            panic!("expected inserted Plain, got {:?}", out[0]);
        };
        assert_eq!(p.content.len(), 1);
        let Inline::Str(s) = &p.content[0] else {
            panic!();
        };
        assert_eq!(s.text, "Figure\u{a0}3: ");
        assert!(matches!(out[1], Block::CodeBlock(_)));
    }

    /// A native `Figure` with a crossref id and a `[Plain[...]]` caption —
    /// what `![cap](img){#fig-x}` parses to.
    fn native_figure_with_plain_caption(id: &str, cap: &str) -> Block {
        let img = Inline::Image(quarto_pandoc_types::inline::Image {
            attr: (String::new(), Vec::new(), LinkedHashMap::new()),
            content: vec![],
            target: ("img.png".to_string(), String::new()),
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
            target_source: TargetSourceInfo::empty(),
        });
        Block::Figure(Figure {
            attr: attr_id(id),
            caption: Caption {
                short: None,
                long: Some(vec![Block::Plain(quarto_pandoc_types::block::Plain {
                    content: vec![str_inline(cap)],
                    source_info: si(),
                })]),
                source_info: si(),
            },
            content: vec![Block::Plain(quarto_pandoc_types::block::Plain {
                content: vec![img],
                source_info: si(),
            })],
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        })
    }

    /// First caption block's inlines, whichever of Plain / Paragraph it is.
    fn first_caption_inlines(long: &Blocks) -> &Inlines {
        match &long[0] {
            Block::Paragraph(p) => &p.content,
            Block::Plain(p) => &p.content,
            other => panic!("caption first block is not inline-bearing: {other:?}"),
        }
    }

    #[tokio::test]
    async fn attr_form_figure_caption_gets_prefix() {
        // bd-n3sark9b: `![Caption B](img){#fig-b}` — the caption arrives as
        // `Plain` and used to lose its "Figure N: " prefix while the
        // reference to it still resolved to "Figure N".
        let ast = run_full(vec![
            fig_div("fig-a", "Caption A"),
            native_figure_with_plain_caption("fig-b", "Caption B"),
        ])
        .await;
        let (outer, f) = float_shape(&ast.blocks[1]);
        assert_eq!(outer.attr.0, "fig-b");
        let long = f.caption.long.as_ref().unwrap();
        let inlines = first_caption_inlines(long);
        let Inline::Str(s) = &inlines[0] else {
            panic!("expected prefix Str, got {:?}", inlines[0]);
        };
        assert_eq!(s.text, "Figure\u{a0}2: ");
        let Inline::Str(s) = &inlines[1] else {
            panic!();
        };
        assert_eq!(s.text, "Caption B");
    }

    #[tokio::test]
    async fn table_with_plain_caption_gets_prefix() {
        // bd-n3sark9b: `Div(#tbl-x) > Table` where the Table's own caption
        // is `[Plain[...]]` (Pandoc's convention, and what the
        // `: cap {#tbl-x}` form desugars to).
        let table = Block::Table(quarto_pandoc_types::table::Table {
            attr: (String::new(), Vec::new(), LinkedHashMap::new()),
            caption: Caption {
                short: None,
                long: Some(vec![Block::Plain(quarto_pandoc_types::block::Plain {
                    content: vec![str_inline("Numbers")],
                    source_info: si(),
                })]),
                source_info: si(),
            },
            colspec: vec![],
            head: quarto_pandoc_types::table::TableHead {
                attr: (String::new(), Vec::new(), LinkedHashMap::new()),
                rows: vec![],
                source_info: si(),
                attr_source: AttrSourceInfo::empty(),
            },
            bodies: vec![],
            foot: quarto_pandoc_types::table::TableFoot {
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
        let ast = run_full(vec![div]).await;
        let (outer, f) = float_shape(&ast.blocks[0]);
        assert_eq!(outer.attr.0, "tbl-nums");
        let long = f.caption.long.as_ref().unwrap();
        let inlines = first_caption_inlines(long);
        let Inline::Str(s) = &inlines[0] else {
            panic!("expected prefix Str, got {:?}", inlines[0]);
        };
        assert_eq!(s.text, "Table\u{a0}1: ");
        let Inline::Str(s) = &inlines[1] else {
            panic!();
        };
        assert_eq!(s.text, "Numbers");
    }

    #[test]
    fn render_preserves_non_crossref_custom_nodes() {
        // A plain Callout-like custom node survives the render pass
        // untouched (it's not one of our two types).
        let mut callout = CustomNode::new(
            "Callout",
            (String::new(), Vec::new(), LinkedHashMap::new()),
            si(),
        );
        callout
            .slots
            .insert("content".into(), Slot::Blocks(vec![para("inside")]));
        let mut block = Block::Custom(callout);
        let mut fs = float_state_for_tests();
        render_block(&mut block, None, &mut fs);
        match block {
            Block::Custom(n) => assert_eq!(n.type_name, "Callout"),
            _ => panic!("callout was mutated"),
        }
    }

    /// Helper to build a Div(.theorem) input block.
    fn theorem_div(id: &str, title: Option<&str>, body: &str) -> Block {
        let mut kvs = LinkedHashMap::new();
        if let Some(t) = title {
            kvs.insert("name".into(), t.to_string());
        }
        Block::Div(Div {
            attr: (id.into(), vec!["theorem".into()], kvs),
            content: vec![para(body)],
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        })
    }

    /// Pull the `Strong` out of the theorem-title `Span` at
    /// `div.content[0].content[0]`. Panics on shape mismatch. Centralizes
    /// the new label structure `Span(theorem-title) > Strong > …` so test
    /// assertions don't duplicate the unwrap dance.
    fn theorem_label_strong(block: &Block) -> &quarto_pandoc_types::inline::Strong {
        let Block::Div(d) = block else {
            panic!("expected Div, got {:?}", block);
        };
        let Block::Paragraph(p) = &d.content[0] else {
            panic!("expected first Paragraph, got {:?}", d.content[0]);
        };
        let Inline::Span(span) = &p.content[0] else {
            panic!("expected theorem-title Span, got {:?}", p.content[0]);
        };
        assert_eq!(
            span.attr.1,
            vec!["theorem-title".to_string()],
            "expected theorem-title class on label span, got {:?}",
            span.attr.1
        );
        let Inline::Strong(s) = &span.content[0] else {
            panic!(
                "expected Strong inside theorem-title span, got {:?}",
                span.content[0]
            );
        };
        s
    }

    #[tokio::test]
    async fn theorem_renders_to_div_with_numbered_label() {
        let ast = run_full(vec![theorem_div("thm-pyth", None, "body text")]).await;
        let Block::Div(div) = &ast.blocks[0] else {
            panic!("expected Div, got {:?}", ast.blocks[0]);
        };
        assert_eq!(div.attr.0, "thm-pyth");
        // Q1 parity: `thm` ref_type produces just `["theorem"]`.
        assert_eq!(div.attr.1, vec!["theorem"]);

        let strong = theorem_label_strong(&ast.blocks[0]);
        let Inline::Str(label) = &strong.content[0] else {
            panic!(
                "first strong inline should be Str, got {:?}",
                strong.content[0]
            );
        };
        // Kind + nbsp + number, no trailing period.
        assert_eq!(label.text, "Theorem\u{a0}1");
        assert_eq!(
            strong.content.len(),
            1,
            "unexpected tail: {:?}",
            strong.content
        );

        // After the label span: a plain space, then the body text.
        let Block::Paragraph(p) = &div.content[0] else {
            panic!();
        };
        let Inline::Str(sp) = &p.content[1] else {
            panic!("expected space after label span, got {:?}", p.content[1]);
        };
        assert_eq!(sp.text, " ");
        let Inline::Str(body) = &p.content[2] else {
            panic!()
        };
        assert_eq!(body.text, "body text");
    }

    #[tokio::test]
    async fn theorem_with_title_renders_parenthesized() {
        let ast = run_full(vec![theorem_div(
            "thm-pyth",
            Some("Pythagoras"),
            "a^2+b^2=c^2.",
        )])
        .await;
        let strong = theorem_label_strong(&ast.blocks[0]);
        // Expected strong contents (new shape):
        //   "Theorem\u{a0}1", " (", "Pythagoras", ")"
        // No trailing period — Q1 doesn't emit one.
        let parts: Vec<String> = strong
            .content
            .iter()
            .map(|i| match i {
                Inline::Str(st) => st.text.clone(),
                _ => "?".into(),
            })
            .collect();
        assert_eq!(parts, vec!["Theorem\u{a0}1", " (", "Pythagoras", ")"]);
    }

    #[tokio::test]
    async fn theorem_counters_independent_from_lemmas() {
        let t1 = theorem_div("thm-a", None, "a");
        let t2 = theorem_div("thm-b", None, "b");
        let lemma_div = Block::Div(Div {
            attr: ("lem-c".into(), vec!["lemma".into()], LinkedHashMap::new()),
            content: vec![para("c")],
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        });
        let ast = run_full(vec![t1, t2, lemma_div]).await;
        let strong_text = |block: &Block| -> String {
            let strong = theorem_label_strong(block);
            let Inline::Str(st) = &strong.content[0] else {
                return String::new();
            };
            st.text.clone()
        };
        assert_eq!(strong_text(&ast.blocks[0]), "Theorem\u{a0}1");
        assert_eq!(strong_text(&ast.blocks[1]), "Theorem\u{a0}2");
        assert_eq!(strong_text(&ast.blocks[2]), "Lemma\u{a0}1");

        // Lemma Div carries the `theorem lemma` class pair.
        let Block::Div(lemma) = &ast.blocks[2] else {
            panic!()
        };
        assert_eq!(lemma.attr.1, vec!["theorem", "lemma"]);
    }

    #[tokio::test]
    async fn unnumbered_theorem_renders_without_number() {
        let div = Block::Div(Div {
            attr: (String::new(), vec!["theorem".into()], LinkedHashMap::new()),
            content: vec![para("unnumbered")],
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        });
        let ast = run_full(vec![div]).await;
        let strong = theorem_label_strong(&ast.blocks[0]);
        let Inline::Str(label) = &strong.content[0] else {
            panic!()
        };
        // No number — no id means no index entry means no order, so the
        // label is just `"Theorem"` (no nbsp either).
        assert_eq!(label.text, "Theorem");
    }

    #[tokio::test]
    async fn theorem_ref_resolves_to_link() {
        let body = vec![
            theorem_div("thm-x", None, "body"),
            Block::Paragraph(Paragraph {
                content: vec![str_inline("see "), cite("thm-x")],
                source_info: si(),
            }),
        ];
        let ast = run_full(body).await;
        let Block::Paragraph(p) = &ast.blocks[1] else {
            panic!()
        };
        let Inline::Link(link) = &p.content[1] else {
            panic!("expected Link for thm-x ref")
        };
        assert_eq!(link.target.0, "#thm-x");
        let Inline::Str(s) = &link.content[0] else {
            panic!()
        };
        assert_eq!(s.text, "Theorem\u{a0}1");
    }

    fn proof_div(title: Option<&str>, body: &str) -> Block {
        let mut kvs = LinkedHashMap::new();
        if let Some(t) = title {
            kvs.insert("name".into(), t.to_string());
        }
        Block::Div(Div {
            attr: (String::new(), vec!["proof".into()], kvs),
            content: vec![para(body)],
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        })
    }

    #[tokio::test]
    async fn proof_renders_with_italic_label() {
        let ast = run_full(vec![proof_div(None, "body text")]).await;
        let Block::Div(div) = &ast.blocks[0] else {
            panic!("expected Div, got {:?}", ast.blocks[0]);
        };
        assert!(div.attr.1.iter().any(|c| c == "proof"));
        let Block::Paragraph(p) = &div.content[0] else {
            panic!()
        };
        // First inline should be Emph("Proof."), then Space " ", then body.
        let Inline::Emph(em) = &p.content[0] else {
            panic!("expected Emph, got {:?}", p.content[0])
        };
        let Inline::Str(s) = &em.content[0] else {
            panic!()
        };
        assert_eq!(s.text, "Proof.");
    }

    #[tokio::test]
    async fn proof_with_custom_title_renders_italic() {
        let ast = run_full(vec![proof_div(Some("of Theorem 1"), "...")]).await;
        let Block::Div(div) = &ast.blocks[0] else {
            panic!()
        };
        let Block::Paragraph(p) = &div.content[0] else {
            panic!()
        };
        let Inline::Emph(em) = &p.content[0] else {
            panic!()
        };
        // Should be "of Theorem 1" + ".".
        let parts: Vec<String> = em
            .content
            .iter()
            .map(|i| match i {
                Inline::Str(st) => st.text.clone(),
                _ => "?".into(),
            })
            .collect();
        assert_eq!(parts, vec!["of Theorem 1", "."]);
    }

    // === Equation rendering tests ===

    use quarto_pandoc_types::inline::{Math, MathType, Span};

    fn eq_para(id: &str, math_text: &str) -> Block {
        Block::Paragraph(Paragraph {
            content: vec![Inline::Span(Span {
                attr: (
                    id.to_string(),
                    vec!["quarto-math-with-attribute".to_string()],
                    LinkedHashMap::new(),
                ),
                content: vec![Inline::Math(Math {
                    math_type: MathType::DisplayMath,
                    text: math_text.to_string(),
                    source_info: si(),
                    text_source: None,
                })],
                source_info: si(),
                attr_source: AttrSourceInfo::empty(),
            })],
            source_info: si(),
        })
    }

    /// After rendering, the equation CustomNode becomes a Span carrying
    /// the number as the reserved `quarto-eq-number` attribute. The math
    /// text is left byte-identical: the number's *encoding* (`\tag{N}`,
    /// `\qquad(N)`, a sibling label) is a format decision that
    /// `EquationNumberStage` makes later, after user post filters.
    #[tokio::test]
    async fn equation_renders_to_span_with_number_attribute() {
        let ast = run_full(vec![eq_para("eq-einstein", "e = mc^2")]).await;
        let Block::Paragraph(p) = &ast.blocks[0] else {
            panic!("expected Paragraph, got {:?}", ast.blocks[0]);
        };
        let Inline::Span(span) = &p.content[0] else {
            panic!("expected Span, got {:?}", p.content[0]);
        };
        assert_eq!(span.attr.0, "eq-einstein");
        assert_eq!(
            span.attr.2.get(EQ_NUMBER_ATTR).map(String::as_str),
            Some("1"),
            "the number rides on the reserved attribute; attrs: {:?}",
            span.attr.2
        );
        assert_eq!(span.content.len(), 1);
        let Inline::Math(math) = &span.content[0] else {
            panic!("expected Math, got {:?}", span.content[0]);
        };
        assert_eq!(math.math_type, MathType::DisplayMath);
        assert_eq!(math.text, "e = mc^2", "the math text is untouched");
    }

    /// A custom node without an `order` (nothing numbered it) renders to
    /// a span with no `quarto-eq-number` attribute and untouched math.
    #[test]
    fn unnumbered_equation_gets_no_attribute() {
        let mut slots = LinkedHashMap::new();
        slots.insert(
            "content".to_string(),
            Slot::Inlines(vec![Inline::Math(Math {
                math_type: MathType::DisplayMath,
                text: "x".to_string(),
                source_info: si(),
                text_source: None,
            })]),
        );
        let node = CustomNode {
            type_name: EQUATION.to_string(),
            slots,
            plain_data: serde_json::json!({}),
            attr: (
                "eq-plain".to_string(),
                vec!["quarto-math-with-attribute".to_string()],
                LinkedHashMap::new(),
            ),
            source_info: si(),
        };
        let Inline::Span(span) = render_equation(node, &float_state_for_tests()) else {
            panic!("expected Span");
        };
        assert!(!span.attr.2.contains_key(EQ_NUMBER_ATTR));
        let Inline::Math(math) = &span.content[0] else {
            panic!("expected Math");
        };
        assert_eq!(math.text, "x");
    }

    /// The math text and its byte-for-byte mapping (bd-ieldbghj) pass
    /// through untouched; the encoding that appends to the text (and
    /// extends the mapping) is `EquationNumberStage`'s, tested there.
    #[tokio::test]
    async fn equation_keeps_text_and_text_source_untouched() {
        let node = SourceInfo::original(FileId(0), 0, 12);
        let text = SourceInfo::original(FileId(0), 2, 10);
        let block = Block::Paragraph(Paragraph {
            content: vec![Inline::Span(Span {
                attr: (
                    "eq-einstein".to_string(),
                    vec!["quarto-math-with-attribute".to_string()],
                    LinkedHashMap::new(),
                ),
                content: vec![Inline::Math(Math {
                    math_type: MathType::DisplayMath,
                    text: "e = mc^2".to_string(),
                    source_info: node.clone(),
                    text_source: Some(text.clone()),
                })],
                source_info: node.clone(),
                attr_source: AttrSourceInfo::empty(),
            })],
            source_info: node,
        });
        let ast = run_full(vec![block]).await;
        let Block::Paragraph(p) = &ast.blocks[0] else {
            panic!("expected Paragraph, got {:?}", ast.blocks[0]);
        };
        let Inline::Span(span) = &p.content[0] else {
            panic!("expected Span, got {:?}", p.content[0]);
        };
        assert_eq!(
            span.attr.2.get(EQ_NUMBER_ATTR).map(String::as_str),
            Some("1")
        );
        let Inline::Math(math) = &span.content[0] else {
            panic!("expected Math, got {:?}", span.content[0]);
        };
        assert_eq!(math.text, "e = mc^2");
        assert_eq!(math.text_source.as_ref(), Some(&text));
    }

    #[tokio::test]
    async fn equation_ref_resolves_to_link() {
        let blocks = vec![
            eq_para("eq-x", "x^2"),
            Block::Paragraph(Paragraph {
                content: vec![str_inline("see "), cite("eq-x")],
                source_info: si(),
            }),
        ];
        let ast = run_full(blocks).await;
        let Block::Paragraph(p) = &ast.blocks[1] else {
            panic!();
        };
        let Inline::Link(link) = &p.content[1] else {
            panic!("expected Link, got {:?}", p.content[1]);
        };
        assert_eq!(link.target.0, "#eq-x");
        let Inline::Str(s) = &link.content[0] else {
            panic!();
        };
        assert_eq!(s.text, "Equation\u{a0}1");
    }

    #[tokio::test]
    async fn multiple_equations_number_sequentially() {
        let blocks = vec![
            eq_para("eq-a", "a"),
            eq_para("eq-b", "b"),
            eq_para("eq-c", "c"),
        ];
        let ast = run_full(blocks).await;
        for (i, block) in ast.blocks.iter().enumerate() {
            let Block::Paragraph(p) = block else {
                panic!();
            };
            let Inline::Span(span) = &p.content[0] else {
                panic!();
            };
            let Inline::Math(math) = &span.content[0] else {
                panic!();
            };
            assert_eq!(
                span.attr.2.get(EQ_NUMBER_ATTR).map(String::as_str),
                Some((i + 1).to_string().as_str()),
                "eq #{i}: attrs {:?}",
                span.attr.2
            );
            assert!(
                !math.text.contains("\\tag"),
                "eq #{i}: crossref-render must not encode the number; got '{}'",
                math.text
            );
        }
    }
}
