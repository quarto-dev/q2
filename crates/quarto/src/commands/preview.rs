//! `q2 preview` — Q2 replacement for the TypeScript Quarto `quarto
//! preview` command.
//!
//! Phase A (bd-mflk): boots an ephemeral hub server, serves the
//! q2-preview SPA on the same port, prints the launch URL, and
//! blocks until Ctrl-C. The hub's samod ws is at `/ws`; the SPA's
//! React mount is at `/`. The SPA fetches the project's index
//! document id from hub's `/health` endpoint at boot, then connects
//! samod through `/ws`.

use std::net::TcpListener as StdTcpListener;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};
use quarto_preview::{
    EnginePolicy, PreviewConfig,
    config::{read_engine_policy_from_project, resolve_project_resource_html},
};
use quarto_system_runtime::NativeRuntime;
use tempfile::TempDir;
use tracing::info;

/// Concrete shape passed through from clap.
///
/// Mirrors the Phase A flag set in `claude-notes/plans/2026-05-13-q2-
/// preview-phase-a.md` §A.1.
pub struct PreviewArgs {
    /// Project root or single file to preview. Default: current dir.
    pub path: Option<PathBuf>,
    /// Port to listen on. Default: probe an OS-assigned free port.
    pub port: Option<u16>,
    /// Host to bind to. Default: 127.0.0.1 (loopback only).
    pub host: Option<String>,
    /// Skip the browser-open step.
    pub no_browser: bool,
    /// Open the preview in this browser instead of the system
    /// default (`--browser <name>`). clap makes it conflict with
    /// `--no-browser`, so the two never both reach here.
    pub browser: Option<String>,
    /// Override the ephemeral samod storage dir. Default: a fresh
    /// `tempfile::TempDir` that's deleted on shutdown.
    pub data_dir: Option<PathBuf>,
    /// Override the embedded SPA bundle with a disk path. Mirrors
    /// `QUARTO_TRACE_VIEWER_DIR`.
    pub preview_dir: Option<PathBuf>,
    /// Run standalone (no local project mode).
    pub no_project: bool,
    /// Allow edits made in the preview UI to be written back to the
    /// files on disk (bd-ov4gqk3m). Off by default: without it the
    /// preview is read-only end to end.
    pub allow_edit: bool,
    /// Share this preview session over an end-to-end encrypted iroh
    /// tunnel (bd-jhvkwosw). The server prints a `q2preview…` join
    /// string; the HTTP port itself stays loopback-bound.
    pub share: bool,
    /// Which embedded frontend the server serves (`--ui`, live-share
    /// plan Phase 4, bd-jt1etjbn): the read-only preview SPA (default)
    /// or the full hub-client editor. Never changes the write policy.
    pub ui: quarto_preview::PreviewUi,
}

pub fn execute(args: PreviewArgs) -> Result<()> {
    // Multi-threaded runtime for the same reasons quarto hub uses
    // one: samod, websockets, file watching, periodic sync.
    let runtime = tokio::runtime::Runtime::new()?;
    // bd-hxhnnlzs: hold a kernel scope for the server's lifetime so
    // re-renders reuse warm kernels. It drops after `block_on` returns
    // (the hub server resolves SIGINT/SIGTERM/Ctrl-C into a graceful
    // return), outside the runtime — so kernels get the polite
    // `shutdown_request` path before the kill backstop.
    let _kernel_scope = quarto_core::engine::jupyter::kernel_scope();
    runtime.block_on(run(args))
}

/// Guest-mode argument shape for `q2 preview --join <TICKET>` (live-share
/// plan Phase 3, bd-6y0p1bne). Deliberately tiny: the guest has no local
/// project, hub, or disk surface — clap rejects every host-mode flag.
pub struct JoinArgs {
    /// The `q2preview…` join string printed by `q2 preview --share`.
    pub ticket: String,
    /// Local proxy port. Default: OS-assigned.
    pub port: Option<u16>,
    /// Local proxy bind host. Default: 127.0.0.1.
    pub host: Option<String>,
    /// Skip the browser-open step.
    pub no_browser: bool,
    /// Open the session in this browser instead of the system
    /// default (`--browser <name>`).
    pub browser: Option<String>,
}

pub fn execute_join(args: JoinArgs) -> Result<()> {
    let runtime = tokio::runtime::Runtime::new()?;
    runtime.block_on(run_join(args))
}

