//! Phase 3 skeletons (bd-tl2j8js8; plan
//! `claude-notes/plans/2026-08-13-live-share-local-spa-assets.md`,
//! design decision 1): `TunnelClient::connect` — the transport half of
//! the guest, without the local TCP listener.
//!
//! `connect(cfg, ticket) -> TunnelConnection` owns the endpoint, the
//! initial dial, the supervisor/re-dial loop, and the status watch, and
//! exposes `open_stream() -> (SendStream, RecvStream)` (token prefix
//! applied internally). `TunnelClient::bind` keeps its signature and
//! behavior, reimplemented as `connect` + the existing splice accept
//! loop — the existing `tunnel.rs` tests pin that (they must stay
//! green **unmodified**).
//!
//! These are structural stubs: the API does not exist yet. Phase 3
//! starts by filling in the bodies (compile-red, the accepted
//! structural failure mode), then implements. All hermetic —
//! `EndpointPreset::HermeticLoopback`, no n0 infrastructure in CI.

#[tokio::test(flavor = "multi_thread")]
#[ignore = "Phase 3 skeleton (bd-tl2j8js8): TunnelClient::connect does not exist yet"]
async fn connect_open_stream_roundtrip() {
    // Spec: spawn a raw TCP echo target (not HTTP — connect() is a
    // transport seam); `TunnelHost::spawn(hermetic_host_cfg(), target)`;
    // `TunnelClient::connect(hermetic_client_cfg(), ticket)`; then
    // `conn.open_stream()`, write bytes, and read the echo back. The
    // host accepted the stream at all => the token prefix was applied
    // internally (a wrong token is the next test). Also assert the
    // status watch reports `TunnelStatus::Connected(_)`.
    //
    // Harness notes: `support.rs` has the hermetic cfgs and
    // STEP_TIMEOUT; add a 20-line TCP echo helper there (the existing
    // targets are axum HTTP).
    todo!("Phase 3: TunnelClient::connect + TunnelConnection::open_stream round-trip")
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "Phase 3 skeleton (bd-tl2j8js8): TunnelClient::connect does not exist yet"]
async fn connect_rejected_token_maps_to_rejected() {
    // Spec: same setup, but flip the ticket's token bytes before
    // connect (`PreviewShareTicket { addr, token }` fields are pub).
    // The host resets the stream and closes the connection with
    // `ERROR_CODE_UNAUTHORIZED`; the connection's status watch must
    // flip to `TunnelStatus::Rejected` and stay terminal (no re-dial
    // spin — the same token can never succeed). This is the
    // `connect()`-level pin for what
    // `tunnel::rejected_token_flips_status_terminal` covers at the
    // `bind()` level; the target must see zero TCP connections
    // (`tunnel::wrong_token_rejected` owns that assertion shape).
    todo!("Phase 3: rejected token -> terminal TunnelStatus::Rejected via connect()")
}
