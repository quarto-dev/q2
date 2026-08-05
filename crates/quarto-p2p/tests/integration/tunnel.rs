//! Tunnel behavior tests (plan Phase 1): HTTP + WebSocket splicing, token
//! auth, re-dial, half-close, shutdown. All hermetic — no n0 infrastructure.

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use axum::Router;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::routing::get;
use futures::{SinkExt, StreamExt};
use iroh::{SecretKey, TransportAddr};
use quarto_p2p::{ALPN, EndpointPreset, TunnelClient, TunnelHost, TunnelHostConfig, TunnelStatus};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::timeout;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::protocol::Message as WsMessage;

use crate::support::{
    STEP_TIMEOUT, hermetic_client_cfg, hermetic_host_cfg, http_get_close, http_get_keepalive,
    spawn_http_target, try_http_get_close,
};

fn any_loopback() -> SocketAddr {
    "127.0.0.1:0".parse().unwrap()
}

/// Host + client pair in front of `target`, hermetic on both ends.
async fn spawn_tunnel_pair(
    target: SocketAddr,
) -> (
    SocketAddr,
    quarto_p2p::TunnelHostHandle,
    quarto_p2p::TunnelClientHandle,
) {
    let (ticket, host) = TunnelHost::spawn(hermetic_host_cfg(), target)
        .await
        .expect("spawn tunnel host");
    let (local, client) = TunnelClient::bind(hermetic_client_cfg(), ticket, any_loopback())
        .await
        .expect("bind tunnel client");
    (local, host, client)
}

#[tokio::test(flavor = "multi_thread")]
async fn http_roundtrip_loopback() {
    let target = spawn_http_target(
        Router::new().route("/body", get(|| async { "hello-through-the-tunnel" })),
    )
    .await;
    let (local, host, client) = spawn_tunnel_pair(target).await;

    // ≥8 concurrent TCP connections = ≥8 concurrent QUIC bi-streams.
    let mut tasks = Vec::new();
    for _ in 0..8 {
        tasks.push(tokio::spawn(
            async move { http_get_close(local, "/body").await },
        ));
    }
    for task in tasks {
        let response = timeout(STEP_TIMEOUT, task)
            .await
            .expect("request timed out")
            .expect("request task panicked");
        assert!(
            response.starts_with("HTTP/1.1 200"),
            "expected a 200 response, got: {response}"
        );
        assert!(
            response.contains("hello-through-the-tunnel"),
            "body did not survive the tunnel: {response}"
        );
    }

    client.shutdown().await.expect("client shutdown");
    host.shutdown().await.expect("host shutdown");
}

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

#[tokio::test(flavor = "multi_thread")]
async fn websocket_frames_survive() {
    let target = spawn_http_target(Router::new().route("/ws", get(ws_echo))).await;
    let (local, host, client) = spawn_tunnel_pair(target).await;

    let (mut ws, _response) = timeout(STEP_TIMEOUT, connect_async(format!("ws://{local}/ws")))
        .await
        .expect("ws upgrade timed out")
        .expect("ws upgrade failed");

    for i in 0..3 {
        let text = format!("frame-{i}");
        ws.send(WsMessage::text(text.clone()))
            .await
            .expect("send text frame");
        let echoed = timeout(STEP_TIMEOUT, ws.next())
            .await
            .expect("echo timed out")
            .expect("ws stream ended early")
            .expect("ws read failed");
        assert_eq!(echoed.into_text().expect("text frame").as_str(), text);
    }

    let payload = vec![0u8, 1, 2, 3, 255];
    ws.send(WsMessage::binary(payload.clone()))
        .await
        .expect("send binary frame");
    let echoed = timeout(STEP_TIMEOUT, ws.next())
        .await
        .expect("binary echo timed out")
        .expect("ws stream ended early")
        .expect("ws read failed");
    match echoed {
        WsMessage::Binary(bytes) => assert_eq!(bytes.as_ref(), payload.as_slice()),
        other => panic!("expected a binary echo, got: {other:?}"),
    }

    ws.close(None).await.expect("ws close");

    client.shutdown().await.expect("client shutdown");
    host.shutdown().await.expect("host shutdown");
}

