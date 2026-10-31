//! Build script that archives the embedded docs site for `q2 docs llms`
//! and `q2 docs serve` (bd-hwop1zii, bd-x248xpyh).
//!
//! 1. If `agents-docs-dist/` holds a staged site (`site/llms.txt` plus
//!    the `llms-manifest.json` ledger — the tree staged by `cargo xtask
//!    build-agents-docs`), archive it.
//! 2. Otherwise archive a placeholder — just an `embed-info.json`
//!    marking the embed as a placeholder — so fresh clones build without
//!    the docs staged. `q2 docs llms` / `q2 docs serve` then fail at
//!    runtime pointing at the xtask.
//!
//! The archive is a deterministic identity-only tar.zst in `OUT_DIR`
//! (the bd-rem4bpee treatment the preview SPA gets: the rendered site is
//! ~60 MB raw but ~3 MB compressed, since every page repeats the same
//! navigation), exposed as `QUARTO_DOCS_EMBED_ARCHIVE` via
//! `cargo:rustc-env` and consumed by `src/commands/docs_embed.rs`
//! through `include_bytes!`.
//!
//! Entry order is part of the format: `embed-info.json`, then
//! `llms-manifest.json`, then every ledger-listed file (sorted), then
//! the rest of the site (sorted). `q2 docs llms` stream-decodes only
//! that prefix and stops at the first non-ledger entry, so it pays ~1 ms
//! instead of decoding the whole site (plan
//! `claude-notes/plans/2026-10-26-q2-docs-serve.md` § Decode latency).

use std::io::Write;
use std::path::{Path, PathBuf};

fn main() {
    let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let workspace_root = manifest_dir.join("..").join("..");
    let real_dist = workspace_root.join("agents-docs-dist");

    let entries = if real_dist.join("site").join("llms.txt").is_file()
        && real_dist.join("llms-manifest.json").is_file()
    {
        real_entries(&real_dist)
    } else {
        println!(
            "cargo:warning=agents-docs-dist/ holds no staged docs site; embedding a \
             placeholder for `q2 docs`. Run `cargo xtask build-agents-docs` and \
             rebuild to embed the real docs."
        );
        vec![(
            "embed-info.json".to_string(),
            b"{\"placeholder\":true}\n".to_vec(),
        )]
    };

    let archive = write_archive(&entries);
    println!(
        "cargo:rustc-env=QUARTO_DOCS_EMBED_ARCHIVE={}",
        archive.display()
    );

    // Re-run if the staged tree changes, appears, or vanishes. A missing
    // tree is created empty rather than watched missing (always stale) or
    // via the workspace root (stale after every build) (bd-7xlph6ki).
    quarto_build_support::watch_dir(&real_dist);
}

/// The staged dist's archive entries, in the llms-first order the
/// runtime relies on.
fn real_entries(dist: &Path) -> Vec<(String, Vec<u8>)> {
    #[derive(serde::Deserialize)]
    struct LlmsManifest {
        generated: Vec<String>,
    }

    let read = |rel: &str| {
        let path = dist.join(rel.replace('/', std::path::MAIN_SEPARATOR_STR));
        std::fs::read(&path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()))
    };
    let ledger_bytes = read("llms-manifest.json");
    let ledger: LlmsManifest =
        serde_json::from_slice(&ledger_bytes).expect("agents-docs-dist/llms-manifest.json parses");

    let mut ledger_paths: Vec<String> = ledger
        .generated
        .iter()
        .map(|rel| format!("site/{rel}"))
        .collect();
    ledger_paths.sort();
    ledger_paths.dedup();

    let mut rest = Vec::new();
    collect_files(&dist.join("site"), "site", &mut rest);
    rest.retain(|rel| ledger_paths.binary_search(rel).is_err());
    rest.sort();

    let mut entries = Vec::new();
    // The sidecar is optional (provenance then reads as unknown).
    if dist.join("embed-info.json").is_file() {
        entries.push(("embed-info.json".to_string(), read("embed-info.json")));
    }
    entries.push(("llms-manifest.json".to_string(), ledger_bytes));
    for rel in ledger_paths.into_iter().chain(rest) {
        let bytes = read(&rel);
        entries.push((rel, bytes));
    }
    entries
}

/// Every file under `dir`, as `/`-separated paths prefixed with
/// `prefix`.
fn collect_files(dir: &Path, prefix: &str, out: &mut Vec<String>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(it) => it,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let rel = format!("{prefix}/{name}");
        if entry.file_type().is_ok_and(|t| t.is_dir()) {
            collect_files(&entry.path(), &rel, out);
        } else {
            out.push(rel);
        }
    }
}

/// Write `entries` as a deterministic tar.zst in `OUT_DIR`: entry order
/// as given, mtime 0, fixed mode.
fn write_archive(entries: &[(String, Vec<u8>)]) -> PathBuf {
    let mut builder = tar::Builder::new(Vec::new());
    for (rel, bytes) in entries {
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(tar::EntryType::Regular);
        header.set_size(bytes.len() as u64);
        header.set_mode(0o644);
        header.set_mtime(0);
        header.set_cksum();
        builder
            .append_data(&mut header, rel, bytes.as_slice())
            .expect("append tar entry");
    }
    let tar_bytes = builder.into_inner().expect("finish tar");

    let mut encoder =
        zstd::stream::Encoder::new(Vec::new(), quarto_build_support::embed_zstd_level())
            .expect("zstd encoder");
    if let Ok(n) = std::thread::available_parallelism() {
        let _ = encoder.multithread(n.get() as u32);
    }
    encoder.write_all(&tar_bytes).expect("zstd compress");
    let zst = encoder.finish().expect("zstd finish");

    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let archive = out_dir.join("docs-embed.tar.zst");
    // Unchanged bytes keep the archive's mtime, so `include_bytes!` of it
    // does not recompile this crate.
    quarto_build_support::write_if_changed(&archive, &zst).expect("write docs embed archive");
    archive
}
