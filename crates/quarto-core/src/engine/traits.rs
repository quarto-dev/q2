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
use crate::extension::types::{FileClaim, ProcessorSpec};

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

    /// Static file claims this engine declares (Plan 7b), construction-free
    /// and side-effect-free — reading it never loads or launches the
    /// engine. Each entry pairs an extension with an optional named content
    /// processor (`ProcessorSpec`); a `None` processor means the extension
    /// is claimed but converted dynamically (the wire fallback for TS
    /// engines, or simply unsupported for a built-in that never overrides
    /// `markdown_for_file`).
    ///
    /// Default: empty (no static file claims).
    fn file_claims(&self) -> Vec<FileClaim> {
        Vec::new()
    }

    /// The claim→spec lookup behind the native dispatch (Plan 7c Phase 2):
    /// the processor declared for `file`'s extension, or `None` when
    /// [`Self::file_claims`] has no entry for it or the entry declares no
    /// processor. Extension lookup only — no file read, no launch.
    fn native_processor_for_file(&self, file: &Path) -> Option<ProcessorSpec> {
        let ext = file
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();
        let claim = self
            .file_claims()
            .into_iter()
            .find(|c| c.extension == ext)?;
        claim.processor
    }

    /// Native, zero-load content-processor **claim** decision (Plan 7b
    /// Phase 6) — the sniff half of "sniff + convert". Consulted by
    /// `SourceConversionStage` *before* it calls the dynamic `claims_file`;
    /// a processor-bearing entry's sniff answer is authoritative (`Some`),
    /// so the stage never falls through to a built-in's always-`false`
    /// `claims_file` default for that extension. Returns `None` when
    /// [`Self::native_processor_for_file`] has no spec for this file — the
    /// caller falls back to its own `claims_file`/`try_claims_file` path.
    ///
    /// The only I/O is the file read (via `runtime`); it never constructs
    /// an engine or spawns a subprocess. A read failure answers `Some(false)`
    /// (does not claim) rather than erroring here — if no other engine
    /// claims the file either, the stage's existing "Can't determine
    /// execution engine" path (or a later `markdown_for_file` call, for a
    /// claim some other engine made) surfaces it properly.
    fn native_claims_file(&self, file: &Path, runtime: &Arc<dyn SystemRuntime>) -> Option<bool> {
        let spec = self.native_processor_for_file(file)?;
        let content = match runtime.file_read_string(file) {
            Ok(c) => c,
            Err(_) => return Some(false),
        };
        Some(super::content_processors::sniff(&spec, file, &content))
    }

    /// Native, zero-load dispatch to a named content processor (Plan 7b),
    /// consulted by the default [`Self::markdown_for_file`] and by any
    /// engine (e.g. `TsEngine`) that must fall back to a dynamic/wire path
    /// when no processor governs the extension.
    ///
    /// Returns `None` when [`Self::native_processor_for_file`] has no spec
    /// for this file — the caller falls back to its own path. The only I/O
    /// is the file read (via `runtime`, so this stays WASM-clean); it never
    /// constructs an engine or spawns a subprocess.
    fn native_markdown_for_file(
        &self,
        file: &Path,
        runtime: &Arc<dyn SystemRuntime>,
    ) -> Option<Result<(String, SourceInfo), ExecutionError>> {
        let spec = self.native_processor_for_file(file)?;
        let content = match runtime.file_read_string(file) {
            Ok(c) => c,
            Err(e) => return Some(Err(ExecutionError::Other(e.to_string()))),
        };
        Some(
            super::content_processors::convert(&spec, file, &content, runtime)
                .map(|converted| (converted.markdown, converted.source_info))
                .map_err(|e| ExecutionError::Other(e.to_string())),
        )
    }

    /// Convert a non-QMD file to QMD text. Called only for files this engine
    /// claimed via `claims_file`. For QMD files, q2 handles parsing directly
    /// and this method is never called.
    ///
    /// Default: dispatches to [`Self::native_markdown_for_file`] — a named
    /// content processor, if `file_claims()` declares one for this
    /// extension — and falls back to
    /// `Err(ExecutionError::NotSupported("markdown_for_file"))` otherwise.
    fn markdown_for_file(
        &self,
        file: &Path,
        runtime: &Arc<dyn SystemRuntime>,
    ) -> Result<(String, SourceInfo), ExecutionError> {
        match self.native_markdown_for_file(file, runtime) {
            Some(result) => result,
            None => Err(ExecutionError::not_supported("markdown_for_file")),
        }
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
        file_claims: Vec<FileClaim>,
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

        fn file_claims(&self) -> Vec<FileClaim> {
            self.file_claims.clone()
        }
    }

    fn test_engine(name: &'static str, available: bool) -> TestEngine {
        TestEngine {
            name,
            available,
            file_claims: Vec::new(),
        }
    }

    #[test]
    fn test_engine_trait_name() {
        let engine = test_engine("test", true);
        assert_eq!(engine.name(), "test");
    }

    #[test]
    fn test_engine_trait_default_can_freeze() {
        let engine = test_engine("test", true);
        assert!(!engine.can_freeze());
    }

    #[test]
    fn test_engine_trait_default_intermediate_files() {
        let engine = test_engine("test", true);
        let files = engine.intermediate_files(Path::new("/test.qmd"));
        assert!(files.is_empty());
    }

    #[test]
    fn test_engine_trait_is_available() {
        let available = test_engine("test", true);
        let unavailable = test_engine("test", false);

        assert!(available.is_available());
        assert!(!unavailable.is_available());
    }

    #[test]
    fn test_engine_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<TestEngine>();
    }

    // --- T-registry (Phase 2): default `markdown_for_file` native dispatch. ---

    use crate::extension::types::ProcessorSpec;
    use tempfile::TempDir;

    fn native_runtime() -> Arc<dyn SystemRuntime> {
        Arc::new(quarto_system_runtime::NativeRuntime::new())
    }

    /// A claim naming a processor dispatches natively — no `NotSupported`,
    /// no engine object beyond the already-constructed `TestEngine`.
    #[test]
    fn default_markdown_for_file_dispatches_to_named_processor() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("script.spintest");
        // No roxygen header / chunk markers -> spin wraps it as one bare
        // code chunk; this test only cares that dispatch occurred (see
        // `content_processors::spin::tests` for spin's own grammar tests).
        std::fs::write(&file, "hello from spin").unwrap();

        let engine = TestEngine {
            name: "test",
            available: true,
            file_claims: vec![FileClaim {
                extension: "spintest".to_string(),
                processor: Some(ProcessorSpec::Spin),
            }],
        };

        let (markdown, _source_info) = engine
            .markdown_for_file(&file, &native_runtime())
            .expect("processor-bearing claim must dispatch natively");
        assert_eq!(markdown, "\n```{r}\nhello from spin\n```\n\n");
    }

    /// A claim with NO processor is not natively dispatchable — the default
    /// trait method falls back to `NotSupported` (an engine like `TsEngine`
    /// instead falls back to its own dynamic/wire path; see `ts_engine.rs`).
    #[test]
    fn default_markdown_for_file_no_processor_falls_back_to_not_supported() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("script.echotest");
        std::fs::write(&file, "irrelevant").unwrap();

        let engine = TestEngine {
            name: "test",
            available: true,
            file_claims: vec![FileClaim {
                extension: "echotest".to_string(),
                processor: None,
            }],
        };

        let err = engine
            .markdown_for_file(&file, &native_runtime())
            .expect_err("a claim with no processor must not natively dispatch");
        assert!(matches!(err, ExecutionError::NotSupported(_)));
    }

    /// The claim→spec lookup helper (Plan 7c Phase 2): returns the declared
    /// processor for a matching extension, `None` for an unmatched extension
    /// or a claim with no processor. No file read — extension lookup only.
    #[test]
    fn native_processor_for_file_returns_declared_spec() {
        let engine = TestEngine {
            name: "test",
            available: true,
            file_claims: vec![
                FileClaim {
                    extension: "ipynb".to_string(),
                    processor: Some(ProcessorSpec::Ipynb),
                },
                FileClaim {
                    extension: "echotest".to_string(),
                    processor: None,
                },
            ],
        };

        assert_eq!(
            engine.native_processor_for_file(Path::new("/proj/nb.ipynb")),
            Some(ProcessorSpec::Ipynb)
        );
        assert_eq!(
            engine.native_processor_for_file(Path::new("/proj/x.echotest")),
            None,
            "a claim with no processor yields None"
        );
        assert_eq!(
            engine.native_processor_for_file(Path::new("/proj/x.unrelated")),
            None,
            "no matching claim yields None"
        );
    }

    /// No matching claim at all (unrelated extension) is likewise
    /// `NotSupported` — proves the extension-match, not just "any claim
    /// exists," gates dispatch.
    #[test]
    fn default_markdown_for_file_no_matching_claim_falls_back_to_not_supported() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("script.unrelated");
        std::fs::write(&file, "irrelevant").unwrap();

        let engine = TestEngine {
            name: "test",
            available: true,
            file_claims: vec![FileClaim {
                extension: "spintest".to_string(),
                processor: Some(ProcessorSpec::Spin),
            }],
        };

        let err = engine
            .markdown_for_file(&file, &native_runtime())
            .expect_err("an unrelated extension must not dispatch");
        assert!(matches!(err, ExecutionError::NotSupported(_)));
    }
}
