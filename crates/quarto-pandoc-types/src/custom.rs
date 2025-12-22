/*
 * custom.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Custom node types for Quarto extensions to the Pandoc AST.
 */

use crate::attr::Attr;
use crate::block::{Block, Blocks};
use crate::inline::{Inline, Inlines};
use hashlink::LinkedHashMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Slot contents in a custom node
///
/// Custom nodes have named slots that can contain blocks, inlines, or lists thereof.
/// The slot type is determined by the custom node handler when it parses a Div/Span.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Slot {
    /// Single block element
    Block(Box<Block>),
    /// Single inline element
    Inline(Box<Inline>),
    /// List of block elements
    Blocks(Blocks),
    /// List of inline elements
    Inlines(Inlines),
}

/// A custom node representing a Quarto extension to the Pandoc AST
///
/// Custom nodes are parsed from Divs or Spans with special class names (e.g., `.callout-warning`).
/// They provide structured, typed access to content that would otherwise be represented as
/// generic containers with special attributes.
///
/// # Serialization
///
/// When serialized to Pandoc JSON (for Lua filter compatibility), custom nodes are
/// "sugared" back into wrapper Divs/Spans with a `__quarto_custom_node` class and
/// special attributes encoding the custom node type and data.
///
/// When deserializing, wrapper Divs/Spans with `__quarto_custom_node` are recognized
/// and converted back to CustomNode.
///
/// # Example
///
/// A callout block:
/// ```text
/// ::: {.callout-warning}
/// ## Title
/// Body content
/// :::
/// ```
///
/// Becomes a `CustomNode` with:
/// - `type_name`: "Callout"
/// - `slots`: { "title": Inlines([...]), "content": Blocks([...]) }
/// - `plain_data`: { "type": "warning", "appearance": "default" }
/// - `attr`: the original Div's attr
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CustomNode {
    /// The type name of this custom node (e.g., "Callout", "PanelTabset")
    pub type_name: String,

    /// Named slots containing AST content
    ///
    /// LinkedHashMap preserves insertion order, which is important for
    /// round-trip serialization to wrapper Divs.
    pub slots: LinkedHashMap<String, Slot>,

    /// Plain JSON data that doesn't contain AST elements
    ///
    /// This is for simple configuration like callout type, appearance settings, etc.
    pub plain_data: Value,

    /// The original attributes from the source Div/Span
    pub attr: Attr,

    /// Source location information
    pub source_info: quarto_source_map::SourceInfo,
}

impl CustomNode {
    /// Create a new custom node
    pub fn new(
        type_name: impl Into<String>,
        attr: Attr,
        source_info: quarto_source_map::SourceInfo,
    ) -> Self {
        CustomNode {
            type_name: type_name.into(),
            slots: LinkedHashMap::new(),
            plain_data: Value::Null,
            attr,
            source_info,
        }
    }

    /// Set a slot value
    pub fn with_slot(mut self, name: impl Into<String>, slot: Slot) -> Self {
        self.slots.insert(name.into(), slot);
        self
    }

    /// Set plain data
    pub fn with_data(mut self, data: Value) -> Self {
        self.plain_data = data;
        self
    }

    /// Get a slot by name
    pub fn get_slot(&self, name: &str) -> Option<&Slot> {
        self.slots.get(name)
    }

    /// Get a mutable reference to a slot by name
    pub fn get_slot_mut(&mut self, name: &str) -> Option<&mut Slot> {
        self.slots.get_mut(name)
    }

    /// Insert or update a slot
    pub fn set_slot(&mut self, name: impl Into<String>, slot: Slot) {
        self.slots.insert(name.into(), slot);
    }

