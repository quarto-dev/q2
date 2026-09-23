//! `q2 preview --static` — the Quarto-1-style preview (bd-sl79jjiq).
//!
//! Render the document or project to disk through the same path `q2
//! render` uses ([`render_once`]), serve the output directory over a
//! static HTTP server (`quarto_preview::static_mode`), and — unless
//! `--no-watch` — watch the sources, re-render what changed, and push a
//! reload to the browser. None of the hub / SPA / WASM machinery behind
//! the default `q2 preview` is involved.
//!
//! Plan: `claude-notes/plans/2026-09-22-q2-preview-static.md`.
//!
//! Shape of the loop (§ Watch policy of the plan): every watcher event
//! is classified against the *last* render's inputs and outputs into
//! ignore / subset / full; actions that arrive while a render is in
//! flight are merged into one pending action and run afterwards, so a
//! burst of saves costs one render. A finished render broadcasts
//! `render-stop` (with the plain-text diagnostics on failure) and, on
//! success, `reload` — with the changed page as the navigation target
//! when exactly one input changed and `--no-navigate` is not set.
//!
//! Code execution is lazy (plan § Lazy code execution): every render
//! runs with `ExecutionPolicy::Only(executed)`, the set of pages the
//! user has viewed. The server reports each page view; the first view
//! of a page the last render left inert adds it to the set and
//! re-renders it, so the browser shows the inert page, the badge, then
//! the executed page. `preview.engine: off` in `_quarto.yml` disables
//! execution entirely.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use quarto_core::engine::ExecutionPolicy;
use quarto_core::format::FormatIdentifier;
use quarto_hub::watch::{FileWatcher, WatchConfig, WatchEvent, WatchFilter};
use quarto_preview::EnginePolicy;
use quarto_preview::config::{
    read_engine_policy_from_project, read_static_preview_defaults_from_project,
};
use quarto_preview::static_mode::{
    Action, ContentTracker, ReloadEvent, ReloadHub, StaticServerConfig, WatchContext, build_router,
    classify, is_config_like, is_input_extension, serve,
};
use quarto_system_runtime::NativeRuntime;
use tracing::{debug, info, warn};

use super::preview::spawn_browser_open_when_ready;
use super::render::{
    RenderAbort, RenderArgs, RenderReport, RenderTarget, detect_single_input_format,
    exit_with_abort, find_project_root_upward, present_report, render_once, resolve_format,
    strip_ansi_escapes,
};

/// Concrete shape passed through from clap (`Commands::Preview` with
/// `--static`). The hub-mode flags never reach here: clap rejects them
/// next to `--static` via `conflicts_with_all`.
pub struct StaticArgs {
    /// Project root or single file to preview. Default: current dir.
    pub path: Option<PathBuf>,
    /// Port to listen on. Default: probe an OS-assigned free port.
    pub port: Option<u16>,
    /// Host to bind to. Default: 127.0.0.1 (loopback only).
    pub host: Option<String>,
    /// Skip the browser-open step.
    pub no_browser: bool,
    /// Open the preview in this browser instead of the system default.
    pub browser: Option<String>,
    /// Render once and serve; never watch or re-render.
    pub no_watch: bool,
    /// After a re-render, reload in place instead of navigating to the
    /// changed page.
    pub no_navigate: bool,
    /// Explicit render format (`--to`), as `q2 render --to`.
    pub to: Option<String>,
}

/// Formats a static server can usefully show. `docx`/`pptx`/`epub`/
/// `typst` render fine but produce nothing to browse, so they are
/// refused up front rather than after a render (plan § CLI). `None`
/// means nothing decided the format yet (no `--to`, no front matter),
/// which resolves to `html`.
fn check_servable_format(to: Option<&str>) -> Result<()> {
    let Some(to) = to else {
        return Ok(());
    };
    let format = resolve_format(to)?;
    if matches!(
        format.identifier,
        FormatIdentifier::Html | FormatIdentifier::Revealjs
    ) {
        return Ok(());
    }
    anyhow::bail!(
        "Format '{}' is not supported with --static (only html and revealjs can be served)",
        format.identifier
    );
}

