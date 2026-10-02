//! The share tree as data: the entries native `extract_share_tree` writes
//! and the version that identifies them.

use std::sync::OnceLock;

use include_dir::{Dir, DirEntry};
use sha2::{Digest, Sha256};

use crate::pandoc_filters::{DATADIR_DIR, FILTERS_DIR, FORMATS_DOCX_DIR};

/// One file of the share tree, `rel_path` relative to the tree root
/// (`filters/...`, `pandoc/datadir/...`, `formats/docx/...`).
pub struct ShareEntry {
    pub rel_path: String,
    pub bytes: &'static [u8],
}

fn collect(dir: &'static Dir<'static>, prefix: &str, out: &mut Vec<ShareEntry>) {
    for entry in dir.entries() {
        match entry {
            DirEntry::Dir(d) => collect(d, prefix, out),
            DirEntry::File(f) => out.push(ShareEntry {
                rel_path: format!("{prefix}/{}", f.path().to_string_lossy().replace('\\', "/")),
                bytes: f.contents(),
            }),
        }
    }
}

/// Every share-tree file, sorted by path and deduped (`include_dir` entry
/// order is not a contract).
pub fn share_tree_entries() -> &'static [ShareEntry] {
    static ENTRIES: OnceLock<Vec<ShareEntry>> = OnceLock::new();
    ENTRIES.get_or_init(|| {
        let mut entries = Vec::new();
        collect(&FILTERS_DIR, "filters", &mut entries);
        collect(&DATADIR_DIR, "pandoc/datadir", &mut entries);
        collect(&FORMATS_DOCX_DIR, "formats/docx", &mut entries);
        entries.sort_by(|a, b| a.rel_path.cmp(&b.rel_path));
        entries.dedup_by(|a, b| a.rel_path == b.rel_path);
        entries
    })
}

/// SHA-256 (hex) over the sorted `(path, bytes)` entries of the share tree.
pub fn share_tree_version() -> &'static str {
    static VERSION: OnceLock<String> = OnceLock::new();
    VERSION.get_or_init(|| {
        let mut hasher = Sha256::new();
        for entry in share_tree_entries() {
            hasher.update((entry.rel_path.len() as u64).to_le_bytes());
            hasher.update(entry.rel_path.as_bytes());
            hasher.update((entry.bytes.len() as u64).to_le_bytes());
            hasher.update(entry.bytes);
        }
        hex::encode(hasher.finalize())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entries_are_sorted_unique_and_cover_the_three_trees() {
        let entries = share_tree_entries();
        assert!(entries.windows(2).all(|w| w[0].rel_path < w[1].rel_path));
        for prefix in ["filters/main.lua", "pandoc/datadir/", "formats/docx/"] {
            assert!(
                entries.iter().any(|e| e.rel_path.starts_with(prefix)),
                "no entry under {prefix}"
            );
        }
        assert_eq!(share_tree_version().len(), 64);
    }
}
