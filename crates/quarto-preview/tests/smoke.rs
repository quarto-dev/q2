//! End-to-end smoke test for the q2-preview server.
//!
//! Phase A.2 (bd-yxqt). Binds the router on an OS-assigned port, hits
//! `GET /` over HTTP, asserts the response is `200 OK` and the body
//! contains `<div id="root">`. That last marker comes from *both* the
//! real SPA bundle (`q2-preview-spa/index.html` has the mount point)
//! and the build.rs placeholder — so the test passes whether or not
//! the SPA has been built. The point isn't to test the SPA's
//! content; it's to confirm routing + embedding + lifecycle hang
//! together in one piece.
//!
//! Subsequent phases:
//!   - A.5 (bd-mflk) layers the hub server's router on top; its
//!     boot.rs test asserts the websocket + tempdir lifecycle.

use quarto_preview::{PreviewConfig, router};

#[tokio::test]
async fn spa_root_serves_html_with_react_mount() {
    let config = PreviewConfig {
        host: "127.0.0.1".to_string(),
        port: 0,
        spa_dir_override: None,
    };

    // Bind manually so we get the assigned port back from the OS;
    // calling `run()` directly would block on the serve loop with no
    // way to recover the address.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind 127.0.0.1:0");
    let addr = listener.local_addr().expect("local_addr");

    let app = router(&config);
    let server = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });

    let body = reqwest::get(format!("http://{addr}/"))
        .await
        .expect("GET /")
        .error_for_status()
        .expect("200 OK")
        .text()
        .await
        .expect("body bytes are UTF-8");

    server.abort();

    assert!(
        body.contains(r#"id="root""#),
        "body missing React mount point (`<div id=\"root\">` or equivalent):\n{body}",
    );
}

#[tokio::test]
async fn unknown_path_falls_back_to_index_html() {
    // Client-side routing: any unmatched path should serve index.html
    // so the SPA's router can handle it. This is the same fallback
    // hub-client's `q2-debug.html` / `q2-preview.html` rely on.
    let config = PreviewConfig {
        host: "127.0.0.1".to_string(),
        port: 0,
        spa_dir_override: None,
    };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = router(&config);
    let server = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });

    let resp = reqwest::get(format!("http://{addr}/preview/some-arbitrary-doc-id"))
        .await
        .expect("GET /preview/...");
    let status = resp.status();
    let body = resp.text().await.expect("body");

    server.abort();

    assert_eq!(
        status.as_u16(),
        200,
        "fallback should serve 200 OK, got {status}"
    );
    assert!(
        body.contains(r#"id="root""#),
        "fallback body should be the SPA index.html:\n{body}",
    );
}
