//! `hub admin scan` — read-only orphan analysis of a samod storage
//! location (bd-eiku4ymo).
//!
//! Enumerates every stored document through the same [`Storage`]
//! adapter the server uses, classifies each by shape
//! ([`classify`](super::classify)), computes liveness as the closure
//! of the reference graph from the roots (project indexes + project
//! sets), and reports orphaned engine-capture docs past the age gate
//! as removable candidates.
//!
//! Safe to run against a live server: strictly read-only, snapshot
//! semantics. (No stronger consistency exists anywhere in this system
//! — an offline store can equally be behind a peer.) The collector
//! re-verifies against current storage before acting, so scan-time
//! staleness cannot cause an unsafe collection.

use std::collections::{BTreeMap, HashMap, HashSet};

use automerge::Automerge;
use samod::storage::{Storage, StorageKey};

use super::classify::{DocKind, classify, referenced_doc_ids};
use super::manifest::{
    CandidateDoc, CandidateMeta, KindStats, MANIFEST_VERSION, ReportedDoc, ScanManifest,
};
use crate::resource::read_capture_meta;

/// Options for a scan run.
#[derive(Debug, Clone)]
pub struct ScanOptions {
    /// Minimum age (days since `meta.createdAt`) for a capture to be
    /// collectible. Decision (2026-07-24): default 30.
    pub older_than_days: i64,
    /// Whether captures without a `meta` envelope (pre-bd-eiku4ymo)
    /// are eligible. Default false: no timestamp means no age
    /// evidence, so they are protected unless the operator opts in.
    pub include_unstamped: bool,
}

impl Default for ScanOptions {
    fn default() -> Self {
        Self {
            older_than_days: 30,
            include_unstamped: false,
        }
    }
}

/// One stored document, loaded and classified.
#[derive(Debug)]
pub struct LoadedDoc {
    pub doc_id: String,
    pub kind: DocKind,
    pub doc: Automerge,
    /// Sum of this doc's chunk sizes on storage.
    pub size_bytes: u64,
    /// Number of chunks that failed to parse (fail-soft, mirroring
    /// samod's own loader). A doc whose every chunk fails loads empty
    /// and classifies as Unknown — i.e. protected.
    pub bad_chunks: usize,
}

/// Enumerate document ids from a **filesystem** samod store.
///
/// Enumeration is deliberately NOT done through
/// `Storage::load_range([])`: samod's filesystem adapter splays a
/// key's first component across two path segments
/// (`key_to_path`), and its `load_range` rebuilds keys from raw path
/// components — so an empty-prefix listing returns the doc id split
/// in half. samod itself only ever uses per-doc prefixes (where the
/// mapping round-trips); whole-store enumeration is simply outside
/// the `Storage` contract. So we enumerate by mirroring the splay:
/// doc id = `<level-1 dir name>` + `<level-2 dir name>`. Per-doc
/// chunk loading then goes through the adapter (`load_range([id])`),
/// which is correct on every backend.
///
/// The adapter's own identity record (`st/orage-adapter-id`) is a
/// *file* at level 2, not a directory, so the `is_dir` requirement
/// skips it structurally.
pub fn list_doc_ids_filesystem(automerge_dir: &std::path::Path) -> Vec<String> {
    let mut out = Vec::new();
    let Ok(level1) = std::fs::read_dir(automerge_dir) else {
        return out;
    };
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
            if let Some(rest) = l2.file_name().to_str() {
                out.push(format!("{prefix}{rest}"));
            }
        }
    }
    out.sort();
    out
}

