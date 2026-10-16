//! Phase 2 of `q2 preview --static` (bd-sl79jjiq; plan
//! `claude-notes/plans/2026-09-22-q2-preview-static.md` § Static server
//! and § Reload channel): the static-file router and the SSE reload
//! channel, driven in-process through `tower::ServiceExt::oneshot`.
//!
//! Contracts pinned here, one test each:
//! - `/` serves `index.html`; without one it redirects to the default
//!   file; without either it lists the rendered HTML outputs.
//! - A directory URL without a trailing slash gets a 301 adding it.
//! - Unknown paths get the site's `404.html` (client injected) or a
//!   plain 404.
//! - Parent traversal (`..`, encoded or not) is refused.
//! - Every response is `Cache-Control: no-store, max-age=0`.
//! - Content types follow the extension.
//! - HTML gets the client script before `</body>`; non-HTML and CORS
//!   fetches (`sec-fetch-mode: cors`) are served raw.
//! - HEAD answers with the GET headers and no body.
//! - `/__q2-preview/events` is an SSE stream that delivers events sent
//!   after subscription and never replays earlier ones.

use std::path::{Path, PathBuf};
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::http::{HeaderMap, Method, Request, StatusCode};
use http_body_util::BodyExt;
use quarto_preview::static_mode::{
    EVENTS_PATH, ReloadEvent, ReloadHub, StaticServerConfig, build_router,
};
use tower::ServiceExt;

const INDEX: &str = "<!DOCTYPE html>\n<html><head><title>Home</title></head>\n<body>\n<p>Welcome home.</p>\n</body>\n</html>\n";
const ABOUT: &str = "<html><body><p>About us.</p></body></html>";
const SUB_INDEX: &str = "<html><body><p>Sub index.</p></body></html>";
const NOT_FOUND_PAGE: &str = "<html><body><p>Custom missing page.</p></body></html>";
const CSS: &str = "body { color: #123456; }\n";

fn write(path: &Path, contents: &[u8]) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, contents).unwrap();
}

/// A rendered-site tree: index, about, a subdirectory with its own
/// index, an image, a stylesheet, and a custom 404 page.
fn full_site(temp: &tempfile::TempDir) -> PathBuf {
    let root = temp.path().canonicalize().unwrap();
    write(&root.join("index.html"), INDEX.as_bytes());
    write(&root.join("about.html"), ABOUT.as_bytes());
    write(&root.join("sub/index.html"), SUB_INDEX.as_bytes());
    write(&root.join("img.png"), b"\x89PNG\r\n\x1a\nnot really");
    write(&root.join("style.css"), CSS.as_bytes());
    write(&root.join("404.html"), NOT_FOUND_PAGE.as_bytes());
    write(
        &root.join("site_libs/quarto-search/manifest.json"),
        b"{\"version\":1}",
    );
    write(
        &root.join("site_libs/quarto-search/pagefind/pagefind-entry.json"),
        b"{\"version\":\"1.5.2\"}",
    );
    root
}

fn router(root: PathBuf, default_file: Option<&str>) -> (Router, ReloadHub) {
    let hub = ReloadHub::new();
    let app = build_router(
        StaticServerConfig {
            root,
            default_file: default_file.map(str::to_string),
            page_requests: None,
            pdfjs: None,
        },
        hub.clone(),
    );
    (app, hub)
}

/// A router that reports every served HTML page on a channel
/// (`q2 preview --static`'s lazy-execution hook, plan Phase 3b).
fn router_with_page_requests(root: PathBuf) -> (Router, tokio::sync::mpsc::Receiver<PathBuf>) {
    let (tx, rx) = tokio::sync::mpsc::channel(16);
    let app = build_router(
        StaticServerConfig {
            root,
            default_file: None,
            page_requests: Some(tx),
            pdfjs: None,
        },
        ReloadHub::new(),
    );
    (app, rx)
}

async fn request(
    app: &Router,
    method: Method,
    path: &str,
    extra: &[(&str, &str)],
) -> (StatusCode, HeaderMap, Vec<u8>) {
    let mut req = Request::builder().method(method).uri(path);
    for (k, v) in extra {
        req = req.header(*k, *v);
    }
    let response = app
        .clone()
        .oneshot(req.body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    (status, headers, body.to_vec())
}

async fn get(app: &Router, path: &str) -> (StatusCode, HeaderMap, Vec<u8>) {
    request(app, Method::GET, path, &[]).await
}

fn header_str<'a>(headers: &'a HeaderMap, name: &str) -> &'a str {
    headers
        .get(name)
        .unwrap_or_else(|| panic!("missing {name} header"))
        .to_str()
        .unwrap()
}