pub fn execute(args: StaticArgs) -> Result<()> {
    check_servable_format(args.to.as_deref())?;
    // Multi-threaded runtime: the server, the watcher, and the blocking
    // render thread all run at once.
    let runtime = tokio::runtime::Runtime::new()?;
    // bd-hxhnnlzs: hold a kernel scope for the whole session so
    // re-renders reuse warm Jupyter kernels. It drops after `block_on`
    // returns, outside the runtime, so kernels get the polite
    // `shutdown_request` path before the kill backstop.
    let _kernel_scope = quarto_core::engine::jupyter::kernel_scope();
    runtime.block_on(run(args))
}

/// The outcome of one render on the blocking thread.
enum RenderOutcome {
    Report(Box<RenderReport>),
    Abort(RenderAbort),
    /// The render thread panicked. The server keeps running; the
    /// message goes to the terminal and the browser panel.
    Panicked(String),
}

/// Run `render_once` on a blocking thread, presenting to the terminal
/// exactly as `q2 render` does.
async fn render_blocking(args: RenderArgs) -> RenderOutcome {
    let join = tokio::task::spawn_blocking(move || {
        render_once(&args, &mut |report| present_report(report, &args))
    });
    match join.await {
        Ok(Ok(report)) => RenderOutcome::Report(Box::new(report)),
        Ok(Err(abort)) => RenderOutcome::Abort(abort),
        Err(e) => {
            let message = match e.try_into_panic() {
                Ok(payload) => payload
                    .downcast_ref::<String>()
                    .cloned()
                    .or_else(|| payload.downcast_ref::<&str>().map(|s| s.to_string()))
                    .unwrap_or_else(|| "unknown panic payload".to_string()),
                Err(join_error) => join_error.to_string(),
            };
            RenderOutcome::Panicked(message)
        }
    }
}

/// The execution policy for the next render: nothing when the project
/// says `preview.engine: off`, otherwise exactly the pages viewed so far.
fn policy_for(executed: &BTreeSet<PathBuf>, engine_off: bool) -> ExecutionPolicy {
    if engine_off {
        ExecutionPolicy::None
    } else {
        ExecutionPolicy::Only(executed.clone())
    }
}

/// `RenderArgs` for one pass of the loop: the whole target for a full
/// render, the changed inputs for a subset render.
fn render_args_for(
    action: &Action,
    full_input: &Path,
    to: Option<&str>,
    execution_policy: ExecutionPolicy,
) -> RenderArgs {
    let inputs = match action {
        Action::Subset(paths) => paths
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect(),
        Action::Full | Action::Ignore => vec![full_input.to_string_lossy().into_owned()],
    };
    RenderArgs {
        inputs,
        to: to.map(str::to_string),
        execution_policy,
        ..RenderArgs::default()
    }
}

/// What the loop remembers from the last finished render.
struct LastRender {
    ctx: WatchContext,
    /// `(input, output)` pairs, for mapping a viewed page back to its
    /// source.
    outputs: Vec<(PathBuf, PathBuf)>,
    /// Inputs rendered inert because the policy excluded them.
    unexecuted: BTreeSet<PathBuf>,
}

impl LastRender {
    fn from_report(report: &RenderReport) -> Self {
        Self {
            ctx: watch_context(report),
            outputs: report.outputs(),
            unexecuted: report.unexecuted_inputs(),
        }
    }

    /// The input whose output is `output`, if the last render wrote it.
    fn input_for_output(&self, output: &Path) -> Option<PathBuf> {
        self.outputs
            .iter()
            .find(|(_, o)| o == output)
            .map(|(i, _)| i.clone())
    }
}

/// The URL path of `input`'s output, relative to the served root
/// (`/posts/foo.html`), when this render produced one.
fn output_url_for(report: &RenderReport, input: &Path) -> Option<String> {
    report
        .outputs()
        .into_iter()
        .find(|(i, _)| i == input)
        .and_then(|(_, output)| output_rel(report, &output))
        .map(|rel| format!("/{rel}"))
}

