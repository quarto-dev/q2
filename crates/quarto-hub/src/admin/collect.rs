//! `hub admin collect` / `restore` / `purge` — the mutating maintainer
//! tools (bd-eiku4ymo).
//!
//! Safety model (see `admin` module docs + the plan): `collect`
//! consumes **only** a scan manifest, re-verifies every candidate
//! against *current* storage, and **quarantines** — it renames each
//! doc's chunk directory into `<data_dir>/trash/<batch>/docs/<doc-id>`
//! and never unlinks anything. `restore` moves a batch back,
//! hash-verified. `purge` is the only unlink in the whole tool set:
//! whole trash batches, past a retention window, dry-run by default.
//!
//! All three acquire the server's own `hub.lock` (exclusive flock)
//! for their duration — a running server makes them refuse, and they
//! symmetrically prevent a server from starting mid-operation.
//!
//! The trash area lives inside the data dir on purpose: renames stay
//! on one filesystem (atomic-ish, no copy), the operator's existing
//! backup regime covers it, and a batch can never be orphaned from
//! its server.

use std::fs::File;
use std::path::{Path, PathBuf};

use fs2::FileExt as _;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use super::classify::DocKind;
use super::manifest::{CandidateDoc, MANIFEST_VERSION, ScanManifest};
use super::scan::{
    ScanOptions, list_doc_ids_filesystem, live_doc_ids, load_all_docs, recover_doc_id,
};
use crate::resource::read_capture_meta;

/// Version of the `batch.json` schema written into quarantine batches.
pub const BATCH_VERSION: u32 = 1;

/// Result of a collect run (dry or executed).
#[derive(Debug)]
pub struct CollectOutcome {
    /// Candidates that passed re-verification (quarantined when
    /// `execute`; the plan when dry-run).
    pub verified: Vec<CandidateDoc>,
    /// Candidates skipped at re-verification, with reasons.
    pub skipped: Vec<(String, String)>,
    /// The batch directory (only when `execute` and something moved).
    pub batch_dir: Option<PathBuf>,
}

