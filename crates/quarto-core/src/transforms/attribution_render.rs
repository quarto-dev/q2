/*
 * attribution_render.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! Attribution render transform.
//!
//! Reads `ctx.attribution_data` (the sidecar `Arc<AttributionData>`),
//! walks the AST once, and produces three artefacts on
//! `ctx.format_options`:
//!
//! 1. `Vec<Option<AttributionRecord>>` indexed by deterministic AST
//!    walk order. The writer-side test invariant uses this — the
//!    vec's length must be `>= 1` whenever attribution data is in
//!    scope (the meta SourceInfo always contributes at least one
//!    entry).
//! 2. A pointer-keyed `HashMap<usize, AttributionRecord>` keyed by
//!    `&block.source_info as *const SourceInfo as usize` /
//!    `&inline.source_info as *const SourceInfo as usize`. The HTML
//!    writer uses this for O(1) per-node lookup inside
//!    `write_block_source_attrs` / `write_inline_source_attrs`; the
//!    JSON writer uses the same key inside `maybe_record_attribution_for`
//!    to accumulate `astContext.attribution` entries. Pointer keys
//!    are stable because this transform is registered as the **last**
//!    transform in the Finalization Phase — no later code mutates the
//!    AST. Keying by the `SourceInfo` field address (rather than the
//!    enclosing `&Block` / `&Inline`) lets writers look up attribution
//!    without threading a typed node reference through every helper —
//!    the helpers already carry `&SourceInfo`.
//! 3. An [`IdentityMap`] containing every distinct actor that
//!    appears in `runs`. Resolves identity **once per distinct
//!    actor**; fires at most K diagnostics per render when the
//!    producer invariant is violated, not N.
//!
//! Registered as the **very last** transform in the Finalization
//! Phase, immediately after `ResourceCollectorTransform`. The entire
//! Finalization Phase runs between
//! [`AttributionGenerateTransform`](super::AttributionGenerateTransform)
//! and this stage.
//!
//! Reads and writes only [`RenderContext`] fields; never reaches for
//! `StageContext`. See `attribution_generate.rs` module docs for the
//! invocation-path invariant.
//!
//! ## Byte-range resolution
//!
//! The transform chain-resolves each node's [`SourceInfo`] to a
//! `(file_id, start, end)` tuple **without** needing a
//! [`SourceContext`](quarto_source_map::SourceContext). It only
//! traverses `Original`/`Substring` chains; `Concat` and
//! `FilterProvenance` resolve to `None`. This mirrors the v1
//! single-doc invariant: project-scoped attribution (v2) will need
//! the full chain resolver via `map_offset`.
//!
//! [`IdentityMap`]: crate::attribution::IdentityMap

use std::collections::HashMap;
use std::sync::Arc;

use quarto_error_reporting::DiagnosticMessage;
use quarto_pandoc_types::block::Block;
use quarto_pandoc_types::custom::{CustomNode, Slot};
use quarto_pandoc_types::inline::Inline;
use quarto_pandoc_types::pandoc::Pandoc;
use quarto_source_map::SourceInfo;

use crate::Result;
use crate::attribution::{
    AttributionMap, AttributionRecord, AttributionSource, Identity, IdentityMap,
    attribution_viewer_enabled_from_meta, resolve_byte_range,
};
use crate::render::RenderContext;
use crate::transform::AstTransform;

pub struct AttributionRenderTransform;

impl AttributionRenderTransform {
    pub fn new() -> Self {
        Self
    }
}

impl Default for AttributionRenderTransform {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait(?Send)]
impl AstTransform for AttributionRenderTransform {
    fn name(&self) -> &str {
        "attribution-render"
    }

    async fn transform(&self, ast: &mut Pandoc, ctx: &mut RenderContext) -> Result<()> {
        let Some(data) = ctx.attribution_data.clone() else {
            return Ok(());
        };

        // Build the actors table from every distinct actor referenced
        // by `runs`. Fire one diagnostic per actor missing from
        // `identities`; use the warning-path placeholder for those.
        // This is K (= distinct actors) identity resolutions, not N
        // (= records).
        let mut actors: IdentityMap = IdentityMap::new();
        for run in data.runs.as_slice() {
            if actors.contains_key(&run.actor) {
                continue;
            }
            let identity = match data.identities.get(&run.actor) {
                Some(id) => id.clone(),
                None => {
                    ctx.diagnostics.push(DiagnosticMessage::warning(format!(
                        "attribution actor '{}' has no resolved identity; \
                         using <unknown>/#888888 placeholder",
                        run.actor
                    )));
                    Identity {
                        display_name: "<unknown>".to_string(),
                        color: "#888888".to_string(),
                    }
                }
            };
            // Preserve the provider's `Arc<str>` as the map key so the
            // writer-side `Arc::ptr_eq` invariant between
            // `AttributionRecord.actor` and the identity map key
            // holds.
            actors.insert(Arc::clone(&run.actor), identity);
        }

        // Walk the AST once. The meta SourceInfo contributes the
        // first slice entry — this is the test-invariant load-bearing
        // bit for the "lookup is non-empty when attribution is on"
        // assertion. Block/Inline visits populate BOTH the slice (in
        // walk order) and the pointer-keyed map (for O(1) writer
        // lookup).
        let mut slice: Vec<Option<AttributionRecord>> = Vec::new();
        let mut by_node: HashMap<usize, AttributionRecord> = HashMap::new();
        // bd-ky14a: determine the primary FileId by inspecting the
        // first block whose source info resolves to a real file.
        // Top-level `ast.meta.source_info` is frequently
        // `SourceInfo::default()` (FileId 0) when the metadata map
        // is synthesized by MetadataMergeStage, so it's not a
        // reliable anchor. The first block's source info, in
        // contrast, comes from the parser and carries the document's
        // hash-based FileId. Nodes whose source spans resolve to a
        // *different* FileId (includes, splices) are skipped.
        let primary_file_id = ast
            .blocks
            .iter()
            .find_map(|b| resolve_byte_range(b.source_info()).map(|(fid, _, _)| fid))
            .unwrap_or(0);
        slice.push(query_attribution(
            &ast.meta.source_info,
            &data.runs,
            primary_file_id,
        ));
        for block in &ast.blocks {
            walk_block(block, &data.runs, primary_file_id, &mut slice, &mut by_node);
        }

        let slice_arc: Arc<[Option<AttributionRecord>]> = Arc::from(slice.into_boxed_slice());
        let by_node_arc = Arc::new(by_node);
        let actors_arc = Arc::new(actors);

        ctx.format_options.html.attribution_lookup = Some(Arc::clone(&slice_arc));
        ctx.format_options.html.attribution_by_node = Some(Arc::clone(&by_node_arc));
        ctx.format_options.html.attribution_identities = Some(Arc::clone(&actors_arc));
        // YAML opt-out for the auto-injected viewer scaffolding;
        // `AttributionViewerTransform` reads this and short-circuits
        // when false. Read once here so the bool travels through
        // `format_options` alongside the other attribution fields.
        ctx.format_options.html.attribution_viewer_enabled =
            attribution_viewer_enabled_from_meta(&ast.meta);
        ctx.format_options.json.attribution_lookup = Some(slice_arc);
        ctx.format_options.json.attribution_by_node = Some(by_node_arc);
        ctx.format_options.json.attribution_actors = Some(actors_arc);

        Ok(())
    }
}

/// Query `runs` for the most-recent `(actor, time)` hit covering the
/// given SourceInfo, applying the v1 single-doc invariant: if the
/// node resolves to a file other than the primary (e.g. spliced in
/// via `{{< include other.qmd >}}`), return `None` without querying.
/// This prevents silent misattribution by byte-range collision
/// against the primary doc's runs.
///
/// bd-ky14a: `primary_file_id` is the [`quarto_yaml::file_id_for_filename`]
/// hash of the primary document — replaces the old `file_id == 0`
/// check now that pampa's FileIds are hash-based.
fn query_attribution(
    si: &SourceInfo,
    runs: &AttributionMap,
    primary_file_id: usize,
) -> Option<AttributionRecord> {
    let (file_id, start, end) = resolve_byte_range(si)?;
    if file_id != primary_file_id || start >= end {
        return None;
    }
    runs.query_byte_range(start, end)
}

fn visit_block(
    block: &Block,
    runs: &AttributionMap,
    primary_file_id: usize,
    slice: &mut Vec<Option<AttributionRecord>>,
    by_node: &mut HashMap<usize, AttributionRecord>,
) {
    let source_info = block.source_info();
    let record = query_attribution(source_info, runs, primary_file_id);
    if let Some(r) = record.as_ref() {
        let key = source_info as *const SourceInfo as usize;
        by_node.insert(key, r.clone());
    }
    slice.push(record);
}

fn visit_inline(
    inline: &Inline,
    runs: &AttributionMap,
    primary_file_id: usize,
    slice: &mut Vec<Option<AttributionRecord>>,
    by_node: &mut HashMap<usize, AttributionRecord>,
) {
    let source_info = inline.source_info();
    let record = query_attribution(source_info, runs, primary_file_id);
    if let Some(r) = record.as_ref() {
        let key = source_info as *const SourceInfo as usize;
        by_node.insert(key, r.clone());
    }
    slice.push(record);
}

fn walk_block(
    block: &Block,
    runs: &AttributionMap,
    primary_file_id: usize,
    slice: &mut Vec<Option<AttributionRecord>>,
    by_node: &mut HashMap<usize, AttributionRecord>,
) {
    visit_block(block, runs, primary_file_id, slice, by_node);
    match block {
        Block::Plain(b) => walk_inlines(&b.content, runs, primary_file_id, slice, by_node),
        Block::Paragraph(b) => walk_inlines(&b.content, runs, primary_file_id, slice, by_node),
        Block::LineBlock(b) => {
            for line in &b.content {
                walk_inlines(line, runs, primary_file_id, slice, by_node);
            }
        }
        Block::BlockQuote(b) => walk_blocks(&b.content, runs, primary_file_id, slice, by_node),
        Block::OrderedList(b) => {
            for item in &b.content {
                walk_blocks(item, runs, primary_file_id, slice, by_node);
            }
        }
        Block::BulletList(b) => {
            for item in &b.content {
                walk_blocks(item, runs, primary_file_id, slice, by_node);
            }
        }
        Block::DefinitionList(b) => {
            for (term, defs) in &b.content {
                walk_inlines(term, runs, primary_file_id, slice, by_node);
                for def in defs {
                    walk_blocks(def, runs, primary_file_id, slice, by_node);
                }
            }
        }
        Block::Header(b) => walk_inlines(&b.content, runs, primary_file_id, slice, by_node),
        Block::Figure(b) => walk_blocks(&b.content, runs, primary_file_id, slice, by_node),
        Block::Div(b) => walk_blocks(&b.content, runs, primary_file_id, slice, by_node),
        Block::Table(t) => {
            if let Some(short) = &t.caption.short {
                walk_inlines(short, runs, primary_file_id, slice, by_node);
            }
            if let Some(long) = &t.caption.long {
                walk_blocks(long, runs, primary_file_id, slice, by_node);
            }
            for row in t.head.rows.iter().chain(t.foot.rows.iter()) {
                for cell in &row.cells {
                    walk_blocks(&cell.content, runs, primary_file_id, slice, by_node);
                }
            }
            for body in &t.bodies {
                for row in &body.body {
                    for cell in &row.cells {
                        walk_blocks(&cell.content, runs, primary_file_id, slice, by_node);
                    }
                }
            }
        }
        Block::NoteDefinitionPara(b) => {
            walk_inlines(&b.content, runs, primary_file_id, slice, by_node)
        }
        Block::NoteDefinitionFencedBlock(b) => {
            walk_blocks(&b.content, runs, primary_file_id, slice, by_node)
        }
        Block::CaptionBlock(b) => walk_inlines(&b.content, runs, primary_file_id, slice, by_node),
        Block::Custom(c) => walk_custom_node(c, runs, primary_file_id, slice, by_node),
        Block::CodeBlock(_)
        | Block::RawBlock(_)
        | Block::HorizontalRule(_)
        | Block::BlockMetadata(_) => {}
    }
}

fn walk_blocks(
    blocks: &[Block],
    runs: &AttributionMap,
    primary_file_id: usize,
    slice: &mut Vec<Option<AttributionRecord>>,
    by_node: &mut HashMap<usize, AttributionRecord>,
) {
    for b in blocks {
        walk_block(b, runs, primary_file_id, slice, by_node);
    }
}

fn walk_inline(
    inline: &Inline,
    runs: &AttributionMap,
    primary_file_id: usize,
    slice: &mut Vec<Option<AttributionRecord>>,
    by_node: &mut HashMap<usize, AttributionRecord>,
) {
    visit_inline(inline, runs, primary_file_id, slice, by_node);
    match inline {
        Inline::Emph(e) => walk_inlines(&e.content, runs, primary_file_id, slice, by_node),
        Inline::Underline(u) => walk_inlines(&u.content, runs, primary_file_id, slice, by_node),
        Inline::Strong(s) => walk_inlines(&s.content, runs, primary_file_id, slice, by_node),
        Inline::Strikeout(s) => walk_inlines(&s.content, runs, primary_file_id, slice, by_node),
        Inline::Superscript(s) => walk_inlines(&s.content, runs, primary_file_id, slice, by_node),
        Inline::Subscript(s) => walk_inlines(&s.content, runs, primary_file_id, slice, by_node),
        Inline::SmallCaps(s) => walk_inlines(&s.content, runs, primary_file_id, slice, by_node),
        Inline::Quoted(q) => walk_inlines(&q.content, runs, primary_file_id, slice, by_node),
        Inline::Cite(c) => walk_inlines(&c.content, runs, primary_file_id, slice, by_node),
        Inline::Link(l) => walk_inlines(&l.content, runs, primary_file_id, slice, by_node),
        Inline::Image(i) => walk_inlines(&i.content, runs, primary_file_id, slice, by_node),
        Inline::Note(n) => walk_blocks(&n.content, runs, primary_file_id, slice, by_node),
        Inline::Span(s) => walk_inlines(&s.content, runs, primary_file_id, slice, by_node),
        Inline::Insert(s) => walk_inlines(&s.content, runs, primary_file_id, slice, by_node),
        Inline::Delete(s) => walk_inlines(&s.content, runs, primary_file_id, slice, by_node),
        Inline::Highlight(s) => walk_inlines(&s.content, runs, primary_file_id, slice, by_node),
        Inline::EditComment(s) => walk_inlines(&s.content, runs, primary_file_id, slice, by_node),
        Inline::Custom(c) => walk_custom_node(c, runs, primary_file_id, slice, by_node),
        Inline::Str(_)
        | Inline::Code(_)
        | Inline::Space(_)
        | Inline::SoftBreak(_)
        | Inline::LineBreak(_)
        | Inline::Math(_)
        | Inline::RawInline(_)
        | Inline::Shortcode(_)
        | Inline::NoteReference(_)
        | Inline::Attr(_) => {}
    }
}

fn walk_inlines(
    inlines: &[Inline],
    runs: &AttributionMap,
    primary_file_id: usize,
    slice: &mut Vec<Option<AttributionRecord>>,
    by_node: &mut HashMap<usize, AttributionRecord>,
) {
    for i in inlines {
        walk_inline(i, runs, primary_file_id, slice, by_node);
    }
}

fn walk_custom_node(
    node: &CustomNode,
    runs: &AttributionMap,
    primary_file_id: usize,
    slice: &mut Vec<Option<AttributionRecord>>,
    by_node: &mut HashMap<usize, AttributionRecord>,
) {
    for (_name, slot) in &node.slots {
        match slot {
            Slot::Block(b) => walk_block(b, runs, primary_file_id, slice, by_node),
            Slot::Blocks(bs) => walk_blocks(bs, runs, primary_file_id, slice, by_node),
            Slot::Inline(i) => walk_inline(i, runs, primary_file_id, slice, by_node),
            Slot::Inlines(is) => walk_inlines(is, runs, primary_file_id, slice, by_node),
        }
    }
}
