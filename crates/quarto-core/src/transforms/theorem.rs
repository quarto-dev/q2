/*
 * transforms/theorem.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Sugar transform for theorem-like blocks.
 */

//! Sugar transform that canonicalizes theorem-like blocks.
//!
//! This transform runs in the **normalization** phase (plan D3). It
//! detects `Div` blocks whose class list names a theorem-like category
//! (`.theorem`, `.lemma`, `.corollary`, `.proposition`, `.conjecture`,
//! `.definition`, `.example`, `.exercise`) and converts them into
//! `CustomNode("Theorem")` with the same `plain_data` shape that
//! [`FloatRefTarget`](crate::crossref::FLOAT_REF_TARGET) uses:
//!
//! - `ref_type` (prefix): `thm`, `lem`, `cor`, `prp`, `cnj`, `def`,
//!   `exm`, `exr`.
//! - `kind` (display name): `Theorem`, `Lemma`, ...
//! - `identifier`: full id, e.g. `"thm-pythagoras"` — taken from the Div
//!   attr. Empty iff the author omitted the id.
//!
//! Slots:
//! - `"content"` (Blocks) — the body of the theorem (after title
//!   extraction).
//! - `"title"` (Inlines) — optional title. Extracted from (in order):
//!   1. The `name=` key-value on the Div's attr (Q1 convention).
//!   2. The first `Header` child inside the Div, if any.
//!
//! This matches the existing `crossref_target_view` contract, so the
//! indexer and resolver see theorem custom nodes uniformly with
//! `FloatRefTarget`s without any changes — populate `plain_data` the
//! same way and it just works.
//!
//! ## Why one `"Theorem"` type, not one per flavor
//!
//! Per plan D1b, theorem-like structures share their structural shape
//! and only differ in kind/numbering. One custom type with a `kind`
//! field is sufficient and keeps the filter surface small. If a later
//! phase introduces kind-specific structure (e.g., example with
//! "solution" slot), we can split then.

use quarto_error_reporting::DiagnosticMessage;
use quarto_pandoc_types::attr::{Attr, AttrSourceInfo};
use quarto_pandoc_types::block::{Block, Blocks, Div, Header};
use quarto_pandoc_types::custom::{CustomNode, Slot};
use quarto_pandoc_types::inline::Inlines;
use quarto_pandoc_types::pandoc::Pandoc;
use quarto_source_map::{By, SourceInfo};
use serde_json::json;

use crate::Result;
use crate::crossref::{RefTypeRegistry, THEOREM};
use crate::render::RenderContext;
use crate::transform::AstTransform;

/// Map of theorem-like class name to `(ref_type_prefix, display_kind)`.
///
/// Kept static so the lookup is branchless. Class names are
/// case-sensitive; users write `.theorem` (not `.Theorem`).
const THEOREM_CLASSES: &[(&str, &str, &str)] = &[
    ("theorem", "thm", "Theorem"),
    ("lemma", "lem", "Lemma"),
    ("corollary", "cor", "Corollary"),
    ("proposition", "prp", "Proposition"),
    ("conjecture", "cnj", "Conjecture"),
    ("definition", "def", "Definition"),
    ("example", "exm", "Example"),
    ("exercise", "exr", "Exercise"),
];

/// Sugar transform that converts `Div(.theorem-like)` into
/// `CustomNode("Theorem")`.
pub struct TheoremSugarTransform;

impl TheoremSugarTransform {
    pub fn new() -> Self {
        Self
    }
}

impl Default for TheoremSugarTransform {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait(?Send)]
impl AstTransform for TheoremSugarTransform {
    fn name(&self) -> &str {
        "theorem-sugar"
    }

    async fn transform(&self, ast: &mut Pandoc, ctx: &mut RenderContext) -> Result<()> {
        // The registry is optional — in tests that don't wire it up, we
        // degrade to class-only detection (matches pre-id-prefix behavior).
        // Take it out of the context temporarily so we can also borrow
        // `ctx.diagnostics` mutably for the inconsistency warnings.
        let registry = ctx.ref_type_registry.take();
        transform_blocks(&mut ast.blocks, registry.as_ref(), &mut ctx.diagnostics);
        ctx.ref_type_registry = registry;
        Ok(())
    }
}