#[tokio::test(flavor = "multi_thread")]
async fn wrong_token_rejected() {
    // Raw TCP target that counts accepted connections; a rejected stream
    // must never produce one.
    let accepts = Arc::new(AtomicUsize::new(0));
    let listener = TcpListener::bind(any_loopback())
        .await
        .expect("bind target");
    let target = listener.local_addr().expect("target addr");
    {
        let accepts = accepts.clone();
        tokio::spawn(async move {
            loop {
                let _ = listener.accept().await;
                accepts.fetch_add(1, Ordering::SeqCst);
            }
        });
    }

    // Fix the session token so the wrong one below is wrong by construction.
    let cfg = TunnelHostConfig {
        preset: EndpointPreset::HermeticLoopback,
        token: Some([0xAA; 32]),
        ..Default::default()
    };
    let (ticket, host) = TunnelHost::spawn(cfg, target).await.expect("spawn host");

    let dialer = iroh::Endpoint::builder(iroh::endpoint::presets::Minimal)
        .relay_mode(iroh::RelayMode::Disabled)
        .clear_ip_transports()
        .bind_addr("127.0.0.1:0")
        .expect("loopback bind addr")
        .bind()
        .await
        .expect("bind raw dialer");

    // Case 1: full-length token with the wrong bytes.
    let conn = dialer
        .connect(ticket.addr.clone(), ALPN)
        .await
        .expect("dial host");
    let (mut send, mut recv) = conn.open_bi().await.expect("open_bi");
    send.write_all(&[0u8; 32]).await.expect("write wrong token");
    let read = timeout(STEP_TIMEOUT, recv.read_to_end(16))
        .await
        .expect("host did not react to the wrong token");
    assert!(
        read.is_err(),
        "stream should be reset after a wrong token, got: {read:?}"
    );
    let closed = timeout(STEP_TIMEOUT, conn.closed())
        .await
        .expect("host did not close the connection after a wrong token");
    drop(closed);

    // Case 2: short token (stream finished after 5 bytes).
    let conn = dialer
        .connect(ticket.addr.clone(), ALPN)
        .await
        .expect("re-dial host");
    let (mut send, mut recv) = conn.open_bi().await.expect("open_bi");
    send.write_all(&[0xAA; 5]).await.expect("write short token");
    send.finish().expect("finish short stream");
    let read = timeout(STEP_TIMEOUT, recv.read_to_end(16))
        .await
        .expect("host did not react to the short token");
    assert!(
        read.is_err(),
        "stream should be reset after a short token, got: {read:?}"
    );

    // The target must never have seen a TCP connection.
    tokio::time::sleep(Duration::from_millis(250)).await;
    assert_eq!(
        accepts.load(Ordering::SeqCst),
        0,
        "unauthenticated streams must not reach the target"
    );

    dialer.close().await;
    host.shutdown().await.expect("host shutdown");
}

