//! What the `pdf` request's compile step needs besides the `.typ` (R4, for
//! host H8): the vendored typst packages and Font Awesome fonts as their
//! own tree with their own version, and the document-date prelude.
//!
//! The assets are a separate export, not part of the share tree docx
//! workers mount: only a PDF compile reads them, and they are 2.5 MB.

use std::sync::OnceLock;

use include_dir::{Dir, DirEntry};
use sha2::{Digest, Sha256};

use crate::pandoc_filters::TYPST_PACKAGES_DIR;

/// One file of the typst assets, `rel_path` relative to the tree root
/// (`packages/preview/<name>/<version>/...`, `fonts/...`): the layout of a
/// typst package cache and a `--font-path`.
pub struct TypstAssetEntry {
    pub rel_path: String,
    pub bytes: &'static [u8],
}

fn collect(dir: &'static Dir<'static>, out: &mut Vec<TypstAssetEntry>) {
    for entry in dir.entries() {
        match entry {
            DirEntry::Dir(d) => collect(d, out),
            DirEntry::File(f) => out.push(TypstAssetEntry {
                rel_path: f.path().to_string_lossy().replace('\\', "/"),
                bytes: f.contents(),
            }),
        }
    }
}

/// Every typst asset, sorted by path and deduped (`include_dir` entry order
/// is not a contract). `README.md` files are documentation, not assets.
pub fn typst_asset_entries() -> &'static [TypstAssetEntry] {
    static ENTRIES: OnceLock<Vec<TypstAssetEntry>> = OnceLock::new();
    ENTRIES.get_or_init(|| {
        let mut entries = Vec::new();
        collect(&TYPST_PACKAGES_DIR, &mut entries);
        entries.retain(|e| e.rel_path != "README.md");
        entries.sort_by(|a, b| a.rel_path.cmp(&b.rel_path));
        entries.dedup_by(|a, b| a.rel_path == b.rel_path);
        entries
    })
}

/// SHA-256 (hex) over the sorted `(path, bytes)` entries, computed like the
/// share tree's version and independent of it.
pub fn typst_assets_version() -> &'static str {
    static VERSION: OnceLock<String> = OnceLock::new();
    VERSION.get_or_init(|| {
        let mut hasher = Sha256::new();
        for entry in typst_asset_entries() {
            hasher.update((entry.rel_path.len() as u64).to_le_bytes());
            hasher.update(entry.rel_path.as_bytes());
            hasher.update((entry.bytes.len() as u64).to_le_bytes());
            hasher.update(entry.bytes);
        }
        hex::encode(hasher.finalize())
    })
}

/// A typst source line that pins the document date to `epoch` (seconds,
/// UTC). typst.ts has no date option, so without it `/CreationDate` and the
/// PDF `/ID` come from the clock. The host prepends this line to the `.typ`
/// before compiling a `post: compile_typst` request, with the epoch from the
/// request's `SOURCE_DATE_EPOCH`; it must be first, since a `set document`
/// rule has to precede content. Checked with typst 0.14.2: two compiles of
/// the pinned source are byte-identical, and the unpinned ones are not.
pub fn typst_date_prelude(epoch: i64) -> Option<String> {
    let t = time::OffsetDateTime::from_unix_timestamp(epoch).ok()?;
    Some(format!(
        "#set document(date: datetime(year: {}, month: {}, day: {}, hour: {}, minute: {}, second: {}))\n",
        t.year(),
        u8::from(t.month()),
        t.day(),
        t.hour(),
        t.minute(),
        t.second()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assets_cover_the_packages_and_fonts_and_are_versioned() {
        let entries = typst_asset_entries();
        assert!(entries.windows(2).all(|w| w[0].rel_path < w[1].rel_path));
        for prefix in ["packages/preview/", "fonts/"] {
            assert!(
                entries.iter().any(|e| e.rel_path.starts_with(prefix)),
                "no entry under {prefix}"
            );
        }
        assert!(entries.iter().all(|e| e.rel_path != "README.md"));
        assert_eq!(typst_assets_version().len(), 64);
        // Independent of the share tree.
        assert_ne!(
            typst_assets_version(),
            crate::pandoc_request::share_tree_version()
        );
    }

    #[test]
    fn the_prelude_is_utc_and_rejects_out_of_range_epochs() {
        assert_eq!(
            typst_date_prelude(1_700_000_000).as_deref(),
            Some(
                "#set document(date: datetime(year: 2023, month: 11, day: 14, hour: 22, minute: 13, second: 20))\n"
            )
        );
        assert!(typst_date_prelude(0).unwrap().contains("year: 1970"));
        assert!(typst_date_prelude(i64::MAX).is_none());
    }
}
