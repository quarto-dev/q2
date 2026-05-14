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
    // Phase D.2 (bd-kw93.13): split out the "what file did the user
    // ask for" signal from the project root. `initial_page` is None
    // unless we can confidently identify a specific page to seed in
    // the SPA — typically: user gave a file inside a _quarto.yml
    // project, OR the project root has an `index.qmd`.
    let (project_root, initial_page) = if args.no_project {
        if args.path.is_some() {
            anyhow::bail!("--no-project and a positional path are mutually exclusive");
        }
        (None, None)
    } else {
        let raw = args
            .path
            .unwrap_or_else(|| std::env::current_dir().expect("cwd"));
        let canonical = raw
            .canonicalize()
            .with_context(|| format!("resolving project root {}", raw.display()))?;
        let (root, page) = resolve_project_and_initial_page(&canonical)?;
        (Some(root), page)
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

/// Phase D.2 (bd-kw93.13): resolve `args.path` to a `(project_root,
/// initial_page)` pair. The initial page (if any) is appended to the
/// boot URL as `?page=<rel>` so the SPA can seed `activeFile` to the
/// user's choice instead of always picking `firstQmd`.
///
/// Three cases:
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
///    Preserve Phase A single-file mode: `project_root` becomes the
///    file path itself (the hub indexes only that file). The
///    `initial_page` is the file name so the SPA's pre-D.2 fallback
///    keeps working; if the SPA's parsing rejects it, the same
///    `firstQmd` path that worked pre-D.2 still applies.
pub(crate) fn resolve_project_and_initial_page(
    canonical: &Path,
) -> Result<(PathBuf, Option<String>)> {
    let metadata = std::fs::metadata(canonical)
        .with_context(|| format!("reading metadata of {}", canonical.display()))?;

    if metadata.is_dir() {
        let index = canonical.join("index.qmd");
        let initial = if index.is_file() {
            Some("index.qmd".to_string())
        } else {
            None
        };
        return Ok((canonical.to_path_buf(), initial));
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
            return Ok((project_dir, Some(rel)));
        }
        // No `_quarto.yml` ancestor → single-file mode (Phase A
        // semantics). project_root is the file path itself.
        let filename = canonical
            .file_name()
            .and_then(|s| s.to_str())
            .map(|s| s.to_string());
        return Ok((canonical.to_path_buf(), filename));
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

        let (root, page) =
            resolve_project_and_initial_page(&canonical).expect("dir-with-index resolves");
        assert_eq!(root, canonical);
        assert_eq!(page.as_deref(), Some("index.qmd"));
    }

    #[test]
    fn resolve_dir_without_index_returns_none() {
        let tmp = TempDir::with_prefix("d2-dir-no-index-").unwrap();
        std::fs::write(tmp.path().join("a.qmd"), "# a").unwrap();
        std::fs::write(tmp.path().join("b.qmd"), "# b").unwrap();
        let canonical = canonical_dir(&tmp);

        let (root, page) =
            resolve_project_and_initial_page(&canonical).expect("dir-without-index resolves");
        assert_eq!(root, canonical);
        assert!(
            page.is_none(),
            "no index.qmd → no initial_page hint; got: {page:?}"
        );
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

        let (root, page) =
            resolve_project_and_initial_page(&file).expect("file-in-project resolves");
        assert_eq!(root, proj);
        assert_eq!(page.as_deref(), Some("posts/intro.qmd"));
    }

    #[test]
    fn resolve_file_at_project_root_returns_filename() {
        let tmp = TempDir::with_prefix("d2-file-at-root-").unwrap();
        let proj = canonical_dir(&tmp);
        std::fs::write(proj.join("_quarto.yml"), "project: {}\n").unwrap();
        let file = proj.join("index.qmd");
        std::fs::write(&file, "# index").unwrap();

        let (root, page) = resolve_project_and_initial_page(&file).expect("resolves");
        assert_eq!(root, proj);
        assert_eq!(
            page.as_deref(),
            Some("index.qmd"),
            "file at project root should resolve to its own filename"
        );
    }

    #[test]
    fn resolve_file_without_quarto_yml_keeps_single_file_mode() {
        let tmp = TempDir::with_prefix("d2-file-no-yml-").unwrap();
        let dir = canonical_dir(&tmp);
        let file = dir.join("doc.qmd");
        std::fs::write(&file, "# doc").unwrap();
        // Deliberately NO _quarto.yml.

        let (root, page) =
            resolve_project_and_initial_page(&file).expect("single-file mode resolves");
        // Preserve pre-D.2 semantics: project_root IS the file path
        // (the hub indexes only this file). `initial_page` is set so
        // the SPA's pickInitialPage seeds activeFile to the same
        // file `firstQmd` would have picked anyway.
        assert_eq!(root, file);
        assert_eq!(page.as_deref(), Some("doc.qmd"));
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
}
