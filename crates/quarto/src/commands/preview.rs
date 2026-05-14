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
use quarto_preview::PreviewConfig;
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
    let port = match args.port {
        Some(p) => p,
        None => probe_free_port(&host)?,
    };

    let url = format!("http://{host}:{port}/");
    info!(%url, "starting q2 preview server");
    println!();
    println!("  q2 preview");
    println!("  → {url}");
    println!();
    if !args.no_browser {
        // Phase A polish gap: no `open` / `webbrowser` crate yet.
        // bd-vpsy (Playwright smoke) will likely pull one in.
        eprintln!("  (--no-browser auto-open isn't implemented yet; open the URL manually)");
        println!();
    }

    let config = PreviewConfig {
        host,
        port,
        project_root,
        data_dir,
        spa_dir_override: args.preview_dir,
        // CLI uses the default engine registry; tests substitute a
        // passthrough engine via the integration-test surface.
        engine_registry: None,
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