fn text(body: &[u8]) -> String {
    String::from_utf8(body.to_vec()).expect("utf-8 body")
}

#[tokio::test]
async fn root_shows_the_render_failure_when_nothing_was_rendered() {
    let temp = tempfile::TempDir::new().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let (app, hub) = router(root, None);

    hub.set_failure(Some("Error: [Q-1-1] broken <chapter>".to_string()));
    let (status, _, body) = get(&app, "/").await;
    let page = text(&body);
    assert_eq!(status, StatusCode::OK);
    assert!(page.contains("The render failed"), "{page}");
    assert!(page.contains("broken &lt;chapter&gt;"), "escaped: {page}");
    assert!(!page.contains("has no <code>index.html</code>"), "{page}");
    assert!(page.contains(EVENTS_PATH), "client injected: {page}");

    hub.set_failure(None);
    let (_, _, body) = get(&app, "/").await;
    assert!(text(&body).contains("Rendered pages"));
}

#[tokio::test]
async fn root_serves_index_html_with_client_injected_before_body_end() {
    let temp = tempfile::TempDir::new().unwrap();
    let (app, _hub) = router(full_site(&temp), None);
    let (status, headers, body) = get(&app, "/").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        header_str(&headers, "content-type"),
        "text/html; charset=utf-8"
    );
    assert_eq!(header_str(&headers, "cache-control"), "no-store, max-age=0");
    let html = text(&body);
    assert!(
        html.contains("<p>Welcome home.</p>"),
        "original content kept: {html}"
    );
    let script_at = html
        .find("EventSource(\"/__q2-preview/events\")")
        .expect("client script names the SSE endpoint");
    let body_end = html.rfind("</body>").expect("</body> still present");
    assert!(
        script_at < body_end,
        "script must be inserted before </body>: {html}"
    );
    assert_eq!(
        html.matches("<script").count(),
        1,
        "exactly one injected script"
    );
}

#[tokio::test]
async fn root_without_index_redirects_to_the_default_file() {
    let temp = tempfile::TempDir::new().unwrap();
    let root = temp.path().canonicalize().unwrap();
    write(&root.join("doc.html"), ABOUT.as_bytes());
    let (app, _hub) = router(root, Some("doc.html"));
    let (status, headers, _) = get(&app, "/").await;
    assert_eq!(status, StatusCode::FOUND);
    assert_eq!(header_str(&headers, "location"), "/doc.html");
    assert_eq!(header_str(&headers, "cache-control"), "no-store, max-age=0");
}

#[tokio::test]
async fn root_without_index_or_default_lists_the_rendered_pages() {
    let temp = tempfile::TempDir::new().unwrap();
    let root = temp.path().canonicalize().unwrap();
    write(&root.join("about.html"), ABOUT.as_bytes());
    write(&root.join("posts/one.html"), ABOUT.as_bytes());
    write(&root.join("style.css"), CSS.as_bytes());
    let (app, _hub) = router(root, None);
    let (status, headers, body) = get(&app, "/").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        header_str(&headers, "content-type"),
        "text/html; charset=utf-8"
    );
    let html = text(&body);
    assert!(
        html.contains("href=\"/about.html\""),
        "lists top-level page: {html}"
    );
    assert!(
        html.contains("href=\"/posts/one.html\""),
        "lists nested page: {html}"
    );
    assert!(
        !html.contains("style.css"),
        "non-HTML files are not listed: {html}"
    );
    assert!(
        html.contains("EventSource(\"/__q2-preview/events\")"),
        "listing is live too"
    );
}

#[tokio::test]
async fn directory_without_trailing_slash_redirects_then_serves_its_index() {
    let temp = tempfile::TempDir::new().unwrap();
    let (app, _hub) = router(full_site(&temp), None);
    let (status, headers, _) = get(&app, "/sub").await;
    assert_eq!(status, StatusCode::MOVED_PERMANENTLY);
    assert_eq!(header_str(&headers, "location"), "/sub/");

    let (status, _, body) = get(&app, "/sub/").await;
    assert_eq!(status, StatusCode::OK);
    assert!(text(&body).contains("<p>Sub index.</p>"));
}

