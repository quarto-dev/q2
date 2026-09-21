//! mitex-lexer integration test binary.
//!
//! q2 local patch: upstream ships `tests/expand_macro.rs` as its own binary;
//! this tree keeps one integration binary per crate
//! (see `.claude/rules/integration-tests.md`).

#[allow(missing_docs)]
pub mod common;

pub mod expand_macro;
