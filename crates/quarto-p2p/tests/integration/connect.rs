//! Phase 3 tests (bd-tl2j8js8; plan
//! `claude-notes/plans/2026-08-13-live-share-local-spa-assets.md`,
//! design decision 1): `TunnelClient::connect` — the transport half of
//! the guest, without the local TCP listener.
//!
//! `connect(cfg, ticket) -> TunnelConnection` owns the endpoint, the
//! initial dial, the supervisor/re-dial loop, and the status watch, and
//! exposes `open_stream() -> (SendStream, RecvStream)` (token prefix
//! applied internally). `TunnelClient::bind` keeps its signature and
//! behavior, reimplemented as `connect` + the existing splice accept
//! loop — the existing `tunnel.rs` tests pin that (they stay
//! green **unmodified**).
//!
//! All hermetic — `EndpointPreset::HermeticLoopback`, no n0
//! infrastructure in CI.

use quarto_p2p::{PreviewShareTicket, TunnelClient, TunnelHost, TunnelHostConfig, TunnelStatus};
use tokio::time::timeout;

use crate::support::{STEP_TIMEOUT, hermetic_client_cfg, hermetic_host_cfg, spawn_tcp_echo_target};

#[tokio::test(flavor = "multi_thread")]
async fn connect_open_stream_roundtrip() {
    // A raw TCP echo target (not HTTP — connect() is a transport seam).
    let target = spawn_tcp_echo_target().await;
    let (ticket, host) = TunnelHost::spawn(hermetic_host_cfg(), target)
        .await
        .expect("spawn tunnel host");
    let conn = TunnelClient::connect(hermetic_client_cfg(), ticket)
        .await
        .expect("connect");

    assert!(
        matches!(*conn.status().borrow(), TunnelStatus::Connected(_)),
        "fresh connection must report Connected"
    );

    // The host accepting the stream at all => the token prefix was
    // applied internally (a wrong token is the next test).
    let (mut send, mut recv) = conn.open_stream().await.expect("open_stream");
    send.write_all(b"hello-tunnel")
        .await
        .expect("write payload");
    let mut buf = [0u8; 12];
    timeout(STEP_TIMEOUT, recv.read_exact(&mut buf))
        .await
        .expect("echo timed out")
        .expect("echo read");
    assert_eq!(&buf, b"hello-tunnel", "payload must echo back verbatim");

    conn.shutdown().await.expect("connection shutdown");
    host.shutdown().await.expect("host shutdown");
}

#[tokio::test(flavor = "multi_thread")]
async fn connect_rejected_token_maps_to_rejected() {
    let target = spawn_tcp_echo_target().await;
    // Fix the session token so the wrong one below is wrong by
    // construction.
    let cfg = TunnelHostConfig {
        token: Some([0xAA; 32]),
        ..hermetic_host_cfg()
    };
    let (ticket, host) = TunnelHost::spawn(cfg, target).await.expect("spawn host");

    // The stale string: same endpoint address, zeroed token.
    let stale = PreviewShareTicket {
        addr: ticket.addr.clone(),
        token: [0u8; 32],
    };
    let conn = TunnelClient::connect(hermetic_client_cfg(), stale)
        .await
        .expect("connect (the QUIC handshake itself carries no token)");

    // Trip the rejection: open_stream writes the (wrong) token prefix;
    // the host resets the stream and closes the connection with
    // `ERROR_CODE_UNAUTHORIZED`.
    let (_send, mut recv) = conn
        .open_stream()
        .await
        .expect("stream opens; auth fails async");
    let read = timeout(STEP_TIMEOUT, recv.read_to_end(16))
        .await
        .expect("host did not react to the wrong token");
    assert!(
        read.is_err(),
        "the stream must be reset after a wrong token, got: {read:?}"
    );

    // The status watch flips to Rejected and stays terminal — no
    // re-dial spin with a token that can never succeed. This is the
    // `connect()`-level pin for what
    // `tunnel::rejected_token_flips_status_terminal` covers at the
    // `bind()` level (which also owns the "target sees zero TCP
    // connections" assertion shape).
    let mut status = conn.status();
    timeout(
        STEP_TIMEOUT,
        status.wait_for(|s| *s == TunnelStatus::Rejected),
    )
    .await
    .expect("connection never reported the token rejection")
    .expect("status channel closed");
    tokio::time::sleep(std::time::Duration::from_millis(750)).await;
    assert_eq!(
        *conn.status().borrow(),
        TunnelStatus::Rejected,
        "Rejected must be terminal"
    );
    // Terminal at the open_stream level too: no new streams.
    assert!(
        conn.open_stream_with_budget(std::time::Duration::from_millis(500))
            .await
            .is_none(),
        "a rejected connection must not open further streams"
    );

    conn.shutdown().await.expect("connection shutdown");
    host.shutdown().await.expect("host shutdown");
}
