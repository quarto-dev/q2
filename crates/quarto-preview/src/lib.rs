//! `q2 preview` server — wraps quarto-hub and serves the embedded
//! q2-preview-spa bundle.
//!
//! Phase A scope (bd-yxqt): the bundle is served, the smoke test
//! passes, but the hub server isn't yet layered in — A.5 (bd-mflk)
//! adds the actual `quarto_hub::server::run_server` integration plus
//! the temp `data_dir` lifecycle. For now this crate is just an axum
//! shell that hosts the SPA, modelled after `quarto-trace-server`.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use axum::{
    Router,
    extract::State,
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::get,
};
use include_dir::{Dir, include_dir};

/// The SPA bundle embedded at build time. See `build.rs` for how the
/// source directory is chosen (real `q2-preview-spa/dist/` if present,
/// else a placeholder).
static EMBEDDED_SPA: Dir<'_> = include_dir!("$QUARTO_PREVIEW_EMBED_DIR");

/// Runtime configuration for the preview server.
#[derive(Debug, Clone)]
pub struct PreviewConfig {
    /// Host to bind to. Defaults to `127.0.0.1`.
    pub host: String,
    /// Port to bind to. `0` lets the OS pick a free port.
    pub port: u16,
    /// If set, serve SPA assets from this directory at runtime instead
    /// of the embedded bundle. Same pattern as `QUARTO_TRACE_VIEWER_DIR`
    /// for the trace viewer; lets UI iteration skip Rust rebuilds.
    pub spa_dir_override: Option<PathBuf>,
}

impl Default for PreviewConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: 0,
            spa_dir_override: std::env::var("QUARTO_PREVIEW_DIR").ok().map(PathBuf::from),
        }
    }
}

/// Shared server state. Cloneable so axum can hand it to each handler.
#[derive(Clone)]
struct AppState {
    spa_dir_override: Option<Arc<PathBuf>>,
}

/// Build the axum router. Exposed for testing — Phase A.5 will layer
/// the hub server's router on top of this one.
pub fn router(config: &PreviewConfig) -> Router {
    let state = AppState {
        spa_dir_override: config.spa_dir_override.clone().map(Arc::new),
    };
    Router::new().fallback(get(spa_handler)).with_state(state)
}

/// Bind the server and run it until shutdown.
///
/// Returns the bound address before serving so callers (e.g. the
/// future A.5 launcher) can know what URL to advertise / pass to
/// `open`. The actual serve runs to completion on the current task.
pub async fn run(config: PreviewConfig) -> Result<()> {
    let addr: SocketAddr = format!("{}:{}", config.host, config.port)
        .parse()
        .with_context(|| format!("parsing bind addr {}:{}", config.host, config.port))?;
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("binding {addr}"))?;
    let bound = listener.local_addr()?;
    tracing::info!(addr = %bound, "q2 preview server listening");

    let app = router(&config);
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("server error")?;
    Ok(())
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
}

// ─── Handlers ────────────────────────────────────────────────────────────────

async fn spa_handler(
    State(state): State<AppState>,
    req: axum::http::Request<axum::body::Body>,
) -> Response {
    let path = req.uri().path();
    let rel = path.trim_start_matches('/');
    let rel = if rel.is_empty() { "index.html" } else { rel };

    if let Some(override_dir) = state.spa_dir_override.as_deref() {
        return serve_from_disk(override_dir, rel).await;
    }

    // Try the exact path first (an asset like `assets/index-<hash>.js`).
    if let Some(file) = EMBEDDED_SPA.get_file(rel) {
        return asset_response(rel, file.contents().to_vec());
    }
    // SPA fallback: any non-asset path gets `index.html` for client-side
    // routing.
    if let Some(index) = EMBEDDED_SPA.get_file("index.html") {
        return asset_response("index.html", index.contents().to_vec());
    }
    (StatusCode::NOT_FOUND, "no spa").into_response()
}

async fn serve_from_disk(root: &std::path::Path, rel: &str) -> Response {
    let abs = root.join(rel);
    match tokio::fs::read(&abs).await {
        Ok(bytes) => asset_response(rel, bytes),
        Err(_) => {
            // Fallback to index.html for client-side routing.
            let index = root.join("index.html");
            match tokio::fs::read(&index).await {
                Ok(bytes) => asset_response("index.html", bytes),
                Err(e) => (StatusCode::NOT_FOUND, e.to_string()).into_response(),
            }
        }
    }
}

fn asset_response(rel: &str, bytes: Vec<u8>) -> Response {
    let mut headers = HeaderMap::new();
    headers.insert(header::CONTENT_TYPE, content_type_for(rel));
    (StatusCode::OK, headers, bytes).into_response()
}

fn content_type_for(path: &str) -> HeaderValue {
    let ext = std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");
    let mime = match ext.to_ascii_lowercase().as_str() {
        "html" => "text/html; charset=utf-8",
        "js" => "application/javascript",
        "css" => "text/css",
        "json" => "application/json",
        "wasm" => "application/wasm",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        _ => "application/octet-stream",
    };
    HeaderValue::from_static(mime)
}
