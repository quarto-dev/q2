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
    routing::{get, post},
};
use include_dir::{Dir, include_dir};
use quarto_core::engine::EngineRegistry;
use quarto_hub::{StorageManager, context::HubConfig, server, watch::WatchFilter};
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

pub mod cache;
pub mod capture_driver;
pub mod config;
pub mod deps;
pub mod diagnostics;
pub mod re_execute;

pub use config::EnginePolicy;

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

/// Map of `resources:`-declared output-relative paths → their absolute source
/// file on disk, used by [`artifact_resource_handler`] to SERVE embedded-example
/// decks (and their `slides_files/…` sidecars) at the artifact-rooted path the
/// preview iframe requests. Resolved once at `run()` from the project's
/// `resources:` set (the publish trust boundary, bd-teh4hbli); empty when there
/// is no project / no resources. CLI/disk-only — diskless hub-client embeds are
/// the service-worker workstream. (bd-kjrpya2d)
static RESOURCE_DISK_MAP: OnceLock<std::collections::HashMap<String, PathBuf>> = OnceLock::new();

/// Whether this preview session allows edits made in the preview UI to
/// persist to disk (bd-ov4gqk3m). Set once at `run()` from
/// [`PreviewConfig::allow_edit`] and served to the SPA via
/// `GET /api/preview/config` so it can enable/disable its edit surface.
/// Same first-writer-wins OnceLock pattern as the other handler state
/// above (one preview server per process; nextest isolates tests).
static ALLOW_EDIT: OnceLock<bool> = OnceLock::new();

/// Runtime configuration for the preview server.
#[derive(Clone)]
pub struct PreviewConfig {
    /// Host to bind to. Defaults to `127.0.0.1`.
    pub host: String,
    /// Port to bind to. `0` lets the OS pick a free port.
    pub port: u16,
    /// Project root to watch + serve. Files in the VFS come from here.
    pub project_root: Option<PathBuf>,
    /// bd-tnm3k: single-file mode. When `Some(rel)`, the hub indexes
    /// only `project_root.join(rel)` and watches just that one file,
    /// instead of walking `project_root`. Set by the CLI when the
    /// user passes a `.qmd` path with no `_quarto.yml` ancestor; the
    /// parent directory becomes `project_root`, and this field
    /// records the file's basename so discovery + watcher stay
    /// narrow (a project_root of `~/Downloads` must not pull in
    /// sibling files).
    pub single_file: Option<PathBuf>,
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
    /// Engine policy resolved from `preview.engine` in the project's
    /// `_quarto.yml` (Phase C.6). Default is [`EnginePolicy::Manual`],
    /// matching pre-C.6 behaviour. The CLI reads this once at session
    /// start via [`config::read_engine_policy_from_project`]; tests
    /// substitute directly.
    pub engine_policy: EnginePolicy,
    /// Resources-scoped `.html` files (project-root-relative) to carry
    /// into the VFS as text, so the preview iframe post-processor can
    /// inline embedded example decks via `srcdoc` (bd-kjrpya2d). The CLI
    /// resolves this once at session start via
    /// [`config::resolve_project_resource_html`]; tests leave it empty.
    /// Empty for single-file mode (no `_quarto.yml`, so no `resources:`).
    pub resource_html_files: Vec<PathBuf>,
    /// Directory for the Phase C.7 capture filesystem cache. When
    /// `None`, the cache lives at `<data_dir>/captures/` — same
    /// per-session lifetime as `data_dir` itself. Tests substitute a
    /// dedicated tempdir; future cross-session reuse (per-project
    /// location) is a Phase D follow-up.
    pub cache_dir: Option<PathBuf>,
    /// Allow edits made in the preview UI to persist to the files on
    /// disk (bd-ov4gqk3m). Set by the `--allow-edit` CLI flag.
    ///
    /// When `false` (the default), the preview is read-only end to end:
    /// the SPA disables its edit surface (it learns the setting from
    /// `GET /api/preview/config`), and — defense in depth — the hub
    /// runs with [`quarto_hub::sync::DiskWritePolicy::ReadOnly`] so
    /// document changes from *any* connected client can never be
    /// written back to the user's files.
    pub allow_edit: bool,
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

