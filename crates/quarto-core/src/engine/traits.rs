/*
 * engine/traits.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * ExecutionEngine trait definition.
 */

//! ExecutionEngine trait for code execution in Quarto documents.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use quarto_source_map::SourceInfo;
use quarto_system_runtime::SystemRuntime;

use super::context::{ExecuteResult, ExecutionContext};
use super::error::ExecutionError;
use crate::engine::LanguageClaim;

/// Execution engine for code cells in Quarto documents.
///
/// Engines transform markdown with executable code cells into markdown
/// with execution outputs. The transformation is text-in/text-out.
///
/// # Thread Safety
///
/// Engines must be `Send + Sync` for use in async pipeline contexts
/// and potential parallel rendering of multiple documents.
///
/// # Implementation Notes
///
/// - The `execute` method receives QMD text and returns QMD text with
///   code cell outputs expanded.
/// - Engines should preserve all non-code content unchanged.
/// - Supporting files (figures, data) should be written to the filesystem
///   and their paths included in the result.
/// - The markdown engine is a no-op that returns input unchanged.
///
/// # Example
///
/// ```ignore
/// use quarto_core::engine::{ExecutionEngine, ExecutionContext, ExecuteResult};
///
/// struct MyEngine;
///
/// impl ExecutionEngine for MyEngine {
///     fn name(&self) -> &str {
///         "my-engine"
///     }
///
///     fn execute(
///         &self,
///         input: &str,
///         ctx: &ExecutionContext,
///     ) -> Result<ExecuteResult, ExecutionError> {
///         // Process code cells and return result
///         Ok(ExecuteResult::new(processed_markdown))
///     }
/// }
/// ```
pub trait ExecutionEngine: Send + Sync {
    /// Human-readable name for this engine.
    ///
    /// This is used for:
    /// - Matching against `engine:` declarations in document metadata
    /// - Log messages and diagnostics
    /// - Registry lookup
    ///
    /// Standard names: "markdown", "knitr", "jupyter"
    fn name(&self) -> &str;

    /// Execute code cells in the input content.
    ///
    /// # Arguments
    ///
    /// * `input` - QMD text with executable code cells
    /// * `ctx` - Execution context with paths, config, and options
    ///
    /// # Returns
    ///
    /// `ExecuteResult` containing:
    /// - Transformed markdown with execution outputs
    /// - Paths to any supporting files created
    /// - Filters to apply during rendering
    /// - Content to inject into the document
    ///
    /// # Errors
    ///
    /// Returns `ExecutionError` if:
    /// - The engine runtime is not available
    /// - Code execution fails
    /// - IO operations fail
    fn execute(&self, input: &str, ctx: &ExecutionContext)
    -> Result<ExecuteResult, ExecutionError>;

    /// Whether this engine supports freeze/thaw caching.
    ///
    /// If true, execution results can be cached in the `_freeze/`
    /// directory and reused on subsequent renders when source
    /// code hasn't changed.
    ///
    /// Default: `false`
    fn can_freeze(&self) -> bool {
        false
    }

    /// Pure prediction of intermediate file paths derived from the input path.
    ///
    /// This is **not** post-execution introspection and **not** a cleanup list.
    /// The argument is the original source path; the return lists paths the engine
    /// will produce alongside the primary output (e.g. a generated `_files/`
    /// directory for figures). The result is used to **exclude those paths from
    /// the project's input-file set** so they are not treated as separate render
    /// targets.
    ///
    /// # Arguments
    ///
    /// * `input_path` - Path to the input document
    ///
    /// # Returns
    ///
    /// Paths to intermediate files/directories the engine will produce.
    fn intermediate_files(&self, _input_path: &Path) -> Vec<PathBuf> {
        Vec::new()
    }

    /// File extensions this engine handles (e.g. `[".jl"]` for a Julia engine).
    ///
    /// Used together with `claims_file` for non-QMD input support. An empty list
    /// (the default) means this engine does not claim any file type.
    fn valid_extensions(&self) -> Vec<String> {
        Vec::new()
    }

    /// Claim level for a computational language found in the document's AST.
    ///
    /// Called once per (language, first_class) pair during engine resolution.
    /// `first_class` is the first non-language class on the cell (e.g. `"marimo"`
    /// for `{python .marimo}`), or `None` for a plain cell.
    ///
    /// Returns a [`LanguageClaim`] indicating whether and how strongly this engine
    /// wants to own cells of this language. The resolver uses kind + priority to
    /// assign ownership; see `claude-notes/designs/engine-resolution.md` §3–§4.
    ///
    /// Default: `LanguageClaim::None` (no claim on any language).
    fn claims_language(&self, _language: &str, _first_class: Option<&str>) -> LanguageClaim {
        LanguageClaim::None
    }

    /// A static language claim if one exists, or `None` meaning "I would
    /// have to load to answer."
    ///
    /// The `None`-ness is a per-engine property, uniform across all
    /// languages: an engine with a static claim source answers every
    /// language (`Some(LanguageClaim::None)` for one it doesn't claim); a
    /// claims-less engine answers `None` for all. This is the no-load claim
    /// surface Pass-1 engine resolution (`resolve_engines_pass1`) attempts
    /// the whole resolution over — a single `None` answer aborts the
    /// attempt and falls through to the loading Pass-2 path.
    ///
    /// Default `None` is fail-safe: an un-overridden engine is treated as
    /// would-load and conservatively falls through.
    fn try_claims_language(
        &self,
        _language: &str,
        _first_class: Option<&str>,
    ) -> Option<LanguageClaim> {
        None
    }

