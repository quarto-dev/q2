//! The `PandocRequest` seam: everything a pandoc run needs, as data.
//!
//! `PandocWriteStage::prepare()` builds a [`PandocRequest`]; the native
//! `execute()` runs it with `Command`, and the wasm host runs it in a worker.
//! The wire contract lives in `schemas/pandoc-request.schema.json`.

pub mod args;
pub mod constants;
pub mod formats;
pub mod mounts;
pub mod options;
pub mod path;
pub mod render;
pub mod resources;
pub mod share;
pub mod types;
pub mod typst_pdf;

pub use args::PandocArg;
pub use constants::constants;
pub use mounts::validate_mounts;
pub use options::PrepareOptions;
pub use path::{normalize_request_path, normalize_request_path_str};
pub use resources::{ResourceCollector, ResourceKind};
pub use share::{share_tree_entries, share_tree_version};
pub use types::{PandocRequest, REQUEST_SCHEMA_VERSION, RequestFile, RequestKind, RequestPost};
pub use typst_pdf::{typst_asset_entries, typst_assets_version, typst_date_prelude};
