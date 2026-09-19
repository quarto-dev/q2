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
    match bottomup_traverse_concrete_tree_with_depth_limit(
        cursor,
        visitor,
        input_bytes,
        context,
        usize::MAX,
    ) {
        Ok(result) => result,
        Err(DepthLimitExceeded { .. }) => {
            unreachable!("no tree is usize::MAX nodes deep")
        }
    }
}

/// Returned by [`bottomup_traverse_concrete_tree_with_depth_limit`] when the
/// tree has a node deeper than `max_depth`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DepthLimitExceeded {
    /// The limit that was exceeded.
    pub max_depth: usize,
}

/// Bottom-up traversal that gives up on trees deeper than `max_depth`.
///
/// Behaves exactly like [`bottomup_traverse_concrete_tree`], except that it
/// returns [`DepthLimitExceeded`] as soon as it reaches a node whose depth
/// (counting the node the cursor starts on as 1) is greater than
/// `max_depth`. The check happens when a node is *entered*, which is
/// top-down, so the visitor never runs on a node deeper than `max_depth`.
/// That lets callers use the traversal itself as a guard against inputs
/// whose results would be too deeply nested for recursive code downstream,
/// without walking the tree a second time.
pub fn bottomup_traverse_concrete_tree_with_depth_limit<F, T: std::fmt::Debug, C>(
    cursor: &mut TreeCursor,
    visitor: &mut F,
    input_bytes: &[u8],
    context: &C,
    max_depth: usize,
) -> Result<(String, T), DepthLimitExceeded>
where
    F: for<'a> FnMut(&'a Node, Vec<(String, T)>, &[u8], &C) -> T,
{
    let mut stack: Vec<BottomUpTraversePhase<T>> =
        vec![BottomUpTraversePhase::Enter(cursor.node())];
    // Depth of the innermost node that has been entered but not exited,
    // i.e. the number of `GoToSiblings` frames on the stack.
    let mut depth: usize = 0;

    loop {
        let top = stack.pop().unwrap();
        match top {
            BottomUpTraversePhase::Enter(node) => {
                if depth >= max_depth {
                    return Err(DepthLimitExceeded { max_depth });
                }
                depth += 1;
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
                depth -= 1;
                let (kind, result) = (
                    node.kind().to_string(),
                    visitor(&node, children, input_bytes, context),
                );
                match stack.last_mut() {
                    None => return Ok((kind, result)), // we are done
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

    fn parse(input: &str) -> tree_sitter_qmd::MarkdownTree {
        tree_sitter_qmd::MarkdownParser::default()
            .parse(input.as_bytes(), None)
            .expect("parse")
    }

    /// Nested bracketed spans nest to any depth without a parse error.
    fn nested_spans(levels: usize) -> String {
        format!("{}x{}\n", "[".repeat(levels), "]{.c}".repeat(levels))
    }

    /// Depth of `node` counting the root as 1.
    fn node_depth(node: &Node) -> usize {
        let mut depth = 1;
        let mut current = *node;
        while let Some(parent) = current.parent() {
            depth += 1;
            current = parent;
        }
        depth
    }

    /// Visitor computing the depth of the subtree below each node, so that
    /// the root's result is the depth of the whole tree (root = 1).
    fn subtree_depth(
        _node: &Node,
        children: Vec<(String, usize)>,
        _input: &[u8],
        _ctx: &(),
    ) -> usize {
        1 + children.iter().map(|(_, d)| *d).max().unwrap_or(0)
    }

    fn limited_tree_depth(
        tree: &tree_sitter_qmd::MarkdownTree,
        max_depth: usize,
    ) -> Result<usize, DepthLimitExceeded> {
        bottomup_traverse_concrete_tree_with_depth_limit(
            &mut tree.walk_cursor(),
            &mut subtree_depth,
            b"",
            &(),
            max_depth,
        )
        .map(|(_kind, depth)| depth)
    }

    #[test]
    fn test_depth_limit_accepts_tree_exactly_at_the_limit() {
        let tree = parse(&nested_spans(10));
        let (_, depth) =
            bottomup_traverse_concrete_tree(&mut tree.walk_cursor(), &mut subtree_depth, b"", &());
        assert!(depth > 10, "fixture should be reasonably deep, got {depth}");
        assert_eq!(limited_tree_depth(&tree, depth), Ok(depth));
        assert_eq!(limited_tree_depth(&tree, usize::MAX), Ok(depth));
    }

    #[test]
    fn test_depth_limit_rejects_tree_one_level_too_deep() {
        let tree = parse(&nested_spans(10));
        let (_, depth) =
            bottomup_traverse_concrete_tree(&mut tree.walk_cursor(), &mut subtree_depth, b"", &());
        assert_eq!(
            limited_tree_depth(&tree, depth - 1),
            Err(DepthLimitExceeded {
                max_depth: depth - 1
            })
        );
    }

    #[test]
    fn test_depth_limit_stops_before_visiting_anything_too_deep() {
        let tree = parse(&nested_spans(50));
        let max_depth = 20;
        let mut deepest_visited = 0;
        let result = bottomup_traverse_concrete_tree_with_depth_limit(
            &mut tree.walk_cursor(),
            &mut |node: &Node, _children: Vec<(String, ())>, _input: &[u8], _ctx: &()| {
                deepest_visited = deepest_visited.max(node_depth(node));
            },
            b"",
            &(),
            max_depth,
        );
        assert_eq!(result.unwrap_err(), DepthLimitExceeded { max_depth });
        assert!(
            deepest_visited <= max_depth,
            "visitor ran on a node at depth {deepest_visited} > {max_depth}"
        );
    }

    #[test]
    fn test_depth_limit_produces_same_result_as_unlimited_traversal() {
        let input = "# Title\n\nSome *emphasis* and [a span]{.c}.\n\n- a\n- b\n";
        let tree = parse(input);
        let mut kinds = |node: &Node, children: Vec<(String, String)>, _input: &[u8], _ctx: &()| {
            let inner: Vec<String> = children.into_iter().map(|(_, s)| s).collect();
            format!("{}({})", node.kind(), inner.join(","))
        };
        let unlimited =
            bottomup_traverse_concrete_tree(&mut tree.walk_cursor(), &mut kinds, b"", &());
        let limited = bottomup_traverse_concrete_tree_with_depth_limit(
            &mut tree.walk_cursor(),
            &mut kinds,
            b"",
            &(),
            usize::MAX,
        );
        assert_eq!(limited, Ok(unlimited));
    }
}
