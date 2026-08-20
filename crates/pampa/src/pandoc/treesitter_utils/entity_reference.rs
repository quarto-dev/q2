/*
 * entity_reference.rs
 * Copyright (c) 2026 Posit, PBC
 */

use std::collections::HashMap;
use std::sync::OnceLock;

use crate::pandoc::ast_context::ASTContext;
use crate::pandoc::inline::{Inline, Str};
use crate::pandoc::location::node_source_info_with_context;
use crate::pandoc::treesitter_utils::pandocnativeintermediate::PandocNativeIntermediate;

/// Named-entity lookup table (`"&gt;"` → `">"`), parsed lazily from the same
/// WHATWG JSON the grammar's `entity_reference` regex is generated from
/// (`tree_sitter_qmd::HTML_ENTITIES_JSON`), so both sides share one source of
/// truth. The `characters` field is the pre-composed replacement string, which
/// covers multi-codepoint entities (`&NotEqualTilde;` → U+2242 U+0338).
fn entity_table() -> &'static HashMap<String, String> {
    static TABLE: OnceLock<HashMap<String, String>> = OnceLock::new();
    TABLE.get_or_init(|| {
        #[derive(serde::Deserialize)]
        struct Entry {
            characters: String,
        }
        let entries: HashMap<String, Entry> =
            serde_json::from_str(tree_sitter_qmd::HTML_ENTITIES_JSON)
                .expect("tree-sitter-qmd's html_entities.json must be valid JSON");
        entries
            .into_iter()
            .map(|(name, entry)| (name, entry.characters))
            .collect()
    })
}

/// Process named entity references to their character values:
/// `&gt;` => `>`, `&nbsp;` => U+00A0, `&copy;` => `©`, etc.
///
/// A name missing from the table passes through verbatim, with no diagnostic.
/// The regex the grammar matched with is generated from the same table
/// (semicolon-terminated names only, since bd-v8qc9zyc), so a miss is not
/// reachable from grammar-produced nodes; the fallback is defense-in-depth
/// against the two data sources drifting apart.
pub fn process_entity_reference(
    node: &tree_sitter::Node,
    input_bytes: &[u8],
    context: &ASTContext,
) -> PandocNativeIntermediate {
    let text = node.utf8_text(input_bytes).unwrap();
    let result_text = match entity_table().get(text) {
        Some(characters) => characters.clone(),
        None => text.to_string(),
    };
    PandocNativeIntermediate::IntermediateInline(Inline::Str(Str {
        text: result_text,
        source_info: node_source_info_with_context(node, context),
    }))
}