async fn run(args: PreviewArgs) -> Result<()> {
    // Project mode is the default (epic plan Q5); --no-project is
    // the explicit standalone-server escape hatch. Canonicalize so
    // log lines and error messages don't show whatever relative form
    // the user typed.
    // Phase D.2 (bd-kw93.13): split out the "what file did the user
    // ask for" signal from the project root. `initial_page` is None
    // unless we can confidently identify a specific page to seed in
    // the SPA — typically: user gave a file inside a _quarto.yml
    // project, OR the project root has an `index.qmd`.
    let (project_root, initial_page, single_file) = if args.no_project {
        if args.path.is_some() {
            anyhow::bail!("--no-project and a positional path are mutually exclusive");
        }
        (None, None, None)
    } else {
        let raw = args
            .path
            .unwrap_or_else(|| std::env::current_dir().expect("cwd"));
        let canonical = raw
            .canonicalize()
            .with_context(|| format!("resolving project root {}", raw.display()))?;
        let resolved = resolve_project_and_initial_page(&canonical)?;
        (
            Some(resolved.root),
            resolved.initial_page,
            resolved.single_file,
        )
    };

    // Each invocation gets a fresh `TempDir` by default — when this
    // `_temp` binding drops at function end (or after Ctrl-C), the
    // directory + its samod store are removed. `--data-dir` lets the
    // user reuse a persistent dir if they need crash-resilience
    // across restarts.
    let (data_dir, _temp): (PathBuf, Option<TempDir>) = match args.data_dir {
        Some(p) => (p, None),
        None => {
            let temp =
                TempDir::with_prefix("q2-preview-").context("creating ephemeral data dir")?;
            (temp.path().to_path_buf(), Some(temp))
        }
    };

    let host = args.host.unwrap_or_else(|| "127.0.0.1".to_string());

    // Resolve port. `0` would let the OS pick at bind time, but we
    // want to print the URL *before* the long-running server starts,
    // which means knowing the port up front. Pre-bind a throwaway
    // TcpListener at the chosen port (or 0), capture the resolved
    // value, drop the listener — there's a tiny race where another
    // process could steal the port before run_server rebinds, but
    // for a foreground developer tool that's an acceptable trade-off.
    // `--port 0` and an absent `--port` are equivalent: both mean
    // "let the OS pick a free port." We probe up front so the URL
    // we print is reachable.
    let port = match args.port {
        Some(0) | None => probe_free_port(&host)?,
        Some(p) => p,
    };

    // Phase D.1 (bd-kw93.8): when the user pinned a *specific* port,
    // pre-probe to produce a friendlier error than the raw bind
    // failure that bubbles out of `run_server_with`. There's a tiny
    // race between this probe and the server's actual bind (another
    // process could steal the port), but the failure mode there is
    // the same opaque bind error we get today, so we're strictly
    // better off. `--port 0` is the documented "let the OS pick"
    // escape hatch — that takes the `probe_free_port` path so the
    // printed URL carries the real bound port instead of `:0`.
    if let Some(p) = args.port
        && p != 0
    {
        validate_explicit_port(&host, p)?;
    }

    // Phase D.2: encode the initial page (if any) as `?page=<rel>`
    // so the SPA's `pickInitialPage` helper can seed `activeFile`.
    //
    // Phase 4 (bd-jt1etjbn): only viewer mode can print its boot URL
    // here. The editor boot URL is the hub-client share route, which
    // needs the index document id — that only exists once the hub is
    // up, so editor mode defers the print + browser-open into the
    // server's `on_ready` callback below (which fires before the
    // listener binds, the same print-before-accept contract as here).
    match args.ui {
        quarto_preview::PreviewUi::Viewer => {
            let url = build_boot_url(&host, port, initial_page.as_deref());
            info!(%url, "starting q2 preview server");
            println!();
            println!("  q2 preview");
            println!("  → {url}");
            println!();

            // Phase D.1 (bd-kw93.8): actually open a browser tab, gated on
            // the server accepting connections (bd-a6dvrdg1 — see
            // `spawn_browser_open_when_ready`). Failure to open is logged +
            // non-fatal (the URL is already printed for copy-paste).
            // Suppressed by --no-browser.
            if !args.no_browser {
                spawn_browser_open_when_ready(host.clone(), port, url, args.browser.clone());
            }
        }
        quarto_preview::PreviewUi::Editor => {
            info!("starting q2 preview server (editor UI)");
            println!();
            println!("  q2 preview — editor UI");
            if let Some(note) = quarto_preview::editor_ephemeral_note(args.allow_edit) {
                println!("  {note}");
            }
        }
    }

    // Phase C.6: read `preview.engine` from `_quarto.yml` so the
    // driver knows whether to skip eager execution (`off`) or auto-
    // re-execute on edits (`auto`). Single-file projects with no
    // `_quarto.yml` get `Manual` (the safe pre-C.6 default).
    let engine_policy = match project_root.as_deref() {
        Some(root) => read_engine_policy_from_project(root, &NativeRuntime::new()),
        None => EnginePolicy::Manual,
    };
    info!(?engine_policy, "resolved preview engine policy");

    // bd-kjrpya2d (part 2): resolve the `.html` files made visible by
    // `project.resources:` so the hub carries them into the VFS source
    // tree. The preview iframe post-processor reads them there and
    // inlines embedded example decks via `srcdoc` (there is no disk
    // server to answer the iframe's request in preview). Single-file
    // mode has no `_quarto.yml`, hence no project resources.
    let resource_html_files = match project_root.as_deref() {
        Some(root) => resolve_project_resource_html(root, &NativeRuntime::new()),
        None => Vec::new(),
    };
    info!(
        resource_html_count = resource_html_files.len(),
        "resolved resources-scoped .html for VFS sync"
    );

    let config = PreviewConfig {
        host,
        port,
        project_root,
        single_file,
        data_dir,
        spa_dir_override: args.preview_dir,
        // CLI uses the default engine registry; tests substitute a
        // passthrough engine via the integration-test surface.
        engine_registry: None,
        engine_policy,
        resource_html_files,
        // Phase C.7: derive the capture cache dir from `data_dir`
        // (per-session; tracked as a follow-up for per-project reuse).
        cache_dir: None,
        allow_edit: args.allow_edit,
        share: args.share,
        ui: args.ui,
    };

    // Host-side Ctrl-C acknowledgment, symmetric with the guest's line
    // in run_join: printed by the hub's own signal task via
    // `HubConfig::shutdown_message` (set in quarto-preview's
    // build_hub_config from `args.share`). Printing there — on the
    // shutdown critical path — keeps a fast teardown from exiting the
    // process before the line appears (bd-wj9smyxg).
    match args.ui {
        quarto_preview::PreviewUi::Viewer => quarto_preview::run(config).await,
        quarto_preview::PreviewUi::Editor => {
            // Phase 4 (bd-jt1etjbn): the share-route boot URL needs the
            // index doc id, which exists only server-side. `on_ready`
            // channels it back: build the URL there, print it, and gate
            // the browser-open on the same accept probe viewer mode uses.
            let host_for_ready = config.host.clone();
            let project_name = config
                .project_root
                .as_deref()
                .and_then(|r| r.file_name())
                .map_or_else(
                    || "q2 preview".to_string(),
                    |n| n.to_string_lossy().into_owned(),
                );
            let no_browser = args.no_browser;
            let browser = args.browser.clone();
            quarto_preview::run_with_on_ready(config, move |ctx| {
                let paths: Vec<String> = ctx.index().get_all_files().into_keys().collect();
                let url = match pick_editor_file(initial_page.as_deref(), &paths) {
                    Some(file) => {
                        // bd-7htq16rx: hand `--join` guests the same
                        // boot params via /api/preview/config so they
                        // boot the share route (skipping the project-set
                        // setup screen) instead of the editor's root
                        // route, which can never join the document.
                        quarto_preview::set_editor_boot(quarto_preview::EditorBootInfo {
                            index_doc_id: ctx.index().document_id(),
                            file: file.clone(),
                            name: project_name.clone(),
                        });
                        build_editor_boot_url(
                            &host_for_ready,
                            port,
                            &ctx.index().document_id(),
                            &file,
                            &project_name,
                        )
                    }
                    None => {
                        // No `.qmd` to seed the share route with (e.g.
                        // --no-project): boot to the editor's project
                        // selector instead of a broken share link.
                        tracing::warn!(
                            "no .qmd file found to open in the editor; \
                             booting to the project selector"
                        );
                        format!("http://{host_for_ready}:{port}/")
                    }
                };
                info!(%url, "editor UI ready");
                println!("  → {url}");
                println!();
                if !no_browser {
                    spawn_browser_open_when_ready(
                        host_for_ready.clone(),
                        port,
                        url,
                        browser.clone(),
                    );
                }
            })
            .await
        }
    }
}