    // bd-kjrpya2d: resolve the project's `resources:` set to (output-relative
    // → disk path) so the artifact route can SERVE embedded-example decks +
    // their `slides_files/…` from disk. Best-effort; empty for single-file /
    // no-resources / no-project. Resolved once here (the resources are static
    // for the session).
    let resource_map: std::collections::HashMap<String, PathBuf> = config
        .project_root
        .as_deref()
        .map(|root| {
            config::resolve_project_resource_files(root, &NativeRuntime::new())
                .into_iter()
                .collect()
        })
        .unwrap_or_default();
    let _ = RESOURCE_DISK_MAP.set(resource_map);

    // Phase C.7: tell the re-execute HTTP handler where the per-doc
    // capture cache lives. Same OnceLock-first-wins pattern; in
    // production this fires once at server boot.
    re_execute::set_cache_dir(
        config
            .cache_dir
            .clone()
            .unwrap_or_else(|| config.data_dir.join("captures")),
    );

    // bd-b9kzg: initialize the per-server diagnostic sink so the
    // HTTP handler (`/api/preview/diagnostics`) and migrating
    // callsites (capture_driver, deps, re_execute) share one
    // page-keyed store. Same first-writer-wins OnceLock pattern;
    // tests inject pre-built sinks by setting SINK before calling
    // run_with_on_ready.
    diagnostics::set_sink(std::sync::Arc::new(diagnostics::DiagnosticSink::new()));

    // bd-ov4gqk3m: stash the edit-permission bit for the
    // `/api/preview/config` handler before the server starts serving.
    let _ = ALLOW_EDIT.set(config.allow_edit);

    let storage = build_storage(&config).context("building storage manager")?;
    let hub_config = build_hub_config(&config);

