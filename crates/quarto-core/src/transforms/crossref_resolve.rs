/*
 * transforms/crossref_resolve.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Resolve @-references classified as crossrefs into
 * CustomNode("CrossrefResolvedRef").
 */

//! Resolve crossref references in the AST.
//!
//! This transform runs in the **crossref phase** (see plan D3), after the
//! [`CrossrefIndexTransform`](super::CrossrefIndexTransform) has populated
//! the index. It walks all inlines and rewrites `Cite` nodes whose first
//! citation id is classified as a crossref (per the
//! [`RefTypeRegistry`](crate::crossref::RefTypeRegistry)) into the
//! canonical `CustomNode("CrossrefResolvedRef")` inline shape (see plan
//! O4). Back-end renderers convert that custom node into a format-specific
//! link (`<a href=..>` for HTML, `\ref{..}` for LaTeX, etc.).
//!
//! ## Classification rules
//!
//! Per plan D5 / D7, a `Cite` is a crossref iff its **first** citation id
//! resolves via `RefTypeRegistry::classify_cite_id`. This means:
//!
//! - `@fig-foo` → crossref.
//! - `@smith2020` → citation (no ref-type prefix; citeproc's problem).
//! - `@fig-foo; @bar2020` → mixed: we conservatively leave the whole
//!   `Cite` untouched and record a diagnostic so the user knows the
//!   crossref wasn't resolved. Q1 has the same limitation — crossrefs
//!   aren't intermixed with citations in the same bracket.
//! - `@Fig-foo` → crossref, same as `@fig-foo` (the id is lowercased
//!   before the registry lookup — see [`classify_cite`]); the original
//!   case is preserved separately as `plain_data.label_upper` (Q1's
//!   `refs.lua:31` capitalization signal). The crossref *index*, however,
//!   is keyed by the target's authored (lowercase) identifier, so an
//!   uppercase-led reference to a lowercase target still comes back
//!   unresolved — see [`classify_cite`]'s doc comment.
//!
//! ## Unresolved refs
//!
//! When a `Cite` is *classified* as a crossref but the id is not in the
//! index (user typo, or realized id from `output: asis` that wasn't
//! promised via `crossref.ids`), we emit a diagnostic and produce an
//! **unresolved** CrossrefResolvedRef node so back-end renderers can emit
//! a visible placeholder rather than dropping the reference silently.

use quarto_analysis::AnalysisContext;
use quarto_error_reporting::DiagnosticMessage;
use quarto_pandoc_types::block::{Block, Blocks};
use quarto_pandoc_types::custom::{CustomNode, Slot};
use quarto_pandoc_types::inline::{CitationMode, Cite, Inline, Inlines};
use quarto_pandoc_types::pandoc::Pandoc;
use serde_json::json;

use crate::Result;
use crate::crossref::{
    CROSSREF_RESOLVED_REF, CrossrefIndex, RefTypeDef, RefTypeRegistry, RefTypeSource,
};
use crate::render::{ChapterSeed, RenderContext};
use crate::transform::{AstTransform, TransformPhase};
use crate::transforms::crossref_render::format_crossref_number;

/// Transform that resolves crossref `Cite`s into
/// `CustomNode("CrossrefResolvedRef")` inlines.
pub struct CrossrefResolveTransform;

impl CrossrefResolveTransform {
    pub fn new() -> Self {
        Self
    }
}

impl Default for CrossrefResolveTransform {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait(?Send)]
impl AstTransform for CrossrefResolveTransform {
    fn name(&self) -> &str {
        "crossref-resolve"
    }

    fn phase(&self) -> TransformPhase {
        TransformPhase::Crossref
    }

    async fn transform(&self, ast: &mut Pandoc, ctx: &mut RenderContext) -> Result<()> {
        // If the registry wasn't set up, we can't classify — no-op.
        let Some(registry) = ctx.ref_type_registry.clone() else {
            return Ok(());
        };
        let index = ctx.crossref_index.clone();
        let mut diags = Vec::new();
        resolve_blocks(
            &mut ast.blocks,
            &registry,
            index.as_ref(),
            ctx.chapter_seed.as_ref(),
            &mut diags,
        );
        for d in diags {
            ctx.add_diagnostic(d);
        }
        Ok(())
    }
}

