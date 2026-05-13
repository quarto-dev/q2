//! `q2 preview` server — wraps quarto-hub and serves the embedded
//! q2-preview-spa bundle.
//!
//! Phase A scope (bd-mflk): one process listening on one port serves
//! both the SPA (at `/` + asset paths via fallback) AND the hub's
//! API/auth/ws routes (`/api/*`, `/auth/*`, `/ws`). The hub registers
//! its samod ws endpoint at `/ws` only (not `/`) so the SPA's
//! `index.html` can own `/`; that's controlled via
//! `HubConfig::register_root_ws = false`.

use std::path::PathBuf;
use std::sync::OnceLock;

use anyhow::{Context, Result};
use axum::{
    Router,
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use include_dir::{Dir, include_dir};
use quarto_hub::{StorageManager, context::HubConfig, server, watch::WatchFilter};

/// The SPA bundle embedded at build time. See `build.rs` for how the
/// source directory is chosen (real `q2-preview-spa/dist/` if present,
/// else a placeholder).
static EMBEDDED_SPA: Dir<'_> = include_dir!("$QUARTO_PREVIEW_EMBED_DIR");

/// Optional override pointing at a SPA bundle on disk. Set once at
/// `run()` start and read on every SPA-fallback invocation. Process-
/// wide because the SPA fallback handler is stateless from axum's POV
/// (it composes onto a router whose typed state is `Arc<HubContext>`,
/// so adding handler state would require nested routers).
static SPA_DIR_OVERRIDE: OnceLock<Option<PathBuf>> = OnceLock::new();

/// Runtime configuration for the preview server.
#[derive(Debug, Clone)]
pub struct PreviewConfig {
    /// Host to bind to. Defaults to `127.0.0.1`.
    pub host: String,
    /// Port to bind to. `0` lets the OS pick a free port.
    pub port: u16,
    /// Project root to watch + serve. Files in the VFS come from here.
    pub project_root: Option<PathBuf>,
    /// Directory the hub's samod store + lockfile live in. Typically a
    /// `tempfile::TempDir` so each `q2 preview` is ephemeral. The
    /// caller owns the `TempDir` so it survives until `run()` returns.
    pub data_dir: PathBuf,
    /// If set, serve SPA assets from this directory at runtime instead
    /// of the embedded bundle. Same pattern as `QUARTO_TRACE_VIEWER_DIR`
    /// for the trace viewer; lets UI iteration skip Rust rebuilds.
    pub spa_dir_override: Option<PathBuf>,
}

/// Run the preview server. Returns when the server is shut down (ctrl-c
/// or SIGTERM, handled inside `quarto_hub::server::run_server_with`).
pub async fn run(config: PreviewConfig) -> Result<()> {
    // Stash the SPA override before serving so the fallback handler
    // can read it. `set` is idempotent — calling `run()` twice in the
    // same process (uncommon but possible in tests) is harmless: the
    // first call's override wins.
    let _ = SPA_DIR_OVERRIDE.set(config.spa_dir_override.clone());

    let storage = build_storage(&config).context("building storage manager")?;
    let hub_config = build_hub_config(&config);

    server::run_server_with(storage, hub_config, Some(extend_with_spa))
        .await
        .context("quarto-hub server failed")?;
    Ok(())
}

/// Construct the StorageManager. `project_root` decides project vs
/// standalone mode; either way, `config.data_dir` is the storage root.
fn build_storage(config: &PreviewConfig) -> Result<StorageManager> {
    match &config.project_root {
        Some(root) => StorageManager::new_with_data_dir(root, &config.data_dir)
            .map_err(|e| anyhow::anyhow!("storage init failed: {e}")),
        None => StorageManager::new_standalone(&config.data_dir)
            .map_err(|e| anyhow::anyhow!("storage init failed: {e}")),
    }
}

fn build_hub_config(config: &PreviewConfig) -> HubConfig {
    HubConfig {
        port: config.port,
        host: config.host.clone(),
        peers: Vec::new(),
        // Phase A is engine-less, but file-watching is what makes
        // `q2 preview` responsive to edits — keep it on.
        watch_enabled: true,
        watch_debounce_ms: 500,
        // Phase B.1 (bd-z529): preview wants config / metadata / image /
        // .tsx edits to trigger re-render, not just .qmd. Hub keeps the
        // default narrow filter for backwards compatibility.
        watch_filter: WatchFilter::PreviewBroad,
        // Light periodic sync — the user can Ctrl-C any time, and
        // shutdown does a final sync anyway. 5 seconds is a reasonable
        // crash-resilience window for a *preview* invocation.
        sync_interval_secs: Some(5),
        // Preview never wants OIDC: the server binds to loopback only
        // and lives only as long as the foreground CLI invocation.
        auth_config: None,
        allow_insecure_auth: false,
        // SPA owns `/`; hub gets `/ws` only.
        register_root_ws: false,
    }
}

/// Hub-router extension that adds the SPA fallback. Anything that
/// doesn't match a hub route (`/api/*`, `/auth/*`, `/ws`, …) falls
/// through to this handler, which serves `index.html` for unknown
/// paths so client-side routing works.
///
/// Pub-visible because the smoke test exercises it directly without
/// spinning up a real hub.
pub fn extend_with_spa<S>(router: Router<S>) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    router.fallback(spa_handler)
}

// ─── Handlers ────────────────────────────────────────────────────────────────

async fn spa_handler(req: axum::http::Request<axum::body::Body>) -> Response {
    let path = req.uri().path();
    let rel = path.trim_start_matches('/');
    let rel = if rel.is_empty() { "index.html" } else { rel };

    if let Some(Some(override_dir)) = SPA_DIR_OVERRIDE.get() {
        return serve_from_disk(override_dir, rel).await;
    }

    // Try the exact path first (an asset like `assets/index-<hash>.js`).
    if let Some(file) = EMBEDDED_SPA.get_file(rel) {
        return asset_response(rel, file.contents().to_vec());
    }
    // SPA fallback: any non-asset path gets `index.html` for client-
    // side routing.
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
