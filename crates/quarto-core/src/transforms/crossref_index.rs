/*
 * transforms/crossref_index.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Build the per-document crossref index.
 */

//! Build the per-document crossref index.
//!
//! This transform runs in the **crossref phase** (see plan D3), after
//! `FloatRefTargetSugarTransform` has canonicalized every float crossref
//! target into [`CustomNode("FloatRefTarget")`](crate::crossref::FLOAT_REF_TARGET).
//! It walks the AST in document order and:
//!
//! 1. Tracks a section counter stack driven by `Header` blocks. The stack
//!    is the same thing the reader would say is "section 1.2.3".
//! 2. For each crossref target it sees, assigns an [`Order`] with the
//!    current section snapshot and a ref-type-scoped 1-based counter.
//! 3. Stashes the assigned order back into the custom node's
//!    `plain_data.order` so renderers can read it without looking up the
//!    index.
//! 4. Populates [`CrossrefIndex::entries`] with a [`CrossrefEntry`] per
//!    target. Duplicates are recorded as a diagnostic and the first
//!    occurrence wins (this mirrors what a reader will actually see when
//!    resolving `@id` — if two targets share an id, only one can be
//!    linked to).
//! 5. At the end, publishes the index as a trace entry via
//!    [`PipelineObserver::on_auxiliary_data`] under
//!    [`TRACE_KIND_CROSSREF_INDEX`], so the trace viewer and tests can
//!    inspect the structured index independently of the rendered HTML.
//!
//! ## Scope
//!
//! Phase 1 implements single-file, flat numbering (no chapters, no
//! appendix-aware renumbering). Subfloats and appendix behavior are
//! deferred to follow-up tasks (see plan notes).

use quarto_analysis::AnalysisContext;
use quarto_error_reporting::DiagnosticMessage;
use quarto_pandoc_types::block::{Block, Blocks, Header};
use quarto_pandoc_types::custom::{CustomNode, Slot};
use quarto_pandoc_types::inline::{Inline, Inlines};
use quarto_pandoc_types::pandoc::Pandoc;
use serde_json::json;

use crate::Result;
use crate::crossref::{
    CrossrefEntry, CrossrefIndex, Order, TRACE_KIND_CROSSREF_INDEX, format_section_number,
};
use crate::render::RenderContext;
use crate::transform::{AstTransform, TransformPhase};

/// Transform that builds the per-document crossref index.
pub struct CrossrefIndexTransform;

impl CrossrefIndexTransform {
    pub fn new() -> Self {
        Self
    }
}

impl Default for CrossrefIndexTransform {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait(?Send)]
impl AstTransform for CrossrefIndexTransform {
    fn name(&self) -> &str {
        "crossref-index"
    }

    fn phase(&self) -> TransformPhase {
        TransformPhase::Crossref
    }

    async fn transform(&self, ast: &mut Pandoc, ctx: &mut RenderContext) -> Result<()> {
        // If no index was seeded by the pre-engine stage, create one here
        // so transforms that bypass the full pipeline still get an index.
        if ctx.crossref_index.is_none() {
            ctx.crossref_index = Some(CrossrefIndex::new(quarto_source_map::FileId(0)));
        }
        let mut index = ctx.crossref_index.take().unwrap();

        // Book-projects P0: a chapter rendered standalone seeds the section
        // counter with its chapter number (`sections = [n-1]`, so the first
        // H1 becomes `n`), reproducing Q1's per-file section offsets. Guard
        // on an untouched stack so a pre-seeded index is never clobbered.
        if let Some(seed) = &ctx.chapter_seed
            && index.sections.is_empty()
            && seed.chapter_number > 1
        {
            index.sections = vec![seed.chapter_number - 1];
        }

        // Q1's `crossref.maxHeading` (options.lua + normalize/flags.lua):
        // 1 when `crossref.chapters` is set, 7 otherwise, then the minimum
        // over every header's level (unnumbered headers included — flags.lua
        // doesn't discriminate). Pre-scanned so the number stash (visible
        // numbering) and `format_section_number` see the final value.
        let chapters = ast
            .meta
            .get("crossref")
            .and_then(|c| c.get("chapters"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        index.max_heading = compute_max_heading(&ast.blocks, chapters);

        // sections.lua: the visible-number stash is gated on `number-sections`
        // (default off) and `number-depth` (default 6) — independent of the
        // `sec`-target registration above, which is unconditional.
        let number_sections = ast
            .meta
            .get("number-sections")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let number_depth = ast
            .meta
            .get("number-depth")
            .and_then(|v| v.as_int_lenient())
            .map_or(6, |n| n.max(0) as u32);

        let mut walker = Walker {
            index: &mut index,
            diagnostics: Vec::new(),
            registry: ctx.ref_type_registry.clone(),
            // sec-target registration is native-HTML-only: the pandoc-hybrid
            // pipeline also runs this transform, and registering there would
            // let crossref-resolve consume `@sec-` cites before the vendored
            // refs.lua sees them.
            html: ctx.format.identifier.is_html_based(),
            // Q1 marks every entry with the per-file appendix state; with
            // per-file seeds the whole file shares it.
            appendix: ctx.chapter_seed.as_ref().is_some_and(|s| s.is_appendix),
            section_ids: Vec::new(),
            number_sections,
            number_depth,
        };
        walker.visit_blocks(&mut ast.blocks);
        let diagnostics = walker.diagnostics;

        // Push diagnostics to context.
        for diag in diagnostics {
            ctx.add_diagnostic(diag);
        }

        // Publish the index to the trace observer. Errors here are
        // intentionally silent — a serialization failure should not break
        // a render (and our types are guaranteed to serialize).
        if let Ok(payload) = serde_json::to_value(&index) {
            ctx.observer
                .on_auxiliary_data(self.name(), 0, TRACE_KIND_CROSSREF_INDEX, &payload);
        }

        ctx.crossref_index = Some(index);
        Ok(())
    }
}

/// Walker state carried through the AST traversal.
struct Walker<'a> {
    index: &'a mut CrossrefIndex,
    diagnostics: Vec<DiagnosticMessage>,
    /// The document's ref-type registry; used to recognize `sec`-classifying
    /// header ids. `None` only in contexts that never installed one.
    registry: Option<crate::crossref::RefTypeRegistry>,
    /// Native-HTML-family render (see `transform` for why registration is
    /// gated on this).
    html: bool,
    /// Per-file appendix state from the chapter seed (Q1's
    /// `currentFileMetadataState().appendix`).
    appendix: bool,
    /// Ids of enclosing `Div.section` wrappers, innermost last.
    /// `SectionizeTransform` (Normalization phase) moves each header's id
    /// onto its section div and empties the header's own id, so
    /// `visit_header` recovers its target id from here when it has none.
    section_ids: Vec<String>,
    /// `number-sections` document metadata (default off) — gates the
    /// visible `number` kv stash, independent of `sec`-target registration.
    number_sections: bool,
    /// `number-depth` document metadata (default 6) — a header deeper than
    /// this never gets the visible `number` kv, even with `number-sections`.
    number_depth: u32,
}

impl<'a> Walker<'a> {
    fn visit_blocks(&mut self, blocks: &mut Blocks) {
        for block in blocks.iter_mut() {
            self.visit_block(block);
        }
    }

