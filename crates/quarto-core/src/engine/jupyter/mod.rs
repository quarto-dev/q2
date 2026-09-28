/*
 * engine/jupyter/mod.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Jupyter engine for Python/Julia code execution.
 */

//! Jupyter engine for Python/Julia code execution.
//!
//! This engine executes code cells using Jupyter kernels. It communicates
//! with kernels via ZeroMQ using the runtimelib crate.
//!
//! # Architecture
//!
//! The Jupyter engine implements the text-in/text-out [`ExecutionEngine`] trait,
//! parsing QMD input, executing code blocks via the Jupyter kernel, and returning
//! markdown with outputs inserted.
//!
//! ```text
//! JupyterEngine (ExecutionEngine)
//!     │
//!     └── JupyterDaemon (in-process, async)
//!            │
//!            └── KernelSession ──► ZeroMQ ──► Jupyter Kernel
//! ```
//!
//! # Kernel Management
//!
//! Kernels are managed by an in-process daemon that:
//! - Starts kernels on demand
//! - Reuses kernels for documents in the same directory
//! - Shuts down idle kernels after a timeout
//! - Cleans up kernel processes on shutdown
//!
//! # Availability
//!
//! This engine is only available in native builds (not WASM).
//! It requires a Jupyter kernel to be installed (e.g., `ipykernel` for Python).

mod daemon;
mod error;
mod execute;
mod kernelspec;
mod session;
mod stored;
mod text_execute;

pub use daemon::{JupyterDaemon, KernelScope, daemon, kernel_scope};
pub use error::{JupyterError, Result};
pub use execute::{CellOutput, ExecuteResult, ExecuteStatus, MimeBundle};
pub use kernelspec::{ResolvedKernel, find_kernelspec, is_jupyter_language, list_kernelspecs};
pub use session::{KernelInfo, KernelSession, SessionKey};
pub use stored::IpynbReplayEngine;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{LazyLock, Mutex, OnceLock};

use super::context::{ExecuteResult as EngineExecuteResult, ExecutionContext};
use super::error::ExecutionError;
use super::traits::ExecutionEngine;
use crate::engine::LanguageClaim;
use crate::extension::types::{FileClaim, ProcessorSpec};

/// Number of times the underlying PATH lookup for the jupyter
/// executable has run during this process. With the
/// `OnceLock`-backed cache in [`JupyterEngine::find_jupyter`] this
/// counter is capped at 1 for the lifetime of the process; it
/// serves as a regression tripwire (see `perf.engine-discover` in
/// `claude-notes/plans/2026-05-22-engine-discovery-cache.md`,
/// bd-c5u2g). The number of `JupyterEngine::new()` calls is *not*
/// what this counts — for that, instrument the caller.
static FIND_JUPYTER_CALL_COUNT: AtomicUsize = AtomicUsize::new(0);

/// Process-wide cache for the resolved jupyter executable path.
/// Initialized lazily on the first call to
/// [`JupyterEngine::find_jupyter`].
static JUPYTER_PATH_CACHE: OnceLock<Option<PathBuf>> = OnceLock::new();