fn transform_blocks(
    blocks: &mut Blocks,
    registry: Option<&RefTypeRegistry>,
    diagnostics: &mut Vec<DiagnosticMessage>,
) {
    for block in blocks.iter_mut() {
        transform_block(block, registry, diagnostics);
    }
}

fn transform_block(
    block: &mut Block,
    registry: Option<&RefTypeRegistry>,
    diagnostics: &mut Vec<DiagnosticMessage>,
) {
    // Recurse into children first so nested theorems are handled bottom-up.
    match block {
        Block::BlockQuote(bq) => transform_blocks(&mut bq.content, registry, diagnostics),
        Block::OrderedList(ol) => {
            for item in &mut ol.content {
                transform_blocks(item, registry, diagnostics);
            }
        }
        Block::BulletList(bl) => {
            for item in &mut bl.content {
                transform_blocks(item, registry, diagnostics);
            }
        }
        Block::DefinitionList(dl) => {
            for (_term, defs) in &mut dl.content {
                for def in defs {
                    transform_blocks(def, registry, diagnostics);
                }
            }
        }
        Block::Figure(fig) => transform_blocks(&mut fig.content, registry, diagnostics),
        Block::Div(div) => transform_blocks(&mut div.content, registry, diagnostics),
        Block::Custom(node) => {
            for (_name, slot) in node.slots.iter_mut() {
                match slot {
                    Slot::Block(b) => transform_block(b, registry, diagnostics),
                    Slot::Blocks(bs) => transform_blocks(bs, registry, diagnostics),
                    _ => {}
                }
            }
        }
        _ => {}
    }

    // Check this node itself.
    //
    // Detection strategy follows Q1 (`is_theorem_div` in theorem.lua): a
    // Div is theorem-like iff **either** its class list names a theorem
    // flavor, **or** its id prefix classifies to a theorem ref-type via
    // the registry. Class takes priority because the author's explicit
    // intent is the clearer signal, but id-only (`::: {#thm-foo}`) must
    // also trigger sugaring — otherwise `FloatRefTargetSugarTransform`
    // greedily claims the div and renders it as a generic Div with a
    // `Kind N:` caption instead of a theorem.
    if let Block::Div(div) = block {
        let class_match = match_theorem_class(&div.attr);
        let id_match = match_theorem_id(&div.attr, registry);

        // If the class says "theorem-like" but the id resolves to a
        // non-theorem registered ref-type, flag the inconsistency (plan
        // §D1 edge case). Sugar as a theorem with the class's ref-type —
        // that's the visible-display intent — but record the mismatch.
        if let (Some((class_rt, _)), Some(reg_id_def)) =
            (class_match, registry_classify(&div.attr, registry))
        {
            if reg_id_def.ref_type != class_rt {
                // The user wrote `.theorem` / `.lemma` / … but `#foo-bar`
                // where `foo` is a registered (non-theorem) prefix like `fig`.
                diagnostics.push(DiagnosticMessage::warning(format!(
                    "inconsistent cross-reference specification: `{}` id prefix is incompatible with `{}` class",
                    reg_id_def.ref_type,
                    theorem_class_for(class_rt),
                )));
            }
        }

        let matched = class_match.or(id_match);
        if let Some((ref_type, kind)) = matched {
            let converted = convert_div(
                std::mem::replace(
                    div,
                    Div {
                        attr: empty_attr(),
                        content: Vec::new(),
                        source_info: div.source_info.clone(),
                        attr_source: AttrSourceInfo::empty(),
                    },
                ),
                ref_type,
                kind,
            );
            *block = Block::Custom(converted);
        }
    }
}

/// If this Div's class list names a theorem-like category, return the
/// matching `(ref_type, kind)` pair. The first matching class wins — a
/// Div with both `.theorem` and `.lemma` is unusual; we don't try to be
/// clever about it.
fn match_theorem_class(attr: &Attr) -> Option<(&'static str, &'static str)> {
    for class in &attr.1 {
        for (name, ref_type, kind) in THEOREM_CLASSES {
            if class == name {
                return Some((ref_type, kind));
            }
        }
    }
    None
}

