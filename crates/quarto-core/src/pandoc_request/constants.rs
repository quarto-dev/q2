//! `resources/pandoc-wasm.json`, the one constants file shared with TS.

use std::sync::OnceLock;

use serde::Deserialize;

const RAW: &str = include_str!("../../../../resources/pandoc-wasm.json");

#[derive(Debug, Deserialize)]
pub struct PandocWasmConstants {
    /// SHA-256 of the decompressed pandoc.wasm.
    pub wasm_sha256: String,
    /// The deterministic share root used on wasm.
    pub share_root: String,
}

pub fn constants() -> &'static PandocWasmConstants {
    static CONSTANTS: OnceLock<PandocWasmConstants> = OnceLock::new();
    CONSTANTS.get_or_init(|| {
        serde_json::from_str(RAW).expect("resources/pandoc-wasm.json must be valid")
    })
}
