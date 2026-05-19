//! End-to-end smoke test for the q2-preview server.
//!
//! Phase A.5 (bd-mflk). Exercises the SPA-fallback extension against
//! a stand-in hub router (a single registered route) so we can assert
//! the composition shape — hub routes take priority over the SPA
//! fallback — *without* booting a real `quarto_hub` runtime (samod
//! init, lockfile, periodic sync, file watcher — all costly for a
//! unit-tier test).
//!
//! The deeper integration (real `quarto_hub::server::run_server_with`
//! plus a tempdir-backed storage + SPA fallback) is covered by the
//! manual `q2 preview` smoke (A.5.5) and will move into a Playwright
//! / subprocess test in A.7 (bd-vpsy).

use axum::{Router, routing::get};
use quarto_preview::extend_with_spa;

async fn spawn_with_addr(app: Router) -> std::net::SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind 127.0.0.1:0");
    let addr = listener.local_addr().expect("local_addr");
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    addr
}

#[tokio::test]
async fn spa_root_serves_html_with_react_mount() {
    // No hub routes at all; everything goes through the SPA fallback.
    let app = extend_with_spa(Router::new());
    let addr = spawn_with_addr(app).await;

    let body = reqwest::get(format!("http://{addr}/"))
        .await
        .expect("GET /")
        .error_for_status()
        .expect("200 OK")
        .text()
        .await
        .expect("body bytes are UTF-8");

    assert!(
        body.contains(r#"id="root""#),
        "body missing React mount point (`<div id=\"root\">`):\n{body}",
    );
}

#[tokio::test]
async fn unknown_path_falls_back_to_index_html() {
    // Client-side routing: any unmatched path should serve index.html
    // so the SPA's router can handle it.
    let app = extend_with_spa(Router::new());
    let addr = spawn_with_addr(app).await;

    let resp = reqwest::get(format!("http://{addr}/preview/some-arbitrary-doc-id"))
        .await
        .expect("GET /preview/...");
    let status = resp.status();
    let body = resp.text().await.expect("body");

    assert_eq!(
        status.as_u16(),
        200,
        "fallback should serve 200 OK, got {status}",
    );
    assert!(
        body.contains(r#"id="root""#),
        "fallback body should be the SPA index.html:\n{body}",
    );
}

#[tokio::test]
async fn registered_hub_route_wins_over_spa_fallback() {
    // The crucial property: when this extension is applied on top of
    // a hub-style router, the hub's named routes (here `/health` as a
    // stand-in) take priority over the SPA fallback. If this asserts
    // ever flips, `quarto preview` would silently serve `index.html`
    // for `/api/...` / `/auth/...` / `/ws` requests and break sync.
    let app: Router = Router::new().route("/health", get(|| async { "ok-from-hub" }));
    let app = extend_with_spa(app);
    let addr = spawn_with_addr(app).await;

    let body = reqwest::get(format!("http://{addr}/health"))
        .await
        .expect("GET /health")
        .error_for_status()
        .expect("200 OK")
        .text()
        .await
        .expect("body");

    assert_eq!(
        body, "ok-from-hub",
        "named hub route should win over SPA fallback; got: {body}",
    );
}