fn resolve_blocks(
    blocks: &mut Blocks,
    reg: &RefTypeRegistry,
    index: Option<&CrossrefIndex>,
    chapter_seed: Option<&ChapterSeed>,
    diags: &mut Vec<DiagnosticMessage>,
) {
    for block in blocks.iter_mut() {
        resolve_block(block, reg, index, chapter_seed, diags);
    }
}

fn resolve_block(
    block: &mut Block,
    reg: &RefTypeRegistry,
    index: Option<&CrossrefIndex>,
    chapter_seed: Option<&ChapterSeed>,
    diags: &mut Vec<DiagnosticMessage>,
) {
    match block {
        Block::Plain(p) => resolve_inlines(&mut p.content, reg, index, chapter_seed, diags),
        Block::Paragraph(p) => resolve_inlines(&mut p.content, reg, index, chapter_seed, diags),
        Block::LineBlock(lb) => {
            for line in &mut lb.content {
                resolve_inlines(line, reg, index, chapter_seed, diags);
            }
        }
        Block::Header(h) => resolve_inlines(&mut h.content, reg, index, chapter_seed, diags),
        Block::BlockQuote(bq) => resolve_blocks(&mut bq.content, reg, index, chapter_seed, diags),
        Block::OrderedList(ol) => {
            for item in &mut ol.content {
                resolve_blocks(item, reg, index, chapter_seed, diags);
            }
        }
        Block::BulletList(bl) => {
            for item in &mut bl.content {
                resolve_blocks(item, reg, index, chapter_seed, diags);
            }
        }
        Block::DefinitionList(dl) => {
            for (term, defs) in &mut dl.content {
                resolve_inlines(term, reg, index, chapter_seed, diags);
                for def in defs {
                    resolve_blocks(def, reg, index, chapter_seed, diags);
                }
            }
        }
        Block::Figure(fig) => {
            // Caption inlines too.
            if let Some(long) = fig.caption.long.as_mut() {
                resolve_blocks(long, reg, index, chapter_seed, diags);
            }
            if let Some(short) = fig.caption.short.as_mut() {
                resolve_inlines(short, reg, index, chapter_seed, diags);
            }
            resolve_blocks(&mut fig.content, reg, index, chapter_seed, diags);
        }
        Block::Div(div) => resolve_blocks(&mut div.content, reg, index, chapter_seed, diags),
        Block::Custom(node) => {
            for (_name, slot) in node.slots.iter_mut() {
                match slot {
                    Slot::Block(b) => resolve_block(b, reg, index, chapter_seed, diags),
                    Slot::Blocks(bs) => resolve_blocks(bs, reg, index, chapter_seed, diags),
                    Slot::Inline(i) => resolve_inline(i, reg, index, chapter_seed, diags),
                    Slot::Inlines(is) => resolve_inlines(is, reg, index, chapter_seed, diags),
                }
            }
        }
        Block::Table(_)
        | Block::CodeBlock(_)
        | Block::RawBlock(_)
        | Block::HorizontalRule(_)
        | Block::BlockMetadata(_)
        | Block::NoteDefinitionPara(_)
        | Block::NoteDefinitionFencedBlock(_)
        | Block::CaptionBlock(_) => {}
    }
}

fn resolve_inlines(
    inlines: &mut Inlines,
    reg: &RefTypeRegistry,
    index: Option<&CrossrefIndex>,
    chapter_seed: Option<&ChapterSeed>,
    diags: &mut Vec<DiagnosticMessage>,
) {
    for inline in inlines.iter_mut() {
        resolve_inline(inline, reg, index, chapter_seed, diags);
    }
}

