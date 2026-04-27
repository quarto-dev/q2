//! Build script for quarto-sass.
//!
//! Computes two hashes at build time:
//!
//! - `SCSS_RESOURCES_HASH`: covers only the `.scss` files under
//!   `resources/scss/`. Callers that want "did a SCSS file change?"
//!   use this one.
//! - `CSS_BUILD_ID`: the one used for the IndexedDB cache
//!   invalidation. Combines `SCSS_RESOURCES_HASH` **and** a hash of
//!   every `.rs` file under `crates/quarto-sass/src/`. Any Rust-side
//!   change that affects SCSS assembly (e.g., adding
//!   `load_highlight_layer` to `compile_default_css`) changes the
//!   build ID even if no `.scss` file changed, so stale IndexedDB
//!   entries from a prior WASM deploy get purged on first load.
//!
//! Why the combined hash instead of hashing the full assembled CSS at
//! runtime: computing a 400 KB SHA-256 per render in WASM is ~3–5 ms
//! overhead on every cache hit, whereas a compile-time hash of code +
//! data is zero-cost at runtime and invalidates with the same
//! fidelity. The cache invalidates once per WASM deploy rather than on
//! every render. The resulting one-compile-per-deploy cost is hidden
//! by a fire-and-forget warm hook in the hub-client's startup path.

use sha2::{Digest, Sha256};
use std::env;
use std::fs::{self, File};
use std::io::Write;
use std::path::Path;

fn main() {
    let out_dir = env::var("OUT_DIR").unwrap();

    let scss_hash = compute_scss_resources_hash();
    write_hash(&out_dir, "scss_resources_hash.txt", &scss_hash);

    // CSS_BUILD_ID = SHA-256(scss_hash + "|" + hash-of-all-our-rust-sources).
    let code_hash = compute_src_code_hash();
    let build_id = compose_build_id(&scss_hash, &code_hash);
    write_hash(&out_dir, "css_build_id.txt", &build_id);

    println!("cargo:rerun-if-changed=../../resources/scss");
    println!("cargo:rerun-if-changed=src");
    println!("cargo:rerun-if-changed=build.rs");
}

fn write_hash(out_dir: &str, filename: &str, hash: &str) {
    let path = Path::new(out_dir).join(filename);
    let mut file = File::create(&path).expect("Failed to create hash file");
    write!(file, "{}", hash).expect("Failed to write hash");
}

/// Compute a SHA-256 hash of all SCSS files in resources/scss/.
///
/// Files are sorted by path to ensure deterministic hashing.
fn compute_scss_resources_hash() -> String {
    let scss_dir = Path::new("../../resources/scss");
    let mut files: Vec<_> = collect_files_with_ext(scss_dir, "scss");
    files.sort();

    let mut hasher = Sha256::new();
    for file_path in files {
        let rel_path = file_path
            .strip_prefix(scss_dir)
            .unwrap_or(&file_path)
            .to_string_lossy();
        hasher.update(rel_path.as_bytes());
        hasher.update(b"\n");

        if let Ok(contents) = fs::read(&file_path) {
            hasher.update(&contents);
        }
        hasher.update(b"\n");
    }

    let hash = hasher.finalize();
    format!("{:x}", hash)[..16].to_string()
}

/// Compute a SHA-256 hash of all `.rs` files under `crates/quarto-sass/src/`.
///
/// These are the Rust sources whose content influences how SCSS gets
/// assembled (layer loading, merging, themed vs default paths). Any
/// edit to them should bust the CSS cache — otherwise users carrying
/// stale IndexedDB entries from a prior deploy see pre-change CSS
/// served without the new Rust logic's contribution.
fn compute_src_code_hash() -> String {
    let src_dir = Path::new("src");
    let mut files: Vec<_> = collect_files_with_ext(src_dir, "rs");
    files.sort();

    let mut hasher = Sha256::new();
    for file_path in files {
        let rel_path = file_path
            .strip_prefix(src_dir)
            .unwrap_or(&file_path)
            .to_string_lossy();
        hasher.update(rel_path.as_bytes());
        hasher.update(b"\n");

        if let Ok(contents) = fs::read(&file_path) {
            hasher.update(&contents);
        }
        hasher.update(b"\n");
    }

    let hash = hasher.finalize();
    format!("{:x}", hash)[..16].to_string()
}

/// Combine SCSS + code hashes into a single build-id string.
fn compose_build_id(scss_hash: &str, code_hash: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(scss_hash.as_bytes());
    hasher.update(b"|");
    hasher.update(code_hash.as_bytes());
    let hash = hasher.finalize();
    format!("{:x}", hash)[..16].to_string()
}

/// Recursively collect all files with a given extension under `dir`.
fn collect_files_with_ext(dir: &Path, ext: &str) -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();

    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                files.extend(collect_files_with_ext(&path, ext));
            } else if path.extension().map_or(false, |e| e == ext) {
                files.push(path);
            }
        }
    }

    files
}
