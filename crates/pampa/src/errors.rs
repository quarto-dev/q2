/*
 * errors.rs
 * Copyright (c) 2025 Posit, PBC
 */

// tree-sitter doesn't have a good mechanism for error reporting,
// so we have to manually traverse the tree and find error nodes.

// In addition to that, the tree-sitter-qmd parser is a combination of
// two separate tree-sitter parsers, one for inline content
// and one for block content, and the standard traverser only
// keeps one inline cursor in memory at a time.
//
// This means we can't easily keep copies of the cursors around,
// and we hack around it by using the cursor id to identify nodes
// in the tree, and build clones that way. The main problem with
// this solution is that cursor cloning requires walking the tree
// and can take O(n) time.

use tree_sitter_qmd::MarkdownTree;

enum TreeSitterError {
    MissingNode,
    UnexpectedNode,
}

fn node_can_have_empty_text<'a>(cursor: &tree_sitter_qmd::MarkdownCursor<'a>) -> bool {
    match cursor.node().kind() {
        "block_continuation" => true,
        _ => false,
    }
}

fn is_error_node<'a>(cursor: &tree_sitter_qmd::MarkdownCursor<'a>) -> Option<TreeSitterError> {
    if cursor.node().kind() == "ERROR" {
        return Some(TreeSitterError::UnexpectedNode);
    }
    let byte_range = cursor.node().byte_range();
    if byte_range.start == byte_range.end && !node_can_have_empty_text(cursor) {
        return Some(TreeSitterError::MissingNode); // empty node, indicates that tree-sitter inserted a "missing" node?
    }
    None
}

fn accumulate_error_nodes<'a>(
    cursor: &mut tree_sitter_qmd::MarkdownCursor<'a>,
    errors: &mut Vec<(bool, usize)>,
) {
    if is_error_node(cursor).is_some() {
        errors.push(cursor.id());
        return;
    }
    if cursor.goto_first_child() {
        loop {
            accumulate_error_nodes(cursor, errors);
            if !cursor.goto_next_sibling() {
                break;
            }
        }
        cursor.goto_parent();
    }
}

pub fn parse_is_good(tree: &MarkdownTree) -> Vec<(bool, usize)> {
    let mut errors = Vec::new();
    let mut cursor = tree.walk();
    accumulate_error_nodes(&mut cursor, &mut errors);
    errors
}

pub fn error_message(error: &mut tree_sitter_qmd::MarkdownCursor, input_bytes: &[u8]) -> String {
    // assert!(error.goto_parent());
    // assert!(error.goto_first_child());

    if let Some(which_error) = is_error_node(error) {
        match which_error {
            TreeSitterError::MissingNode => {
                return format!(
                    "Error: Missing {} at {}:{}",
                    error.node().kind(),
                    error.node().start_position().row,
                    error.node().start_position().column,
                );
            }
            TreeSitterError::UnexpectedNode => {
                return format!(
                    "Error: Unexpected {} at {}:{}",
                    error.node().utf8_text(input_bytes).unwrap_or(""),
                    error.node().start_position().row,
                    error.node().start_position().column,
                );
            }
        }
    }
    assert!(false, "No error message available for this node");
    String::new() // unreachable
}