/// `output` relative to the served root, with forward slashes.
fn output_rel(report: &RenderReport, output: &Path) -> Option<String> {
    let rel = output.strip_prefix(&report.output_dir).ok()?;
    Some(
        rel.components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/"),
    )
}

fn watch_context(report: &RenderReport) -> WatchContext {
    WatchContext {
        project_dir: report.project_dir.clone(),
        output_dir: report.output_dir.clone(),
        inputs: report.inputs.iter().cloned().collect(),
        outputs: report.outputs().into_iter().map(|(_, o)| o).collect(),
    }
}

/// Ctrl-C (and SIGTERM on unix). Installed with [`Self::install`]
/// *before* the port opens, so a signal that arrives the instant a
/// client can connect is caught rather than killing the process with
/// the default disposition.
struct ShutdownSignal {
    #[cfg(unix)]
    interrupt: tokio::signal::unix::Signal,
    #[cfg(unix)]
    terminate: tokio::signal::unix::Signal,
    #[cfg(windows)]
    ctrl_c: tokio::signal::windows::CtrlC,
}

impl ShutdownSignal {
    fn install() -> Result<Self> {
        #[cfg(unix)]
        {
            use tokio::signal::unix::{SignalKind, signal};
            Ok(Self {
                interrupt: signal(SignalKind::interrupt()).context("installing SIGINT handler")?,
                terminate: signal(SignalKind::terminate()).context("installing SIGTERM handler")?,
            })
        }
        #[cfg(windows)]
        {
            Ok(Self {
                ctrl_c: tokio::signal::windows::ctrl_c().context("installing Ctrl-C handler")?,
            })
        }
    }

    async fn wait(&mut self) {
        #[cfg(unix)]
        {
            tokio::select! {
                _ = self.interrupt.recv() => {}
                _ = self.terminate.recv() => {}
            }
        }
        #[cfg(windows)]
        {
            let _ = self.ctrl_c.recv().await;
        }
    }
}

