//! Integration tests for `GET /api/preview/config` and the
//! `allow_edit` → `DiskWritePolicy` wiring (bd-ov4gqk3m).
//!
//! Each test boots a real preview server against a fixture project and
//! verifies two things end to end:
//!
//! 1. **Wire shape** — the SPA bootstraps its edit surface from
//!    `GET /api/preview/config`, so the endpoint must report
//!    `{ "allowEdit": <bool> }` matching the [`PreviewConfig`] field.
//! 2. **Disk behavior** — a document-side edit (standing in for a
//!    browser edit arriving over `/ws`) followed by a server sync must
//!    reach the file on disk **only** when `allow_edit` is set. This
//!    exercises the `build_hub_config` → `HubConfig::disk_write_policy`
//!    plumbing for real, not just the JSON.

use std::net::TcpListener as StdTcpListener;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use automerge::{ROOT, ReadDoc, transaction::Transactable};
use quarto_hub::HubContext;
use quarto_preview::{PreviewConfig, run_with_on_ready};
use tokio::sync::oneshot;

const INITIAL_QMD: &str = "---\ntitle: Config endpoint test\n---\n\nHello.\n";

fn pick_free_port() -> u16 {
    let listener = StdTcpListener::bind("127.0.0.1:0").expect("probe bind");
    let port = listener.local_addr().expect("local_addr").port();
    drop(listener);
    port
}

async fn boot_server_for_test(
    allow_edit: bool,
) -> (
    u16,
    Arc<HubContext>,
    tokio::task::JoinHandle<anyhow::Result<()>>,
    tempfile::TempDir,
    tempfile::TempDir,
) {
    let project = tempfile::TempDir::with_prefix("q2-preview-config-endpoint-").unwrap();
    std::fs::write(project.path().join("index.qmd"), INITIAL_QMD).unwrap();
    let data = tempfile::TempDir::with_prefix("q2-preview-config-endpoint-data-").unwrap();

    let port = pick_free_port();
    let config = PreviewConfig {
        host: "127.0.0.1".to_string(),
        port,
        project_root: Some(project.path().to_path_buf()),
        single_file: None,
        data_dir: data.path().to_path_buf(),
        spa_dir_override: None,
        engine_registry: None,
        engine_policy: Default::default(),
        resource_html_files: Vec::new(),
        cache_dir: None,
        allow_edit,
        share: false,
        ui: Default::default(),
    };

    let (ready_tx, ready_rx) = oneshot::channel::<Arc<HubContext>>();
    let mut ready_tx = Some(ready_tx);
    let handle = tokio::spawn(async move {
        run_with_on_ready(config, move |ctx| {
            if let Some(tx) = ready_tx.take() {
                let _ = tx.send(ctx);
            }
        })
        .await
    });

    let ctx = tokio::time::timeout(Duration::from_secs(10), ready_rx)
        .await
        .expect("server reached on_ready within 10s")
        .expect("on_ready callback fired");
    (port, ctx, handle, project, data)
}

/// Fetch `/api/preview/config` and return the parsed JSON body.
async fn fetch_config(port: u16) -> serde_json::Value {
    let url = format!("http://127.0.0.1:{port}/api/preview/config");
    let resp = reqwest::get(&url).await.expect("GET succeeds");
    assert_eq!(resp.status(), 200);
    resp.json().await.expect("response is JSON")
}

