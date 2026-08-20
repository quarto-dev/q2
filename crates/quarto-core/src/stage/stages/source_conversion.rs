/*
 * stage/stages/source_conversion.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Pre-parse source-conversion stage.
 */

//! Pre-parse source-conversion stage.
//!
//! # Naming (D4)
//!
//! **Conversion is the action; claiming is only the predicate that selects
//! it.** Hence `SourceConversionStage` / `"source-conversion"` rather than
//! the older `EngineClaimsFileStage` / `"engine-claims-file"`. The name
//! stays accurate if built-in (non-engine) converters ever appear, it reads
//! correctly immediately before `parse-document`, and it follows the house
//! convention (`metadata-merge`, `language-resolve`, `include-expansion`).
//!
//! The engine-facing name `claims_file` / `claimsFile` is deliberately
//! **not** renamed: it is a wire message pair
//! (`ToEngine::ClaimsFile` / `FromEngine::ClaimsFileResult`) and a required
//! export of the public engine-author API — `engine-loader.ts` throws
//! `engine module … is missing required export: claimsFile`. Renaming it
//! would break every third-party engine. The engine claims; the stage
//! converts.
//!
//! `SourceConversionStage` runs immediately before `ParseDocumentStage` in
//! both the full HTML pipeline and the Pass-1 profile pipeline.  Its job:
//!
//! 1. Ask each registered engine (in deterministic order) whether it claims
//!    the input file via [`ExecutionEngine::claims_file`].  First claimer wins.
//! 2. For a claimed file, call [`ExecutionEngine::markdown_for_file`] to get
//!    the converted QMD text.  The engine may serve this from its own internal
//!    cache (e.g. `TsEngine::conversion_cache`) so Pass 1 and Pass 2 share one
//!    conversion per render (P2-17).
//! 3. Replace `source.content` with the QMD bytes, set `source.source_type =
//!    Qmd`, stamp `source.conversion = Some(ConversionProvenance { engine })`,
//!    and record `ctx.claimed_engine_name = Some(engine.name())`.
//! 4. For a `.qmd` / `.md` file with no claimer: pass through unchanged.
//! 5. For any other file no engine claims: return a loud error
//!    "Can't determine execution engine for <file>" (P2-11).
//!
//! # Scope note
//!
//! This stage runs on **every** target, native and WASM alike. It is
//! inserted by the shared `build_html_pipeline_stages_with_options`, which
//! has no `cfg` gate, and the live WASM entry points reach it through
//! `render_qmd_to_html` and `render_qmd_to_preview_ast` (the latter via
//! `build_q2_preview_pipeline_stages`, itself a filtered call to the shared
//! builder). So WASM already converts, and already hard-errors on an
//! unclaimed non-native extension.
//!
//! (An earlier version of this note claimed the stage was native-only. That
//! was wrong, and the wrongness outlived the code it described — the one
//! builder that genuinely lacked this stage, `build_wasm_html_pipeline`, was
//! dead code with no production caller and has been deleted.)

use std::path::PathBuf;

use async_trait::async_trait;

use crate::stage::{
    ConversionProvenance, EventLevel, PipelineData, PipelineDataKind, PipelineError, PipelineStage,
    SourceType, StageContext,
};
use crate::trace_event;

/// Pre-parse stage: ask engines whether they claim the input file and, if so,
/// convert it to QMD before `ParseDocumentStage` sees the bytes.
pub struct SourceConversionStage;

impl SourceConversionStage {
    pub fn new() -> Self {
        Self
    }
}

impl Default for SourceConversionStage {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait(?Send)]
impl PipelineStage for SourceConversionStage {
    fn name(&self) -> &str {
        "source-conversion"
    }

    fn input_kind(&self) -> PipelineDataKind {
        PipelineDataKind::LoadedSource
    }

    fn output_kind(&self) -> PipelineDataKind {
        PipelineDataKind::LoadedSource
    }