    fn visit_block(&mut self, block: &mut Block) {
        match block {
            Block::Header(h) => {
                self.visit_header(h);
                self.visit_inlines(&mut h.content);
            }
            Block::Paragraph(p) => self.visit_inlines(&mut p.content),
            Block::Plain(p) => self.visit_inlines(&mut p.content),
            Block::LineBlock(lb) => {
                for line in &mut lb.content {
                    self.visit_inlines(line);
                }
            }
            Block::Div(div) => {
                // Track section-div ids while descending so a header whose
                // id sectionize moved onto its wrapper can still register.
                if div.attr.1.iter().any(|c| c == "section") {
                    self.section_ids.push(div.attr.0.clone());
                    self.visit_blocks(&mut div.content);
                    self.section_ids.pop();
                } else {
                    self.visit_blocks(&mut div.content);
                }
            }
            Block::BlockQuote(bq) => self.visit_blocks(&mut bq.content),
            Block::OrderedList(ol) => {
                for item in &mut ol.content {
                    self.visit_blocks(item);
                }
            }
            Block::BulletList(bl) => {
                for item in &mut bl.content {
                    self.visit_blocks(item);
                }
            }
            Block::DefinitionList(dl) => {
                for (term, defs) in &mut dl.content {
                    self.visit_inlines(term);
                    for def in defs {
                        self.visit_blocks(def);
                    }
                }
            }
            Block::Figure(fig) => {
                self.visit_blocks(&mut fig.content);
                if let Some(long) = fig.caption.long.as_mut() {
                    self.visit_blocks(long);
                }
                if let Some(short) = fig.caption.short.as_mut() {
                    self.visit_inlines(short);
                }
            }
            Block::Custom(node) => self.visit_custom(node),
            _ => {}
        }
    }

    fn visit_inlines(&mut self, inlines: &mut Inlines) {
        for inline in inlines.iter_mut() {
            self.visit_inline(inline);
        }
    }

    fn visit_inline(&mut self, inline: &mut Inline) {
        // Recurse into container inlines.
        match inline {
            Inline::Emph(e) => self.visit_inlines(&mut e.content),
            Inline::Underline(u) => self.visit_inlines(&mut u.content),
            Inline::Strong(s) => self.visit_inlines(&mut s.content),
            Inline::Strikeout(s) => self.visit_inlines(&mut s.content),
            Inline::Superscript(s) => self.visit_inlines(&mut s.content),
            Inline::Subscript(s) => self.visit_inlines(&mut s.content),
            Inline::SmallCaps(s) => self.visit_inlines(&mut s.content),
            Inline::Quoted(q) => self.visit_inlines(&mut q.content),
            Inline::Link(l) => self.visit_inlines(&mut l.content),
            Inline::Image(i) => self.visit_inlines(&mut i.content),
            Inline::Note(n) => self.visit_blocks(&mut n.content),
            Inline::Span(s) => self.visit_inlines(&mut s.content),
            Inline::Insert(i) => self.visit_inlines(&mut i.content),
            Inline::Delete(d) => self.visit_inlines(&mut d.content),
            Inline::Highlight(h) => self.visit_inlines(&mut h.content),
            Inline::Custom(node) => {
                // Recurse into slots first.
                for (_k, slot) in node.slots.iter_mut() {
                    match slot {
                        Slot::Block(b) => self.visit_block(b),
                        Slot::Blocks(bs) => self.visit_blocks(bs),
                        Slot::Inline(i) => self.visit_inline(i),
                        Slot::Inlines(is) => self.visit_inlines(is),
                    }
                }
                // Index inline custom nodes with crossref triple.
                if has_crossref_plain_data(node) {
                    self.index_custom_target(node);
                }
            }
            _ => {}
        }
    }

    fn visit_header(&mut self, header: &mut Header) {
        // Port of sections.lua's Header handler, in its exact order:
        // index the heading unconditionally, THEN early-return on
        // `.unnumbered` before any counter or registration logic.
        let unnumbered = header.attr.1.iter().any(|c| c == "unnumbered");
        if !unnumbered {
            advance_sections(&mut self.index.sections, header.level);
        }
        // Record the heading; needed for cross-file book fixup in future
        // phases, and cheap to collect here. Numbered headings get their
        // own (post-advance) path; unnumbered ones the enclosing path.
        // The header's own id wins when present (pandoc-hybrid pipeline,
        // where sectionize doesn't run; blockquoted headers, which
        // sectionize doesn't descend into); otherwise fall back to the
        // innermost enclosing section div's id — sound because sectionize
        // makes every header it touches the first child of its own section
        // div with an empty id.
        let identifier = if !header.attr.0.is_empty() {
            Some(header.attr.0.clone())
        } else {
            self.section_ids
                .iter()
                .rev()
                .find(|id| !id.is_empty())
                .cloned()
        };
        self.index.headings.push(crate::crossref::HeadingRecord {
            identifier: identifier.clone(),
            level: header.level as u8,
            section: self.index.sections.clone(),
            source_info: header.source_info.clone(),
        });
        if unnumbered || !self.html {
            return;
        }
        // Book-projects P0 / sections.lua: stash the visible section number
        // as a `number` kv (the HTML writer emits it as `data-number` for
        // free) — gated only on `number-sections`/`number-depth`, and
        // deliberately placed before the identifier/`sec`-target checks
        // below, since Q1 stashes this for every numbered header regardless
        // of whether it's also an `@sec-` target.
        if self.number_sections && (header.level as u32) <= self.number_depth {
            header.attr.2.insert(
                "number".to_string(),
                format_section_number(&self.index.sections, self.index.max_heading, self.appendix),
            );
        }
        // Register `sec`-classifying ids as crossref targets — this, not
        // `number-sections`, is what makes `@sec-` refs resolve (Q1
        // registers unconditionally; only visible numbering is gated).
        let Some(identifier) = identifier else { return };
        let is_sec = self
            .registry
            .as_ref()
            .and_then(|r| r.classify_cite_id(&identifier))
            .is_some_and(|def| def.ref_type == "sec");
        if !is_sec {
            return;
        }
        self.index_target(
            identifier,
            "sec".to_string(),
            Some(header.content.clone()),
            &header.source_info,
        );
    }

