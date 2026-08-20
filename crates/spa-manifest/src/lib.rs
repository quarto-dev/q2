//! SPA asset manifest generation and parsing (live-share plan Phase 2,
//! bd-ee2fqm95; `claude-notes/plans/2026-08-13-live-share-local-spa-assets.md`,
//! design decision 4).
//!
//! A manifest describes one embedded SPA bundle as the preview server
//! will serve it: sorted `(path, sha256, size, content_type,
//! content_encoding?)` entries plus a top-level hash. The host
//! advertises the top-level hash in `GET /api/preview/config`; a `--join`
//! guest whose embedded manifest hash matches exactly serves assets
//! locally instead of funneling them through the tunnel. Any mismatch —
//! or a missing manifest on either side — falls back to full tunneling.
//!
//! **Determinism is the contract.** Release CI builds on different
//! platforms and jobs must produce equal hashes for equal inputs, or
//! cross-platform local mode silently disables itself. Generation is
//! therefore byte-oriented throughout: entries sort by path (byte
//! order), relative paths always use `/` separators, and the top-level
//! hash covers a canonical `\0`-joined text encoding of the entries —
//! never a JSON serialization, so the formula is trivially identical
//! across implementations (the npm-side `scripts/manifest-dist.mjs`
//! must agree with this crate; an integration test in quarto-preview
//! pins that on real dists).
//!
//! Two producers share this crate:
//!
//! - `quarto-preview/build.rs` writes the **editor** manifest into the
//!   post-dedupe editor embed dir, over the *post-resolution* view
//!   (editor embed files plus the viewer-embed fallback for stripped
//!   duplicates) — what `lookup_embedded(Editor, path)` returns.
//! - The viewer manifest is written into `q2-preview-spa/dist/` by the
//!   SPA's own npm build (single producer, like the `.gz`
//!   precompression pass, so no build path can wipe it); this crate's
//!   generator must produce the identical manifest for the identical
//!   tree.
//!
//! `.gz` precompression siblings are never listed as entries; they fold
//! into the identity entry's `content_encoding`. The manifest hashes
//! identity bytes only, so cross-platform zlib differences in the `.gz`
//! siblings are invisible to compatibility. A manifest never lists
//! itself: a manifest cannot contain its own hash.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// The manifest filename at the root of a dist/embed dir.
pub const MANIFEST_FILENAME: &str = "spa-manifest.json";

/// Schema version of the manifest format.
pub const MANIFEST_VERSION: u32 = 1;

/// One served asset.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManifestEntry {
    /// Slash-separated path relative to the dist root, exactly as the
    /// HTTP request path normalization produces it (`assets/main-x.js`).
    pub path: String,
    /// Lowercase hex SHA-256 of the identity (uncompressed) bytes.
    pub sha256: String,
    /// Identity byte length.
    pub size: u64,
    /// The `Content-Type` the server emits for this path.
    pub content_type: String,
    /// `"gzip"` when a precompressed `<path>.gz` sibling exists;
    /// absent otherwise.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_encoding: Option<String>,
}

/// A parsed or generated manifest.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub version: u32,
    /// Top-level compatibility hash: SHA-256 over the canonical entry
    /// encoding (see the module docs). Lowercase hex.
    pub hash: String,
    /// Sorted by `path` (byte order).
    pub entries: Vec<ManifestEntry>,
}

/// One file feeding generation: a `/`-separated relative path plus the
/// absolute path to read its bytes from. Keeping the two separate lets
/// `build.rs` describe the editor's post-resolution view — files drawn
/// from *two* directories under their shared relative paths.
#[derive(Clone, Debug)]
pub struct SourceFile {
    pub rel: String,
    pub abs: PathBuf,
}

/// The canonical `Content-Type` table for served assets. Single source
/// of truth: quarto-preview's `asset_response` wraps this, and
/// `scripts/manifest-dist.mjs` carries a mirror (kept honest by the
/// quarto-preview equivalence test on real dists).
pub fn content_type_for(path: &str) -> &'static str {
    let ext = Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");
    match ext.to_ascii_lowercase().as_str() {
        "html" => "text/html; charset=utf-8",
        "js" => "application/javascript",
        "css" => "text/css",
        "json" => "application/json",
        "wasm" => "application/wasm",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        _ => "application/octet-stream",
    }
}