    async fn run(
        &self,
        input: PipelineData,
        ctx: &mut StageContext,
    ) -> Result<PipelineData, PipelineError> {
        let PipelineData::LoadedSource(mut source) = input else {
            return Err(PipelineError::unexpected_input(
                self.name(),
                self.input_kind(),
                input.kind(),
            ));
        };

        // Normalize the path to absolute + lexically normalized (no symlink
        // resolution) so engine calls and cache keys are consistent regardless
        // of how the path entered the pipeline.
        let path: PathBuf = if source.path.is_absolute() {
            source.path.clone()
        } else {
            ctx.project
                .dir
                .join(&source.path)
                .components()
                .collect::<PathBuf>()
        };

        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();

        // Determine whether the file is a native QMD / MD type that never
        // needs an engine to claim it.  Empty extension is treated as QMD
        // (the common case for virtual / in-memory documents).
        //
        // Single source of truth with discovery's widening rule (D1), which
        // must exclude exactly this set from the synthetic default patterns.
        let is_qmd_or_md = crate::project::discovery::NATIVE_EXTENSIONS.contains(&ext.as_str());

        // Extensions are undotted, lowercase everywhere on the Rust side
        // (canonical form, established at parse time in `extension/read.rs`'s
        // `normalize_ext`). `ext` (from `path.extension()`) is already in this
        // form, so the candidate the stage asks engines about is passed through
        // as-is. The dot is wire-only: `engine::ts_engine::to_wire_ext` re-adds
        // it exactly at the Rust -> TS seam (the `ClaimsFile` wire message), not
        // here. A file with no extension stays empty (never dotted).
        let ext_for_engine = if ext.is_empty() {
            String::new()
        } else {
            ext.clone()
        };

        let file_str = path.to_string_lossy();

        // Iterate engines in deterministic order: contribution_order (TS
        // engines) → built-ins → alphabetical remainder.
        let engines = ctx.registry.engines_in_order();
        let mut claimer: Option<(String, String)> = None; // (engine_name, qmd_text)

        // Engines that tried to claim a natively-owned extension (D5). Names
        // are collected rather than warned about inline: the refusal sits
        // inside this loop, so an inline warning would emit N diagnostics for
        // one file when N engines claim it. One file, one diagnostic.
        let mut refused_native: Vec<String> = Vec::new();

        // D5: q2 owns the native set outright, so a claim on it is refused no
        // matter what an engine would answer. Decide that WITHOUT asking.
        //
        // Asking would be actively harmful, not merely redundant: a
        // dynamically-claiming engine answers `claims_file` by loading, so a
        // project of nothing but `.qmd` paid a subprocess spawn per engine to
        // produce an answer this stage then discarded. `try_claims_file` reads
        // the extension's static declarations instead — free, and `None` for
        // exactly the engines that would have needed the load.
        //
        // The cost of not asking is that a dynamic claimer is no longer named
        // in `Q-2-51`; we cannot know what it would have said. That is the
        // intended trade (bd-exhbc6h8 follow-up, Gordon's 2(b)): the diagnostic
        // is kept where it is free — a declared `claims-files: [".md"]` is the
        // case actually worth telling an extension author about — and dropped
        // where it costs a subprocess.
        //
        // The refusal itself also closes a real bug rather than merely
        // tightening policy: without it an engine *can* claim `.md`, and the
        // converted file (source_type stamped Qmd) then has execution
        // suppressed anyway by `EngineExecutionStage`'s Q-2-40 guard, which
        // reads the *original* path — still `.md`. The user would get a
        // spurious "engine specification ignored" warning for a conversion
        // that silently happened.
        if is_qmd_or_md {
            for engine in &engines {
                if engine.try_claims_file(&file_str, &ext_for_engine) == Some(true) {
                    trace_event!(
                        ctx,
                        EventLevel::Debug,
                        "engine '{}' static claim on native extension {:?} refused (Q-2-51)",
                        engine.name(),
                        source.path
                    );
                    refused_native.push(engine.name().to_string());
                }
            }
        }

        for engine in engines.iter().filter(|_| !is_qmd_or_md) {
            if !engine.claims_file(&file_str, &ext_for_engine) {
                continue;
            }

            trace_event!(
                ctx,
                EventLevel::Debug,
                "engine '{}' claims {:?}",
                engine.name(),
                source.path
            );

            let qmd_text = engine
                .markdown_for_file(&path, &ctx.runtime)
                .map_err(|e| {
                    PipelineError::other(format!(
                        "Engine '{}' failed to convert {:?}: {}",
                        engine.name(),
                        source.path,
                        e
                    ))
                })
                .map(|(text, _source_info)| text)?;

            claimer = Some((engine.name().to_string(), qmd_text));
            break; // first claimer wins
        }

        // One file, one diagnostic — naming every engine that was refused.
        if !refused_native.is_empty() {
            let engine_list = refused_native
                .iter()
                .map(|n| format!("`{n}`"))
                .collect::<Vec<_>>()
                .join(", ");
            let plural = if refused_native.len() == 1 {
                "engine"
            } else {
                "engines"
            };
            let display_ext = if ext.is_empty() {
                "a file with no extension".to_string()
            } else {
                format!("`.{ext}` files")
            };
            ctx.add_diagnostic(
                quarto_error_reporting::DiagnosticMessageBuilder::warning(format!(
                    "engine claim on `{}` ignored",
                    source.path.display()
                ))
                .with_code("Q-2-51")
                .problem(format!(
                    "The {plural} {engine_list} claimed {display_ext}, but Quarto handles \
                     markdown natively. The claim is ignored and the file is rendered by \
                     Quarto's own parser."
                ))
                .add_info(
                    "Quarto owns `.qmd`, `.md`, `.markdown` and extension-less inputs. An \
                     engine that claimed one of them would bypass Quarto's parser entirely. \
                     Engine extensions should claim their own file extension instead.",
                )
                .build(),
            );
        }

        match claimer {
            Some((engine_name, qmd_text)) => {
                // Convert: replace content with QMD bytes, stamp provenance.
                source.content = qmd_text.into_bytes();
                // Only the conversion branch stamps a type. The
                // pass-through branch below is already correct from load
                // (`.qmd`/`.md` got `Some(...)` in `LoadedSource::new`), and
                // the unclaimed-non-native branch hard-errors — which is what
                // makes "always `Some` after this stage" an invariant.
                source.source_type = Some(SourceType::Qmd);
                source.conversion = Some(ConversionProvenance {
                    engine: engine_name.clone(),
                });
                ctx.claimed_engine_name = Some(engine_name);
            }
            None if is_qmd_or_md => {
                // Common fast path: .qmd / .md with no engine claiming it.
                // Pass through unchanged.
            }
            None => {
                // A non-QMD / non-MD file no engine claims — loud error (P2-11).
                return Err(PipelineError::other(format!(
                    "Can't determine execution engine for {}",
                    source.path.display()
                )));
            }
        }

        Ok(PipelineData::LoadedSource(source))
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::{EngineRegistry, ExecuteResult, ExecutionContext, ExecutionEngine};
    use crate::format::Format;
    use crate::project::{DocumentInfo, ProjectContext};
    use crate::stage::data::SourceType;
    use crate::stage::{LoadedSource, PipelineData};
    use quarto_source_map::SourceInfo;
    use quarto_system_runtime::SystemRuntime;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};

