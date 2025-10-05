/*
 * traversals.rs
 *
 * Copyright (c) 2025 Posit, PBC
 *
 * traversal helpers for tree-sitter-qmd's MarkdownCursor.
 *
 * We can't use tree-sitter walking APIs directly because it involves
 * a mix of block and inline parsers that are handled by
 * two separate tree-sitter grammars.
 *
 */

use crate::pandoc::ast_context::ASTContext;

#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub enum TraversePhase {
    Enter,
    Exit,
}

pub fn topdown_traverse_concrete_tree<F>(
    cursor: &mut tree_sitter_qmd::MarkdownCursor,
    visitor: &mut F,
) where
    F: for<'a> FnMut(&'a tree_sitter::Node, TraversePhase) -> bool,
{
    let mut stack: Vec<usize> = vec![0];
    while stack.len() > 0 {
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

#[derive(Debug)]
pub enum BottomUpTraversePhase<'a, T: std::fmt::Debug> {
    Enter(tree_sitter::Node<'a>),
    GoToSiblings(tree_sitter::Node<'a>, Vec<(String, T)>), // accumulated children
    Exit(tree_sitter::Node<'a>),
}

pub fn bottomup_traverse_concrete_tree<F, T: std::fmt::Debug>(
    cursor: &mut tree_sitter_qmd::MarkdownCursor,
    visitor: &mut F,
    input_bytes: &[u8],
    context: &ASTContext,
) -> (String, T)
where
    F: for<'a> FnMut(&'a tree_sitter::Node, Vec<(String, T)>, &[u8], &ASTContext) -> T,
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
