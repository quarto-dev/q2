/*
 * lib.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! `quarto-math`: a micro-pandoc for math expressions.
//!
//! One reader (a vendored copy of mitex's LaTeX lexer + parser, under
//! `vendor/`), a q2-owned command spec, a normalization pass to a small
//! `MathAst`, and one writer per target (OMML for docx, Typst, and MathML
//! Core for HTML). Every node carries a `SourceInfo` derived from
//! the math text's own `SourceInfo`, so diagnostics point into the `.qmd`.
//!
//! The crate deliberately does **not** depend on `quarto-pandoc-types`: the
//! entry point takes the math text, a display/inline flag, and the text's
//! `SourceInfo`; pampa adapts `Inline::Math` at the call site.
//!
//! Plan: `claude-notes/plans/2026-09-21-quarto-math-and-native-docx.md`
//! (bd-entbg6x3, epic bd-pq9k90z2).

pub mod ast;
pub mod convert;
pub mod diagnostics;
pub mod mathml;
pub mod normalize;
pub mod omml;
pub mod reader;
pub mod spec;
pub mod split;
pub mod typst;