#[tokio::test]
async fn unknown_path_serves_the_custom_404_page_with_client() {
    let temp = tempfile::TempDir::new().unwrap();
    let (app, _hub) = router(full_site(&temp), None);
    let (status, headers, body) = get(&app, "/nope.html").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(
        header_str(&headers, "content-type"),
        "text/html; charset=utf-8"
    );
    let html = text(&body);
    assert!(html.contains("<p>Custom missing page.</p>"), "{html}");
    assert!(
        html.contains("EventSource("),
        "404 page is live too: {html}"
    );
}

#[tokio::test]
async fn unknown_path_without_a_404_page_is_a_plain_404() {
    let temp = tempfile::TempDir::new().unwrap();
    let root = temp.path().canonicalize().unwrap();
    write(&root.join("index.html"), INDEX.as_bytes());
    let (app, _hub) = router(root, None);
    let (status, headers, body) = get(&app, "/nope.html").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(header_str(&headers, "cache-control"), "no-store, max-age=0");
    assert_eq!(text(&body), "Not Found");
}

#[tokio::test]
async fn parent_traversal_is_refused() {
    let temp = tempfile::TempDir::new().unwrap();
    let root = full_site(&temp);
    // A real file one level above the served root.
    write(&root.parent().unwrap().join("secret.txt"), b"top secret");
    let (app, _hub) = router(root, None);
    for path in [
        "/../secret.txt",
        "/%2e%2e/secret.txt",
        "/sub/../../secret.txt",
    ] {
        let (status, _, body) = get(&app, path).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{path}");
        assert!(
            !text(&body).contains("top secret"),
            "{path} leaked the file"
        );
    }
}

#[tokio::test]
async fn content_types_follow_the_extension_and_non_html_is_served_raw() {
    let temp = tempfile::TempDir::new().unwrap();
    let (app, _hub) = router(full_site(&temp), None);
    let (status, headers, body) = get(&app, "/img.png").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(header_str(&headers, "content-type"), "image/png");
    assert_eq!(body, b"\x89PNG\r\n\x1a\nnot really");

    let (status, headers, body) = get(&app, "/style.css").await;
    assert_eq!(status, StatusCode::OK);
    assert!(header_str(&headers, "content-type").starts_with("text/css"));
    assert_eq!(text(&body), CSS, "no injection into non-HTML");
}

#[tokio::test]
async fn cors_fetch_of_html_is_not_injected() {
    let temp = tempfile::TempDir::new().unwrap();
    let (app, _hub) = router(full_site(&temp), None);
    let (status, _, body) = request(
        &app,
        Method::GET,
        "/about.html",
        &[("sec-fetch-mode", "cors")],
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        text(&body),
        ABOUT,
        "a fetch() of a page must get the raw bytes"
    );
}

#[tokio::test]
async fn head_returns_the_get_headers_without_a_body() {
    let temp = tempfile::TempDir::new().unwrap();
    let (app, _hub) = router(full_site(&temp), None);
    let (_, get_headers, get_body) = get(&app, "/index.html").await;
    let (status, headers, body) = request(&app, Method::HEAD, "/index.html", &[]).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.is_empty(), "HEAD carries no body");
    assert_eq!(
        header_str(&headers, "content-type"),
        header_str(&get_headers, "content-type")
    );
    assert_eq!(
        header_str(&headers, "content-length")
            .parse::<usize>()
            .unwrap(),
        get_body.len(),
        "HEAD reports the injected GET body's length"
    );
}

#[tokio::test]
async fn other_methods_are_not_allowed() {
    let temp = tempfile::TempDir::new().unwrap();
    let (app, _hub) = router(full_site(&temp), None);
    let (status, _, _) = request(&app, Method::POST, "/index.html", &[]).await;
    assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);
}

#[tokio::test]
async fn reserved_prefix_shadows_files_in_the_output_dir() {
    let temp = tempfile::TempDir::new().unwrap();
    let root = full_site(&temp);
    write(&root.join("__q2-preview/events"), b"a file, not a stream");
    let (app, _hub) = router(root, None);
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/__q2-preview/events")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        header_str(response.headers(), "content-type").starts_with("text/event-stream"),
        "reserved path is the SSE endpoint, not the file"
    );
}