/// Load and classify the given documents through the storage adapter.
/// Shared by scan and by the collector's re-verification pass.
///
/// Enumeration is the caller's job (backend-specific — see
/// [`list_doc_ids_filesystem`]); chunk loading uses per-doc
/// `load_range`, which round-trips correctly on every adapter.
pub async fn load_all_docs<S: Storage>(storage: &S, doc_ids: &[String]) -> Vec<LoadedDoc> {
    let mut out = Vec::new();
    for doc_id in doc_ids {
        let Ok(prefix) = StorageKey::from_parts([doc_id.as_str()]) else {
            tracing::warn!(doc_id = %doc_id, "scan: id not a valid storage key; skipping");
            continue;
        };
        let chunk_map = storage.load_range(prefix).await;
        let mut chunks: Vec<(Vec<String>, Vec<u8>)> = chunk_map
            .into_iter()
            .map(|(key, bytes)| ((&key).into_iter().cloned().collect(), bytes))
            .collect();
        if chunks.is_empty() {
            continue;
        }
        // Snapshots load before incrementals (samod's own order);
        // then key order, for deterministic runs.
        chunks.sort_by_key(|(parts, _)| {
            (parts.get(1).is_none_or(|s| s != "snapshot"), parts.clone())
        });
        let mut doc = Automerge::new();
        let mut size_bytes = 0u64;
        let mut bad_chunks = 0usize;
        for (parts, bytes) in &chunks {
            size_bytes += bytes.len() as u64;
            if let Err(e) = doc.load_incremental(bytes) {
                bad_chunks += 1;
                tracing::warn!(
                    doc_id = %doc_id,
                    key = %parts.join("/"),
                    error = %e,
                    "scan: bad storage chunk (doc may classify as unknown)"
                );
            }
        }
        let kind = classify(&doc);
        out.push(LoadedDoc {
            doc_id: doc_id.clone(),
            kind,
            doc,
            size_bytes,
            bad_chunks,
        });
    }
    out
}

/// Compute the live set: closure of the reference graph from the
/// roots (project indexes + project sets). Today only roots have
/// outgoing references, but the closure is a proper worklist so a
/// future referencing kind stays correct automatically.
pub fn live_doc_ids(docs: &[LoadedDoc]) -> HashSet<String> {
    let by_id: HashMap<&str, &LoadedDoc> = docs.iter().map(|d| (d.doc_id.as_str(), d)).collect();

    let mut live: HashSet<String> = HashSet::new();
    let mut worklist: Vec<&LoadedDoc> = docs.iter().filter(|d| d.kind.is_root()).collect();
    for d in &worklist {
        live.insert(d.doc_id.clone());
    }
    while let Some(d) = worklist.pop() {
        for referenced in referenced_doc_ids(&d.doc) {
            if live.insert(referenced.clone())
                && let Some(next) = by_id.get(referenced.as_str())
            {
                worklist.push(next);
            }
        }
    }
    live
}

/// Whether a capture doc passes the age gate. Returns
/// `(passes, evidence-string)`.
fn age_gate(
    meta_created_at: Option<&str>,
    now: chrono::DateTime<chrono::Utc>,
    opts: &ScanOptions,
) -> (bool, String) {
    match meta_created_at {
        Some(created_at) => match chrono::DateTime::parse_from_rfc3339(created_at) {
            Ok(t) => {
                let age_days = (now - t.with_timezone(&chrono::Utc)).num_days();
                (
                    age_days > opts.older_than_days,
                    format!("age {age_days}d vs gate {}d", opts.older_than_days),
                )
            }
            Err(_) => (false, format!("unparseable createdAt {created_at:?}")),
        },
        None => (
            opts.include_unstamped,
            if opts.include_unstamped {
                "unstamped (pre-envelope), included by --include-unstamped".to_string()
            } else {
                "unstamped (pre-envelope), excluded by default".to_string()
            },
        ),
    }
}