/// Mutate the automerge text document for `rel` in place, standing in
/// for a browser edit that arrived through the `/ws` samod sync.
async fn edit_doc_text(ctx: &HubContext, rel: &str, new_text: &str) {
    let doc_id_str = ctx.index().get_file(rel).expect("file present in index");
    let doc_id = samod::DocumentId::from_str(&doc_id_str).expect("valid doc id");
    let handle = ctx
        .repo()
        .find(doc_id)
        .await
        .expect("repo running")
        .expect("document found");
    handle.with_document(|doc| {
        let (_, text_obj) = doc
            .get(ROOT, "text")
            .expect("doc readable")
            .expect("doc has text field");
        doc.transact::<_, _, automerge::AutomergeError>(|tx| {
            tx.update_text(&text_obj, new_text)?;
            Ok(())
        })
        .expect("transaction applies");
    });
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn config_reports_read_only_and_doc_edits_never_reach_disk() {
    let (port, ctx, handle, project, _data) = boot_server_for_test(false).await;

    // Wire shape: the SPA must learn that editing is disabled.
    let body = fetch_config(port).await;
    assert_eq!(
        body.get("allowEdit").and_then(|v| v.as_bool()),
        Some(false),
        "without --allow-edit the endpoint must report allowEdit: false; body was {body:?}"
    );
    assert!(
        body.get("editorBoot").is_none(),
        "editorBoot must be absent when nothing was stashed (bd-7htq16rx); body was {body:?}"
    );

    // Disk behavior: a document-side edit + explicit sync must leave
    // the file untouched (DiskWritePolicy::ReadOnly).
    let edited = INITIAL_QMD.replace("Hello.", "Hello, edited in the browser.");
    edit_doc_text(&ctx, "index.qmd", &edited).await;
    let abs = project.path().join("index.qmd");
    ctx.sync_file(&abs).await.expect("sync_file succeeds");

    assert_eq!(
        std::fs::read_to_string(&abs).unwrap(),
        INITIAL_QMD,
        "read-only preview must never write document changes to disk"
    );

    handle.abort();
    let _ = handle.await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn config_reports_allow_edit_and_doc_edits_persist_to_disk() {
    let (port, ctx, handle, project, _data) = boot_server_for_test(true).await;

    let body = fetch_config(port).await;
    assert_eq!(
        body.get("allowEdit").and_then(|v| v.as_bool()),
        Some(true),
        "with --allow-edit the endpoint must report allowEdit: true; body was {body:?}"
    );
    assert!(
        body.get("editorBoot").is_none(),
        "editorBoot must be absent when nothing was stashed (bd-7htq16rx); body was {body:?}"
    );

    let edited = INITIAL_QMD.replace("Hello.", "Hello, edited in the browser.");
    edit_doc_text(&ctx, "index.qmd", &edited).await;
    let abs = project.path().join("index.qmd");
    ctx.sync_file(&abs).await.expect("sync_file succeeds");

    assert_eq!(
        std::fs::read_to_string(&abs).unwrap(),
        edited,
        "with --allow-edit a synced document change must reach the file on disk"
    );

    handle.abort();
    let _ = handle.await;
}

/// bd-7htq16rx: an editor-UI host stashes its share-route boot params
/// (mirroring what the CLI's editor-mode `on_ready` does) and
/// `GET /api/preview/config` then carries them as `editorBoot`, so a
/// `--join` guest can build the same share URL (with `ephemeral=true`)
/// against its local proxy and land straight in the document.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn config_reports_editor_boot_when_stashed() {
    let project = tempfile::TempDir::with_prefix("q2-preview-config-editor-boot-").unwrap();
    std::fs::write(project.path().join("index.qmd"), INITIAL_QMD).unwrap();
    let data = tempfile::TempDir::with_prefix("q2-preview-config-editor-boot-data-").unwrap();

    let port = pick_free_port();
    let config = PreviewConfig {
        host: "127.0.0.1".to_string(),
        port,
        project_root: Some(project.path().to_path_buf()),
        single_file: None,
        data_dir: data.path().to_path_buf(),
        spa_dir_override: None,
        engine_registry: None,
        engine_policy: Default::default(),
        resource_html_files: Vec::new(),
        cache_dir: None,
        allow_edit: false,
        share: false,
        ui: quarto_preview::PreviewUi::Editor,
    };

    let (ready_tx, ready_rx) = oneshot::channel::<String>();
    let mut ready_tx = Some(ready_tx);
    let handle = tokio::spawn(async move {
        run_with_on_ready(config, move |ctx| {
            // Mirror the CLI's editor-mode on_ready: stash the params
            // the host's own share-route boot URL is built from.
            let doc_id = ctx.index().document_id();
            quarto_preview::set_editor_boot(quarto_preview::EditorBootInfo {
                index_doc_id: doc_id.clone(),
                file: "index.qmd".to_string(),
                name: "fixture-project".to_string(),
            });
            if let Some(tx) = ready_tx.take() {
                let _ = tx.send(doc_id);
            }
        })
        .await
    });

    let doc_id = tokio::time::timeout(Duration::from_secs(10), ready_rx)
        .await
        .expect("server reached on_ready within 10s")
        .expect("on_ready callback fired");

    let body = fetch_config(port).await;
    assert_eq!(
        body.get("allowEdit").and_then(|v| v.as_bool()),
        Some(false),
        "allowEdit keeps reporting independently of editorBoot; body was {body:?}"
    );
    let boot = body
        .get("editorBoot")
        .expect("editorBoot must be present once stashed");
    assert_eq!(
        boot.get("indexDocId").and_then(|v| v.as_str()),
        Some(doc_id.as_str()),
        "editorBoot names the hub's index document"
    );
    assert_eq!(boot.get("file").and_then(|v| v.as_str()), Some("index.qmd"));
    assert_eq!(
        boot.get("name").and_then(|v| v.as_str()),
        Some("fixture-project")
    );

    handle.abort();
    let _ = handle.await;
}