    fn visit_custom(&mut self, node: &mut CustomNode) {
        // Recurse into child blocks first so nested targets are indexed
        // in document order.
        for (_name, slot) in node.slots.iter_mut() {
            match slot {
                Slot::Block(b) => self.visit_block(b),
                Slot::Blocks(bs) => self.visit_blocks(bs),
                _ => {}
            }
        }
        // Any custom node that carries the standard crossref triple
        // (ref_type + kind + non-empty identifier) is eligible for
        // indexing. Today that's `FloatRefTarget` and `Theorem`;
        // Callouts with crossref ids get it via the callout annotation
        // pass. We inline the predicate rather than call
        // `crossref_target_view` to avoid synthesizing a Block just for
        // the check.
        if has_crossref_plain_data(node) {
            self.index_custom_target(node);
        }
    }

    fn index_custom_target(&mut self, node: &mut CustomNode) {
        let identifier = node.attr.0.clone();
        if identifier.is_empty() {
            return;
        }

        let ref_type = match node.plain_data.get("ref_type").and_then(|v| v.as_str()) {
            Some(s) => s.to_string(),
            None => return,
        };

        let caption = extract_caption_inlines(node);
        let source_info = node.source_info.clone();
        let Some(order) = self.index_target(identifier, ref_type, caption, &source_info) else {
            return; // duplicate id; diagnostic already emitted
        };

        // Write the order back into the node so renderers don't need to
        // round-trip through the index. `plain_data.order` is an object
        // `{ "section": [...], "order": n }` — same shape as `Order` itself.
        if let Some(obj) = node.plain_data.as_object_mut() {
            obj.insert(
                "order".into(),
                json!({
                    "section": order.section,
                    "order": order.order,
                }),
            );
        }
    }