/// List every file under `dir` as [`SourceFile`]s with `/`-separated
/// relative paths (on every platform — the manifest is
/// platform-independent). Order is unspecified; [`generate`] sorts.
pub fn list_dir(dir: &Path) -> std::io::Result<Vec<SourceFile>> {
    fn walk(root: &Path, dir: &Path, out: &mut Vec<SourceFile>) -> std::io::Result<()> {
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if entry.file_type()?.is_dir() {
                walk(root, &path, out)?;
                continue;
            }
            let rel = path.strip_prefix(root).unwrap_or(&path);
            let rel = rel
                .components()
                .map(|c| {
                    c.as_os_str().to_str().ok_or_else(|| {
                        std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            format!("non-UTF-8 path component in {}", path.display()),
                        )
                    })
                })
                .collect::<Result<Vec<&str>, _>>()?
                .join("/");
            out.push(SourceFile { rel, abs: path });
        }
        Ok(())
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out)?;
    Ok(out)
}

/// Generate a manifest over `files`. On duplicate relative paths the
/// **later** occurrence wins — callers list fallback sources first
/// (build.rs lists the viewer embed before the editor embed, so the
/// editor's own copy wins, matching `lookup_embedded`). The manifest
/// file itself (`spa-manifest.json` at the root) is excluded, so
/// regenerating over an already-manifested tree is stable.
pub fn generate(files: Vec<SourceFile>) -> std::io::Result<Manifest> {
    // BTreeMap: byte-order path sort + last-occurrence-wins on
    // duplicates (fallback sources listed first lose to their
    // shadowing copies), both in one pass.
    let by_path: std::collections::BTreeMap<String, PathBuf> =
        files.into_iter().map(|f| (f.rel, f.abs)).collect();
    let mut entries = Vec::with_capacity(by_path.len());
    for (rel, abs) in &by_path {
        if rel == MANIFEST_FILENAME || rel.ends_with(".gz") {
            continue;
        }
        let bytes = std::fs::read(abs)?;
        entries.push(ManifestEntry {
            path: rel.clone(),
            sha256: sha256_hex(&bytes),
            size: bytes.len() as u64,
            content_type: content_type_for(rel).to_string(),
            content_encoding: by_path
                .contains_key(&format!("{rel}.gz"))
                .then(|| "gzip".to_string()),
        });
    }
    let hash = entries_hash(&entries);
    Ok(Manifest {
        version: MANIFEST_VERSION,
        hash,
        entries,
    })
}

/// The canonical top-level hash: SHA-256 over each entry's fields in
/// declaration order, every field terminated by `\0` (which no field
/// can contain — OS paths exclude NUL, and the remaining fields are
/// fixed tokens, digits, or hex). Deliberately *not* a JSON
/// serialization: this encoding is trivially reproduced by any
/// implementation, in any language, with no escaping questions.
fn entries_hash(entries: &[ManifestEntry]) -> String {
    let mut hasher = Sha256::new();
    for entry in entries {
        for field in [
            entry.path.as_str(),
            entry.sha256.as_str(),
            &entry.size.to_string(),
            entry.content_type.as_str(),
            entry.content_encoding.as_deref().unwrap_or(""),
        ] {
            hasher.update(field.as_bytes());
            hasher.update([0]);
        }
    }
    hex_encode(&hasher.finalize())
}

/// Serialize deterministically: pretty 2-space JSON, trailing newline.
/// (Struct field order is fixed, entries are pre-sorted, and there are
/// no map types — serde_json's output is fully determined.)
pub fn serialize(manifest: &Manifest) -> String {
    // Infallible for this schema: no non-string map keys, no custom
    // serializers.
    let mut json =
        serde_json::to_string_pretty(manifest).expect("manifest serialization never fails");
    json.push('\n');
    json
}

/// Write `manifest` to `<dir>/spa-manifest.json`.
pub fn write_manifest(dir: &Path, manifest: &Manifest) -> std::io::Result<()> {
    std::fs::write(dir.join(MANIFEST_FILENAME), serialize(manifest))
}

/// Parse a manifest (the guest side reads its embedded copy).
pub fn parse(json: &str) -> Result<Manifest, serde_json::Error> {
    serde_json::from_str(json)
}

/// Lowercase hex SHA-256 of `bytes`.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex_encode(&hasher.finalize())
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        // `write!` to String never fails — unwrap is acceptable here.
        write!(s, "{:02x}", b).expect("writing to String never fails");
    }
    s
}

