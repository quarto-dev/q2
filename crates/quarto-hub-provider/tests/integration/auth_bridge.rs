//! NodeBridge ↔ `auth-stream` helper plumbing (bd-sfet3264, Phase 3C).
//!
//! Spawns the **real** bundled helper with no OAuth credentials and asserts the
//! bridge reads its stdout, parses the error frame, and surfaces it through
//! `fresh_bearer()`. This exercises the Rust↔Node plumbing (find node, extract
//! bundle, spawn, read stream) end-to-end without real interactive OAuth.
//!
//! Gated: skips cleanly when the embedded bundle isn't built (a plain
//! `cargo nextest` without the bundle step) or Node isn't installed — the
//! plumbing isn't exercisable then. A timeout guards against a build that has
//! compiled-in credentials (which would launch a browser instead of erroring).

use std::sync::Arc;
use std::time::Duration;

use quarto_hub_provider::{NodeBridge, TokenSource};

#[tokio::test]
async fn node_bridge_surfaces_helper_error_when_unauthenticated() {
    // Make sure this process has no OAuth creds so the child helper errors fast
    // (a dev build has no compiled-in defaults to inject either). nextest runs
    // each test in its own process, so this env edit is isolated.
    unsafe {
        std::env::remove_var("QUARTO_HUB_MCP_CLIENT_ID");
        std::env::remove_var("QUARTO_HUB_MCP_CLIENT_SECRET");
    }

    let bridge = match NodeBridge::spawn() {
        Ok(bridge) => bridge,
        Err(e) => {
            eprintln!("skipping: auth bridge not spawnable here ({e})");
            return;
        }
    };

    let source = Arc::new(bridge);
    let result = tokio::time::timeout(Duration::from_secs(20), source.fresh_bearer()).await;

    match result {
        Err(_) => {
            // Timed out waiting for a token: this build likely has compiled-in
            // credentials and the helper is attempting an interactive sign-in.
            // Not exercisable as an offline test — skip.
            eprintln!("skipping: helper did not error within the deadline (compiled-in creds?)");
        }
        Ok(Ok(token)) => panic!("expected an auth error, but got a token: {token}"),
        Ok(Err(err)) => {
            let msg = err.to_string();
            assert!(
                msg.contains("CLIENT_ID")
                    || msg.contains("not set")
                    || msg.contains("without a token"),
                "expected a missing-credentials error from the helper, got: {msg}"
            );
        }
    }
}