fn resolve_inline(
    inline: &mut Inline,
    reg: &RefTypeRegistry,
    index: Option<&CrossrefIndex>,
    chapter_seed: Option<&ChapterSeed>,
    diags: &mut Vec<DiagnosticMessage>,
) {
    // Recurse into children of container inlines first so nested
    // references also get resolved (e.g. a `@fig-..` inside an Emph).
    match inline {
        Inline::Emph(e) => resolve_inlines(&mut e.content, reg, index, chapter_seed, diags),
        Inline::Underline(u) => resolve_inlines(&mut u.content, reg, index, chapter_seed, diags),
        Inline::Strong(s) => resolve_inlines(&mut s.content, reg, index, chapter_seed, diags),
        Inline::Strikeout(s) => resolve_inlines(&mut s.content, reg, index, chapter_seed, diags),
        Inline::Superscript(s) => resolve_inlines(&mut s.content, reg, index, chapter_seed, diags),
        Inline::Subscript(s) => resolve_inlines(&mut s.content, reg, index, chapter_seed, diags),
        Inline::SmallCaps(s) => resolve_inlines(&mut s.content, reg, index, chapter_seed, diags),
        Inline::Quoted(q) => resolve_inlines(&mut q.content, reg, index, chapter_seed, diags),
        Inline::Link(l) => resolve_inlines(&mut l.content, reg, index, chapter_seed, diags),
        Inline::Image(i) => resolve_inlines(&mut i.content, reg, index, chapter_seed, diags),
        Inline::Note(n) => resolve_blocks(&mut n.content, reg, index, chapter_seed, diags),
        Inline::Span(s) => resolve_inlines(&mut s.content, reg, index, chapter_seed, diags),
        Inline::Insert(i) => resolve_inlines(&mut i.content, reg, index, chapter_seed, diags),
        Inline::Delete(d) => resolve_inlines(&mut d.content, reg, index, chapter_seed, diags),
        Inline::Highlight(h) => resolve_inlines(&mut h.content, reg, index, chapter_seed, diags),
        Inline::Custom(node) => {
            for (_name, slot) in node.slots.iter_mut() {
                match slot {
                    Slot::Block(b) => resolve_block(b, reg, index, chapter_seed, diags),
                    Slot::Blocks(bs) => resolve_blocks(bs, reg, index, chapter_seed, diags),
                    Slot::Inline(i) => resolve_inline(i, reg, index, chapter_seed, diags),
                    Slot::Inlines(is) => resolve_inlines(is, reg, index, chapter_seed, diags),
                }
            }
        }
        _ => {}
    }

    // Now check this inline itself: is it a crossref Cite?
    if let Inline::Cite(cite) = inline
        && let Some(replacement) = classify_cite(cite, reg, index, chapter_seed, diags)
    {
        *inline = Inline::Custom(replacement);
    }
}