    /// Check if this custom node has any block-level slots
    ///
    /// This can be used to determine whether this was originally parsed from
    /// a Div (block-level) or Span (inline-level).
    pub fn has_block_slots(&self) -> bool {
        self.slots
            .values()
            .any(|slot| matches!(slot, Slot::Block(_) | Slot::Blocks(_)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::attr::empty_attr;
    use crate::block::Paragraph;
    use crate::inline::Str;

    fn dummy_source_info() -> quarto_source_map::SourceInfo {
        quarto_source_map::SourceInfo::original(quarto_source_map::FileId(0), 0, 0)
    }

    #[test]
    fn test_custom_node_creation() {
        let node = CustomNode::new("Callout", empty_attr(), dummy_source_info());
        assert_eq!(node.type_name, "Callout");
        assert!(node.slots.is_empty());
        assert_eq!(node.plain_data, Value::Null);
    }

    #[test]
    fn test_custom_node_with_slots() {
        let node = CustomNode::new("Callout", empty_attr(), dummy_source_info())
            .with_slot("title", Slot::Inlines(vec![]))
            .with_slot("content", Slot::Blocks(vec![]));

        assert_eq!(node.slots.len(), 2);
        assert!(node.get_slot("title").is_some());
        assert!(node.get_slot("content").is_some());
        assert!(node.get_slot("nonexistent").is_none());
    }

    #[test]
    fn test_custom_node_with_data() {
        use serde_json::json;

        let node = CustomNode::new("Callout", empty_attr(), dummy_source_info())
            .with_data(json!({"type": "warning", "appearance": "simple"}));

        assert_eq!(node.plain_data["type"], "warning");
        assert_eq!(node.plain_data["appearance"], "simple");
    }

    #[test]
    fn test_custom_node_serialization() {
        use serde_json::json;

        let node = CustomNode::new("Callout", empty_attr(), dummy_source_info())
            .with_data(json!({"type": "note"}));

        let json = serde_json::to_string(&node).unwrap();
        let deserialized: CustomNode = serde_json::from_str(&json).unwrap();

        assert_eq!(node, deserialized);
    }

    #[test]
    fn test_slot_variants() {
        let block_slot = Slot::Block(Box::new(Block::Paragraph(Paragraph {
            content: vec![],
            source_info: dummy_source_info(),
        })));

        let inline_slot = Slot::Inline(Box::new(Inline::Str(Str {
            text: "test".to_string(),
            source_info: dummy_source_info(),
        })));

        let blocks_slot = Slot::Blocks(vec![]);
        let inlines_slot = Slot::Inlines(vec![]);

        // Test serialization round-trip
        for slot in [block_slot, inline_slot, blocks_slot, inlines_slot] {
            let json = serde_json::to_string(&slot).unwrap();
            let deserialized: Slot = serde_json::from_str(&json).unwrap();
            assert_eq!(slot, deserialized);
        }
    }

    #[test]
    fn test_slot_order_preserved() {
        // Verify that LinkedHashMap preserves insertion order
        let node = CustomNode::new("Test", empty_attr(), dummy_source_info())
            .with_slot("first", Slot::Inlines(vec![]))
            .with_slot("second", Slot::Blocks(vec![]))
            .with_slot("third", Slot::Inlines(vec![]));

        let keys: Vec<&String> = node.slots.keys().collect();
        assert_eq!(keys, vec!["first", "second", "third"]);
    }

    #[test]
    fn test_has_block_slots() {
        let inline_only = CustomNode::new("InlineOnly", empty_attr(), dummy_source_info())
            .with_slot("text", Slot::Inlines(vec![]));
        assert!(!inline_only.has_block_slots());

        let with_blocks = CustomNode::new("WithBlocks", empty_attr(), dummy_source_info())
            .with_slot("content", Slot::Blocks(vec![]));
        assert!(with_blocks.has_block_slots());

        let with_single_block = CustomNode::new("WithBlock", empty_attr(), dummy_source_info())
            .with_slot(
                "body",
                Slot::Block(Box::new(Block::Paragraph(Paragraph {
                    content: vec![],
                    source_info: dummy_source_info(),
                }))),
            );
        assert!(with_single_block.has_block_slots());
    }
}
