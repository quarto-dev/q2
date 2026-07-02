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
mod text_execute;

pub use daemon::{JupyterDaemon, daemon};
pub use error::{JupyterError, Result};
pub use execute::{CellOutput, ExecuteResult, ExecuteStatus, MimeBundle};
pub use kernelspec::{ResolvedKernel, find_kernelspec, is_jupyter_language, list_kernelspecs};
pub use session::{KernelInfo, KernelSession, SessionKey};

use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::context::{ExecuteResult as EngineExecuteResult, ExecutionContext};
use super::error::ExecutionError;
use super::traits::ExecutionEngine;

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

    fn is_available(&self) -> bool {
        self.jupyter_path.is_some()
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
