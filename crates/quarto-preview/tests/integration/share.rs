//! `--share` glue tests (live-share plan Phase 2, bd-jhvkwosw).
//!
//! Hermetic: `EndpointPreset::HermeticLoopback` on both sides — no n0
//! relays, pkarr, or DNS in CI. The n0-preset production path is covered
//! by the phase's mandatory recorded end-to-end run instead.

use std::time::Duration;

use quarto_p2p::{EndpointPreset, TunnelClient, TunnelClientConfig, TunnelHostConfig};
use quarto_preview::share::{format_share_banner, spawn_share_task, start_share_session};

/// Generous cap for individual awaits so a broken tunnel fails the test
/// instead of hanging it. Sized for CI, not for the happy path: iroh
/// endpoint operations inflate 5-10x under full-suite load (every
/// endpoint bind pays for netmon setup + handshake crypto on contended
/// cores), and ubuntu runners are the slowest we have. Never binds on a
/// healthy run — the uncontended cost of a step is milliseconds.
const STEP_TIMEOUT: Duration = Duration::from_secs(60);

fn hermetic_host_cfg() -> TunnelHostConfig {
    TunnelHostConfig {
        preset: EndpointPreset::HermeticLoopback,
        ..Default::default()
    }
}

/// The core Phase 2 unit: the share glue spawns a tunnel whose target is
/// `127.0.0.1:{port}` (the pre-resolved preview port), and the join
/// banner goes through the injected callback — no stdout scraping.
///
/// multi_thread like every other test that runs an iroh endpoint
/// (tunnel.rs, join_tunnel.rs): on a current-thread runtime the endpoint
/// actors, the axum stand-in, and reqwest all share one thread, and
/// under CI load the 20s step caps below get exceeded (ubuntu CI
/// failure, 2026-08-09).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn share_glue_tunnels_to_preview_port_and_announces_join_string() {
    // Tiny axum server standing in for the preview hub on the loopback
    // port the CLI would have pre-resolved.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind stand-in preview server");
    let port = listener.local_addr().expect("local_addr").port();
    let app = axum::Router::new().route(
        "/health",
        axum::routing::get(|| async { "SHARE-GLUE-MARKER" }),
    );
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("axum serve");
    });

    let mut announced: Vec<String> = Vec::new();
    let session = tokio::time::timeout(
        STEP_TIMEOUT,
        start_share_session(hermetic_host_cfg(), "127.0.0.1", port, false, |banner| {
            announced.push(banner.to_string())
        }),
    )
    .await
    .expect("start_share_session should not hang")
    .expect("share session starts");

    // Banner delivered exactly once, via the callback.
    assert_eq!(announced.len(), 1, "banner must be announced exactly once");
    let banner = &announced[0];
    let join_string = session.ticket.to_string();
    assert!(join_string.starts_with("q2preview"));

    // The ready-to-paste line is bare — `q2 preview --join <ticket>` with
    // nothing else on it — and nothing follows it in the banner, so a
    // triple-click / drag copy survives terminal wrapping.
    let join_line = banner
        .lines()
        .find(|l| l.contains("--join"))
        .expect("banner has a join line");
    assert_eq!(join_line, format!("q2 preview --join {join_string}"));
    assert!(
        banner.trim_end().ends_with(&join_string),
        "nothing may follow the join line; got banner:\n{banner}"
    );

    // Hermetic endpoints have no relay addr → the banner must carry the
    // direct/LAN-only notice (this is how relay-unreachable surfaces to
    // users; quarto-p2p's tracing::warn is filtered out at default -v 0).
    assert!(
        !session.ticket.has_relay_addr(),
        "hermetic ticket unexpectedly carries a relay addr"
    );
    assert!(
        banner.contains("direct/LAN"),
        "no-relay banner must warn about direct/LAN-only reachability:\n{banner}"
    );

    // The ticket's tunnel target is the stand-in server: a hermetic
    // guest client can fetch through it.
    let (local, client) = tokio::time::timeout(
        STEP_TIMEOUT,
        TunnelClient::bind(
            TunnelClientConfig {
                preset: EndpointPreset::HermeticLoopback,
            },
            session.ticket.clone(),
            "127.0.0.1:0".parse().unwrap(),
        ),
    )
    .await
    .expect("client bind should not hang")
    .expect("tunnel client binds");

    let body = tokio::time::timeout(STEP_TIMEOUT, reqwest::get(format!("http://{local}/health")))
        .await
        .expect("GET through tunnel should not hang")
        .expect("GET through tunnel succeeds")
        .text()
        .await
        .expect("response body");
    assert_eq!(
        body, "SHARE-GLUE-MARKER",
        "tunnel target must be 127.0.0.1:{port}"
    );

    client.shutdown().await.expect("client shutdown");
    session.shutdown().await.expect("session shutdown");
}

