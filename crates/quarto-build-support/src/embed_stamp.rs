//! Staleness stamps for generated bundles that get embedded in a binary.
//!
//! `hub-client/dist-preview-embed/` (the editor UI behind
//! `q2 preview --ui editor`) is a gitignored build artifact. Nothing
//! rebuilds it when its sources change, so a plain `cargo build --bin q2`
//! silently embeds an old bundle. The fix is a *stamp*:
//!
//! 1. The bundle's producer (`cargo xtask build-hub-client-embed`) writes
//!    [`STAMP_FILENAME`] into the bundle, recording the [`fingerprint`]
//!    of the sources it was built from.
//! 2. The embedding build script recomputes the fingerprint of the same
//!    sources and calls [`check`]. A mismatch means the sources changed
//!    after the bundle was built.
//!
//! This module is pure (no cargo or process state) so the producer, the
//! build script, and the tests all share one definition of "the inputs".

use sha2::{Digest, Sha256};
use std::io;
use std::path::{Path, PathBuf};

/// Name of the stamp file inside the bundle directory.
pub const STAMP_FILENAME: &str = "build-info.json";

/// Directory names never hashed: dependency trees, build outputs, and
/// test artifacts. Names beginning with `dist` are skipped too.
const SKIP_DIRS: &[&str] = &[
    "node_modules",
    "target",
    "coverage",
    "test-results",
    "playwright-report",
    "__pycache__",
    "__tests__",
    "e2e",
    "e2e-author",
];

/// Whether a directory entry named `name` is skipped.
fn skip_dir(name: &str) -> bool {
    name.starts_with("dist") || name.starts_with('.') || SKIP_DIRS.contains(&name)
}

/// Whether a file named `name` is skipped: OS junk and test sources
/// (editing a test must not mark the shipped bundle stale).
fn skip_file(name: &str) -> bool {
    name == ".DS_Store" || name.contains(".test.") || name.contains(".spec.")
}

/// The files and directories (relative to the workspace root `root`)
/// that feed the hub-client editor bundle. Missing entries are ignored
/// by [`fingerprint`].
///
/// The render WASM is deliberately *not* an input: it is deduplicated
/// against the viewer SPA's copy and has its own staleness trap
/// (`AGENTS.md`, "Verifying Rust changes in `q2 preview`").
pub fn editor_bundle_inputs(root: &Path) -> Vec<PathBuf> {
    [
        "hub-client/src",
        "hub-client/public",
        "hub-client/index.html",
        "hub-client/package.json",
        "hub-client/vite.config.ts",
        "hub-client/tsconfig.json",
        "hub-client/tsconfig.app.json",
        "hub-client/tsconfig.node.json",
        "ts-packages",
        "package.json",
        "package-lock.json",
    ]
    .iter()
    .map(|p| root.join(p))
    .collect()
}

/// A stable SHA-256 (hex) over the content and root-relative path of
/// every non-skipped file under `inputs`. Independent of directory
/// iteration order and of the path separator. Missing inputs are
/// ignored (a checkout may lack some).
pub fn fingerprint(root: &Path, inputs: &[PathBuf]) -> io::Result<String> {
    let mut files = Vec::new();
    for input in inputs {
        collect(input, &mut files)?;
    }
    let mut keyed: Vec<(String, PathBuf)> = files
        .into_iter()
        .map(|p| {
            let rel = p.strip_prefix(root).unwrap_or(&p);
            let key = rel
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/");
            (key, p)
        })
        .collect();
    keyed.sort();
    keyed.dedup_by(|a, b| a.0 == b.0);

    let mut hasher = Sha256::new();
    for (key, path) in &keyed {
        let bytes = std::fs::read(path)?;
        hasher.update((key.len() as u64).to_le_bytes());
        hasher.update(key.as_bytes());
        hasher.update((bytes.len() as u64).to_le_bytes());
        hasher.update(&bytes);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

fn collect(path: &Path, out: &mut Vec<PathBuf>) -> io::Result<()> {
    let meta = match std::fs::metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };
    if meta.is_file() {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if !skip_file(name) {
            out.push(path.to_path_buf());
        }
    } else if meta.is_dir() {
        for entry in std::fs::read_dir(path)? {
            let entry = entry?;
            let name = entry.file_name();
            let name = name.to_string_lossy();
            let child = entry.path();
            // `metadata` follows symlinks (npm workspaces link
            // `node_modules/@scope/pkg` back into `ts-packages/`).
            if child.is_dir() && skip_dir(&name) {
                continue;
            }
            collect(&child, out)?;
        }
    }
    Ok(())
}

/// The stamp file's content for a bundle built from `fingerprint`.
pub fn render_stamp(fingerprint: &str) -> String {
    let mut s = serde_json::to_string_pretty(&serde_json::json!({
        "inputsFingerprint": fingerprint,
    }))
    .expect("a JSON object always serializes");
    s.push('\n');
    s
}

/// How a bundle's stamp compares to the sources as they are now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Freshness {
    /// The stamp matches the current sources.
    Fresh,
    /// The sources changed after the bundle was built.
    Stale,
    /// The bundle carries no stamp (built by hand, or before stamping).
    NoStamp,
    /// The stamp exists but could not be read as a stamp.
    Unreadable,
}

