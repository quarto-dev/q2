/*
 * stage/stages/engine_claims_file.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Pre-parse file-claim and conversion stage.
 */

//! Pre-parse file-claim and conversion stage.
//!
//! `EngineClaimsFileStage` runs immediately before `ParseDocumentStage` in
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
//! This stage is inserted in the **native** pipeline builders only.  The WASM
//! pipeline will need it once built-in engines claim non-QMD files, but that
//! is a future plan item.

use std::path::PathBuf;

use async_trait::async_trait;

use crate::stage::{
    ConversionProvenance, EventLevel, PipelineData, PipelineDataKind, PipelineError, PipelineStage,
    SourceType, StageContext,
};
use crate::trace_event;

/// Pre-parse stage: ask engines whether they claim the input file and, if so,
/// convert it to QMD before `ParseDocumentStage` sees the bytes.
pub struct EngineClaimsFileStage;

impl EngineClaimsFileStage {
    pub fn new() -> Self {
        Self
    }
}

impl Default for EngineClaimsFileStage {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait(?Send)]
impl PipelineStage for EngineClaimsFileStage {
    fn name(&self) -> &str {
        "engine-claims-file"
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
        let is_qmd_or_md = ext.is_empty() || ext == "qmd" || ext == "md" || ext == "markdown";

        // Engines declare and match extensions in *dotted* form (".echo"),
        // matching `_extension.yml`'s `file-extensions:` / `claims-files:`, the
        // engine's `claimsFile` JS contract (`ext === ".echo"`), and TsEngine's
        // own unit tests. `Path::extension()` strips the leading dot, so re-add
        // it for the engine-facing extension string. A file with no extension
        // stays empty (never dotted).
        let ext_for_engine = if ext.is_empty() {
            String::new()
        } else {
            format!(".{ext}")
        };

        let file_str = path.to_string_lossy();

        // Iterate engines in deterministic order: contribution_order (TS
        // engines) → built-ins → alphabetical remainder.
        let engines = ctx.registry.engines_in_order();
        let mut claimer: Option<(String, String)> = None; // (engine_name, qmd_text)

        for engine in &engines {
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

        match claimer {
            Some((engine_name, qmd_text)) => {
                // Convert: replace content with QMD bytes, stamp provenance.
                source.content = qmd_text.into_bytes();
                source.source_type = SourceType::Qmd;
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
    }

    impl MockEngine {
        fn new(name: &str, claimed_extensions: &[&str]) -> Arc<Self> {
            Arc::new(Self {
                engine_name: name.to_string(),
                claimed_extensions: claimed_extensions.iter().map(|s| s.to_string()).collect(),
                markdown_for_file_calls: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
                conversion_cache: Mutex::new(std::collections::HashMap::new()),
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
        let mock = MockEngine::new("echo-engine", &[".echo"]);
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
        let stage = EngineClaimsFileStage::new();

        let output = stage.run(input, &mut ctx).await.unwrap();

        let PipelineData::LoadedSource(result) = output else {
            panic!("expected LoadedSource output");
        };

        // source_type must be Qmd after conversion.
        assert_eq!(result.source_type, SourceType::Qmd);
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
        let mock = MockEngine::new("echo-engine", &[".echo"]);
        let mut reg = EngineRegistry::empty();
        reg.register(Arc::clone(&mock) as Arc<dyn ExecutionEngine>);
        reg.contribution_order.push("echo-engine".to_string());

        let mut ctx = make_ctx_with_registry(reg);

        let original_content = b"---\ntitle: Test\n---\n\nHello world.".to_vec();
        let source =
            LoadedSource::new(PathBuf::from("/project/test.qmd"), original_content.clone());
        let input = PipelineData::LoadedSource(source);
        let stage = EngineClaimsFileStage::new();

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
        let mock = MockEngine::new("echo-engine", &[".echo"]);
        let mut reg = EngineRegistry::empty();
        reg.register(Arc::clone(&mock) as Arc<dyn ExecutionEngine>);
        reg.contribution_order.push("echo-engine".to_string());

        let mut ctx = make_ctx_with_registry(reg);

        let source = LoadedSource::new(PathBuf::from("/project/data.foo"), b"some data".to_vec());
        let input = PipelineData::LoadedSource(source);
        let stage = EngineClaimsFileStage::new();

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
    /// Running `EngineClaimsFileStage` twice (Pass 1, Pass 2) with the same
    /// engine Arc therefore incurs only one actual conversion.
    ///
    /// Named revert: remove the conversion_cache block from `MockEngine::
    /// markdown_for_file` so every call is counted, and this test goes RED.
    #[tokio::test]
    async fn test_p2_17_one_conversion_per_render() {
        let mock = MockEngine::new("echo-engine", &[".echo"]);
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

        let stage = EngineClaimsFileStage::new();
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

        let mock = MockEngine::new("echo-engine", &[".echo"]);
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
        let claims_stage = EngineClaimsFileStage::new();
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
}
