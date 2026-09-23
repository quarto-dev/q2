//! The static-file router for `q2 preview --static` (bd-sl79jjiq, plan
//! § Static server): serves a rendered output directory the way
//! Quarto 1's preview server does, with the reload client spliced into
//! every HTML page and the SSE endpoint mounted under a reserved
//! prefix.
//!
//! Behaviour (each line has a test in
//! `tests/integration/static_mode.rs`):
//! - `/` serves `index.html`; without one, redirects to the configured
//!   default file; without either, lists the rendered HTML pages.
//! - A directory URL without a trailing slash 301s to add it, then
//!   serves the directory's `index.html`.
//! - Unknown paths get the site's own `404.html` (client injected) or
//!   a plain 404.
//! - Any `..` segment, decoded or not, is refused with a 404. No
//!   canonicalization: symlinks inside the output tree are followed
//!   like any static server would, and the server is loopback-bound.
//! - Every response is `Cache-Control: no-store, max-age=0`.
//! - Content types come from the extension (`mime_guess`).
//! - `text/html` bodies get the client before the last `</body>`
//!   (appended if there is none) — unless the request is a `fetch()`
//!   (`sec-fetch-mode: cors`), whose caller wants the raw bytes.
//! - HEAD gets the GET headers, including the injected length, and no
//!   body. Other methods are 405.
//! - Every HTML page actually *viewed* (a 200 GET that is not a CORS
//!   fetch) is reported on `StaticServerConfig::page_requests`, which
//!   is how lazy code execution learns what the user is looking at
//!   (plan Phase 3b).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::{HeaderValue, Method, StatusCode, header};
use axum::response::sse::{KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use percent_encoding::{AsciiSet, CONTROLS, percent_decode_str, utf8_percent_encode};
use tokio_stream::StreamExt;
use tokio_stream::wrappers::errors::BroadcastStreamRecvError;
use tokio_stream::wrappers::{BroadcastStream, WatchStream};

use super::reload::ReloadHub;

/// Path of the server-sent-events endpoint. The `/__q2-preview/`
/// prefix is reserved: a file at that path in the output directory is
/// shadowed.
pub const EVENTS_PATH: &str = "/__q2-preview/events";

/// The injected browser script. Embedded inline in a `<script>` tag,
/// so it must never contain the closing tag (pinned by a test).
const CLIENT_JS: &str = include_str!("client.js");

const NO_STORE: HeaderValue = HeaderValue::from_static("no-store, max-age=0");
const HTML_UTF8: HeaderValue = HeaderValue::from_static("text/html; charset=utf-8");

/// Characters to percent-encode when a filename becomes a URL path
/// segment: controls, space, and the delimiters that would change the
/// URL's meaning. Dots and most punctuation stay readable.
const PATH_SEGMENT: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'#')
    .add(b'%')
    .add(b'<')
    .add(b'>')
    .add(b'?')
    .add(b'[')
    .add(b']')
    .add(b'`')
    .add(b'{')
    .add(b'}');

/// What to serve.
#[derive(Debug, Clone)]
pub struct StaticServerConfig {
    /// The rendered output directory (`RenderReport::output_dir`).
    pub root: PathBuf,
    /// Where `/` goes when there is no `index.html`: the output of the
    /// initial page (project mode) or of the document (single-file
    /// mode), relative to `root` with forward slashes.
    pub default_file: Option<String>,
    /// Receives the absolute path of every HTML file served as a page
    /// view. `None` when nobody needs to know. Sends never block: a
    /// full channel drops the report (the next view sends again).
    pub page_requests: Option<tokio::sync::mpsc::Sender<PathBuf>>,
}

#[derive(Clone)]
struct StaticState {
    root: Arc<Path>,
    default_file: Option<Arc<str>>,
    hub: ReloadHub,
    page_requests: Option<tokio::sync::mpsc::Sender<PathBuf>>,
}

impl StaticState {
    /// Report a page view: an HTML file about to be served with 200 to
    /// a navigation (not HEAD, not a `fetch()`).
    fn note_page_view(&self, path: &Path, is_head: bool, is_cors_fetch: bool) {
        if is_head || is_cors_fetch {
            return;
        }
        if !path.extension().is_some_and(|e| e == "html") {
            return;
        }
        if let Some(tx) = &self.page_requests {
            let _ = tx.try_send(path.to_path_buf());
        }
    }
}

