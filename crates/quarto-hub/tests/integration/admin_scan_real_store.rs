//! `hub admin scan` against a REAL samod filesystem store
//! (bd-eiku4ymo) — not fabricated chunks: docs are created through a
//! live `HubContext` repo (the same path the server uses), flushed to
//! disk via `Repo::stop()`, and then scanned offline through
//! `TokioFilesystemStorage`, exactly as the CLI will.

use quarto_hub::admin::scan::{ScanOptions, list_doc_ids_filesystem, scan};
use quarto_hub::context::{HubConfig, HubContext};
use quarto_hub::index::CaptureRef;
use quarto_hub::resource::{CaptureDocMeta, create_capture_document_at};
use quarto_hub::storage::StorageManager;
use samod::storage::TokioFilesystemStorage;
use tempfile::TempDir;

/// An old-stamped capture doc (safely past any age gate).
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

#[tokio::test(flavor = "multi_thread")]
async fn scan_real_store_finds_orphaned_capture_only() {
    let temp = TempDir::new().unwrap();
    let project_root = temp.path().join("project");
    std::fs::create_dir(&project_root).unwrap();
    std::fs::write(project_root.join("a.qmd"), "# Hello\n").unwrap();

    let storage = StorageManager::new(&project_root).unwrap();
    let automerge_dir = storage.automerge_dir();
    let ctx = HubContext::new(storage, HubConfig::default())
        .await
        .unwrap();

    // A "current" capture, referenced from the index sidecar → live.
    let live = ctx.repo().create(old_capture_doc()).await.unwrap();
    ctx.index()
        .set_capture(
            "a.qmd",
            &CaptureRef {
                capture_doc_id: live.document_id().to_string(),
                staleness: Some(false),
                state: None,
                last_error: None,
            },
        )
        .unwrap();

    // A superseded capture nobody references → the orphan. This is
    // exactly what perform_re_execute leaves behind when it repoints
    // the sidecar.
    let orphan = ctx.repo().create(old_capture_doc()).await.unwrap();
    let orphan_id = orphan.document_id().to_string();
    drop(orphan);
    drop(live);

    // Flush the repo to disk and let go of the store.
    ctx.repo().stop().await;

    // Scan offline through the same adapter the server uses.
    let fs_storage = TokioFilesystemStorage::new(&automerge_dir);
    let doc_ids = list_doc_ids_filesystem(&automerge_dir);
    assert!(
        doc_ids.len() >= 4,
        "expected at least index + file + 2 captures; got {doc_ids:?}"
    );
    let manifest = scan(
        &fs_storage,
        &doc_ids,
        &automerge_dir.to_string_lossy(),
        &ScanOptions::default(),
    )
    .await;

    let ids: Vec<&str> = manifest
        .candidates
        .iter()
        .map(|c| c.doc_id.as_str())
        .collect();
    assert_eq!(
        ids,
        vec![orphan_id.as_str()],
        "exactly the superseded capture is removable; manifest: {manifest:#?}"
    );
    // Evidence carried from the Phase A meta envelope.
    let meta = manifest.candidates[0].meta.as_ref().unwrap();
    assert_eq!(meta.source_path.as_deref(), Some("a.qmd"));
    assert_eq!(meta.engines, vec!["knitr"]);

    // The index, the file doc, and the live capture are all
    // inventoried and protected.
    assert_eq!(manifest.inventory["project-index"].count, 1);
    assert_eq!(manifest.inventory["engine-capture"].count, 2);
    assert!(
        manifest.inventory.contains_key("text-file"),
        "the synced a.qmd file doc should be inventoried; got {:?}",
        manifest.inventory
    );
}
