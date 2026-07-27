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

use std::path::{Path, PathBuf};

use crate::{Brand, BrandLogoResource};

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

    /// The prefix that turns a path written inside this brand into one
    /// written relative to `project_dir`.
    ///
    /// This is Quarto 1's `relative(this.projectDir, this.brandDir)`
    /// (`external-sources/quarto-cli/src/core/brand/brand.ts:248`),
    /// computed on demand rather than stored on the brand. Empty when
    /// the brand has no directory of its own (an inline `brand:` block,
    /// whose paths are already written relative to the project) or when
    /// the two directories cannot be related — distinct Windows volumes,
    /// say — in which case leaving the path alone beats emitting an
    /// absolute one into HTML.
    pub fn path_prefix_relative_to(&self, project_dir: &Path) -> PathBuf {
        self.dir
            .as_deref()
            .and_then(|brand_dir| pathdiff::diff_paths(brand_dir, project_dir))
            .unwrap_or_default()
    }

    /// Rewrite a logo resource's path to be relative to `project_dir`,
    /// preserving its alt text.
    ///
    /// Q1 rewrites every logo path eagerly at construction, so its
    /// `Brand` always carries project-relative paths. Q2 keeps [`Brand`]
    /// a faithful mirror of the YAML and rewrites on demand, here — so
    /// the brand stays comparable and cacheable regardless of where it
    /// was loaded from.
    ///
    /// URLs and rooted paths pass through untouched; see
    /// [`BrandLogoResource::with_path_relative_to`], which owns that
    /// rule for every logo consumer.
    ///
    /// Named logos are `small` / `medium` / `large`; extra images live
    /// under `logo.images.*` and are reached with
    /// [`Brand::logo_image`]. Returns `None` when the name resolves to
    /// nothing, or to a light/dark pair (which has no single path).
    pub fn logo_resource_relative_to(
        &self,
        name: &str,
        project_dir: &Path,
    ) -> Option<BrandLogoResource> {
        let prefix = self.path_prefix_relative_to(project_dir);
        self.brand
            .logo(name)
            .and_then(|entry| entry.single())
            .or_else(|| self.brand.logo_image(name))
            .map(|resource| resource.with_path_relative_to(&prefix))
    }

    /// Path of this brand's favicon, relative to `project_dir`.
    ///
    /// The favicon is the *small* logo, mirroring Q1's `getFavicon`.
    /// Returns `None` when no small logo is configured, or when it is a
    /// light/dark pair — [`Brand::favicon`] declines to pick a side, and
    /// choosing one is deferred to the light/dark work (bd-v5z8w).
    ///
    /// The result may be a URL rather than a path. Callers that copy the
    /// file must check with [`quarto_util::is_external_url`] first.
    pub fn favicon_relative_to(&self, project_dir: &Path) -> Option<String> {
        let prefix = self.path_prefix_relative_to(project_dir);
        self.brand.favicon().map(|path| {
            BrandLogoResource::Path(path.to_string())
                .with_path_relative_to(&prefix)
                .path()
                .to_string()
        })
    }
}
