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
pub const HIGHLIGHT_QUERY: &str = include_str!("../../tree-sitter-markdown/queries/highlights.scm");

/// The language injection queries for the markdown grammar.
pub const INJECTION_QUERY: &str = include_str!("../../tree-sitter-markdown/queries/injections.scm");

/// The content of the [`node-types.json`][] file for the markdown grammar.
///
/// [`node-types.json`]: https://tree-sitter.github.io/tree-sitter/using-parsers#static-node-types
pub const NODE_TYPES: &str = include_str!("../../tree-sitter-markdown/src/node-types.json");

/// The WHATWG HTML entities table (`name → {codepoints, characters}`) that the
/// grammar's `entity_reference` regex is generated from (see
/// `common/common.js` `html_entity_regex()`). Exposed so consumers resolving
/// `entity_reference` nodes decode against the same table the grammar matched
/// with. Source: <https://html.spec.whatwg.org/multipage/entities.json>.
pub const HTML_ENTITIES_JSON: &str = include_str!("../../common/html_entities.json");

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

    // Builds CRLF in-process so Linux CI catches regressions — corpus
    // fixtures are checked out as LF on Linux and miss this case.
    #[test]
    fn pipe_table_crlf_matches_lf() {
        let lf = "before\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\nafter\n";
        let crlf = lf.replace('\n', "\r\n");

        let mut parser = MarkdownParser::default();
        let lf_tree = parser.parse(lf.as_bytes(), None).unwrap();
        let crlf_tree = parser.parse(crlf.as_bytes(), None).unwrap();

        assert_eq!(
            lf_tree.block_tree().root_node().to_sexp(),
            crlf_tree.block_tree().root_node().to_sexp(),
        );
    }
}