    // Phase C.1 hook: once the hub context is ready, spawn the eager
    // capture driver on a blocking worker so the multi-threaded tokio
    // runtime stays free for the HTTP listener and the file watcher.
    // The pipeline futures inside the driver are intentionally `?Send`
    // (see `.claude/rules/wasm.md`); `pollster::block_on` runs them
    // on the spawned thread without requiring `Send`.
    let engine_registry = config.engine_registry.clone();
    let engine_policy = config.engine_policy;
    let cache_dir = config
        .cache_dir
        .clone()
        .unwrap_or_else(|| config.data_dir.join("captures"));
    let registry_for_on_ready = engine_registry.clone();
    let cache_dir_for_on_ready = cache_dir.clone();
    let project_root_for_scripts = config.project_root.clone();
    let on_ready: server::OnReadyCallback = Box::new(move |ctx| {
        let registry = registry_for_on_ready;
        let cache_dir = cache_dir_for_on_ready;
        let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
        let ctx_for_driver = ctx.clone();
        tokio::task::spawn_blocking(move || {
            // bd-w348iu63 (plan D7): run the project's `pre-render`
            // scripts once, at boot, before the eager captures — a
            // script may generate data the engines read. Deliberately
            // never re-run (not on file edits, not on `_quarto.yml`
            // changes; restart the preview to re-run), and
            // post-render scripts don't run in preview at all — both
            // documented deviations from Quarto 1.
            if let Some(root) = &project_root_for_scripts {
                run_boot_pre_render_scripts(root);
            }
            let result = pollster::block_on(capture_driver::record_eager_captures(
                ctx_for_driver,
                runtime,
                registry,
                engine_policy,
                &cache_dir,
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
    let registry_for_on_file_changed = engine_registry.clone();
    let cache_dir_for_on_file_changed = cache_dir.clone();
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
        let registry = registry_for_on_file_changed.clone();
        let cache_dir = cache_dir_for_on_file_changed.clone();
        tokio::task::spawn_blocking(move || {
            let result = pollster::block_on(capture_driver::recompute_staleness(
                ctx,
                runtime,
                &rel_path,
                engine_policy,
                registry,
                &cache_dir,
            ));
            if let Err(e) = result {
                tracing::warn!(error = %e, rel_path = %rel_path, "staleness recompute failed");
            }
        });
    });

    server::run_server_with(
        storage,
        hub_config,
        Some(extend_with_preview),
        Some(on_ready),
        Some(on_file_changed),
    )
    .await
    .context("quarto-hub server failed")?;
    Ok(())
}

/// bd-w348iu63: run the project's `project.pre-render` scripts once at
/// preview boot. Failures are reported but never fatal — the preview
/// keeps serving (matching Q1 preview's catch-and-continue), and the
/// fix-it path is "repair the script, restart `q2 preview`".
fn run_boot_pre_render_scripts(project_root: &std::path::Path) {
    use quarto_core::ProjectContext;
    use quarto_core::project::render_scripts;

    let runtime = NativeRuntime::new();
    let project = match ProjectContext::discover(project_root, &runtime) {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!(error = %e, "pre-render scripts: project discovery failed");
            return;
        }
    };
    for diagnostic in render_scripts::underscore_typo_diagnostics(&project.config)
        .into_iter()
        .chain(quarto_core::project::project_kind_diagnostics(
            &project.config,
        ))
    {
        eprintln!("{}", diagnostic.to_text(None));
    }
    if project.config.pre_render_scripts.is_empty() {
        return;
    }
    let input_files: Vec<PathBuf> = project
        .files
        .iter()
        .map(|f| {
            f.input
                .strip_prefix(&project.dir)
                .map_or_else(|_| f.input.clone(), |r| r.to_path_buf())
        })
        .collect();
    let ctx = render_scripts::RenderScriptsContext {
        project_dir: &project.dir,
        output_dir: &project.output_dir,
        config_path: project.config.config_path.as_deref(),
        render_all: true,
        quiet: false,
        file_count: input_files.len(),
    };
    if let Err(parse_error) = render_scripts::run_render_scripts(
        render_scripts::ScriptPhase::PreRender,
        &project.config.pre_render_scripts,
        &ctx,
        &input_files,
    ) {
        eprintln!("{parse_error}");
        eprintln!(
            "note: the preview keeps serving; fix the script and restart `q2 preview` to re-run it."
        );
    }
}

/// Construct the StorageManager. `project_root` decides project vs
/// standalone mode; either way, `config.data_dir` is the storage root.
///
/// Secrets are always ephemeral (bd-tp1l6a0w): the preview is a
/// short-lived embedded hub — loopback only, no auth, per-session data
/// dir by default — so the server/session secrets live in memory only.
/// Nothing is persisted to `hub.json`, and the hub's multi-instance
/// secret-pinning warning does not apply.
fn build_storage(config: &PreviewConfig) -> Result<StorageManager> {
    match &config.project_root {
        Some(root) => StorageManager::new_with_data_dir_ephemeral(root, &config.data_dir)
            .map_err(|e| anyhow::anyhow!("storage init failed: {e}")),
        None => StorageManager::new_standalone_ephemeral(&config.data_dir)
            .map_err(|e| anyhow::anyhow!("storage init failed: {e}")),
    }
}

fn build_hub_config(config: &PreviewConfig) -> HubConfig {
    // bd-9cyza5vy: in single-file mode (no `_quarto.yml`), the deck's
    // transitive dependencies aren't found by a dir walk. Resolve the full
    // closure once — included `.qmd` (text) + referenced images (binary) — by
    // running the renderer's own include-expansion natively (quarto-preview has
    // the qmd parser + quarto-core). Supersedes the old direct-image-only
    // `resolve_single_file_assets`.
    let single_file_deps = match (
        config.project_root.as_deref(),
        config.single_file.as_deref(),
    ) {
        (Some(root), Some(rel)) => {
            config::resolve_single_file_deps(root, rel, std::sync::Arc::new(NativeRuntime::new()))
        }
        _ => config::SingleFileDeps::default(),
    };

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
        // bd-tnm3k: forward single-file mode to the hub so discovery
        // and the watcher stay scoped to the one file the user named.
        single_file: config.single_file.clone(),
        // bd-kjrpya2d: resources-scoped `.html` (embedded example decks)
        // resolved at session start, carried into the VFS as text so the
        // preview iframe post-processor can inline them via `srcdoc`.
        resource_files: config.resource_html_files.clone(),
        // bd-kpuweafo / bd-9cyza5vy: sibling images the deck references —
        // including images inside `{{< include >}}`d files — sync into the VFS
        // like project mode (resolved above via the real expansion pass).
        single_file_assets: single_file_deps.binary_files,
        // bd-9cyza5vy: the deck's transitive `{{< include >}}` closure, synced
        // as invisible text deps so includes expand in the WASM pipeline.
        single_file_text_deps: single_file_deps.qmd_files,
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
        // bd-ov4gqk3m: browser edits reach the user's files only when
        // `--allow-edit` was given. Without it the hub is
        // disk-authoritative — document changes from any connected
        // client are never written back to disk.
        disk_write_policy: if config.allow_edit {
            quarto_hub::sync::DiskWritePolicy::WriteBack
        } else {
            quarto_hub::sync::DiskWritePolicy::ReadOnly
        },
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

/// Hub-router extension used by the q2 preview server: the SPA
/// fallback (above) plus the Phase C.5 `POST /api/preview/re-execute`
/// endpoint. The route shares the hub's `SharedContext` state via
/// the standard `State<SharedContext>` extractor in the handler.
pub fn extend_with_preview(
    router: Router<quarto_hub::context::SharedContext>,
) -> Router<quarto_hub::context::SharedContext> {
    router
        // bd-kjrpya2d: serve `resources:`-declared embedded-example decks (+
        // their `slides_files/…`) from disk at the artifact-rooted path the
        // embed iframe requests. Takes precedence over the SPA fallback for
        // this prefix; non-resource paths fall through to the SPA index.
        .route(
            "/.quarto/project-artifacts/{*rest}",
            get(artifact_resource_handler),
        )
        .route(
            "/api/preview/re-execute",
            post(re_execute::re_execute_handler),
        )
        // Phase D.6 (bd-kw93.12): SPA fetches the active page's
        // include-shortcode dep set here so it can filter unrelated
        // sibling edits out of `onFileContent`-driven re-renders.
        .route("/api/preview/deps", get(deps::deps_handler))
        // bd-b9kzg: SPA fetches accumulated server-side diagnostics
        // (capture-driver / deps-parse / re-execute failures) per
        // page. Merged with the WASM render's own `result.warnings`
        // in the overlay.
        .route(
            "/api/preview/diagnostics",
            get(diagnostics::diagnostics_handler),
        )
        // bd-ov4gqk3m: session-level preview settings the SPA reads
        // once at boot — currently just `allowEdit`, which gates the
        // inline block-editing surface.
        .route("/api/preview/config", get(preview_config_handler))
        .fallback(spa_handler)
}

// ─── Handlers ────────────────────────────────────────────────────────────────

/// `GET /api/preview/config` — session-level preview settings the SPA
/// reads once at boot (bd-ov4gqk3m). `allowEdit` mirrors the
/// `--allow-edit` CLI flag; the SPA uses it to enable or fully disable
/// the inline block-editing surface, and the server independently
/// enforces the same setting via [`quarto_hub::sync::DiskWritePolicy`].
async fn preview_config_handler() -> Response {
    let allow_edit = ALLOW_EDIT.get().copied().unwrap_or(false);
    axum::Json(serde_json::json!({ "allowEdit": allow_edit })).into_response()
}

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

/// Serve a `resources:`-declared file from disk at the artifact-rooted path the
/// preview iframe requests (`/.quarto/project-artifacts/<output-relative>`), so
/// embedded-example decks (+ their linked `slides_files/…`) load like a real
/// served page — relative asset resolution and all. (bd-kjrpya2d)
///
/// `rest` is the axum-decoded wildcard (the `<output-relative>` suffix). Only
/// paths in [`RESOURCE_DISK_MAP`] (the declared `resources:` set) are served —
/// the publish trust boundary (bd-teh4hbli); any other `/.quarto/…` path falls
/// through to the SPA index, exactly as before this route existed.
async fn artifact_resource_handler(
    axum::extract::Path(rest): axum::extract::Path<String>,
) -> Response {
    if let Some(map) = RESOURCE_DISK_MAP.get()
        && let Some(disk) = map.get(&rest)
        && let Ok(bytes) = tokio::fs::read(disk).await
    {
        return asset_response(&rest, bytes);
    }
    serve_spa_index().await
}

/// Serve the SPA `index.html` (override dir if set, else the embedded bundle).
/// The index branch of [`spa_handler`], shared with the artifact route's
/// fall-through (a non-resource `/.quarto/…` path got `index.html` before this
/// route existed, so preserve that).
async fn serve_spa_index() -> Response {
    if let Some(Some(override_dir)) = SPA_DIR_OVERRIDE.get() {
        return serve_from_disk(override_dir, "index.html").await;
    }
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