/// Run a scan and produce the manifest.
///
/// `data_dir` is recorded (canonicalized by the caller) so `collect`
/// can refuse a manifest pointed at the wrong server.
pub async fn scan<S: Storage>(
    storage: &S,
    doc_ids: &[String],
    data_dir: &str,
    opts: &ScanOptions,
) -> ScanManifest {
    let docs = load_all_docs(storage, doc_ids).await;
    let live = live_doc_ids(&docs);
    let now = chrono::Utc::now();

    let mut inventory: BTreeMap<String, KindStats> = BTreeMap::new();
    let mut candidates = Vec::new();
    let mut not_collectible = Vec::new();

    for d in &docs {
        let stats = inventory.entry(d.kind.as_str().to_string()).or_default();
        stats.count += 1;
        stats.bytes += d.size_bytes;

        if live.contains(&d.doc_id) {
            continue;
        }
        // Unreferenced. Only engine captures are ever collectible
        // (allowlist); everything else is reported.
        if d.kind != DocKind::EngineCapture {
            not_collectible.push(ReportedDoc {
                doc_id: d.doc_id.clone(),
                kind: d.kind.as_str().to_string(),
                size_bytes: d.size_bytes,
                reason: format!(
                    "unreferenced, but kind {:?} is not collectible in v1",
                    d.kind.as_str()
                ),
            });
            continue;
        }
        let meta = read_capture_meta(&d.doc);
        let created_at = meta.as_ref().and_then(|m| m.created_at.clone());
        let (passes, age_evidence) = age_gate(created_at.as_deref(), now, opts);
        let manifest_meta = meta.map(|m| CandidateMeta {
            created_at: m.created_at,
            source_path: m.source_path,
            engines: m.engines,
        });
        if passes {
            candidates.push(CandidateDoc {
                doc_id: d.doc_id.clone(),
                kind: d.kind.as_str().to_string(),
                size_bytes: d.size_bytes,
                meta: manifest_meta,
                reason: format!(
                    "capture MIME; not referenced by any project index, captures \
                     sidecar, or project set; {age_evidence}"
                ),
            });
        } else {
            not_collectible.push(ReportedDoc {
                doc_id: d.doc_id.clone(),
                kind: d.kind.as_str().to_string(),
                size_bytes: d.size_bytes,
                reason: format!("unreferenced capture, but {age_evidence}"),
            });
        }
    }

    ScanManifest {
        manifest_version: MANIFEST_VERSION,
        tool: format!("hub admin scan {}", env!("CARGO_PKG_VERSION")),
        scanned_at: now.to_rfc3339(),
        data_dir: data_dir.to_string(),
        older_than_days: opts.older_than_days,
        include_unstamped: opts.include_unstamped,
        inventory,
        candidates,
        not_collectible,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use automerge::transaction::Transactable;
    use automerge::{ObjType, ROOT};
    use samod::storage::InMemoryStorage;

    use crate::resource::{
        CAPTURE_MIME_TYPE, CaptureDocMeta, create_binary_document, create_capture_document,
        create_capture_document_at,
    };

    /// Store `doc` as a single snapshot chunk under samod's key
    /// layout. (Scan treats doc ids as opaque strings, so tests can
    /// use readable ids.)
    async fn put_doc(storage: &InMemoryStorage, doc_id: &str, doc: &mut Automerge) {
        let key = StorageKey::from_parts([doc_id, "snapshot", "h1"]).unwrap();
        storage.put(key, doc.save()).await;
    }

    fn index_doc(files: &[(&str, &str)], captures: &[(&str, &str)]) -> Automerge {
        let mut doc = Automerge::new();
        doc.transact::<_, _, automerge::AutomergeError>(|tx| {
            let files_obj = tx.put_object(ROOT, "files", ObjType::Map)?;
            for (path, id) in files {
                tx.put(&files_obj, *path, *id)?;
            }
            if !captures.is_empty() {
                let caps_obj = tx.put_object(ROOT, "captures", ObjType::Map)?;
                for (path, id) in captures {
                    let entry = tx.put_object(&caps_obj, *path, ObjType::Map)?;
                    tx.put(&entry, "captureDocId", *id)?;
                }
            }
            Ok(())
        })
        .unwrap();
        doc
    }

    fn old_capture() -> Automerge {
        create_capture_document_at(
            b"gz",
            &CaptureDocMeta {
                source_path: "a.qmd".into(),
                engines: vec!["knitr".into()],
            },
            "2020-01-01T00:00:00+00:00",
        )
        .unwrap()
    }

    /// A storage with one project: index → {text file, live capture},
    /// plus one ORPHANED old capture and one legacy (unstamped)
    /// orphaned capture.
    async fn fixture_storage() -> (InMemoryStorage, Vec<String>) {
        let storage = InMemoryStorage::new();
        put_doc(
            &storage,
            "idx1",
            &mut index_doc(&[("a.qmd", "fileA")], &[("a.qmd", "capLive")]),
        )
        .await;
        let mut text = Automerge::new();
        text.transact::<_, _, automerge::AutomergeError>(|tx| {
            let t = tx.put_object(ROOT, "text", ObjType::Text)?;
            tx.update_text(&t, "hello")?;
            Ok(())
        })
        .unwrap();
        put_doc(&storage, "fileA", &mut text).await;
        put_doc(&storage, "capLive", &mut old_capture()).await;
        put_doc(&storage, "capOrphan", &mut old_capture()).await;
        put_doc(
            &storage,
            "capLegacy",
            &mut create_binary_document(b"gz", CAPTURE_MIME_TYPE).unwrap(),
        )
        .await;
        let ids = ["idx1", "fileA", "capLive", "capOrphan", "capLegacy"]
            .map(str::to_string)
            .to_vec();
        (storage, ids)
    }

    #[tokio::test]
    async fn scan_finds_only_the_orphaned_stamped_capture() {
        let (storage, ids) = fixture_storage().await;
        let m = scan(&storage, &ids, "/tmp/x", &ScanOptions::default()).await;

        let ids: Vec<&str> = m.candidates.iter().map(|c| c.doc_id.as_str()).collect();
        assert_eq!(ids, vec!["capOrphan"], "manifest: {m:#?}");
        // The live capture and the file doc are inventoried, not candidates.
        assert_eq!(m.inventory["engine-capture"].count, 3);
        assert_eq!(m.inventory["project-index"].count, 1);
        assert_eq!(m.inventory["text-file"].count, 1);
        // The legacy capture is reported as protected.
        assert!(
            m.not_collectible
                .iter()
                .any(|r| r.doc_id == "capLegacy" && r.reason.contains("unstamped")),
            "manifest: {m:#?}"
        );
        // Evidence strings are present.
        assert!(m.candidates[0].reason.contains("not referenced"));
        assert_eq!(
            m.candidates[0]
                .meta
                .as_ref()
                .unwrap()
                .source_path
                .as_deref(),
            Some("a.qmd")
        );
    }

    #[tokio::test]
    async fn include_unstamped_makes_legacy_capture_a_candidate() {
        let (storage, ids) = fixture_storage().await;
        let m = scan(
            &storage,
            &ids,
            "/tmp/x",
            &ScanOptions {
                include_unstamped: true,
                ..Default::default()
            },
        )
        .await;
        let mut ids: Vec<&str> = m.candidates.iter().map(|c| c.doc_id.as_str()).collect();
        ids.sort_unstable();
        assert_eq!(ids, vec!["capLegacy", "capOrphan"]);
    }

    #[tokio::test]
    async fn young_capture_is_protected_by_age_gate() {
        let storage = InMemoryStorage::new();
        // Freshly-stamped orphan (createdAt = now).
        put_doc(
            &storage,
            "capYoung",
            &mut create_capture_document(
                b"gz",
                &CaptureDocMeta {
                    source_path: "y.qmd".into(),
                    engines: vec![],
                },
            )
            .unwrap(),
        )
        .await;
        let m = scan(
            &storage,
            &["capYoung".to_string()],
            "/tmp/x",
            &ScanOptions::default(),
        )
        .await;
        assert!(m.candidates.is_empty());
        assert!(
            m.not_collectible
                .iter()
                .any(|r| r.doc_id == "capYoung" && r.reason.contains("age")),
            "manifest: {m:#?}"
        );
    }

    #[tokio::test]
    async fn unreferenced_unknown_and_binary_docs_are_protected() {
        let storage = InMemoryStorage::new();
        let mut unknown = Automerge::new();
        unknown
            .transact::<_, _, automerge::AutomergeError>(|tx| {
                tx.put_object(ROOT, "widgets", ObjType::Map)?;
                Ok(())
            })
            .unwrap();
        put_doc(&storage, "mystery", &mut unknown).await;
        put_doc(
            &storage,
            "img",
            &mut create_binary_document(b"png", "image/png").unwrap(),
        )
        .await;

        let m = scan(
            &storage,
            &["mystery".to_string(), "img".to_string()],
            "/tmp/x",
            &ScanOptions::default(),
        )
        .await;
        assert!(m.candidates.is_empty());
        assert_eq!(m.not_collectible.len(), 2);
    }

    #[tokio::test]
    async fn project_set_keeps_its_indexes_and_their_captures_live() {
        let storage = InMemoryStorage::new();
        let mut set = Automerge::new();
        set.transact::<_, _, automerge::AutomergeError>(|tx| {
            let projects = tx.put_object(ROOT, "projects", ObjType::Map)?;
            let entry = tx.put_object(&projects, "idx1", ObjType::Map)?;
            tx.put(&entry, "indexDocId", "idx1")?;
            Ok(())
        })
        .unwrap();
        put_doc(&storage, "set1", &mut set).await;
        put_doc(&storage, "idx1", &mut index_doc(&[], &[("a.qmd", "capA")])).await;
        put_doc(&storage, "capA", &mut old_capture()).await;

        let m = scan(
            &storage,
            &["set1".to_string(), "idx1".to_string(), "capA".to_string()],
            "/tmp/x",
            &ScanOptions::default(),
        )
        .await;
        assert!(
            m.candidates.is_empty(),
            "capA is live via set1 → idx1 → captures; manifest: {m:#?}"
        );
    }

    #[tokio::test]
    async fn manifest_roundtrips_through_json() {
        let (storage, ids) = fixture_storage().await;
        let m = scan(&storage, &ids, "/tmp/x", &ScanOptions::default()).await;
        let json = serde_json::to_string_pretty(&m).unwrap();
        let back: ScanManifest = serde_json::from_str(&json).unwrap();
        assert_eq!(back.manifest_version, MANIFEST_VERSION);
        assert_eq!(back.candidates.len(), m.candidates.len());
        // Wire format is camelCase (hub-client admin page contract).
        assert!(json.contains("\"manifestVersion\""));
        assert!(json.contains("\"sizeBytes\""));
    }
}

/// Human summary of a manifest for terminal output.
pub fn human_summary(m: &ScanManifest) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "Scanned {} at {}\n\nInventory:\n",
        m.data_dir, m.scanned_at
    ));
    for (kind, stats) in &m.inventory {
        out.push_str(&format!(
            "  {kind:<16} {:>6} docs  {:>12} bytes\n",
            stats.count, stats.bytes
        ));
    }
    let reclaimable: u64 = m.candidates.iter().map(|c| c.size_bytes).sum();
    out.push_str(&format!(
        "\nRemovable candidates: {} ({} bytes reclaimable)\n",
        m.candidates.len(),
        reclaimable
    ));
    for c in &m.candidates {
        let path = c
            .meta
            .as_ref()
            .and_then(|meta| meta.source_path.as_deref())
            .unwrap_or("<unstamped>");
        out.push_str(&format!(
            "  {}  {:>10} bytes  {}\n",
            c.doc_id, c.size_bytes, path
        ));
    }
    if !m.not_collectible.is_empty() {
        out.push_str(&format!(
            "\nUnreferenced but protected (not collectible): {}\n",
            m.not_collectible.len()
        ));
        for r in &m.not_collectible {
            out.push_str(&format!("  {}  {}  {}\n", r.doc_id, r.kind, r.reason));
        }
    }
    out
}
