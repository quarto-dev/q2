//! Phase 3 money test (live-share plan, bd-6y0p1bne): a real preview hub
//! in-process on a fixture project, a hermetic tunnel in front of it, and
//! a samod client dialing `/ws` through the guest port — proves
//! automerge-sync-over-tunnel without a browser.
//!
//! Hermetic: `EndpointPreset::HermeticLoopback` on both tunnel ends — no
//! n0 relays, pkarr, or DNS in CI. `repo.dial_websocket` works here
//! because preview's `/ws` takes no credentials (`auth_config: None`
//! skips credential and Origin checks; see the plan's Phase 3 notes).

use std::net::TcpListener as StdTcpListener;
use std::time::{Duration, Instant};

use quarto_hub::index::IndexDocument;
use quarto_p2p::{EndpointPreset, TunnelClient, TunnelClientConfig, TunnelHost, TunnelHostConfig};
use quarto_preview::PreviewConfig;
use samod::{BackoffConfig, Repo};

/// Bind `127.0.0.1:0`, capture the assigned port, release the listener.
/// Same tiny-race trade-off as the CLI's own port probe.
fn pick_free_port() -> u16 {
    let listener = StdTcpListener::bind("127.0.0.1:0").expect("probe bind");
    let port = listener.local_addr().expect("local_addr").port();
    drop(listener);
    port
}

/// Poll `GET /health` (directly, not through the tunnel) until the hub
/// is up — HubContext::new can take a beat (samod init, initial fs sync).
async fn wait_for_health(port: u16) {
    let url = format!("http://127.0.0.1:{port}/health");
    let deadline = Instant::now() + Duration::from_secs(20);
    let client = reqwest::Client::new();
    loop {
        if let Ok(resp) = client.get(&url).send().await
            && resp.status().is_success()
        {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "preview server didn't come up on port {port} within 20s"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn guest_syncs_project_through_tunnel() {
    // Fixture project: `_quarto.yml` + two pages, like the e2e fixtures.
    let project = tempfile::TempDir::with_prefix("q2-join-tunnel-proj-").unwrap();
    std::fs::write(
        project.path().join("_quarto.yml"),
        "project:\n  type: website\n",
    )
    .unwrap();
    std::fs::write(project.path().join("index.qmd"), "# Index\n\nHello.\n").unwrap();
    std::fs::write(project.path().join("about.qmd"), "# About\n").unwrap();
    let data = tempfile::TempDir::with_prefix("q2-join-tunnel-data-").unwrap();

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
    };

    // The real hub, exactly as `q2 preview` runs it. `run()` blocks until
    // shutdown; the task dies with the test process (nextest isolates).
    let _server = tokio::spawn(async move {
        let _ = quarto_preview::run(config).await;
    });
    wait_for_health(port).await;

    // Hermetic tunnel pair in front of the hub — the same shape
    // `--share`/`--join` wire up with the production n0 preset.
    let (ticket, tunnel_host) = TunnelHost::spawn(
        TunnelHostConfig {
            preset: EndpointPreset::HermeticLoopback,
            ..Default::default()
        },
        ([127, 0, 0, 1], port).into(),
    )
    .await
    .expect("spawn tunnel host");
    let (guest, tunnel_client) = TunnelClient::bind(
        TunnelClientConfig {
            preset: EndpointPreset::HermeticLoopback,
        },
        ticket,
        "127.0.0.1:0".parse().unwrap(),
    )
    .await
    .expect("bind tunnel client");

    // (a) `/health` through the guest port answers with the *host's*
    // index document id — byte-identical to the direct answer.
    let through_tunnel: serde_json::Value = reqwest::get(format!("http://{guest}/health"))
        .await
        .expect("GET /health through tunnel")
        .error_for_status()
        .expect("200 through tunnel")
        .json()
        .await
        .expect("health json");
    let doc_id = through_tunnel["index_document_id"]
        .as_str()
        .expect("health carries index_document_id")
        .to_string();
    assert!(!doc_id.is_empty(), "index_document_id must be non-empty");

    let direct: serde_json::Value = reqwest::get(format!("http://127.0.0.1:{port}/health"))
        .await
        .expect("GET /health direct")
        .json()
        .await
        .expect("direct health json");
    assert_eq!(
        doc_id,
        direct["index_document_id"].as_str().unwrap(),
        "tunnel and direct /health must name the same index document"
    );

    // (b) samod client dials `/ws` through the guest port and loads the
    // index document: automerge sync itself flows over the tunnel.
    let repo = Repo::build_tokio().load().await;
    let ws_url: url::Url = format!("ws://{guest}/ws").parse().unwrap();
    let handle = repo
        .dial_websocket(ws_url, BackoffConfig::default())
        .expect("dial /ws through tunnel");
    tokio::time::timeout(Duration::from_secs(10), handle.established())
        .await
        .expect("ws connection through tunnel timed out")
        .expect("ws connection through tunnel failed");

    let index = IndexDocument::load(&repo, &doc_id)
        .await
        .expect("load index document")
        .expect("index document found through tunnel");

    // The files map converges to the fixture's two pages.
    let deadline = Instant::now() + Duration::from_secs(15);
    let files = loop {
        let files = index.get_all_files();
        if files.contains_key("index.qmd") && files.contains_key("about.qmd") {
            break files;
        }
        assert!(
            Instant::now() < deadline,
            "fixture files never synced through the tunnel; got: {:?}",
            files.keys().collect::<Vec<_>>()
        );
        tokio::time::sleep(Duration::from_millis(250)).await;
    };
    // Every file entry maps to a per-file automerge doc id.
    for (path, file_doc_id) in &files {
        assert!(
            !file_doc_id.is_empty(),
            "file {path} has an empty document id"
        );
    }

    tunnel_client.shutdown().await.expect("client shutdown");
    tunnel_host.shutdown().await.expect("host shutdown");
}