async fn run(args: StaticArgs) -> Result<()> {
    let raw = args
        .path
        .clone()
        .unwrap_or_else(|| std::env::current_dir().expect("cwd"));
    let path = raw
        .canonicalize()
        .with_context(|| format!("resolving {}", raw.display()))?;
    let path_arg = path.to_string_lossy().into_owned();

    // The front-matter format is decided before any render happens, so
    // a typst document is refused without first producing a PDF.
    let effective_format = args
        .to
        .clone()
        .or_else(|| detect_single_input_format(std::slice::from_ref(&path_arg)));
    check_servable_format(effective_format.as_deref())?;

    // A file inside a project previews the *project*, opened on that
    // page (Q1 `cmd.ts:362-390`): the whole site renders, so sidebars,
    // listings and cross-page links are all live. A directory, or a
    // file with no `_quarto.yml` above it, is rendered as given.
    let full_input = match path.parent() {
        Some(parent) if path.is_file() => {
            match find_project_root_upward(parent, &NativeRuntime::new()) {
                Ok(Some(root)) => root,
                _ => path.clone(),
            }
        }
        _ => path.clone(),
    };

    // `preview.engine: off` (the hub preview's knob, shared here) turns
    // lazy execution off entirely: code cells stay inert.
    let policy_root = if full_input.is_dir() {
        full_input.clone()
    } else {
        full_input
            .parent()
            .map_or_else(|| full_input.clone(), Path::to_path_buf)
    };
    let engine_off = matches!(
        read_engine_policy_from_project(&policy_root, &NativeRuntime::new()),
        EnginePolicy::Off
    );
    // Q1's `project: preview:` keys are the defaults; CLI flags win
    // (plan Phase 4). `--no-watch` / `--no-navigate` / `--no-browser`
    // only ever turn things off, so a config `false` cannot be undone
    // from the command line — the same shape as Q1's flags.
    let defaults = read_static_preview_defaults_from_project(&policy_root, &NativeRuntime::new());
    for key in &defaults.unsupported {
        eprintln!(
            "warning: `project.preview.{key}` in _quarto.yml is not supported by \
             q2 preview --static and was ignored"
        );
    }
    let host = args
        .host
        .clone()
        .or_else(|| defaults.host.clone())
        .unwrap_or_else(|| "127.0.0.1".to_string());
    let open_browser =
        !args.no_browser && (args.browser.is_some() || defaults.browser.unwrap_or(true));
    let watching = !args.no_watch && defaults.watch_inputs.unwrap_or(true);
    let navigate = !args.no_navigate && defaults.navigate.unwrap_or(true);
    // Pages the user has viewed; the only ones that execute code.
    let mut executed: BTreeSet<PathBuf> = BTreeSet::new();

    // ── Boot render ────────────────────────────────────────────────
    let boot_args = render_args_for(
        &Action::Full,
        &full_input,
        args.to.as_deref(),
        policy_for(&executed, engine_off),
    );
    let report = match render_blocking(boot_args).await {
        RenderOutcome::Report(report) => *report,
        RenderOutcome::Abort(abort) => {
            // Fails the way `q2 render` would: structured errors print
            // and exit 1, the rest surface through `main`.
            let args_for_exit = render_args_for(
                &Action::Full,
                &full_input,
                args.to.as_deref(),
                policy_for(&executed, engine_off),
            );
            return Err(exit_with_abort(abort, &args_for_exit));
        }
        RenderOutcome::Panicked(message) => anyhow::bail!("render panicked: {message}"),
    };
    if report.exit_nonzero {
        // Per-page failures are not fatal: the pages that rendered are
        // served and the browser panel shows the diagnostics (plan
        // § Error handling matrix, Q5).
        warn!("the initial render reported errors; serving what rendered");
    }

    // ── Watcher and signals, before anyone can connect ─────────────
    // Order matters: the port opens last. A client that connects the
    // moment the port is up may edit a file at once, and macOS FSEvents
    // only reports changes made after the stream exists; likewise a
    // Ctrl-C right after connecting must find its handler installed.
    let mut watcher = if watching {
        Some(start_watcher(&report)?)
    } else {
        None
    };
    let mut shutdown = ShutdownSignal::install()?;

    // ── Server ─────────────────────────────────────────────────────
    // Bind first, print second: the listener is ours, so there is no
    // probe-then-rebind gap for another process to slip into (the
    // hub-mode preview has to probe because the hub binds internally).
    let requested_port = args.port.or(defaults.port).unwrap_or(0);
    let listener = match tokio::net::TcpListener::bind((host.as_str(), requested_port)).await {
        Ok(listener) => listener,
        Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => anyhow::bail!(
            "port {requested_port} on {host} is already in use; pass --port 0 to let the OS \
             pick a free port, or omit --port for the default behaviour"
        ),
        Err(e) => return Err(e).with_context(|| format!("binding {host}:{requested_port}")),
    };
    let port = listener
        .local_addr()
        .context("local_addr of the bound listener")?
        .port();
    // Where the browser opens: the requested page's output when a file
    // was named, else the root (which serves index.html or, failing
    // that, redirects to the first rendered page).
    let requested_page = if path.is_file() {
        output_url_for(&report, &path).map(|url| url.trim_start_matches('/').to_string())
    } else {
        None
    };
    let first_output = report
        .outputs()
        .first()
        .and_then(|(_, o)| output_rel(&report, o));
    let default_file = requested_page.clone().or(first_output);
    let hub = ReloadHub::new();
    let (page_tx, mut page_rx) = tokio::sync::mpsc::channel::<PathBuf>(64);
    let router = build_router(
        StaticServerConfig {
            root: report.output_dir.clone(),
            default_file,
            page_requests: Some(page_tx),
        },
        hub.clone(),
    );
    let url = format!(
        "http://{host}:{port}/{}",
        requested_page
            .as_deref()
            .filter(|p| *p != "index.html")
            .unwrap_or("")
    );
    info!(%url, "starting q2 preview --static server");
    println!();
    println!("  q2 preview --static");
    println!("  → {url}");
    println!("  Serving {}", report.output_dir.display());
    if watching {
        println!(
            "  Watching {} for changes (Ctrl-C to stop)",
            report.project_dir.display()
        );
    } else {
        println!("  --no-watch: serving this render as is (Ctrl-C to stop)");
    }
    println!();
    if open_browser {
        spawn_browser_open_when_ready(host.clone(), port, url.clone(), args.browser.clone());
    }

    let (stop_server_tx, stop_server_rx) = tokio::sync::oneshot::channel::<()>();
    let server = tokio::spawn(serve(router, listener, async move {
        let _ = stop_server_rx.await;
    }));

    // ── Loop ───────────────────────────────────────────────────────
    let to = args.to.clone();
    let mut last = LastRender::from_report(&report);
    // Events are not edits: a render reading a file raises one on Linux
    // (`notify` subscribes to inotify OPEN), and editors touch files
    // without changing them. Seed with what the boot render read so its
    // own reads never trigger the first re-render.
    let mut tracker = ContentTracker::default();
    tracker.seed(
        report
            .inputs
            .iter()
            .chain(report.config_sources.iter())
            .cloned(),
    );
    let mut pending = Action::Ignore;
    let mut in_flight: Option<Action> = None;
    let (done_tx, mut done_rx) = tokio::sync::mpsc::channel::<(Action, RenderOutcome)>(1);

    // The page the browser opens on counts as viewed before it loads:
    // when it has code, its execution starts now, and the browser
    // either gets the executed page or an inert one plus a reload.
    if !engine_off
        && let Some(page) = &requested_page
        && let Some(input) = last.input_for_output(&report.output_dir.join(page))
        && last.unexecuted.contains(&input)
    {
        executed.insert(input.clone());
        pending = Action::Subset(BTreeSet::from([input]));
    }
    if pending != Action::Ignore {
        start_render(
            &mut pending,
            &mut in_flight,
            &full_input,
            to.as_deref(),
            policy_for(&executed, engine_off),
            &hub,
            &done_tx,
        );
    }

    loop {
        tokio::select! {
            _ = shutdown.wait() => break,
            Some(output) = page_rx.recv() => {
                // First view of a page the last render left inert: it
                // joins the executed set and re-renders. Anything else
                // (no code, already executed, unknown output) is free.
                let Some(input) = last.input_for_output(&output) else { continue };
                if engine_off || !last.unexecuted.contains(&input) {
                    continue;
                }
                info!(page = %input.display(), "first view of a page with code; executing it");
                executed.insert(input.clone());
                pending = std::mem::replace(&mut pending, Action::Ignore)
                    .merge(Action::Subset(BTreeSet::from([input])));
                if in_flight.is_none() {
                    start_render(
                        &mut pending,
                        &mut in_flight,
                        &full_input,
                        to.as_deref(),
                        policy_for(&executed, engine_off),
                        &hub,
                        &done_tx,
                    );
                }
            }
            event = watcher_recv(&mut watcher) => {
                let Some(WatchEvent::Modified(changed)) = event else {
                    // The watcher stopped; keep serving without it.
                    warn!("filesystem watcher stopped; further edits will not re-render");
                    watcher = None;
                    continue;
                };
                let action = classify(&changed, &last.ctx);
                if action == Action::Ignore {
                    continue;
                }
                if !tracker.changed(&changed) {
                    debug!(path = %changed.display(), "event without a content change; ignored");
                    continue;
                }
                // Mid-render, a `Full` from a path that is neither an
                // input nor config is presumed to be the render's own
                // write (an engine cache, a sidecar in an unexpected
                // place). Inputs and config are never dropped.
                if in_flight.is_some()
                    && action == Action::Full
                    && !last.ctx.inputs.contains(&changed)
                    && !is_input_extension(&changed)
                    && !is_config_like(&changed)
                {
                    debug!(path = %changed.display(), "dropping change that arrived mid-render");
                    continue;
                }
                debug!(path = %changed.display(), ?action, "change classified");
                pending = std::mem::replace(&mut pending, Action::Ignore).merge(action);
                if in_flight.is_none() {
                    start_render(
                        &mut pending,
                        &mut in_flight,
                        &full_input,
                        to.as_deref(),
                        policy_for(&executed, engine_off),
                        &hub,
                        &done_tx,
                    );
                }
            }
            Some((ran, outcome)) = done_rx.recv() => {
                in_flight = None;
                finish_render(&ran, outcome, navigate, &hub, &mut last);
                if pending != Action::Ignore {
                    start_render(
                        &mut pending,
                        &mut in_flight,
                        &full_input,
                        to.as_deref(),
                        policy_for(&executed, engine_off),
                        &hub,
                        &done_tx,
                    );
                }
            }
        }
    }

    // ── Shutdown ───────────────────────────────────────────────────
    println!("Received Ctrl-C, shutting down the static preview");
    drop(watcher);
    hub.shutdown();
    let _ = stop_server_tx.send(());
    match tokio::time::timeout(Duration::from_secs(5), server).await {
        Ok(Ok(Ok(()))) => {}
        Ok(Ok(Err(e))) => warn!(error = %e, "server error during shutdown"),
        Ok(Err(e)) => warn!(error = %e, "server task failed"),
        Err(_) => warn!("server did not finish shutting down within 5s; exiting anyway"),
    }
    if in_flight.is_some() {
        info!("a render was still running; its output is left as is");
    }
    Ok(())
}

