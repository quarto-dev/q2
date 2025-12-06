/*
 * citation.rs
 *
 * Functions for processing citation nodes in the tree-sitter AST.
 *
 * Copyright (c) 2025 Posit, PBC
 */

use crate::pandoc::ast_context::ASTContext;
use crate::pandoc::inline::{Citation, CitationMode, Cite, Inline, Space, Str};
use crate::pandoc::location::node_source_info_with_context;

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
    let mut citation_id_source = None;
    for (node, child) in children {
        if node == "citation_id_suppress_author" {
            citation_type = CitationMode::SuppressAuthor;
            if let PandocNativeIntermediate::IntermediateBaseText(id, range) = child {
                citation_id = id;
                citation_id_source = Some(
                    crate::pandoc::location::range_to_source_info_with_context(&range, context),
                );
            } else {
                panic!(
                    "Expected BaseText in citation_id_suppress_author, got {:?}",
                    child
                );
            }
        } else if node == "citation_id_author_in_text" {
            citation_type = CitationMode::AuthorInText;
            if let PandocNativeIntermediate::IntermediateBaseText(id, range) = child {
                citation_id = id;
                citation_id_source = Some(
                    crate::pandoc::location::range_to_source_info_with_context(&range, context),
                );
            } else {
                panic!(
                    "Expected BaseText in citation_id_author_in_text, got {:?}",
                    child
                );
            }
        }
    }

    // Get the citation text and check for leading whitespace
    let text = node_text();
    let has_leading_space = text.starts_with(char::is_whitespace);
    let trimmed_text = text.trim().to_string();

    let cite = Inline::Cite(Cite {
        citations: vec![Citation {
            id: citation_id,
            prefix: vec![],
            suffix: vec![],
            mode: citation_type,
            note_num: 1, // Pandoc expects citations to be numbered from 1
            hash: 0,
            id_source: citation_id_source,
        }],
        content: vec![Inline::Str(Str {
            text: trimmed_text,
            source_info: crate::pandoc::location::node_source_info_with_context(node, context),
        })],
        source_info: crate::pandoc::location::node_source_info_with_context(node, context),
    });

    // Build result with leading Space if needed to distinguish "Hi @cite" from "Hi@cite"
    if has_leading_space {
        PandocNativeIntermediate::IntermediateInlines(vec![
            Inline::Space(Space {
                source_info: node_source_info_with_context(node, context),
            }),
            cite,
        ])
    } else {
        PandocNativeIntermediate::IntermediateInline(cite)
    }
}
