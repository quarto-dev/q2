//! mitex-parser integration test binary.
//!
//! q2 local patch: upstream ships `tests/ast.rs` and `tests/properties.rs`
//! as separate binaries; this tree keeps one integration binary per crate
//! (see `.claude/rules/integration-tests.md`).

#[allow(missing_docs)]
pub mod common;

pub mod ast;
pub mod properties;
