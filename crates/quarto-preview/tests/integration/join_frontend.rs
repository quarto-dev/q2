//! Phase 3 tests (bd-tl2j8js8; plan
//! `claude-notes/plans/2026-08-13-live-share-local-spa-assets.md`,
//! design decisions 3 + 6): the L7 join frontend — per-connection
//! head-peek routing between the guest's embedded assets and the
//! tunnel.
//!
//! Harness:
//!
//! - Host side: a real preview hub on a fixture project (the
//!   `join_tunnel.rs` boot pattern) with a **request-logging TCP shim**
//!   between the tunnel host and the hub (read each request head,
//!   record method+path, forward verbatim) — that log is how the tests
//!   observe which requests traversed the tunnel.
//! - Guest side: the join frontend bound on a loopback port, fed the
//!   host ticket and a guest manifest state (matching / mismatched /
//!   absent — the fixture knob the frontend API takes so the mismatch
//!   cases are exercisable hermetically).
//! - "Served locally" assertions compare against what the host serves
//!   for the same path directly: byte-identical bodies and the same
//!   Content-Type / Content-Length / Content-Encoding / Cache-Control
//!   (the frontend shares `asset_response`; header logic is never
//!   forked).
//!
//! Placeholder trees (fresh clone, no built dist) ship no manifest;
//! the local-serving tests no-op there, matching the crate's
//! both-tree-states embed-test pattern.

use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use quarto_p2p::{EndpointPreset, TunnelClient, TunnelClientConfig, TunnelHost, TunnelHostConfig};
use quarto_preview::join_frontend::{JoinFrontendHandle, LocalServing};
use quarto_preview::{PreviewConfig, PreviewUi};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

fn any_loopback() -> SocketAddr {
    "127.0.0.1:0".parse().unwrap()
}

/// Bind `127.0.0.1:0`, capture the assigned port, release the listener.
/// Same tiny-race trade-off as the CLI's own port probe.
fn pick_free_port() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("probe bind");
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

// ─── Request-logging shim ────────────────────────────────────────────────────

/// Recorded request heads (method + path) crossing the shim.
#[derive(Clone, Default)]
struct RequestLog(Arc<Mutex<Vec<(String, String)>>>);

impl RequestLog {
    fn record(&self, method: &str, path: &str) {
        self.0
            .lock()
            .unwrap()
            .push((method.to_string(), path.to_string()));
    }

    /// All recorded request lines as `"METHOD path"`.
    fn entries(&self) -> Vec<String> {
        self.0
            .lock()
            .unwrap()
            .iter()
            .map(|(m, p)| format!("{m} {p}"))
            .collect()
    }
}

/// A TCP forwarder between the tunnel host and the hub: reads each
/// connection's request head (bounded), records method + path, then
/// forwards the consumed bytes verbatim and splices. The tests'
/// observation point for which requests traversed the tunnel.
async fn spawn_logging_shim(target: SocketAddr) -> (SocketAddr, RequestLog) {
    let listener = TcpListener::bind(any_loopback()).await.expect("bind shim");
    let addr = listener.local_addr().expect("shim addr");
    let log = RequestLog::default();
    let task_log = log.clone();
    tokio::spawn(async move {
        loop {
            let Ok((mut inbound, _)) = listener.accept().await else {
                break;
            };
            let log = task_log.clone();
            tokio::spawn(async move {
                let mut buf = Vec::new();
                let head_end = loop {
                    if let Some(end) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                        break end + 4;
                    }
                    if buf.len() > 64 * 1024 {
                        return; // test traffic never approaches this
                    }
                    let mut chunk = [0u8; 8192];
                    match inbound.read(&mut chunk).await {
                        Ok(0) | Err(_) => return,
                        Ok(n) => buf.extend_from_slice(&chunk[..n]),
                    }
                };
                if let Some(line) = buf[..head_end].split(|&b| b == b'\n').next() {
                    let line = String::from_utf8_lossy(line);
                    let mut parts = line.split_whitespace();
                    if let (Some(method), Some(path)) = (parts.next(), parts.next()) {
                        log.record(method, path);
                    }
                }
                let Ok(mut outbound) = TcpStream::connect(target).await else {
                    return;
                };
                if outbound.write_all(&buf).await.is_err() {
                    return;
                }
                let _ = tokio::io::copy_bidirectional(&mut inbound, &mut outbound).await;
            });
        }
    });
    (addr, log)
}

