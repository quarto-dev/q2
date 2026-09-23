/*
 * reader.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! The TeX reader: mitex's lossless CST plus a side table mapping every leaf
//! token to the byte range of the *original* input it was lexed from.
//!
//! rowan's own `text_range()` indexes the tree's text, which for macro-free
//! input equals the input. Once `\newcommand` is involved the lexer expands
//! macros and the tree text diverges from the input; the side table is what
//! lets diagnostics and downstream source maps point at the `.qmd` anyway
//! (macro bodies map to their definition, macro arguments to the use site).
//!
//! Two facts about mitex's tree that consumers must not assume away:
//!
//! - it is not byte-lossless: the whitespace between a command and a bare
//!   argument (`\hat x`) is dropped, so consecutive leaf spans may leave
//!   whitespace-only gaps;
//! - the macro engine synthesizes the text of arity-mismatch error tokens,
//!   which therefore carry no span ([`Leaf::span`] is `None`).

use std::ops::Range;

use mitex_parser::syntax::{SyntaxKind, SyntaxNode};
use rowan::NodeOrToken;

use crate::spec::Spec;

/// One leaf token of the CST with its original span.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Leaf {
    pub kind: SyntaxKind,
    pub text: String,
    /// Byte range in the original input. `None` only for error tokens whose
    /// text the macro engine synthesizes (e.g. "invalid number of
    /// arguments"); every lexed token, macro expansions included, has one.
    pub span: Option<Range<usize>>,
}

/// Result of [`parse`].
#[derive(Debug, Clone)]
pub struct Parsed {
    /// The lossless syntax tree.
    pub root: SyntaxNode,
    /// `spans[i]` is the original span of the i-th leaf in document order.
    pub spans: Vec<Option<Range<usize>>>,
    /// The leaves in document order, zipped with their spans.
    pub leaves: Vec<Leaf>,
}

impl Parsed {
    /// Number of leaf tokens in the tree.
    pub fn token_count(&self) -> usize {
        count_tokens(&self.root)
    }
}

fn count_tokens(node: &SyntaxNode) -> usize {
    node.children_with_tokens()
        .map(|c| match c {
            NodeOrToken::Node(n) => count_tokens(&n),
            NodeOrToken::Token(_) => 1,
        })
        .sum()
}

fn collect_leaves(node: &SyntaxNode, spans: &[Option<Range<usize>>], out: &mut Vec<Leaf>) {
    for child in node.children_with_tokens() {
        match child {
            NodeOrToken::Node(n) => collect_leaves(&n, spans, out),
            NodeOrToken::Token(t) => {
                let span = spans.get(out.len()).cloned().flatten();
                out.push(Leaf {
                    kind: t.kind(),
                    text: t.text().to_string(),
                    span,
                });
            }
        }
    }
}

/// Parse `text` (the body of a math node, delimiters excluded) with `spec`.
pub fn parse(text: &str, spec: &Spec) -> Parsed {
    let (root, spans) = mitex_parser::parse_with_spans(text, spec.command_spec().clone());
    let mut leaves = Vec::with_capacity(spans.len());
    collect_leaves(&root, &spans, &mut leaves);
    Parsed {
        root,
        spans,
        leaves,
    }
}