/// Guest path (live-share plan Phase 3, bd-6y0p1bne): parse the join
/// string, dial the host over iroh, serve the session on a local
/// loopback proxy, and report connection status until Ctrl-C. Bypasses
/// project resolution, the TempDir, and the hub entirely — the *host's*
/// preview server serves everything through the tunnel.
async fn run_join(args: JoinArgs) -> Result<()> {
    let ticket: quarto_p2p::PreviewShareTicket = args.ticket.trim().parse().map_err(|e| {
        anyhow::anyhow!(
            "invalid join string ({e})\n\
             Expected the `q2preview…` string printed by `q2 preview --share` on the \
             host machine. Copy the whole string — it is long and may wrap across \
             several terminal lines."
        )
    })?;

    let host = args.host.unwrap_or_else(|| "127.0.0.1".to_string());
    // An explicit --port gets the friendly availability check. Port 0 /
    // absent means the OS assigns one; unlike host mode there is no
    // pre-probe — `TunnelClient::bind` reports the port it bound.
    if let Some(p) = args.port
        && p != 0
    {
        validate_explicit_port(&host, p)?;
    }
    let requested_port = args.port.unwrap_or(0);
    let local = tokio::net::lookup_host((host.as_str(), requested_port))
        .await
        .ok()
        .and_then(|mut addrs| addrs.next())
        .ok_or_else(|| anyhow::anyhow!("could not resolve --host {host}"))?;

    println!();
    println!("  q2 preview — joining a shared session (end-to-end encrypted via iroh)");

    // The initial dial happens inside `bind`: an unreachable host is a
    // clear error right here, not a silent background retry.
    let (bound, handle) =
        quarto_p2p::TunnelClient::bind(quarto_p2p::TunnelClientConfig::default(), ticket, local)
            .await
            .map_err(join_bind_error)?;

    // Resolve the boot URL through the tunnel before printing it.
    // Readiness = the first successful GET /health *through the
    // tunnel*: a bare TCP accept (host mode's readiness signal) would
    // lie here — the local proxy accepts even when the host is
    // unreachable — so only an end-to-end HTTP roundtrip proves the
    // session is usable. Editor-UI hosts then carry their share-route
    // boot params in /api/preview/config (bd-7htq16rx): boot the guest
    // to the same share URL the host printed, ephemeral flag included,
    // so hub-client skips project-set onboarding and joins the
    // document. Viewer-mode and older hosts answer without
    // `editorBoot` and keep the root URL.
    const READY_TIMEOUT: Duration = Duration::from_secs(15);
    let url = if wait_until_healthy(bound, READY_TIMEOUT).await {
        info!(local = %bound, "shared session healthy through the tunnel");
        match fetch_editor_boot(bound).await {
            Some(boot) => build_guest_editor_url(&bound, &boot),
            None => format!("http://{bound}/"),
        }
    } else {
        // We still consider a >15s startup a bug; printing the root
        // URL anyway (rather than never) preserves the old behavior as
        // a floor, and the warning gives the slow start visibility
        // instead of leaving it silent.
        tracing::warn!(
            local = %bound,
            timeout_secs = READY_TIMEOUT.as_secs(),
            "shared session did not answer /health within the timeout; \
             the URL below may need a manual reload"
        );
        format!("http://{bound}/")
    };

    println!("  → {url}");
    println!();
    println!("  Press Ctrl-C to leave the session.");
    println!();

    if !args.no_browser {
        open_browser_or_log(&url, args.browser.as_deref());
    }

    // Report status transitions ("connected via relay", "reconnecting…")
    // until Ctrl-C — or fail fast when the host rejects the token.
    let mut status = handle.status();
    let status_reporter = async move {
        loop {
            match *status.borrow_and_update() {
                quarto_p2p::TunnelStatus::Connected(kind) => {
                    println!("  ● connected via {kind}");
                }
                quarto_p2p::TunnelStatus::Reconnecting => {
                    println!("  ○ connection lost — reconnecting…");
                }
                quarto_p2p::TunnelStatus::Rejected => {
                    return Err(anyhow::anyhow!(
                        "the host rejected this join string — the share session has \
                         ended or was restarted with a new string.\n\
                         Ask the host for a fresh `q2 preview --share` join string."
                    ));
                }
            }
            if status.changed().await.is_err() {
                // Sender gone = tunnel client shut down; stop reporting.
                return Ok(());
            }
        }
    };

    let outcome = tokio::select! {
        _ = tokio::signal::ctrl_c() => {
            println!();
            println!("  Received Ctrl-C, leaving the shared session…");
            Ok(())
        }
        reported = status_reporter => reported,
    };

    if let Err(e) = handle.shutdown().await {
        tracing::warn!(error = %e, "tunnel client shutdown failed");
    }
    outcome
}

/// Map a tunnel bind failure to actionable CLI guidance.
fn join_bind_error(e: quarto_p2p::TunnelError) -> anyhow::Error {
    use quarto_p2p::TunnelError;
    match e {
        TunnelError::Connect(src) => anyhow::anyhow!(
            "could not reach the share host ({src})\n\
             Check that `q2 preview --share` is still running on the host machine \
             and that both machines are online, then retry with the same join string."
        ),
        TunnelError::Proxy(src) => anyhow::anyhow!(
            "could not bind the local proxy port: {src}\n\
             Pass --port 0 to let the OS pick a free port."
        ),
        other => anyhow::Error::new(other).context("starting the tunnel client"),
    }
}

/// Poll `GET /health` on the local proxy until it answers 200 — i.e. the
/// host's preview hub answered *through the tunnel* — or `total_timeout`
/// elapses. Join mode's readiness gate; same backoff shape and
/// open-anyway-on-timeout contract as [`wait_until_accepting`].
async fn wait_until_healthy(addr: std::net::SocketAddr, total_timeout: Duration) -> bool {
    const INITIAL_BACKOFF: Duration = Duration::from_millis(50);
    const MAX_BACKOFF: Duration = Duration::from_secs(1);
    // A tunnel roundtrip can legitimately take a relay RTT, but one
    // wedged attempt must not eat the whole budget.
    const ATTEMPT_TIMEOUT: Duration = Duration::from_secs(5);

    let deadline = tokio::time::Instant::now() + total_timeout;
    let mut backoff = INITIAL_BACKOFF;

    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            return false;
        }
        if let Ok(true) =
            tokio::time::timeout(remaining.min(ATTEMPT_TIMEOUT), health_get_ok(addr)).await
        {
            return true;
        }

        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            return false;
        }
        tokio::time::sleep(backoff.min(remaining)).await;
        backoff = (backoff * 8 / 5).min(MAX_BACKOFF);
    }
}

/// One raw HTTP/1.1 `GET /health` against `addr`; `true` iff the status
/// line comes back 200. Hand-rolled so the CLI doesn't grow an HTTP
/// client dependency for a one-line probe.
async fn health_get_ok(addr: std::net::SocketAddr) -> bool {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let Ok(mut stream) = tokio::net::TcpStream::connect(addr).await else {
        return false;
    };
    if stream
        .write_all(b"GET /health HTTP/1.1\r\nHost: q2-preview-join\r\nConnection: close\r\n\r\n")
        .await
        .is_err()
    {
        return false;
    }
    let mut response = Vec::new();
    if stream.read_to_end(&mut response).await.is_err() {
        return false;
    }
    response.starts_with(b"HTTP/1.1 200")
}

/// One raw HTTP/1.1 `GET /api/preview/config` against `addr`; the
/// parsed `editorBoot` params when the host is an editor-UI session
/// that stashed them (bd-7htq16rx). Hand-rolled for the same reason as
/// [`health_get_ok`]. Any failure — connect, non-200, malformed body,
/// absent field — is `None`, and the caller falls back to the root URL.
async fn fetch_editor_boot(addr: std::net::SocketAddr) -> Option<quarto_preview::EditorBootInfo> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut stream = tokio::net::TcpStream::connect(addr).await.ok()?;
    stream
        .write_all(
            b"GET /api/preview/config HTTP/1.1\r\nHost: q2-preview-join\r\nConnection: close\r\n\r\n",
        )
        .await
        .ok()?;
    let mut response = Vec::new();
    stream.read_to_end(&mut response).await.ok()?;
    if !response.starts_with(b"HTTP/1.1 200") {
        return None;
    }
    let body_start = response.windows(4).position(|w| w == b"\r\n\r\n")? + 4;
    parse_editor_boot(&response[body_start..])
}