/// `batch.json`: the audit record embedded in every quarantine batch.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchRecord {
    pub batch_version: u32,
    pub tool: String,
    /// RFC 3339 UTC creation time — `purge`'s retention gate reads this.
    pub created_at: String,
    pub data_dir: String,
    /// The scan manifest that authorized this batch, embedded whole.
    pub manifest: ScanManifest,
    /// Per-doc quarantine records.
    pub docs: Vec<BatchDoc>,
    /// Candidates skipped at re-verification (kept for the audit trail).
    pub skipped: Vec<BatchSkip>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchDoc {
    pub doc_id: String,
    /// Chunk files moved, relative to the doc's directory, with
    /// SHA-256 hashes — `restore` verifies these on the way back.
    pub chunks: Vec<BatchChunk>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchChunk {
    pub rel_path: String,
    pub sha256: String,
    pub bytes: u64,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchSkip {
    pub doc_id: String,
    pub reason: String,
}

/// Hold the server's own exclusive lock for the duration of a
/// mutating operation. Refuses (with a clear message) when a server —
/// or another admin operation — holds it.
pub struct AdminLock {
    _file: File,
}

impl AdminLock {
    pub fn acquire(data_dir: &Path) -> Result<Self, String> {
        let lock_path = data_dir.join("hub.lock");
        let file = File::options()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&lock_path)
            .map_err(|e| format!("cannot open {}: {e}", lock_path.display()))?;
        file.try_lock_exclusive().map_err(|_| {
            format!(
                "{} is locked — a hub server (or another admin command) is running \
                 against this data dir. Stop it first; mutating admin operations \
                 never run under a live server.",
                lock_path.display()
            )
        })?;
        Ok(Self { _file: file })
    }
}

/// The splayed on-disk directory of a doc's chunks:
/// `<automerge>/<first-2-chars>/<rest>` (samod's `key_to_path`).
///
/// Only used to *construct* destinations (restore). Anything that
/// removes or moves data must locate its source through
/// [`locate_all_doc_dirs`] instead: constructing a path from an id
/// and letting a case-insensitive filesystem resolve it is the lossy
/// channel that mis-identified documents in the first place
/// (bd-eb2wnxkp).
fn doc_dir(automerge_dir: &Path, doc_id: &str) -> PathBuf {
    let first_two: String = doc_id.chars().take(2).collect();
    let rest: String = doc_id.chars().skip(2).collect();
    automerge_dir.join(first_two).join(rest)
}

/// Belt-and-braces guard (bd-eb2wnxkp): map every recoverable doc id
/// in the store to the on-disk director(ies) whose *actual names*
/// round-trip to it via [`recover_doc_id`]. The quarantine path only
/// trusts this map — an id that does not round-trip to exactly one
/// real directory is refused regardless of its liveness verdict.
/// Pairs that fail recovery are skipped here (enumeration has already
/// hard-failed the collection on those before this runs).
fn locate_all_doc_dirs(
    automerge_dir: &Path,
) -> Result<std::collections::HashMap<String, Vec<PathBuf>>, String> {
    let mut map: std::collections::HashMap<String, Vec<PathBuf>> = std::collections::HashMap::new();
    let level1 = std::fs::read_dir(automerge_dir)
        .map_err(|e| format!("cannot read {}: {e}", automerge_dir.display()))?;
    for l1 in level1.flatten() {
        if !l1.path().is_dir() {
            continue;
        }
        let Some(prefix) = l1.file_name().to_str().map(str::to_string) else {
            continue;
        };
        let Ok(level2) = std::fs::read_dir(l1.path()) else {
            continue;
        };
        for l2 in level2.flatten() {
            if !l2.path().is_dir() {
                continue;
            }
            let name = l2.file_name();
            let Some(rest) = name.to_str() else {
                continue;
            };
            if let Ok(id) = recover_doc_id(&prefix, rest) {
                map.entry(id).or_default().push(l2.path());
            }
        }
    }
    Ok(map)
}

/// Resolve `doc_id` through the round-trip map: exactly one on-disk
/// directory, or a refusal reason.
fn locate_verified(
    doc_dirs: &std::collections::HashMap<String, Vec<PathBuf>>,
    doc_id: &str,
) -> Result<PathBuf, String> {
    match doc_dirs.get(doc_id).map(Vec::as_slice) {
        Some([one]) => Ok(one.clone()),
        None | Some([]) => Err(
            "id does not round-trip to any storage directory; refusing to quarantine".to_string(),
        ),
        Some(many) => Err(format!(
            "id round-trips to {} distinct storage directories; refusing to quarantine",
            many.len()
        )),
    }
}

/// Re-verify + quarantine. Dry-run unless `execute`.
///
/// `data_dir` is the hub data dir (contains `automerge/`, `hub.lock`,
/// and — after this — `trash/`).
pub async fn collect(
    data_dir: &Path,
    manifest: &ScanManifest,
    execute: bool,
) -> Result<CollectOutcome, String> {
    if manifest.manifest_version != MANIFEST_VERSION {
        return Err(format!(
            "manifest version {} is not supported by this tool (expected {})",
            manifest.manifest_version, MANIFEST_VERSION
        ));
    }
    // Paranoia gate: a manifest scanned elsewhere must not authorize
    // collection here.
    let canonical = data_dir
        .canonicalize()
        .map_err(|e| format!("cannot canonicalize {}: {e}", data_dir.display()))?;
    let manifest_dir = Path::new(&manifest.data_dir)
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from(&manifest.data_dir));
    if manifest_dir != canonical {
        return Err(format!(
            "manifest was scanned from {:?} but --data-dir is {:?}; refusing \
             (re-scan this data dir to produce a matching manifest)",
            manifest.data_dir, canonical
        ));
    }

    let _lock = AdminLock::acquire(&canonical)?;
    let automerge_dir = canonical.join("automerge");

    // Re-verification (principle 5): recompute kind + liveness + age
    // against CURRENT storage, with the manifest's own gate options.
    let storage = samod::storage::TokioFilesystemStorage::new(&automerge_dir);
    // A pair we cannot identify aborts the collection outright: the
    // collector must never act on a guessed doc id (bd-eb2wnxkp).
    let doc_ids =
        list_doc_ids_filesystem(&automerge_dir).map_err(|e| format!("refusing to collect: {e}"))?;
    let docs = load_all_docs(&storage, &doc_ids).await;
    let live = live_doc_ids(&docs);
    let opts = ScanOptions {
        older_than_days: manifest.older_than_days,
        include_unstamped: manifest.include_unstamped,
    };
    let now = chrono::Utc::now();

    // Defense in depth: every candidate must also round-trip to
    // exactly one real directory (checked again at rename time).
    let doc_dirs =
        locate_all_doc_dirs(&automerge_dir).map_err(|e| format!("refusing to collect: {e}"))?;

    let mut verified = Vec::new();
    let mut skipped = Vec::new();
    for candidate in &manifest.candidates {
        let reason = verify_candidate(candidate, &docs, &live, &opts, now)
            .and_then(|()| locate_verified(&doc_dirs, &candidate.doc_id).map(|_| ()));
        match reason {
            Ok(()) => verified.push(candidate.clone()),
            Err(why) => skipped.push((candidate.doc_id.clone(), why)),
        }
    }

    if !execute || verified.is_empty() {
        return Ok(CollectOutcome {
            verified,
            skipped,
            batch_dir: None,
        });
    }

    // Quarantine: one rename per doc directory, hashes recorded first.
    let batch_name = format!(
        "{}-scan{}",
        now.format("%Y%m%dT%H%M%SZ"),
        short_hash(&manifest.scanned_at)
    );
    let batch_dir = canonical.join("trash").join(&batch_name);
    let batch_docs_dir = batch_dir.join("docs");
    std::fs::create_dir_all(&batch_docs_dir)
        .map_err(|e| format!("cannot create {}: {e}", batch_docs_dir.display()))?;

    let mut batch_docs = Vec::new();
    for candidate in &verified {
        // The rename source comes from the round-trip map — the real
        // on-disk directory names — never from doc_dir construction,
        // which a case-insensitive filesystem would resolve even for a
        // mis-identified id (bd-eb2wnxkp).
        let src = locate_verified(&doc_dirs, &candidate.doc_id)
            .map_err(|why| format!("{}: {why}", candidate.doc_id))?;
        let chunks =
            hash_dir_files(&src).map_err(|e| format!("hashing {} failed: {e}", src.display()))?;
        let dst = batch_docs_dir.join(&candidate.doc_id);
        std::fs::rename(&src, &dst).map_err(|e| {
            format!(
                "quarantine rename {} -> {} failed: {e} (already-moved docs remain \
                 quarantined in {})",
                src.display(),
                dst.display(),
                batch_dir.display()
            )
        })?;
        // Best-effort: drop the now-possibly-empty 2-char splay dir.
        if let Some(parent) = src.parent() {
            let _ = std::fs::remove_dir(parent); // fails (kept) unless empty
        }
        batch_docs.push(BatchDoc {
            doc_id: candidate.doc_id.clone(),
            chunks,
        });
    }

    let record = BatchRecord {
        batch_version: BATCH_VERSION,
        tool: format!("hub admin collect {}", env!("CARGO_PKG_VERSION")),
        created_at: now.to_rfc3339(),
        data_dir: canonical.to_string_lossy().into_owned(),
        manifest: manifest.clone(),
        docs: batch_docs,
        skipped: skipped
            .iter()
            .map(|(doc_id, reason)| BatchSkip {
                doc_id: doc_id.clone(),
                reason: reason.clone(),
            })
            .collect(),
    };
    let record_json =
        serde_json::to_vec_pretty(&record).map_err(|e| format!("serializing batch.json: {e}"))?;
    std::fs::write(batch_dir.join("batch.json"), record_json)
        .map_err(|e| format!("writing batch.json: {e}"))?;

    Ok(CollectOutcome {
        verified,
        skipped,
        batch_dir: Some(batch_dir),
    })
}