/// Compare `stamp_json` (the stamp file's content, `None` when the file
/// is absent) with the `current` fingerprint. Never panics: a malformed
/// stamp is [`Freshness::Unreadable`].
pub fn check(stamp_json: Option<&str>, current: &str) -> Freshness {
    let Some(text) = stamp_json else {
        return Freshness::NoStamp;
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(text) else {
        return Freshness::Unreadable;
    };
    match value.get("inputsFingerprint").and_then(|v| v.as_str()) {
        Some(built) if built == current => Freshness::Fresh,
        Some(_) => Freshness::Stale,
        None => Freshness::Unreadable,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(root: &Path, rel: &str, body: &str) {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, body).unwrap();
    }

    fn fp(root: &Path) -> String {
        fingerprint(root, &editor_bundle_inputs(root)).unwrap()
    }

    fn tree() -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        write(tmp.path(), "hub-client/src/App.tsx", "app");
        write(tmp.path(), "hub-client/src/util/a.ts", "a");
        write(tmp.path(), "ts-packages/x/src/index.ts", "x");
        write(tmp.path(), "package-lock.json", "{}");
        tmp
    }

    #[test]
    fn identical_trees_hash_identically() {
        assert_eq!(fp(tree().path()), fp(tree().path()));
    }

    #[test]
    fn content_change_changes_hash() {
        let t = tree();
        let before = fp(t.path());
        write(t.path(), "hub-client/src/App.tsx", "app2");
        assert_ne!(before, fp(t.path()));
    }

    #[test]
    fn adding_or_renaming_a_file_changes_hash() {
        let t = tree();
        let before = fp(t.path());
        write(t.path(), "hub-client/src/new.ts", "n");
        let added = fp(t.path());
        assert_ne!(before, added);
        std::fs::rename(
            t.path().join("hub-client/src/new.ts"),
            t.path().join("hub-client/src/renamed.ts"),
        )
        .unwrap();
        assert_ne!(added, fp(t.path()));
    }

    #[test]
    fn ignored_paths_do_not_change_hash() {
        let t = tree();
        let before = fp(t.path());
        write(t.path(), "ts-packages/x/node_modules/dep/i.js", "d");
        write(t.path(), "ts-packages/x/dist/out.js", "o");
        write(t.path(), "ts-packages/x/dist-bundle/out.js", "o");
        write(t.path(), "hub-client/src/.DS_Store", "junk");
        write(t.path(), "hub-client/src/App.test.tsx", "t");
        write(t.path(), "hub-client/src/__tests__/a.ts", "t");
        assert_eq!(before, fp(t.path()));
    }

    #[test]
    fn missing_inputs_are_ignored() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(fp(tmp.path()), fp(tmp.path()));
    }

    #[test]
    fn stamp_round_trips_as_fresh() {
        let t = tree();
        let stamp = render_stamp(&fp(t.path()));
        assert_eq!(check(Some(&stamp), &fp(t.path())), Freshness::Fresh);
    }

    #[test]
    fn stamp_goes_stale_after_a_source_edit() {
        let t = tree();
        let stamp = render_stamp(&fp(t.path()));
        write(t.path(), "hub-client/src/App.tsx", "edited");
        assert_eq!(check(Some(&stamp), &fp(t.path())), Freshness::Stale);
    }

    #[test]
    fn missing_or_malformed_stamps_degrade() {
        assert_eq!(check(None, "abc"), Freshness::NoStamp);
        assert_eq!(check(Some("not json"), "abc"), Freshness::Unreadable);
        assert_eq!(check(Some("{}"), "abc"), Freshness::Unreadable);
        assert_eq!(
            check(Some(r#"{"inputsFingerprint": 3}"#), "abc"),
            Freshness::Unreadable
        );
    }
}