/// Receive from the watcher, or wait forever when there is none
/// (`--no-watch`, or the watcher stopped).
async fn watcher_recv(watcher: &mut Option<FileWatcher>) -> Option<WatchEvent> {
    match watcher.as_mut() {
        Some(w) => w.recv().await,
        None => std::future::pending().await,
    }
}

/// Project mode watches the project root recursively (the policy
/// excludes the output and generated directories). A single document
/// outside a project watches the file plus its include / image closure,
/// non-recursively — the same allow-list the hub preview uses.
fn start_watcher(report: &RenderReport) -> Result<FileWatcher> {
    let mut config = WatchConfig::with_filter(WatchFilter::All);
    if let RenderTarget::SingleDoc(input) = &report.target {
        let root = report.project_dir.clone();
        let rel = input.strip_prefix(&root).map_or_else(
            |_| PathBuf::from(input.file_name().unwrap_or_default()),
            Path::to_path_buf,
        );
        let deps = quarto_preview::config::resolve_single_file_deps(
            &root,
            &rel,
            Arc::new(NativeRuntime::new()),
        );
        config.single_file = Some(input.clone());
        config.single_file_deps = deps
            .qmd_files
            .iter()
            .chain(deps.binary_files.iter())
            .map(|d| root.join(d))
            .collect();
    }
    FileWatcher::new(&report.project_dir, config)
        .map_err(|e| anyhow::anyhow!("starting the filesystem watcher: {e}"))
}

