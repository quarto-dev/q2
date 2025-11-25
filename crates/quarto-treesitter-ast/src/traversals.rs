/*
 * traversals.rs
 *
 * Copyright (c) 2025 Posit, PBC
 *
 * Generic traversal helpers for tree-sitter TreeCursor.
 *
 * These traversal functions work with any tree-sitter grammar and allow
 * different parsers (qmd, templates, etc.) to reuse the same traversal logic.
 */

use tree_sitter::{Node, TreeCursor};

/// Phase of tree traversal - whether we're entering or exiting a node.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub enum TraversePhase {
    Enter,
    Exit,
}

/// Top-down traversal of a tree-sitter tree.
///
/// Visits each node twice: once on entry (before children) and once on exit (after children).
/// The visitor returns `true` to descend into children, `false` to skip them.
///
/// # Arguments
/// * `cursor` - A tree-sitter cursor positioned at the starting node
/// * `visitor` - A function called for each node with the node and phase
///
/// # Example
/// ```ignore
/// topdown_traverse_concrete_tree(&mut cursor, &mut |node, phase| {
///     println!("{:?}: {}", phase, node.kind());
///     true // descend into children
/// });
/// ```
pub fn topdown_traverse_concrete_tree<F>(cursor: &mut TreeCursor, visitor: &mut F)
where
    F: for<'a> FnMut(&'a Node, TraversePhase) -> bool,
{
    let mut stack: Vec<usize> = vec![0];
    while !stack.is_empty() {
        match stack.pop().unwrap() {
            0 => {
                stack.push(2); // exit
                if visitor(&cursor.node(), TraversePhase::Enter) && cursor.goto_first_child() {
                    stack.push(1); // go to parent
                    stack.push(3); // check for next sibling
                    stack.push(0); // recurse
                }
            }
            1 => {
                cursor.goto_parent();
            }
            2 => {
                visitor(&cursor.node(), TraversePhase::Exit);
            }
            3 => {
                if cursor.goto_next_sibling() {
                    stack.push(3); // continue sibling traversal
                    stack.push(0); // recurse
                }
            }
            _ => unreachable!(),
        }
    }
}

/// Phase tracking for bottom-up traversal, holding accumulated children.
#[derive(Debug)]
pub enum BottomUpTraversePhase<'a, T: std::fmt::Debug> {
    Enter(Node<'a>),
    GoToSiblings(Node<'a>, Vec<(String, T)>), // accumulated children
    Exit(Node<'a>),
}

/// Bottom-up traversal of a tree-sitter tree with context.
///
/// Processes children before parents, accumulating results from children
/// and passing them to the parent's visitor call.
///
/// # Type Parameters
/// * `F` - The visitor function type
/// * `T` - The result type produced by the visitor for each node
/// * `C` - The context type passed through to visitors
///
/// # Arguments
/// * `cursor` - A tree-sitter cursor positioned at the starting node
/// * `visitor` - A function called for each node with node, children results, input, and context
/// * `input_bytes` - The source text as bytes
/// * `context` - Parser-specific context (e.g., ASTContext for qmd, simpler for templates)
///
/// # Returns
/// A tuple of (node_kind, result) for the root node.
///
/// # Example
/// ```ignore
/// let (kind, result) = bottomup_traverse_concrete_tree(
///     &mut cursor,
///     &mut |node, children, input, ctx| {
///         // Process node with accumulated children results
///         MyASTNode::from_children(node, children, input, ctx)
///     },
///     input_bytes,
///     &my_context,
/// );
/// ```
pub fn bottomup_traverse_concrete_tree<F, T: std::fmt::Debug, C>(
    cursor: &mut TreeCursor,
    visitor: &mut F,
    input_bytes: &[u8],
    context: &C,
) -> (String, T)
where
    F: for<'a> FnMut(&'a Node, Vec<(String, T)>, &[u8], &C) -> T,
{
    let mut stack: Vec<BottomUpTraversePhase<T>> =
        vec![BottomUpTraversePhase::Enter(cursor.node())];

    loop {
        let top = stack.pop().unwrap();
        match top {
            BottomUpTraversePhase::Enter(node) => {
                stack.push(BottomUpTraversePhase::GoToSiblings(node, Vec::new()));
                if cursor.goto_first_child() {
                    stack.push(BottomUpTraversePhase::Enter(cursor.node()));
                } else {
                    stack.push(BottomUpTraversePhase::Exit(node));
                }
            }
            BottomUpTraversePhase::GoToSiblings(node, vec) => {
                stack.push(BottomUpTraversePhase::GoToSiblings(node, vec));
                if cursor.goto_next_sibling() {
                    stack.push(BottomUpTraversePhase::Enter(cursor.node()));
                } else {
                    stack.push(BottomUpTraversePhase::Exit(node));
                    cursor.goto_parent();
                }
            }
            BottomUpTraversePhase::Exit(node) => {
                let Some(BottomUpTraversePhase::GoToSiblings(_, children)) = stack.pop() else {
                    panic!("Expected GoToSiblings phase on stack");
                };
                let (kind, result) = (
                    node.kind().to_string(),
                    visitor(&node, children, input_bytes, context),
                );
                match stack.last_mut() {
                    None => return (kind, result), // we are done
                    Some(BottomUpTraversePhase::GoToSiblings(_, next_children)) => {
                        next_children.push((kind, result));
                    }
                    _ => {
                        panic!("Expected GoToSiblings phase on stack");
                    }
                }
            }
        }
    }
}

/// Bottom-up traversal without external context.
///
/// A simpler version when no external context is needed - the visitor
/// can capture any needed context via closure.
///
/// # Type Parameters
/// * `F` - The visitor function type
/// * `T` - The result type produced by the visitor for each node
///
/// # Arguments
/// * `cursor` - A tree-sitter cursor positioned at the starting node
/// * `visitor` - A function called for each node with node, children results, and input
/// * `input_bytes` - The source text as bytes
///
/// # Returns
/// A tuple of (node_kind, result) for the root node.
pub fn bottomup_traverse_concrete_tree_no_context<F, T: std::fmt::Debug>(
    cursor: &mut TreeCursor,
    visitor: &mut F,
    input_bytes: &[u8],
) -> (String, T)
where
    F: for<'a> FnMut(&'a Node, Vec<(String, T)>, &[u8]) -> T,
{
    // Use unit type as context
    bottomup_traverse_concrete_tree(
        cursor,
        &mut |node, children, input, _ctx: &()| visitor(node, children, input),
        input_bytes,
        &(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    // Basic tests to ensure the traversal functions compile and work
    // More comprehensive tests would require a tree-sitter grammar

    #[test]
    fn test_traverse_phase_ordering() {
        assert!(TraversePhase::Enter < TraversePhase::Exit);
    }
}
