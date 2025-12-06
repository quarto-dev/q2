/*
 * concrete_tree_depth.rs
 * Copyright (c) 2025 Posit, PBC
 */
use crate::traversals;
use tree_sitter_qmd::MarkdownTree;

pub fn concrete_tree_depth(tree: &MarkdownTree) -> usize {
    let mut this_depth = 1;
    let mut max_depth = 1;
    crate::traversals::topdown_traverse_concrete_tree(&mut tree.walk(), &mut |_, phase| {
        if phase == traversals::TraversePhase::Enter {
            this_depth += 1;
            if this_depth > max_depth {
                max_depth = this_depth;
            }
        } else {
            this_depth -= 1;
        }
        true // continue traversing
    });
    max_depth
}