/// Parse the `editorBoot` field of a `/api/preview/config` body.
/// `None` for viewer-mode and older hosts (no field), for malformed
/// JSON, and for a field whose doc id or file is empty — a boot URL
/// built from those could never join the document.
fn parse_editor_boot(body: &[u8]) -> Option<quarto_preview::EditorBootInfo> {
    #[derive(serde::Deserialize)]
    struct PreviewConfigWire {
        #[serde(rename = "editorBoot")]
        editor_boot: Option<quarto_preview::EditorBootInfo>,
    }
    let boot = serde_json::from_slice::<PreviewConfigWire>(body)
        .ok()?
        .editor_boot?;
    if boot.index_doc_id.is_empty() || boot.file.is_empty() {
        return None;
    }
    Some(boot)
}

/// Bind `host:0`, read the OS-assigned port, drop the listener.
/// Returns the port number so the caller can pre-print the URL.
fn probe_free_port(host: &str) -> Result<u16> {
    let listener =
        StdTcpListener::bind((host, 0)).with_context(|| format!("probing free port on {host}"))?;
    let port = listener
        .local_addr()
        .context("local_addr of probed listener")?
        .port();
    drop(listener);
    Ok(port)
}

/// Phase D.1: pre-probe an explicit `--port` so we can produce a
/// clean "port N already in use" error instead of the raw bind
/// failure from inside `quarto_hub::server::run_server_with`. The
/// returned error message names `--port 0` as the escape hatch.
fn validate_explicit_port(host: &str, port: u16) -> Result<()> {
    match StdTcpListener::bind((host, port)) {
        Ok(listener) => {
            drop(listener);
            Ok(())
        }
        Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => Err(anyhow::anyhow!(
            "port {port} on {host} is already in use; pass --port 0 to let the OS pick a free \
             port, or omit --port for the default probe behaviour"
        )),
        Err(e) => Err(anyhow::anyhow!("could not bind to {host}:{port}: {e}")),
    }
}

/// Phase D.1: open the boot URL in the user's browser. `browser` is
/// the `--browser <name>` value: `Some` opens that specific application
/// (via `open::with`, i.e. `open -a` on macOS), `None` the system
/// default. Failure is logged + non-fatal — the URL was already
/// printed for copy-paste before this fires. Callers gate on
/// `--no-browser` before calling.
fn open_browser_or_log(url: &str, browser: Option<&str>) {
    let result = match browser {
        Some(app) => open::with(url, app),
        None => open::that(url),
    };
    if let Err(e) = result {
        if let Some(app) = browser {
            tracing::warn!(
                error = %e,
                browser = app,
                "could not open the preview in the requested browser; the URL is printed above"
            );
        } else {
            tracing::warn!(
                error = %e,
                "could not auto-open browser; the URL is printed above"
            );
        }
    }
}

/// bd-a6dvrdg1: spawn the "open the browser once the server accepts
/// connections" task both UI modes share. The server doesn't start
/// until `quarto_preview::run` below (which then blocks until
/// shutdown), so the open has to run on a spawned task that waits for
/// readiness while the main task goes on to start the server. Opening
/// eagerly — as we used to — raced the server's startup: on larger
/// projects the browser connected before `axum::serve` was live and
/// showed "Unable to connect" until a manual reload. The probe
/// (`wait_until_accepting`) closes that race by gating on the real
/// accept condition. The port is the pre-probed one; nothing is
/// listening on it until the server binds, so the probe naturally
/// retries across the gap. A >10 s startup is still considered a bug,
/// but opening anyway (rather than never) preserves the old behavior
/// as a floor, and the warning gives the slow start visibility instead
/// of leaving it silent.
fn spawn_browser_open_when_ready(host: String, port: u16, url: String, browser: Option<String>) {
    tokio::spawn(async move {
        const READY_TIMEOUT: Duration = Duration::from_secs(10);
        if wait_until_accepting(&host, port, READY_TIMEOUT).await {
            info!(host = %host, port, "preview server accepting connections; opening browser");
        } else {
            tracing::warn!(
                host = %host,
                port,
                timeout_secs = READY_TIMEOUT.as_secs(),
                "preview server has not accepted a connection within the timeout; \
                 opening the browser anyway (it may need a manual reload)"
            );
        }
        open_browser_or_log(&url, browser.as_deref());
    });
}

/// bd-a6dvrdg1: poll `host:port` with `TcpStream::connect` until the
/// preview server is accepting connections, or `total_timeout` elapses.
///
/// Returns `true` as soon as a connection succeeds, `false` if the
/// deadline passes first. A successful connect means the server's
/// listener is bound and the kernel is completing TCP handshakes — the
/// real readiness threshold. (Per the plan: `tokio`'s `TcpListener::bind`
/// also `listen`s, so the kernel backlogs connections from bind onward,
/// even before `axum::serve` runs its accept loop. So bind, not the
/// accept loop, is the point past which the browser won't be refused.)
///
/// The backoff starts tight so the common ~250ms startup opens the
/// browser with no perceptible delay, and decays to a 1s cap so a slow
/// project adds at most ~1s of post-ready latency. Callers must treat a
/// `false` return as "open anyway, but warn" — never a hard failure.
///
/// Why this is a spawned-task probe rather than a readiness signal
/// threaded out of the server: it observes the externally-observable
/// accept condition, so it stays correct regardless of how the server's
/// internal startup phases are ordered, and it keeps browser-opening (a
/// CLI concern) out of the `quarto-hub` / `quarto-preview` libraries.
async fn wait_until_accepting(host: &str, port: u16, total_timeout: Duration) -> bool {
    const INITIAL_BACKOFF: Duration = Duration::from_millis(20);
    const MAX_BACKOFF: Duration = Duration::from_secs(1);

    let deadline = tokio::time::Instant::now() + total_timeout;
    let mut backoff = INITIAL_BACKOFF;

    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            return false;
        }
        // Bound each attempt so a stuck connect can't outlive the
        // deadline. Localhost connects resolve fast (refused or
        // connected); this cap is belt-and-suspenders.
        let attempt_timeout = remaining.min(MAX_BACKOFF);
        if let Ok(Ok(_stream)) = tokio::time::timeout(
            attempt_timeout,
            tokio::net::TcpStream::connect((host, port)),
        )
        .await
        {
            return true;
        }

        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            return false;
        }
        tokio::time::sleep(backoff.min(remaining)).await;
        // Grow ×1.6 per miss, capped: 20 → 32 → 51 → 81 → … → 1000ms.
        backoff = (backoff * 8 / 5).min(MAX_BACKOFF);
    }
}

/// Resolution of `args.path` into the inputs the hub needs:
/// the project root, an optional initial page hint for the SPA, and
/// an optional single-file constraint (bd-tnm3k) used to keep
/// discovery + the watcher narrow when there's no `_quarto.yml`.
#[derive(Debug)]
pub(crate) struct ResolvedProject {
    pub root: PathBuf,
    /// Page slug appended to the boot URL as `?page=<rel>` so the
    /// SPA's `pickInitialPage` helper can seed `activeFile`.
    pub initial_page: Option<String>,
    /// When `Some(rel)`, `project_root.join(rel)` is the one file
    /// `q2 preview` was invoked on, and the hub must skip the
    /// directory walk + watch only that file (bd-tnm3k).
    pub single_file: Option<PathBuf>,
}

