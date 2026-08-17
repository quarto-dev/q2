//! Full collect → restore → purge lifecycle against a REAL samod
//! filesystem store (bd-eiku4ymo), exercising every damage-
//! minimization gate: dry-run inertness, re-verification skips,
//! quarantine (no unlink), hash-verified restore, retention-gated
//! purge, and the hub.lock guard.

use std::path::{Path, PathBuf};

use quarto_hub::admin::collect::{collect, purge, restore};
use quarto_hub::admin::scan::{ScanOptions, list_doc_ids_filesystem, scan};
use quarto_hub::context::{HubConfig, HubContext};
use quarto_hub::index::CaptureRef;
use quarto_hub::resource::{CaptureDocMeta, create_capture_document_at};
use quarto_hub::storage::StorageManager;
use samod::storage::TokioFilesystemStorage;
use tempfile::TempDir;

fn old_capture_doc() -> automerge::Automerge {
    create_capture_document_at(
        b"gzipped-capture-bytes",
        &CaptureDocMeta {
            source_path: "a.qmd".into(),
            engines: vec!["knitr".into()],
        },
        "2020-01-01T00:00:00+00:00",
    )
    .unwrap()
}

/// Build a real store with one live and one orphaned capture; return
/// (project tempdir, hub data dir, orphan doc id, live doc id).
async fn build_store() -> (TempDir, PathBuf, String, String) {
    let temp = TempDir::new().unwrap();
    let project_root = temp.path().join("project");
    std::fs::create_dir(&project_root).unwrap();
    std::fs::write(project_root.join("a.qmd"), "# Hello\n").unwrap();

    let storage = StorageManager::new(&project_root).unwrap();
    let hub_dir = storage.hub_dir().to_path_buf();
    let ctx = HubContext::new(storage, HubConfig::default())
        .await
        .unwrap();

    let live = ctx.repo().create(old_capture_doc()).await.unwrap();
    let live_id = live.document_id().to_string();
    ctx.index()
        .set_capture(
            "a.qmd",
            &CaptureRef {
                capture_doc_id: live_id.clone(),
                staleness: Some(false),
                state: None,
                last_error: None,
            },
        )
        .unwrap();
    let orphan = ctx.repo().create(old_capture_doc()).await.unwrap();
    let orphan_id = orphan.document_id().to_string();
    drop(orphan);
    drop(live);
    ctx.repo().stop().await;
    (temp, hub_dir, orphan_id, live_id)
}

/// Recursive sorted listing of (relative path, size) — a cheap tree
/// fingerprint for dry-run inertness checks.
fn tree_fingerprint(dir: &Path) -> Vec<(String, u64)> {
    fn walk(root: &Path, dir: &Path, out: &mut Vec<(String, u64)>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(root, &path, out);
            } else if let Ok(meta) = path.metadata() {
                let rel = path
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned();
                // The (empty) lockfile is admin infrastructure:
                // AdminLock creates it if absent, exactly as a server
                // start would. It is not data.
                if rel == "hub.lock" {
                    continue;
                }
                out.push((rel, meta.len()));
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out);
    out.sort();
    out
}

async fn scan_store(hub_dir: &Path) -> quarto_hub::admin::manifest::ScanManifest {
    let automerge_dir = hub_dir.join("automerge");
    let storage = TokioFilesystemStorage::new(&automerge_dir);
    let ids = list_doc_ids_filesystem(&automerge_dir).unwrap();
    // The manifest's dataDir must be the HUB dir (collect's contract).
    scan(
        &storage,
        &ids,
        &hub_dir.canonicalize().unwrap().to_string_lossy(),
        &ScanOptions::default(),
    )
    .await
}