#[cfg(test)]
mod tests {
    //! Phase 2 generator specs (bd-ee2fqm95), moved here from
    //! `quarto-preview/tests/integration/asset_manifest.rs` per that
    //! file's placement note: the generator is shared between
    //! `build.rs` and (by byte-agreement) the npm build, so its tests
    //! live with the implementation. The mode-decision tests stay in
    //! quarto-preview, which owns the embedded manifests.
    use super::*;

    /// Write `files` (rel path → bytes) under a fresh tempdir.
    fn fixture_tree(files: &[(&str, &[u8])]) -> tempfile::TempDir {
        let dir = tempfile::TempDir::with_prefix("spa-manifest-test-").unwrap();
        for (rel, bytes) in files {
            let abs = dir.path().join(rel);
            std::fs::create_dir_all(abs.parent().unwrap()).unwrap();
            std::fs::write(abs, bytes).unwrap();
        }
        dir
    }

    /// A small tree exercising nesting, the content-type table, and a
    /// `.gz` sibling (which must fold into `content_encoding`, never
    /// appear as its own entry).
    fn standard_fixture() -> tempfile::TempDir {
        fixture_tree(&[
            ("index.html", b"<h1>hi</h1>"),
            ("index.html.gz", b"gzip-bytes"),
            ("assets/app.js", b"console.log(1)"),
            ("assets/wide.woff2", b"woff2-bytes"),
        ])
    }

    /// Design decision 4: generation is deterministic — sorted entries,
    /// stable hash — so regenerating over an unchanged tree is
    /// byte-identical (release CI builds on different platforms/jobs
    /// must produce equal hashes or cross-platform local mode silently
    /// disables).
    #[test]
    fn manifest_regeneration_is_byte_identical() {
        let dir = standard_fixture();
        let first = generate(list_dir(dir.path()).unwrap()).unwrap();
        let second = generate(list_dir(dir.path()).unwrap()).unwrap();
        assert_eq!(
            serialize(&first),
            serialize(&second),
            "regeneration over an unchanged tree must be byte-identical"
        );

        // Sorted by path, byte order.
        let paths: Vec<&str> = first.entries.iter().map(|e| e.path.as_str()).collect();
        let mut sorted = paths.clone();
        sorted.sort_unstable();
        assert_eq!(paths, sorted, "entries must be sorted by path");

        // The `.gz` sibling folded into the identity entry.
        let index = first
            .entries
            .iter()
            .find(|e| e.path == "index.html")
            .expect("index.html entry");
        assert_eq!(index.content_encoding.as_deref(), Some("gzip"));
        assert_eq!(index.size, 11);
        assert_eq!(index.content_type, "text/html; charset=utf-8");
        assert_eq!(index.sha256, sha256_hex(b"<h1>hi</h1>"));
        assert!(
            first.entries.iter().all(|e| !e.path.ends_with(".gz")),
            "compressed siblings are never listed: {:?}",
            paths
        );
        // No `.gz` for the woff2 (precompress skips already-compressed
        // containers; the fixture has none) → no content_encoding.
        let woff2 = first
            .entries
            .iter()
            .find(|e| e.path == "assets/wide.woff2")
            .expect("woff2 entry");
        assert_eq!(woff2.content_encoding, None);
        assert_eq!(woff2.content_type, "font/woff2");
    }

    /// The top-level hash is the compatibility signal: any asset byte
    /// change must change it (a guest with a stale embed must mismatch
    /// and fall back to tunneling).
    #[test]
    fn manifest_hash_changes_when_any_asset_byte_changes() {
        let dir = standard_fixture();
        let before = generate(list_dir(dir.path()).unwrap()).unwrap();
        std::fs::write(dir.path().join("assets/app.js"), b"console.log(2)").unwrap();
        let after = generate(list_dir(dir.path()).unwrap()).unwrap();
        assert_ne!(
            before.hash, after.hash,
            "one flipped asset byte must change the top-level hash"
        );
        // Untouched entries are unchanged.
        let before_index = before.entries.iter().find(|e| e.path == "index.html");
        let after_index = after.entries.iter().find(|e| e.path == "index.html");
        assert_eq!(before_index, after_index);
    }

    /// A manifest cannot contain its own hash: regeneration over a dist
    /// that already contains a previous manifest must exclude that file
    /// and reproduce the same top-level hash.
    #[test]
    fn manifest_excludes_itself_on_regeneration() {
        let dir = standard_fixture();
        let first = generate(list_dir(dir.path()).unwrap()).unwrap();
        write_manifest(dir.path(), &first).unwrap();
        assert!(dir.path().join(MANIFEST_FILENAME).is_file());

        let second = generate(list_dir(dir.path()).unwrap()).unwrap();
        assert!(
            second.entries.iter().all(|e| e.path != MANIFEST_FILENAME),
            "the manifest must never list itself"
        );
        assert_eq!(
            first.hash, second.hash,
            "regenerating over a manifested tree must not move the hash"
        );
    }