    /// Shared registration path for every crossref target kind (float
    /// custom nodes, theorem nodes, and — book-projects P0 — `sec`-typed
    /// headers). Duplicate-id check (first occurrence wins, Q-15-1
    /// diagnostic), per-ref-type counter advance, `CrossrefEntry`
    /// construction and insert. Returns the assigned [`Order`] so callers
    /// with a node can write it back into `plain_data`; `None` on a
    /// duplicate.
    fn index_target(
        &mut self,
        identifier: String,
        ref_type: String,
        caption: Option<Inlines>,
        source_info: &quarto_source_map::SourceInfo,
    ) -> Option<Order> {
        // Duplicate id check: skip numbering the duplicate.
        if self.index.entries.contains_key(&identifier) {
            let existing_src = self.index.entries[&identifier].source_info.clone();
            self.diagnostics.push(duplicate_id_diagnostic(
                &identifier,
                &existing_src,
                source_info,
            ));
            return None;
        }

        // Increment the per-ref-type counter.
        let order_num = {
            let counter = self.index.next_order.entry(ref_type.clone()).or_insert(0);
            *counter += 1;
            *counter
        };

        let order = Order {
            section: self.index.sections.clone(),
            order: order_num,
        };

        let entry = CrossrefEntry {
            identifier: identifier.clone(),
            ref_type,
            parent: None, // subfloats deferred
            order: order.clone(),
            caption,
            in_appendix: self.appendix,
            source_info: source_info.clone(),
        };
        self.index.insert(entry);
        Some(order)
    }
}

/// True if `node` carries the standard crossref triple in `plain_data`
/// (non-empty `identifier` on attr, plus `ref_type` and `kind` strings
/// in `plain_data`). Mirrors what `crossref_target_view` checks, but
/// avoids synthesizing a `Block` for the query.
fn has_crossref_plain_data(node: &CustomNode) -> bool {
    if node.attr.0.is_empty() {
        return false;
    }
    let has_ref_type = node
        .plain_data
        .get("ref_type")
        .and_then(|v| v.as_str())
        .is_some();
    let has_kind = node
        .plain_data
        .get("kind")
        .and_then(|v| v.as_str())
        .is_some();
    has_ref_type && has_kind
}

/// Advance the section counter stack when a header of `level` is seen.
fn advance_sections(sections: &mut Vec<u32>, level: usize) {
    if sections.len() > level {
        sections.truncate(level);
    }
    while sections.len() < level {
        sections.push(0);
    }
    if let Some(last) = sections.last_mut() {
        *last += 1;
    }
}

/// Q1's `crossref.maxHeading` (options.lua + normalize/flags.lua): 1 when
/// `crossref.chapters` is set, 7 otherwise, then the minimum over every
/// header's level anywhere in the document.
fn compute_max_heading(blocks: &Blocks, chapters: bool) -> u32 {
    let mut max = if chapters { 1 } else { 7 };
    scan_min_header_level(blocks, &mut max);
    max
}

fn scan_min_header_level(blocks: &Blocks, min: &mut u32) {
    for block in blocks.iter() {
        scan_block(block, min);
    }
}

fn scan_block(block: &Block, min: &mut u32) {
    match block {
        Block::Header(h) => *min = (*min).min(h.level as u32),
        Block::Div(d) => scan_min_header_level(&d.content, min),
        Block::BlockQuote(bq) => scan_min_header_level(&bq.content, min),
        Block::OrderedList(ol) => {
            for item in &ol.content {
                scan_min_header_level(item, min);
            }
        }
        Block::BulletList(bl) => {
            for item in &bl.content {
                scan_min_header_level(item, min);
            }
        }
        Block::DefinitionList(dl) => {
            for (_term, defs) in &dl.content {
                for def in defs {
                    scan_min_header_level(def, min);
                }
            }
        }
        Block::Figure(fig) => scan_min_header_level(&fig.content, min),
        Block::Custom(node) => {
            for (_name, slot) in node.slots.iter() {
                match slot {
                    Slot::Block(b) => scan_block(b, min),
                    Slot::Blocks(bs) => scan_min_header_level(bs, min),
                    _ => {}
                }
            }
        }
        _ => {}
    }
}

/// Pull caption inlines out of a FloatRefTarget custom node for use as
/// link text when `@id` is resolved.
///
/// Prefers `caption_short` (a short form authored by the user). Falls back
/// to the inlines of `caption_long`'s first inline-bearing block — a
/// `Paragraph` (div-form trailing paragraph) or a `Plain` (Pandoc-native
/// `Figure` / `Table` captions). Matching Paragraph only used to leave
/// every attr-form figure and every table without an indexed caption
/// (bd-n3sark9b).
fn extract_caption_inlines(node: &CustomNode) -> Option<quarto_pandoc_types::inline::Inlines> {
    if let Some(Slot::Inlines(short)) = node.slots.get("caption_short")
        && !short.is_empty()
    {
        return Some(short.clone());
    }
    if let Some(Slot::Blocks(long)) = node.slots.get("caption_long") {
        for block in long {
            match block {
                Block::Paragraph(p) => return Some(p.content.clone()),
                Block::Plain(p) => return Some(p.content.clone()),
                _ => {}
            }
        }
    }
    None
}

/// Build the `Q-15-1` diagnostic for a crossref id defined more than
/// once (bd-rr6qzcvu).
///
/// `first` is the occurrence already in the index; `second` is the
/// duplicate the indexer is about to skip. The primary location points
/// at `second` (the duplicate the user just added); a located detail
/// points back at `first`, so the renderer underlines both sites.
fn duplicate_id_diagnostic(
    id: &str,
    first: &quarto_source_map::SourceInfo,
    second: &quarto_source_map::SourceInfo,
) -> DiagnosticMessage {
    quarto_error_reporting::DiagnosticMessageBuilder::error("Duplicate crossref identifier")
        .with_code("Q-15-1")
        .with_location(second.clone())
        .problem(format!(
            "The crossref identifier `{id}` is defined more than once. Each \
             crossref id must be unique within a document so a reference like \
             `@{id}` resolves to exactly one target."
        ))
        .add_detail_at("first defined here", first.clone())
        .add_hint("Give one of the targets a different `label:` (or `#id`).")
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crossref::RefTypeRegistry;
    use crate::transforms::FloatRefTargetSugarTransform;
    use hashlink::LinkedHashMap;
    use quarto_pandoc_types::attr::{Attr, AttrSourceInfo};
    use quarto_pandoc_types::block::{Block, CodeBlock, Div, Header, Paragraph};
    use quarto_pandoc_types::inline::{Inline, Str};
    use quarto_source_map::{FileId, SourceInfo};

    fn si() -> SourceInfo {
        SourceInfo::original(FileId(0), 0, 0)
    }

    fn attr_id(id: &str) -> Attr {
        (id.to_string(), Vec::new(), LinkedHashMap::new())
    }

    fn header(level: usize, id: &str, text: &str) -> Block {
        Block::Header(Header {
            level,
            attr: attr_id(id),
            content: vec![Inline::Str(Str {
                text: text.to_string(),
                source_info: si(),
            })],
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        })
    }

    fn header_with_class(level: usize, id: &str, class: &str, text: &str) -> Block {
        Block::Header(Header {
            level,
            attr: (
                id.to_string(),
                vec![class.to_string()],
                LinkedHashMap::new(),
            ),
            content: vec![Inline::Str(Str {
                text: text.to_string(),
                source_info: si(),
            })],
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        })
    }

    fn fig_div(id: &str, caption: &str) -> Block {
        Block::Div(Div {
            attr: attr_id(id),
            content: vec![
                Block::CodeBlock(CodeBlock {
                    attr: (String::new(), vec!["python".into()], LinkedHashMap::new()),
                    text: "x=1".into(),
                    source_info: si(),
                    attr_source: AttrSourceInfo::empty(),
                }),
                Block::Paragraph(Paragraph {
                    content: vec![Inline::Str(Str {
                        text: caption.into(),
                        source_info: si(),
                    })],
                    source_info: si(),
                }),
            ],
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        })
    }

    fn para(text: &str) -> Block {
        Block::Paragraph(Paragraph {
            content: vec![Inline::Str(Str {
                text: text.into(),
                source_info: si(),
            })],
            source_info: si(),
        })
    }

    /// The shape `SectionizeTransform` produces: a `Div#id.section.levelN`
    /// whose first child is the header with its id moved onto the div (the
    /// header's own id is empty).
    fn section_div(id: &str, level: usize, children: Vec<Block>) -> Block {
        Block::Div(Div {
            attr: (
                id.to_string(),
                vec!["section".to_string(), format!("level{level}")],
                LinkedHashMap::new(),
            ),
            content: children,
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        })
    }

    /// Run sugaring + indexing end-to-end on a block list and return the
    /// resulting (ast, index, diagnostics).
    async fn run(blocks: Vec<Block>) -> (Pandoc, CrossrefIndex, Vec<DiagnosticMessage>) {
        use crate::format::Format;
        use crate::project::{DocumentInfo, ProjectConfig, ProjectContext};
        use crate::render::{BinaryDependencies, RenderContext};
        use std::path::PathBuf;

        let project = ProjectContext {
            dir: PathBuf::from("/project"),
            config: ProjectConfig::default(),
            is_single_file: true,
            files: vec![],
            output_dir: PathBuf::from("/project"),

            ..Default::default()
        };
        let doc = DocumentInfo::from_path("/project/test.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);
        ctx.ref_type_registry = Some(RefTypeRegistry::builtin());
        ctx.crossref_index = Some(CrossrefIndex::new(FileId(0)));

        let mut ast = Pandoc {
            meta: quarto_pandoc_types::ConfigValue::default(),
            blocks,
        };

        FloatRefTargetSugarTransform::new()
            .transform(&mut ast, &mut ctx)
            .await
            .unwrap();
        CrossrefIndexTransform::new()
            .transform(&mut ast, &mut ctx)
            .await
            .unwrap();

        (ast, ctx.crossref_index.unwrap(), ctx.diagnostics)
    }

    /// Like `run`, but with an explicit `ChapterSeed` (book-projects P0:
    /// chapters seed the section counter and mark entries `in_appendix`).
    async fn run_with_seed(
        blocks: Vec<Block>,
        seed: crate::render::ChapterSeed,
    ) -> (Pandoc, CrossrefIndex, Vec<DiagnosticMessage>) {
        use crate::format::Format;
        use crate::project::{DocumentInfo, ProjectConfig, ProjectContext};
        use crate::render::{BinaryDependencies, RenderContext};
        use std::path::PathBuf;

        let project = ProjectContext {
            dir: PathBuf::from("/project"),
            config: ProjectConfig::default(),
            is_single_file: true,
            files: vec![],
            output_dir: PathBuf::from("/project"),

            ..Default::default()
        };
        let doc = DocumentInfo::from_path("/project/test.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);
        ctx.ref_type_registry = Some(RefTypeRegistry::builtin());
        ctx.crossref_index = Some(CrossrefIndex::new(FileId(0)));
        ctx.chapter_seed = Some(seed);

        let mut ast = Pandoc {
            meta: quarto_pandoc_types::ConfigValue::default(),
            blocks,
        };

        FloatRefTargetSugarTransform::new()
            .transform(&mut ast, &mut ctx)
            .await
            .unwrap();
        CrossrefIndexTransform::new()
            .transform(&mut ast, &mut ctx)
            .await
            .unwrap();

        (ast, ctx.crossref_index.unwrap(), ctx.diagnostics)
    }

    /// Like `run`, but with an explicit output format — used to pin the
    /// HTML-only gating of sec-target registration (the pandoc-hybrid
    /// pipeline also runs crossref-index, and registering there would let
    /// resolve consume `@sec-` cites before the vendored Lua sees them).
    async fn run_with_format(
        blocks: Vec<Block>,
        format: crate::format::Format,
    ) -> (Pandoc, CrossrefIndex, Vec<DiagnosticMessage>) {
        run_with_format_and_meta(blocks, format, quarto_pandoc_types::ConfigValue::default()).await
    }

    /// `run_with_format` with explicit document metadata (P0's
    /// number-sections gating tests need both knobs at once).
    async fn run_with_format_and_meta(
        blocks: Vec<Block>,
        format: crate::format::Format,
        meta: quarto_pandoc_types::ConfigValue,
    ) -> (Pandoc, CrossrefIndex, Vec<DiagnosticMessage>) {
        use crate::project::{DocumentInfo, ProjectConfig, ProjectContext};
        use crate::render::{BinaryDependencies, RenderContext};
        use std::path::PathBuf;

        let project = ProjectContext {
            dir: PathBuf::from("/project"),
            config: ProjectConfig::default(),
            is_single_file: true,
            files: vec![],
            output_dir: PathBuf::from("/project"),

            ..Default::default()
        };
        let doc = DocumentInfo::from_path("/project/test.qmd");
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);
        ctx.ref_type_registry = Some(RefTypeRegistry::builtin());
        ctx.crossref_index = Some(CrossrefIndex::new(FileId(0)));

        let mut ast = Pandoc { meta, blocks };

        FloatRefTargetSugarTransform::new()
            .transform(&mut ast, &mut ctx)
            .await
            .unwrap();
        CrossrefIndexTransform::new()
            .transform(&mut ast, &mut ctx)
            .await
            .unwrap();

        (ast, ctx.crossref_index.unwrap(), ctx.diagnostics)
    }

    /// Like `run`, but with explicit document metadata — used to pin the
    /// `crossref.chapters` effect on `max_heading` (book-projects P0).
    async fn run_with_meta(
        blocks: Vec<Block>,
        meta: quarto_pandoc_types::ConfigValue,
    ) -> (Pandoc, CrossrefIndex, Vec<DiagnosticMessage>) {
        use crate::format::Format;
        use crate::project::{DocumentInfo, ProjectConfig, ProjectContext};
        use crate::render::{BinaryDependencies, RenderContext};
        use std::path::PathBuf;

        let project = ProjectContext {
            dir: PathBuf::from("/project"),
            config: ProjectConfig::default(),
            is_single_file: true,
            files: vec![],
            output_dir: PathBuf::from("/project"),

            ..Default::default()
        };
        let doc = DocumentInfo::from_path("/project/test.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);
        ctx.ref_type_registry = Some(RefTypeRegistry::builtin());
        ctx.crossref_index = Some(CrossrefIndex::new(FileId(0)));

        let mut ast = Pandoc { meta, blocks };

        FloatRefTargetSugarTransform::new()
            .transform(&mut ast, &mut ctx)
            .await
            .unwrap();
        CrossrefIndexTransform::new()
            .transform(&mut ast, &mut ctx)
            .await
            .unwrap();

        (ast, ctx.crossref_index.unwrap(), ctx.diagnostics)
    }

    fn chapters_meta(v: bool) -> quarto_pandoc_types::ConfigValue {
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

    // ── P0: headers register as `sec` crossref targets ──────────────
    //
    // sections.lua registers every numbered (non-`.unnumbered`) header
    // whose id classifies as `sec`, regardless of `number-sections` —
    // registration is what makes `@sec-` refs resolve; only the *visible*
    // numbering is gated on the option.

    /// `number-sections: <v>` document metadata (top-level key).
    fn number_sections_meta(v: bool) -> quarto_pandoc_types::ConfigValue {
        use quarto_pandoc_types::{ConfigMapEntry, ConfigValue};
        ConfigValue::new_map(
            vec![ConfigMapEntry {
                key: "number-sections".to_string(),
                key_source: si(),
                value: ConfigValue::new_bool(v, si()),
            }],
            si(),
        )
    }

    #[tokio::test]
    async fn number_attr_stashed_only_with_number_sections() {
        // sections.lua: with `number-sections` on (and level within
        // number-depth), every numbered header carries a `number` kv the
        // HTML writer emits as `data-number`. Off by default.
        let blocks = || vec![header(1, "sec-a", "A"), header(2, "sec-b", "B")];

        let (ast, _idx, _d) = run_with_meta(blocks(), number_sections_meta(true)).await;
        let Block::Header(h1) = &ast.blocks[0] else {
            panic!()
        };
        assert_eq!(h1.attr.2.get("number").map(String::as_str), Some("1"));
        let Block::Header(h2) = &ast.blocks[1] else {
            panic!()
        };
        assert_eq!(h2.attr.2.get("number").map(String::as_str), Some("1.1"));

        let (ast, _idx, _d) = run(blocks()).await;
        let Block::Header(h1) = &ast.blocks[0] else {
            panic!()
        };
        assert!(
            h1.attr.2.get("number").is_none(),
            "no number kv without number-sections"
        );
    }

    #[tokio::test]
    async fn number_attr_skips_unnumbered_and_non_html() {
        // Unnumbered headers never get a number (the early-return precedes
        // the stash, as in sections.lua).
        let (ast, _idx, _d) = run_with_meta(
            vec![
                header_with_class(1, "sec-u", "unnumbered", "U"),
                header(1, "sec-a", "A"),
            ],
            number_sections_meta(true),
        )
        .await;
        let Block::Header(u) = &ast.blocks[0] else {
            panic!()
        };
        assert!(u.attr.2.get("number").is_none());
        let Block::Header(a) = &ast.blocks[1] else {
            panic!()
        };
        assert_eq!(a.attr.2.get("number").map(String::as_str), Some("1"));

        // Pandoc-hybrid formats never get the stash — their numbering is
        // the vendored Lua filters' job (or the writer's).
        let (ast, _idx, _d) = run_with_format_and_meta(
            vec![header(1, "sec-a", "A")],
            crate::format::Format::pdf(),
            number_sections_meta(true),
        )
        .await;
        let Block::Header(h) = &ast.blocks[0] else {
            panic!()
        };
        assert!(
            h.attr.2.get("number").is_none(),
            "number stash is native-HTML-only"
        );
    }

    #[tokio::test]
    async fn headers_register_as_sec_targets() {
        let (_ast, idx, diags) = run(vec![
            header(1, "sec-intro", "Intro"),
            header(2, "sec-deep", "Deep"),
            header(1, "sec-next", "Next"),
            header(1, "introduction", "No prefix"),
        ])
        .await;
        assert!(diags.is_empty(), "diagnostics: {diags:?}");
        let intro = idx.get("sec-intro").expect("sec-intro registered");
        assert_eq!(intro.ref_type, "sec");
        assert_eq!(intro.order.section, vec![1]);
        assert_eq!(intro.order.order, 1);
        let deep = idx.get("sec-deep").expect("sec-deep registered");
        assert_eq!(deep.order.section, vec![1, 1]);
        let next = idx.get("sec-next").expect("sec-next registered");
        assert_eq!(next.order.section, vec![2]);
        assert!(
            idx.get("introduction").is_none(),
            "ids that don't classify as sec are not registered"
        );
        // The header's content inlines become the entry's caption (link
        // text source for cross-file refs, P5).
        let caption = intro.caption.as_ref().expect("caption from header");
        assert!(
            matches!(&caption[0], Inline::Str(s) if s.text == "Intro"),
            "caption: {caption:?}"
        );
    }

    #[tokio::test]
    async fn sectionized_headers_register_with_section_div_ids() {
        // Amendment A: in the real native-HTML pipeline, SectionizeTransform
        // has moved every header id onto its `Div.section` wrapper by the
        // time this transform runs — the header's own id is empty. The
        // walker recovers the id from the innermost enclosing section div,
        // and numbering follows document order across nesting.
        let (ast, idx, diags) = run(vec![
            section_div(
                "sec-intro",
                1,
                vec![
                    header(1, "", "Intro"),
                    para("body"),
                    section_div("sec-deep", 2, vec![header(2, "", "Deep")]),
                ],
            ),
            section_div("sec-next", 1, vec![header(1, "", "Next")]),
        ])
        .await;
        assert!(diags.is_empty(), "diagnostics: {diags:?}");
        assert_eq!(idx.get("sec-intro").unwrap().order.section, vec![1]);
        assert_eq!(idx.get("sec-deep").unwrap().order.section, vec![1, 1]);
        assert_eq!(idx.get("sec-next").unwrap().order.section, vec![2]);
        // The section divs survive sugaring as Divs (never FloatRefTargets).
        assert!(
            matches!(&ast.blocks[0], Block::Div(d) if d.attr.0 == "sec-intro"),
            "section div stays a Div: {:?}",
            ast.blocks[0]
        );
        // The recovered ids also land on the headings records (P4/P5's
        // cross-file link fixup keys off them).
        assert!(
            idx.headings
                .iter()
                .any(|h| h.identifier.as_deref() == Some("sec-deep")),
            "heading recorded with its section div's id: {:?}",
            idx.headings
        );
    }

    #[tokio::test]
    async fn unnumbered_header_skips_numbering_but_is_recorded() {
        // sections.lua's order of operations: indexAddHeading runs
        // unconditionally, THEN the .unnumbered early-return skips the
        // counter advance and sec registration.
        let (_ast, idx, diags) = run(vec![
            header(1, "sec-first", "First"),
            header_with_class(1, "sec-unnum", "unnumbered", "Unnumbered"),
            header(1, "sec-second", "Second"),
        ])
        .await;
        assert!(diags.is_empty(), "diagnostics: {diags:?}");
        assert!(
            idx.get("sec-unnum").is_none(),
            "unnumbered headings are not registered as sec targets"
        );
        assert_eq!(
            idx.get("sec-second").unwrap().order.section,
            vec![2],
            "the unnumbered heading must not advance the section counter"
        );
        assert!(
            idx.headings
                .iter()
                .any(|h| h.identifier.as_deref() == Some("sec-unnum")),
            "unnumbered headings are still recorded in index.headings \
             (cross-file link fixup needs every chapter's heading)"
        );
    }

    #[tokio::test]
    async fn chapter_seed_offsets_section_numbering() {
        // The P4 contract: a chapter rendered standalone starts its
        // counter at its chapter number (seed sections = [n-1]).
        let (_ast, idx, diags) = run_with_seed(
            vec![header(1, "sec-a", "A"), header(2, "sec-b", "B")],
            crate::render::ChapterSeed {
                chapter_number: 2,
                is_appendix: false,
            },
        )
        .await;
        assert!(diags.is_empty(), "diagnostics: {diags:?}");
        assert_eq!(idx.get("sec-a").unwrap().order.section, vec![2]);
        assert_eq!(idx.get("sec-b").unwrap().order.section, vec![2, 1]);
    }

    #[tokio::test]
    async fn appendix_seed_marks_entries_in_appendix() {
        let (_ast, idx, diags) = run_with_seed(
            vec![header(1, "sec-app", "Appendix")],
            crate::render::ChapterSeed {
                chapter_number: 1,
                is_appendix: true,
            },
        )
        .await;
        assert!(diags.is_empty(), "diagnostics: {diags:?}");
        assert!(
            idx.get("sec-app").unwrap().in_appendix,
            "entries registered under an appendix seed are marked in_appendix"
        );
        // Ordinary (unseeded) documents never mark entries in_appendix.
        let (_ast, idx, _) = run(vec![header(1, "sec-plain", "Plain")]).await;
        assert!(!idx.get("sec-plain").unwrap().in_appendix);
    }

    #[tokio::test]
    async fn sec_registration_is_html_only() {
        let (_ast, idx, _diags) =
            run_with_format(vec![header(1, "sec-a", "A")], crate::format::Format::pdf()).await;
        assert!(
            idx.get("sec-a").is_none(),
            "pandoc-hybrid formats keep Lua-native @sec- resolution; \
             crossref-index must not register sec targets for them"
        );
        // The section counter still advances for every format — floats get
        // section-relative numbers from it.
        let (_ast, idx, _diags) = run_with_format(
            vec![header(1, "intro", "Intro"), fig_div("fig-x", "cap")],
            crate::format::Format::pdf(),
        )
        .await;
        assert_eq!(idx.get("fig-x").unwrap().order.section, vec![1]);
    }

    #[tokio::test]
    async fn max_heading_tracks_shallowest_header_level() {
        // Port of Q1's maxHeading (crossref/options.lua +
        // normalize/flags.lua): min(7, min header level), forced to 1 when
        // `crossref.chapters` is set.
        let (_a, idx, _d) = run(vec![header(2, "sec-a", "A"), header(3, "sec-b", "B")]).await;
        assert_eq!(idx.max_heading, 2, "H2-led document");
        let (_a, idx, _d) = run(vec![header(2, "sec-a", "A"), header(1, "sec-b", "B")]).await;
        assert_eq!(idx.max_heading, 1, "a later H1 still lowers the scan");
        let (_a, idx, _d) = run_with_meta(vec![header(2, "sec-a", "A")], chapters_meta(true)).await;
        assert_eq!(idx.max_heading, 1, "chapters forces 1");
        let (_a, idx, _d) = run(vec![para("no headers")]).await;
        assert_eq!(idx.max_heading, 7, "headerless document keeps the cap");
    }

    #[tokio::test]
    async fn assigns_order_to_single_figure() {
        let (ast, idx, diags) = run(vec![fig_div("fig-one", "Cap 1")]).await;
        assert!(diags.is_empty());
        assert_eq!(idx.entries.len(), 1);
        let entry = idx.get("fig-one").unwrap();
        assert_eq!(entry.ref_type, "fig");
        assert_eq!(entry.order.order, 1);
        assert!(entry.order.section.is_empty());

        // And the order is written into the node's plain_data too.
        let Block::Custom(node) = &ast.blocks[0] else {
            panic!();
        };
        assert_eq!(
            node.plain_data.get("order").unwrap().get("order").unwrap(),
            1
        );
    }

    #[tokio::test]
    async fn counts_per_ref_type() {
        let (_, idx, _) = run(vec![
            fig_div("fig-one", "f1"),
            fig_div("fig-two", "f2"),
            // tbl has its own counter — wrap in a tbl-prefix div.
            Block::Div(Div {
                attr: attr_id("tbl-one"),
                content: vec![Block::Paragraph(Paragraph {
                    content: vec![],
                    source_info: si(),
                })],
                source_info: si(),
                attr_source: AttrSourceInfo::empty(),
            }),
        ])
        .await;
        assert_eq!(idx.get("fig-one").unwrap().order.order, 1);
        assert_eq!(idx.get("fig-two").unwrap().order.order, 2);
        assert_eq!(idx.get("tbl-one").unwrap().order.order, 1);
    }

    #[tokio::test]
    async fn captures_section_path() {
        let (_, idx, _) = run(vec![
            header(1, "intro", "Intro"),
            fig_div("fig-in-intro", "cap"),
            header(1, "methods", "Methods"),
            header(2, "details", "Details"),
            fig_div("fig-in-details", "cap"),
        ])
        .await;
        assert_eq!(idx.get("fig-in-intro").unwrap().order.section, vec![1]);
        assert_eq!(idx.get("fig-in-details").unwrap().order.section, vec![2, 1]);
    }

    #[tokio::test]
    async fn duplicate_id_emits_diagnostic_and_keeps_first() {
        let (_, idx, diags) = run(vec![
            fig_div("fig-dup", "first caption"),
            fig_div("fig-dup", "second caption"),
        ])
        .await;
        assert_eq!(diags.len(), 1);
        // Only the first entry is in the index.
        assert_eq!(idx.entries.len(), 1);
    }

    // bd-rr6qzcvu: the duplicate-id diagnostic must be a structured,
    // coded, located diagnostic — not a bare string. It carries `Q-15-1`,
    // a primary location (the duplicate occurrence), and a detail located
    // at the first occurrence, so the renderer underlines both sites.
    #[tokio::test]
    async fn duplicate_id_diagnostic_is_coded_and_located() {
        use quarto_error_reporting::DiagnosticKind;

        let (_, _idx, diags) = run(vec![
            fig_div("fig-dup", "first caption"),
            fig_div("fig-dup", "second caption"),
        ])
        .await;
        assert_eq!(diags.len(), 1);
        let diag = &diags[0];

        assert_eq!(diag.code.as_deref(), Some("Q-15-1"), "must carry the code");
        assert_eq!(diag.kind, DiagnosticKind::Error, "duplicate ids are errors");
        assert!(
            diag.location.is_some(),
            "must have a primary location (the duplicate occurrence)"
        );
        assert!(
            diag.details.iter().any(|d| d.location.is_some()),
            "must point at the first occurrence via a located detail"
        );
        // The offending id is still named, for users reading the bare text.
        let text = diag.to_text(None);
        assert!(
            text.contains("fig-dup"),
            "diagnostic text must name the duplicate id; got: {text}"
        );
    }

    #[test]
    fn section_stack_algo_basic() {
        let mut s = Vec::new();
        advance_sections(&mut s, 1);
        assert_eq!(s, vec![1]);
        advance_sections(&mut s, 1);
        assert_eq!(s, vec![2]);
        advance_sections(&mut s, 2);
        assert_eq!(s, vec![2, 1]);
        advance_sections(&mut s, 3);
        assert_eq!(s, vec![2, 1, 1]);
        advance_sections(&mut s, 3);
        assert_eq!(s, vec![2, 1, 2]);
        advance_sections(&mut s, 2);
        assert_eq!(s, vec![2, 2]);
        advance_sections(&mut s, 1);
        assert_eq!(s, vec![3]);
    }

    #[test]
    fn section_stack_skips_levels() {
        // H1 then H3 without an H2: level-2 entry is implicit-zero, which
        // is... a reader convention, not a crossref correctness concern.
        // We mirror the classic behavior: pad with zeros, then increment.
        let mut s = Vec::new();
        advance_sections(&mut s, 1);
        advance_sections(&mut s, 3);
        assert_eq!(s, vec![1, 0, 1]);
    }

    #[tokio::test]
    async fn caption_inlines_recorded_from_plain_caption() {
        // bd-n3sark9b: a Plain-first caption_long (what `![cap](img){#fig-x}`
        // and Table captions produce) must still populate the index
        // entry's caption, not leave it `None`.
        let mut node = CustomNode::new("FloatRefTarget", attr_id("fig-plain"), si());
        node.plain_data = serde_json::json!({
            "ref_type": "fig",
            "kind": "Figure",
            "identifier": "fig-plain",
        });
        node.slots.insert("content".into(), Slot::Blocks(vec![]));
        node.slots.insert(
            "caption_long".into(),
            Slot::Blocks(vec![Block::Plain(quarto_pandoc_types::block::Plain {
                content: vec![Inline::Str(Str {
                    text: "Plain caption".into(),
                    source_info: si(),
                })],
                source_info: si(),
            })]),
        );
        let caption = extract_caption_inlines(&node).expect("caption inlines from Plain");
        assert_eq!(caption.len(), 1);
        match &caption[0] {
            Inline::Str(s) => assert_eq!(s.text, "Plain caption"),
            other => panic!("unexpected inline {other:?}"),
        }
    }

    #[tokio::test]
    async fn caption_inlines_recorded_on_entry() {
        let (_, idx, _) = run(vec![fig_div("fig-caption", "Hello caption")]).await;
        let entry = idx.get("fig-caption").unwrap();
        let caption = entry.caption.as_ref().expect("caption inlines recorded");
        assert_eq!(caption.len(), 1);
        match &caption[0] {
            Inline::Str(s) => assert_eq!(s.text, "Hello caption"),
            other => panic!("expected Str, got {:?}", other),
        }
    }

    /// Run equation label sugaring + float sugaring + indexing.
    async fn run_with_equations(
        blocks: Vec<Block>,
    ) -> (Pandoc, CrossrefIndex, Vec<DiagnosticMessage>) {
        use crate::format::Format;
        use crate::project::{DocumentInfo, ProjectConfig, ProjectContext};
        use crate::render::{BinaryDependencies, RenderContext};
        use crate::transforms::EquationLabelTransform;
        use std::path::PathBuf;

        let project = ProjectContext {
            dir: PathBuf::from("/project"),
            config: ProjectConfig::default(),
            is_single_file: true,
            files: vec![],
            output_dir: PathBuf::from("/project"),

            ..Default::default()
        };
        let doc = DocumentInfo::from_path("/project/test.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);
        ctx.ref_type_registry = Some(RefTypeRegistry::builtin());
        ctx.crossref_index = Some(CrossrefIndex::new(FileId(0)));

        let mut ast = Pandoc {
            meta: quarto_pandoc_types::ConfigValue::default(),
            blocks,
        };

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

        (ast, ctx.crossref_index.unwrap(), ctx.diagnostics)
    }

    fn eq_para(id: &str, math_text: &str) -> Block {
        use quarto_pandoc_types::inline::{Math, MathType, Span};
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

    #[tokio::test]
    async fn indexes_labelled_equation() {
        let (_, idx, diags) = run_with_equations(vec![eq_para("eq-einstein", "e = mc^2")]).await;
        assert!(diags.is_empty(), "no diagnostics: {:?}", diags);
        assert_eq!(idx.entries.len(), 1);
        let entry = idx.get("eq-einstein").unwrap();
        assert_eq!(entry.ref_type, "eq");
        assert_eq!(entry.order.order, 1);
    }

    #[tokio::test]
    async fn equation_counter_independent_from_figures() {
        let (_, idx, _) = run_with_equations(vec![
            fig_div("fig-one", "f1"),
            eq_para("eq-first", "x^2"),
            fig_div("fig-two", "f2"),
            eq_para("eq-second", "y^2"),
        ])
        .await;
        assert_eq!(idx.get("fig-one").unwrap().order.order, 1);
        assert_eq!(idx.get("fig-two").unwrap().order.order, 2);
        assert_eq!(idx.get("eq-first").unwrap().order.order, 1);
        assert_eq!(idx.get("eq-second").unwrap().order.order, 2);
    }

    #[tokio::test]
    async fn equation_captures_section_path() {
        let (_, idx, _) = run_with_equations(vec![
            header(1, "sec1", "Section 1"),
            eq_para("eq-a", "a"),
            header(2, "sec1-1", "Section 1.1"),
            eq_para("eq-b", "b"),
        ])
        .await;
        assert_eq!(idx.get("eq-a").unwrap().order.section, vec![1]);
        assert_eq!(idx.get("eq-b").unwrap().order.section, vec![1, 1]);
    }
}
