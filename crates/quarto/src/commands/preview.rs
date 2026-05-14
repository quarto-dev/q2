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
use std::path::PathBuf;

use anyhow::{Context, Result};
use quarto_preview::{EnginePolicy, PreviewConfig, config::read_engine_policy_from_project};
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
    /// Override the ephemeral samod storage dir. Default: a fresh
    /// `tempfile::TempDir` that's deleted on shutdown.
    pub data_dir: Option<PathBuf>,
    /// Override the embedded SPA bundle with a disk path. Mirrors
    /// `QUARTO_TRACE_VIEWER_DIR`.
    pub preview_dir: Option<PathBuf>,
    /// Run standalone (no local project mode).
    pub no_project: bool,
}

pub fn execute(args: PreviewArgs) -> Result<()> {
    // Multi-threaded runtime for the same reasons quarto hub uses
    // one: samod, websockets, file watching, periodic sync.
    let runtime = tokio::runtime::Runtime::new()?;
    runtime.block_on(run(args))
}

async fn run(args: PreviewArgs) -> Result<()> {
    // Project mode is the default (epic plan Q5); --no-project is
    // the explicit standalone-server escape hatch. Canonicalize so
    // log lines and error messages don't show whatever relative form
    // the user typed.
    let project_root = if args.no_project {
        if args.path.is_some() {
            anyhow::bail!("--no-project and a positional path are mutually exclusive");
        }
        None
    } else {
        let raw = args
            .path
            .unwrap_or_else(|| std::env::current_dir().expect("cwd"));
        let canonical = raw
            .canonicalize()
            .with_context(|| format!("resolving project root {}", raw.display()))?;
        Some(canonical)
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

    let url = format!("http://{host}:{port}/");
    info!(%url, "starting q2 preview server");
    println!();
    println!("  q2 preview");
    println!("  → {url}");
    println!();

    // Phase D.1 (bd-kw93.8): actually open a browser tab. Failure to
    // open is logged + non-fatal (the URL is already printed for
    // copy-paste). Suppressed by --no-browser.
    open_browser_or_log(&url, args.no_browser);

    // Phase C.6: read `preview.engine` from `_quarto.yml` so the
    // driver knows whether to skip eager execution (`off`) or auto-
    // re-execute on edits (`auto`). Single-file projects with no
    // `_quarto.yml` get `Manual` (the safe pre-C.6 default).
    let engine_policy = match project_root.as_deref() {
        Some(root) => read_engine_policy_from_project(root, &NativeRuntime::new()),
        None => EnginePolicy::Manual,
    };
    info!(?engine_policy, "resolved preview engine policy");

    let config = PreviewConfig {
        host,
        port,
        project_root,
        data_dir,
        spa_dir_override: args.preview_dir,
        // CLI uses the default engine registry; tests substitute a
        // passthrough engine via the integration-test surface.
        engine_registry: None,
        engine_policy,
        // Phase C.7: derive the capture cache dir from `data_dir`
        // (per-session; tracked as a follow-up for per-project reuse).
        cache_dir: None,
    };
    quarto_preview::run(config).await
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

/// Phase D.1: open the boot URL in the user's default browser unless
/// `--no-browser` was passed. Failure is logged + non-fatal — the
/// URL was already printed for copy-paste before this fires.
fn open_browser_or_log(url: &str, suppress: bool) {
    if suppress {
        return;
    }
    if let Err(e) = open::that(url) {
        tracing::warn!(
            error = %e,
            "could not auto-open browser; the URL is printed above"
        );
    }
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

    #[test]
    fn open_browser_or_log_is_noop_when_suppressed() {
        // The `suppress` branch must return without touching the
        // OS — we never want a test run to fork a browser. Asserting
        // "doesn't panic, returns" is the contract.
        open_browser_or_log("https://invalid.example.invalid/", true);
    }
}
