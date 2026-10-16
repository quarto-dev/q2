//! F8: the live viewer's render flags. The `render` block of
//! `GET /api/preview/config`, and the runtime factory every native site of
//! the server goes through (`--profile`).

use std::net::TcpListener as StdTcpListener;
use std::time::Duration;

use quarto_core::ProjectContext;
use quarto_core::format_resolution::resolve_project_format;
use quarto_core::format_resolution::resolve_render_scope;
use quarto_core::project::orchestrator::RenderMode;
use quarto_preview::config::read_engine_policy_from_project;
use quarto_preview::render_flags::native_runtime_for;
use quarto_preview::{EnginePolicy, PreviewConfig, RenderFlags, run_with_on_ready};
use quarto_system_runtime::{NativeRuntime, SystemRuntime};
use tokio::sync::oneshot;

fn pick_free_port() -> u16 {
    let listener = StdTcpListener::bind("127.0.0.1:0").expect("probe bind");
    let port = listener.local_addr().expect("local_addr").port();
    drop(listener);
    port
}

/// Boot a real server with `render` and return the parsed config body.
async fn config_body(render: RenderFlags) -> serde_json::Value {
    let project = tempfile::TempDir::with_prefix("q2-preview-render-flags-").unwrap();
    std::fs::write(project.path().join("index.qmd"), "# Hi\n").unwrap();
    let data = tempfile::TempDir::with_prefix("q2-preview-render-flags-data-").unwrap();
    let port = pick_free_port();
    let config = PreviewConfig {
        render,
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
        ui: Default::default(),
    };
    let (ready_tx, ready_rx) = oneshot::channel();
    let mut ready_tx = Some(ready_tx);
    let handle = tokio::spawn(async move {
        run_with_on_ready(config, move |_ctx| {
            if let Some(tx) = ready_tx.take() {
                let _ = tx.send(());
            }
        })
        .await
    });
    tokio::time::timeout(Duration::from_secs(10), ready_rx)
        .await
        .expect("server reached on_ready within 10s")
        .expect("on_ready fired");
    let url = format!("http://127.0.0.1:{port}/api/preview/config");
    let resp = reqwest::get(&url).await.expect("GET succeeds");
    assert_eq!(resp.status(), 200);
    let body = resp.json().await.expect("JSON body");
    handle.abort();
    body
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn config_carries_the_render_block_when_flags_are_given() {
    let body = config_body(RenderFlags {
        to: Some("typst".to_string()),
        metadata: Some(serde_json::json!({ "toc": true, "format": { "typst": { "toc": true } } })),
        // `--profile` stays on the server: the browser cannot honor it.
        profile: vec!["print".to_string()],
    })
    .await;
    assert_eq!(
        body["render"],
        serde_json::json!({
            "to": "typst",
            "metadata": { "toc": true, "format": { "typst": { "toc": true } } }
        })
    );
    assert!(body.get("profile").is_none() && body["render"].get("profile").is_none());
    assert_eq!(
        body["allowEdit"], false,
        "the existing fields are unchanged"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn config_omits_the_render_block_without_flags() {
    let body = config_body(RenderFlags::default()).await;
    assert!(body.get("render").is_none(), "{body}");
    // `--profile` alone changes nothing the browser reads.
    // (A separate process per test: the session state is first-writer-wins.)
}

fn project_with_profiles() -> tempfile::TempDir {
    let root = tempfile::TempDir::with_prefix("q2-preview-factory-").unwrap();
    std::fs::write(
        root.path().join("_quarto.yml"),
        "project:\n  type: website\nformat: html\n",
    )
    .unwrap();
    std::fs::write(
        root.path().join("_quarto-print.yml"),
        "format: typst\npreview:\n  engine: off\n",
    )
    .unwrap();
    std::fs::write(root.path().join("index.qmd"), "# Hi\n").unwrap();
    root
}

fn project_format(root: &std::path::Path, runtime: &dyn SystemRuntime) -> String {
    let project = ProjectContext::discover(root, runtime).unwrap();
    let request = resolve_render_scope(&project, &RenderMode::Full, runtime);
    resolve_project_format(&project, &request, None, runtime).key
}

/// A profile that supplies `format: typst` is seen as typst through the
/// factory's runtime by plain `discover`, and on no other runtime; the
/// config reader honors a `preview.engine` the profile supplies.
#[test]
fn the_factory_runtime_carries_the_profile_to_discovery() {
    let root = project_with_profiles();
    let print = native_runtime_for(&["print".to_string()]);
    assert_eq!(project_format(root.path(), &print), "typst");
    assert_eq!(project_format(root.path(), &NativeRuntime::new()), "html");
    assert_eq!(
        project_format(root.path(), &native_runtime_for(&[])),
        "html",
        "an empty selection is a plain runtime"
    );
    assert_eq!(
        read_engine_policy_from_project(root.path(), &print, None),
        EnginePolicy::Off
    );
    assert_eq!(
        read_engine_policy_from_project(root.path(), &NativeRuntime::new(), None),
        EnginePolicy::Manual
    );
}

/// Two sessions' runtimes with different selections in one process do
/// not see each other's, and nothing reaches the process environment.
#[test]
fn two_selections_in_one_process_are_independent() {
    let root = project_with_profiles();
    std::fs::write(root.path().join("_quarto-slides.yml"), "format: revealjs\n").unwrap();
    let print = native_runtime_for(&["print".to_string()]);
    let slides = native_runtime_for(&["slides".to_string()]);
    assert_eq!(project_format(root.path(), &print), "typst");
    assert_eq!(project_format(root.path(), &slides), "revealjs");
    assert_eq!(project_format(root.path(), &print), "typst");
    assert!(std::env::var("QUARTO_PROFILE").is_err());
}