/// Read the next SSE frame (as text) from a streaming body, or panic
/// after `timeout`.
async fn next_frame(body: &mut Body, timeout: Duration) -> String {
    let frame = tokio::time::timeout(timeout, body.frame())
        .await
        .expect("frame within timeout")
        .expect("stream still open")
        .expect("frame ok");
    let data = frame.into_data().expect("data frame");
    String::from_utf8(data.to_vec()).unwrap()
}

#[tokio::test]
async fn sse_delivers_events_sent_after_subscription() {
    let temp = tempfile::TempDir::new().unwrap();
    let (app, hub) = router(full_site(&temp), None);
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/__q2-preview/events")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        header_str(response.headers(), "cache-control"),
        "no-cache",
        "SSE responses use axum's no-cache, never a browser-cacheable value"
    );
    let mut body = response.into_body();

    assert_eq!(hub.send(ReloadEvent::RenderStart), 1, "one subscriber");
    let frame = next_frame(&mut body, Duration::from_secs(5)).await;
    assert!(frame.starts_with("event: render-start\n"), "{frame:?}");
    assert!(
        frame.contains("data: {\"type\":\"render-start\"}"),
        "{frame:?}"
    );

    hub.send(ReloadEvent::RenderStop {
        ok: false,
        errors: 2,
        warnings: 1,
        text: "Error: boom\nline two".to_string(),
    });
    let frame = next_frame(&mut body, Duration::from_secs(5)).await;
    assert!(frame.starts_with("event: render-stop\n"), "{frame:?}");
    assert!(
        frame.contains(
            "data: {\"type\":\"render-stop\",\"ok\":false,\"errors\":2,\"warnings\":1,\"text\":\"Error: boom\\nline two\"}"
        ),
        "multi-line text travels JSON-escaped in one data line: {frame:?}"
    );

    hub.send(ReloadEvent::Reload {
        target: Some("/about.html".to_string()),
    });
    let frame = next_frame(&mut body, Duration::from_secs(5)).await;
    assert!(frame.starts_with("event: reload\n"), "{frame:?}");
    assert!(
        frame.contains("data: {\"type\":\"reload\",\"target\":\"/about.html\"}"),
        "{frame:?}"
    );
}

#[tokio::test]
async fn sse_never_replays_events_sent_before_subscription() {
    let temp = tempfile::TempDir::new().unwrap();
    let (app, hub) = router(full_site(&temp), None);
    assert_eq!(
        hub.send(ReloadEvent::RenderStart),
        0,
        "nobody listening yet"
    );

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/__q2-preview/events")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let mut body = response.into_body();
    hub.send(ReloadEvent::Reload { target: None });
    let frame = next_frame(&mut body, Duration::from_secs(5)).await;
    assert!(
        frame.starts_with("event: reload\n"),
        "first frame is the post-subscription event, not the earlier one: {frame:?}"
    );
    assert!(
        frame.contains("data: {\"type\":\"reload\",\"target\":null}"),
        "{frame:?}"
    );
}

#[tokio::test]
async fn sse_streams_end_when_the_hub_shuts_down() {
    let temp = tempfile::TempDir::new().unwrap();
    let (app, hub) = router(full_site(&temp), None);
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/__q2-preview/events")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let mut body = response.into_body();
    hub.shutdown();
    let end = tokio::time::timeout(Duration::from_secs(5), body.frame())
        .await
        .expect("stream ends within timeout");
    assert!(
        end.is_none(),
        "stream must end (no more frames) after shutdown"
    );
}

#[tokio::test]
async fn served_html_pages_are_reported_on_the_page_request_channel() {
    let temp = tempfile::TempDir::new().unwrap();
    let root = full_site(&temp);
    let (app, mut rx) = router_with_page_requests(root.clone());

    assert_eq!(get(&app, "/about.html").await.0, StatusCode::OK);
    assert_eq!(rx.try_recv().unwrap(), root.join("about.html"));

    assert_eq!(get(&app, "/sub/").await.0, StatusCode::OK);
    assert_eq!(rx.try_recv().unwrap(), root.join("sub").join("index.html"));

    assert_eq!(get(&app, "/").await.0, StatusCode::OK);
    assert_eq!(rx.try_recv().unwrap(), root.join("index.html"));
    assert!(rx.try_recv().is_err(), "one report per page view");
}

