//! Directory helpers: deterministic content hashing and manifests.
//! Manifest and tree-id paths always use `/` so they agree across platforms.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::Path;
use walkdir::WalkDir;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestEntry {
    pub path: String,
    pub size: u64,
    pub sha256: String,
}

/// Every file under `root` (sorted, `/`-separated, relative), except any
/// whose relative path is in `skip`.
pub fn list_files(root: &Path, skip: &[&str]) -> Result<Vec<ManifestEntry>> {
    let mut out = Vec::new();
    for entry in WalkDir::new(root).sort_by_file_name() {
        let entry = entry?;
        if !entry.file_type().is_file() {
            continue;
        }
        let rel = entry
            .path()
            .strip_prefix(root)?
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/");
        if skip.contains(&rel.as_str()) {
            continue;
        }
        let bytes = std::fs::read(entry.path())
            .with_context(|| format!("reading {}", entry.path().display()))?;
        out.push(ManifestEntry {
            path: rel,
            size: bytes.len() as u64,
            sha256: hex(&Sha256::digest(&bytes)),
        });
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(out)
}

/// A short, deterministic id for a tree: sha256 over the sorted
/// (path, content hash) list.
pub fn tree_id(entries: &[ManifestEntry]) -> String {
    let mut h = Sha256::new();
    for e in entries {
        h.update(e.path.as_bytes());
        h.update([0]);
        h.update(e.sha256.as_bytes());
        h.update(*b"\n");
    }
    hex(&h.finalize())[..16].to_string()
}

pub fn write_manifest(path: &Path, entries: &[ManifestEntry]) -> Result<()> {
    let json = serde_json::to_string_pretty(entries)?;
    std::fs::write(path, json + "\n").with_context(|| format!("writing {}", path.display()))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tree_id_depends_on_content_and_paths_not_order() {
        let a = ManifestEntry {
            path: "a".into(),
            size: 1,
            sha256: "x".into(),
        };
        let b = ManifestEntry {
            path: "b".into(),
            size: 1,
            sha256: "y".into(),
        };
        let id = tree_id(&[a.clone(), b.clone()]);
        assert_eq!(id.len(), 16);
        let renamed = ManifestEntry {
            path: "c".into(),
            ..b.clone()
        };
        assert_ne!(id, tree_id(&[a.clone(), renamed]));
        let changed = ManifestEntry {
            sha256: "z".into(),
            ..b
        };
        assert_ne!(id, tree_id(&[a, changed]));
    }

    #[test]
    fn list_files_is_sorted_slash_relative_and_skips() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("d")).unwrap();
        std::fs::write(dir.path().join("d/b.txt"), "b").unwrap();
        std::fs::write(dir.path().join("a.txt"), "a").unwrap();
        std::fs::write(dir.path().join("manifest.json"), "{}").unwrap();
        let got = list_files(dir.path(), &["manifest.json"]).unwrap();
        let paths: Vec<_> = got.iter().map(|e| e.path.as_str()).collect();
        assert_eq!(paths, ["a.txt", "d/b.txt"]);
    }
}