/// Phase D.2 (bd-kw93.13) + bd-tnm3k: resolve `args.path` to a
/// [`ResolvedProject`]. Three cases:
///
/// 1. **`canonical` is a directory.** Project mode. The directory is
///    the project root. If `index.qmd` exists at the root, prefer
///    it; otherwise leave `initial_page = None` and let the SPA fall
///    through to its own selection.
/// 2. **`canonical` is a file inside a `_quarto.yml` project.** Walk
///    up from the file's parent looking for `_quarto.yml`. When
///    found, that ancestor is the project root and the file's path
///    relative to it is the initial page. This is the case that
///    `q2 preview posts/intro.qmd` is supposed to do something
///    useful with.
/// 3. **`canonical` is a file with no `_quarto.yml` ancestor.**
///    Single-file mode (bd-tnm3k): `project_root` is the file's
///    *parent directory* (the file path itself made
///    `project_root.join("")` produce an ENOTDIR path), and
///    `single_file` carries the file's basename so the hub
///    constrains discovery + the watcher to just that one file.
pub(crate) fn resolve_project_and_initial_page(canonical: &Path) -> Result<ResolvedProject> {
    let metadata = std::fs::metadata(canonical)
        .with_context(|| format!("reading metadata of {}", canonical.display()))?;

    if metadata.is_dir() {
        // Prefer `index.qmd`; fall back to `index.md` (bd-6d2wj4zp Phase 5 —
        // a render-list `.md` can be the landing page). No index file →
        // None, and the SPA falls through to its own selection.
        let initial = ["index.qmd", "index.md"]
            .into_iter()
            .find(|name| canonical.join(name).is_file())
            .map(str::to_string);
        return Ok(ResolvedProject {
            root: canonical.to_path_buf(),
            initial_page: initial,
            single_file: None,
        });
    }

    if metadata.is_file() {
        let parent = canonical
            .parent()
            .ok_or_else(|| anyhow::anyhow!("file path has no parent: {}", canonical.display()))?;
        if let Some(project_dir) = find_project_root_above(parent) {
            // `_quarto.yml` ancestor found → multi-file project mode.
            // Compute the path relative to the project root using
            // forward slashes (the SPA's index always stores paths
            // that way regardless of platform).
            let rel = canonical
                .strip_prefix(&project_dir)
                .with_context(|| {
                    format!(
                        "file {} is not under project root {}",
                        canonical.display(),
                        project_dir.display()
                    )
                })?
                .to_string_lossy()
                .replace('\\', "/");
            return Ok(ResolvedProject {
                root: project_dir,
                initial_page: Some(rel),
                single_file: None,
            });
        }
        // bd-tnm3k: no `_quarto.yml` ancestor → single-file mode.
        // The parent directory is the project root; the file's
        // basename is both the initial page hint and the single-file
        // constraint that keeps discovery + watcher narrow.
        let filename = canonical
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or_else(|| anyhow::anyhow!("file path has no filename: {}", canonical.display()))?;
        return Ok(ResolvedProject {
            root: parent.to_path_buf(),
            initial_page: Some(filename.to_string()),
            single_file: Some(PathBuf::from(filename)),
        });
    }

    anyhow::bail!(
        "path {} is neither a regular file nor a directory",
        canonical.display()
    )
}

/// Walk up from `start` (a directory) looking for the nearest ancestor
/// that contains `_quarto.yml`. Returns the ancestor directory, or
/// `None` if the walk reaches the filesystem root without finding one.
fn find_project_root_above(start: &Path) -> Option<PathBuf> {
    let mut current = Some(start.to_path_buf());
    while let Some(dir) = current {
        if dir.join("_quarto.yml").is_file() {
            return Some(dir);
        }
        current = dir.parent().map(|p| p.to_path_buf());
    }
    None
}

/// Build the boot URL with an optional `?page=<rel>` query. The
/// relative path is percent-encoded so paths with spaces or `/`
/// survive the round-trip.
pub(crate) fn build_boot_url(host: &str, port: u16, initial_page: Option<&str>) -> String {
    let base = format!("http://{host}:{port}/");
    match initial_page {
        Some(rel) if !rel.is_empty() => {
            format!("{base}?page={}", percent_encode_path(rel))
        }
        _ => base,
    }
}

/// Minimal RFC 3986 component encoder. Leaves unreserved characters
/// (`A-Z a-z 0-9 - _ . ~`) and `/` alone (paths read more naturally
/// with literal slashes); percent-encodes everything else. Avoids
/// pulling in a full URL crate just for one helper.
fn percent_encode_path(s: &str) -> String {
    percent_encode(s, true)
}

/// Strict variant of [`percent_encode_path`] for query-param *values*:
/// also encodes `/`, matching what `URLSearchParams.toString()` emits
/// on the hub-client side (its parser is what decodes these).
fn percent_encode_component(s: &str) -> String {
    percent_encode(s, false)
}

fn percent_encode(s: &str, keep_slash: bool) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        let unreserved = b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~');
        if unreserved || (keep_slash && b == b'/') {
            out.push(b as char);
        } else {
            use std::fmt::Write;
            write!(out, "%{:02X}", b).expect("writing to String never fails");
        }
    }
    out
}

/// Phase 4 (bd-jt1etjbn): build the `--ui editor` boot URL — the
/// hub-client share route against this host's own origin. See
/// [`editor_share_route`] for the route shape and param contract.
pub(crate) fn build_editor_boot_url(
    host: &str,
    port: u16,
    index_doc_id: &str,
    file: &str,
    project_name: &str,
) -> String {
    format!(
        "http://{host}:{port}{}",
        editor_share_route(index_doc_id, file, project_name)
    )
}

/// The hub-client share route both editor boot-URL builders emit (host
/// above, guest below). Single source for the route shape: the params
/// ride the *hash fragment* (the SPA router parses `location.hash`, not
/// the URL query; `hub-client/src/utils/routing.ts`), the client's
/// validation requires `server` / `file` / `name`, and `server=%2Fws`
/// is the relative sync endpoint hub-client resolves against the page
/// origin — the preview server for the host, the local tunnel proxy
/// for a `--join` guest. The doc id travels bare: the client re-adds
/// the `automerge:` prefix, and `buildShareableUrl` on the TS side
/// strips it symmetrically.
///
/// `ephemeral=true` (bd-zf4ryvuq) marks the serving hub as a throwaway
/// per-session preview server: the client captures the flag before the
/// share handler clears the URL, silently establishes a project-set
/// root against `/ws`, and skips the setup/migration gate so the user
/// lands straight in the preview. Only preview boot URLs carry it —
/// `buildShareableUrl` never emits it.
fn editor_share_route(index_doc_id: &str, file: &str, project_name: &str) -> String {
    let doc_id = index_doc_id
        .strip_prefix("automerge:")
        .unwrap_or(index_doc_id);
    format!(
        "/#/share/{}?server=%2Fws&file={}&name={}&ephemeral=true",
        percent_encode_component(doc_id),
        percent_encode_component(file),
        percent_encode_component(project_name),
    )
}

/// bd-7htq16rx: build the `--join` guest's boot URL — the same share
/// route the host prints, but against the guest's local proxy origin.
/// The params arrive from the host's `/api/preview/config` through the
/// tunnel ([`fetch_editor_boot`]); `server=%2Fws` resolves against the
/// page origin, i.e. the proxy, so the guest's hub-client syncs with
/// the host through the tunnel.
fn build_guest_editor_url(
    addr: &std::net::SocketAddr,
    boot: &quarto_preview::EditorBootInfo,
) -> String {
    format!(
        "http://{addr}{}",
        editor_share_route(&boot.index_doc_id, &boot.file, &boot.name)
    )
}