/// If `cite`'s first citation is classified as a crossref, produce the
/// replacement custom node. Emits diagnostics for mixed-citation bundles
/// and unresolved crossrefs.
///
/// Classification lowercases the id before the registry lookup (so
/// `@Fig-alpha` and `@fig-alpha` classify identically), but the index
/// lookup below still uses the *raw* id. Targets are indexed under their
/// authored (lowercase) identifier, so an uppercase-led reference against
/// a lowercase-authored target classifies as a crossref yet still comes
/// back `resolved: false` — `build_resolved_ref` is called regardless, so
/// the unresolved node still carries the correct `label_upper`.
fn classify_cite(
    cite: &Cite,
    reg: &RefTypeRegistry,
    index: Option<&CrossrefIndex>,
    chapter_seed: Option<&ChapterSeed>,
    diags: &mut Vec<DiagnosticMessage>,
) -> Option<CustomNode> {
    let first = cite.citations.first()?;
    // The registry's ref-type keys are registered lowercase; lowercase the
    // id before classifying so `@Fig-alpha` matches the same `"fig"` entry
    // as `@fig-alpha` (Q1's refs.lua treats the leading-uppercase form as
    // the *same* crossref, just with a capitalized label — see
    // `label_upper` below). `classify_cite_id` itself stays case-sensitive;
    // this lowercasing is local to Cite classification, not a general
    // change to the registry.
    let def = reg.classify_cite_id(&first.id.to_lowercase())?;

    // If there are multiple citations and any of them *aren't* classified
    // as the same ref-type (i.e., look like a bibliographic citation),
    // conservatively bail so citeproc gets a chance at the bibliographic
    // ones. Emit a diagnostic so the user understands why `@fig-..` mixed
    // with `@smith2020` didn't get resolved.
    if cite.citations.len() > 1 {
        let all_same_kind = cite
            .citations
            .iter()
            .skip(1)
            .all(|c| reg.classify_cite_id(&c.id.to_lowercase()).is_some());
        if !all_same_kind {
            diags.push(DiagnosticMessage::warning(format!(
                "crossref `@{id}` appears in a `Cite` with bibliographic citations; \
                 split the references into separate brackets to resolve the crossref.",
                id = first.id
            )));
            return None;
        }

        // All citations classify as the same ref-type (e.g. `[@fig-a; @fig-b]`).
        // We currently resolve only the first and silently drop the rest
        // (Phase 1 scope: single-id crossref cites; multi-crossref ranges
        // like "Figures 1-3" are deferred). Flag the drop so it's
        // diagnosable rather than silent.
        let dropped: Vec<&str> = cite.citations[1..].iter().map(|c| c.id.as_str()).collect();
        diags.push(DiagnosticMessage::warning(format!(
            "crossref `@{first}` is part of a multi-id citation `[@{first}; ...]`; \
             only the first id is resolved and the rest ({dropped}) are dropped. \
             Use separate `@{first}` references if you need to cite each one.",
            first = first.id,
            dropped = dropped.join(", @")
        )));
    }

    // Ensure the resolved reference carries the empty suffix expected
    // (we keep the suffix if present as a literal hint; renderers might
    // emit it).
    let (resolved, entry_kind_override) = match index.and_then(|idx| idx.get(&first.id)) {
        Some(entry) => (true, Some(entry)),
        None => (false, None),
    };

    if !resolved {
        diags.push(DiagnosticMessage::warning(format!(
            "unresolved crossref `@{id}`: no target with this identifier was found.",
            id = first.id,
        )));
    }

    Some(build_resolved_ref(
        &first.id,
        def,
        resolved,
        entry_kind_override,
        chapter_seed,
        cite,
    ))
}

