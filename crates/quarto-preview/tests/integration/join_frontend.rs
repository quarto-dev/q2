//! Phase 3 skeletons (bd-tl2j8js8; plan
//! `claude-notes/plans/2026-08-13-live-share-local-spa-assets.md`,
//! design decisions 3 + 6): the L7 join frontend — per-connection
//! head-peek routing between the guest's embedded assets and the
//! tunnel.
//!
//! These are structural stubs: `quarto_preview::join_frontend` does not
//! exist yet. Phase 3 starts by filling in the bodies (compile-red,
//! the accepted structural failure mode), then implements. Harness
//! notes common to most tests:
//!
//! - Host side: a real preview hub on a fixture project (the
//!   `join_tunnel.rs` boot pattern) with a **request-logging TCP shim**
//!   between the tunnel host and the hub (read each request head,
//!   record method+path, forward verbatim) — that log is how the tests
//!   observe which requests traversed the tunnel.
//! - Guest side: the join frontend bound on a loopback port, fed the
//!   host ticket and a guest manifest state (matching / mismatched /
//!   absent — the fixture knob the frontend API must take so the
//!   mismatch cases are exercisable hermetically).
//! - "Served locally" assertions compare against what the host serves
//!   for the same path directly: byte-identical bodies and the same
//!   Content-Type / Content-Length / Content-Encoding / Cache-Control
//!   (the frontend shares `asset_response`; header logic is never
//!   forked).

/// Routing rule (design decision 3): a GET/HEAD whose path — after
/// `spa_handler`'s exact normalization (query stripped, leading `/`
/// trimmed, empty → `index.html`, raw percent-encoded path, no
/// decoding) — hits the manifest exactly is served from the embedded
/// bundle; everything else tunnels.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "Phase 3 skeleton (bd-tl2j8js8): join_frontend does not exist yet"]
async fn matching_manifest_serves_assets_locally() {
    // Spec: matching manifest. Through the guest port: GET `/` (→
    // index.html manifest hit), GET a real `/assets/*` path, GET
    // `/health`, GET `/api/preview/config`, GET `/auth/me`, and a `/ws`
    // upgrade. Assert: the host request log shows ZERO asset requests
    // (index + assets served locally, byte-identical to direct host
    // responses) while `/health`, `/api/preview/config`, `/auth/me`,
    // and `/ws` all reached the host. Local responses carry
    // `Connection: close` (the mixed-keep-alive mitigation).
    todo!("Phase 3: hash match -> assets local, dynamic traffic tunnels")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "Phase 3 skeleton (bd-tl2j8js8): join_frontend does not exist yet"]
async fn mismatched_manifest_tunnels_everything() {
    // Spec: guest manifest state forced to mismatch. The same request
    // set as above ALL reaches the host (asset requests included), and
    // the guest responses are byte-identical to direct host responses.
    todo!("Phase 3: hash mismatch -> full tunnel fallback")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "Phase 3 skeleton (bd-tl2j8js8): join_frontend does not exist yet"]
async fn unknown_path_tunnels_and_receives_host_index() {
    // Spec: matching manifest. GET `/no-such-path` (no manifest hit)
    // through the guest port. Assert: the request reached the host
    // (request log) and the body is the host's `index.html`
    // (byte-identical to a direct fetch) — never a locally synthesized
    // one. There is deliberately no local SPA-index fallback: the host
    // stays the single authority on what is dynamic, so a
    // present-or-future host route can never be shadowed.
    todo!("Phase 3: unknown path tunnels; host index.html returned")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "Phase 3 skeleton (bd-tl2j8js8): join_frontend does not exist yet"]
async fn websocket_survives_fallback_splice() {
    // Spec: matching manifest. Upgrade `/ws` through the guest port and
    // round-trip a few frames (the `tunnel::websocket_frames_survive`
    // shape, but through the frontend's fallback splice: consumed head
    // bytes written verbatim, then copy_bidirectional). Proves the
    // head-peek didn't eat or corrupt the upgrade.
    todo!("Phase 3: /ws upgrade flows through the fallback splice untouched")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "Phase 3 skeleton (bd-tl2j8js8): join_frontend does not exist yet"]
async fn oversize_head_gets_431_and_close() {
    // Spec: send a request head larger than the 64 KiB peek bound.
    // Assert a `431 Request Header Fields Too Large` response and that
    // the connection closes (nothing is tunneled — the head was never
    // fully read, so it cannot be replayed verbatim).
    todo!("Phase 3: oversize head -> 431 + close")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "Phase 3 skeleton (bd-tl2j8js8): join_frontend does not exist yet"]
async fn head_peek_timeout_closes() {
    // Spec: connect and send an incomplete head (no `\r\n\r\n`) within
    // the 5 s peek timeout. Assert the connection closes without any
    // response and without the host logging a request.
    todo!("Phase 3: head-peek timeout -> close")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "Phase 3 skeleton (bd-tl2j8js8): join_frontend does not exist yet"]
async fn head_request_gets_headers_only_with_content_length() {
    // Spec: matching manifest. `HEAD /assets/<real asset>` through the
    // guest port: same status + Content-Type + Content-Length (+ any
    // Content-Encoding / Cache-Control) as the GET, but an empty body.
    // HEAD semantics live in the shared `asset_response` helper.
    todo!("Phase 3: HEAD -> headers only, correct Content-Length")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "Phase 3 skeleton (bd-tl2j8js8): join_frontend does not exist yet"]
async fn editor_ui_boots_from_local_editor_index() {
    // Spec: editor-UI host (the `editor_ui.rs` boot pattern) with a
    // matching editor manifest. GET `/` through the guest port
    // normalizes to an exact `index.html` manifest hit and is served
    // from the guest's *editor* embed (byte-identical to the host's
    // editor index); the host request log shows no asset requests.
    // Pins the post-resolution editor manifest view (design decision
    // 4) end to end.
    todo!("Phase 3: editor-UI guest boots from the locally served editor index")
}
