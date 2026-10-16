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
use quarto_core::format_resolution::{
    ScopeRequest, resolve_document_format, resolve_project_format, resolve_render_scope,
};
use quarto_core::project::book::format_consolidates_book;
use quarto_core::project::orchestrator::RenderMode;
use quarto_core::project::project_profile::cli_selection;
use quarto_core::{ProjectContext, metadata_flag};
use quarto_hub::watch::{FileWatcher, WatchConfig, WatchEvent, WatchFilter};
use quarto_preview::EnginePolicy;
use quarto_preview::config::{
    read_engine_policy_from_project, read_static_preview_defaults_from_project,
};
use quarto_preview::render_flags::native_runtime_for;
use quarto_preview::static_mode::{
    Action, ContentTracker, ReloadEvent, ReloadHub, StaticServerConfig, WatchContext, build_router,
    classify, is_config_like, is_input_extension, serve,
};
use quarto_system_runtime::SystemRuntime;
use tracing::{debug, info, warn};

use super::preview::spawn_browser_open_when_ready;
use super::render::{
    RenderAbort, RenderArgs, RenderReport, RenderTarget, classify_inputs, exit_with_abort,
    find_project_root_upward, present_report, render_once, resolve_format, strip_ansi_escapes,
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
    /// `-M/--metadata` arguments, unparsed, as `q2 render -M`.
    pub metadata: Vec<String>,
    /// `--profile` values, as `q2 render --profile`.
    pub profile: Vec<String>,
}

impl StaticArgs {
    /// The render flags every render of the session shares (F5b-D1): the
    /// boot render and every re-render clone this and fill in the inputs
    /// and the execution policy.
    fn base_render_args(&self) -> RenderArgs {
        RenderArgs {
            to: self.to.clone(),
            metadata: self.metadata.clone(),
            profile: self.profile.clone(),
            ..RenderArgs::default()
        }
    }
}

/// Formats a static server can usefully show: html and revealjs pages,
/// and typst's PDF (in the bundled pdf.js viewer, hub-parity Plan 8).
/// `docx`/`pptx`/`epub` render fine but produce nothing to browse, so
/// they are refused up front, before any render (plan § CLI). A key that
/// names no built-in format (an extension format) is not judged here.
fn check_servable_format(key: &str) -> Result<()> {
    let Ok(format) = resolve_format(key) else {
        return Ok(());
    };
    if matches!(
        format.identifier,
        FormatIdentifier::Html | FormatIdentifier::Revealjs | FormatIdentifier::Typst
    ) {
        return Ok(());
    }
    anyhow::bail!(
        "Format '{}' is not supported with --static (only html, revealjs and typst can be served)",
        format.identifier
    );
}

/// The format the render of `input` will use, decided the way `q2 render`
/// decides it (F5b-D4): the project's format for a project root, the
/// document's own for a standalone file, with `--to`, `-M` and `--profile`
/// applied. `None` when the resolver cannot get that far (a profile that
/// does not exist, a malformed `-M`): the render reports that error, which
/// is more specific than anything the gate could say.
fn resolved_format_key(input: &Path, base: &RenderArgs) -> Option<String> {
    let overlay = metadata_flag::overlay_from_metadata_flags(&base.metadata).ok()?;
    let plain = native_runtime_for(&base.profile);
    let selection = cli_selection(&base.profile);
    let cwd = plain.cwd().ok()?;
    let target = classify_inputs(
        &[input.to_string_lossy().into_owned()],
        &cwd,
        &plain,
        selection,
    )
    .ok()?;
    let runtime: Arc<dyn SystemRuntime> = match overlay {
        Some(overlay) => overlay.apply(Arc::new(plain)),
        None => Arc::new(plain),
    };
    let to = base.to.as_deref();
    match target {
        RenderTarget::FullProject { project_dir } => {
            let project =
                ProjectContext::discover_with_profile(&project_dir, runtime.as_ref(), selection)
                    .ok()?;
            let request = resolve_render_scope(&project, &RenderMode::Full, runtime.as_ref());
            Some(resolve_project_format(&project, &request, to, runtime.as_ref()).key)
        }
        RenderTarget::SingleDoc(path) => {
            let project =
                ProjectContext::discover_with_profile(&path, runtime.as_ref(), selection).ok()?;
            Some(
                resolve_document_format(
                    &project,
                    &path,
                    None,
                    &ScopeRequest::chapter(),
                    to,
                    runtime.as_ref(),
                )
                .key,
            )
        }
        RenderTarget::Subset { .. } => None,
    }
}

