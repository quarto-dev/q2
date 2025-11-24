/*
 * lib.rs
 * Copyright (c) 2025 Posit, PBC
 */

//! This crate provides Pandoc document template language support for the [tree-sitter][] parsing library.
//!
//! Pandoc templates use a simple syntax with `$...$` delimiters for variable interpolation,
//! conditionals, loops, and partials.
//!
//! [tree-sitter]: https://tree-sitter.github.io/

use tree_sitter_language::LanguageFn;

unsafe extern "C" {
    fn tree_sitter_doctemplate() -> *const ();
}

/// The tree-sitter [`LanguageFn`] for the document template grammar.
pub const LANGUAGE: LanguageFn = unsafe { LanguageFn::from_raw(tree_sitter_doctemplate) };

/// The content of the [`node-types.json`][] file for the template grammar.
///
/// [`node-types.json`]: https://tree-sitter.github.io/tree-sitter/using-parsers#static-node-types
pub const NODE_TYPES: &str = include_str!("../grammar/src/node-types.json");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn can_load_grammar() {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&LANGUAGE.into())
            .expect("Error loading doctemplate grammar");
    }
}
