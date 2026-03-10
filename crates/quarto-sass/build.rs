//! Build script for quarto-sass.
//!
//! Computes a SHA-256 hash of all embedded SCSS resources at build time.
//! This hash is used as part of the CSS cache key so that changes to
//! built-in SCSS files (Bootstrap, themes, Quarto customizations) invalidate
//! the cache without needing to read every file at runtime.

use sha2::{Digest, Sha256};
use std::env;
use std::fs::{self, File};
use std::io::Write;
use std::path::Path;

fn main() {
    let out_dir = env::var("OUT_DIR").unwrap();
    let scss_hash = compute_scss_resources_hash();

    let hash_path = Path::new(&out_dir).join("scss_resources_hash.txt");
    let mut file = File::create(&hash_path).expect("Failed to create hash file");
    write!(file, "{}", scss_hash).expect("Failed to write hash");

    println!("cargo:rerun-if-changed=../../resources/scss");
    println!("cargo:rerun-if-changed=build.rs");
}

/// Compute a SHA-256 hash of all SCSS files in resources/scss/.
///
/// Files are sorted by path to ensure deterministic hashing.
fn compute_scss_resources_hash() -> String {
    let scss_dir = Path::new("../../resources/scss");

    let mut hasher = Sha256::new();
    let mut files: Vec<_> = collect_scss_files(scss_dir);
    files.sort();

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