/// If this Div's id prefix classifies (via the registry) to one of the
/// built-in theorem ref-types, return the matching `(ref_type, kind)`
/// pair. Mirrors Q1's `has_theorem_ref` — detection purely by id prefix.
///
/// Returns `None` if no registry is available or the id doesn't match a
/// theorem flavor. We filter on the fixed [`THEOREM_CLASSES`] set rather
/// than trust the registry's `kind`, because user-defined categories
/// could collide with theorem prefixes in exotic metadata and we want
/// only the built-in theorem types to take this sugar path.
fn match_theorem_id(
    attr: &Attr,
    registry: Option<&RefTypeRegistry>,
) -> Option<(&'static str, &'static str)> {
    let registry = registry?;
    let def = registry.classify_cite_id(&attr.0)?;
    for (_class, ref_type, kind) in THEOREM_CLASSES {
        if *ref_type == def.ref_type {
            return Some((ref_type, kind));
        }
    }
    None
}

/// Classify the id via the registry without the theorem-flavor filter.
/// Used by the inconsistency diagnostic to see what the *id* alone says,
/// independent of what the class says.
fn registry_classify<'r>(
    attr: &Attr,
    registry: Option<&'r RefTypeRegistry>,
) -> Option<&'r crate::crossref::RefTypeDef> {
    registry?.classify_cite_id(&attr.0)
}

fn empty_attr() -> Attr {
    use hashlink::LinkedHashMap;
    (String::new(), Vec::new(), LinkedHashMap::new())
}

/// Convert a Div we've already matched to the theorem-like set into a
/// `CustomNode("Theorem")`.
///
/// Preserves the original Div's `attr` (so the id flows through intact)
/// but strips the theorem class name itself from the attr's class list —
/// the `plain_data.kind` is the authoritative source from here on. This
/// prevents double-rendering later if a CSS-style transform matches on
/// `.theorem`.
fn convert_div(mut div: Div, ref_type: &str, kind: &str) -> CustomNode {
    // Extract title:
    //   1. `name=` attribute on the Div (Q1 convention).
    //   2. First Header child, if present.
    let title: Option<Inlines> = extract_name_attr(&mut div.attr, &div.attr_source)
        .or_else(|| extract_first_header_title(&mut div.content));

    // Strip the theorem class so downstream transforms don't re-match.
    div.attr
        .1
        .retain(|c| c.as_str() != theorem_class_for(ref_type));

    let identifier = div.attr.0.clone();

    let mut node = CustomNode::new(THEOREM, div.attr, div.source_info);
    node.plain_data = json!({
        "ref_type":   ref_type,
        "kind":       kind,
        "identifier": identifier,
    });
    node.slots
        .insert("content".into(), Slot::Blocks(div.content));
    if let Some(inlines) = title {
        if !inlines.is_empty() {
            node.slots.insert("title".into(), Slot::Inlines(inlines));
        }
    }
    node
}

