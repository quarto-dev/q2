/*
 * parser.rs
 * Copyright (c) 2025 Posit, PBC
 */

use std::num::NonZeroU16;

use crate::LANGUAGE;
use tree_sitter::{InputEdit, Language, Node, Parser, Point, Tree, TreeCursor};

/// A parser that produces [`MarkdownTree`]s.
///
/// This is a convenience wrapper around the unified [`LANGUAGE`] grammar.
pub struct MarkdownParser {
    pub parser: Parser,
    block_language: Language,
}

/// A stateful object for walking a [`MarkdownTree`] efficiently.
///
/// This is a thin wrapper around [`TreeCursor`] for the unified markdown tree.
///
/// Note: This type exists for backwards compatibility. For new code using
/// `quarto-treesitter-ast` traversals, use [`MarkdownTree::walk_cursor`] to get
/// a raw [`TreeCursor`] directly.
pub struct MarkdownCursor<'a> {
    block_cursor: TreeCursor<'a>,
}

impl<'a> MarkdownCursor<'a> {
    /// Unwrap this into the underlying [`TreeCursor`].
    pub fn into_inner(self) -> TreeCursor<'a> {
        self.block_cursor
    }

    /// Get a reference to the underlying [`TreeCursor`].
    pub fn as_cursor(&self) -> &TreeCursor<'a> {
        &self.block_cursor
    }

    /// Get a mutable reference to the underlying [`TreeCursor`].
    pub fn as_cursor_mut(&mut self) -> &mut TreeCursor<'a> {
        &mut self.block_cursor
    }
}

impl<'a> MarkdownCursor<'a> {
    /// Get the cursor's current [`Node`].
    pub fn node(&self) -> Node<'a> {
        self.block_cursor.node()
    }

    /// Get the numerical field id of this tree cursor’s current node.
    ///
    /// See also [`field_name`](Self::field_name).
    pub fn field_id(&self) -> Option<NonZeroU16> {
        self.block_cursor.field_id()
    }

    /// Get the field name of this tree cursor’s current node.
    pub fn field_name(&self) -> Option<&'static str> {
        self.block_cursor.field_name()
    }

    /// Move this cursor to the first child of its current node.
    ///
    /// This returns `true` if the cursor successfully moved, and returns `false` if there were no
    /// children.
    pub fn goto_first_child(&mut self) -> bool {
        self.block_cursor.goto_first_child()
    }

    /// Move this cursor to the parent of its current node.
    ///
    /// This returns true if the cursor successfully moved, and returns false if there was no
    /// parent node (the cursor was already on the root node).
    pub fn goto_parent(&mut self) -> bool {
        self.block_cursor.goto_parent()
    }

    pub fn goto_top(&mut self) {
        loop {
            if !self.goto_parent() {
                break;
            }
        }
    }

    /// Move this cursor to the next sibling of its current node.
    ///
    /// This returns true if the cursor successfully moved, and returns false if there was no next
    /// sibling node.
    pub fn goto_next_sibling(&mut self) -> bool {
        self.block_cursor.goto_next_sibling()
    }

    /// Move this cursor to the first child of its current node that extends beyond the given byte offset.
    ///
    /// This returns the index of the child node if one was found, and returns None if no such child was found.
    pub fn goto_first_child_for_byte(&mut self, index: usize) -> Option<usize> {
        self.block_cursor.goto_first_child_for_byte(index)
    }

    /// Move this cursor to the first child of its current node that extends beyond the given point.
    ///
    /// This returns the index of the child node if one was found, and returns None if no such child was found.
    pub fn goto_first_child_for_point(&mut self, index: Point) -> Option<usize> {
        self.block_cursor.goto_first_child_for_point(index)
    }

    pub fn id(&self) -> (bool, usize) {
        (false, self.block_cursor.node().id())
    }

    fn inner_goto_id(&mut self, id: (bool, usize)) -> bool {
        if self.id() == id {
            return true;
        }
        if self.goto_first_child() {
            if self.inner_goto_id(id) {
                return true;
            }
            loop {
                if !self.goto_next_sibling() {
                    break;
                }
                if self.inner_goto_id(id) {
                    return true;
                }
            }
            self.goto_parent();
        }
        false
    }

    /// Move this cursor to the node with the given id.
    ///
    /// takes time O(n) to find the node, where n is the number of nodes in the tree.
    pub fn goto_id(&mut self, id: (bool, usize)) -> bool {
        self.goto_top();
        return self.inner_goto_id(id);
    }
}

