/*
 * transforms/proof.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Sugar transform for proof blocks.
 */

//! Sugar transform that canonicalizes proof blocks.
//!
//! A proof is a `Div(.proof)`, optionally with an id and a `name=` title.
//! Unlike theorems, proofs are **not** numbered — they render with an
//! italicized "Proof." prefix. The shared crossref infrastructure would
//! otherwise give them a number, so this transform does **not**
//! populate `plain_data.ref_type`; the indexer therefore skips the
//! resulting `CustomNode("Proof")`, and references to a proof id
//! won't find a numbered entry.
//!
//! Author ids on proofs (uncommon but valid) still flow through as the
//! `attr.identifier`, so deep links with `#my-proof-anchor` work.
//!
//! Scope: `.proof` only. `.remark` and `.solution` have ref-types
//! (`rem`, `sol`) and are numberable in principle but need their own
//! treatment. See the Phase 2 plan follow-up.

use quarto_pandoc_types::attr::{Attr, AttrSourceInfo};
use quarto_pandoc_types::block::{Block, Blocks, Div, Header};
use quarto_pandoc_types::custom::{CustomNode, Slot};
use quarto_pandoc_types::inline::Inlines;
use quarto_pandoc_types::pandoc::Pandoc;
use serde_json::json;

use crate::Result;
use crate::crossref::PROOF;
use crate::render::RenderContext;
use crate::transform::AstTransform;

/// Sugar transform that converts `Div(.proof)` into
/// `CustomNode("Proof")`.
pub struct ProofSugarTransform;

impl ProofSugarTransform {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ProofSugarTransform {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait(?Send)]
impl AstTransform for ProofSugarTransform {
    fn name(&self) -> &str {
        "proof-sugar"
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
    // Recurse first so proofs nested inside other containers are handled.
    match block {
        Block::BlockQuote(bq) => transform_blocks(&mut bq.content),
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
        Block::Figure(fig) => transform_blocks(&mut fig.content),
        Block::Div(div) => transform_blocks(&mut div.content),
        Block::Custom(node) => {
            for (_name, slot) in node.slots.iter_mut() {
                match slot {
                    Slot::Block(b) => transform_block(b),
                    Slot::Blocks(bs) => transform_blocks(bs),
                    _ => {}
                }
            }
        }
        _ => {}
    }

    // Convert this node.
    if let Block::Div(div) = block {
        if has_proof_class(&div.attr) {
            let converted = convert_div(std::mem::replace(
                div,
                Div {
                    attr: empty_attr(),
                    content: Vec::new(),
                    source_info: div.source_info.clone(),
                    attr_source: AttrSourceInfo::empty(),
                },
            ));
            *block = Block::Custom(converted);
        }
    }
}

fn has_proof_class(attr: &Attr) -> bool {
    attr.1.iter().any(|c| c == "proof")
}

fn empty_attr() -> Attr {
    use hashlink::LinkedHashMap;
    (String::new(), Vec::new(), LinkedHashMap::new())
}

fn convert_div(mut div: Div) -> CustomNode {
    // Extract title: `name=` attribute, then first Header. Same rule as
    // theorem sugar.
    let title: Option<Inlines> = extract_name_attr(&mut div.attr, &div.attr_source)
        .or_else(|| extract_first_header_title(&mut div.content));

    // Strip the `.proof` class so a later "match div.proof" filter
    // doesn't double-apply (same pattern as theorem sugar).
    div.attr.1.retain(|c| c != "proof");

    let mut node = CustomNode::new(PROOF, div.attr, div.source_info);
    // Intentionally no `ref_type` / `kind` — proofs are unnumbered and
    // shouldn't be picked up by the indexer or resolver.
    node.plain_data = json!({
        "kind": "Proof",
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

/// Read and remove the `name` attribute from `attr`. See
/// `crate::transforms::theorem::extract_name_attr` for the
/// positional-alignment rationale (this is the parallel implementation
/// for `.proof` Divs).
fn extract_name_attr(attr: &mut Attr, attr_source: &AttrSourceInfo) -> Option<Inlines> {
    let (_id, _classes, kvs) = attr;

    let name_idx = kvs.keys().position(|k| k == "name")?;

    // See `theorem::extract_name_attr` — empty attr_source signals
    // "no provenance available" (common in tests); only assert on
    // populated-but-misaligned input.
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
            source_info: value_source.unwrap_or_default(),
        },
    )])
}

fn extract_first_header_title(content: &mut Blocks) -> Option<Inlines> {
    if let Some(Block::Header(_)) = content.first() {
        let first = content.remove(0);
        if let Block::Header(Header { content: title, .. }) = first {
            return Some(title);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crossref::crossref_target_view;
    use hashlink::LinkedHashMap;
    use quarto_pandoc_types::block::Paragraph;
    use quarto_pandoc_types::inline::{Inline, Str};
    use quarto_source_map::{FileId, SourceInfo};

    fn si() -> SourceInfo {
        SourceInfo::original(FileId(0), 0, 0)
    }

    fn attr(id: &str, classes: &[&str]) -> Attr {
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
        transform_blocks(&mut blocks);
        blocks
    }

    #[test]
    fn div_proof_becomes_proof_custom_node() {
        let div = Block::Div(Div {
            attr: attr("", &["proof"]),
            content: vec![para("Proof body.")],
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        });
        let out = run(vec![div]);
        let Block::Custom(node) = &out[0] else {
            panic!("expected custom node");
        };
        assert_eq!(node.type_name, PROOF);
        // No ref_type — proofs aren't indexed.
        assert!(node.plain_data.get("ref_type").is_none());
        // `.proof` class stripped.
        assert!(!node.attr.1.iter().any(|c| c == "proof"));
    }

    #[test]
    fn proof_not_recognized_as_crossref_target() {
        let div = Block::Div(Div {
            attr: attr("my-proof", &["proof"]),
            content: vec![para("body")],
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        });
        let out = run(vec![div]);
        // Even with an id, a Proof doesn't carry `plain_data.ref_type`,
        // so `crossref_target_view` returns None — the indexer and
        // resolver skip it.
        assert!(crossref_target_view(&out[0]).is_none());
    }

    #[test]
    fn proof_with_name_attribute_captures_title() {
        let mut kvs = LinkedHashMap::new();
        kvs.insert("name".into(), "Custom proof title".into());
        let div = Block::Div(Div {
            attr: ("".into(), vec!["proof".into()], kvs),
            content: vec![para("body")],
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        });
        let out = run(vec![div]);
        let Block::Custom(node) = &out[0] else {
            panic!()
        };
        let Some(Slot::Inlines(title)) = node.slots.get("title") else {
            panic!()
        };
        match &title[0] {
            Inline::Str(s) => assert_eq!(s.text, "Custom proof title"),
            _ => panic!(),
        }
    }

    #[test]
    fn div_without_proof_class_untouched() {
        let div = Block::Div(Div {
            attr: attr("", &["not-a-proof"]),
            content: vec![para("body")],
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        });
        let out = run(vec![div.clone()]);
        assert_eq!(out, vec![div]);
    }
}