/// Read and remove the `name` attribute from `attr`, returning its value
/// parsed as plain inlines (just a `Str`, since attrs are strings).
///
/// The user-facing `name="Pythagoras"` becomes
/// `vec![Str("Pythagoras")]`. Inline markup inside the title (bold,
/// italic, etc.) isn't supported today because attribute values are
/// bare strings in Pandoc's data model — matching Q1's behavior.
///
/// The returned `Str` carries the attribute value's parser-recorded
/// source range (an `Original` covering the bytes between the `=` and
/// the matching quote / whitespace) so attribution and the incremental
/// writer can resolve the title back to user-editable bytes.
///
/// Uses `AttrSourceInfo`'s positional-alignment invariant (see
/// `crates/quarto-pandoc-types/src/attr.rs`) to find the value's
/// `SourceInfo`; falls back to `None` if alignment fails (bd-3aolj
/// / bd-1e6a5) so production never panics.
fn extract_name_attr(attr: &mut Attr, attr_source: &AttrSourceInfo) -> Option<Inlines> {
    let (_id, _classes, kvs) = attr;

    // Find the positional index of "name" before removing it so we can
    // index into attr_source.attributes (which is parallel to kvs in
    // insertion order).
    let name_idx = kvs.keys().position(|k| k == "name")?;

    // Validate the positional-alignment invariant. An empty `attr_source`
    // signals "no provenance available" (common pattern in tests that
    // construct theorem divs by hand) — that case isn't a bug, so don't
    // assert. Only assert when `attr_source.attributes` is populated but
    // misaligned with `kvs` (the bd-3aolj / bd-1e6a5 parser bugs).
    debug_assert!(
        attr_source.attributes.is_empty() || kvs.len() == attr_source.attributes.len(),
        "AttrSourceInfo.attributes is out of sync with Attr.2 (bd-3aolj / bd-1e6a5): kvs={}, attr_source={}",
        kvs.len(),
        attr_source.attributes.len(),
    );
    let value_source = if kvs.len() == attr_source.attributes.len() {
        attr_source.attributes[name_idx].1.clone()
    } else {
        None
    };

    let name = kvs.remove("name")?;
    if name.is_empty() {
        return None;
    }
    Some(vec![quarto_pandoc_types::inline::Inline::Str(
        quarto_pandoc_types::inline::Str {
            text: name,
            source_info: value_source
                .unwrap_or_else(|| SourceInfo::generated(By::programmatic_config())),
        },
    )])
}

/// Pop the first `Header` from the content blocks and return its inline
/// content as a title. If the first block isn't a Header, leaves
/// `content` unchanged and returns None.
fn extract_first_header_title(content: &mut Blocks) -> Option<Inlines> {
    if let Some(Block::Header(_)) = content.first() {
        let first = content.remove(0);
        if let Block::Header(Header { content: title, .. }) = first {
            return Some(title);
        }
    }
    None
}