/// An object that holds a combined markdown tree.
#[derive(Debug, Clone)]
pub struct MarkdownTree {
    block_tree: Tree,
}

impl MarkdownTree {
    /// Edit the block tree and inline trees to keep them in sync with source code that has been
    /// edited.
    ///
    /// You must describe the edit both in terms of byte offsets and in terms of
    /// row/column coordinates.
    pub fn edit(&mut self, edit: &InputEdit) {
        self.block_tree.edit(edit);
    }

    /// Returns the block tree for the parsed document
    pub fn block_tree(&self) -> &Tree {
        &self.block_tree
    }

    /// Create a new [`MarkdownCursor`] starting from the root of the tree.
    ///
    /// For new code using `quarto-treesitter-ast` traversals, prefer [`walk_cursor`](Self::walk_cursor)
    /// which returns a raw [`TreeCursor`] directly.
    pub fn walk(&self) -> MarkdownCursor<'_> {
        MarkdownCursor {
            block_cursor: self.block_tree.walk(),
        }
    }

    /// Create a new [`TreeCursor`] starting from the root of the tree.
    ///
    /// This returns the raw tree-sitter cursor directly, suitable for use with
    /// generic traversal functions from `quarto-treesitter-ast`.
    pub fn walk_cursor(&self) -> TreeCursor<'_> {
        self.block_tree.walk()
    }
}

impl Default for MarkdownParser {
    fn default() -> Self {
        let block_language = LANGUAGE.into();
        let parser = Parser::new();
        MarkdownParser {
            parser,
            block_language,
        }
    }
}

impl MarkdownParser {
    /// Parse a slice of UTF8 text.
    ///
    /// # Arguments:
    /// * `text` The UTF8-encoded text to parse.
    /// * `old_tree` A previous syntax tree parsed from the same document.
    ///   If the text of the document has changed since `old_tree` was
    ///   created, then you must edit `old_tree` to match the new text using
    ///   [MarkdownTree::edit].
    ///
    /// Returns a [MarkdownTree] if parsing succeeded, or `None` if:
    ///  * The timeout set with [tree_sitter::Parser::set_timeout_micros] expired
    ///  * The cancellation flag set with [tree_sitter::Parser::set_cancellation_flag] was flipped
    pub fn parse_with<T: AsRef<[u8]>, F: FnMut(usize, Point) -> T>(
        &mut self,
        callback: &mut F,
        old_tree: Option<&MarkdownTree>,
    ) -> Option<MarkdownTree> {
        let MarkdownParser {
            parser,
            block_language,
        } = self;
        parser
            .set_included_ranges(&[])
            .expect("Can not set included ranges to whole document");
        parser
            .set_language(block_language)
            .expect("Could not load block grammar");
        let block_tree =
            parser.parse_with_options(callback, old_tree.map(|tree| &tree.block_tree), None)?;
        Some(MarkdownTree { block_tree })
    }

    /// Parse a slice of UTF8 text.
    ///
    /// # Arguments:
    /// * `text` The UTF8-encoded text to parse.
    /// * `old_tree` A previous syntax tree parsed from the same document.
    ///   If the text of the document has changed since `old_tree` was
    ///   created, then you must edit `old_tree` to match the new text using
    ///   [MarkdownTree::edit].
    ///
    /// Returns a [MarkdownTree] if parsing succeeded, or `None` if:
    ///  * The timeout set with [tree_sitter::Parser::set_timeout_micros] expired
    ///  * The cancellation flag set with [tree_sitter::Parser::set_cancellation_flag] was flipped
    pub fn parse(&mut self, text: &[u8], old_tree: Option<&MarkdownTree>) -> Option<MarkdownTree> {
        self.parse_with(&mut |byte, _| &text[byte..], old_tree)
    }
}