    /// The editor manifest records the *post-resolution* view — what
    /// `lookup_embedded(Editor, path)` actually returns: editor-embed
    /// files plus the viewer-embed fallback for stripped duplicates.
    /// Otherwise editor-mode guests spuriously mismatch (plan risk:
    /// editor/viewer dedupe).
    #[test]
    fn editor_manifest_covers_post_resolution_view() {
        // Viewer dist: the shared file + a viewer-only file.
        let viewer = fixture_tree(&[
            ("index.html", b"viewer index"),
            ("assets/shared.js", b"shared bytes"),
        ]);
        // Editor dist pre-dedupe: a byte-identical shared.js (the
        // dedupe target), an editor-only file, and its own index.html.
        // Post-dedupe embed: shared.js stripped (served via the viewer
        // fallback at runtime).
        let editor_embed = fixture_tree(&[
            ("index.html", b"editor index"),
            ("assets/editor-only.js", b"editor bytes"),
        ]);

        // build.rs order: fallback source (viewer) first, editor
        // second so the editor's own copy wins on conflicts.
        let mut files = list_dir(viewer.path()).unwrap();
        files.extend(list_dir(editor_embed.path()).unwrap());
        let manifest = generate(files).unwrap();

        let by_path = |p: &str| {
            manifest
                .entries
                .iter()
                .find(|e| e.path == p)
                .unwrap_or_else(|| panic!("entry {p} must exist"))
        };
        assert_eq!(manifest.entries.len(), 3);
        // The dedupe target is listed with the bytes the runtime
        // fallback serves — the viewer's.
        assert_eq!(
            by_path("assets/shared.js").sha256,
            sha256_hex(b"shared bytes")
        );
        assert_eq!(
            by_path("assets/editor-only.js").sha256,
            sha256_hex(b"editor bytes")
        );
        // On conflict the editor's own copy wins (never the viewer's).
        assert_eq!(by_path("index.html").sha256, sha256_hex(b"editor index"));
    }

    /// Pin the canonical hash formula with a known-answer vector. The
    /// npm-side generator (`scripts/manifest-dist.mjs`) must produce
    /// the same hash for the same tree; if this test's expected value
    /// ever needs to change, the formula moved and the two
    /// implementations must move together.
    #[test]
    fn top_level_hash_matches_pinned_formula_vector() {
        let dir = fixture_tree(&[
            ("index.html", b"<h1>hi</h1>"),
            ("assets/app.js", b"console.log(1)"),
        ]);
        let manifest = generate(list_dir(dir.path()).unwrap()).unwrap();
        // Expected value computed independently of this crate (Python
        // hashlib over the canonical `\0`-terminated field lines); see
        // the module docs for the formula.
        let expected = "1b01f547e2647a70a3d520a8c5dcd4b886efde051d70b685aae40df00d257799";
        assert_eq!(manifest.hash, expected);
    }

    /// Relative paths are `/`-separated on every platform — the
    /// manifest must be byte-identical no matter which OS built it.
    #[test]
    fn rel_paths_use_forward_slashes() {
        let dir = fixture_tree(&[("a/b/c.js", b"x")]);
        let manifest = generate(list_dir(dir.path()).unwrap()).unwrap();
        assert_eq!(manifest.entries.len(), 1);
        assert_eq!(manifest.entries[0].path, "a/b/c.js");
    }

    /// Round-trip: parse(serialize(m)) == m, and the serialized form is
    /// stable JSON with the documented field names.
    #[test]
    fn serialize_parse_roundtrip() {
        let dir = standard_fixture();
        let manifest = generate(list_dir(dir.path()).unwrap()).unwrap();
        let json = serialize(&manifest);
        assert!(json.ends_with('\n'));
        let parsed = parse(&json).unwrap();
        assert_eq!(parsed, manifest);
        // Field names are the wire contract (the npm generator and the
        // config endpoint test read them).
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["version"], 1);
        assert!(value["hash"].is_string());
        assert!(value["entries"][0]["contentType"].is_string());
        assert!(value["entries"][0]["sha256"].is_string());
    }
}
