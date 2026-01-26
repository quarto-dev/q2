/*
 * transforms/mod.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * AST transforms for pampa.
 */

//! AST transforms for pampa.
//!
//! This module provides transforms that modify the Pandoc AST before rendering.
//! Transforms are implemented as functions that take and return `Vec<Block>`,
//! enabling composition and reuse across different rendering contexts.
//!
//! ## Available Transforms
//!
//! - [`sectionize`] - Wrap headers in section Divs (analogous to Pandoc's `--section-divs`)

pub mod sectionize;

pub use sectionize::sectionize_blocks;
