/*
 * resolved.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * A parsed Brand together with the directory it was read from.
 */

//! [`ResolvedBrand`] — a [`Brand`] plus the directory its definition
//! came from.
//!
//! Paths *inside* a `_brand.yml` (logos, font files) are written
//! relative to that file's own directory, so a `Brand` on its own is
//! not enough to locate them. Quarto 1 solves this by storing
//! `brandDir` / `projectDir` on the `Brand` object itself and eagerly
//! rewriting every path at construction time (`Brand.resolvePath` in
//! `external-sources/quarto-cli/src/core/brand/brand.ts:247`).
//!
//! Q2 keeps [`Brand`] a pure, path-agnostic mirror of the YAML and
//! puts the directory on the *resolution result* instead. The brand
//! itself stays comparable and cacheable regardless of where it was
//! loaded from; consumers that need a usable path ask
//! [`ResolvedBrand`] for one.
//!
//! This is also the seam future logo consumers share — the navbar
//! brand image (bd-hp3tx) needs exactly the same rebasing the favicon
//! fallback (bd-97yc) does.

use std::path::PathBuf;

use crate::Brand;

/// A [`Brand`] together with the directory its definition came from.
///
/// `dir` is the directory containing the `_brand.yml` that produced
/// `brand`, and is the base that relative paths inside the brand are
/// written against. It is `None` for an inline `brand:` block, which
/// has no file of its own — consumers should treat that as "relative
/// to the project root", matching how an inline block is authored in
/// `_quarto.yml`.
#[derive(Debug, Clone)]
pub struct ResolvedBrand {
    /// The parsed brand.
    pub brand: Brand,
    /// Directory the brand file was read from; `None` for inline blocks.
    pub dir: Option<PathBuf>,
}

impl ResolvedBrand {
    /// Construct from a brand and the directory it was read from.
    pub fn new(brand: Brand, dir: Option<PathBuf>) -> Self {
        Self { brand, dir }
    }
}