/// Process-wide cache of availability probe results, keyed by
/// executable path. `jupyter --version` runs at most once per
/// resolved path per process — same rationale as
/// [`JUPYTER_PATH_CACHE`] (bd-c5u2g): the engine registry is
/// re-queried per render, and spawning jupyter per document would
/// dominate multi-document render time. Keyed by path (rather than a
/// bare `OnceLock<bool>`) so tests can probe their own stub binaries
/// without polluting each other. Lookup-only; order is never
/// observed.
static JUPYTER_PROBE_CACHE: LazyLock<Mutex<HashMap<PathBuf, bool>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Probe whether a jupyter executable actually runs: spawn
/// `jupyter --version` and require exit status 0. This catches the
/// stale-shim case (the name resolves on PATH but the binary exits
/// non-zero, e.g. a tool-manager trampoline pointing at a deleted
/// environment), which a PATH lookup alone cannot (bd-1eu34vpy).
fn probe_jupyter(path: &Path) -> bool {
    Command::new(path)
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

/// [`probe_jupyter`] memoized through [`JUPYTER_PROBE_CACHE`].
fn probe_jupyter_cached(path: &Path) -> bool {
    if let Some(&cached) = JUPYTER_PROBE_CACHE.lock().unwrap().get(path) {
        return cached;
    }
    // Probe without holding the lock: a spawn under the lock would
    // block every other engine's availability check. A duplicate
    // probe from a racing caller is benign (same result, idempotent).
    let result = probe_jupyter(path);
    JUPYTER_PROBE_CACHE
        .lock()
        .unwrap()
        .insert(path.to_path_buf(), result);
    result
}

/// Read the current value of [`FIND_JUPYTER_CALL_COUNT`].
pub fn find_jupyter_call_count() -> usize {
    FIND_JUPYTER_CALL_COUNT.load(Ordering::Relaxed)
}

/// Jupyter engine for Python/Julia code execution.
///
/// This engine communicates with Jupyter kernels to execute code cells.
///
/// # Requirements
///
/// - Jupyter must be installed (`jupyter` command accessible)
/// - Appropriate kernel for the language (e.g., `ipykernel` for Python)
///
/// # Supported Languages
///
/// - Python (via ipykernel)
/// - Julia (via IJulia)
/// - Other Jupyter-compatible kernels
pub struct JupyterEngine {
    /// Path to jupyter executable (for availability check).
    jupyter_path: Option<PathBuf>,
}

impl JupyterEngine {
    /// Create a new jupyter engine, attempting to find jupyter.
    pub fn new() -> Self {
        Self {
            jupyter_path: Self::find_jupyter(),
        }
    }

    /// Try to find jupyter executable on the system. Memoized for
    /// the lifetime of the process — see [`JUPYTER_PATH_CACHE`].
    fn find_jupyter() -> Option<PathBuf> {
        JUPYTER_PATH_CACHE
            .get_or_init(|| {
                // Counter is incremented only on cache miss so the
                // `perf.engine-discover` gauge tracks the
                // *expensive* work, not the cheap cached lookups.
                FIND_JUPYTER_CALL_COUNT.fetch_add(1, Ordering::Relaxed);
                // `which::which` walks PATH in-process — no
                // subprocess spawn. Brings us in line with
                // `find_rscript` (`engine/knitr/subprocess.rs`).
                // The previous `sh -c "command -v jupyter"` form
                // dominated profile time on multi-document renders
                // (bd-9eltv).
                which::which("jupyter").ok()
            })
            .clone()
    }

    /// Get the path to jupyter, if found.
    pub fn jupyter_path(&self) -> Option<&Path> {
        self.jupyter_path.as_deref()
    }

    /// Static, construction-free `percent` claims (Plan 7b Phase 5) — the
    /// same data `file_claims()` returns, callable without `JupyterEngine::
    /// new()` (which probes `PATH` for the `jupyter` binary). Used by
    /// `builtin_file_claims()` for discovery's gate 1.
    ///
    /// `comment` defaults to `"#"` for every extension except `.q`, which
    /// uses `/` (Plan 7b Decision 2).
    pub fn static_file_claims() -> Vec<FileClaim> {
        vec![
            FileClaim {
                extension: "py".to_string(),
                processor: Some(ProcessorSpec::Percent {
                    language: "python".to_string(),
                    comment: "#".to_string(),
                }),
            },
            FileClaim {
                extension: "jl".to_string(),
                processor: Some(ProcessorSpec::Percent {
                    language: "julia".to_string(),
                    comment: "#".to_string(),
                }),
            },
            FileClaim {
                extension: "r".to_string(),
                processor: Some(ProcessorSpec::Percent {
                    language: "r".to_string(),
                    comment: "#".to_string(),
                }),
            },
            FileClaim {
                extension: "q".to_string(),
                processor: Some(ProcessorSpec::Percent {
                    language: "q".to_string(),
                    comment: "/".to_string(),
                }),
            },
            // Plan 7c Phase 2: notebooks route through the native ipynb
            // content processor (sniff + convert + per-cell SourceInfo),
            // not the jupyter kernel.
            FileClaim {
                extension: "ipynb".to_string(),
                processor: Some(ProcessorSpec::Ipynb),
            },
        ]
    }
}

impl Default for JupyterEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl ExecutionEngine for JupyterEngine {
    fn name(&self) -> &str {
        "jupyter"
    }

    fn execute(
        &self,
        input: &str,
        ctx: &ExecutionContext,
    ) -> std::result::Result<EngineExecuteResult, ExecutionError> {
        // Check if jupyter is available
        if self.jupyter_path.is_none() {
            return Err(ExecutionError::runtime_not_found("jupyter", "jupyter"));
        }

        // Execute code blocks via the Jupyter daemon
        text_execute::execute_qmd(input, ctx)
    }

    fn can_freeze(&self) -> bool {
        true
    }

    /// Deliberate q2 design choice: jupyter is the universal fallback kernel.
    /// It returns `Fallback(0)` for *every* language it is asked about — it never
    /// enumerates, and it does NOT claim "julia" at `Primary(1)` the way Q1 did.
    /// Under the enum this falls out naturally: jupyter's `Fallback(0)` loses to a
    /// dedicated extension's `Primary(1)` (kind dominates priority), so the Julia
    /// extension wins when installed, and `{julia}` without it still reaches jupyter
    /// via the T4 fallback tier. See `claude-notes/designs/engine-resolution.md` §4.3.
    fn claims_language(&self, _language: &str, _first_class: Option<&str>) -> LanguageClaim {
        LanguageClaim::Fallback(0)
    }

    /// Pure Rust, always static. See `test_jupyter_try_claims_language_answers_statically`.
    fn try_claims_language(
        &self,
        language: &str,
        first_class: Option<&str>,
    ) -> Option<LanguageClaim> {
        Some(self.claims_language(language, first_class))
    }

    fn is_available(&self) -> bool {
        // Path presence alone is not enough: probe that the binary
        // actually runs so a broken jupyter reports unavailable and
        // jupyter-gated tests skip instead of hard-failing
        // (bd-1eu34vpy).
        self.jupyter_path
            .as_deref()
            .is_some_and(probe_jupyter_cached)
    }

    fn file_claims(&self) -> Vec<FileClaim> {
        Self::static_file_claims()
    }

    fn intermediate_files(&self, input_path: &Path) -> Vec<PathBuf> {
        // Jupyter may produce {input}_files/ directory for outputs
        let stem = input_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("");

        if stem.is_empty() {
            return Vec::new();
        }

        let parent = input_path.parent().unwrap_or(Path::new("."));
        let files_dir = parent.join(format!("{}_files", stem));

        vec![files_dir]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_jupyter_engine_name() {
        let engine = JupyterEngine::new();
        assert_eq!(engine.name(), "jupyter");
    }

    #[test]
    fn test_jupyter_engine_can_freeze() {
        let engine = JupyterEngine::new();
        assert!(engine.can_freeze());
    }

    #[test]
    fn test_jupyter_engine_intermediate_files() {
        let engine = JupyterEngine::new();

        let files = engine.intermediate_files(Path::new("/project/notebook.qmd"));
        assert_eq!(files.len(), 1);
        assert_eq!(files[0], PathBuf::from("/project/notebook_files"));
    }

    // --- Plan 7b Phase 5: builtin file claims. ---

    /// `static_file_claims()` (and `file_claims()`, which delegates to it)
    /// returns the four percent claims plus the ipynb claim (Plan 7c
    /// Phase 2) — no registry, no engine construction beyond the struct
    /// literal this test itself makes.
    #[test]
    fn static_file_claims_returns_percent_and_ipynb_claims() {
        let claims = JupyterEngine::static_file_claims();
        assert_eq!(claims.len(), 5);
        let by_ext = |ext: &str| claims.iter().find(|c| c.extension == ext).unwrap();

        for (ext, language, comment) in [
            ("py", "python", "#"),
            ("jl", "julia", "#"),
            ("r", "r", "#"),
            ("q", "q", "/"),
        ] {
            let claim = by_ext(ext);
            assert_eq!(
                claim.processor,
                Some(crate::extension::types::ProcessorSpec::Percent {
                    language: language.to_string(),
                    comment: comment.to_string(),
                }),
                "extension {ext}"
            );
        }

        assert_eq!(
            by_ext("ipynb").processor,
            Some(crate::extension::types::ProcessorSpec::Ipynb)
        );
    }

    /// A percent `.py` converts via the default `markdown_for_file`
    /// dispatch (native, no jupyter kernel/subprocess) — `JupyterEngine`
    /// overrides neither `claims_file` nor `markdown_for_file` itself; the
    /// `file_claims()` override is enough for the trait default to work.
    #[test]
    fn py_percent_converts_via_native_dispatch() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("script.py");
        std::fs::write(&file, "# %% [markdown]\n# hello\n").unwrap();

        let engine = JupyterEngine::new();
        let runtime: std::sync::Arc<dyn quarto_system_runtime::SystemRuntime> =
            std::sync::Arc::new(quarto_system_runtime::NativeRuntime::new());
        let (markdown, _source_info) = engine
            .markdown_for_file(&file, &runtime)
            .expect("percent .py must convert via native dispatch");
        assert_eq!(markdown, "hello\n\n");
    }

    #[test]
    fn test_jupyter_engine_is_available_depends_on_jupyter() {
        let engine = JupyterEngine::new();
        // Availability depends on whether jupyter is installed
        // We just verify it doesn't panic
        let _ = engine.is_available();
    }

    #[test]
    fn test_jupyter_engine_execute_requires_jupyter() {
        // Create engine with no jupyter path to test the error case
        let engine = JupyterEngine { jupyter_path: None };

        let ctx = ExecutionContext::new(
            PathBuf::from("/tmp"),
            PathBuf::from("/project"),
            PathBuf::from("/project/doc.qmd"),
            "html",
        );

        let result = engine.execute("# Test", &ctx);
        assert!(result.is_err());

        let err = result.unwrap_err();
        let msg = format!("{}", err);
        assert!(msg.contains("jupyter"));
    }

    #[test]
    fn test_jupyter_engine_default() {
        let engine = JupyterEngine::default();
        assert_eq!(engine.name(), "jupyter");
    }

    #[test]
    fn test_jupyter_engine_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<JupyterEngine>();
    }

    // === Test Seam Row 1: JupyterEngine claim table ===

    /// jupyter returns Fallback(0) for every language it is asked about —
    /// including "julia" (verifying it loses to a Primary(1) julia claim)
    /// and "r" and "python". This is the deliberate q2 design: jupyter is the
    /// universal fallback, not a dedicated primary for any language.
    #[test]
    fn test_jupyter_claims_language_julia_is_fallback_0() {
        let engine = JupyterEngine::new();
        assert_eq!(
            engine.claims_language("julia", None),
            LanguageClaim::Fallback(0)
        );
    }

    #[test]
    fn test_jupyter_claims_language_python_is_fallback_0() {
        let engine = JupyterEngine::new();
        assert_eq!(
            engine.claims_language("python", None),
            LanguageClaim::Fallback(0)
        );
    }

    #[test]
    fn test_jupyter_claims_language_r_is_fallback_0() {
        let engine = JupyterEngine::new();
        assert_eq!(
            engine.claims_language("r", None),
            LanguageClaim::Fallback(0)
        );
    }

    // === Phase 4: builtins_answer_statically ===

    /// JupyterEngine's `try_claims_language` must answer statically
    /// (`Some`), equal to `claims_language`, for representative languages.
    /// Revert binding: without the override, the trait default `None`
    /// (would-load) would sink every Pass-1 lift — jupyter is a universal
    /// fallback candidate for every doc.
    #[test]
    fn test_jupyter_try_claims_language_answers_statically() {
        let engine = JupyterEngine::new();
        for lang in ["r", "python", "julia"] {
            assert_eq!(
                engine.try_claims_language(lang, None),
                Some(engine.claims_language(lang, None)),
                "try_claims_language must equal claims_language for {lang}"
            );
        }
    }

    // === bd-1eu34vpy: is_available() must probe a working jupyter ===

    /// Write an executable `jupyter` stub into `dir` and return its
    /// path. `body` is the script body (batch syntax on Windows, `sh`
    /// syntax elsewhere) run when the stub is invoked with
    /// `--version`.
    fn write_jupyter_stub(dir: &Path, body: &str) -> PathBuf {
        #[cfg(windows)]
        {
            let path = dir.join("jupyter.bat");
            std::fs::write(&path, format!("@echo off\r\n{body}\r\n")).unwrap();
            path
        }
        #[cfg(not(windows))]
        {
            use std::os::unix::fs::PermissionsExt;
            let path = dir.join("jupyter");
            std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
            path
        }
    }

    /// A jupyter stub that exits non-zero (the stale-shim case: the
    /// name resolves on PATH but the binary is broken) must report
    /// NOT available, so jupyter-gated tests skip instead of
    /// hard-failing.
    #[test]
    fn is_available_false_for_broken_stub() {
        let tmp = tempfile::tempdir().unwrap();
        #[cfg(windows)]
        let stub = write_jupyter_stub(tmp.path(), "exit /b 1");
        #[cfg(not(windows))]
        let stub = write_jupyter_stub(tmp.path(), "exit 1");

        let engine = JupyterEngine {
            jupyter_path: Some(stub),
        };
        assert!(
            !engine.is_available(),
            "a jupyter that exits non-zero on `--version` must not be reported available"
        );
    }

    /// A jupyter stub that prints a version and exits 0 must report
    /// available.
    #[test]
    fn is_available_true_for_working_stub() {
        let tmp = tempfile::tempdir().unwrap();
        #[cfg(windows)]
        let stub = write_jupyter_stub(tmp.path(), "echo 5.7.0\r\nexit /b 0");
        #[cfg(not(windows))]
        let stub = write_jupyter_stub(tmp.path(), "echo 5.7.0\nexit 0");

        let engine = JupyterEngine {
            jupyter_path: Some(stub),
        };
        assert!(
            engine.is_available(),
            "a jupyter that answers `--version` with exit 0 must be reported available"
        );
    }

    /// The probe result is cached: repeated `is_available()` calls
    /// (engine registry is re-queried per render) must not re-spawn
    /// the binary. The stub appends one line to a counter file on
    /// every invocation; two calls must produce exactly one line.
    #[test]
    fn is_available_probe_runs_at_most_once_per_path() {
        let tmp = tempfile::tempdir().unwrap();
        let counter = tmp.path().join("counter.txt");
        #[cfg(windows)]
        let stub = write_jupyter_stub(
            tmp.path(),
            &format!("echo x>> \"{}\"\r\nexit /b 0", counter.display()),
        );
        #[cfg(not(windows))]
        let stub = write_jupyter_stub(
            tmp.path(),
            &format!("echo x >> '{}'\nexit 0", counter.display()),
        );

        let engine = JupyterEngine {
            jupyter_path: Some(stub),
        };
        assert!(engine.is_available());
        assert!(engine.is_available());

        // A missing counter file means the stub never ran at all
        // (e.g. is_available() short-circuits on path presence) —
        // read as zero invocations so the assertion fails cleanly.
        let invocations = std::fs::read_to_string(&counter)
            .unwrap_or_default()
            .lines()
            .count();
        assert_eq!(
            invocations, 1,
            "repeated is_available() calls must not re-spawn jupyter (probe result cached)"
        );
    }

    // bd-c5u2g: per-process memoization of `find_jupyter`. Pre-fix
    // the underlying executable lookup runs once per
    // `JupyterEngine::new()` (= once per document rendered), which
    // is the dominant cost on multi-document projects (see the
    // 2026-05-21 quarto-web profile). Post-fix the lookup runs at
    // most once for the lifetime of the process.
    //
    // The assertion is `delta <= 1` rather than `== 1` so the test
    // is stable regardless of test execution order: another test
    // in the same process may have already warmed the cache, in
    // which case this test's calls add zero new invocations.
    #[test]
    fn find_jupyter_is_memoized_across_engine_construction() {
        let before = find_jupyter_call_count();
        for _ in 0..10 {
            let _ = JupyterEngine::new();
        }
        let delta = find_jupyter_call_count() - before;
        assert!(
            delta <= 1,
            "expected find_jupyter to run at most once across 10 JupyterEngine::new() \
             calls (delta = {delta})"
        );
    }
}