// ─── Raw HTTP client ─────────────────────────────────────────────────────────

/// A raw HTTP/1.1 response, as read off the wire.
struct RawResponse {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl RawResponse {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

/// One HTTP/1.1 request with `Connection: close` on a fresh TCP
/// connection; reads the response to EOF. Raw so the assertions see
/// the exact wire behavior (reqwest would hide `Connection` and
/// content negotiation).
async fn raw_http(
    addr: SocketAddr,
    method: &str,
    path: &str,
    extra: &[(&str, &str)],
) -> RawResponse {
    let mut stream = TcpStream::connect(addr).await.expect("connect");
    let mut request = format!("{method} {path} HTTP/1.1\r\nHost: join-frontend-test\r\n");
    for (name, value) in extra {
        request.push_str(&format!("{name}: {value}\r\n"));
    }
    request.push_str("Connection: close\r\n\r\n");
    stream
        .write_all(request.as_bytes())
        .await
        .expect("write request");
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).await.expect("read response");
    parse_response(&raw)
}

fn parse_response(raw: &[u8]) -> RawResponse {
    let head_end = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .expect("response head terminator")
        + 4;
    let head = String::from_utf8_lossy(&raw[..head_end]);
    let mut lines = head.split("\r\n");
    let status_line = lines.next().expect("status line");
    let status: u16 = status_line
        .split(' ')
        .nth(1)
        .expect("status code")
        .parse()
        .expect("numeric status");
    let headers = lines
        .filter_map(|line| {
            let (name, value) = line.split_once(':')?;
            Some((name.trim().to_string(), value.trim().to_string()))
        })
        .collect();
    RawResponse {
        status,
        headers,
        body: raw[head_end..].to_vec(),
    }
}

/// The headers the shared `asset_response` builder owns — the set the
/// plan requires to be identical between a locally served response and
/// the host's.
const SHARED_ASSET_HEADERS: [&str; 4] = [
    "content-type",
    "content-length",
    "content-encoding",
    "cache-control",
];

/// Assert the guest's response matches the host's direct response:
/// byte-identical body plus the shared-builder headers.
fn assert_same_response(guest: &RawResponse, host: &RawResponse, what: &str) {
    assert_eq!(guest.status, host.status, "{what}: status");
    assert_eq!(guest.body, host.body, "{what}: body must be byte-identical");
    for name in SHARED_ASSET_HEADERS {
        assert_eq!(guest.header(name), host.header(name), "{what}: {name}");
    }
}

// ─── The rig ─────────────────────────────────────────────────────────────────

/// The full hermetic rig: fixture project → hub → logging shim →
/// tunnel host → tunnel connection → join frontend on the guest port.
/// `hub` is the hub's own port — direct fetches there bypass the shim,
/// so comparison fetches never pollute the request log.
struct Rig {
    guest: SocketAddr,
    hub: SocketAddr,
    log: RequestLog,
    frontend: JoinFrontendHandle,
    tunnel_host: quarto_p2p::TunnelHostHandle,
    server: tokio::task::JoinHandle<()>,
    _project: tempfile::TempDir,
    _data: tempfile::TempDir,
}

async fn boot_rig(ui: PreviewUi, serving: LocalServing) -> Rig {
    let project = tempfile::TempDir::with_prefix("q2-join-frontend-proj-").unwrap();
    std::fs::write(
        project.path().join("_quarto.yml"),
        "project:\n  type: website\n",
    )
    .unwrap();
    std::fs::write(project.path().join("index.qmd"), "# Index\n\nHello.\n").unwrap();
    std::fs::write(project.path().join("about.qmd"), "# About\n").unwrap();
    let data = tempfile::TempDir::with_prefix("q2-join-frontend-data-").unwrap();

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
        ui,
    };
    // The real hub, exactly as `q2 preview` runs it. `run()` blocks
    // until shutdown; the task dies with the test process (nextest
    // isolates).
    let server = tokio::spawn(async move {
        let _ = quarto_preview::run(config).await;
    });
    wait_for_health(port).await;
    let hub: SocketAddr = ([127, 0, 0, 1], port).into();

    let (shim, log) = spawn_logging_shim(hub).await;
    let (ticket, tunnel_host) = TunnelHost::spawn(
        TunnelHostConfig {
            preset: EndpointPreset::HermeticLoopback,
            ..Default::default()
        },
        shim,
    )
    .await
    .expect("spawn tunnel host");
    let conn = TunnelClient::connect(
        TunnelClientConfig {
            preset: EndpointPreset::HermeticLoopback,
        },
        ticket,
    )
    .await
    .expect("connect");
    let (guest, frontend) = quarto_preview::join_frontend::bind(conn, any_loopback(), serving)
        .await
        .expect("bind join frontend");

