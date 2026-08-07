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
    let url = build_boot_url(&host, port, initial_page.as_deref());
    info!(%url, "starting q2 preview server");
    println!();
    println!("  q2 preview");
    println!("  → {url}");
    println!();

    // Phase D.1 (bd-kw93.8): actually open a browser tab. Failure to
    // open is logged + non-fatal (the URL is already printed for
    // copy-paste). Suppressed by --no-browser.
    //
    // bd-a6dvrdg1: open only once the server is actually accepting
    // connections. The server doesn't start until `quarto_preview::run`
    // below (which then blocks until shutdown), so the open has to run
    // on a spawned task that waits for readiness while the main task
    // goes on to start the server. Opening eagerly here — as we used to
    // — raced the server's startup: on larger projects the browser
    // connected before `axum::serve` was live and showed "Unable to
    // connect" until a manual reload. The probe (`wait_until_accepting`)
    // closes that race by gating on the real accept condition. The port
    // is the one we pre-probed; nothing is listening on it until the
    // server binds, so the probe naturally retries across the gap.
    if !args.no_browser {
        let url_for_open = url.clone();
        let host_for_open = host.clone();
        tokio::spawn(async move {
            const READY_TIMEOUT: Duration = Duration::from_secs(10);
            if wait_until_accepting(&host_for_open, port, READY_TIMEOUT).await {
                info!(host = %host_for_open, port, "preview server accepting connections; opening browser");
            } else {
                // We still consider a >10s startup a bug; opening anyway
                // (rather than never) preserves the old behavior as a
                // floor, and the warning gives the slow start visibility
                // instead of leaving it silent.
                tracing::warn!(
                    host = %host_for_open,
                    port,
                    timeout_secs = READY_TIMEOUT.as_secs(),
                    "preview server has not accepted a connection within the timeout; \
                     opening the browser anyway (it may need a manual reload)"
                );
            }
            open_browser_or_log(&url_for_open, false);
        });
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
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~' | b'/') {
            out.push(b as char);
        } else {
            use std::fmt::Write;
            write!(out, "%{:02X}", b).expect("writing to String never fails");
        }
    }
    out
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
}