/// Build the router: the SSE endpoint plus a fallback that serves
/// `config.root`.
pub fn build_router(config: StaticServerConfig, hub: ReloadHub) -> Router {
    let state = StaticState {
        root: Arc::from(config.root),
        default_file: config.default_file.map(Into::into),
        hub,
        page_requests: config.page_requests,
    };
    Router::new()
        .route(EVENTS_PATH, get(events))
        .fallback(serve_path)
        .with_state(state)
}

/// `GET /__q2-preview/events`: one SSE stream per open page. A lagged
/// subscriber (more than the channel capacity behind) skips what it
/// missed; the next event it does see is still a reload.
async fn events(State(state): State<StaticState>) -> impl IntoResponse {
    // The event stream and the hub's closing flag are merged so the
    // stream ends the moment `ReloadHub::shutdown` runs, even with no
    // event in flight.
    enum Item {
        Event(Result<super::reload::ReloadEvent, BroadcastStreamRecvError>),
        Closing(bool),
    }
    let events = BroadcastStream::new(state.hub.subscribe()).map(Item::Event);
    let closing: WatchStream<bool> = state.hub.closing_stream();
    let stream = events
        .merge(closing.map(Item::Closing))
        .take_while(|item| !matches!(item, Item::Closing(true)))
        .filter_map(|item| match item {
            Item::Event(Ok(event)) => Some(Ok::<_, std::convert::Infallible>(event.to_sse())),
            // A lagged subscriber skips what it missed; the next event
            // it does see is still a reload.
            Item::Event(Err(_)) | Item::Closing(_) => None,
        });
    Sse::new(stream).keep_alive(KeepAlive::default())
}

/// Serve `router` on `listener` until `shutdown` resolves, then finish
/// gracefully. Callers must also call [`ReloadHub::shutdown`] so the
/// open SSE streams end; otherwise the graceful drain waits on them.
pub async fn serve(
    router: Router,
    listener: tokio::net::TcpListener,
    shutdown: impl std::future::Future<Output = ()> + Send + 'static,
) -> std::io::Result<()> {
    axum::serve(listener, router)
        .with_graceful_shutdown(shutdown)
        .await
}

async fn serve_path(State(state): State<StaticState>, req: Request) -> Response {
    let is_head = match *req.method() {
        Method::GET => false,
        Method::HEAD => true,
        _ => return StatusCode::METHOD_NOT_ALLOWED.into_response(),
    };
    let raw_path = req.uri().path().to_string();
    let is_cors_fetch = req
        .headers()
        .get("sec-fetch-mode")
        .is_some_and(|v| v.as_bytes() == b"cors");

    let Some(segments) = decode_segments(&raw_path) else {
        return not_found(&state, is_head).await;
    };
    if segments.is_empty() {
        return serve_root(&state, is_head).await;
    }
    let full = segments
        .iter()
        .fold(state.root.to_path_buf(), |p, s| p.join(s));

    match tokio::fs::metadata(&full).await {
        Ok(meta) if meta.is_dir() => {
            if !raw_path.ends_with('/') {
                return redirect(StatusCode::MOVED_PERMANENTLY, &format!("{raw_path}/"));
            }
            let index = full.join("index.html");
            match tokio::fs::read(&index).await {
                Ok(bytes) => {
                    state.note_page_view(&index, is_head, is_cors_fetch);
                    file_response(&index, bytes, StatusCode::OK, is_cors_fetch, is_head)
                }
                Err(_) => not_found(&state, is_head).await,
            }
        }
        Ok(meta) if meta.is_file() => match tokio::fs::read(&full).await {
            Ok(bytes) => {
                state.note_page_view(&full, is_head, is_cors_fetch);
                file_response(&full, bytes, StatusCode::OK, is_cors_fetch, is_head)
            }
            Err(_) => not_found(&state, is_head).await,
        },
        _ => not_found(&state, is_head).await,
    }
}