fn theorem_class_for(ref_type: &str) -> &'static str {
    for (class, rt, _kind) in THEOREM_CLASSES {
        if *rt == ref_type {
            return class;
        }
    }
    ""
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crossref::crossref_target_view;
    use hashlink::LinkedHashMap;
    use quarto_pandoc_types::attr::{Attr, AttrSourceInfo};
    use quarto_pandoc_types::block::{Block, Div, Header, Paragraph};
    use quarto_pandoc_types::inline::{Inline, Str};
    use quarto_source_map::{FileId, SourceInfo};

    fn si() -> SourceInfo {
        SourceInfo::original(FileId(0), 0, 0)
    }

    fn attr_id_classes(id: &str, classes: &[&str]) -> Attr {
        (
            id.to_string(),
            classes.iter().map(|s| s.to_string()).collect(),
            LinkedHashMap::new(),
        )
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

    fn run(mut blocks: Vec<Block>) -> Vec<Block> {
        let mut diags = Vec::new();
        transform_blocks(&mut blocks, None, &mut diags);
        blocks
    }

    /// Run with a built-in registry so id-prefix detection kicks in.
    fn run_with_registry(mut blocks: Vec<Block>) -> (Vec<Block>, Vec<DiagnosticMessage>) {
        let registry = RefTypeRegistry::builtin();
        let mut diags = Vec::new();
        transform_blocks(&mut blocks, Some(&registry), &mut diags);
        (blocks, diags)
    }

    #[test]
    fn plain_theorem_div_becomes_theorem_custom_node() {
        let div = Block::Div(Div {
            attr: attr_id_classes("thm-pyth", &["theorem"]),
            content: vec![para("For a right triangle...")],
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        });
        let out = run(vec![div]);

        let Block::Custom(node) = &out[0] else {
            panic!("expected custom node, got {:?}", out[0]);
        };
        assert_eq!(node.type_name, THEOREM);
        assert_eq!(node.plain_data["ref_type"], "thm");
        assert_eq!(node.plain_data["kind"], "Theorem");
        assert_eq!(node.plain_data["identifier"], "thm-pyth");
        // `.theorem` class stripped from attr so a later "match div.theorem"
        // filter doesn't double-apply.
        assert!(!node.attr.1.iter().any(|c| c == "theorem"));

        // Content preserved in slot.
        let Some(Slot::Blocks(bs)) = node.slots.get("content") else {
            panic!();
        };
        assert_eq!(bs.len(), 1);
        assert!(matches!(bs[0], Block::Paragraph(_)));
    }

    #[test]
    fn lemma_corollary_etc_all_recognized() {
        for (class, ref_type, kind) in [
            ("lemma", "lem", "Lemma"),
            ("corollary", "cor", "Corollary"),
            ("proposition", "prp", "Proposition"),
            ("conjecture", "cnj", "Conjecture"),
            ("definition", "def", "Definition"),
            ("example", "exm", "Example"),
            ("exercise", "exr", "Exercise"),
        ] {
            let div = Block::Div(Div {
                attr: attr_id_classes(&format!("{ref_type}-x"), &[class]),
                content: vec![para("body")],
                source_info: si(),
                attr_source: AttrSourceInfo::empty(),
            });
            let out = run(vec![div]);
            let Block::Custom(node) = &out[0] else {
                panic!("{} did not sugar", class);
            };
            assert_eq!(node.plain_data["ref_type"], ref_type);
            assert_eq!(node.plain_data["kind"], kind);
        }
    }

    #[test]
    fn div_without_theorem_class_untouched() {
        let div = Block::Div(Div {
            attr: attr_id_classes("thm-fake", &["callout-note"]),
            content: vec![para("not a theorem")],
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        });
        let out = run(vec![div.clone()]);
        assert_eq!(out, vec![div]);
    }

    #[test]
    fn name_attribute_becomes_title_slot() {
        let mut kvs = LinkedHashMap::new();
        kvs.insert("name".into(), "Pythagorean Theorem".into());
        let div = Block::Div(Div {
            attr: ("thm-pyth".into(), vec!["theorem".into()], kvs),
            content: vec![para("body")],
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        });
        let out = run(vec![div]);
        let Block::Custom(node) = &out[0] else {
            panic!()
        };
        let Some(Slot::Inlines(title)) = node.slots.get("title") else {
            panic!("no title slot");
        };
        assert_eq!(title.len(), 1);
        match &title[0] {
            Inline::Str(s) => assert_eq!(s.text, "Pythagorean Theorem"),
            _ => panic!(),
        }
        // `name` key removed from attr so it doesn't leak into rendered
        // output (would otherwise appear as a data-name="...").
        assert!(!node.attr.2.contains_key("name"));
    }

    #[test]
    fn first_header_becomes_title_if_no_name_attribute() {
        let div = Block::Div(Div {
            attr: attr_id_classes("thm-x", &["theorem"]),
            content: vec![
                Block::Header(Header {
                    level: 3,
                    attr: (String::new(), Vec::new(), LinkedHashMap::new()),
                    content: vec![Inline::Str(Str {
                        text: "Header title".into(),
                        source_info: si(),
                    })],
                    source_info: si(),
                    attr_source: AttrSourceInfo::empty(),
                }),
                para("body"),
            ],
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        });
        let out = run(vec![div]);
        let Block::Custom(node) = &out[0] else {
            panic!();
        };
        let Some(Slot::Inlines(title)) = node.slots.get("title") else {
            panic!()
        };
        match &title[0] {
            Inline::Str(s) => assert_eq!(s.text, "Header title"),
            _ => panic!(),
        }
        // Header removed from content, leaving just the body para.
        let Some(Slot::Blocks(bs)) = node.slots.get("content") else {
            panic!()
        };
        assert_eq!(bs.len(), 1);
        assert!(matches!(bs[0], Block::Paragraph(_)));
    }

    #[test]
    fn crossref_target_view_recognizes_sugared_theorem() {
        let div = Block::Div(Div {
            attr: attr_id_classes("thm-x", &["theorem"]),
            content: vec![para("body")],
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        });
        let out = run(vec![div]);
        let view = crossref_target_view(&out[0]).expect("theorem visible as crossref target");
        assert_eq!(view.identifier, "thm-x");
        assert_eq!(view.ref_type, "thm");
        assert_eq!(view.kind, "Theorem");
    }

    #[test]
    fn theorem_without_id_still_sugars() {
        // Unnumbered theorem: no id. Still becomes a Theorem custom
        // node so renderers can style it consistently.
        let div = Block::Div(Div {
            attr: attr_id_classes("", &["theorem"]),
            content: vec![para("body")],
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        });
        let out = run(vec![div]);
        let Block::Custom(node) = &out[0] else {
            panic!()
        };
        assert_eq!(node.plain_data["identifier"], "");
    }

    #[test]
    fn id_prefix_alone_triggers_theorem_sugar() {
        // `::: {#thm-x}` without `.theorem` class still sugars — Q1 parity
        // (see plan bd-gvhe §D1). Without this, FloatRefTargetSugarTransform
        // would claim it later and render as a generic Div with a
        // "Theorem 1: " caption on the last paragraph.
        let div = Block::Div(Div {
            attr: attr_id_classes("thm-x", &[]),
            content: vec![para("body")],
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        });
        let (out, diags) = run_with_registry(vec![div]);
        assert!(diags.is_empty(), "unexpected diags: {diags:?}");
        let Block::Custom(node) = &out[0] else {
            panic!("expected Theorem custom node, got {:?}", out[0]);
        };
        assert_eq!(node.type_name, THEOREM);
        assert_eq!(node.plain_data["ref_type"], "thm");
        assert_eq!(node.plain_data["kind"], "Theorem");
    }

    #[test]
    fn id_prefix_detects_all_theorem_flavors() {
        for (_class, ref_type, kind) in THEOREM_CLASSES {
            let div = Block::Div(Div {
                attr: attr_id_classes(&format!("{ref_type}-x"), &[]),
                content: vec![para("body")],
                source_info: si(),
                attr_source: AttrSourceInfo::empty(),
            });
            let (out, _) = run_with_registry(vec![div]);
            let Block::Custom(node) = &out[0] else {
                panic!("{ref_type} by id alone did not sugar");
            };
            assert_eq!(node.plain_data["ref_type"], *ref_type);
            assert_eq!(node.plain_data["kind"], *kind);
        }
    }

    #[test]
    fn id_only_non_theorem_prefix_leaves_div_alone() {
        // `::: {#fig-foo}` must NOT be theorem-sugared — it's a float.
        let div = Block::Div(Div {
            attr: attr_id_classes("fig-foo", &[]),
            content: vec![para("caption")],
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        });
        let (out, diags) = run_with_registry(vec![div.clone()]);
        assert!(diags.is_empty());
        // Still a Div, not a Custom node.
        assert!(matches!(out[0], Block::Div(_)));
    }

    #[test]
    fn class_id_mismatch_emits_inconsistency_diagnostic() {
        // Plan §D1 edge case: `.theorem #fig-x` → warn. The div still
        // sugars as a theorem (class wins for display), but the id
        // prefix (`fig`) disagrees with the theorem flavor.
        let div = Block::Div(Div {
            attr: attr_id_classes("fig-x", &["theorem"]),
            content: vec![para("body")],
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        });
        let (out, diags) = run_with_registry(vec![div]);
        assert!(matches!(out[0], Block::Custom(_)));
        assert_eq!(
            diags.len(),
            1,
            "expected 1 inconsistency diag, got {diags:?}"
        );
        let msg = format!("{:?}", diags[0]);
        assert!(
            msg.contains("fig") && msg.contains("theorem"),
            "expected message naming both `fig` prefix and `theorem` class, got: {msg}"
        );
    }

    #[test]
    fn class_id_match_emits_no_diagnostic() {
        // `::: {.theorem #thm-x}` — consistent. No warning.
        let div = Block::Div(Div {
            attr: attr_id_classes("thm-x", &["theorem"]),
            content: vec![para("body")],
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        });
        let (_out, diags) = run_with_registry(vec![div]);
        assert!(diags.is_empty(), "unexpected diags: {diags:?}");
    }
}