    // ── Helpers ──────────────────────────────────────────────────────────────

    /// Minimal `SystemRuntime` that accepts the operations this stage needs.
    struct MockRuntime;

    #[async_trait::async_trait]
    impl SystemRuntime for MockRuntime {
        fn file_read(
            &self,
            _path: &std::path::Path,
        ) -> quarto_system_runtime::RuntimeResult<Vec<u8>> {
            Ok(vec![])
        }
        fn file_write(
            &self,
            _path: &std::path::Path,
            _contents: &[u8],
        ) -> quarto_system_runtime::RuntimeResult<()> {
            Ok(())
        }
        fn path_exists(
            &self,
            _path: &std::path::Path,
            _kind: Option<quarto_system_runtime::PathKind>,
        ) -> quarto_system_runtime::RuntimeResult<bool> {
            Ok(true)
        }
        fn canonicalize(
            &self,
            path: &std::path::Path,
        ) -> quarto_system_runtime::RuntimeResult<PathBuf> {
            Ok(path.to_path_buf())
        }
        fn path_metadata(
            &self,
            _path: &std::path::Path,
        ) -> quarto_system_runtime::RuntimeResult<quarto_system_runtime::PathMetadata> {
            unimplemented!()
        }
        fn file_copy(
            &self,
            _src: &std::path::Path,
            _dst: &std::path::Path,
        ) -> quarto_system_runtime::RuntimeResult<()> {
            Ok(())
        }
        fn path_rename(
            &self,
            _old: &std::path::Path,
            _new: &std::path::Path,
        ) -> quarto_system_runtime::RuntimeResult<()> {
            Ok(())
        }
        fn file_remove(&self, _path: &std::path::Path) -> quarto_system_runtime::RuntimeResult<()> {
            Ok(())
        }
        fn dir_create(
            &self,
            _path: &std::path::Path,
            _recursive: bool,
        ) -> quarto_system_runtime::RuntimeResult<()> {
            Ok(())
        }
        fn dir_remove(
            &self,
            _path: &std::path::Path,
            _recursive: bool,
        ) -> quarto_system_runtime::RuntimeResult<()> {
            Ok(())
        }
        fn dir_list(
            &self,
            _path: &std::path::Path,
        ) -> quarto_system_runtime::RuntimeResult<Vec<PathBuf>> {
            Ok(vec![])
        }
        fn cwd(&self) -> quarto_system_runtime::RuntimeResult<PathBuf> {
            Ok(PathBuf::from("/project"))
        }
        fn temp_dir(
            &self,
            _template: &str,
        ) -> quarto_system_runtime::RuntimeResult<quarto_system_runtime::TempDir> {
            Ok(quarto_system_runtime::TempDir::new(PathBuf::from(
                "/tmp/test",
            )))
        }
        fn exec_pipe(
            &self,
            _command: &str,
            _args: &[&str],
            _stdin: &[u8],
        ) -> quarto_system_runtime::RuntimeResult<Vec<u8>> {
            Ok(vec![])
        }
        fn exec_command(
            &self,
            _command: &str,
            _args: &[&str],
            _stdin: Option<&[u8]>,
        ) -> quarto_system_runtime::RuntimeResult<quarto_system_runtime::CommandOutput> {
            Ok(quarto_system_runtime::CommandOutput {
                code: 0,
                stdout: vec![],
                stderr: vec![],
            })
        }
        fn env_get(&self, _name: &str) -> quarto_system_runtime::RuntimeResult<Option<String>> {
            Ok(None)
        }
        fn env_all(
            &self,
        ) -> quarto_system_runtime::RuntimeResult<std::collections::HashMap<String, String>>
        {
            Ok(std::collections::HashMap::new())
        }
        async fn fetch_url(
            &self,
            _url: &str,
        ) -> quarto_system_runtime::RuntimeResult<(Vec<u8>, String)> {
            Err(quarto_system_runtime::RuntimeError::NotSupported(
                "mock".to_string(),
            ))
        }
        fn os_name(&self) -> &'static str {
            "mock"
        }
        fn arch(&self) -> &'static str {
            "mock"
        }
        fn cpu_time(&self) -> quarto_system_runtime::RuntimeResult<u64> {
            Ok(0)
        }
        fn xdg_dir(
            &self,
            _kind: quarto_system_runtime::XdgDirKind,
            _subpath: Option<&std::path::Path>,
        ) -> quarto_system_runtime::RuntimeResult<PathBuf> {
            Ok(PathBuf::from("/xdg"))
        }
        fn stdout_write(&self, _data: &[u8]) -> quarto_system_runtime::RuntimeResult<()> {
            Ok(())
        }
        fn stderr_write(&self, _data: &[u8]) -> quarto_system_runtime::RuntimeResult<()> {
            Ok(())
        }
    }

    /// Mock execution engine for testing file claims.
    ///
    /// Claims every file whose extension is in `claimed_extensions`.
    /// `markdown_for_file` wraps the file path in a minimal QMD document,
    /// unless the internal `conversion_cache` already has the result.
    struct MockEngine {
        engine_name: String,
        claimed_extensions: Vec<String>,
        /// Counts how many times `markdown_for_file` was actually invoked
        /// (excluding cache hits). Useful for P2-17 caching assertions.
        markdown_for_file_calls: Arc<std::sync::atomic::AtomicUsize>,
        /// Simulates the per-engine conversion cache (as TsEngine does).
        conversion_cache: Mutex<std::collections::HashMap<PathBuf, String>>,
        /// When true, `try_claims_file` answers `None` ("I would have to load
        /// to tell you"), modelling a Q1-style dynamically-claiming engine.
        dynamic: bool,
    }

    impl MockEngine {
        fn new(name: &str, claimed_extensions: &[&str]) -> Arc<Self> {
            Arc::new(Self {
                engine_name: name.to_string(),
                claimed_extensions: claimed_extensions.iter().map(|s| s.to_string()).collect(),
                markdown_for_file_calls: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
                conversion_cache: Mutex::new(std::collections::HashMap::new()),
                dynamic: false,
            })
        }

        /// A dynamically-claiming engine: `claims_file` still answers, but the
        /// static probe reports "I would have to load".
        fn new_dynamic(name: &str, claimed_extensions: &[&str]) -> Arc<Self> {
            Arc::new(Self {
                engine_name: name.to_string(),
                claimed_extensions: claimed_extensions.iter().map(|s| s.to_string()).collect(),
                markdown_for_file_calls: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
                conversion_cache: Mutex::new(std::collections::HashMap::new()),
                dynamic: true,
            })
        }

        fn call_count(&self) -> usize {
            self.markdown_for_file_calls
                .load(std::sync::atomic::Ordering::Relaxed)
        }
    }

    impl ExecutionEngine for MockEngine {
        fn name(&self) -> &str {
            &self.engine_name
        }

        fn execute(
            &self,
            input: &str,
            _ctx: &ExecutionContext,
        ) -> Result<ExecuteResult, crate::engine::ExecutionError> {
            Ok(ExecuteResult::passthrough(input))
        }

        fn is_available(&self) -> bool {
            true
        }

        fn claims_file(&self, _file: &str, ext: &str) -> bool {
            self.claimed_extensions
                .iter()
                .any(|e| e.as_str() == ext.to_lowercase().as_str())
        }

        /// `claimed_extensions` is a fixed list known without loading, so
        /// `MockEngine` models a **static** claimer and the static probe must
        /// give the same answer as `claims_file`.
        ///
        /// Leaving this at the trait default (`Some(false)`) would silently
        /// re-model it as a dynamic claimer, and the native-set tests below
        /// would stop covering anything — they would pass because nothing is
        /// ever refused, not because refusal works. Use
        /// [`MockEngine::new_dynamic`] when a dynamic claimer is what you
        /// actually want.
        fn try_claims_file(&self, file: &str, ext: &str) -> Option<bool> {
            if self.dynamic {
                return None;
            }
            Some(self.claims_file(file, ext))
        }

        fn markdown_for_file(
            &self,
            file: &std::path::Path,
            _runtime: &Arc<dyn SystemRuntime>,
        ) -> Result<(String, SourceInfo), crate::engine::ExecutionError> {
            // Check cache first (simulating TsEngine.conversion_cache).
            {
                let guard = self.conversion_cache.lock().unwrap();
                if let Some(cached) = guard.get(file) {
                    return Ok((cached.clone(), SourceInfo::for_test()));
                }
            }
            // Cache miss — produce the result.
            self.markdown_for_file_calls
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let qmd = format!(
                "---\ntitle: Converted by {}\n---\n\nConverted from {}.\n",
                self.engine_name,
                file.display()
            );
            self.conversion_cache
                .lock()
                .unwrap()
                .insert(file.to_path_buf(), qmd.clone());
            Ok((qmd, SourceInfo::for_test()))
        }
    }

    fn make_ctx_with_registry(reg: EngineRegistry) -> StageContext {
        let runtime: Arc<dyn SystemRuntime> = Arc::new(MockRuntime);
        let project = ProjectContext {
            dir: PathBuf::from("/project"),
            config: crate::project::ProjectConfig::default(),
            is_single_file: true,
            output_dir: PathBuf::from("/project"),
            registry: Arc::new(reg),
            ..Default::default()
        };
        let doc = DocumentInfo::from_path("/project/test.echo");
        let format = Format::html();
        // StageContext::new clones project.registry into ctx.registry, so no
        // override needed.
        StageContext::new(runtime, format, project, doc).unwrap()
    }

    // ── Seam tests ────────────────────────────────────────────────────────────

    /// **Conversion happy path** (seam: claimed non-QMD file is converted).
    ///
    /// Named revert: remove the claim/convert branch in `run()` (just
    /// pass through without setting content/source_type/conversion/
    /// claimed_engine_name) and this test goes RED.
    #[tokio::test]
    async fn test_claimed_file_is_converted() {
        let mock = MockEngine::new("echo-engine", &["echo"]);
        let mut reg = EngineRegistry::empty();
        reg.register(Arc::clone(&mock) as Arc<dyn ExecutionEngine>);
        reg.contribution_order.push("echo-engine".to_string());

        let mut ctx = make_ctx_with_registry(reg);
        // Redirect document path so it's under the project dir.
        let doc = DocumentInfo::from_path("/project/hello.echo");
        ctx.document = doc;

        let source = LoadedSource::new(
            PathBuf::from("/project/hello.echo"),
            b"some echo content".to_vec(),
        );
        let input = PipelineData::LoadedSource(source);
        let stage = SourceConversionStage::new();

        let output = stage.run(input, &mut ctx).await.unwrap();

        let PipelineData::LoadedSource(result) = output else {
            panic!("expected LoadedSource output");
        };

        // source_type must be Qmd after conversion.
        assert_eq!(result.source_type, Some(SourceType::Qmd));
        // conversion provenance must be set to the claiming engine.
        let conv = result
            .conversion
            .expect("conversion must be Some after a claim");
        assert_eq!(conv.engine, "echo-engine");
        // content must be the engine's converted QMD, not the original bytes.
        let content = String::from_utf8(result.content).unwrap();
        assert!(
            content.contains("Converted by echo-engine"),
            "content should contain engine-produced QMD; got: {content}"
        );
        // claimed_engine_name on the context must reflect the claiming engine.
        assert_eq!(ctx.claimed_engine_name, Some("echo-engine".to_string()));
        // path is unchanged.
        assert_eq!(result.path, PathBuf::from("/project/hello.echo"));
    }

    /// **Pass-through .qmd** (seam: QMD files are never converted).
    ///
    /// Named revert: treat `.qmd` files as "no claimer → loud error" instead
    /// of pass-through, and this test goes RED.
    #[tokio::test]
    async fn test_qmd_passthrough() {
        // Registry with an engine that claims `.echo` only — NOT `.qmd`.
        let mock = MockEngine::new("echo-engine", &["echo"]);
        let mut reg = EngineRegistry::empty();
        reg.register(Arc::clone(&mock) as Arc<dyn ExecutionEngine>);
        reg.contribution_order.push("echo-engine".to_string());

        let mut ctx = make_ctx_with_registry(reg);

        let original_content = b"---\ntitle: Test\n---\n\nHello world.".to_vec();
        let source =
            LoadedSource::new(PathBuf::from("/project/test.qmd"), original_content.clone());
        let input = PipelineData::LoadedSource(source);
        let stage = SourceConversionStage::new();

        let output = stage.run(input, &mut ctx).await.unwrap();

        let PipelineData::LoadedSource(result) = output else {
            panic!("expected LoadedSource output");
        };

        // Content unchanged.
        assert_eq!(result.content, original_content);
        // No conversion provenance.
        assert!(
            result.conversion.is_none(),
            "QMD files must not set conversion"
        );
        // claimed_engine_name stays None.
        assert_eq!(ctx.claimed_engine_name, None);
    }

    /// **P2-11 no-claimer non-QMD** (seam: unclaimed non-QMD yields loud error).
    ///
    /// Named revert: change the `None` / non-QMD branch to pass through silently
    /// instead of returning an error, and this test goes RED.
    #[tokio::test]
    async fn test_p2_11_unclaimed_non_qmd_errors() {
        // Registry with an engine that claims `.echo` — NOT `.foo`.
        let mock = MockEngine::new("echo-engine", &["echo"]);
        let mut reg = EngineRegistry::empty();
        reg.register(Arc::clone(&mock) as Arc<dyn ExecutionEngine>);
        reg.contribution_order.push("echo-engine".to_string());

        let mut ctx = make_ctx_with_registry(reg);

        let source = LoadedSource::new(PathBuf::from("/project/data.foo"), b"some data".to_vec());
        let input = PipelineData::LoadedSource(source);
        let stage = SourceConversionStage::new();

        let result = stage.run(input, &mut ctx).await;

        assert!(
            result.is_err(),
            "unclaimed non-QMD file must return an error"
        );
        let err_msg = format!("{}", result.unwrap_err());
        assert!(
            err_msg.contains("Can't determine execution engine"),
            "error message must mention 'Can't determine execution engine'; got: {err_msg}"
        );
        assert!(
            err_msg.contains("data.foo"),
            "error message must mention the filename; got: {err_msg}"
        );
    }

    /// **P2-17 one-conversion-per-render** (seam: the engine's
    /// `markdown_for_file` is called exactly once across two stage runs
    /// when the engine caches internally).
    ///
    /// The `MockEngine` simulates `TsEngine`'s `conversion_cache`: second call
    /// for the same path hits the cache and does NOT increment the call counter.
    /// Running `SourceConversionStage` twice (Pass 1, Pass 2) with the same
    /// engine Arc therefore incurs only one actual conversion.
    ///
    /// Named revert: remove the conversion_cache block from `MockEngine::
    /// markdown_for_file` so every call is counted, and this test goes RED.
    #[tokio::test]
    async fn test_p2_17_one_conversion_per_render() {
        let mock = MockEngine::new("echo-engine", &["echo"]);
        let mock_arc = Arc::clone(&mock) as Arc<dyn ExecutionEngine>;

        let mut reg = EngineRegistry::empty();
        reg.register(mock_arc);
        reg.contribution_order.push("echo-engine".to_string());
        let reg_arc = Arc::new(reg);

        let runtime: Arc<dyn SystemRuntime> = Arc::new(MockRuntime);
        let project = ProjectContext {
            dir: PathBuf::from("/project"),
            config: crate::project::ProjectConfig::default(),
            is_single_file: true,
            output_dir: PathBuf::from("/project"),
            registry: Arc::clone(&reg_arc),
            ..Default::default()
        };

        // Pass 1 context.
        let mut ctx1 = StageContext::new(
            Arc::clone(&runtime),
            Format::html(),
            project.clone(),
            DocumentInfo::from_path("/project/hello.echo"),
        )
        .unwrap();

        // Pass 2 context (same project / registry Arc).
        let mut ctx2 = StageContext::new(
            Arc::clone(&runtime),
            Format::html(),
            project,
            DocumentInfo::from_path("/project/hello.echo"),
        )
        .unwrap();

        let stage = SourceConversionStage::new();
        let source1 = LoadedSource::new(
            PathBuf::from("/project/hello.echo"),
            b"echo content".to_vec(),
        );
        let source2 = LoadedSource::new(
            PathBuf::from("/project/hello.echo"),
            b"echo content".to_vec(),
        );

        // Pass 1 — cache miss.
        stage
            .run(PipelineData::LoadedSource(source1), &mut ctx1)
            .await
            .unwrap();
        // Pass 2 — cache hit (no additional subprocess call).
        stage
            .run(PipelineData::LoadedSource(source2), &mut ctx2)
            .await
            .unwrap();

        assert_eq!(
            mock.call_count(),
            1,
            "markdown_for_file must be called exactly once across both passes; \
             called {} times",
            mock.call_count()
        );
    }

    /// **ParseDocumentStage synthetic name (C′) smoke** — verify that a
    /// `LoadedSource` with `conversion = Some(...)` doesn't break
    /// `ParseDocumentStage` and that the output AST is non-empty.
    ///
    /// The full C′ seam test (asserting the exact synthetic name appears in
    /// the source_context) lives in `parse_document.rs`.
    #[tokio::test]
    async fn test_converted_source_parses_ok() {
        use crate::stage::stages::ParseDocumentStage;

        let mock = MockEngine::new("echo-engine", &["echo"]);
        let mut reg = EngineRegistry::empty();
        reg.register(Arc::clone(&mock) as Arc<dyn ExecutionEngine>);
        reg.contribution_order.push("echo-engine".to_string());

        let mut ctx = make_ctx_with_registry(reg);

        // Stage 1: claims-file.
        let qmd_content = "---\ntitle: Converted\n---\n\nHello from echo.\n";
        let source = LoadedSource::new(
            PathBuf::from("/project/test.echo"),
            // Pre-supply QMD so the mock engine wraps it.
            b"any".to_vec(),
        );
        let input = PipelineData::LoadedSource(source);
        let claims_stage = SourceConversionStage::new();
        let after_claims = claims_stage.run(input, &mut ctx).await.unwrap();

        // Manually stamp the content with known-good QMD to isolate parsing.
        let PipelineData::LoadedSource(mut converted) = after_claims else {
            panic!("expected LoadedSource");
        };
        converted.content = qmd_content.as_bytes().to_vec();

        // Stage 2: parse.
        let parse_stage = ParseDocumentStage::new();
        let parse_result = parse_stage
            .run(PipelineData::LoadedSource(converted), &mut ctx)
            .await;

        assert!(
            parse_result.is_ok(),
            "ParseDocumentStage must succeed on converted QMD; err: {:?}",
            parse_result.err()
        );
        let doc_ast = parse_result
            .unwrap()
            .into_document_ast()
            .expect("must be DocumentAst");
        assert!(
            !doc_ast.ast.blocks.is_empty(),
            "parsed AST must be non-empty"
        );
    }

    /// T10 (1c.2 Task 1, change C): the stage must pass the candidate
    /// extension to `claims_file` **undotted**, mirroring the parse-time
    /// normalized (undotted, lowercase) storage of a static `FileClaim {
    /// extension: "echo" }` claims-files declaration. A file named `X.ECHO`
    /// (mixed-case extension, no leading dot from `Path::extension()`) must
    /// still be claimed by an engine whose claimed set is undotted-lowercase
    /// "echo".
    ///
    /// Named revert: put the `:118` re-dot back (`format!(".{ext}")`) while
    /// the engine's claimed set stays undotted ("echo") — the stage would ask
    /// `claims_file(file, ".echo")`, which never equals the stored "echo" →
    /// the file goes unclaimed → `.echo`/`.ECHO` isn't a QMD/MD extension →
    /// the stage returns the P2-11 loud error instead of converting → this
    /// test's `.unwrap()` panics → RED.
    #[tokio::test]
    async fn test_t10_claims_file_undotted_lowercase_candidate() {
        // The engine's claimed set stores an UNDOTTED extension, mirroring a
        // `FileClaim { extension: "echo" }` claims-files declaration after
        // parse-time normalization (change B).
        let mock = MockEngine::new("echo-engine", &["echo"]);
        let mut reg = EngineRegistry::empty();
        reg.register(Arc::clone(&mock) as Arc<dyn ExecutionEngine>);
        reg.contribution_order.push("echo-engine".to_string());

        let mut ctx = make_ctx_with_registry(reg);
        // Mixed-case extension: the stage must lowercase it (already did,
        // pre-change) AND pass it undotted (this task's change).
        let doc = DocumentInfo::from_path("/project/X.ECHO");
        ctx.document = doc;

        let source = LoadedSource::new(
            PathBuf::from("/project/X.ECHO"),
            b"some echo content".to_vec(),
        );
        let input = PipelineData::LoadedSource(source);
        let stage = SourceConversionStage::new();

        let output = stage.run(input, &mut ctx).await.unwrap();

        let PipelineData::LoadedSource(result) = output else {
            panic!("expected LoadedSource output");
        };

        assert_eq!(
            result.source_type,
            Some(SourceType::Qmd),
            "X.ECHO must be claimed and converted by the undotted-lowercase-'echo' engine"
        );
        assert_eq!(ctx.claimed_engine_name, Some("echo-engine".to_string()));
    }

    // ── D5 / B3: engines may not claim the native set (Q-2-51) ──────────────

    /// Every member of the native set is refused, not just `.md`. An engine
    /// claiming `.qmd` would bypass q2's own parser entirely.
    /// A DYNAMIC claimer of a native extension is never asked, and therefore
    /// never named in `Q-2-51`.
    ///
    /// This is the deliberate cost of not probing (2(b)): q2 refuses the claim
    /// regardless, so asking would spawn a subprocess purely to produce an
    /// answer that is then discarded. We cannot name an engine whose answer we
    /// declined to obtain.
    ///
    /// Named revert: make `SourceConversionStage` call `claims_file` for the
    /// native set again and this goes RED -- `greedy` reappears in the warning.
    #[tokio::test]
    async fn dynamic_claimer_of_native_extension_is_not_probed_or_named() {
        let mut reg = EngineRegistry::new();
        reg.register(MockEngine::new_dynamic("greedy", &["md", "qmd"]));
        let mut ctx = make_ctx_with_registry(reg);

        let original = b"# native content".to_vec();
        let source = LoadedSource::new(PathBuf::from("/project/notes.md"), original.clone());
        let stage = SourceConversionStage::new();
        let output = stage
            .run(PipelineData::LoadedSource(source), &mut ctx)
            .await
            .expect("native file must render");

        let PipelineData::LoadedSource(result) = output else {
            panic!("expected LoadedSource output");
        };
        assert_eq!(result.content, original, "content must pass through");
        assert!(result.conversion.is_none(), "no conversion provenance");

        let q_2_50: Vec<_> = ctx
            .diagnostics
            .iter()
            .filter(|d| d.code.as_deref() == Some("Q-2-51"))
            .collect();
        assert!(
            q_2_50.is_empty(),
            "a dynamic claimer must not be named in Q-2-51 -- it was never asked; got {:?}",
            q_2_50.iter().map(|d| d.to_text(None)).collect::<Vec<_>>()
        );
    }

    /// A STATIC claimer of a native extension IS still named. The static
    /// declarations are free to read, so the diagnostic is kept exactly where
    /// it costs nothing -- and a declared `claims-files: [".md"]` is the case
    /// actually worth telling an extension author about.
    #[tokio::test]
    async fn static_claimer_of_native_extension_is_still_named() {
        let mut reg = EngineRegistry::new();
        reg.register(MockEngine::new("declared", &["md"]));
        let mut ctx = make_ctx_with_registry(reg);

        let stage = SourceConversionStage::new();
        stage
            .run(
                PipelineData::LoadedSource(LoadedSource::new(
                    PathBuf::from("/project/notes.md"),
                    b"# native".to_vec(),
                )),
                &mut ctx,
            )
            .await
            .expect("native file must render");

        let text = ctx
            .diagnostics
            .iter()
            .filter(|d| d.code.as_deref() == Some("Q-2-51"))
            .map(|d| d.to_text(None))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            text.contains("declared"),
            "a statically-declared claim on a native extension must still be \
             reported; Q-2-51 output was:\n{text}"
        );
    }

    #[tokio::test]
    async fn native_extension_claims_are_refused_with_q_2_50() {
        for (file, ext_label) in [
            ("/project/notes.md", "md"),
            ("/project/doc.qmd", "qmd"),
            ("/project/doc.markdown", "markdown"),
            ("/project/README", "(extension-less)"),
        ] {
            let mut reg = EngineRegistry::new();
            reg.register(MockEngine::new("greedy", &["md", "qmd", "markdown", ""]));
            let mut ctx = make_ctx_with_registry(reg);

            let original = b"# native content".to_vec();
            let source = LoadedSource::new(PathBuf::from(file), original.clone());
            let stage = SourceConversionStage::new();
            let output = stage
                .run(PipelineData::LoadedSource(source), &mut ctx)
                .await
                .expect("a refused claim must still render, not error");

            let PipelineData::LoadedSource(result) = output else {
                panic!("expected LoadedSource output");
            };

            // Fell through to the pass-through path: content untouched, no
            // conversion provenance, no claimed engine.
            assert_eq!(
                result.content, original,
                "{ext_label}: content must pass through unconverted"
            );
            assert!(
                result.conversion.is_none(),
                "{ext_label}: a refused claim must not stamp conversion provenance"
            );
            assert_eq!(
                ctx.claimed_engine_name, None,
                "{ext_label}: a refused claim must not set claimed_engine_name"
            );

            let q2_50: Vec<_> = ctx
                .diagnostics
                .iter()
                .filter(|d| d.code.as_deref() == Some("Q-2-51"))
                .collect();
            assert_eq!(
                q2_50.len(),
                1,
                "{ext_label}: expected exactly one Q-2-51; got {:?}",
                ctx.diagnostics
            );
            assert!(
                format!("{:?}", q2_50[0]).contains("greedy"),
                "{ext_label}: the diagnostic must name the refused engine"
            );
        }
    }

    /// The diagnostic is emitted once per FILE, not once per claiming
    /// engine. The refusal sits inside the engine loop, so a naive
    /// `continue` with an inline warning would produce N diagnostics for one
    /// file when N engines claim it.
    #[tokio::test]
    async fn q_2_50_is_emitted_once_per_file_naming_every_refused_engine() {
        let mut reg = EngineRegistry::new();
        reg.register(MockEngine::new("greedy-one", &["md"]));
        reg.register(MockEngine::new("greedy-two", &["md"]));
        reg.register(MockEngine::new("greedy-three", &["md"]));
        let mut ctx = make_ctx_with_registry(reg);

        let source = LoadedSource::new(PathBuf::from("/project/notes.md"), b"x".to_vec());
        let stage = SourceConversionStage::new();
        stage
            .run(PipelineData::LoadedSource(source), &mut ctx)
            .await
            .unwrap();

        let q2_50: Vec<_> = ctx
            .diagnostics
            .iter()
            .filter(|d| d.code.as_deref() == Some("Q-2-51"))
            .collect();
        assert_eq!(
            q2_50.len(),
            1,
            "three claiming engines must still yield ONE diagnostic; got {:?}",
            ctx.diagnostics
        );

        let text = format!("{:?}", q2_50[0]);
        for name in ["greedy-one", "greedy-two", "greedy-three"] {
            assert!(
                text.contains(name),
                "the single diagnostic must name every refused engine; {name} missing from {text}"
            );
        }
    }

    /// The refusal is scoped to the native set: a claim on a non-native
    /// extension still succeeds, so B3 does not disable engine claiming.
    #[tokio::test]
    async fn non_native_extension_claim_still_succeeds_after_b3() {
        let mut reg = EngineRegistry::new();
        reg.register(MockEngine::new("echo-engine", &["echo"]));
        let mut ctx = make_ctx_with_registry(reg);

        let source = LoadedSource::new(PathBuf::from("/project/a.echo"), b"x".to_vec());
        let stage = SourceConversionStage::new();
        let output = stage
            .run(PipelineData::LoadedSource(source), &mut ctx)
            .await
            .unwrap();

        let PipelineData::LoadedSource(result) = output else {
            panic!("expected LoadedSource output");
        };
        assert_eq!(result.source_type, Some(SourceType::Qmd));
        assert_eq!(ctx.claimed_engine_name, Some("echo-engine".to_string()));
        assert!(
            !ctx.diagnostics
                .iter()
                .any(|d| d.code.as_deref() == Some("Q-2-51")),
            "a non-native claim must not warn; got {:?}",
            ctx.diagnostics
        );
    }
}
