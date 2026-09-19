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

use super::{DATADIR_DIR, FILTERS_DIR};
use crate::resources::ResourceError;

/// Extracts the embedded Q1 filter trees into `dest`, creating
/// `dest/filters/` and `dest/pandoc/datadir/`. `dest` must already exist.
pub fn extract_share_tree(dest: &Path) -> Result<(), ResourceError> {
    let filters_dest = dest.join("filters");
    let datadir_dest = dest.join("pandoc").join("datadir");

    // `Dir::extract` requires its destination to already exist (it only
    // creates directories for nested entries, not the base path itself).
    std::fs::create_dir_all(&filters_dest).map_err(|e| ResourceError::Extract(e.to_string()))?;
    std::fs::create_dir_all(&datadir_dest).map_err(|e| ResourceError::Extract(e.to_string()))?;

    FILTERS_DIR
        .extract(&filters_dest)
        .map_err(|e| ResourceError::Extract(e.to_string()))?;
    DATADIR_DIR
        .extract(&datadir_dest)
        .map_err(|e| ResourceError::Extract(e.to_string()))?;

    Ok(())
}