/// One candidate's re-verification. `Ok(())` = still safely removable.
fn verify_candidate(
    candidate: &CandidateDoc,
    docs: &[super::scan::LoadedDoc],
    live: &std::collections::HashSet<String>,
    opts: &ScanOptions,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<(), String> {
    let Some(current) = docs.iter().find(|d| d.doc_id == candidate.doc_id) else {
        return Err("no longer exists in storage".to_string());
    };
    if current.kind != DocKind::EngineCapture {
        return Err(format!(
            "kind is now {:?}, not engine-capture",
            current.kind.as_str()
        ));
    }
    if live.contains(&candidate.doc_id) {
        return Err("became referenced since the scan".to_string());
    }
    let created_at = read_capture_meta(&current.doc).and_then(|m| m.created_at);
    match created_at {
        Some(ts) => {
            let t = chrono::DateTime::parse_from_rfc3339(&ts)
                .map_err(|e| format!("unparseable createdAt {ts:?}: {e}"))?;
            let age_days = (now - t.with_timezone(&chrono::Utc)).num_days();
            if age_days <= opts.older_than_days {
                return Err(format!(
                    "age {age_days}d no longer passes the {}d gate",
                    opts.older_than_days
                ));
            }
        }
        None => {
            if !opts.include_unstamped {
                return Err("unstamped and manifest did not include unstamped".to_string());
            }
        }
    }
    Ok(())
}

/// Restore a quarantine batch (or a subset of its docs) back into the
/// automerge store. Hash-verified; refuses per-doc when the target
/// directory already exists (a doc re-created under the same id).
pub fn restore(
    data_dir: &Path,
    batch_dir: &Path,
    only_doc_ids: &[String],
) -> Result<Vec<(String, Result<(), String>)>, String> {
    let canonical = data_dir
        .canonicalize()
        .map_err(|e| format!("cannot canonicalize {}: {e}", data_dir.display()))?;
    let _lock = AdminLock::acquire(&canonical)?;
    let automerge_dir = canonical.join("automerge");

    let record: BatchRecord = serde_json::from_slice(
        &std::fs::read(batch_dir.join("batch.json"))
            .map_err(|e| format!("reading batch.json: {e}"))?,
    )
    .map_err(|e| format!("parsing batch.json: {e}"))?;

    let mut results = Vec::new();
    for doc in &record.docs {
        if !only_doc_ids.is_empty() && !only_doc_ids.contains(&doc.doc_id) {
            continue;
        }
        results.push((
            doc.doc_id.clone(),
            restore_one(&automerge_dir, batch_dir, doc),
        ));
    }
    Ok(results)
}

fn restore_one(automerge_dir: &Path, batch_dir: &Path, doc: &BatchDoc) -> Result<(), String> {
    let src = batch_dir.join("docs").join(&doc.doc_id);
    if !src.is_dir() {
        return Err("not present in this batch (already restored?)".to_string());
    }
    // Verify hashes BEFORE moving anything back.
    let current = hash_dir_files(&src).map_err(|e| format!("hashing quarantined chunks: {e}"))?;
    let expected: std::collections::BTreeMap<&str, &str> = doc
        .chunks
        .iter()
        .map(|c| (c.rel_path.as_str(), c.sha256.as_str()))
        .collect();
    for chunk in &current {
        match expected.get(chunk.rel_path.as_str()) {
            Some(hash) if *hash == chunk.sha256 => {}
            Some(_) => {
                return Err(format!(
                    "chunk {} hash mismatch — quarantined bytes were modified; refusing",
                    chunk.rel_path
                ));
            }
            None => {
                return Err(format!(
                    "chunk {} not in batch record — refusing",
                    chunk.rel_path
                ));
            }
        }
    }
    let dst = doc_dir(automerge_dir, &doc.doc_id);
    if dst.exists() {
        return Err(format!(
            "{} already exists (doc re-created since collection); refusing to overwrite",
            dst.display()
        ));
    }
    if let Some(parent) = dst.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("mkdir {}: {e}", parent.display()))?;
    }
    std::fs::rename(&src, &dst).map_err(|e| format!("restore rename failed: {e}"))
}

