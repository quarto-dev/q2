/*
 * citation.rs
 *
 * Functions for processing citation nodes in the tree-sitter AST.
 *
 * Copyright (c) 2025 Posit, PBC
 */

use crate::pandoc::ast_context::ASTContext;
use crate::pandoc::inline::{Citation, CitationMode, Cite, Inline, Space, Str};
use crate::pandoc::location::{
    leading_whitespace_source_info, node_source_info_with_context, tight_source_info_for_node,
};

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

    // Get the citation text and check for leading whitespace.
    // ASCII-only by intent: per the policy in
    // claude-notes/plans/2026-04-30-unicode-whitespace-handling.md
    // (bd-rmx3, bd-8oe4), non-ASCII whitespace is content, not
    // whitespace, so it must not be peeled off into a Space node here.
    let text = node_text();
    let has_leading_space = text.starts_with(|c: char| c.is_ascii_whitespace());
    let trimmed_text = text.trim_ascii().to_string();

    let whole_si = node_source_info_with_context(node, context);
    let tight_si = tight_source_info_for_node(node, context);

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
            source_info: tight_si.clone(),
        })],
        source_info: tight_si.clone(),
    });

    // Build result with leading Space if needed to distinguish "Hi @cite" from "Hi@cite"
    if has_leading_space {
        let space_si = leading_whitespace_source_info(&whole_si, &tight_si).unwrap_or(whole_si);
        PandocNativeIntermediate::IntermediateInlines(vec![
            Inline::Space(Space {
                source_info: space_si,
            }),
            cite,
        ])
    } else {
        PandocNativeIntermediate::IntermediateInline(cite)
    }
}
