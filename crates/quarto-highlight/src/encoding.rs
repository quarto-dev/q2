//! The wire-format types for highlight spans live in the small
//! `quarto-highlight-encoding` crate so both producers (this crate, on
//! native) and consumers (pampa's HTML writer, wasm32-safe) can share
//! the definition without pulling in wasmtime or the grammar crates.
//!
//! This module re-exports the types for convenience — `HighlightSpan`,
//! `encode`, `decode`. The encoding format is documented in the
//! shared crate.

pub use quarto_highlight_encoding::{HighlightSpan, SPANS_ATTR_KEY, decode, encode};