/// Take the pending action and run it on the blocking thread.
fn start_render(
    pending: &mut Action,
    in_flight: &mut Option<Action>,
    full_input: &Path,
    to: Option<&str>,
    execution_policy: ExecutionPolicy,
    hub: &ReloadHub,
    done_tx: &tokio::sync::mpsc::Sender<(Action, RenderOutcome)>,
) {
    let action = std::mem::replace(pending, Action::Ignore);
    let args = render_args_for(&action, full_input, to, execution_policy);
    match &action {
        Action::Subset(paths) => {
            let names: BTreeSet<String> = paths
                .iter()
                .map(|p| {
                    p.file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned()
                })
                .collect();
            info!(changed = ?names, "re-rendering changed inputs");
        }
        _ => info!("re-rendering the whole project"),
    }
    hub.send(ReloadEvent::RenderStart);
    *in_flight = Some(action.clone());
    let done_tx = done_tx.clone();
    tokio::spawn(async move {
        let outcome = render_blocking(args).await;
        let _ = done_tx.send((action, outcome)).await;
    });
}

/// Broadcast a finished render's result and refresh the loop's
/// snapshot of the last render.
fn finish_render(
    ran: &Action,
    outcome: RenderOutcome,
    navigate: bool,
    hub: &ReloadHub,
    last: &mut LastRender,
) {
    match outcome {
        RenderOutcome::Report(report) => {
            let counts = report.diagnostic_counts();
            let ok = !report.exit_nonzero;
            hub.send(ReloadEvent::RenderStop {
                ok,
                errors: counts.errors,
                warnings: counts.warnings,
                text: report.diagnostics_text(false),
            });
            if ok {
                let target = match ran {
                    Action::Subset(paths) if navigate && paths.len() == 1 => paths
                        .iter()
                        .next()
                        .and_then(|input| output_url_for(&report, input)),
                    _ => None,
                };
                hub.send(ReloadEvent::Reload { target });
            }
            *last = LastRender::from_report(&report);
        }
        RenderOutcome::Abort(abort) => {
            let message = abort.user_message();
            eprintln!("{message}");
            hub.send(ReloadEvent::RenderStop {
                ok: false,
                errors: 1,
                warnings: 0,
                text: strip_ansi_escapes(&message),
            });
        }
        RenderOutcome::Panicked(message) => {
            eprintln!("Error: the render panicked: {message}");
            hub.send(ReloadEvent::RenderStop {
                ok: false,
                errors: 1,
                warnings: 0,
                text: format!("The render panicked: {message}"),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn servable_formats_pass_the_gate() {
        assert!(check_servable_format(None).is_ok());
        assert!(check_servable_format(Some("html")).is_ok());
        assert!(check_servable_format(Some("revealjs")).is_ok());
    }

    #[test]
    fn unservable_formats_are_refused_by_name() {
        for to in ["typst", "docx", "pptx", "epub"] {
            let err = check_servable_format(Some(to))
                .expect_err("format should be refused")
                .to_string();
            assert!(
                err.contains("not supported with --static"),
                "{to}: unexpected message {err}"
            );
            assert!(
                err.contains(to),
                "{to}: message should name the format: {err}"
            );
        }
    }

    #[test]
    fn subset_actions_render_the_changed_inputs_and_full_renders_the_target() {
        let a = PathBuf::from("/p/a.qmd");
        let b = PathBuf::from("/p/b.qmd");
        let subset = Action::Subset([a.clone(), b.clone()].into_iter().collect());
        let args = render_args_for(&subset, Path::new("/p"), Some("html"), ExecutionPolicy::All);
        assert_eq!(args.inputs, vec!["/p/a.qmd", "/p/b.qmd"]);
        assert_eq!(args.to.as_deref(), Some("html"));
        assert_eq!(args.execution_policy, ExecutionPolicy::All);
        let args = render_args_for(&Action::Full, Path::new("/p"), None, ExecutionPolicy::None);
        assert_eq!(args.inputs, vec!["/p"]);
        assert_eq!(args.to, None);
        assert_eq!(args.execution_policy, ExecutionPolicy::None);
    }

    #[test]
    fn policy_is_the_viewed_set_unless_the_project_turns_execution_off() {
        let viewed: BTreeSet<PathBuf> = BTreeSet::from([PathBuf::from("/p/a.qmd")]);
        assert_eq!(
            policy_for(&viewed, false),
            ExecutionPolicy::Only(viewed.clone())
        );
        assert_eq!(policy_for(&viewed, true), ExecutionPolicy::None);
        assert_eq!(
            policy_for(&BTreeSet::new(), false),
            ExecutionPolicy::Only(BTreeSet::new()),
            "nothing viewed yet: nothing executes"
        );
    }
}