/// A trash batch eligible (or not) for purging.
#[derive(Debug)]
pub struct PurgeCandidate {
    pub batch_dir: PathBuf,
    pub created_at: Option<String>,
    pub age_days: Option<i64>,
    pub eligible: bool,
}

/// List trash batches and, with `execute`, delete those older than
/// `retention_days`. THE ONLY UNLINK IN THE TOOL SET. Batch-level
/// only — never individual docs, never anything outside `trash/`.
pub fn purge(
    data_dir: &Path,
    retention_days: i64,
    execute: bool,
) -> Result<Vec<PurgeCandidate>, String> {
    let canonical = data_dir
        .canonicalize()
        .map_err(|e| format!("cannot canonicalize {}: {e}", data_dir.display()))?;
    let _lock = AdminLock::acquire(&canonical)?;
    let trash = canonical.join("trash");
    let now = chrono::Utc::now();

    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(&trash) else {
        return Ok(out); // no trash dir: nothing to purge
    };
    for entry in entries.flatten() {
        let batch_dir = entry.path();
        if !batch_dir.is_dir() {
            continue;
        }
        let created_at: Option<String> = std::fs::read(batch_dir.join("batch.json"))
            .ok()
            .and_then(|bytes| serde_json::from_slice::<BatchRecord>(&bytes).ok())
            .map(|r| r.created_at);
        let age_days = created_at.as_deref().and_then(|ts| {
            chrono::DateTime::parse_from_rfc3339(ts)
                .ok()
                .map(|t| (now - t.with_timezone(&chrono::Utc)).num_days())
        });
        // A batch with no readable batch.json has no age evidence:
        // never eligible (same protective stance as unstamped captures).
        let eligible = age_days.is_some_and(|d| d > retention_days);
        if eligible && execute {
            std::fs::remove_dir_all(&batch_dir)
                .map_err(|e| format!("purging {}: {e}", batch_dir.display()))?;
        }
        out.push(PurgeCandidate {
            batch_dir,
            created_at,
            age_days,
            eligible,
        });
    }
    Ok(out)
}

