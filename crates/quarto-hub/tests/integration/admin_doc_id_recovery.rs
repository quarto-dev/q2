//! Doc-id recovery from case-folded splay directories (bd-eb2wnxkp,
//! bd-u0tldu4z).
//!
//! samod splays a doc id across `<first-2-chars>/<rest>/`. Base58 is
//! case-sensitive but APFS/NTFS are not: when two ids' 2-char prefixes
//! differ only by case, both land in one on-disk directory and every id
//! read back through it inherits the first-creator's casing.
//! `list_doc_ids_filesystem` must recover the true id by testing the
//! ≤4 case variants of the prefix against bs58check's checksum.
//!
//! The hand-built-directory tests are deterministic on every platform
//! (no random id draw — the flake they replace fired ~2%/run). The
//! collect-level test needs a real case-insensitive filesystem and
//! skips honestly elsewhere.
//!
//! Fixture ids are real bs58check strings (checksum-verified): the
//! case-flipped variants used here fail to parse, which is exactly the
//! property recovery relies on.

use std::path::Path;

use quarto_hub::admin::collect::collect;
use quarto_hub::admin::scan::{ScanOptions, list_doc_ids_filesystem, scan};
use quarto_hub::context::{HubConfig, HubContext};
use quarto_hub::index::CaptureRef;
use quarto_hub::resource::{CaptureDocMeta, create_capture_document_at};
use quarto_hub::storage::StorageManager;
use samod::storage::TokioFilesystemStorage;
use tempfile::TempDir;

/// True id captured from a real store (see
/// `claude-notes/plans/flaky-admin-collect-lifecycle-investigation/`).
/// `2CPAD…` (the case-flip) fails bs58check.
const TRUE_ID: &str = "2cPADPZ85aBLaaLaLrS2BNcVza1n";

/// A valid id whose prefix case-folds into samod's always-present
/// `st/` splay dir (`st/orage-adapter-id` adapter identity).
const ST_ID: &str = "StntkRJtG7hVPkKPY4Qkeu6f5bZ";

/// Build `<automerge>/<l1>/<l2>/` with one chunk-like file inside.
fn add_doc_dir(automerge_dir: &Path, l1: &str, l2: &str) {
    let dir = automerge_dir.join(l1).join(l2);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("snapshot"), b"x").unwrap();
}

#[test]
fn list_recovers_true_id_from_case_folded_splay_dir() {
    let temp = TempDir::new().unwrap();
    let automerge_dir = temp.path().join("automerge");
    // The doc's true id is 2cPAD…, but the on-disk level-1 dir carries
    // the folded casing "2C" (first-creator wins on APFS/NTFS).
    add_doc_dir(&automerge_dir, "2C", &TRUE_ID[2..]);
    // samod's adapter identity: a *file* at level 2, skipped via is_dir.
    let st = automerge_dir.join("st");
    std::fs::create_dir_all(&st).unwrap();
    std::fs::write(st.join("orage-adapter-id"), b"id").unwrap();

    let ids = list_doc_ids_filesystem(&automerge_dir).unwrap();
    assert_eq!(ids, vec![TRUE_ID.to_string()]);
}

#[test]
fn list_recovers_id_folded_into_adapter_st_dir() {
    // Every store has an `st/` level-1 dir (adapter identity), so a doc
    // id starting St/ST/sT is *guaranteed* a folding partner: on a
    // case-insensitive filesystem its chunks land inside `st/` and the
    // id reads back as `st…`.
    let temp = TempDir::new().unwrap();
    let automerge_dir = temp.path().join("automerge");
    let st = automerge_dir.join("st");
    std::fs::create_dir_all(&st).unwrap();
    std::fs::write(st.join("orage-adapter-id"), b"id").unwrap();
    add_doc_dir(&automerge_dir, "st", &ST_ID[2..]);

    let ids = list_doc_ids_filesystem(&automerge_dir).unwrap();
    assert_eq!(ids, vec![ST_ID.to_string()]);
}

/// Runtime probe: does `dir`'s filesystem fold case?
fn fs_is_case_insensitive(dir: &Path) -> bool {
    let probe = dir.join("CaseProbeQ2");
    std::fs::write(&probe, b"x").unwrap();
    let hit = dir.join("caseprobeq2").exists();
    std::fs::remove_file(&probe).ok();
    hit
}

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

/// The data-loss scenario end to end: a LIVE doc whose splay dir
/// carries folded casing must classify as live (not orphaned) and must
/// survive `collect --execute`.
///
/// Forces the fold deterministically by case-renaming the live doc's
/// level-1 dir — no random draw. Only meaningful where the filesystem
/// folds case; skips honestly elsewhere.
#[tokio::test(flavor = "multi_thread")]
async fn collect_does_not_quarantine_live_doc_with_folded_dir() {
    let temp = TempDir::new().unwrap();
    if !fs_is_case_insensitive(temp.path()) {
        eprintln!("SKIP: filesystem is case-sensitive; fold cannot occur here");
        return;
    }
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
    drop(ctx); // releases hub.lock, which collect() must acquire

    // Force the fold on the LIVE doc: flip the case of an alphabetic
    // char in its level-1 splay dir name. (A digits-only prefix has no
    // case to fold — vanishingly rare; skip honestly.)
    let automerge_dir = hub_dir.join("automerge");
    let prefix: String = live_id.chars().take(2).collect();
    let folded: String = {
        let mut done = false;
        prefix
            .chars()
            .map(|c| {
                if !done && c.is_ascii_alphabetic() {
                    done = true;
                    if c.is_ascii_uppercase() {
                        c.to_ascii_lowercase()
                    } else {
                        c.to_ascii_uppercase()
                    }
                } else {
                    c
                }
            })
            .collect()
    };
    if folded == prefix {
        eprintln!("SKIP: live doc id {live_id} has a caseless splay prefix");
        return;
    }
    std::fs::rename(automerge_dir.join(&prefix), automerge_dir.join(&folded)).unwrap();

    // Scan: the live doc must NOT appear as a candidate; the orphan's
    // id must come back with its true casing.
    let fs_storage = TokioFilesystemStorage::new(&automerge_dir);
    let ids = list_doc_ids_filesystem(&automerge_dir).unwrap();
    assert!(
        ids.contains(&live_id),
        "recovery must yield the live doc's true id (got {ids:?})"
    );
    let manifest = scan(
        &fs_storage,
        &ids,
        &hub_dir.canonicalize().unwrap().to_string_lossy(),
        &ScanOptions::default(),
    )
    .await;
    assert_eq!(
        manifest
            .candidates
            .iter()
            .map(|c| c.doc_id.as_str())
            .collect::<Vec<_>>(),
        vec![orphan_id.as_str()],
        "only the orphan may be a candidate; the folded live doc must stay live"
    );

    // Execute the collection; the live doc must survive and still load.
    let outcome = collect(&hub_dir, &manifest, true).await.unwrap();
    assert_eq!(outcome.verified.len(), 1);
    assert_eq!(outcome.verified[0].doc_id, orphan_id);
    {
        use std::str::FromStr;
        let storage = StorageManager::new(temp.path().join("project")).unwrap();
        let ctx = HubContext::new(storage, HubConfig::default())
            .await
            .unwrap();
        let handle = ctx
            .repo()
            .find(samod::DocumentId::from_str(&live_id).unwrap())
            .await
            .unwrap();
        assert!(
            handle.is_some(),
            "live capture must still load after collect"
        );
        ctx.repo().stop().await;
    }
}