/// The share task is gated on the server's ready signal: the tunnel's
/// relay wait must never delay server startup, and the banner must
/// print after the boot URL. Nothing may start before the gate fires —
/// with the hermetic preset an ungated spawn would finish in
/// milliseconds, so an early banner proves the gate is broken.
/// multi_thread: see `share_glue_tunnels_to_preview_port_and_announces_join_string`.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn share_task_waits_for_server_ready_gate() {
    // Stand-in for the preview hub, as in the glue test above.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind stand-in preview server");
    let port = listener.local_addr().expect("local_addr").port();
    let app = axum::Router::new().route("/health", axum::routing::get(|| async { "GATE-MARKER" }));
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("axum serve");
    });

    let announced = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let (ready_tx, ready_rx) = tokio::sync::watch::channel(false);
    let task = {
        let announced = announced.clone();
        spawn_share_task(
            hermetic_host_cfg(),
            "127.0.0.1".to_string(),
            port,
            false,
            ready_rx,
            move |banner| announced.lock().unwrap().push(banner.to_string()),
        )
    };

    // Before the gate: no banner, no finished task.
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(
        announced.lock().unwrap().is_empty(),
        "the banner must wait for the server-ready gate"
    );
    assert!(
        !task.is_finished(),
        "the share task must park on the ready gate"
    );

    // Fire the gate: the tunnel starts and the banner prints.
    ready_tx.send(true).expect("send ready");
    let session = tokio::time::timeout(STEP_TIMEOUT, task)
        .await
        .expect("share task should finish once gated")
        .expect("share task panicked")
        .expect("tunnel starts once the gate fires");
    assert_eq!(announced.lock().unwrap().len(), 1, "banner printed once");

    // And the tunnel actually works.
    let (local, client) = tokio::time::timeout(
        STEP_TIMEOUT,
        TunnelClient::bind(
            TunnelClientConfig {
                preset: EndpointPreset::HermeticLoopback,
            },
            session.ticket.clone(),
            "127.0.0.1:0".parse().unwrap(),
        ),
    )
    .await
    .expect("client bind should not hang")
    .expect("tunnel client binds");
    let body = tokio::time::timeout(STEP_TIMEOUT, reqwest::get(format!("http://{local}/health")))
        .await
        .expect("GET through tunnel should not hang")
        .expect("GET through tunnel succeeds")
        .text()
        .await
        .expect("response body");
    assert_eq!(body, "GATE-MARKER");

    client.shutdown().await.expect("client shutdown");
    session.shutdown().await.expect("session shutdown");
}

/// A server that dies before `on_ready` (the gate drops without ever
/// firing) must end the share task quietly: no banner, no session, no
/// lingering task for the caller to reap.
#[tokio::test]
async fn share_task_exits_quietly_when_server_dies_before_ready() {
    let announced = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let (ready_tx, ready_rx) = tokio::sync::watch::channel(false);
    let task = {
        let announced = announced.clone();
        spawn_share_task(
            hermetic_host_cfg(),
            "127.0.0.1".to_string(),
            1, // never dialed: the gate drops before the tunnel starts
            false,
            ready_rx,
            move |banner| announced.lock().unwrap().push(banner.to_string()),
        )
    };

    drop(ready_tx);
    let out = tokio::time::timeout(STEP_TIMEOUT, task)
        .await
        .expect("the share task must end when the gate drops")
        .expect("share task panicked");
    assert!(out.is_none(), "no session when the server never readied");
    assert!(
        announced.lock().unwrap().is_empty(),
        "no banner when the server never readied"
    );
}

/// Banner wording: what the token grants must be printed at share time
/// (security model). Read-only sessions must not claim edit capability;
/// `--allow-edit` sessions must warn about disk writes.
#[test]
fn banner_states_capabilities_per_allow_edit() {
    let read_only = format_share_banner("q2previewexample", false, true);
    assert!(
        read_only.contains("VIEW") && read_only.contains("RE-RUN"),
        "banner must state the view + re-run capability:\n{read_only}"
    );
    assert!(
        !read_only.contains("EDIT"),
        "read-only banner must not claim edit capability:\n{read_only}"
    );

    let editable = format_share_banner("q2previewexample", true, true);
    assert!(
        editable.contains("EDIT") && editable.contains("--allow-edit"),
        "--allow-edit banner must warn that guests can edit files on this machine:\n{editable}"
    );
}

/// Relay reachability: when the endpoint never came online, the banner
/// says so (guests on other networks won't be able to join).
#[test]
fn banner_warns_when_relay_unreachable() {
    let no_relay = format_share_banner("q2previewexample", false, false);
    assert!(
        no_relay.contains("direct/LAN"),
        "no-relay banner must warn about direct/LAN-only reachability:\n{no_relay}"
    );

    let with_relay = format_share_banner("q2previewexample", false, true);
    assert!(
        !with_relay.contains("direct/LAN"),
        "relay-reachable banner must not carry the LAN-only warning:\n{with_relay}"
    );
}

/// The join line stays last in every banner variant (copy-paste contract).
#[test]
fn banner_join_line_is_always_last() {
    for (allow_edit, relay) in [(false, true), (true, true), (false, false), (true, false)] {
        let banner = format_share_banner("q2previewexample", allow_edit, relay);
        assert!(
            banner
                .trim_end()
                .ends_with("q2 preview --join q2previewexample"),
            "join line must be the banner's last content \
             (allow_edit={allow_edit}, relay={relay}):\n{banner}"
        );
    }
}