/// Construct the canonical `CustomNode("CrossrefResolvedRef")` inline.
///
/// When `entry` resolves locally and `chapter_seed` is present, also
/// composes `resolved_number` right here (book-projects P8) — reusing
/// [`format_crossref_number`], the same helper `CrossrefRenderTransform`
/// calls at Finalization. That later, real-render composition is
/// unaffected: it already prefers a pre-set `resolved_number` over
/// recomputing one (that's how it already consumes P5's cross-chapter
/// patches), and for a local resolution the seed is identical either way,
/// so the final rendered number is unchanged. This exists because
/// `q2 preview` excludes `CrossrefRenderTransform` (it needs the
/// still-structured node, not flattened HTML) but always runs this
/// Crossref-phase transform — without this, a chapter-seeded preview
/// would show a flat, unscoped number for every non-`sec` crossref kind.
/// `sec` targets are untouched: their raw `order.section` is already
/// chapter-scoped by `CrossrefIndexTransform`'s seed offset (P0), so no
/// extra composition is needed or performed here.
fn build_resolved_ref(
    identifier: &str,
    def: &RefTypeDef,
    resolved: bool,
    entry: Option<&crate::crossref::CrossrefEntry>,
    chapter_seed: Option<&ChapterSeed>,
    original: &Cite,
) -> CustomNode {
    use hashlink::LinkedHashMap;
    let attr = (String::new(), Vec::new(), LinkedHashMap::new());
    let mut node = CustomNode::new(CROSSREF_RESOLVED_REF, attr, original.source_info.clone());
    let mut data = serde_json::Map::new();
    data.insert("identifier".into(), json!(identifier));
    data.insert("ref_type".into(), json!(def.ref_type));
    data.insert("kind".into(), json!(def.kind));
    data.insert("resolved".into(), json!(resolved));
    // Flag whether this id was *registered* only through the promised
    // mechanism (i.e. the category wasn't declared via crossref.custom).
    // The indexer still emits a diagnostic; we include the flag so
    // renderers can distinguish "authored but undeclared" from "normal".
    data.insert(
        "kind_source".into(),
        json!(match def.source {
            RefTypeSource::BuiltIn => "builtin",
            RefTypeSource::CustomFromMetadata => "custom",
            RefTypeSource::Promised => "promised",
        }),
    );
    // Q1's `refs.lua:56` branches on `cite.mode ~= pandoc.SuppressAuthor`;
    // carry the mode through so renderers can make the same distinction
    // (e.g. suppressing the "Figure"/"Table" label text).
    data.insert(
        "cite_mode".into(),
        json!(match original.citations[0].mode {
            CitationMode::AuthorInText => "author_in_text",
            CitationMode::SuppressAuthor => "suppress_author",
            CitationMode::NormalCitation => "normal_citation",
        }),
    );
    // Q1's `refs.lua:31`: `not not string.match(cite.id, "^[A-Z]")` — true
    // iff the *original* (non-lowercased) id starts with an ASCII
    // uppercase letter, independent of the lowercased id used above only
    // for the registry lookup.
    data.insert(
        "label_upper".into(),
        json!(
            identifier
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_uppercase())
        ),
    );
    if let Some(e) = entry {
        data.insert(
            "order".into(),
            json!({ "section": e.order.section, "order": e.order.order }),
        );
        // The render side needs the per-file appendix flag for `sec`
        // presentation (letter-formatted numbers, "Appendix" prefix) —
        // book-projects P0.
        data.insert("in_appendix".into(), json!(e.in_appendix));
        // book-projects P8: compose the chapter-scoped display number now,
        // for `q2 preview`'s benefit (see this function's doc comment).
        // `sec` numbers need no composition — the section path is already
        // chapter-scoped by CrossrefIndexTransform's seed offset.
        if e.ref_type != "sec"
            && let Some(seed) = chapter_seed
        {
            data.insert(
                "resolved_number".into(),
                json!(format_crossref_number(e.order.order, Some(seed))),
            );
        }
    }
    node.plain_data = serde_json::Value::Object(data);

    // Keep the original Cite's suffix as a slot so renderers can carry it
    // over — e.g. `[@fig-foo, p. 12]` often carries a page hint.
    if !original.citations[0].suffix.is_empty() {
        node.slots.insert(
            "suffix".into(),
            Slot::Inlines(original.citations[0].suffix.clone()),
        );
    }
    // Likewise for the prefix — e.g. `[see @fig-foo]` — which must be a
    // slot (Inlines) rather than a `plain_data` field: `plain_data` is
    // contractually AST-free (`quarto-pandoc-types/src/custom.rs`), and
    // stringifying inline markup here would silently drop it.
    if !original.citations[0].prefix.is_empty() {
        node.slots.insert(
            "cite_prefix".into(),
            Slot::Inlines(original.citations[0].prefix.clone()),
        );
    }
    node
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crossref::{CrossrefEntry, CrossrefIndex, Order, RefTypeRegistry};
    use hashlink::LinkedHashMap;
    use quarto_error_reporting::DiagnosticKind;
    use quarto_pandoc_types::attr::{Attr, AttrSourceInfo};
    use quarto_pandoc_types::block::{Block, Paragraph};
    use quarto_pandoc_types::inline::{Citation, CitationMode, Str};
    use quarto_source_map::{FileId, SourceInfo};

    fn si() -> SourceInfo {
        SourceInfo::original(FileId(0), 0, 0)
    }

    fn _attr_id(id: &str) -> Attr {
        (id.to_string(), Vec::new(), LinkedHashMap::new())
    }

    fn make_cite(id: &str) -> Inline {
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
            content: vec![Inline::Str(Str {
                text: format!("@{}", id),
                source_info: si(),
            })],
            source_info: si(),
        })
    }

    fn make_multi_cite(ids: &[&str]) -> Inline {
        let citations = ids
            .iter()
            .map(|id| Citation {
                id: (*id).to_string(),
                prefix: vec![],
                suffix: vec![],
                mode: CitationMode::NormalCitation,
                note_num: 0,
                hash: 0,
                id_source: None,
            })
            .collect();
        Inline::Cite(Cite {
            citations,
            content: vec![],
            source_info: si(),
        })
    }

    fn make_index_with(ids: &[(&str, &str)]) -> CrossrefIndex {
        let mut idx = CrossrefIndex::new(FileId(0));
        for (i, (id, ref_type)) in ids.iter().enumerate() {
            idx.insert(CrossrefEntry {
                identifier: (*id).to_string(),
                ref_type: (*ref_type).to_string(),
                parent: None,
                order: Order {
                    section: vec![],
                    order: (i + 1) as u32,
                },
                caption: None,
                in_appendix: false,
                source_info: si(),
            });
        }
        idx
    }

    fn resolve(
        inline: &mut Inline,
        reg: &RefTypeRegistry,
        index: Option<&CrossrefIndex>,
    ) -> Vec<DiagnosticMessage> {
        let mut diags = Vec::new();
        resolve_inline(inline, reg, index, None, &mut diags);
        diags
    }

    fn resolve_with_seed(
        inline: &mut Inline,
        reg: &RefTypeRegistry,
        index: Option<&CrossrefIndex>,
        chapter_seed: Option<&ChapterSeed>,
    ) -> Vec<DiagnosticMessage> {
        let mut diags = Vec::new();
        resolve_inline(inline, reg, index, chapter_seed, &mut diags);
        diags
    }

    fn plain_data(inline: &Inline) -> &serde_json::Value {
        let Inline::Custom(node) = inline else {
            panic!("expected a resolved CustomNode, got {inline:?}");
        };
        &node.plain_data
    }

    #[test]
    fn resolves_known_crossref() {
        let reg = RefTypeRegistry::builtin();
        let idx = make_index_with(&[("fig-foo", "fig")]);
        let mut inline = make_cite("fig-foo");
        let diags = resolve(&mut inline, &reg, Some(&idx));
        assert!(diags.is_empty());
        let Inline::Custom(node) = inline else {
            panic!("expected resolved CustomNode");
        };
        assert_eq!(node.type_name, CROSSREF_RESOLVED_REF);
        assert_eq!(node.plain_data["identifier"], "fig-foo");
        assert_eq!(node.plain_data["ref_type"], "fig");
        assert_eq!(node.plain_data["kind"], "Figure");
        assert_eq!(node.plain_data["resolved"], true);
        assert_eq!(node.plain_data["order"]["order"], 1);
    }

    #[test]
    fn in_appendix_flows_into_plain_data() {
        // Book-projects P0: the render side needs the entry's appendix flag
        // (letter-formatted numbers, "Appendix" prefix) — resolve passes it
        // through `plain_data` (plain_data is contractually AST-free, so a
        // bool field is the channel).
        let reg = RefTypeRegistry::builtin();
        let mut idx = CrossrefIndex::new(FileId(0));
        idx.insert(CrossrefEntry {
            identifier: "sec-app".to_string(),
            ref_type: "sec".to_string(),
            parent: None,
            order: Order {
                section: vec![1],
                order: 1,
            },
            caption: None,
            in_appendix: true,
            source_info: si(),
        });
        let mut inline = make_cite("sec-app");
        let diags = resolve(&mut inline, &reg, Some(&idx));
        assert!(diags.is_empty());
        let Inline::Custom(node) = inline else {
            panic!("expected resolved CustomNode");
        };
        assert_eq!(node.plain_data["in_appendix"], true);
    }

    #[test]
    fn unknown_crossref_emits_diagnostic_and_placeholder() {
        let reg = RefTypeRegistry::builtin();
        let idx = CrossrefIndex::new(FileId(0));
        let mut inline = make_cite("fig-missing");
        let diags = resolve(&mut inline, &reg, Some(&idx));
        assert_eq!(diags.len(), 1);
        let Inline::Custom(node) = inline else {
            panic!("should still produce a placeholder");
        };
        assert_eq!(node.plain_data["resolved"], false);
        assert!(node.plain_data.get("order").is_none());
    }

    #[test]
    fn bibliographic_cite_left_alone() {
        let reg = RefTypeRegistry::builtin();
        let idx = CrossrefIndex::new(FileId(0));
        let mut inline = make_cite("smith2020");
        let diags = resolve(&mut inline, &reg, Some(&idx));
        assert!(diags.is_empty(), "no warnings — citeproc's problem");
        assert!(matches!(inline, Inline::Cite(_)));
    }

    #[test]
    fn citation_with_hyphen_left_alone_when_prefix_not_registered() {
        // `@smith-2020` — splits to prefix "smith", not registered.
        let reg = RefTypeRegistry::builtin();
        let idx = CrossrefIndex::new(FileId(0));
        let mut inline = make_cite("smith-2020");
        let diags = resolve(&mut inline, &reg, Some(&idx));
        assert!(diags.is_empty());
        assert!(matches!(inline, Inline::Cite(_)));
    }

    #[test]
    fn mixed_cite_emits_diagnostic_and_leaves_alone() {
        let reg = RefTypeRegistry::builtin();
        let idx = make_index_with(&[("fig-foo", "fig")]);
        let mut inline = make_multi_cite(&["fig-foo", "smith2020"]);
        let diags = resolve(&mut inline, &reg, Some(&idx));
        assert_eq!(diags.len(), 1);
        // Cite is unchanged.
        assert!(matches!(inline, Inline::Cite(_)));
    }

    #[test]
    fn multi_crossref_cite_resolved_to_first() {
        // `@fig-a; @fig-b` — both crossrefs. We currently resolve to the
        // first; the second is dropped. (Phase 1 scope: single-id
        // crossref cites. Multi-crossref ranges like "Figures 1-3" are
        // deferred.) Flagged, not fixed (bd-h1ub8f8z's sibling triage,
        // P7 Task 8): the drop is diagnosed rather than silent.
        let reg = RefTypeRegistry::builtin();
        let idx = make_index_with(&[("fig-a", "fig"), ("fig-b", "fig")]);
        let mut inline = make_multi_cite(&["fig-a", "fig-b"]);
        let diags = resolve(&mut inline, &reg, Some(&idx));
        assert_eq!(diags.len(), 1);
        assert!(diags[0].title.contains("fig-a"));
        assert!(diags[0].title.contains("fig-b"));
        let Inline::Custom(node) = inline else {
            panic!();
        };
        assert_eq!(node.plain_data["identifier"], "fig-a");
    }

    #[test]
    fn test_multi_crossref_drop_is_diagnosed() {
        // T8.1 (flag branch): `[@fig-a; @fig-b; @fig-c]`, all same ref-type.
        // Exactly one diagnostic is emitted, and it names every dropped id.
        let reg = RefTypeRegistry::builtin();
        let idx = make_index_with(&[("fig-a", "fig"), ("fig-b", "fig"), ("fig-c", "fig")]);
        let mut inline = make_multi_cite(&["fig-a", "fig-b", "fig-c"]);
        let diags = resolve(&mut inline, &reg, Some(&idx));
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].kind, DiagnosticKind::Warning);
        assert!(diags[0].title.contains("fig-a"));
        assert!(diags[0].title.contains("fig-b"));
        assert!(diags[0].title.contains("fig-c"));
    }

    #[test]
    fn resolve_walks_into_paragraph() {
        let reg = RefTypeRegistry::builtin();
        let idx = make_index_with(&[("fig-foo", "fig")]);
        let mut block = Block::Paragraph(Paragraph {
            content: vec![
                Inline::Str(Str {
                    text: "see ".into(),
                    source_info: si(),
                }),
                make_cite("fig-foo"),
            ],
            source_info: si(),
        });
        let mut diags = Vec::new();
        resolve_block(&mut block, &reg, Some(&idx), None, &mut diags);
        assert!(diags.is_empty());
        let Block::Paragraph(p) = block else { panic!() };
        assert!(matches!(p.content[1], Inline::Custom(_)));
    }

    #[test]
    fn local_resolution_composes_chapter_scoped_number_when_seeded() {
        // book-projects P8: a local (same-chapter) figure resolution with a
        // ChapterSeed present must get `resolved_number` composed right
        // here, at Crossref-phase resolution time — q2 preview never runs
        // the Finalization-phase transform that would otherwise do this.
        let reg = RefTypeRegistry::builtin();
        let idx = make_index_with(&[("fig-foo", "fig")]);
        let mut inline = make_cite("fig-foo");
        let seed = ChapterSeed {
            chapter_number: 2,
            is_appendix: false,
        };
        let diags = resolve_with_seed(&mut inline, &reg, Some(&idx), Some(&seed));
        assert!(diags.is_empty());
        assert_eq!(plain_data(&inline)["resolved_number"], "2.1");
    }

    #[test]
    fn local_sec_resolution_gets_no_composed_number() {
        // `sec` targets need no composition: CrossrefIndexTransform already
        // offsets the raw section path by the seed (book-projects P0), so
        // the render side reads `order.section` directly. Composing a
        // `resolved_number` here too would be redundant, not just useless
        // (the display code takes a different branch entirely for `sec`).
        let reg = RefTypeRegistry::builtin();
        let idx = make_index_with(&[("sec-intro", "sec")]);
        let mut inline = make_cite("sec-intro");
        let seed = ChapterSeed {
            chapter_number: 2,
            is_appendix: false,
        };
        let diags = resolve_with_seed(&mut inline, &reg, Some(&idx), Some(&seed));
        assert!(diags.is_empty());
        assert!(
            plain_data(&inline).get("resolved_number").is_none(),
            "sec resolution must not gain a composed number: {:?}",
            plain_data(&inline)
        );
    }

    #[test]
    fn local_resolution_without_seed_gets_no_composed_number() {
        // Non-book (or single-document) renders pass no ChapterSeed at
        // all — this pins today's exact behavior for that case, so the
        // new composition never fires for a document that was never
        // chapter-seeded.
        let reg = RefTypeRegistry::builtin();
        let idx = make_index_with(&[("fig-foo", "fig")]);
        let mut inline = make_cite("fig-foo");
        let diags = resolve(&mut inline, &reg, Some(&idx));
        assert!(diags.is_empty());
        assert!(
            plain_data(&inline).get("resolved_number").is_none(),
            "an unseeded resolution must not gain a composed number: {:?}",
            plain_data(&inline)
        );
    }

    #[tokio::test]
    async fn missing_registry_is_noop() {
        // If ctx.ref_type_registry is None, transform returns Ok with no
        // changes.
        let cite = make_cite("fig-foo");
        let ast = Pandoc {
            meta: quarto_pandoc_types::ConfigValue::default(),
            blocks: vec![Block::Paragraph(Paragraph {
                content: vec![cite.clone()],
                source_info: si(),
            })],
        };

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
        // ctx.ref_type_registry stays None.
        let mut ast = ast;
        CrossrefResolveTransform::new()
            .transform(&mut ast, &mut ctx)
            .await
            .unwrap();
        let Block::Paragraph(p) = &ast.blocks[0] else {
            panic!();
        };
        // Cite is still a Cite.
        assert!(matches!(p.content[0], Inline::Cite(_)));
    }

    // silence unused-variable warning for `_` placeholder
    #[allow(dead_code)]
    fn _touch_attr(_: AttrSourceInfo, _: Attr) {}
}
