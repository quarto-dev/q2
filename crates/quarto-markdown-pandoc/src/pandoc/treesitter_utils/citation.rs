/*
 * citation.rs
 *
 * Functions for processing citation nodes in the tree-sitter AST.
 *
 * Copyright (c) 2025 Posit, PBC
 */

use crate::pandoc::ast_context::ASTContext;
use crate::pandoc::inline::{Citation, CitationMode, Cite, Inline, Str};
use crate::pandoc::source_map_compat;

use super::pandocnativeintermediate::PandocNativeIntermediate;

pub fn process_citation<F>(
    node: &tree_sitter::Node,
    node_text: F,
    children: Vec<(String, PandocNativeIntermediate)>,
    context: &ASTContext,
) -> PandocNativeIntermediate
where
    F: Fn() -> String,
{
    let mut citation_type = CitationMode::NormalCitation;
    let mut citation_id = String::new();
    for (node, child) in children {
        if node == "citation_id_suppress_author" {
            citation_type = CitationMode::SuppressAuthor;
            if let PandocNativeIntermediate::IntermediateBaseText(id, _) = child {
                citation_id = id;
            } else {
                panic!(
                    "Expected BaseText in citation_id_suppress_author, got {:?}",
                    child
                );
            }
        } else if node == "citation_id_author_in_text" {
            citation_type = CitationMode::AuthorInText;
            if let PandocNativeIntermediate::IntermediateBaseText(id, _) = child {
                citation_id = id;
            } else {
                panic!(
                    "Expected BaseText in citation_id_author_in_text, got {:?}",
                    child
                );
            }
        }
    }
    PandocNativeIntermediate::IntermediateInline(Inline::Cite(Cite {
        citations: vec![Citation {
            id: citation_id,
            prefix: vec![],
            suffix: vec![],
            mode: citation_type,
            note_num: 1, // Pandoc expects citations to be numbered from 1
            hash: 0,
        }],
        content: vec![Inline::Str(Str {
            text: node_text(),
            source_info: source_map_compat::node_to_source_info_with_context(
                node, context,
            ),
        })],
        source_info: source_map_compat::node_to_source_info_with_context(node, context),
    }))
}
