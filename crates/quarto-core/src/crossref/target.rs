/*
 * crossref/target.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Uniform inspection API for crossref-capable blocks.
 */

//! Uniform inspection API for crossref-capable blocks.
//!
//! Float-ref targets post-sugaring are `CustomNode("FloatRefTarget", ..)`.
//! Future block-level crossref categories (theorems, callouts with ids, ...)
//! live in their own [`CustomNode`] types and carry their own slot structure,
//! but the *index builder* and *reference resolver* both want to treat any
//! crossref-capable block uniformly.
//!
//! This module exposes that shared view. Adding a new crossref-capable custom
//! node type in the future is a matter of extending
//! [`crossref_target_view`] to recognize it — all call sites gain support
//! automatically.
//!
//! Design reference: plan D1b ("shared inspection API") in
//! `claude-notes/plans/2026-04-15-crossref-design.md`.

use quarto_pandoc_types::block::Block;
use quarto_pandoc_types::custom::CustomNode;
use quarto_pandoc_types::inline::Inline;
use quarto_source_map::SourceInfo;

/// A uniform read-only view over the crossref-relevant fields of a block.
///
/// Borrowed from the underlying node — cheap to construct, cheap to pass
/// around. Callers that need ownership should `.to_owned()` individual fields.
#[derive(Debug, Clone, Copy)]
pub struct CrossrefTargetView<'a> {
    /// Full identifier, e.g. `"fig-myplot"`. Canonically taken from the
    /// node's `attr.identifier` (the block-level id); the CustomNode's
    /// `plain_data.identifier` is redundant and kept only for JSON
    /// readability.
    pub identifier: &'a str,

    /// Id prefix, e.g. `"fig"` — matches [`crate::crossref::RefTypeRegistry`]
    /// keys. Read from `plain_data.ref_type`.
    pub ref_type: &'a str,

    /// Display / category name, e.g. `"Figure"`. Read from `plain_data.kind`.
    pub kind: &'a str,

    /// Source location of the target in the authored document. Used for
    /// diagnostics (duplicate ids, unresolved refs).
    pub source_info: &'a SourceInfo,
}

/// Return a view over the block if it is a crossref-capable target.
///
/// Recognizes custom-node types whose `plain_data` carries the standard
/// crossref triple (`ref_type`, `kind`, `identifier`):
///
/// - `FloatRefTarget` — figures, tables, listings, custom floats.
/// - `Theorem` — theorem-like blocks.
///
/// Callouts with an explicit crossref id are recognized too — their
/// `plain_data` is populated during normalization (plan 2.2).
///
/// All supported types share the same plain-data shape, so a single
/// read path works for every recognized category.
pub fn crossref_target_view(block: &Block) -> Option<CrossrefTargetView<'_>> {
    let Block::Custom(node) = block else {
        return None;
    };
    // The `ref_type` field in plain_data is the signal for "this custom
    // node participates in crossrefs". `FloatRefTarget` and `Theorem`
    // always carry it; `Callout` gets it populated only when the user
    // gave it a crossref id.
    view_from_plain_data(node)
}

/// Inline-level counterpart to [`crossref_target_view`].
///
/// Inline crossref targets exist too — labelled display equations are
/// `Inline::Custom("Equation")` with the standard plain_data triple. A
/// walker that wants a uniform outline of all crossref targets (as the
/// LSP outline does) must recognize both shapes.
pub fn crossref_target_view_inline(inline: &Inline) -> Option<CrossrefTargetView<'_>> {
    let Inline::Custom(node) = inline else {
        return None;
    };
    view_from_plain_data(node)
}

/// Return the ref-type prefix if the block is a crossref target.
///
/// Convenience wrapper over [`crossref_target_view`] for callers that only
/// care about the prefix (e.g. bucketing targets by category before numbering).
pub fn ref_type_of(block: &Block) -> Option<&str> {
    crossref_target_view(block).map(|v| v.ref_type)
}

/// Return the identifier if the block is a crossref target.
pub fn identifier_of(block: &Block) -> Option<&str> {
    crossref_target_view(block).map(|v| v.identifier)
}