#[tokio::test(flavor = "multi_thread")]
async fn collect_lifecycle_quarantine_restore_purge() {
    let (_temp, hub_dir, orphan_id, live_id) = build_store().await;
    let manifest = scan_store(&hub_dir).await;
    assert_eq!(manifest.candidates.len(), 1);
    assert_eq!(manifest.candidates[0].doc_id, orphan_id);

    // ── Dry-run changes NOTHING on disk ─────────────────────────────
    let before = tree_fingerprint(&hub_dir);
    let dry = collect(&hub_dir, &manifest, false).await.unwrap();
    assert_eq!(dry.verified.len(), 1);
    assert!(dry.batch_dir.is_none());
    assert_eq!(
        tree_fingerprint(&hub_dir),
        before,
        "dry-run must not touch the data dir"
    );

    // ── Execute: quarantine, never unlink ───────────────────────────
    let outcome = collect(&hub_dir, &manifest, true).await.unwrap();
    let batch_dir = outcome.batch_dir.expect("batch created");
    // collect canonicalizes the data dir (macOS: /var → /private/var),
    // so compare against the canonical form.
    assert!(batch_dir.starts_with(hub_dir.canonicalize().unwrap().join("trash")));
    // Orphan's chunks moved (not copied, not deleted).
    let orphan_quarantined = batch_dir.join("docs").join(&orphan_id);
    assert!(orphan_quarantined.is_dir());
    assert!(
        !list_doc_ids_filesystem(&hub_dir.join("automerge"))
            .unwrap()
            .contains(&orphan_id)
    );
    // batch.json embeds the manifest + chunk hashes.
    let record: quarto_hub::admin::collect::BatchRecord =
        serde_json::from_slice(&std::fs::read(batch_dir.join("batch.json")).unwrap()).unwrap();
    assert_eq!(record.manifest.candidates[0].doc_id, orphan_id);
    assert!(!record.docs[0].chunks.is_empty());

    // The project still opens and serves everything live.
    {
        let storage = StorageManager::new(_temp.path().join("project")).unwrap();
        let ctx = HubContext::new(storage, HubConfig::default())
            .await
            .unwrap();
        let cap = ctx.index().get_capture("a.qmd").unwrap();
        assert_eq!(cap.capture_doc_id, live_id);
        let handle = ctx
            .repo()
            .find(samod::DocumentId::from_str(&live_id).unwrap())
            .await
            .unwrap();
        assert!(handle.is_some(), "live capture must still load");
        ctx.repo().stop().await;
    }

    // ── Restore: hash-verified, byte-identical ──────────────────────
    let results = restore(&hub_dir, &batch_dir, &[]).unwrap();
    assert_eq!(results.len(), 1);
    assert!(results[0].1.is_ok(), "restore failed: {:?}", results[0].1);
    assert!(
        list_doc_ids_filesystem(&hub_dir.join("automerge"))
            .unwrap()
            .contains(&orphan_id),
        "orphan chunks back in place"
    );
    // Restored doc loads as a valid capture again.
    let m2 = scan_store(&hub_dir).await;
    assert_eq!(
        m2.candidates.len(),
        1,
        "restored orphan is a candidate again"
    );

    // ── Purge: retention-gated, batch-level ─────────────────────────
    // Re-collect so there is a batch to purge.
    let outcome2 = collect(&hub_dir, &m2, true).await.unwrap();
    let batch2 = outcome2.batch_dir.unwrap();
    // Young batch: listed, not eligible, survives --execute.
    let purged = purge(&hub_dir, 30, true).unwrap();
    assert!(purged.iter().all(|p| !p.eligible));
    assert!(batch2.exists());
    // Age the batch by rewriting its createdAt, then purge for real.
    let mut record2: quarto_hub::admin::collect::BatchRecord =
        serde_json::from_slice(&std::fs::read(batch2.join("batch.json")).unwrap()).unwrap();
    record2.created_at = "2020-01-01T00:00:00+00:00".to_string();
    std::fs::write(
        batch2.join("batch.json"),
        serde_json::to_vec(&record2).unwrap(),
    )
    .unwrap();
    let purged2 = purge(&hub_dir, 30, true).unwrap();
    assert!(purged2.iter().any(|p| p.eligible));
    assert!(!batch2.exists(), "aged batch purged");
}

#[tokio::test(flavor = "multi_thread")]
async fn collect_reverification_skips_rereferenced_candidate() {
    let (_temp, hub_dir, orphan_id, _live_id) = build_store().await;
    let manifest = scan_store(&hub_dir).await;
    // Assert the count before indexing: when this test flaked under
    // bd-eb2wnxkp, a mis-identified LIVE doc joined the candidate list
    // and candidates[0] was a completely different id — confusing the
    // failure signature.
    assert_eq!(manifest.candidates.len(), 1);
    assert_eq!(manifest.candidates[0].doc_id, orphan_id);

    // Between scan and collect, the orphan becomes referenced again
    // (e.g. a client synced back an older index state).
    {
        let storage = StorageManager::new(_temp.path().join("project")).unwrap();
        let ctx = HubContext::new(storage, HubConfig::default())
            .await
            .unwrap();
        ctx.index()
            .set_capture(
                "a.qmd",
                &CaptureRef {
                    capture_doc_id: orphan_id.clone(),
                    staleness: Some(false),
                    state: None,
                    last_error: None,
                },
            )
            .unwrap();
        ctx.repo().stop().await;
    }

    let outcome = collect(&hub_dir, &manifest, true).await.unwrap();
    assert!(outcome.verified.is_empty());
    assert!(outcome.batch_dir.is_none());
    assert_eq!(outcome.skipped.len(), 1);
    assert!(
        outcome.skipped[0].1.contains("referenced"),
        "skip reason: {}",
        outcome.skipped[0].1
    );
    // Nothing moved.
    assert!(
        list_doc_ids_filesystem(&hub_dir.join("automerge"))
            .unwrap()
            .contains(&orphan_id)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn collect_refuses_wrong_data_dir_and_held_lock() {
    let (_temp, hub_dir, _orphan_id, _live_id) = build_store().await;
    let manifest = scan_store(&hub_dir).await;

    // Wrong data dir (a manifest from server A pointed at server B).
    let other = TempDir::new().unwrap();
    std::fs::create_dir_all(other.path().join("automerge")).unwrap();
    let err = collect(other.path(), &manifest, true).await.unwrap_err();
    assert!(err.contains("refusing"), "got: {err}");

    // Held lock (a live server): StorageManager holds the exclusive
    // flock for its lifetime — collect must refuse.
    let _held = StorageManager::new(_temp.path().join("project")).unwrap();
    let err = collect(&hub_dir, &manifest, true).await.unwrap_err();
    assert!(err.contains("locked"), "got: {err}");
}

use std::str::FromStr as _;
