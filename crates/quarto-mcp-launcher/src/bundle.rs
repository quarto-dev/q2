//! The embedded hub MCP server bundle.
//!
//! `build.rs` points `QUARTO_HUB_MCP_EMBED_DIR` either at the real
//! `ts-packages/quarto-hub-mcp/dist-bundle/` or at a placeholder
//! containing only a `BUNDLE_NOT_BUILT` marker (fresh clone, no npm
//! step yet).

use include_dir::{Dir, include_dir};
use sha2::{Digest, Sha256};
use std::borrow::Cow;
use std::path::PathBuf;

static EMBEDDED_BUNDLE: Dir<'_> = include_dir!("$QUARTO_HUB_MCP_EMBED_DIR");

/// One file of the bundle: workspace-relative path (forward slashes,
/// as `include_dir` stores them) plus contents. The extraction and
/// hashing code works on this representation so tests can fabricate
/// bundles without compile-time embedding.
pub type BundleFile = (PathBuf, Cow<'static, [u8]>);

/// True when the build embedded the placeholder instead of a real
/// bundle (`cargo build` before `cargo xtask build-hub-mcp-bundle`).
pub fn is_placeholder() -> bool {
    EMBEDDED_BUNDLE.get_file("BUNDLE_NOT_BUILT").is_some()
}

/// Flatten the embedded directory tree into (path, contents) pairs.
pub fn embedded_files() -> Vec<BundleFile> {
    let mut files = Vec::new();
    collect(&EMBEDDED_BUNDLE, &mut files);
    files
}

fn collect(dir: &Dir<'static>, out: &mut Vec<BundleFile>) {
    for file in dir.files() {
        out.push((file.path().to_path_buf(), Cow::Borrowed(file.contents())));
    }
    for sub in dir.dirs() {
        collect(sub, out);
    }
}

/// Content hash of a bundle: 16 hex chars of SHA-256 over the sorted
/// (path, length, contents) sequence. Used as the cache directory name
/// — same bundle bytes, same directory, across q2 builds.
pub fn content_hash(files: &[BundleFile]) -> String {
    let mut sorted: Vec<&BundleFile> = files.iter().collect();
    sorted.sort_by(|a, b| a.0.cmp(&b.0));
    let mut hasher = Sha256::new();
    for (path, contents) in sorted {
        // Hash the path with forward slashes so the same bundle hashes
        // identically regardless of host separator conventions.
        let path_str = path
            .components()
            .map(|c| c.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/");
        hasher.update(path_str.as_bytes());
        hasher.update([0u8]);
        hasher.update((contents.len() as u64).to_le_bytes());
        hasher.update(contents);
    }
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(16);
    for byte in &digest[..8] {
        hex.push_str(&format!("{:02x}", byte));
    }
    hex
}

/// The embedded `build-info.json` stamp, raw (pretty JSON written by
/// the bundler), or None for placeholder builds.
pub fn build_info_json() -> Option<&'static str> {
    EMBEDDED_BUNDLE
        .get_file("build-info.json")
        .and_then(|f| f.contents_utf8())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(path: &str, contents: &[u8]) -> BundleFile {
        (PathBuf::from(path), Cow::Owned(contents.to_vec()))
    }

    #[test]
    fn hash_is_order_independent() {
        let a = vec![file("a.txt", b"one"), file("b/c.txt", b"two")];
        let b = vec![file("b/c.txt", b"two"), file("a.txt", b"one")];
        assert_eq!(content_hash(&a), content_hash(&b));
    }

    #[test]
    fn hash_changes_with_content() {
        let a = vec![file("a.txt", b"one")];
        let b = vec![file("a.txt", b"two")];
        assert_ne!(content_hash(&a), content_hash(&b));
    }

    #[test]
    fn hash_distinguishes_path_vs_content_boundary() {
        // ("ab", "c") must not collide with ("a", "bc")
        let a = vec![file("ab", b"c")];
        let b = vec![file("a", b"bc")];
        assert_ne!(content_hash(&a), content_hash(&b));
    }

    #[test]
    fn hash_is_16_hex_chars() {
        let h = content_hash(&[file("x", b"y")]);
        assert_eq!(h.len(), 16);
        assert!(h.chars().all(|c| c.is_ascii_hexdigit()));
    }
}
