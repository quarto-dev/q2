//! Materializes the embedded Q1 filter trees to disk in the single-root
//! layout `init.lua` requires: `<share>/filters/` and
//! `<share>/pandoc/datadir/` as siblings two levels apart.
//!
//! `init.lua:257` builds the filter search path as
//! `pandoc.path.normalize(PANDOC_STATE.user_data_dir .. '/../../filters/?.lua')`.
//! `PANDOC_STATE.user_data_dir` is set by pandoc's `--data-dir` flag, so the
//! data dir and the filters dir must share a common parent two levels above
//! the data dir — i.e. exactly the layout extracted here. Two independent
//! `ResourceBundle`s (each with its own temp root) cannot produce this
//! layout, so this module extracts both embedded `Dir`s directly into
//! subdirectories of one caller-provided destination.
//!
//! Extraction re-runs on every call (measured: ~27ms for the 247-file tree,
//! negligible next to a pandoc subprocess spawn) rather than caching in a
//! process-global `static`, which — unlike `StageContext::temp_dir`'s
//! instance-scoped `OnceLock` (`crate::stage::context`) — would never run
//! `Drop` for its `TempDir` at process exit and leaked one directory per
//! process that reached it (one per `q2 render --to docx`, one per `L`-tier
//! test process under nextest). Callers own the destination's lifecycle —
//! `PandocWriteStage` extracts into `ctx.temp_dir()`, which the pipeline
//! already cleans up.
//!
//! Tests: `crates/quarto-core/tests/integration/pandoc_transport.rs`
//! (T2.1, T2.5).

use std::path::Path;

use super::{
    DATADIR_DIR, FILTERS_DIR, FORMATS_DIR, FORMATS_DOCX_DIR, TYPST_PACKAGES_DIR, TYPST_TEMPLATE_DIR,
};
use crate::resources::ResourceError;

/// Extracts the embedded Q1 filter trees into `dest`, creating
/// `dest/filters/`, `dest/pandoc/datadir/`, and `dest/formats/docx/`.
/// `dest` must already exist.
pub fn extract_share_tree(dest: &Path) -> Result<(), ResourceError> {
    let filters_dest = dest.join("filters");
    let datadir_dest = dest.join("pandoc").join("datadir");
    let formats_docx_dest = dest.join("formats").join("docx");

    // `Dir::extract` requires its destination to already exist (it only
    // creates directories for nested entries, not the base path itself).
    std::fs::create_dir_all(&filters_dest).map_err(|e| ResourceError::Extract(e.to_string()))?;
    std::fs::create_dir_all(&datadir_dest).map_err(|e| ResourceError::Extract(e.to_string()))?;
    std::fs::create_dir_all(&formats_docx_dest)
        .map_err(|e| ResourceError::Extract(e.to_string()))?;

    FILTERS_DIR
        .extract(&filters_dest)
        .map_err(|e| ResourceError::Extract(e.to_string()))?;
    DATADIR_DIR
        .extract(&datadir_dest)
        .map_err(|e| ResourceError::Extract(e.to_string()))?;
    FORMATS_DOCX_DIR
        .extract(&formats_docx_dest)
        .map_err(|e| ResourceError::Extract(e.to_string()))?;

    Ok(())
}

/// Extracts the embedded `resources/formats/` tree (per-format
/// `--include-in-header` CSS, currently just epub's two files) into
/// `dest/formats/`. `dest` must already exist.
///
/// No `init.lua`-shaped layout constraint here (unlike
/// [`extract_share_tree`]) — this is a plain resource tree consumed by
/// CLI flag paths, not by the Lua filter search path.
pub fn extract_formats_tree(dest: &Path) -> Result<(), ResourceError> {
    let formats_dest = dest.join("formats");
    std::fs::create_dir_all(&formats_dest).map_err(|e| ResourceError::Extract(e.to_string()))?;
    FORMATS_DIR
        .extract(&formats_dest)
        .map_err(|e| ResourceError::Extract(e.to_string()))?;
    Ok(())
}

/// Extracts the 8 vendored typst doctemplate partials into `dest`, flat
/// (no subdirectory) — Pandoc's `--template` flag takes the orchestrator
/// (`template.typ`) and resolves each `$partial.typ()$` call relative to
/// that file's own directory, so every partial must be a sibling of it.
/// Independent of [`extract_share_tree`]'s `<share>/filters/` +
/// `<share>/pandoc/datadir/` layout (verified against `pandoc_write.rs`'s
/// invocation: `--template` and `--data-dir`/`-L` are unrelated pandoc
/// mechanisms with no positional constraint between them) — callers may
/// stage this anywhere, e.g. a sibling temp directory.
///
/// `dest` must already exist.
pub fn extract_typst_template(dest: &Path) -> Result<(), ResourceError> {
    TYPST_TEMPLATE_DIR
        .extract(dest)
        .map_err(|e| ResourceError::Extract(e.to_string()))
}

/// Extracts the vendored typst packages + fonts into `dest`, producing
/// `dest/packages/preview/<name>/<version>/` and `dest/fonts/` — the exact
/// layout a real typst package cache and `--font-path` argument expect
/// (pandoc-hybrid-typst Phase 2's compile step). `dest` must already exist.
///
/// Unconditional, full staging (all 5 vendored packages every render) —
/// simpler than `typst-gather`'s selective per-document analysis, and
/// correct for any document that only references Quarto's bundled
/// packages. Selective staging for arbitrary user-referenced `@preview`
/// packages beyond the vendored 5 is `typst-gather` integration, tracked
/// separately (see the plan's Phase 2 package-staging bullet).
pub fn extract_typst_packages(dest: &Path) -> Result<(), ResourceError> {
    TYPST_PACKAGES_DIR
        .extract(dest)
        .map_err(|e| ResourceError::Extract(e.to_string()))
}
