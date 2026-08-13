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

// ─── Phase 2 (bd-ee2fqm95): the `assets` manifest handshake ─────────────────
//
// Plan `claude-notes/plans/2026-08-13-live-share-local-spa-assets.md`,
// design decision 5: `GET /api/preview/config` gains
// `assets: { "viewer": "<sha256…>", "editor": "<sha256…>" }` — the
// top-level hashes of the embedded bundles' manifests. Fields are
// omitted when the corresponding embed has no manifest (fresh-clone
// placeholder), and the whole block is omitted under `SPA_DIR_OVERRIDE`
// (disk-served bytes are not described by the embedded manifest). The
// `--join` preflight compares the hash for the session's UI against its
// own embedded manifest to pick local serving vs. full tunnel.

/// The viewer manifest on disk, if this tree has one (fresh clones and
/// placeholder embeds do not). The test binary embeds whatever
/// `build.rs` saw at compile time, and `build.rs` watches the dist
/// tree, so the on-disk manifest and the embedded one move together.
fn viewer_manifest_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../q2-preview-spa/dist/spa-manifest.json")
}

fn is_sha256_hex(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn config_reports_embedded_asset_manifest_hashes() {
    let (port, _ctx, handle, _project, _data) = boot_server_for_test(false).await;

    // Is this process's embedded viewer dist real or the fresh-clone
    // placeholder? Ask the server: the placeholder page names itself.
    let index = reqwest::get(format!("http://127.0.0.1:{port}/"))
        .await
        .expect("GET /")
        .text()
        .await
        .expect("index body");
    let placeholder = index.contains("SPA is not built");

    let body = fetch_config(port).await;

    if placeholder {
        // No manifest exists for a placeholder embed, so no hash can
        // be advertised — the guest tunnels (self-healing fallback).
        assert!(
            body.get("assets").and_then(|a| a.get("viewer")).is_none(),
            "placeholder viewer embed must not advertise a manifest hash; body was {body:?}"
        );
    } else {
        // Real embed: the config must advertise the viewer manifest's
        // top-level hash — the exact value the guest compares against.
        let viewer = body
            .get("assets")
            .and_then(|a| a.get("viewer"))
            .and_then(|v| v.as_str())
            .expect("a real embedded viewer dist must advertise assets.viewer");
        assert!(
            is_sha256_hex(viewer),
            "assets.viewer must be a lowercase sha256 hex string; got {viewer:?}"
        );
        // Strong correspondence once the manifest exists on disk (the
        // test binary embeds what build.rs saw, and build.rs watches
        // the dist tree, so they move together). Field name `hash` per
        // the plan's "top-level hash"; adjust if Phase 2 pins another.
        let manifest = viewer_manifest_path();
        if manifest.is_file() {
            let manifest_json: serde_json::Value = serde_json::from_str(
                &std::fs::read_to_string(&manifest).expect("read viewer manifest"),
            )
            .expect("viewer manifest is JSON");
            let expected = manifest_json
                .get("hash")
                .and_then(|v| v.as_str())
                .expect("viewer manifest carries a top-level hash");
            assert_eq!(
                viewer, expected,
                "assets.viewer must equal the embedded viewer manifest's top-level hash"
            );
        }
        if let Some(editor) = body
            .get("assets")
            .and_then(|a| a.get("editor"))
            .and_then(|v| v.as_str())
        {
            // Presence depends on the editor embed's build state (it is
            // opt-in), but when present it must be a real hash.
            assert!(
                is_sha256_hex(editor),
                "assets.editor must be a lowercase sha256 hex string; got {editor:?}"
            );
        }
    }

    handle.abort();
    let _ = handle.await;
}

/// Design decision 5's carve-out: under `SPA_DIR_OVERRIDE` the served
/// bytes come from disk and are *not* described by the embedded
/// manifest, so the config must omit the whole `assets` block (guests
/// then tunnel everything). Green today (the block does not exist
/// yet); stays green as Phase 2's guard for the override path.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn config_omits_assets_under_spa_dir_override() {
    let project = tempfile::TempDir::with_prefix("q2-preview-config-override-").unwrap();
    std::fs::write(project.path().join("index.qmd"), INITIAL_QMD).unwrap();
    let data = tempfile::TempDir::with_prefix("q2-preview-config-override-data-").unwrap();
    let spa = tempfile::TempDir::with_prefix("q2-preview-config-override-spa-").unwrap();
    std::fs::write(
        spa.path().join("index.html"),
        "<!doctype html><div id=\"root\">override</div>",
    )
    .unwrap();

    let port = pick_free_port();
    let config = PreviewConfig {
        host: "127.0.0.1".to_string(),
        port,
        project_root: Some(project.path().to_path_buf()),
        single_file: None,
        data_dir: data.path().to_path_buf(),
        spa_dir_override: Some(spa.path().to_path_buf()),
        engine_registry: None,
        engine_policy: Default::default(),
        resource_html_files: Vec::new(),
        cache_dir: None,
        allow_edit: false,
        share: false,
        ui: Default::default(),
    };
    let handle = tokio::spawn(async move {
        let _ = quarto_preview::run(config).await;
    });

    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    let body = loop {
        match reqwest::get(format!("http://127.0.0.1:{port}/api/preview/config")).await {
            Ok(resp) if resp.status().is_success() => {
                break resp.json::<serde_json::Value>().await.expect("config json");
            }
            _ if std::time::Instant::now() < deadline => {
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            other => panic!("server didn't come up on port {port}: {other:?}"),
        }
    };

    assert!(
        body.get("assets").is_none(),
        "SPA_DIR_OVERRIDE sessions serve from disk; the embedded manifest says \
         nothing about those bytes, so `assets` must be omitted; body was {body:?}"
    );

    handle.abort();
    let _ = handle.await;
}
