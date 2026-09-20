//! Vendored Quarto 1 pandoc filters embedded at compile time.

use include_dir::include_dir;

// Native-only: `bundle` materializes the embedded trees to disk via
// `tempfile` (not available on wasm32-unknown-unknown, no real filesystem),
// and `harness` shells out to a real `pandoc` subprocess via
// `std::process::Command` (not meaningful in the WASM hub-client preview).
// Neither has a legitimate WASM use case — this is test/dev-harness
// infrastructure only. Mirrors the `embedded` module gate in
// `crate::resources`.
#[cfg(not(target_arch = "wasm32"))]
pub mod bundle;
mod crossref_params;
#[cfg(not(target_arch = "wasm32"))]
pub mod diagnostics;
pub mod format_defaults;
#[cfg(not(target_arch = "wasm32"))]
pub mod harness;
pub mod meta_coerce;
pub mod params;
pub mod params_codec;
pub mod version;

pub const QUARTO_CLI_PIN: &str = "v1.11.3";
pub const PANDOC_PIN: &str = "3.10";

pub static FILTERS_DIR: include_dir::Dir =
    include_dir!("$CARGO_MANIFEST_DIR/../../resources/pandoc-filters/filters");
pub static DATADIR_DIR: include_dir::Dir =
    include_dir!("$CARGO_MANIFEST_DIR/../../resources/pandoc-filters/pandoc/datadir");

/// Static per-format `--include-in-header` resources (currently just the
/// epub follow-on's two CSS files), vendored from Q1's
/// `src/resources/formats/` per the External Sources Policy.
pub static FORMATS_DIR: include_dir::Dir =
    include_dir!("$CARGO_MANIFEST_DIR/../../resources/formats");
/// The 5 docx callout-icon PNGs (P7 Task 4), vendored from
/// `v1.11.3:src/resources/formats/docx/` — outside P4's traced
/// `src/resources/filters/` vendoring closure, so extracted into the share
/// tree separately by [`bundle::extract_share_tree`].
pub static FORMATS_DOCX_DIR: include_dir::Dir =
    include_dir!("$CARGO_MANIFEST_DIR/../../resources/formats/docx");
