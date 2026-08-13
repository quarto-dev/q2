//! Phase 1 skeletons (bd-ee0qcq3c; plan
//! `claude-notes/plans/2026-08-13-live-share-local-spa-assets.md`):
//! precompressed `.br` serving and cache headers for the embedded-SPA
//! asset path.
//!
//! These tests are **complete and ignored**: they compile against
//! today's public seams (the smoke-test pattern — `extend_with_spa` on
//! a stand-in router, no hub boot) and fail at runtime until Phase 1
//! extends `asset_response`. Unignore to start Phase 1 red.
//!
//! Contracts pinned here:
//!
//! - `Accept-Encoding: br` on a precompressible asset →
//!   `Content-Encoding: br` + `Vary: Accept-Encoding` + the `.br`
//!   sibling bytes; Content-Type unchanged.
//! - No `br` in `Accept-Encoding` → identity bytes, no
//!   `Content-Encoding`. (Identity stays embedded for clients that
//!   don't send `br`.)
//! - The `.br` bytes brotli-decompress to the identity bytes.
//! - Cache headers match the local-prod contract
//!   (`scripts/local-prod-server.mjs`): paths under `/assets/` (Vite
//!   content-hashed) get `public, max-age=31536000, immutable`;
//!   everything else (e.g. `/`) gets `no-cache`.
//!
//! Placeholder trees (fresh clone, no built dist) have no `/assets/*`
//! files to fetch; the tests no-op there, matching the crate's
//! both-tree-states embed-test pattern.

use axum::Router;
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

/// Fetch `/` and extract the first referenced `/assets/<name>-<hash>.<ext>`
/// path (the dist's content-hashed filenames change every build, so the
/// test discovers one instead of hardcoding). `None` on a placeholder
/// tree (nothing to compress there).
async fn discover_asset_path(addr: std::net::SocketAddr) -> Option<String> {
    let index = reqwest::get(format!("http://{addr}/"))
        .await
        .expect("GET /")
        .error_for_status()
        .expect("200 OK")
        .text()
        .await
        .expect("index body");
    extract_asset_path(&index)
}

/// Find the first `assets/<name>-<hash>.<ext>` reference in the index
/// HTML (references look like `./assets/main-D3X1slXQ.js`). Hand-rolled
/// to avoid a regex dev-dep for one scan.
fn extract_asset_path(html: &str) -> Option<String> {
    let start = html.find("assets/")?;
    let rest = &html[start..];
    let end = rest.find(['"', '\''])?;
    let candidate = &rest[..end];
    match candidate.rsplit('.').next() {
        Some("js" | "css" | "wasm") => Some(format!("/{candidate}")),
        _ => None,
    }
}

/// GET `path` with an explicit `Accept-Encoding`; returns (response
/// headers, raw body bytes). reqwest is built without compression
/// features, so it neither sends `Accept-Encoding` on its own nor
/// transparently decompresses — the raw wire behavior is what the
/// assertions see.
async fn get_with_encoding(
    addr: std::net::SocketAddr,
    path: &str,
    accept_encoding: Option<&str>,
) -> (reqwest::header::HeaderMap, Vec<u8>) {
    let client = reqwest::Client::new();
    let mut req = client.get(format!("http://{addr}{path}"));
    if let Some(enc) = accept_encoding {
        req = req.header(reqwest::header::ACCEPT_ENCODING, enc);
    }
    let resp = req.send().await.expect("GET asset");
    assert_eq!(resp.status(), 200, "asset must serve 200");
    let headers = resp.headers().clone();
    let body = resp.bytes().await.expect("body bytes").to_vec();
    (headers, body)
}

