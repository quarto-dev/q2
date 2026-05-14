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
use std::sync::{Arc, OnceLock};

use anyhow::{Context, Result};
use axum::{
    Router,
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use include_dir::{Dir, include_dir};
use quarto_core::engine::EngineRegistry;
use quarto_hub::{StorageManager, context::HubConfig, server, watch::WatchFilter};
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

pub mod capture_driver;

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
#[derive(Clone)]
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
    /// Optional engine registry override forwarded to the Phase C.1
    /// capture driver. Tests substitute a passthrough engine here so
    /// the integration suite doesn't need a real R / Python runtime.
    /// Production callers leave this `None` to use the default
    /// (`markdown` + native engines).
    pub engine_registry: Option<EngineRegistry>,
}

/// Run the preview server. Returns when the server is shut down (ctrl-c
/// or SIGTERM, handled inside `quarto_hub::server::run_server_with`).
pub async fn run(config: PreviewConfig) -> Result<()> {
    run_with_on_ready(config, |_ctx| {}).await
}

/// Like [`run`], but also fires `extra_on_ready` with `Arc<HubContext>`
/// once the hub has settled and the eager-capture driver has been
/// spawned. The extra callback runs *after* the driver is enqueued so
/// integration tests can stash the context (e.g. via a oneshot
/// channel) and poll for capture-sidecar state through real samod /
/// IndexDocument reads instead of re-implementing the protocol.
///
/// Library callers embedding q2 preview can also use this seam to
/// attach their own startup logic (metrics, logs, custom listeners).
/// Production CLI usage goes through [`run`] which passes a no-op.
pub async fn run_with_on_ready<F>(config: PreviewConfig, extra_on_ready: F) -> Result<()>
where
    F: FnOnce(Arc<quarto_hub::HubContext>) + Send + 'static,
{
    // Stash the SPA override before serving so the fallback handler
    // can read it. `set` is idempotent — calling `run()` twice in the
    // same process (uncommon but possible in tests) is harmless: the
    // first call's override wins.
    let _ = SPA_DIR_OVERRIDE.set(config.spa_dir_override.clone());

    let storage = build_storage(&config).context("building storage manager")?;
    let hub_config = build_hub_config(&config);

    // Phase C.1 hook: once the hub context is ready, spawn the eager
    // capture driver on a blocking worker so the multi-threaded tokio
    // runtime stays free for the HTTP listener and the file watcher.
    // The pipeline futures inside the driver are intentionally `?Send`
    // (see `.claude/rules/wasm.md`); `pollster::block_on` runs them
    // on the spawned thread without requiring `Send`.
    let engine_registry = config.engine_registry.clone();
    let on_ready: server::OnReadyCallback = Box::new(move |ctx| {
        let registry = engine_registry;
        let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
        let ctx_for_driver = ctx.clone();
        tokio::task::spawn_blocking(move || {
            let result = pollster::block_on(capture_driver::record_eager_captures(
                ctx_for_driver,
                runtime,
                registry,
            ));
            if let Err(e) = result {
                tracing::warn!(error = %e, "eager capture driver failed");
            }
        });
        // Fire the extra hook after the driver is enqueued so callers
        // can rely on it being either in-flight or already done.
        extra_on_ready(ctx);
    });

    // Phase C.2 hook: after sync_file updates samod with the new
    // bytes, recompute capture staleness against the existing sidecar
    // entry. Like the C.1 driver, the staleness recompute runs the
    // pipeline (non-Send futures), so it dispatches via
    // spawn_blocking + pollster::block_on. Errors are logged.
    //
    // notify-rs (the underlying file watcher on macOS / Linux) emits
    // canonicalized paths from FSEvents/inotify, but `StorageManager`
    // stores the project root as the caller supplied it. Canonicalize
    // both sides before strip_prefix so the comparison holds when the
    // caller passes `/tmp/foo` and the watcher reports
    // `/private/tmp/foo` (the same symlink-resolution issue
    // `sync_file_by_path` already has — see canonicalize note in
    // tests/staleness.rs).
    let on_file_changed: server::OnFileChangedCallback = Arc::new(move |ctx, abs_path| {
        let Some(project_root_raw) = ctx.storage().project_root().map(|p| p.to_path_buf()) else {
            return;
        };
        let project_root = project_root_raw.canonicalize().unwrap_or(project_root_raw);
        let abs_path_canonical = abs_path.canonicalize().unwrap_or(abs_path);
        let Ok(rel) = abs_path_canonical.strip_prefix(&project_root) else {
            return;
        };
        let rel_path = rel.to_string_lossy().to_string();
        let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
        tokio::task::spawn_blocking(move || {
            let result =
                pollster::block_on(capture_driver::recompute_staleness(ctx, runtime, &rel_path));
            if let Err(e) = result {
                tracing::warn!(error = %e, rel_path = %rel_path, "staleness recompute failed");
            }
        });
    });

    server::run_server_with(
        storage,
        hub_config,
        Some(extend_with_spa),
        Some(on_ready),
        Some(on_file_changed),
    )
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