/// The one servable-format gate (F5b-D4), run before any render on the
/// resolved format. F8 reuses it for the live viewer.
pub(crate) fn check_servable(input: &Path, base: &RenderArgs) -> Result<()> {
    match resolved_format_key(input, base) {
        Some(key) => check_servable_format(&key),
        None => Ok(()),
    }
}

pub fn execute(args: StaticArgs) -> Result<()> {
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

/// The action whose arguments a re-render is built from (F5b-D2, FR16): a
/// book that consolidates into one file (typst) re-renders whole whatever
/// changed, since one chapter alone is not the served output. Anything else
/// keeps the per-document re-render.
fn rerender_action(is_consolidating_book: bool, action: Action) -> Action {
    match action {
        Action::Subset(_) if is_consolidating_book => Action::Full,
        other => other,
    }
}

/// `RenderArgs` for one pass of the loop: `base` (`--to`, `-M`,
/// `--profile`) plus the whole target for a full render or the changed
/// inputs for a subset render, converted by [`rerender_action`].
fn render_args_for(
    action: &Action,
    full_input: &Path,
    base: &RenderArgs,
    is_consolidating_book: bool,
    execution_policy: ExecutionPolicy,
) -> RenderArgs {
    let inputs = match rerender_action(is_consolidating_book, action.clone()) {
        Action::Subset(paths) => paths
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect(),
        Action::Full | Action::Ignore => vec![full_input.to_string_lossy().into_owned()],
    };
    RenderArgs {
        inputs,
        execution_policy,
        ..base.clone()
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
    /// The project is a book whose resolved format consolidates into one
    /// file: a re-render is whole ([`rerender_action`]).
    consolidating_book: bool,
}

impl LastRender {
    fn from_report(report: &RenderReport) -> Self {
        Self {
            ctx: watch_context(report),
            outputs: report.outputs(),
            unexecuted: report.unexecuted_inputs(),
            consolidating_book: report.project_is_book
                && resolve_format(&report.project_format_key)
                    .is_ok_and(|f| format_consolidates_book(&f)),
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

/// Ctrl-C, plus SIGTERM on unix and Ctrl-Break on Windows. Installed
/// with [`Self::install`] *before* the port opens, so a signal that
/// arrives the instant a client can connect is caught rather than
/// killing the process with the default disposition.
struct ShutdownSignal {
    #[cfg(unix)]
    interrupt: tokio::signal::unix::Signal,
    #[cfg(unix)]
    terminate: tokio::signal::unix::Signal,
    #[cfg(windows)]
    ctrl_c: tokio::signal::windows::CtrlC,
    #[cfg(windows)]
    ctrl_break: tokio::signal::windows::CtrlBreak,
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
                ctrl_break: tokio::signal::windows::ctrl_break()
                    .context("installing Ctrl-Break handler")?,
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
            tokio::select! {
                _ = self.ctrl_c.recv() => {}
                _ = self.ctrl_break.recv() => {}
            }
        }
    }
}

async fn run(args: StaticArgs) -> Result<()> {
    let raw = args
        .path
        .clone()
        .unwrap_or_else(|| std::env::current_dir().expect("cwd"));
    let path = quarto_system_runtime::canonicalize(&raw)
        .with_context(|| format!("resolving {}", raw.display()))?;

    let base_args = args.base_render_args();

    // A file inside a project previews the *project*, opened on that
    // page (Q1 `cmd.ts:362-390`): the whole site renders, so sidebars,
    // listings and cross-page links are all live. A directory, or a
    // file with no `_quarto.yml` above it, is rendered as given.
    let full_input = match path.parent() {
        Some(parent) if path.is_file() => {
            match find_project_root_upward(parent, &native_runtime_for(&args.profile)) {
                Ok(Some(root)) => root,
                _ => path.clone(),
            }
        }
        _ => path.clone(),
    };

    // The resolved format decides whether this can be served, before any
    // render happens, so an epub project is refused without first
    // producing an epub.
    check_servable(&full_input, &base_args)?;

    let profile = cli_selection(&args.profile);
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
        read_engine_policy_from_project(&policy_root, &native_runtime_for(&args.profile), profile),
        EnginePolicy::Off
    );
    // Q1's `project: preview:` keys are the defaults; CLI flags win
    // (plan Phase 4). `--no-watch` / `--no-navigate` / `--no-browser`
    // only ever turn things off, so a config `false` cannot be undone
    // from the command line — the same shape as Q1's flags.
    let defaults = read_static_preview_defaults_from_project(
        &policy_root,
        &native_runtime_for(&args.profile),
        profile,
    );
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
        &base_args,
        false,
        policy_for(&executed, engine_off),
    );
    let report = match render_blocking(boot_args.clone()).await {
        RenderOutcome::Report(report) => *report,
        RenderOutcome::Abort(abort) => {
            // Fails the way `q2 render` would: structured errors print
            // and exit 1, the rest surface through `main`.
            return Err(exit_with_abort(abort, &boot_args));
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
        Some(start_watcher(&report, &args.profile)?)
    } else {
        None
    };
    // Events are not edits: a render reading a file raises one on Linux
    // (`notify` subscribes to inotify OPEN), and editors touch files
    // without changing them. Seed with what the boot render read so its
    // own reads never trigger the first re-render. Seeding is part of
    // the before-anyone-can-connect setup: an edit that lands before
    // the seed would become the baseline, and its event would then
    // compare equal and be dropped (bd-tp0yym04).
    let mut tracker = ContentTracker::default();
    tracker.seed(
        report
            .inputs
            .iter()
            .chain(report.config_sources.iter())
            .cloned(),
    );
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
    record_failure(&hub, &report);
    let (page_tx, mut page_rx) = tokio::sync::mpsc::channel::<PathBuf>(64);
    let router = build_router(
        StaticServerConfig {
            root: report.output_dir.clone(),
            default_file,
            page_requests: Some(page_tx),
            pdfjs: quarto_preview::embedded_pdfjs_tree(),
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
    let mut last = LastRender::from_report(&report);
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
            &base_args,
            last.consolidating_book,
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
                        &base_args,
            last.consolidating_book,
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
                        &base_args,
            last.consolidating_book,
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
                        &base_args,
            last.consolidating_book,
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
fn start_watcher(report: &RenderReport, profile: &[String]) -> Result<FileWatcher> {
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
            Arc::new(native_runtime_for(profile)),
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
    base: &RenderArgs,
    is_consolidating_book: bool,
    execution_policy: ExecutionPolicy,
    hub: &ReloadHub,
    done_tx: &tokio::sync::mpsc::Sender<(Action, RenderOutcome)>,
) {
    let action = std::mem::replace(pending, Action::Ignore);
    let args = render_args_for(
        &action,
        full_input,
        base,
        is_consolidating_book,
        execution_policy,
    );
    // Logged as what will render; `in_flight` keeps the original action.
    match &rerender_action(is_consolidating_book, action.clone()) {
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

/// Tell the server what to show at `/` when there is nothing to serve: the
/// diagnostics of a render that wrote no output (F5b-D3), cleared by one
/// that did.
fn record_failure(hub: &ReloadHub, report: &RenderReport) {
    if !report.outputs().is_empty() {
        hub.set_failure(None);
        return;
    }
    let text = report.diagnostics_text(false);
    hub.set_failure(Some(if text.trim().is_empty() {
        "The render produced no output.".to_string()
    } else {
        text
    }));
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
            record_failure(hub, &report);
            *last = LastRender::from_report(&report);
        }
        RenderOutcome::Abort(abort) => {
            let message = abort.user_message();
            eprintln!("{message}");
            hub.set_failure(Some(strip_ansi_escapes(&message)));
            hub.send(ReloadEvent::RenderStop {
                ok: false,
                errors: 1,
                warnings: 0,
                text: strip_ansi_escapes(&message),
            });
        }
        RenderOutcome::Panicked(message) => {
            eprintln!("Error: the render panicked: {message}");
            hub.set_failure(Some(format!("The render panicked: {message}")));
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

    fn write(dir: &Path, rel: &str, body: &str) {
        let path = dir.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, body).unwrap();
    }

    /// A project root (canonical) with the given `_quarto.yml`.
    fn project(yml: &str) -> (tempfile::TempDir, PathBuf) {
        let temp = tempfile::TempDir::new().unwrap();
        let root = temp.path().canonicalize().unwrap();
        write(&root, "_quarto.yml", yml);
        write(&root, "index.qmd", "# Home\n");
        (temp, root)
    }

    fn base(to: Option<&str>, metadata: &[&str], profile: &[&str]) -> RenderArgs {
        RenderArgs {
            to: to.map(str::to_string),
            metadata: metadata.iter().map(|s| s.to_string()).collect(),
            profile: profile.iter().map(|s| s.to_string()).collect(),
            ..RenderArgs::default()
        }
    }

    #[test]
    fn servable_formats_pass_the_gate() {
        for key in ["html", "revealjs", "typst", "my-extension-format"] {
            assert!(check_servable_format(key).is_ok(), "{key}");
        }
    }

    #[test]
    fn unservable_formats_are_refused_by_name() {
        for to in ["docx", "pptx", "epub"] {
            let err = check_servable_format(to)
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
    fn the_gate_judges_the_project_format() {
        for fmt in ["epub", "docx", "pptx"] {
            let (_t, root) = project(&format!("project:\n  type: book\nformat: {fmt}\n"));
            assert_eq!(
                resolved_format_key(&root, &base(None, &[], &[])).as_deref(),
                Some(fmt)
            );
            assert!(
                check_servable(&root, &base(None, &[], &[])).is_err(),
                "{fmt}"
            );
        }
        for fmt in ["html", "typst", "revealjs"] {
            let (_t, root) = project(&format!("project:\n  type: book\nformat: {fmt}\n"));
            assert!(
                check_servable(&root, &base(None, &[], &[])).is_ok(),
                "{fmt}"
            );
        }
    }

    #[test]
    fn the_gate_judges_a_file_inside_the_project_by_the_project_root() {
        let (_t, root) = project("project:\n  type: book\nformat: epub\n");
        // `run` maps a file inside a project to the project root first.
        assert!(check_servable(&root, &base(None, &[], &[])).is_err());
    }

    #[test]
    fn to_and_scalar_metadata_format_reach_the_gate() {
        let (_t, root) = project("project:\n  type: book\nformat: html\n");
        assert!(check_servable(&root, &base(Some("epub"), &[], &[])).is_err());
        assert!(check_servable(&root, &base(None, &["format:epub"], &[])).is_err());
        // `--to` beats a scalar `-M format:` (FR2).
        assert!(check_servable(&root, &base(Some("html"), &["format:epub"], &[])).is_ok());
        assert_eq!(
            resolved_format_key(&root, &base(None, &["format:typst"], &[])).as_deref(),
            Some("typst")
        );
    }

    #[test]
    fn pseudo_format_to_still_passes_the_gate() {
        let (_t, root) = project("project:\n  type: website\n");
        assert!(check_servable(&root, &base(Some("q2-preview"), &[], &[])).is_ok());
    }

    #[test]
    fn a_standalone_document_is_judged_by_its_own_format() {
        let temp = tempfile::TempDir::new().unwrap();
        let dir = temp.path().canonicalize().unwrap();
        write(&dir, "a.qmd", "---\nformat: docx\n---\n\nhi\n");
        write(&dir, "b.qmd", "---\nformat: html\n---\n\nhi\n");
        assert!(check_servable(&dir.join("a.qmd"), &base(None, &[], &[])).is_err());
        assert!(check_servable(&dir.join("b.qmd"), &base(None, &[], &[])).is_ok());
        // `-M format:typst` selects typst over the document's own (FR2).
        assert_eq!(
            resolved_format_key(&dir.join("b.qmd"), &base(None, &["format:typst"], &[])).as_deref(),
            Some("typst")
        );
    }

    #[test]
    fn a_profile_can_supply_the_project_format() {
        let (_t, root) = project("project:\n  type: book\nformat: html\n");
        write(&root, "_quarto-epub.yml", "format: epub\n");
        assert!(check_servable(&root, &base(None, &[], &[])).is_ok());
        assert!(check_servable(&root, &base(None, &[], &["epub"])).is_err());
    }

    #[test]
    fn a_resolver_error_is_left_to_the_render() {
        let (_t, root) = project("project:\n  type: book\nformat: html\n");
        // A malformed `-M`, and a profile that does not exist: the gate
        // passes and the render reports the specific error.
        assert_eq!(
            resolved_format_key(&root, &base(None, &["nocolon"], &[])),
            None
        );
        assert!(check_servable(&root, &base(None, &["nocolon"], &[])).is_ok());
        assert!(check_servable(&root, &base(None, &[], &["no-such-profile"])).is_ok());
    }

    #[test]
    fn rerender_action_table() {
        let one = Action::Subset(BTreeSet::from([PathBuf::from("/p/a.qmd")]));
        // A consolidating book re-renders whole.
        assert_eq!(rerender_action(true, one.clone()), Action::Full);
        assert_eq!(rerender_action(true, Action::Full), Action::Full);
        assert_eq!(rerender_action(true, Action::Ignore), Action::Ignore);
        // html books and typst websites keep the per-document re-render.
        assert_eq!(rerender_action(false, one.clone()), one);
        assert_eq!(rerender_action(false, Action::Full), Action::Full);
        assert_eq!(rerender_action(false, Action::Ignore), Action::Ignore);
    }

    #[test]
    fn subset_actions_render_the_changed_inputs_and_full_renders_the_target() {
        let a = PathBuf::from("/p/a.qmd");
        let b = PathBuf::from("/p/b.qmd");
        let subset = Action::Subset([a.clone(), b.clone()].into_iter().collect());
        let html = base(Some("html"), &[], &[]);
        let args = render_args_for(&subset, Path::new("/p"), &html, false, ExecutionPolicy::All);
        assert_eq!(args.inputs, vec!["/p/a.qmd", "/p/b.qmd"]);
        assert_eq!(args.to.as_deref(), Some("html"));
        assert_eq!(args.execution_policy, ExecutionPolicy::All);
        let args = render_args_for(
            &Action::Full,
            Path::new("/p"),
            &base(None, &[], &[]),
            false,
            ExecutionPolicy::None,
        );
        assert_eq!(args.inputs, vec!["/p"]);
        assert_eq!(args.to, None);
        assert_eq!(args.execution_policy, ExecutionPolicy::None);
    }

    #[test]
    fn a_consolidating_book_re_renders_the_project_for_a_subset() {
        let subset = Action::Subset(BTreeSet::from([PathBuf::from("/p/a.qmd")]));
        let args = render_args_for(
            &subset,
            Path::new("/p"),
            &base(None, &[], &[]),
            true,
            ExecutionPolicy::All,
        );
        assert_eq!(args.inputs, vec!["/p"]);
    }

    #[test]
    fn render_flags_reach_every_render() {
        let flags = base(Some("typst"), &["toc:true", "format:typst"], &["a", "b"]);
        let subset = Action::Subset(BTreeSet::from([PathBuf::from("/p/a.qmd")]));
        for action in [Action::Full, subset] {
            let args = render_args_for(
                &action,
                Path::new("/p"),
                &flags,
                false,
                ExecutionPolicy::All,
            );
            assert_eq!(args.to.as_deref(), Some("typst"));
            assert_eq!(args.metadata, vec!["toc:true", "format:typst"]);
            assert_eq!(args.profile, vec!["a", "b"]);
        }
    }

    #[tokio::test]
    async fn the_action_recorded_as_in_flight_stays_the_subset() {
        let one = PathBuf::from("/definitely/not/here/a.qmd");
        let mut pending = Action::Subset(BTreeSet::from([one]));
        let expected = pending.clone();
        let mut in_flight = None;
        let hub = ReloadHub::new();
        let (tx, mut rx) = tokio::sync::mpsc::channel(1);
        start_render(
            &mut pending,
            &mut in_flight,
            Path::new("/definitely/not/here"),
            &base(None, &[], &[]),
            true,
            ExecutionPolicy::None,
            &hub,
            &tx,
        );
        assert_eq!(pending, Action::Ignore);
        assert_eq!(in_flight, Some(expected.clone()));
        // The render itself was the whole project (it aborts: no such path),
        // but `finish_render` is handed the original subset.
        let (ran, _outcome) = rx.recv().await.expect("render finished");
        assert_eq!(ran, expected);
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
