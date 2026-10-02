//! The `PandocRequest` seam: everything a pandoc run needs, as data.
//!
//! `PandocWriteStage::prepare()` builds a [`PandocRequest`]; the native
//! `execute()` runs it with `Command`, and the wasm host runs it in a worker.
//! The wire contract lives in `schemas/pandoc-request.schema.json`.

pub mod args;
pub mod constants;
pub mod mounts;
pub mod options;
pub mod path;
pub mod share;
pub mod types;

pub use args::PandocArg;
pub use constants::constants;
pub use mounts::validate_mounts;
pub use options::PrepareOptions;
pub use path::{normalize_request_path, normalize_request_path_str};
pub use share::{share_tree_entries, share_tree_version};
pub use types::{PandocRequest, REQUEST_SCHEMA_VERSION, RequestFile, RequestKind, RequestPost};
