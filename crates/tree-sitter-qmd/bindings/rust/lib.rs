/*
 * lib.rs
 * Copyright (c) 2025 Posit, PBC
 */

//! This crate provides Quarto Markdown language support for the [tree-sitter][] parsing library.
//!
//! It contains a unified grammar ([`LANGUAGE`]) that parses both the block structure and inline
//! content of markdown documents in a single parse tree.
//!
//! It supplies [`MarkdownParser`] as a convenience wrapper around the grammar.
//! [`MarkdownParser::parse`] returns a [`MarkdownTree`] which contains the parsed syntax tree.
//!
//! [LanguageFn]: https://docs.rs/tree-sitter-language/*/tree_sitter_language/struct.LanguageFn.html
//! [Tree]: https://docs.rs/tree-sitter/*/tree_sitter/struct.Tree.html
//! [tree-sitter]: https://tree-sitter.github.io/

#![cfg_attr(docsrs, feature(doc_cfg))]

use tree_sitter_language::LanguageFn;

unsafe extern "C" {
    fn tree_sitter_markdown() -> *const ();
}

/// The tree-sitter [`LanguageFn`][LanguageFn] for the unified markdown grammar.
///
/// This grammar handles both block structure and inline content in a single parse tree.
pub const LANGUAGE: LanguageFn = unsafe { LanguageFn::from_raw(tree_sitter_markdown) };

/// The syntax highlighting queries for the markdown grammar.
pub const HIGHLIGHT_QUERY: &str =
    include_str!("../../tree-sitter-markdown/queries/highlights.scm");

/// The language injection queries for the markdown grammar.
pub const INJECTION_QUERY: &str =
    include_str!("../../tree-sitter-markdown/queries/injections.scm");

/// The content of the [`node-types.json`][] file for the markdown grammar.
///
/// [`node-types.json`]: https://tree-sitter.github.io/tree-sitter/using-parsers#static-node-types
pub const NODE_TYPES: &str = include_str!("../../tree-sitter-markdown/src/node-types.json");

mod parser;

pub use parser::*;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn can_load_grammar() {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&LANGUAGE.into())
            .expect("Error loading Markdown grammar");
    }
}