#[tokio::test]
async fn non_page_views_are_not_reported() {
    let temp = tempfile::TempDir::new().unwrap();
    let root = full_site(&temp);
    let (app, mut rx) = router_with_page_requests(root);

    assert_eq!(get(&app, "/style.css").await.0, StatusCode::OK);
    assert_eq!(get(&app, "/nope.html").await.0, StatusCode::NOT_FOUND);
    assert_eq!(
        request(&app, Method::HEAD, "/about.html", &[]).await.0,
        StatusCode::OK
    );
    assert_eq!(
        request(
            &app,
            Method::GET,
            "/about.html",
            &[("sec-fetch-mode", "cors")]
        )
        .await
        .0,
        StatusCode::OK
    );
    assert!(
        rx.try_recv().is_err(),
        "a stylesheet, a 404, a HEAD, and a fetch() are not page views"
    );
}

/// The website search bundle is readable cross-origin, so a federated
/// parent site previewed on another port can merge this site's index;
/// nothing else is (bd-eip0naf4).
#[tokio::test]
async fn search_bundle_is_served_with_open_cors_and_nothing_else_is() {
    let temp = tempfile::TempDir::new().unwrap();
    let (app, _hub) = router(full_site(&temp), None);
    for path in [
        "/site_libs/quarto-search/manifest.json",
        "/site_libs/quarto-search/pagefind/pagefind-entry.json",
    ] {
        let (status, headers, _body) = get(&app, path).await;
        assert_eq!(status, StatusCode::OK, "{path}");
        assert_eq!(
            header_str(&headers, "access-control-allow-origin"),
            "*",
            "{path}"
        );
        assert_eq!(header_str(&headers, "cache-control"), "no-store, max-age=0");
    }
    for path in ["/about.html", "/style.css", "/img.png"] {
        let (_status, headers, _body) = get(&app, path).await;
        assert!(
            headers.get("access-control-allow-origin").is_none(),
            "{path} must not be CORS-open"
        );
    }
}

// ── pdf.js viewer for PDF output (hub-parity Plan 8) ─────────────────

const NAVIGATE: [(&str, &str); 2] = [
    ("sec-fetch-mode", "navigate"),
    ("sec-fetch-dest", "document"),
];
const PDF_BYTES: &[u8] = b"%PDF-1.7\nnot really a pdf";
const VIEWER_HTML: &str = "<html><head><meta http-equiv=\"Content-Security-Policy\" content=\"script-src 'self'\"></head><body><div id=\"viewer\"></div></body></html>";

/// A tiny in-memory pdf.js tree: viewer page, a module, and the en-US locale.
fn fake_pdfjs() -> quarto_preview::PdfjsTree {
    std::sync::Arc::new(|rel: &str| match rel {
        "pdfjs/web/viewer.html" => Some(VIEWER_HTML.as_bytes().to_vec()),
        "pdfjs/build/pdf.mjs" => Some(b"export {};".to_vec()),
        "pdfjs/web/locale/en-US/viewer.ftl" => Some(b"en = 1".to_vec()),
        _ => None,
    })
}

fn pdf_router(
    default_file: Option<&str>,
    pdfjs: Option<quarto_preview::PdfjsTree>,
) -> (
    Router,
    tokio::sync::mpsc::Receiver<PathBuf>,
    PathBuf,
    tempfile::TempDir,
) {
    let temp = tempfile::TempDir::new().unwrap();
    let root = temp.path().canonicalize().unwrap();
    write(&root.join("my doc.pdf"), PDF_BYTES);
    write(&root.join("about.html"), ABOUT.as_bytes());
    let (tx, rx) = tokio::sync::mpsc::channel(16);
    let app = build_router(
        StaticServerConfig {
            root: root.clone(),
            default_file: default_file.map(str::to_string),
            page_requests: Some(tx),
            pdfjs,
        },
        ReloadHub::new(),
    );
    (app, rx, root, temp)
}

#[tokio::test]
async fn pdf_navigation_redirects_to_the_viewer_without_a_hash() {
    let (app, mut rx, root, _t) = pdf_router(None, Some(fake_pdfjs()));
    let (status, headers, _) = request(&app, Method::GET, "/my%20doc.pdf", &NAVIGATE).await;
    assert_eq!(status, StatusCode::FOUND);
    let location = header_str(&headers, "location");
    assert_eq!(
        location,
        "/__q2-preview/pdfjs/web/viewer.html?file=/my%20doc.pdf"
    );
    assert!(!location.contains('#'), "a hash resets page and zoom");
    assert_eq!(
        rx.try_recv().unwrap(),
        root.join("my doc.pdf"),
        "the redirect is the page view (the viewer's own fetch is cors and skipped)"
    );
}