    /// Whether this engine claims a particular non-QMD input file.
    ///
    /// Called during Pass-1 project scanning for files whose extension appears
    /// in `valid_extensions`. Return `true` to take ownership; if no engine
    /// claims the file, rendering halts with a loud error.
    ///
    /// Default: `false`.
    fn claims_file(&self, _file: &str, _ext: &str) -> bool {
        false
    }

    /// A static file claim if one exists, or `None` meaning "I would have to
    /// load to answer." The file counterpart of [`Self::try_claims_language`],
    /// and side-effect-free in the same way: no load, no cache write, no
    /// `static_file_answers` record.
    ///
    /// Default `Some(false)`, which differs from `try_claims_language`'s
    /// `None` on purpose. There, `None` is fail-safe because a missed language
    /// claim silently misroutes a cell. Here the default mirrors
    /// [`Self::claims_file`]'s own default of `false`: no built-in engine
    /// overrides `claims_file`, so "definitively does not claim, no load
    /// needed" is the truth for every engine that does not opt in — and
    /// answering `None` would instead assert that built-ins might need
    /// loading, which they never do.
    ///
    /// Used where q2 must decide a claim it is going to refuse anyway (the
    /// natively-owned extensions, `Q-2-51`). Asking `claims_file` there would
    /// spawn a subprocess to produce an answer that is immediately discarded.
    fn try_claims_file(&self, _file: &str, _ext: &str) -> Option<bool> {
        Some(false)
    }

    /// Convert a non-QMD file to QMD text. Called only for files this engine
    /// claimed via `claims_file`. For QMD files, q2 handles parsing directly
    /// and this method is never called.
    ///
    /// Returns the converted text; the `SourceInfo` slot is reserved for
    /// faithful original-file provenance (deferred — see "Provenance" in
    /// plan1a-engine) and is `SourceInfo::default()` in v1.
    ///
    /// Default: returns `Err(ExecutionError::NotSupported("markdown_for_file"))`.
    fn markdown_for_file(
        &self,
        _file: &Path,
        _runtime: &Arc<dyn SystemRuntime>,
    ) -> Result<(String, SourceInfo), ExecutionError> {
        Err(ExecutionError::not_supported("markdown_for_file"))
    }

    /// Check if this engine is available in the current environment.
    ///
    /// This checks whether the required runtime (R, Python, etc.)
    /// is installed and accessible.
    ///
    /// Default: `true` (assume available)
    fn is_available(&self) -> bool {
        true
    }

    /// The engine's `quartoRequired` version constraint, if it reported one.
    ///
    /// Returns the constraint string declared by the engine module (e.g. `">=1.9"`).
    /// Inert in Plan 1c — no gate reads it; Phase 12 adds the `satisfies()` checks.
    ///
    /// Default: `None` (no constraint declared or not yet loaded).
    fn quarto_required(&self) -> Option<&str> {
        None
    }

    /// Absolute path to the `_extension.yml` that contributed this engine,
    /// if any (Plan 6 Phase 5 provenance). `None` for built-in engines
    /// (markdown/knitr/jupyter) and for any engine without a known
    /// extension origin — used by [`super::EngineRegistry::engines_needing_load`]
    /// to point the Pass-1 fall-through warning at the file to edit.
    ///
    /// Default: `None`.
    fn extension_yml_path(&self) -> Option<PathBuf> {
        None
    }

    /// Shut down any subprocess / daemon backing this engine. Default is a no-op
    /// (built-in engines have no subprocess). MUST be idempotent — `shutdown_all`
    /// may call it more than once when several engines share one host. Called by
    /// `EngineRegistry::shutdown_all` at end-of-render (q2 uses explicit shutdown,
    /// not Drop).
    fn shutdown(&self) -> Result<(), ExecutionError> {
        Ok(())
    }

    /// Whether this engine currently holds a live backing subprocess.
    ///
    /// Default `false`: built-in engines (markdown / knitr / jupyter) have no
    /// persistent subprocess, so "alive" is meaningless for them. A `TsEngine`
    /// overrides this to report its shared host's real child-process state, so
    /// callers (and the orchestrator-teardown E2E) can observe that
    /// `shutdown_all` actually reaped the Deno subprocess.
    fn is_alive(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal test engine for verification
    struct TestEngine {
        name: &'static str,
        available: bool,
    }

    impl ExecutionEngine for TestEngine {
        fn name(&self) -> &str {
            self.name
        }

        fn execute(
            &self,
            input: &str,
            _ctx: &ExecutionContext,
        ) -> Result<ExecuteResult, ExecutionError> {
            Ok(ExecuteResult::passthrough(input))
        }

        fn is_available(&self) -> bool {
            self.available
        }
    }

    #[test]
    fn test_engine_trait_name() {
        let engine = TestEngine {
            name: "test",
            available: true,
        };
        assert_eq!(engine.name(), "test");
    }

    #[test]
    fn test_engine_trait_default_can_freeze() {
        let engine = TestEngine {
            name: "test",
            available: true,
        };
        assert!(!engine.can_freeze());
    }

    #[test]
    fn test_engine_trait_default_intermediate_files() {
        let engine = TestEngine {
            name: "test",
            available: true,
        };
        let files = engine.intermediate_files(Path::new("/test.qmd"));
        assert!(files.is_empty());
    }

    #[test]
    fn test_engine_trait_is_available() {
        let available = TestEngine {
            name: "test",
            available: true,
        };
        let unavailable = TestEngine {
            name: "test",
            available: false,
        };

        assert!(available.is_available());
        assert!(!unavailable.is_available());
    }

    #[test]
    fn test_engine_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<TestEngine>();
    }
}