#[tokio::test(flavor = "multi_thread")]
async fn client_redials_after_connection_loss() {
    let target = spawn_http_target(Router::new().route("/", get(|| async { "redial-ok" }))).await;

    // Fixed identity + token + (after first spawn) UDP port, so the restarted
    // host is reachable via the unchanged ticket.
    let secret_key = SecretKey::from_bytes(&[13u8; 32]);
    let token = [0x42u8; 32];
    let cfg = TunnelHostConfig {
        preset: EndpointPreset::HermeticLoopback,
        secret_key: Some(secret_key.clone()),
        token: Some(token),
        bind_addr: None,
    };
    let (ticket, first_host) = TunnelHost::spawn(cfg, target).await.expect("spawn host");
    let udp_addr = ticket
        .addr
        .addrs
        .iter()
        .find_map(|a| match a {
            TransportAddr::Ip(sa) => Some(*sa),
            _ => None,
        })
        .expect("hermetic ticket has an ip transport addr");

    let (local, client) = TunnelClient::bind(hermetic_client_cfg(), ticket.clone(), any_loopback())
        .await
        .expect("bind client");
    let mut status = client.status();
    assert_eq!(*status.borrow(), TunnelStatus::Connected);

    let first = http_get_close(local, "/").await;
    assert!(
        first.contains("redial-ok"),
        "sanity roundtrip failed: {first}"
    );

    // Drop the host-side connection by shutting the host down entirely.
    first_host.shutdown().await.expect("first host shutdown");

    timeout(
        STEP_TIMEOUT,
        status.wait_for(|s| *s == TunnelStatus::Reconnecting),
    )
    .await
    .expect("client never noticed the connection loss")
    .expect("status channel closed");

    // Restart the host: same identity, token, target, and UDP port.
    let cfg = TunnelHostConfig {
        preset: EndpointPreset::HermeticLoopback,
        secret_key: Some(secret_key),
        token: Some(token),
        bind_addr: Some(udp_addr),
    };
    let (restart_ticket, second_host) = TunnelHost::spawn(cfg, target)
        .await
        .expect("respawn host on the same udp addr");
    assert_eq!(restart_ticket.addr.id, ticket.addr.id);

    // The next local TCP connections succeed once the client re-dialed.
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut recovered = false;
    while Instant::now() < deadline {
        if let Ok(response) = try_http_get_close(local, "/").await
            && response.contains("redial-ok")
        {
            recovered = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    assert!(recovered, "tunnel did not recover after the host restarted");

    timeout(
        STEP_TIMEOUT,
        status.wait_for(|s| *s == TunnelStatus::Connected),
    )
    .await
    .expect("status never returned to Connected")
    .expect("status channel closed");

    client.shutdown().await.expect("client shutdown");
    second_host.shutdown().await.expect("second host shutdown");
}

#[tokio::test(flavor = "multi_thread")]
async fn half_close_propagates() {
    // Raw TCP target driven by a task whose asserts propagate via join.
    let listener = TcpListener::bind(any_loopback())
        .await
        .expect("bind target");
    let target = listener.local_addr().expect("target addr");
    let target_task = tokio::spawn(async move {
        let (mut sock, _) = listener.accept().await.expect("accept");
        let mut buf = [0u8; 4];
        sock.read_exact(&mut buf).await.expect("read ping");
        assert_eq!(&buf, b"ping");

        // The guest shut down its write half → we must see EOF…
        let n = sock.read(&mut [0u8; 8]).await.expect("read eof");
        assert_eq!(n, 0, "expected EOF after guest write-half shutdown");

        // …while the reverse direction keeps flowing.
        sock.write_all(b"pong").await.expect("write pong");
        sock.shutdown().await.expect("target write shutdown");
    });

    let (local, host, client) = spawn_tunnel_pair(target).await;

    let mut guest = TcpStream::connect(local)
        .await
        .expect("connect local proxy");
    guest.write_all(b"ping").await.expect("write ping");
    guest.shutdown().await.expect("guest write-half shutdown");

    let mut buf = [0u8; 4];
    timeout(STEP_TIMEOUT, guest.read_exact(&mut buf))
        .await
        .expect("pong timed out")
        .expect("read pong");
    assert_eq!(&buf, b"pong");

    let n = timeout(STEP_TIMEOUT, guest.read(&mut [0u8; 8]))
        .await
        .expect("guest EOF timed out")
        .expect("read guest eof");
    assert_eq!(n, 0, "expected EOF after target write shutdown");

    timeout(STEP_TIMEOUT, target_task)
        .await
        .expect("target task timed out")
        .expect("target task panicked");

    client.shutdown().await.expect("client shutdown");
    host.shutdown().await.expect("host shutdown");
}

#[tokio::test(flavor = "multi_thread")]
async fn clean_shutdown() {
    let target = spawn_http_target(Router::new().route("/", get(|| async { "shutdown-ok" }))).await;
    let (local, host, client) = spawn_tunnel_pair(target).await;

    // Exercise the pair once so shutdown happens on a live tunnel.
    let response = http_get_close(local, "/").await;
    assert!(response.contains("shutdown-ok"));

    timeout(STEP_TIMEOUT, client.shutdown())
        .await
        .expect("client shutdown hung")
        .expect("client shutdown failed");

    // The local proxy port must be unbound again.
    let connect = TcpStream::connect(local).await;
    assert!(
        connect.is_err(),
        "local proxy port still accepting after shutdown"
    );

    timeout(STEP_TIMEOUT, host.shutdown())
        .await
        .expect("host shutdown hung")
        .expect("host shutdown failed");
}

/// Verifies the plan's QUIC-keep-alive-vs-browser-connection-pooling item:
/// iroh's default 5 s keep-alive must hold the QUIC connection (30 s idle
/// timeout) open underneath an idle pooled HTTP/1.1 connection, so the next
/// request on that pooled TCP connection still succeeds. Idles for 35 s by
/// design — this is deliberately the slowest test in the crate.
#[tokio::test(flavor = "multi_thread")]
async fn idle_pooled_conn_survives_quic_keepalive() {
    let target =
        spawn_http_target(Router::new().route("/", get(|| async { "keepalive-ok" }))).await;
    let (local, host, client) = spawn_tunnel_pair(target).await;

    let mut pooled = TcpStream::connect(local)
        .await
        .expect("connect local proxy");
    let first = http_get_keepalive(&mut pooled, "/").await;
    assert!(first.contains("keepalive-ok"), "first response: {first}");

    // Longer than the 30 s QUIC connection idle timeout.
    tokio::time::sleep(Duration::from_secs(35)).await;

    let second = http_get_keepalive(&mut pooled, "/").await;
    assert!(
        second.contains("keepalive-ok"),
        "pooled connection died during idle: {second}"
    );

    client.shutdown().await.expect("client shutdown");
    host.shutdown().await.expect("host shutdown");
}
