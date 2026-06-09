/*
 * revealjs/mod.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * format: revealjs presentation support.
 */

//! `format: revealjs` support: slide construction + standalone document
//! assembly.
//!
//! This module is the **native render** side of the revealjs epic. The slide
//! construction ([`slides`] / [`RevealSlidesTransform`]) is the shared
//! contract the hub-client preview will later consume (it is WASM-compatible:
//! pure AST manipulation, no native-only deps). Document assembly
//! ([`render_revealjs_document`]) wraps the rendered slide body in a
//! self-contained reveal.js scaffold.
//!
//! See `claude-notes/plans/2026-06-08-revealjs-presentations.md` (Phase 1).

mod assemble;
mod columns;
mod footnotes;
mod slides;
mod transform;

pub use assemble::render_revealjs_document;
pub use columns::RevealColumnsTransform;
pub use footnotes::RevealFootnotesTransform;
pub use slides::{DEFAULT_SLIDE_LEVEL, build_reveal_slides};
pub use transform::RevealSlidesTransform;