/// Read a [`CrossrefTargetView`] off a custom node's plain_data. Returns
/// `None` for nodes that don't carry the standard crossref triple or
/// whose identifier is empty (unnumbered / not a crossref target).
fn view_from_plain_data(node: &CustomNode) -> Option<CrossrefTargetView<'_>> {
    let identifier = node.attr.0.as_str();
    if identifier.is_empty() {
        return None;
    }
    let ref_type = node.plain_data.get("ref_type")?.as_str()?;
    let kind = node.plain_data.get("kind")?.as_str()?;
    Some(CrossrefTargetView {
        identifier,
        ref_type,
        kind,
        source_info: &node.source_info,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crossref::FLOAT_REF_TARGET;
    use hashlink::LinkedHashMap;
    use quarto_pandoc_types::attr::empty_attr;
    use quarto_pandoc_types::block::{Block, Paragraph};
    use quarto_pandoc_types::custom::{CustomNode, Slot};
    use quarto_source_map::{FileId, SourceInfo};
    use serde_json::json;

    fn si() -> SourceInfo {
        SourceInfo::original(FileId(0), 0, 0)
    }

    fn make_float_ref_target(ident: &str, ref_type: &str, kind: &str) -> Block {
        let attr = (ident.to_string(), vec![], LinkedHashMap::new());
        let mut node = CustomNode::new(FLOAT_REF_TARGET, attr, si());
        node.plain_data = json!({
            "ref_type": ref_type,
            "kind": kind,
            "identifier": ident,
        });
        node.slots.insert("content".into(), Slot::Blocks(vec![]));
        Block::Custom(node)
    }

    #[test]
    fn view_for_float_ref_target() {
        let block = make_float_ref_target("fig-one", "fig", "Figure");
        let view = crossref_target_view(&block).expect("should be a crossref target");
        assert_eq!(view.identifier, "fig-one");
        assert_eq!(view.ref_type, "fig");
        assert_eq!(view.kind, "Figure");
    }

    #[test]
    fn view_none_for_plain_paragraph() {
        let block = Block::Paragraph(Paragraph {
            content: vec![],
            source_info: si(),
        });
        assert!(crossref_target_view(&block).is_none());
    }

    #[test]
    fn view_none_for_unrelated_custom_node() {
        // Callout without crossref plain_data returns None.
        let node = CustomNode::new("Callout", empty_attr(), si());
        let block = Block::Custom(node);
        assert!(crossref_target_view(&block).is_none());
    }

    #[test]
    fn view_recognizes_theorem_custom_node() {
        use crate::crossref::THEOREM;
        let attr = ("thm-one".to_string(), vec![], LinkedHashMap::new());
        let mut node = CustomNode::new(THEOREM, attr, si());
        node.plain_data = json!({
            "ref_type": "thm",
            "kind": "Theorem",
            "identifier": "thm-one",
        });
        let block = Block::Custom(node);
        let view = crossref_target_view(&block).expect("theorem recognized");
        assert_eq!(view.ref_type, "thm");
        assert_eq!(view.kind, "Theorem");
    }

    #[test]
    fn view_recognizes_callout_with_crossref_plain_data() {
        // A Callout that *does* carry crossref plain_data is treated as a
        // target (this is how Phase 2.2's callout integration works once
        // a sugaring pass populates these fields on Callouts with
        // crossref ids).
        let attr = ("nte-one".to_string(), vec![], LinkedHashMap::new());
        let mut node = CustomNode::new("Callout", attr, si());
        node.plain_data = json!({
            "ref_type": "nte",
            "kind": "Note",
            "identifier": "nte-one",
        });
        let block = Block::Custom(node);
        let view = crossref_target_view(&block).expect("callout recognized");
        assert_eq!(view.ref_type, "nte");
    }

    #[test]
    fn view_none_for_float_ref_target_without_identifier() {
        let attr = (String::new(), vec![], LinkedHashMap::new());
        let mut node = CustomNode::new(FLOAT_REF_TARGET, attr, si());
        node.plain_data = json!({"ref_type": "fig", "kind": "Figure"});
        let block = Block::Custom(node);
        assert!(crossref_target_view(&block).is_none());
    }

    #[test]
    fn view_none_when_plain_data_missing_ref_type() {
        let attr = ("fig-one".to_string(), vec![], LinkedHashMap::new());
        let mut node = CustomNode::new(FLOAT_REF_TARGET, attr, si());
        node.plain_data = json!({"kind": "Figure"});
        let block = Block::Custom(node);
        assert!(crossref_target_view(&block).is_none());
    }

    #[test]
    fn convenience_helpers_agree_with_view() {
        let block = make_float_ref_target("tbl-x", "tbl", "Table");
        assert_eq!(ref_type_of(&block), Some("tbl"));
        assert_eq!(identifier_of(&block), Some("tbl-x"));
    }
}
