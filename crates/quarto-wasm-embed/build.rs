//! Build script that locates the single render-WASM copy embedded into
//! the q2 binary.
//!
//! The q2 binary needs `wasm_quarto_hub_client_bg.wasm` (~36 MB) in two
//! places: `q2 preview` serves it to the browser, and `q2 mcp` extracts
//! it next to the MCP bundle's `wasm-host.mjs`. Both consumers read the
//! bytes from this crate so the binary carries the WASM exactly once
//! (plan `claude-notes/plans/2026-10-13-wasm-embed-dedupe.md`).
//!
//! Scan order:
//!
//! 1. `q2-preview-spa/dist/assets/wasm_quarto_hub_client_bg-*.wasm` —
//!    exactly one match. This is the file the preview's own build.rs
//!    strips from the viewer archive (same exactly-one rule), so the
//!    bytes served at the Vite-hashed path are identical to what Vite
//!    content-hashed, by construction.
//! 2. `ts-packages/quarto-hub-mcp/dist-bundle/wasm_quarto_hub_client_bg.wasm`
//!    — a bundle-only dev flow (`cargo xtask build-hub-mcp-bundle`
//!    without the SPA) still embeds the WASM once, and it is the very
//!    file the bundle's glue was built against.
//!
//! Otherwise an empty archive is embedded and `wasm_bytes()` returns
//! `None`. No cargo warning here: the consumers' placeholder paths
//! (`q2 preview`'s placeholder SPA, `q2 mcp`'s BUNDLE_NOT_BUILT) already
//! carry the actionable messages.
//!
//! The selected file is zstd-compressed (see
//! `quarto_build_support::embed_zstd_level`, multithread — the
//! same treatment as quarto-preview's embed archives) and exposed via
//! `QUARTO_WASM_EMBED_ARCHIVE`; the runtime decompresses lazily on first
//! use so `q2 render`-class invocations pay nothing for the embed.

use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

/// Basename prefix of the Vite-hashed WASM in the preview SPA dist.
const HASHED_WASM_PREFIX: &str = "wasm_quarto_hub_client_bg-";
/// The fixed name every consumer materializes the WASM under.
const WASM_FILENAME: &str = "wasm_quarto_hub_client_bg.wasm";

fn main() {
    let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let workspace_root = manifest_dir.join("..").join("..");

    let spa_assets = workspace_root
        .join("q2-preview-spa")
        .join("dist")
        .join("assets");
    let bundle_wasm = workspace_root
        .join("ts-packages")
        .join("quarto-hub-mcp")
        .join("dist-bundle")
        .join(WASM_FILENAME);

    let selected = select_wasm(&spa_assets, &bundle_wasm);

    // Re-run when the candidates change. Cargo scans a watched directory
    // recursively, so watching the SPA `dist/` and the MCP `dist-bundle/`
    // catches a hashed WASM appearing, vanishing, or being rewritten, and
    // the bundle copy changing. Either tree may be missing; `watch_dir`
    // then creates it empty rather than watching a missing path, which
    // cargo would treat as always stale (bd-7xlph6ki).
    quarto_build_support::watch_dir(spa_assets.parent().expect("assets has a parent"));
    quarto_build_support::watch_dir(bundle_wasm.parent().expect("bundle WASM has a parent"));

    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let archive = out_dir.join("wasm.zst");
    match selected.file {
        Some(path) => {
            let bytes = std::fs::read(&path).expect("read selected render WASM");
            // Stamp the payload's identity so consumers can key caches
            // and staleness checks on it without decoding the archive
            // (the launcher's warm-cache path must not pay the ~36 MB
            // decode/hash). The stamp covers the very bytes compressed
            // below, so stamp and payload agree by construction.
            let sha = Sha256::digest(&bytes);
            println!(
                "cargo:rustc-env=QUARTO_WASM_EMBED_SHA256={}",
                sha.iter().map(|b| format!("{b:02x}")).collect::<String>()
            );
            println!("cargo:rustc-env=QUARTO_WASM_EMBED_LEN={}", bytes.len());
            let mut encoder =
                zstd::stream::Encoder::new(Vec::new(), quarto_build_support::embed_zstd_level())
                    .expect("zstd encoder");
            if let Ok(n) = std::thread::available_parallelism() {
                let _ = encoder.multithread(n.get() as u32);
            }
            std::io::Write::write_all(&mut encoder, &bytes).expect("zstd compress");
            let zst = encoder.finish().expect("zstd finish");
            // Unchanged bytes keep the archive's mtime, so `include_bytes!`
            // of it does not recompile this crate.
            quarto_build_support::write_if_changed(&archive, &zst).expect("write wasm.zst");
        }
        None => {
            // Placeholder: an empty archive means `wasm_bytes()` is None.
            quarto_build_support::write_if_changed(&archive, &[]).expect("write empty wasm.zst");
        }
    }
    println!(
        "cargo:rustc-env=QUARTO_WASM_EMBED_ARCHIVE={}",
        archive.display()
    );
}

struct Selection {
    file: Option<PathBuf>,
}

/// The exactly-one rule for the SPA dist's hashed WASM (mirrored by
/// quarto-preview's build.rs — the two must agree or the preview would
/// strip a file this crate did not embed, or vice versa), with the
/// dist-bundle copy as the fallback.
fn select_wasm(spa_assets: &Path, bundle_wasm: &Path) -> Selection {
    let mut hashed: Vec<PathBuf> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(spa_assets) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            if name.starts_with(HASHED_WASM_PREFIX)
                && name.ends_with(".wasm")
                && entry.file_type().is_ok_and(|t| t.is_file())
            {
                hashed.push(entry.path());
            }
        }
    }
    hashed.sort();
    if hashed.len() == 1 {
        return Selection {
            file: Some(hashed[0].clone()),
        };
    }
    // Zero matches (no SPA dist) or several (a dirty dist the preview
    // refuses to strip): fall back to the MCP bundle's own copy.
    Selection {
        file: bundle_wasm.is_file().then(|| bundle_wasm.to_path_buf()),
    }
}