    Rig {
        guest,
        hub,
        log,
        frontend,
        tunnel_host,
        server,
        _project: project,
        _data: data,
    }
}

impl Rig {
    async fn shutdown(self) {
        self.frontend.shutdown().await.expect("frontend shutdown");
        self.tunnel_host
            .shutdown()
            .await
            .expect("tunnel host shutdown");
        self.server.abort();
    }
}

/// The guest's own embedded manifest for `ui` — the "matching" fixture
/// state (host and guest are the same binary in-process, so the hashes
/// match by construction). `None` on a placeholder tree.
fn matching_manifest(ui: PreviewUi) -> Option<spa_manifest::Manifest> {
    quarto_preview::embedded_manifest(ui)
}

/// A real content-hashed JS asset path from the manifest (the dist's
/// hashed filenames change every build, so tests discover one).
fn discover_js_asset(manifest: &spa_manifest::Manifest) -> String {
    manifest
        .entries
        .iter()
        .find(|e| e.path.starts_with("assets/") && e.path.ends_with(".js"))
        .expect("a built dist has a JS asset")
        .path
        .clone()
}

// ─── Tests ───────────────────────────────────────────────────────────────────

/// Routing rule (design decision 3): a GET/HEAD whose path — after
/// `spa_handler`'s exact normalization (query stripped, leading `/`
/// trimmed, empty → `index.html`, raw percent-encoded path, no
/// decoding) — hits the manifest exactly is served from the embedded
/// bundle; everything else tunnels.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn matching_manifest_serves_assets_locally() {
    let Some(manifest) = matching_manifest(PreviewUi::Viewer) else {
        eprintln!("placeholder embed (no built dist); nothing to serve locally");
        return;
    };
    let asset = discover_js_asset(&manifest);
    let rig = boot_rig(
        PreviewUi::Viewer,
        LocalServing::Embedded {
            ui: PreviewUi::Viewer,
            manifest,
        },
    )
    .await;

    // GET / (→ exact `index.html` manifest hit) is served locally:
    // byte-identical body and the same shared-builder headers as the
    // host's direct answer, plus `Connection: close` (the
    // mixed-keep-alive mitigation).
    let guest_index = raw_http(rig.guest, "GET", "/", &[("Accept-Encoding", "gzip")]).await;
    let host_index = raw_http(rig.hub, "GET", "/", &[("Accept-Encoding", "gzip")]).await;
    assert_same_response(&guest_index, &host_index, "GET /");
    assert_eq!(
        guest_index.header("connection"),
        Some("close"),
        "local responses carry Connection: close; headers: {:?}",
        guest_index.headers
    );

    // A real `/assets/*` path: same, with the immutable cache contract.
    let asset_path = format!("/{asset}");
    let guest_asset = raw_http(
        rig.guest,
        "GET",
        &asset_path,
        &[("Accept-Encoding", "gzip")],
    )
    .await;
    let host_asset = raw_http(rig.hub, "GET", &asset_path, &[("Accept-Encoding", "gzip")]).await;
    assert_same_response(&guest_asset, &host_asset, "GET /assets/*");
    assert_eq!(
        guest_asset.header("cache-control"),
        Some("public, max-age=31536000, immutable"),
        "content-hashed assets are immutable"
    );
    assert_eq!(
        guest_asset.header("connection"),
        Some("close"),
        "local responses carry Connection: close"
    );

    // The dynamic traffic tunnels: `/health`, `/api/preview/config`,
    // `/auth/me` all reach the host and answer as the host would.
    for path in ["/health", "/api/preview/config", "/auth/me"] {
        let guest = raw_http(rig.guest, "GET", path, &[]).await;
        let host = raw_http(rig.hub, "GET", path, &[]).await;
        assert_eq!(guest.status, host.status, "{path}: status");
        assert_eq!(guest.body, host.body, "{path}: body");
    }

    // A `/ws` upgrade flows through the fallback splice.
    let (mut ws, response) = tokio_tungstenite::connect_async(format!("ws://{}/ws", rig.guest))
        .await
        .expect("ws upgrade through the guest port");
    assert_eq!(response.status(), 101, "the /ws upgrade must complete");
    ws.close(None).await.expect("ws close");

    // The host saw the dynamic requests — and ZERO asset requests.
    let entries = rig.log.entries();
    for path in ["/health", "/api/preview/config", "/auth/me", "/ws"] {
        assert!(
            entries.iter().any(|e| e == &format!("GET {path}")),
            "the host must have seen GET {path}; log: {entries:?}"
        );
    }
    assert!(
        !entries.iter().any(|e| e == "GET /"),
        "the index must be served locally, never tunneled; log: {entries:?}"
    );
    assert!(
        !entries.iter().any(|e| e == &format!("GET /{asset}")),
        "assets must be served locally, never tunneled; log: {entries:?}"
    );

    rig.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn mismatched_manifest_tunnels_everything() {
    // The mismatch fixture knob: the mode decision's Tunnel outcome,
    // fed straight to the frontend.
    let rig = boot_rig(PreviewUi::Viewer, LocalServing::Tunnel).await;

    // The full boot request set — asset requests included — reaches
    // the host, and the guest's responses are the host's responses.
    let mut paths = vec![
        "/".to_string(),
        "/health".to_string(),
        "/api/preview/config".to_string(),
    ];
    if let Some(manifest) = matching_manifest(PreviewUi::Viewer) {
        paths.push(format!("/{}", discover_js_asset(&manifest)));
    }
    for path in &paths {
        let guest = raw_http(rig.guest, "GET", path, &[("Accept-Encoding", "gzip")]).await;
        let host = raw_http(rig.hub, "GET", path, &[("Accept-Encoding", "gzip")]).await;
        assert_same_response(&guest, &host, &format!("GET {path}"));
    }

    let entries = rig.log.entries();
    for path in &paths {
        assert!(
            entries.iter().any(|e| e == &format!("GET {path}")),
            "mismatched guest must tunnel GET {path}; log: {entries:?}"
        );
    }

    rig.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn unknown_path_tunnels_and_receives_host_index() {
    let Some(manifest) = matching_manifest(PreviewUi::Viewer) else {
        eprintln!("placeholder embed (no built dist); nothing to serve locally");
        return;
    };
    let rig = boot_rig(
        PreviewUi::Viewer,
        LocalServing::Embedded {
            ui: PreviewUi::Viewer,
            manifest,
        },
    )
    .await;

    // No manifest hit for `/no-such-path`: it tunnels, and the answer
    // is the host's own SPA-fallback `index.html` — never a locally
    // synthesized one. There is deliberately no local index fallback:
    // the host stays the single authority on what is dynamic, so a
    // present-or-future host route can never be shadowed.
    let guest = raw_http(rig.guest, "GET", "/no-such-path", &[]).await;
    let host = raw_http(rig.hub, "GET", "/no-such-path", &[]).await;
    assert_same_response(&guest, &host, "GET /no-such-path");
    let host_index = raw_http(rig.hub, "GET", "/", &[]).await;
    assert_eq!(
        guest.body, host_index.body,
        "an unknown path gets the host's index.html, byte-identical"
    );

    let entries = rig.log.entries();
    assert!(
        entries.iter().any(|e| e == "GET /no-such-path"),
        "the unknown path must have reached the host; log: {entries:?}"
    );

    rig.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn websocket_survives_fallback_splice() {
    // No hub needed: the splice is transport-level. A ws echo target
    // behind the tunnel (the `tunnel::websocket_frames_survive` shape)
    // proves the head-peek didn't eat or corrupt the upgrade.
    use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
    use axum::routing::get;
    use futures::{SinkExt, StreamExt};

    async fn ws_echo(ws: WebSocketUpgrade) -> axum::response::Response {
        ws.on_upgrade(|mut socket: WebSocket| async move {
            while let Some(Ok(msg)) = socket.recv().await {
                if matches!(msg, Message::Close(_)) {
                    break;
                }
                if socket.send(msg).await.is_err() {
                    break;
                }
            }
        })
    }

    let listener = TcpListener::bind(any_loopback())
        .await
        .expect("bind echo target");
    let target = listener.local_addr().expect("echo addr");
    tokio::spawn(async move {
        axum::serve(listener, axum::Router::new().route("/ws", get(ws_echo)))
            .await
            .expect("axum serve");
    });

    let (ticket, tunnel_host) = TunnelHost::spawn(
        TunnelHostConfig {
            preset: EndpointPreset::HermeticLoopback,
            ..Default::default()
        },
        target,
    )
    .await
    .expect("spawn tunnel host");
    let conn = TunnelClient::connect(
        TunnelClientConfig {
            preset: EndpointPreset::HermeticLoopback,
        },
        ticket,
    )
    .await
    .expect("connect");
    // The matching-manifest state is the point of the test (the splice
    // is the *fallback within* local serving), so skip on placeholder
    // trees where no manifest exists.
    let Some(manifest) = matching_manifest(PreviewUi::Viewer) else {
        eprintln!("placeholder embed (no built dist); nothing to serve locally");
        return;
    };
    let (guest, frontend) = quarto_preview::join_frontend::bind(
        conn,
        any_loopback(),
        LocalServing::Embedded {
            ui: PreviewUi::Viewer,
            manifest,
        },
    )
    .await
    .expect("bind join frontend");

    let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://{guest}/ws"))
        .await
        .expect("ws upgrade through the frontend");
    for i in 0..3 {
        let text = format!("frame-{i}");
        ws.send(tokio_tungstenite::tungstenite::Message::text(text.clone()))
            .await
            .expect("send text frame");
        let echoed = tokio::time::timeout(Duration::from_secs(30), ws.next())
            .await
            .expect("echo timed out")
            .expect("ws stream ended early")
            .expect("ws read failed");
        assert_eq!(
            echoed.into_text().expect("text frame").as_str(),
            text,
            "frames must round-trip through the fallback splice untouched"
        );
    }
    ws.close(None).await.expect("ws close");

    frontend.shutdown().await.expect("frontend shutdown");
    tunnel_host.shutdown().await.expect("tunnel host shutdown");
}

/// A TCP listener that counts accepted connections — the "nothing
/// tunneled" observation point for the oversize/timeout tests (no hub
/// needed: those paths never open a stream).
async fn spawn_counting_target() -> (SocketAddr, Arc<AtomicUsize>) {
    let accepts = Arc::new(AtomicUsize::new(0));
    let listener = TcpListener::bind(any_loopback())
        .await
        .expect("bind counting target");
    let addr = listener.local_addr().expect("target addr");
    {
        let accepts = accepts.clone();
        tokio::spawn(async move {
            loop {
                let _ = listener.accept().await;
                accepts.fetch_add(1, Ordering::SeqCst);
            }
        });
    }
    (addr, accepts)
}

/// A frontend in front of a counting target, for the tests that must
/// prove nothing tunneled.
async fn boot_counting_frontend() -> (
    SocketAddr,
    Arc<AtomicUsize>,
    JoinFrontendHandle,
    quarto_p2p::TunnelHostHandle,
) {
    let (target, accepts) = spawn_counting_target().await;
    let (ticket, tunnel_host) = TunnelHost::spawn(
        TunnelHostConfig {
            preset: EndpointPreset::HermeticLoopback,
            ..Default::default()
        },
        target,
    )
    .await
    .expect("spawn tunnel host");
    let conn = TunnelClient::connect(
        TunnelClientConfig {
            preset: EndpointPreset::HermeticLoopback,
        },
        ticket,
    )
    .await
    .expect("connect");
    let (guest, frontend) =
        quarto_preview::join_frontend::bind(conn, any_loopback(), LocalServing::Tunnel)
            .await
            .expect("bind join frontend");
    (guest, accepts, frontend, tunnel_host)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn oversize_head_gets_431_and_close() {
    let (guest, accepts, frontend, tunnel_host) = boot_counting_frontend().await;

    // A request head larger than the 64 KiB peek bound. The head was
    // never fully read, so it cannot be replayed verbatim: 431 +
    // close, and nothing tunnels.
    let mut stream = TcpStream::connect(guest).await.expect("connect guest");
    let request = format!(
        "GET / HTTP/1.1\r\nX-Filler: {}\r\n\r\n",
        "A".repeat(128 * 1024)
    );
    stream
        .write_all(request.as_bytes())
        .await
        .expect("write oversize head");
    let mut raw = Vec::new();
    stream
        .read_to_end(&mut raw)
        .await
        .expect("read 431 response");
    let text = String::from_utf8_lossy(&raw);
    assert!(
        text.starts_with("HTTP/1.1 431"),
        "an oversize head must get 431 Request Header Fields Too Large; got: {text:?}"
    );
    assert_eq!(
        accepts.load(Ordering::SeqCst),
        0,
        "an oversize head must never tunnel"
    );

    frontend.shutdown().await.expect("frontend shutdown");
    tunnel_host.shutdown().await.expect("tunnel host shutdown");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn head_peek_timeout_closes() {
    let (guest, accepts, frontend, tunnel_host) = boot_counting_frontend().await;

    // An incomplete head (no `\r\n\r\n`) within the 5 s peek timeout:
    // the connection closes without any response and without the host
    // seeing a request.
    let mut stream = TcpStream::connect(guest).await.expect("connect guest");
    stream
        .write_all(b"GET / HTTP/1.1\r\nHost: join-frontend-test\r\n")
        .await
        .expect("write partial head");
    let started = Instant::now();
    let mut raw = Vec::new();
    let read = tokio::time::timeout(Duration::from_secs(30), stream.read_to_end(&mut raw)).await;
    assert!(
        read.is_ok(),
        "the connection must close after the 5 s peek timeout, not hang"
    );
    assert!(raw.is_empty(), "a timed-out peek gets no response: {raw:?}");
    assert!(
        started.elapsed() >= Duration::from_secs(5),
        "the close must come from the 5 s peek timeout, not earlier; elapsed {:?}",
        started.elapsed()
    );
    assert_eq!(
        accepts.load(Ordering::SeqCst),
        0,
        "a stalled head must never tunnel"
    );

    frontend.shutdown().await.expect("frontend shutdown");
    tunnel_host.shutdown().await.expect("tunnel host shutdown");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn head_request_gets_headers_only_with_content_length() {
    let Some(manifest) = matching_manifest(PreviewUi::Viewer) else {
        eprintln!("placeholder embed (no built dist); nothing to serve locally");
        return;
    };
    let asset = discover_js_asset(&manifest);
    let rig = boot_rig(
        PreviewUi::Viewer,
        LocalServing::Embedded {
            ui: PreviewUi::Viewer,
            manifest,
        },
    )
    .await;

    // HEAD semantics live in the shared `asset_response` builder: same
    // status + headers as the GET (Content-Length describes the
    // representation), but an empty body.
    let asset_path = format!("/{asset}");
    let get = raw_http(
        rig.guest,
        "GET",
        &asset_path,
        &[("Accept-Encoding", "gzip")],
    )
    .await;
    let head = raw_http(
        rig.guest,
        "HEAD",
        &asset_path,
        &[("Accept-Encoding", "gzip")],
    )
    .await;
    assert_eq!(head.status, get.status, "HEAD status must match GET");
    for name in SHARED_ASSET_HEADERS {
        assert_eq!(
            head.header(name),
            get.header(name),
            "HEAD {name} must match GET"
        );
    }
    assert!(
        head.header("content-length")
            .is_some_and(|v| v.parse::<usize>().unwrap() > 0),
        "HEAD must carry the representation's Content-Length; headers: {:?}",
        head.headers
    );
    assert!(head.body.is_empty(), "HEAD body must be empty");

    // Served locally: the host saw neither request.
    let entries = rig.log.entries();
    assert!(
        !entries.iter().any(|e| e.ends_with(&asset_path)),
        "HEAD/GET on a manifest hit must not tunnel; log: {entries:?}"
    );

    rig.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn editor_ui_boots_from_local_editor_index() {
    let Some(manifest) = matching_manifest(PreviewUi::Editor) else {
        eprintln!("placeholder editor embed (dist-preview-embed not built); skipping");
        return;
    };
    let rig = boot_rig(
        PreviewUi::Editor,
        LocalServing::Embedded {
            ui: PreviewUi::Editor,
            manifest,
        },
    )
    .await;

    // GET `/` normalizes to an exact `index.html` manifest hit and is
    // served from the guest's *editor* embed — byte-identical to the
    // host's editor index. Pins the post-resolution editor manifest
    // view (design decision 4) end to end.
    let guest = raw_http(rig.guest, "GET", "/", &[]).await;
    let host = raw_http(rig.hub, "GET", "/", &[]).await;
    assert_same_response(&guest, &host, "GET / (editor)");
    let editor_index = quarto_preview::embedded_editor_index_html()
        .expect("the editor embed always has an index.html (real dist or placeholder)");
    assert_eq!(
        guest.body,
        editor_index.as_bytes(),
        "the guest must serve the *editor* embed's index, never the viewer's"
    );

    let entries = rig.log.entries();
    assert!(
        !entries.iter().any(|e| e == "GET /"),
        "the editor index must be served locally, never tunneled; log: {entries:?}"
    );

    rig.shutdown().await;
}