fn short_hash(input: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    hex::encode(hasher.finalize())[..8].to_string()
}

/// Hash every regular file under `dir` (recursively), keyed by
/// forward-slash relative path.
fn hash_dir_files(dir: &Path) -> std::io::Result<Vec<BatchChunk>> {
    fn walk(root: &Path, dir: &Path, out: &mut Vec<BatchChunk>) -> std::io::Result<()> {
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                walk(root, &path, out)?;
            } else if path.is_file() {
                let bytes = std::fs::read(&path)?;
                let mut hasher = Sha256::new();
                hasher.update(&bytes);
                let rel = path
                    .strip_prefix(root)
                    .expect("walk stays under root")
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy().into_owned())
                    .collect::<Vec<_>>()
                    .join("/");
                out.push(BatchChunk {
                    rel_path: rel,
                    sha256: hex::encode(hasher.finalize()),
                    bytes: bytes.len() as u64,
                });
            }
        }
        Ok(())
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out)?;
    out.sort_by(|a, b| a.rel_path.cmp(&b.rel_path));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Checksum-verified real id (see scan.rs tests): the case-flip
    /// `2CPAD…` fails to parse, so recovery is unambiguous.
    const TRUE_ID: &str = "2cPADPZ85aBLaaLaLrS2BNcVza1n";

    fn add_doc_dir(automerge_dir: &Path, l1: &str, l2: &str) {
        let dir = automerge_dir.join(l1).join(l2);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("snapshot"), b"x").unwrap();
    }

    #[test]
    fn locate_resolves_folded_dir_by_actual_name() {
        let temp = tempfile::TempDir::new().unwrap();
        let automerge_dir = temp.path().join("automerge");
        // On-disk casing is folded ("2C"); the id's true prefix is "2c".
        add_doc_dir(&automerge_dir, "2C", &TRUE_ID[2..]);
        let map = locate_all_doc_dirs(&automerge_dir).unwrap();
        let path = locate_verified(&map, TRUE_ID).unwrap();
        // The located path carries the REAL directory name, not a
        // constructed one — this is what makes the rename sound even
        // on case-sensitive filesystems holding a folded store.
        assert!(path.ends_with(Path::new("2C").join(&TRUE_ID[2..])));
    }

    #[test]
    fn locate_refuses_id_absent_from_storage() {
        let temp = tempfile::TempDir::new().unwrap();
        let automerge_dir = temp.path().join("automerge");
        std::fs::create_dir_all(&automerge_dir).unwrap();
        let map = locate_all_doc_dirs(&automerge_dir).unwrap();
        let err = locate_verified(&map, TRUE_ID).unwrap_err();
        assert!(err.contains("does not round-trip"), "got: {err}");
    }

    #[test]
    fn locate_refuses_ambiguous_duplicate_dirs() {
        // Two directories that both round-trip to the same id can only
        // coexist on a case-SENSITIVE filesystem (a folded store copied
        // from macOS, say). Where the filesystem folds them into one,
        // the single entry must resolve fine — branch on what the
        // filesystem actually did.
        let temp = tempfile::TempDir::new().unwrap();
        let automerge_dir = temp.path().join("automerge");
        add_doc_dir(&automerge_dir, "2c", &TRUE_ID[2..]);
        add_doc_dir(&automerge_dir, "2C", &TRUE_ID[2..]);
        let level1_entries = std::fs::read_dir(&automerge_dir).unwrap().count();
        let map = locate_all_doc_dirs(&automerge_dir).unwrap();
        match level1_entries {
            2 => {
                let err = locate_verified(&map, TRUE_ID).unwrap_err();
                assert!(err.contains("distinct storage directories"), "got: {err}");
            }
            1 => {
                locate_verified(&map, TRUE_ID).unwrap();
            }
            n => panic!("unexpected level-1 entry count {n}"),
        }
    }
}
