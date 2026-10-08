//! The built `q2` serves the bundled pdf.js viewer at `/pdfjs/` (hub-parity Plan 4, P4-D1):
//! module workers need a JavaScript MIME type, a pruned locale is answered with the en-US file,
//! and a missing `/pdfjs/...` path is a 404 rather than the SPA's `index.html`.
//!
//! Needs a real SPA dist (the SPA e2e job builds one; `test-suite.yml` embeds the placeholder),
//! so it skips with a message on the placeholder tree. The path decision itself is unit-tested
//! in `quarto-preview/src/lib.rs` over an in-memory tree.

use axum::Router;
use quarto_preview::extend_with_spa;

async fn spawn(app: Router) -> std::net::SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind 127.0.0.1:0");
    let addr = listener.local_addr().expect("local_addr");
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    addr
}

async fn get(addr: std::net::SocketAddr, path: &str) -> reqwest::Response {
    reqwest::get(format!("http://{addr}{path}"))
        .await
        .unwrap_or_else(|e| panic!("GET {path}: {e}"))
}

/// A real dist references content-hashed files under `assets/`; the placeholder has none.
async fn has_real_dist(addr: std::net::SocketAddr) -> bool {
    get(addr, "/")
        .await
        .text()
        .await
        .expect("index body")
        .contains("assets/")
}

fn content_type(resp: &reqwest::Response) -> String {
    resp.headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string()
}

#[tokio::test]
async fn serves_the_viewer_tree_with_module_mime_types() {
    let addr = spawn(extend_with_spa(Router::new())).await;
    if !has_real_dist(addr).await {
        eprintln!("skipping: placeholder SPA embed (no built dist, so no pdfjs tree)");
        return;
    }
    let viewer = get(addr, "/pdfjs/web/viewer.html").await;
    assert_eq!(viewer.status(), 200);
    assert!(content_type(&viewer).starts_with("text/html"));
    for module in ["pdf.mjs", "pdf.worker.mjs"] {
        let resp = get(addr, &format!("/pdfjs/build/{module}")).await;
        assert_eq!(resp.status(), 200, "{module}");
        assert_eq!(content_type(&resp), "application/javascript", "{module}");
    }
    let viewer_mjs = get(addr, "/pdfjs/web/viewer.mjs").await;
    assert_eq!(viewer_mjs.status(), 200);
    assert_eq!(content_type(&viewer_mjs), "application/javascript");
}

#[tokio::test]
async fn pruned_locales_get_the_en_us_file_and_other_misses_404() {
    let addr = spawn(extend_with_spa(Router::new())).await;
    if !has_real_dist(addr).await {
        eprintln!("skipping: placeholder SPA embed (no built dist, so no pdfjs tree)");
        return;
    }
    let en = get(addr, "/pdfjs/web/locale/en-US/viewer.ftl").await;
    assert_eq!(en.status(), 200);
    let en_body = en.bytes().await.expect("en-US body");
    assert!(!en_body.is_empty());

    let de = get(addr, "/pdfjs/web/locale/de-DE/viewer.ftl").await;
    assert_eq!(de.status(), 200, "a pruned locale is answered, not 404");
    assert_eq!(de.bytes().await.expect("de-DE body"), en_body);

    for missing in [
        "/pdfjs/nope.js",
        "/pdfjs/web/locale/de-DE/other.ftl",
        "/pdfjs/",
    ] {
        assert_eq!(get(addr, missing).await.status(), 404, "{missing}");
    }
}
