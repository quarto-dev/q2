//! Build script for wasm-quarto-hub-client.
//!
//! Computes a hash of all embedded SCSS resources at build time.
//! This hash is used to invalidate the SASS cache in hub-client when
//! the embedded SCSS files change.

use sha2::{Digest, Sha256};
use std::env;
use std::fs::{self, File};
use std::io::Write;
use std::path::Path;

fn main() {
    let out_dir = env::var("OUT_DIR").unwrap();
    let scss_hash = compute_scss_resources_hash();

    // Write hash to file for include_str!
    let hash_path = Path::new(&out_dir).join("scss_resources_hash.txt");
    let mut file = File::create(&hash_path).expect("Failed to create hash file");
    write!(file, "{}", scss_hash).expect("Failed to write hash");

    // Tell Cargo to rerun if any SCSS file changes
    println!("cargo:rerun-if-changed=../../resources/scss");

    // Also rerun if build.rs changes
    println!("cargo:rerun-if-changed=build.rs");
}

/// Compute a SHA-256 hash of all SCSS files in resources/scss/.
///
/// Files are sorted by path to ensure deterministic hashing.
fn compute_scss_resources_hash() -> String {
    let scss_dir = Path::new("../../resources/scss");

    let mut hasher = Sha256::new();
    let mut files: Vec<_> = collect_scss_files(scss_dir);
    files.sort(); // Deterministic ordering

    for file_path in files {
        // Hash the relative path (for determinism across machines)
        let rel_path = file_path
            .strip_prefix(scss_dir)
            .unwrap_or(&file_path)
            .to_string_lossy();
        hasher.update(rel_path.as_bytes());
        hasher.update(b"\n");

        // Hash the file contents
        if let Ok(contents) = fs::read(&file_path) {
            hasher.update(&contents);
        }
        hasher.update(b"\n");
    }

    // Return first 16 chars of hex hash (64 bits, sufficient for cache invalidation)
    let hash = hasher.finalize();
    format!("{:x}", hash)[..16].to_string()
}

/// Recursively collect all .scss files in a directory.
fn collect_scss_files(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();

    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                files.extend(collect_scss_files(&path));
            } else if path.extension().map_or(false, |ext| ext == "scss") {
                files.push(path);
            }
        }
    }

    files
}
