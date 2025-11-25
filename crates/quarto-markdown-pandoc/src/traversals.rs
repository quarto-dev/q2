/*
 * traversals.rs
 *
 * Copyright (c) 2025 Posit, PBC
 *
 * Re-exports generic traversal utilities from quarto-treesitter-ast.
 *
 * This module provides traversal functions for working with tree-sitter parse trees.
 * The implementations are now shared via the quarto-treesitter-ast crate, which allows
 * other parsers (like the template parser) to reuse the same traversal logic.
 *
 * Historical note: The MarkdownCursor wrapper previously existed because qmd used
 * separate block and inline parsers. Now that the grammar is unified, MarkdownCursor
 * is vestigial and these traversals work with raw TreeCursor.
 */

use crate::pandoc::ast_context::ASTContext;

// Re-export types from the shared crate
pub use quarto_treesitter_ast::TraversePhase;

/// Top-down traversal of a tree-sitter tree via MarkdownCursor.
///
/// This is a thin wrapper around the generic traversal that accepts
/// a MarkdownCursor for backwards compatibility.
///
/// For new code, consider using `quarto_treesitter_ast::topdown_traverse_concrete_tree`
/// directly with a raw `TreeCursor`.
pub fn topdown_traverse_concrete_tree<F>(
    cursor: &mut tree_sitter_qmd::MarkdownCursor,
    visitor: &mut F,
) where
    F: for<'a> FnMut(&'a tree_sitter::Node, TraversePhase) -> bool,
{
    quarto_treesitter_ast::topdown_traverse_concrete_tree(cursor.as_cursor_mut(), visitor)
}

/// Bottom-up traversal of a tree-sitter tree via MarkdownCursor.
///
/// This is a thin wrapper around the generic traversal that accepts
/// a MarkdownCursor and ASTContext for backwards compatibility.
///
/// For new code, consider using `quarto_treesitter_ast::bottomup_traverse_concrete_tree`
/// directly with a raw `TreeCursor`.
pub fn bottomup_traverse_concrete_tree<F, T: std::fmt::Debug>(
    cursor: &mut tree_sitter_qmd::MarkdownCursor,
    visitor: &mut F,
    input_bytes: &[u8],
    context: &ASTContext,
) -> (String, T)
where
    F: for<'a> FnMut(&'a tree_sitter::Node, Vec<(String, T)>, &[u8], &ASTContext) -> T,
{
    quarto_treesitter_ast::bottomup_traverse_concrete_tree(
        cursor.as_cursor_mut(),
        visitor,
        input_bytes,
        context,
    )
}