#[tokio::test]
async fn pdf_bytes_stay_raw_for_the_viewers_fetch_and_for_non_navigations() {
    let (app, mut rx, _root, _t) = pdf_router(None, Some(fake_pdfjs()));
    for extra in [
        vec![("sec-fetch-mode", "cors"), ("sec-fetch-dest", "empty")],
        vec![],
    ] {
        let (status, headers, body) = request(&app, Method::GET, "/my%20doc.pdf", &extra).await;
        assert_eq!(status, StatusCode::OK, "{extra:?}");
        assert_eq!(header_str(&headers, "content-type"), "application/pdf");
        assert_eq!(body, PDF_BYTES);
    }
    // The cors fetch is not a view; the bare GET (curl) is.
    assert!(rx.try_recv().is_ok());
    assert!(rx.try_recv().is_err());
}

#[tokio::test]
async fn without_a_pdfjs_tree_a_pdf_navigation_is_served_raw() {
    let (app, _rx, _root, _t) = pdf_router(None, None);
    let (status, headers, body) = request(&app, Method::GET, "/my%20doc.pdf", &NAVIGATE).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(header_str(&headers, "content-type"), "application/pdf");
    assert_eq!(body, PDF_BYTES);
}

#[tokio::test]
async fn root_of_a_pdf_only_output_goes_straight_to_the_viewer() {
    let (app, mut rx, root, _t) = pdf_router(Some("my doc.pdf"), Some(fake_pdfjs()));
    std::fs::remove_file(root.join("about.html")).unwrap();
    let (status, headers, _) = get(&app, "/").await;
    assert_eq!(status, StatusCode::FOUND);
    assert_eq!(
        header_str(&headers, "location"),
        "/__q2-preview/pdfjs/web/viewer.html?file=/my%20doc.pdf"
    );
    assert_eq!(rx.try_recv().unwrap(), root.join("my doc.pdf"));
}

#[tokio::test]
async fn the_pdfjs_tree_is_served_with_types_cache_and_locale_alias() {
    let (app, _rx, _root, _t) = pdf_router(None, Some(fake_pdfjs()));
    let (status, headers, body) = get(&app, "/__q2-preview/pdfjs/build/pdf.mjs").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        header_str(&headers, "content-type"),
        "application/javascript"
    );
    assert!(
        !header_str(&headers, "cache-control").contains("no-store"),
        "the tree only changes with the binary"
    );
    assert_eq!(body, b"export {};");

    let (status, _, body) = get(&app, "/__q2-preview/pdfjs/web/locale/de/viewer.ftl").await;
    assert_eq!(
        status,
        StatusCode::OK,
        "a pruned locale is answered with en-US"
    );
    assert_eq!(body, b"en = 1");

    let (status, _, _) = get(&app, "/__q2-preview/pdfjs/build/missing.mjs").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn the_viewer_page_gets_the_external_reload_client() {
    let (app, _rx, _root, _t) = pdf_router(None, Some(fake_pdfjs()));
    let (status, headers, body) = get(&app, "/__q2-preview/pdfjs/web/viewer.html").await;
    assert_eq!(status, StatusCode::OK);
    assert!(header_str(&headers, "content-type").starts_with("text/html"));
    let html = text(&body);
    let tag = "<script src=\"/__q2-preview/client.js\"></script>";
    assert!(html.find(tag).unwrap() < html.find("</body>").unwrap());
    assert!(
        !html.contains("EventSource"),
        "no inline copy: the viewer's CSP would block it"
    );

    let (status, headers, js) = get(&app, "/__q2-preview/client.js").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        header_str(&headers, "content-type"),
        "application/javascript"
    );
    let js = text(&js);
    assert!(js.contains("/__q2-preview/events"));
    assert!(
        !js.contains("setAttribute(\"style\""),
        "the viewer's style-src blocks setAttribute(\"style\"); use the CSSOM"
    );
    assert!(js.contains("/__q2-preview/pdfjs/web/viewer.html"));
}

#[tokio::test]
async fn pdfjs_paths_traversal_is_refused() {
    let (app, _rx, _root, _t) = pdf_router(None, Some(fake_pdfjs()));
    let (status, _, _) = get(&app, "/__q2-preview/pdfjs/../about.html").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _, _) = get(&app, "/__q2-preview/pdfjs/%2e%2e/about.html").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}