#[tokio::test]
#[ignore = "Phase 1 skeleton (bd-ee0qcq3c): precompressed .br serving not implemented yet"]
async fn br_served_when_accepted() {
    let addr = spawn_with_addr(extend_with_spa(Router::new())).await;
    let Some(asset) = discover_asset_path(addr).await else {
        eprintln!("placeholder tree (no built dist); nothing to compress");
        return;
    };

    let (headers, body) = get_with_encoding(addr, &asset, Some("br")).await;
    assert_eq!(
        headers
            .get(reqwest::header::CONTENT_ENCODING)
            .and_then(|v| v.to_str().ok()),
        Some("br"),
        "Accept-Encoding: br must yield Content-Encoding: br; headers: {headers:?}"
    );
    let vary = headers
        .get(reqwest::header::VARY)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert!(
        vary.split(',')
            .any(|t| t.trim().eq_ignore_ascii_case("accept-encoding")),
        "encoded responses must carry Vary: Accept-Encoding; got Vary: {vary:?}"
    );
    // Content-Type describes the *decoded* entity.
    let expected_ct = if asset.ends_with(".js") {
        "application/javascript"
    } else if asset.ends_with(".css") {
        "text/css"
    } else {
        "application/wasm"
    };
    assert_eq!(
        headers
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok()),
        Some(expected_ct),
        "Content-Type must survive content negotiation"
    );
    if let Some(cl) = headers.get(reqwest::header::CONTENT_LENGTH) {
        assert_eq!(
            cl.to_str().unwrap().parse::<usize>().unwrap(),
            body.len(),
            "Content-Length must describe the encoded (transport) body"
        );
    }
}

#[tokio::test]
#[ignore = "Phase 1 skeleton (bd-ee0qcq3c): precompressed .br serving not implemented yet"]
async fn identity_served_without_br_acceptance() {
    let addr = spawn_with_addr(extend_with_spa(Router::new())).await;
    let Some(asset) = discover_asset_path(addr).await else {
        eprintln!("placeholder tree (no built dist); nothing to compress");
        return;
    };

    for accept in [None, Some("gzip")] {
        let (headers, body) = get_with_encoding(addr, &asset, accept).await;
        assert!(
            headers.get(reqwest::header::CONTENT_ENCODING).is_none(),
            "Accept-Encoding {accept:?} must yield identity (no Content-Encoding); headers: {headers:?}"
        );
        assert!(
            !body.is_empty(),
            "identity body must be the embedded asset bytes"
        );
    }
}

#[tokio::test]
#[ignore = "Phase 1 skeleton (bd-ee0qcq3c): precompressed .br serving not implemented yet"]
async fn br_bytes_roundtrip_to_identity() {
    let addr = spawn_with_addr(extend_with_spa(Router::new())).await;
    let Some(asset) = discover_asset_path(addr).await else {
        eprintln!("placeholder tree (no built dist); nothing to compress");
        return;
    };

    let (_, br_body) = get_with_encoding(addr, &asset, Some("br")).await;
    let (_, identity) = get_with_encoding(addr, &asset, None).await;

    assert!(
        br_body.len() < identity.len(),
        "brotli should shrink the asset ({} !< {})",
        br_body.len(),
        identity.len()
    );
    let mut decoded = Vec::new();
    std::io::Read::read_to_end(
        &mut brotli::Decompressor::new(br_body.as_slice(), 4096),
        &mut decoded,
    )
    .expect("br body brotli-decompresses");
    assert_eq!(
        decoded, identity,
        "decompressed .br bytes must equal the identity bytes"
    );
}

#[tokio::test]
#[ignore = "Phase 1 skeleton (bd-ee0qcq3c): cache headers not implemented yet"]
async fn cache_headers_match_local_prod_contract() {
    let addr = spawn_with_addr(extend_with_spa(Router::new())).await;

    // `/` (the SPA index) must always revalidate.
    let index = reqwest::get(format!("http://{addr}/"))
        .await
        .expect("GET /")
        .error_for_status()
        .expect("200 OK");
    let cc = index
        .headers()
        .get(reqwest::header::CACHE_CONTROL)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert_eq!(
        cc, "no-cache",
        "the SPA index must always revalidate (local-prod contract); got Cache-Control: {cc:?}"
    );
    let index_body = index.text().await.expect("index body");

    // Content-hashed `/assets/*` are immutable.
    let Some(asset) = extract_asset_path(&index_body) else {
        eprintln!("placeholder tree (no built dist); no /assets/* to check");
        return;
    };
    let (headers, _) = get_with_encoding(addr, &asset, None).await;
    let cc = headers
        .get(reqwest::header::CACHE_CONTROL)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert_eq!(
        cc, "public, max-age=31536000, immutable",
        "content-hashed assets are immutable (local-prod contract); got Cache-Control: {cc:?}"
    );
}
