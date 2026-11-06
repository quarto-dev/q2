//! `hub admin` over a REAL samod store holding both capture payload kinds
//! (EXR-13): an orphaned record is collected exactly like an orphaned legacy
//! capture, a referenced record stays live, and an unrelated binary is never
//! collected.

use std::path::PathBuf;

use quarto_hub::admin::collect::collect;
use quarto_hub::admin::scan::{ScanOptions, list_doc_ids_filesystem, scan};
use quarto_hub::context::{HubConfig, HubContext};
use quarto_hub::index::CaptureRef;
use quarto_hub::resource::{
    CaptureDocMeta, create_binary_document, create_capture_document_at, create_record_document_at,
};
use quarto_hub::storage::StorageManager;
use samod::storage::TokioFilesystemStorage;
use tempfile::TempDir;

const OLD: &str = "2020-01-01T00:00:00+00:00";

fn meta() -> CaptureDocMeta {
    CaptureDocMeta {
        source_path: "a.qmd".into(),
        engines: vec!["jupyter".into()],
    }
}

struct Store {
    _temp: TempDir,
    hub_dir: PathBuf,
    orphan_record: String,
    orphan_capture: String,
    live_record: String,
    binary: String,
}

async fn build_store() -> Store {
    let temp = TempDir::new().unwrap();
    let project_root = temp.path().join("project");
    std::fs::create_dir(&project_root).unwrap();
    std::fs::write(project_root.join("a.qmd"), "# Hello\n").unwrap();
    let storage = StorageManager::new(&project_root).unwrap();
    let hub_dir = storage.hub_dir().to_path_buf();
    let ctx = HubContext::new(storage, HubConfig::default())
        .await
        .unwrap();

    let live = ctx
        .repo()
        .create(create_record_document_at(b"live", &meta(), OLD).unwrap())
        .await
        .unwrap();
    let live_record = live.document_id().to_string();
    ctx.index()
        .set_capture(
            "a.qmd",
            &CaptureRef {
                capture_doc_id: live_record.clone(),
                staleness: Some(false),
                state: None,
                last_error: None,
            },
        )
        .unwrap();
    let orphan_record = ctx
        .repo()
        .create(create_record_document_at(b"orphan-rec", &meta(), OLD).unwrap())
        .await
        .unwrap()
        .document_id()
        .to_string();
    let orphan_capture = ctx
        .repo()
        .create(create_capture_document_at(b"orphan-cap", &meta(), OLD).unwrap())
        .await
        .unwrap()
        .document_id()
        .to_string();
    let binary = ctx
        .repo()
        .create(create_binary_document(b"png", "image/png").unwrap())
        .await
        .unwrap()
        .document_id()
        .to_string();
    drop(live);
    ctx.repo().stop().await;
    Store {
        _temp: temp,
        hub_dir,
        orphan_record,
        orphan_capture,
        live_record,
        binary,
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn orphaned_records_are_collected_like_orphaned_captures() {
    let s = build_store().await;
    let automerge_dir = s.hub_dir.join("automerge");
    let storage = TokioFilesystemStorage::new(&automerge_dir);
    let ids = list_doc_ids_filesystem(&automerge_dir).unwrap();
    let manifest = scan(
        &storage,
        &ids,
        &quarto_system_runtime::canonicalize(&s.hub_dir)
            .unwrap()
            .to_string_lossy(),
        &ScanOptions::default(),
    )
    .await;

    let mut cands: Vec<&str> = manifest
        .candidates
        .iter()
        .map(|c| c.doc_id.as_str())
        .collect();
    cands.sort_unstable();
    let mut want = vec![s.orphan_record.as_str(), s.orphan_capture.as_str()];
    want.sort_unstable();
    assert_eq!(cands, want, "manifest: {manifest:#?}");
    assert!(!cands.contains(&s.live_record.as_str()));
    assert!(!cands.contains(&s.binary.as_str()));
    assert!(
        manifest
            .not_collectible
            .iter()
            .any(|r| r.doc_id == s.binary)
    );
    assert_eq!(manifest.capture_inventory["engine-record"].count, 2);
    assert_eq!(manifest.capture_inventory["engine-capture"].count, 1);

    // The collector re-verifies and quarantines both orphans.
    let outcome = collect(&s.hub_dir, &manifest, true).await.unwrap();
    assert_eq!(outcome.verified.len(), 2, "outcome: {outcome:#?}");
    assert!(outcome.skipped.is_empty(), "outcome: {outcome:#?}");
}