/// Percent-decode a request path into its non-empty segments, or
/// `None` when it must be refused: undecodable, a `..` segment (any
/// spelling, since decoding happened first), a backslash (a separator
/// on Windows), or a NUL.
fn decode_segments(raw_path: &str) -> Option<Vec<String>> {
    let decoded = percent_decode_str(raw_path).decode_utf8().ok()?;
    let mut segments = Vec::new();
    for segment in decoded.split('/') {
        if segment.is_empty() || segment == "." {
            continue;
        }
        if segment == ".." || segment.contains('\\') || segment.contains('\0') {
            return None;
        }
        segments.push(segment.to_string());
    }
    Some(segments)
}

/// `/`: `index.html`, else a redirect to the default file, else a
/// listing of the rendered pages.
async fn serve_root(state: &StaticState, is_head: bool) -> Response {
    let index = state.root.join("index.html");
    if let Ok(bytes) = tokio::fs::read(&index).await {
        state.note_page_view(&index, is_head, false);
        return file_response(&index, bytes, StatusCode::OK, false, is_head);
    }
    if let Some(default_file) = &state.default_file
        && tokio::fs::metadata(state.root.join(default_file.as_ref()))
            .await
            .is_ok_and(|m| m.is_file())
    {
        let location = format!("/{}", encode_path(default_file));
        return redirect(StatusCode::FOUND, &location);
    }
    let pages = list_html_pages(&state.root);
    let html = render_listing(&pages);
    html_response(inject_client(html.into_bytes()), StatusCode::OK, is_head)
}

/// Every `.html` under `root` except `404.html`, as `/`-separated
/// paths relative to `root`, sorted. Generated and hidden directories
/// are skipped; the walk stops after a generous cap so a huge site
/// still answers promptly.
fn list_html_pages(root: &Path) -> Vec<String> {
    const CAP: usize = 5000;
    let mut pages = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            if pages.len() >= CAP {
                break;
            }
            let path = entry.path();
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if path.is_dir() {
                if name.starts_with('.') || name == "site_libs" || name.ends_with("_files") {
                    continue;
                }
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "html")
                && name != "404.html"
                && let Ok(rel) = path.strip_prefix(root)
            {
                pages.push(
                    rel.components()
                        .map(|c| c.as_os_str().to_string_lossy().into_owned())
                        .collect::<Vec<_>>()
                        .join("/"),
                );
            }
        }
    }
    pages.sort();
    pages
}

fn render_listing(pages: &[String]) -> String {
    let mut html = String::from(
        "<!DOCTYPE html>\n<html><head><meta charset=\"utf-8\">\
         <title>q2 preview</title></head>\n<body>\n\
         <h1>Rendered pages</h1>\n<p>This site has no <code>index.html</code>; \
         pick a page.</p>\n<ul>\n",
    );
    for page in pages {
        let href = format!("/{}", encode_path(page));
        html.push_str(&format!(
            "<li><a href=\"{}\">{}</a></li>\n",
            escape_html(&href),
            escape_html(page)
        ));
    }
    html.push_str("</ul>\n</body>\n</html>\n");
    html
}

/// Percent-encode a `/`-separated relative path segment by segment.
fn encode_path(rel: &str) -> String {
    rel.split('/')
        .map(|s| utf8_percent_encode(s, PATH_SEGMENT).to_string())
        .collect::<Vec<_>>()
        .join("/")
}

fn escape_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
    out
}

/// The site's `404.html` with the client injected (so a page opened
/// before it rendered goes live once it exists), else a plain 404.
async fn not_found(state: &StaticState, is_head: bool) -> Response {
    let page = state.root.join("404.html");
    if let Ok(bytes) = tokio::fs::read(&page).await {
        return file_response(&page, bytes, StatusCode::NOT_FOUND, false, is_head);
    }
    let body = b"Not Found".to_vec();
    let mut response = Response::builder()
        .status(StatusCode::NOT_FOUND)
        .header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
        .header(header::CONTENT_LENGTH, body.len())
        .header(header::CACHE_CONTROL, NO_STORE)
        .body(Body::empty())
        .expect("static headers are valid");
    if !is_head {
        *response.body_mut() = Body::from(body);
    }
    response
}

fn redirect(status: StatusCode, location: &str) -> Response {
    Response::builder()
        .status(status)
        .header(header::LOCATION, location)
        .header(header::CACHE_CONTROL, NO_STORE)
        .body(Body::empty())
        .unwrap_or_else(|_| StatusCode::NOT_FOUND.into_response())
}

