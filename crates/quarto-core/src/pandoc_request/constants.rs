//! `resources/pandoc-wasm.json`, the one constants file shared with TS.

use std::sync::OnceLock;

use serde::Deserialize;

const RAW: &str = include_str!("../../../../resources/pandoc-wasm.json");

/// The mount size limits (the host enforces the same numbers).
#[derive(Debug, Clone, Copy, Deserialize)]
pub struct PandocWasmLimits {
    /// Each image (a mounted file with an image extension).
    pub image_bytes: u64,
    /// The `--reference-doc` file.
    pub reference_doc_bytes: u64,
    /// Every mounted byte: share tree, `files` and `resource_refs`.
    pub total_bytes: u64,
    /// Each file collected from a `collect_dirs` entry; a larger one is dropped with a warning.
    pub collected_file_bytes: u64,
    /// All collected files together; the one that would pass it is dropped with a warning.
    pub collected_total_bytes: u64,
}

#[derive(Debug, Deserialize)]
pub struct PandocWasmConstants {
    /// SHA-256 of the decompressed pandoc.wasm.
    pub wasm_sha256: String,
    /// The deterministic share root used on wasm.
    pub share_root: String,
    pub limits: PandocWasmLimits,
}

pub fn constants() -> &'static PandocWasmConstants {
    static CONSTANTS: OnceLock<PandocWasmConstants> = OnceLock::new();
    CONSTANTS.get_or_init(|| {
        serde_json::from_str(RAW).expect("resources/pandoc-wasm.json must be valid")
    })
}