/// Choose the share route's `file` param: the CLI-resolved initial
/// page when there is one, else a root `index.qmd` when the project
/// has one (the front door of a website project), else the
/// lexicographically first `.qmd` known to the index (its files map is
/// unordered; taking the minimum keeps the boot deterministic). `None`
/// when the project has no `.qmd` at all.
pub(crate) fn pick_editor_file(
    initial_page: Option<&str>,
    index_paths: &[String],
) -> Option<String> {
    if let Some(page) = initial_page {
        return Some(page.to_string());
    }
    if index_paths.iter().any(|p| p == "index.qmd") {
        return Some("index.qmd".to_string());
    }
    index_paths
        .iter()
        .filter(|p| p.ends_with(".qmd"))
        .min()
        .cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_explicit_port_ok_for_zero() {
        // Port 0 always succeeds (OS picks). Cheap smoke that the
        // helper doesn't reject a valid request.
        validate_explicit_port("127.0.0.1", 0).expect("port 0 should always be bindable");
    }

    #[test]
    fn validate_explicit_port_errors_clearly_for_bound_port() {
        // Hold a listener on an OS-assigned port, then ask the
        // helper to validate that same port. The probe must fail
        // with the friendly message that names `--port 0`.
        let held = StdTcpListener::bind("127.0.0.1:0").expect("probe bind");
        let port = held.local_addr().expect("local_addr").port();

        let err = validate_explicit_port("127.0.0.1", port)
            .expect_err("port should be unavailable while we hold the listener");
        let msg = format!("{err}");
        assert!(
            msg.contains(&port.to_string()),
            "error should name the port; got: {msg}"
        );
        assert!(
            msg.contains("--port 0"),
            "error should suggest the --port 0 escape hatch; got: {msg}"
        );
    }

    // ──────────────────────────────────────────────────────────────
    // Phase D.2 (bd-kw93.13): resolve_project_and_initial_page +
    //                        build_boot_url + percent_encode_path
    // ──────────────────────────────────────────────────────────────

    use tempfile::TempDir;

    fn canonical_dir(t: &TempDir) -> std::path::PathBuf {
        t.path()
            .canonicalize()
            .expect("canonicalize tempdir for cross-platform stability")
    }

    #[test]
    fn resolve_dir_with_index_returns_index_qmd() {
        let tmp = TempDir::with_prefix("d2-dir-with-index-").unwrap();
        std::fs::write(tmp.path().join("index.qmd"), "# index").unwrap();
        std::fs::write(tmp.path().join("about.qmd"), "# about").unwrap();
        let canonical = canonical_dir(&tmp);

        let resolved =
            resolve_project_and_initial_page(&canonical).expect("dir-with-index resolves");
        assert_eq!(resolved.root, canonical);
        assert_eq!(resolved.initial_page.as_deref(), Some("index.qmd"));
        assert!(resolved.single_file.is_none());
    }

    /// bd-6d2wj4zp Phase 5: a project whose landing page is a
    /// render-list `.md` (e.g. the Connect docs port) should boot the
    /// preview on `index.md` when there is no `index.qmd`.
    #[test]
    fn resolve_dir_with_only_index_md_returns_index_md() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("index.md"), "# index").unwrap();
        std::fs::write(tmp.path().join("about.qmd"), "# about").unwrap();
        let canonical = tmp.path().canonicalize().unwrap();
        let resolved = resolve_project_and_initial_page(&canonical).unwrap();
        assert_eq!(resolved.initial_page.as_deref(), Some("index.md"));
    }

    /// `index.qmd` wins over `index.md` when both exist — `.qmd` is
    /// the canonical source; also keeps the pre-`.md` behavior stable.
    #[test]
    fn resolve_dir_prefers_index_qmd_over_index_md() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("index.qmd"), "# q").unwrap();
        std::fs::write(tmp.path().join("index.md"), "# m").unwrap();
        let canonical = tmp.path().canonicalize().unwrap();
        let resolved = resolve_project_and_initial_page(&canonical).unwrap();
        assert_eq!(resolved.initial_page.as_deref(), Some("index.qmd"));
    }

    #[test]
    fn resolve_dir_without_index_returns_none() {
        let tmp = TempDir::with_prefix("d2-dir-no-index-").unwrap();
        std::fs::write(tmp.path().join("a.qmd"), "# a").unwrap();
        std::fs::write(tmp.path().join("b.qmd"), "# b").unwrap();
        let canonical = canonical_dir(&tmp);

        let resolved =
            resolve_project_and_initial_page(&canonical).expect("dir-without-index resolves");
        assert_eq!(resolved.root, canonical);
        assert!(
            resolved.initial_page.is_none(),
            "no index.qmd → no initial_page hint; got: {:?}",
            resolved.initial_page,
        );
        assert!(resolved.single_file.is_none());
    }

    #[test]
    fn resolve_file_in_quarto_project_walks_up_to_project_root() {
        // /tmp/proj/_quarto.yml
        // /tmp/proj/posts/intro.qmd
        // `q2 preview posts/intro.qmd` → project_root = /tmp/proj,
        // initial_page = "posts/intro.qmd".
        let tmp = TempDir::with_prefix("d2-file-in-proj-").unwrap();
        let proj = canonical_dir(&tmp);
        std::fs::write(proj.join("_quarto.yml"), "project:\n  type: website\n").unwrap();
        std::fs::create_dir_all(proj.join("posts")).unwrap();
        let file = proj.join("posts").join("intro.qmd");
        std::fs::write(&file, "# intro").unwrap();

        let resolved = resolve_project_and_initial_page(&file).expect("file-in-project resolves");
        assert_eq!(resolved.root, proj);
        assert_eq!(resolved.initial_page.as_deref(), Some("posts/intro.qmd"));
        assert!(
            resolved.single_file.is_none(),
            "multi-file _quarto.yml project must not flip single-file mode",
        );
    }

    #[test]
    fn resolve_file_at_project_root_returns_filename() {
        let tmp = TempDir::with_prefix("d2-file-at-root-").unwrap();
        let proj = canonical_dir(&tmp);
        std::fs::write(proj.join("_quarto.yml"), "project: {}\n").unwrap();
        let file = proj.join("index.qmd");
        std::fs::write(&file, "# index").unwrap();

        let resolved = resolve_project_and_initial_page(&file).expect("resolves");
        assert_eq!(resolved.root, proj);
        assert_eq!(
            resolved.initial_page.as_deref(),
            Some("index.qmd"),
            "file at project root should resolve to its own filename"
        );
        assert!(resolved.single_file.is_none());
    }

    /// bd-tnm3k: when there is no `_quarto.yml` ancestor, single-file
    /// mode resolves `project_root` to the file's *parent directory*
    /// (not the file path itself, which made downstream
    /// `project_root.join(rel)` produce a trailing-slash path that
    /// tripped ENOTDIR in `reconcile_files_with_index`). The
    /// single-file constraint is carried separately so the hub does
    /// not start indexing or watching sibling files in the parent dir.
    #[test]
    fn resolve_file_without_quarto_yml_resolves_parent_as_root() {
        let tmp = TempDir::with_prefix("d2-file-no-yml-").unwrap();
        let dir = canonical_dir(&tmp);
        let file = dir.join("doc.qmd");
        std::fs::write(&file, "# doc").unwrap();
        // Deliberately NO _quarto.yml.

        let resolved = resolve_project_and_initial_page(&file).expect("single-file mode resolves");
        assert_eq!(resolved.root, dir);
        assert_eq!(resolved.initial_page.as_deref(), Some("doc.qmd"));
        assert_eq!(
            resolved.single_file.as_deref(),
            Some(std::path::Path::new("doc.qmd")),
            "single-file mode must carry the relative path so the hub \
             constrains discovery and the watcher to that one file",
        );
    }

    #[test]
    fn build_boot_url_omits_query_when_no_initial_page() {
        assert_eq!(
            build_boot_url("127.0.0.1", 8080, None),
            "http://127.0.0.1:8080/"
        );
        assert_eq!(
            build_boot_url("127.0.0.1", 8080, Some("")),
            "http://127.0.0.1:8080/",
            "empty initial_page should be omitted, not produce ?page="
        );
    }

    #[test]
    fn build_boot_url_with_initial_page_encodes_query() {
        assert_eq!(
            build_boot_url("127.0.0.1", 8080, Some("posts/intro.qmd")),
            "http://127.0.0.1:8080/?page=posts/intro.qmd",
            "literal slashes survive (paths read more naturally)"
        );
        // Space and other reserved chars get encoded.
        assert_eq!(
            build_boot_url("127.0.0.1", 8080, Some("a b.qmd")),
            "http://127.0.0.1:8080/?page=a%20b.qmd"
        );
    }

    #[test]
    fn percent_encode_path_leaves_unreserved_alone() {
        assert_eq!(percent_encode_path("simple.qmd"), "simple.qmd");
        assert_eq!(
            percent_encode_path("posts/intro-1.qmd"),
            "posts/intro-1.qmd"
        );
        assert_eq!(percent_encode_path("a_b~c.d"), "a_b~c.d");
    }

    #[test]
    fn percent_encode_path_encodes_reserved() {
        // Space, query-string-flipping `?`, and high-bit bytes.
        assert_eq!(percent_encode_path(" "), "%20");
        assert_eq!(percent_encode_path("a?b"), "a%3Fb");
        assert_eq!(percent_encode_path("a&b=c"), "a%26b%3Dc");
    }

    // ──────────────────────────────────────────────────────────────
    // Phase 4 (bd-jt1etjbn): editor boot URL — the hub-client share
    // route. Params ride the *hash fragment* (the SPA router parses
    // location.hash, hub-client/src/utils/routing.ts), the doc id is
    // bare (the client re-adds the `automerge:` prefix), and `server`
    // is the relative `/ws` the client resolves against the page
    // origin — which the preview server itself serves.
    // ──────────────────────────────────────────────────────────────

    #[test]
    fn build_editor_boot_url_emits_share_route_in_hash() {
        assert_eq!(
            build_editor_boot_url(
                "127.0.0.1",
                8080,
                "4XyZabc123",
                "posts/intro.qmd",
                "My Project"
            ),
            "http://127.0.0.1:8080/#/share/4XyZabc123?server=%2Fws&file=posts%2Fintro.qmd&name=My%20Project&ephemeral=true",
        );
    }

    #[test]
    fn build_editor_boot_url_marks_hub_ephemeral() {
        // The preview hub is a throwaway per-session server; the client
        // reads `ephemeral=true` to skip project-set onboarding and go
        // straight to the preview (bd-zf4ryvuq).
        let url = build_editor_boot_url("127.0.0.1", 8080, "4XyZ", "a.qmd", "p");
        assert!(
            url.ends_with("&ephemeral=true"),
            "preview boot URLs must carry the ephemeral flag; got {url}"
        );
    }

    #[test]
    fn build_editor_boot_url_strips_automerge_prefix() {
        // `ctx.index().document_id()` is already bare, but the share
        // route must never carry the prefix even if a caller passes it
        // (mirrors routing.ts's buildShareableUrl, which strips it).
        let url = build_editor_boot_url("127.0.0.1", 8080, "automerge:4XyZ", "a.qmd", "p");
        assert!(
            url.contains("#/share/4XyZ?"),
            "the route wants the bare doc id; got {url}"
        );
        assert!(
            !url.contains("automerge"),
            "the automerge: prefix must not leak into the URL; got {url}"
        );
    }

    // ──────────────────────────────────────────────────────────────
    // bd-7htq16rx: `--join` guests of an editor-UI host boot the same
    // share route (ephemeral flag included) so hub-client skips
    // project-set onboarding and joins the document. The boot params
    // arrive from the host's `/api/preview/config` through the tunnel.
    // ──────────────────────────────────────────────────────────────

    fn guest_addr() -> std::net::SocketAddr {
        "127.0.0.1:8080".parse().unwrap()
    }

    #[test]
    fn guest_editor_url_is_share_route_with_ephemeral_flag() {
        let boot = quarto_preview::EditorBootInfo {
            index_doc_id: "4XyZabc123".to_string(),
            file: "posts/intro.qmd".to_string(),
            name: "My Project".to_string(),
        };
        assert_eq!(
            build_guest_editor_url(&guest_addr(), &boot),
            "http://127.0.0.1:8080/#/share/4XyZabc123?server=%2Fws&file=posts%2Fintro.qmd&name=My%20Project&ephemeral=true",
        );
    }

    #[test]
    fn guest_editor_url_strips_automerge_prefix() {
        let boot = quarto_preview::EditorBootInfo {
            index_doc_id: "automerge:4XyZ".to_string(),
            file: "a.qmd".to_string(),
            name: "p".to_string(),
        };
        let url = build_guest_editor_url(&guest_addr(), &boot);
        assert!(
            url.contains("#/share/4XyZ?"),
            "the route wants the bare doc id; got {url}"
        );
        assert!(
            !url.contains("automerge"),
            "the automerge: prefix must not leak into the URL; got {url}"
        );
    }

    #[test]
    fn parse_editor_boot_reads_config_body() {
        let body = br#"{"allowEdit":false,"editorBoot":{"indexDocId":"4XyZ","file":"index.qmd","name":"proj"}}"#;
        let boot = parse_editor_boot(body).expect("editorBoot parses");
        assert_eq!(boot.index_doc_id, "4XyZ");
        assert_eq!(boot.file, "index.qmd");
        assert_eq!(boot.name, "proj");
    }

    #[test]
    fn parse_editor_boot_absent_without_the_field() {
        // Viewer-mode (and older) hosts answer config without
        // editorBoot — the guest falls back to the root URL.
        assert_eq!(parse_editor_boot(br#"{"allowEdit":false}"#), None);
    }

    #[test]
    fn parse_editor_boot_rejects_malformed_or_empty() {
        assert_eq!(parse_editor_boot(b"not json"), None);
        assert_eq!(parse_editor_boot(b""), None);
        // A boot URL with an empty doc id or file can never join.
        assert_eq!(
            parse_editor_boot(br#"{"editorBoot":{"indexDocId":"","file":"index.qmd","name":"p"}}"#),
            None
        );
        assert_eq!(
            parse_editor_boot(br#"{"editorBoot":{"indexDocId":"4XyZ","file":"","name":"p"}}"#),
            None
        );
    }

    #[test]
    fn pick_editor_file_prefers_initial_page() {
        let files = vec!["about.qmd".to_string(), "index.qmd".to_string()];
        assert_eq!(
            pick_editor_file(Some("posts/intro.qmd"), &files).as_deref(),
            Some("posts/intro.qmd"),
            "an initial page resolved by the CLI wins over the index scan"
        );
    }

    #[test]
    fn pick_editor_file_prefers_root_index_qmd_over_sorted_first() {
        // A website project's front door wins over the deterministic
        // sorted-first fallback even when it isn't alphabetically first.
        let files = vec!["about.qmd".to_string(), "index.qmd".to_string()];
        assert_eq!(pick_editor_file(None, &files).as_deref(), Some("index.qmd"));
        // Only a *root* index.qmd gets the preference — a nested one is
        // just another page.
        let files = vec!["about.qmd".to_string(), "posts/index.qmd".to_string()];
        assert_eq!(pick_editor_file(None, &files).as_deref(), Some("about.qmd"));
    }

    #[test]
    fn pick_editor_file_falls_back_to_first_qmd_sorted() {
        // The index files map is unordered (HashMap); the fallback must
        // sort so the chosen page is deterministic across boots.
        let files = vec![
            "zeta.qmd".to_string(),
            "styles.css".to_string(),
            "about.qmd".to_string(),
        ];
        assert_eq!(pick_editor_file(None, &files).as_deref(), Some("about.qmd"));
    }

    #[test]
    fn pick_editor_file_none_without_any_qmd() {
        let files = vec!["styles.css".to_string()];
        assert_eq!(pick_editor_file(None, &files), None);
        assert_eq!(pick_editor_file(None, &[]), None);
    }

    // ──────────────────────────────────────────────────────────────
    // bd-a6dvrdg1: wait_until_accepting — gate the browser-open on the
    // preview server actually accepting connections (not on the
    // throwaway port probe, which is dropped before the server binds).
    // See claude-notes/plans/2026-06-15-q2-preview-browser-race.md.
    // ──────────────────────────────────────────────────────────────

    /// Bind on an OS-assigned port, read it, drop the listener.
    /// Returns a port that was free a moment ago (a tiny TOCTOU window,
    /// acceptable for these timing tests).
    fn reserve_free_port() -> u16 {
        let l = StdTcpListener::bind("127.0.0.1:0").expect("probe bind");
        l.local_addr().expect("local_addr").port()
        // listener drops here
    }

    #[tokio::test]
    async fn wait_until_accepting_returns_true_when_listener_present() {
        // A bound listener — even one that never calls accept() — is
        // enough: the kernel completes the TCP handshake into the
        // backlog, so connect() succeeds. This is the exact readiness
        // threshold the fix relies on (bind, not the accept loop).
        let listener = StdTcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("local_addr").port();

        let start = std::time::Instant::now();
        let ready =
            wait_until_accepting("127.0.0.1", port, std::time::Duration::from_secs(5)).await;
        let elapsed = start.elapsed();

        assert!(ready, "a bound listener should be reported as accepting");
        assert!(
            elapsed < std::time::Duration::from_secs(1),
            "should connect on the first (fast) attempt; elapsed={elapsed:?}"
        );
    }

    #[tokio::test]
    async fn wait_until_accepting_times_out_when_nothing_listening() {
        // Nothing is listening on a just-freed port → connect keeps
        // failing → we return false only after the total timeout.
        let port = reserve_free_port();

        let timeout = std::time::Duration::from_millis(150);
        let start = std::time::Instant::now();
        let ready = wait_until_accepting("127.0.0.1", port, timeout).await;
        let elapsed = start.elapsed();

        assert!(!ready, "no listener → must report not-accepting");
        assert!(
            elapsed >= timeout,
            "must wait out the full timeout before giving up; elapsed={elapsed:?}, timeout={timeout:?}"
        );
    }

    #[tokio::test]
    async fn wait_until_accepting_unblocks_when_listener_appears_late() {
        // The direct regression for the race: the server's listener
        // shows up *after* we start waiting. We must keep polling and
        // return true once it binds — not give up early, not return
        // before it appears.
        let port = reserve_free_port();

        // Bind the "server" ~150ms from now and hold it open well past
        // the expected connect so the handshake has something to land on.
        let late = tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(150)).await;
            let listener = StdTcpListener::bind(("127.0.0.1", port)).expect("late bind");
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            drop(listener);
        });

        let start = std::time::Instant::now();
        let ready =
            wait_until_accepting("127.0.0.1", port, std::time::Duration::from_secs(5)).await;
        let elapsed = start.elapsed();

        assert!(ready, "should connect once the late listener binds");
        assert!(
            elapsed >= std::time::Duration::from_millis(120),
            "should have waited for the late bind rather than returning instantly; elapsed={elapsed:?}"
        );
        assert!(
            elapsed < std::time::Duration::from_secs(5),
            "should return well before the timeout once the listener is up; elapsed={elapsed:?}"
        );
        late.abort();
    }

    // ──────────────────────────────────────────────────────────────
    // Phase 3 (bd-6y0p1bne): wait_until_healthy — join mode's
    // browser-open gate is an HTTP /health roundtrip through the local
    // proxy, not a bare TCP accept (which the proxy always grants,
    // even when the tunnel's far side is gone).
    // ──────────────────────────────────────────────────────────────

    /// Serve a fixed HTTP/1.1 response to every connection on a fresh
    /// loopback port; returns the bound address. Task dies with the test.
    async fn spawn_canned_http(response: &'static str) -> std::net::SocketAddr {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind canned server");
        let addr = listener.local_addr().expect("local_addr");
        tokio::spawn(async move {
            loop {
                let Ok((mut sock, _)) = listener.accept().await else {
                    break;
                };
                tokio::spawn(async move {
                    let mut buf = [0u8; 1024];
                    let _ = sock.read(&mut buf).await;
                    let _ = sock.write_all(response.as_bytes()).await;
                });
            }
        });
        addr
    }

    #[tokio::test]
    async fn wait_until_healthy_true_on_200() {
        let addr = spawn_canned_http(
            "HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok",
        )
        .await;
        assert!(
            wait_until_healthy(addr, std::time::Duration::from_secs(5)).await,
            "a 200 /health must count as ready"
        );
    }

    #[tokio::test]
    async fn wait_until_healthy_false_on_non_200() {
        // A reachable server that answers 503 (e.g. the proxy is up but
        // the host hub is not) must NOT count as ready.
        let addr = spawn_canned_http(
            "HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        )
        .await;
        let timeout = std::time::Duration::from_millis(300);
        let start = std::time::Instant::now();
        let ready = wait_until_healthy(addr, timeout).await;
        assert!(!ready, "non-200 answers must not count as healthy");
        assert!(
            start.elapsed() >= timeout,
            "must keep polling until the deadline in case health recovers"
        );
    }

    #[tokio::test]
    async fn wait_until_healthy_false_when_nothing_listening() {
        let port = reserve_free_port();
        let addr: std::net::SocketAddr = format!("127.0.0.1:{port}").parse().unwrap();
        let ready = wait_until_healthy(addr, std::time::Duration::from_millis(200)).await;
        assert!(!ready, "no listener → not healthy");
    }
}