/// A file from disk with its content type. HTML gets the client
/// unless the request was a CORS fetch.
fn file_response(
    path: &Path,
    bytes: Vec<u8>,
    status: StatusCode,
    is_cors_fetch: bool,
    is_head: bool,
) -> Response {
    let mime = mime_guess::from_path(path).first_or_octet_stream();
    if mime.type_() == mime_guess::mime::TEXT && mime.subtype() == mime_guess::mime::HTML {
        let body = if is_cors_fetch {
            bytes
        } else {
            inject_client(bytes)
        };
        return html_response(body, status, is_head);
    }
    let content_type = HeaderValue::from_str(mime.as_ref())
        .unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream"));
    bytes_response(bytes, status, content_type, is_head)
}

fn html_response(body: Vec<u8>, status: StatusCode, is_head: bool) -> Response {
    bytes_response(body, status, HTML_UTF8, is_head)
}

fn bytes_response(
    body: Vec<u8>,
    status: StatusCode,
    content_type: HeaderValue,
    is_head: bool,
) -> Response {
    let mut response = Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, content_type)
        .header(header::CONTENT_LENGTH, body.len())
        .header(header::CACHE_CONTROL, NO_STORE)
        .body(Body::empty())
        .expect("static headers are valid");
    if !is_head {
        *response.body_mut() = Body::from(body);
    }
    response
}

/// Splice the client script in before the last `</body>` (any case),
/// or append it when the page has none.
fn inject_client(mut html: Vec<u8>) -> Vec<u8> {
    let tag = format!("<script>\n{CLIENT_JS}</script>\n");
    let lower = html.to_ascii_lowercase();
    let needle = b"</body>";
    match lower
        .windows(needle.len())
        .rposition(|window| window == needle)
    {
        Some(at) => {
            html.splice(at..at, tag.into_bytes());
        }
        None => html.extend_from_slice(tag.as_bytes()),
    }
    html
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The script is embedded inside a `<script>` element; a literal
    /// closing tag in it would end the element early.
    #[test]
    fn client_script_never_closes_its_own_tag() {
        assert!(!CLIENT_JS.to_ascii_lowercase().contains("</script"));
        assert!(
            CLIENT_JS.contains(&format!("\"{EVENTS_PATH}\"")),
            "client must subscribe to EVENTS_PATH exactly"
        );
    }

    #[test]
    fn inject_goes_before_the_last_body_tag_of_any_case() {
        let out = inject_client(b"<html><BODY><p>x</p></BODY></html>".to_vec());
        let s = String::from_utf8(out).unwrap();
        let script = s.find("<script>").unwrap();
        let body_end = s.find("</BODY>").unwrap();
        assert!(script < body_end);
        assert!(s.ends_with("</BODY></html>"));
    }

    #[test]
    fn inject_appends_when_there_is_no_body_tag() {
        let out = inject_client(b"<p>fragment</p>".to_vec());
        let s = String::from_utf8(out).unwrap();
        assert!(s.starts_with("<p>fragment</p><script>"));
    }

    #[test]
    fn decode_segments_refuses_traversal_and_normalizes() {
        assert_eq!(decode_segments("/"), Some(vec![]));
        assert_eq!(
            decode_segments("/a//b/./c.html"),
            Some(vec!["a".into(), "b".into(), "c.html".into()])
        );
        assert_eq!(
            decode_segments("/sp%20ace/%C3%A9.html"),
            Some(vec!["sp ace".into(), "é.html".into()])
        );
        assert_eq!(decode_segments("/../x"), None);
        assert_eq!(decode_segments("/%2e%2e/x"), None);
        assert_eq!(decode_segments("/a/..%2Fx"), None);
        assert_eq!(decode_segments("/a%5Cb"), None);
        assert_eq!(decode_segments("/%00"), None);
        assert_eq!(decode_segments("/%ff"), None, "not UTF-8");
    }

    #[test]
    fn encode_path_keeps_dots_and_slashes_readable() {
        assert_eq!(encode_path("posts/my page.html"), "posts/my%20page.html");
        assert_eq!(encode_path("a#b?c.html"), "a%23b%3Fc.html");
    }
}
