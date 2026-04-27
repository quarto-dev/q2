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
use quarto_pandoc_types::inline::{Cite, Inline, Inlines};
use quarto_pandoc_types::pandoc::Pandoc;
use serde_json::json;

use crate::Result;
use crate::crossref::{
    CROSSREF_RESOLVED_REF, CrossrefIndex, RefTypeDef, RefTypeRegistry, RefTypeSource,
};
use crate::render::RenderContext;
use crate::transform::AstTransform;

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

    async fn transform(&self, ast: &mut Pandoc, ctx: &mut RenderContext) -> Result<()> {
        // If the registry wasn't set up, we can't classify — no-op.
        let Some(registry) = ctx.ref_type_registry.clone() else {
            return Ok(());
        };
        let index = ctx.crossref_index.clone();
        let mut diags = Vec::new();
        resolve_blocks(&mut ast.blocks, &registry, index.as_ref(), &mut diags);
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
    diags: &mut Vec<DiagnosticMessage>,
) {
    for block in blocks.iter_mut() {
        resolve_block(block, reg, index, diags);
    }
}

fn resolve_block(
    block: &mut Block,
    reg: &RefTypeRegistry,
    index: Option<&CrossrefIndex>,
    diags: &mut Vec<DiagnosticMessage>,
) {
    match block {
        Block::Plain(p) => resolve_inlines(&mut p.content, reg, index, diags),
        Block::Paragraph(p) => resolve_inlines(&mut p.content, reg, index, diags),
        Block::LineBlock(lb) => {
            for line in &mut lb.content {
                resolve_inlines(line, reg, index, diags);
            }
        }
        Block::Header(h) => resolve_inlines(&mut h.content, reg, index, diags),
        Block::BlockQuote(bq) => resolve_blocks(&mut bq.content, reg, index, diags),
        Block::OrderedList(ol) => {
            for item in &mut ol.content {
                resolve_blocks(item, reg, index, diags);
            }
        }
        Block::BulletList(bl) => {
            for item in &mut bl.content {
                resolve_blocks(item, reg, index, diags);
            }
        }
        Block::DefinitionList(dl) => {
            for (term, defs) in &mut dl.content {
                resolve_inlines(term, reg, index, diags);
                for def in defs {
                    resolve_blocks(def, reg, index, diags);
                }
            }
        }
        Block::Figure(fig) => {
            // Caption inlines too.
            if let Some(long) = fig.caption.long.as_mut() {
                resolve_blocks(long, reg, index, diags);
            }
            if let Some(short) = fig.caption.short.as_mut() {
                resolve_inlines(short, reg, index, diags);
            }
            resolve_blocks(&mut fig.content, reg, index, diags);
        }
        Block::Div(div) => resolve_blocks(&mut div.content, reg, index, diags),
        Block::Custom(node) => {
            for (_name, slot) in node.slots.iter_mut() {
                match slot {
                    Slot::Block(b) => resolve_block(b, reg, index, diags),
                    Slot::Blocks(bs) => resolve_blocks(bs, reg, index, diags),
                    Slot::Inline(i) => resolve_inline(i, reg, index, diags),
                    Slot::Inlines(is) => resolve_inlines(is, reg, index, diags),
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
    diags: &mut Vec<DiagnosticMessage>,
) {
    for inline in inlines.iter_mut() {
        resolve_inline(inline, reg, index, diags);
    }
}

fn resolve_inline(
    inline: &mut Inline,
    reg: &RefTypeRegistry,
    index: Option<&CrossrefIndex>,
    diags: &mut Vec<DiagnosticMessage>,
) {
    // Recurse into children of container inlines first so nested
    // references also get resolved (e.g. a `@fig-..` inside an Emph).
    match inline {
        Inline::Emph(e) => resolve_inlines(&mut e.content, reg, index, diags),
        Inline::Underline(u) => resolve_inlines(&mut u.content, reg, index, diags),
        Inline::Strong(s) => resolve_inlines(&mut s.content, reg, index, diags),
        Inline::Strikeout(s) => resolve_inlines(&mut s.content, reg, index, diags),
        Inline::Superscript(s) => resolve_inlines(&mut s.content, reg, index, diags),
        Inline::Subscript(s) => resolve_inlines(&mut s.content, reg, index, diags),
        Inline::SmallCaps(s) => resolve_inlines(&mut s.content, reg, index, diags),
        Inline::Quoted(q) => resolve_inlines(&mut q.content, reg, index, diags),
        Inline::Link(l) => resolve_inlines(&mut l.content, reg, index, diags),
        Inline::Image(i) => resolve_inlines(&mut i.content, reg, index, diags),
        Inline::Note(n) => resolve_blocks(&mut n.content, reg, index, diags),
        Inline::Span(s) => resolve_inlines(&mut s.content, reg, index, diags),
        Inline::Insert(i) => resolve_inlines(&mut i.content, reg, index, diags),
        Inline::Delete(d) => resolve_inlines(&mut d.content, reg, index, diags),
        Inline::Highlight(h) => resolve_inlines(&mut h.content, reg, index, diags),
        Inline::Custom(node) => {
            for (_name, slot) in node.slots.iter_mut() {
                match slot {
                    Slot::Block(b) => resolve_block(b, reg, index, diags),
                    Slot::Blocks(bs) => resolve_blocks(bs, reg, index, diags),
                    Slot::Inline(i) => resolve_inline(i, reg, index, diags),
                    Slot::Inlines(is) => resolve_inlines(is, reg, index, diags),
                }
            }
        }
        _ => {}
    }

    // Now check this inline itself: is it a crossref Cite?
    if let Inline::Cite(cite) = inline {
        if let Some(replacement) = classify_cite(cite, reg, index, diags) {
            *inline = Inline::Custom(replacement);
        }
    }
}

/// If `cite`'s first citation is classified as a crossref, produce the
/// replacement custom node. Emits diagnostics for mixed-citation bundles
/// and unresolved crossrefs.
fn classify_cite(
    cite: &Cite,
    reg: &RefTypeRegistry,
    index: Option<&CrossrefIndex>,
    diags: &mut Vec<DiagnosticMessage>,
) -> Option<CustomNode> {
    let first = cite.citations.first()?;
    let def = reg.classify_cite_id(&first.id)?;

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
            .all(|c| reg.classify_cite_id(&c.id).is_some());
        if !all_same_kind {
            diags.push(DiagnosticMessage::warning(format!(
                "crossref `@{id}` appears in a `Cite` with bibliographic citations; \
                 split the references into separate brackets to resolve the crossref.",
                id = first.id
            )));
            return None;
        }
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
        cite,
    ))
}

/// Construct the canonical `CustomNode("CrossrefResolvedRef")` inline.
fn build_resolved_ref(
    identifier: &str,
    def: &RefTypeDef,
    resolved: bool,
    entry: Option<&crate::crossref::CrossrefEntry>,
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
    if let Some(e) = entry {
        data.insert(
            "order".into(),
            json!({ "section": e.order.section, "order": e.order.order }),
        );
    }
    node.plain_data = serde_json::Value::Object(data);

    // Keep the original Cite's suffix as a slot so renderers can carry it
    // over — e.g. `[@fig-foo, p. 12]` often carries a page hint. The
    // prefix is dropped because crossref references usually don't carry
    // a leading textual prefix (unlike citations where a prefix is
    // meaningful for citeproc).
    if !original.citations[0].suffix.is_empty() {
        node.slots.insert(
            "suffix".into(),
            Slot::Inlines(original.citations[0].suffix.clone()),
        );
    }
    node
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crossref::{CrossrefEntry, CrossrefIndex, Order, RefTypeRegistry};
    use hashlink::LinkedHashMap;
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
        resolve_inline(inline, reg, index, &mut diags);
        diags
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
        // deferred.)
        let reg = RefTypeRegistry::builtin();
        let idx = make_index_with(&[("fig-a", "fig"), ("fig-b", "fig")]);
        let mut inline = make_multi_cite(&["fig-a", "fig-b"]);
        let diags = resolve(&mut inline, &reg, Some(&idx));
        assert!(diags.is_empty());
        let Inline::Custom(node) = inline else {
            panic!();
        };
        assert_eq!(node.plain_data["identifier"], "fig-a");
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
        resolve_block(&mut block, &reg, Some(&idx), &mut diags);
        assert!(diags.is_empty());
        let Block::Paragraph(p) = block else { panic!() };
        assert!(matches!(p.content[1], Inline::Custom(_)));
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
