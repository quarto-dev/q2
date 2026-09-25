/*
 * stage/stages/parse_document.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Parse QMD content to Pandoc AST.
 */

//! Parse QMD content to Pandoc AST.
//!
//! This stage takes raw source content and parses it into a Pandoc AST
//! using the pampa parser.

use async_trait::async_trait;
use quarto_source_map::SourceContext;

use crate::stage::{
    ConversionStash, DocumentAst, EventLevel, PipelineData, PipelineDataKind, PipelineError,
    PipelineStage, StageContext,
};
use crate::trace_event;

/// Parse QMD content to Pandoc AST.
///
/// This stage:
/// 1. Takes raw source content (LoadedSource)
/// 2. Creates a SourceContext for error reporting
/// 3. Parses the content using pampa
/// 4. Returns a DocumentAst with the parsed AST and warnings
///
/// # Input
///
/// - `LoadedSource` - Raw file content with detected source type
///
/// # Output
///
/// - `DocumentAst` - Parsed Pandoc AST with source context and warnings
///
/// # Errors
///
/// Returns an error if parsing fails.
pub struct ParseDocumentStage;

impl ParseDocumentStage {
    /// Create a new ParseDocumentStage.
    pub fn new() -> Self {
        Self
    }
}

impl Default for ParseDocumentStage {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait(?Send)]
impl PipelineStage for ParseDocumentStage {
    fn name(&self) -> &str {
        "parse-document"
    }

    fn input_kind(&self) -> PipelineDataKind {
        PipelineDataKind::LoadedSource
    }

    fn output_kind(&self) -> PipelineDataKind {
        PipelineDataKind::DocumentAst
    }

    async fn run(
        &self,
        input: PipelineData,
        ctx: &mut StageContext,
    ) -> Result<PipelineData, PipelineError> {
        let PipelineData::LoadedSource(source) = input else {
            return Err(PipelineError::unexpected_input(
                self.name(),
                self.input_kind(),
                input.kind(),
            ));
        };

        trace_event!(
            ctx,
            EventLevel::Debug,
            "parsing {} bytes from {:?}",
            source.content.len(),
            source.path
        );

        // Create SourceContext for error reporting and location mapping.
        // This contains the file content needed for ariadne to show source snippets.
        let mut source_context = SourceContext::new();
        let content_str = source.content_string();

        // Plan 7c seam 2: stash conversion provenance on the stage context so
        // `run_pipeline`'s StageError arm can rebuild a SourceContext that
        // resolves parse-error diagnostics against the converted buffer, the
        // original file, and the per-cell virtual files. Set before the parse
        // so it is present even when the parse itself fails.
        if let Some(ref conv) = source.conversion {
            ctx.conversion_stash = Some(ConversionStash {
                engine: conv.engine.clone(),
                converted: content_str.clone(),
                source_info: source.source_info.clone(),
                files: source.files.clone(),
            });
        }

        // C′ — when an engine converted this file, register the converted QMD
        // content under an engine-reflecting synthetic name so that AST nodes
        // get honest `SourceInfo::Original(qmd_id, range)` positions into the
        // converted buffer.  The synthetic name makes it clear the bytes are
        // from the engine's output, not the original file.
        //
        // When `conversion` is None the stage behaves exactly as before (A′
        // dual-registration / faithful remap is deferred — see plan1c §1060).
        let source_name = if let Some(ref conv) = source.conversion {
            format!("<{} (converted by {})>", source.path.display(), conv.engine)
        } else {
            source.path.display().to_string()
        };
        source_context.add_file(source_name.clone(), Some(content_str));

        // Parse the QMD content
        let mut output_stream = std::io::sink();
        let parse_result = pampa::readers::qmd::read(
            &source.content,
            false,        // loose mode
            &source_name, // filename for error messages
            &mut output_stream,
            true,                       // track source locations
            source.source_info.clone(), // Plan 7b "A+": parent_source_info
        );

        match parse_result {
            Ok((ast, mut ast_context, warnings)) => {
                // Log any warnings
                if !warnings.is_empty() {
                    trace_event!(
                        ctx,
                        EventLevel::Debug,
                        "parsing produced {} warnings",
                        warnings.len()
                    );
                    // Also add diagnostics to context for pipeline-level collection
                    ctx.add_diagnostics(warnings.clone());
                }

                // Plan 7b "A+": when this file was natively converted by a
                // content processor, `source.source_info` is a genuine
                // `Concat`/`Original` whose pieces point at
                // `content_processors::ORIGINAL_FILE_ID` in the original
                // file. Every AST node's `SourceInfo` is now a `Substring`
                // over that mapping (via `parent_source_info` above), so
                // resolving it — ariadne snippets, `map_offset` — needs the
                // *original* file's bytes registered at that same FileId,
                // in both SourceContexts that flow out of this stage (they
                // must stay in lockstep — see `read()`'s doc + the
                // include-expansion precedent for the same pattern).
                if source.source_info.is_some() {
                    let original_content =
                        ctx.runtime.file_read_string(&source.path).map_err(|e| {
                            PipelineError::other(format!(
                                "Could not re-read {} to register its A+ provenance: {}",
                                source.path.display(),
                                e
                            ))
                        })?;
                    let original_name = source.path.display().to_string();
                    // The FileId is no assumption: ORIGINAL_FILE_ID is the
                    // well-known constant the content processor itself built
                    // the Concat pieces against, and the name/content are
                    // re-derived from the one file those pieces describe
                    // (`source.path`).
                    // lint:allow(add-file-with-id) — see reason above
                    ast_context.source_context.add_file_with_id(
                        crate::engine::content_processors::ORIGINAL_FILE_ID,
                        original_name.clone(),
                        Some(original_content.clone()),
                    );
                    // Same registration, into the second SourceContext that flows
                    // out of this stage (the two must stay in lockstep).
                    // lint:allow(add-file-with-id) — see reason above
                    source_context.add_file_with_id(
                        crate::engine::content_processors::ORIGINAL_FILE_ID,
                        original_name,
                        Some(original_content),
                    );

                    // Register the per-cell virtual files the converter
                    // emitted, contiguous after ORIGINAL_FILE_ID in piece
                    // order — exactly the ids the converter's Concat pieces
                    // point at (Plan 7c decision 6), so per-cell diagnostics
                    // resolve into their own virtual file.
                    for (i, cell_file) in source.files.iter().enumerate() {
                        let cell_id = quarto_source_map::FileId(
                            crate::engine::content_processors::ORIGINAL_FILE_ID.0 + 1 + i,
                        );
                        let label = cell_file.label.clone();
                        let text = cell_file.text.clone();
                        // Structured provenance for the virtual file (plan 7c
                        // Phase 4): diagnostics render the real notebook path
                        // and cell from this instead of decoding the label.
                        let origin = quarto_source_map::FileOrigin::NotebookCell {
                            notebook_path: source.path.display().to_string(),
                            cell_index: i + 1,
                            cell_id: cell_file.cell_id.clone(),
                            cell_type: cell_file.cell_type.clone(),
                        };
                        // lint:allow(add-file-with-id) — same reason as above
                        ast_context.source_context.add_file_with_id(
                            cell_id,
                            label.clone(),
                            Some(text.clone()),
                        );
                        if let Some(f) = ast_context.source_context.get_file_mut(cell_id) {
                            f.metadata.origin = Some(origin.clone());
                        }
                        // lint:allow(add-file-with-id) — same reason as above
                        source_context.add_file_with_id(cell_id, label, Some(text));
                        if let Some(f) = source_context.get_file_mut(cell_id) {
                            f.metadata.origin = Some(origin);
                        }
                    }
                }

                Ok(PipelineData::DocumentAst(DocumentAst {
                    path: source.path,
                    ast,
                    ast_context,
                    source_context,
                    warnings,
                    recorded_includes: Vec::new(),
                }))
            }
            Err(diagnostics) => {
                // Return error with diagnostics
                Err(PipelineError::stage_error_with_diagnostics(
                    self.name(),
                    diagnostics,
                ))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stage::LoadedSource;
    use std::path::PathBuf;

    #[tokio::test]
    async fn test_parse_simple_document() {
        use crate::format::Format;
        use crate::project::{DocumentInfo, ProjectContext};
        use crate::stage::StageContext;
        use quarto_system_runtime::TempDir;
        use std::sync::Arc;

        // Create a mock runtime
        struct MockRuntime;

        #[async_trait::async_trait]
        impl quarto_system_runtime::SystemRuntime for MockRuntime {
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
            ) -> quarto_system_runtime::RuntimeResult<quarto_system_runtime::PathMetadata>
            {
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
            fn file_remove(
                &self,
                _path: &std::path::Path,
            ) -> quarto_system_runtime::RuntimeResult<()> {
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
                Ok(PathBuf::from("/"))
            }
            fn temp_dir(&self, _template: &str) -> quarto_system_runtime::RuntimeResult<TempDir> {
                Ok(TempDir::new(PathBuf::from("/tmp/test")))
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
            ) -> quarto_system_runtime::RuntimeResult<quarto_system_runtime::CommandOutput>
            {
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

        let runtime = Arc::new(MockRuntime);
        let project = ProjectContext {
            dir: PathBuf::from("/project"),
            config: crate::project::ProjectConfig::default(),
            is_single_file: true,
            files: vec![],
            output_dir: PathBuf::from("/project"),

            ..Default::default()
        };
        let doc = DocumentInfo::from_path("/project/test.qmd");
        let format = Format::html();

        let mut ctx = StageContext::new(runtime, format, project, doc).unwrap();

        let stage = ParseDocumentStage::new();

        let content = b"---\ntitle: Test\n---\n\nHello, world!";
        let source = LoadedSource::new(PathBuf::from("/project/test.qmd"), content.to_vec());

        let input = PipelineData::LoadedSource(source);
        let output = stage.run(input, &mut ctx).await.unwrap();

        let doc_ast = output.into_document_ast().expect("Should be DocumentAst");
        assert_eq!(doc_ast.path, PathBuf::from("/project/test.qmd"));
        // The AST should have at least one block (the paragraph)
        assert!(!doc_ast.ast.blocks.is_empty());
    }

    /// **ParseDocumentStage C′ — synthetic source name** (seam).
    ///
    /// When `source.conversion` is `Some(ConversionProvenance { engine })`,
    /// `ParseDocumentStage` must register the converted content under the
    /// synthetic name `"<{path} (converted by {engine})>"` in the
    /// `source_context`, so AST nodes carry honest `Original(qmd_id)` positions
    /// into the converted buffer.
    ///
    /// Named revert: remove the `if let Some(ref conv) = source.conversion { … }`
    /// branch in `run()` and always use `source.path.display().to_string()` —
    /// the synthetic name is absent and this test goes RED.
    #[tokio::test]
    async fn test_parse_document_c_prime_synthetic_name() {
        use crate::format::Format;
        use crate::project::{DocumentInfo, ProjectContext};
        use crate::stage::{ConversionProvenance, LoadedSource, StageContext};
        use quarto_system_runtime::TempDir;
        use std::sync::Arc;

        struct MockRuntime;

        #[async_trait::async_trait]
        impl quarto_system_runtime::SystemRuntime for MockRuntime {
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
            ) -> quarto_system_runtime::RuntimeResult<quarto_system_runtime::PathMetadata>
            {
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
            fn file_remove(
                &self,
                _path: &std::path::Path,
            ) -> quarto_system_runtime::RuntimeResult<()> {
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
                Ok(PathBuf::from("/"))
            }
            fn temp_dir(&self, _template: &str) -> quarto_system_runtime::RuntimeResult<TempDir> {
                Ok(TempDir::new(PathBuf::from("/tmp/test")))
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
            ) -> quarto_system_runtime::RuntimeResult<quarto_system_runtime::CommandOutput>
            {
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

        let runtime = Arc::new(MockRuntime);
        let project = ProjectContext {
            dir: PathBuf::from("/project"),
            config: crate::project::ProjectConfig::default(),
            is_single_file: true,
            files: vec![],
            output_dir: PathBuf::from("/project"),
            ..Default::default()
        };
        let doc = DocumentInfo::from_path("/project/test.echo");
        let format = Format::html();
        let mut ctx = StageContext::new(runtime, format, project, doc).unwrap();

        let stage = ParseDocumentStage::new();
        let content = b"---\ntitle: Converted\n---\n\nHello from echo engine.\n";

        // Build a LoadedSource that looks like it came through SourceConversionStage.
        let mut source = LoadedSource::new(PathBuf::from("/project/test.echo"), content.to_vec());
        source.conversion = Some(ConversionProvenance {
            engine: "echo-engine".to_string(),
        });

        let output = stage
            .run(PipelineData::LoadedSource(source), &mut ctx)
            .await
            .unwrap();
        let doc_ast = output.into_document_ast().expect("Should be DocumentAst");

        // The source_context must contain the synthetic name, not the bare path.
        // ParseDocumentStage creates a fresh SourceContext and adds exactly one
        // file — so FileId(0) is the one registered for this document.
        // `source.path.display()` for `/project/test.echo` → `/project/test.echo`,
        // so the synthetic name is `</project/test.echo (converted by echo-engine)>`.
        let expected_synthetic = "</project/test.echo (converted by echo-engine)>";
        let file0 = doc_ast
            .source_context
            .get_file(quarto_source_map::FileId(0))
            .expect("source_context must have FileId(0)");
        assert_eq!(
            file0.path, expected_synthetic,
            "source_context FileId(0) must be the C′ synthetic name; got: {}",
            file0.path
        );
        // Original path is preserved on the DocumentAst.
        assert_eq!(doc_ast.path, PathBuf::from("/project/test.echo"));
        // AST must be non-empty.
        assert!(!doc_ast.ast.blocks.is_empty());
    }

    /// **Plan 7b "A+" — original file registered at the fixed FileId**
    /// (seam). When `source.source_info` is `Some` (a content processor's
    /// faithful mapping), `ParseDocumentStage` must re-read the *original*
    /// file and register it at `content_processors::ORIGINAL_FILE_ID` in
    /// BOTH `ast_context.source_context` (what AST-node `Substring`s
    /// resolve against) and the top-level `source_context` (ariadne
    /// snippets), in lockstep.
    ///
    /// Named revert: remove the `if source.source_info.is_some() { … }`
    /// block in `run()` and this test goes RED (FileId(1) absent from both
    /// contexts).
    #[tokio::test]
    async fn test_parse_document_a_plus_registers_original_file() {
        use crate::format::Format;
        use crate::project::{DocumentInfo, ProjectContext};
        use crate::stage::{LoadedSource, StageContext};
        use quarto_system_runtime::TempDir;
        use std::sync::Arc;

        const ORIGINAL_CONTENT: &str = "# %% [markdown]\n# hello\n";

        struct MockRuntime;

        #[async_trait::async_trait]
        impl quarto_system_runtime::SystemRuntime for MockRuntime {
            fn file_read(
                &self,
                _path: &std::path::Path,
            ) -> quarto_system_runtime::RuntimeResult<Vec<u8>> {
                Ok(ORIGINAL_CONTENT.as_bytes().to_vec())
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
            ) -> quarto_system_runtime::RuntimeResult<quarto_system_runtime::PathMetadata>
            {
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
            fn file_remove(
                &self,
                _path: &std::path::Path,
            ) -> quarto_system_runtime::RuntimeResult<()> {
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
                Ok(PathBuf::from("/"))
            }
            fn temp_dir(&self, _template: &str) -> quarto_system_runtime::RuntimeResult<TempDir> {
                Ok(TempDir::new(PathBuf::from("/tmp/test")))
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
            ) -> quarto_system_runtime::RuntimeResult<quarto_system_runtime::CommandOutput>
            {
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

        let runtime = Arc::new(MockRuntime);
        let project = ProjectContext {
            dir: PathBuf::from("/project"),
            config: crate::project::ProjectConfig::default(),
            is_single_file: true,
            output_dir: PathBuf::from("/project"),
            ..Default::default()
        };
        let doc = DocumentInfo::from_path("/project/hello.py");
        let format = Format::html();
        let mut ctx = StageContext::new(runtime, format, project, doc).unwrap();

        let stage = ParseDocumentStage::new();
        let converted = "---\ntitle: hello\n---\n\nhello\n";
        let mut source = LoadedSource::new(PathBuf::from("/project/hello.py"), converted.into());
        source.source_info = Some(quarto_source_map::SourceInfo::original(
            crate::engine::content_processors::ORIGINAL_FILE_ID,
            0,
            ORIGINAL_CONTENT.len(),
        ));

        let output = stage
            .run(PipelineData::LoadedSource(source), &mut ctx)
            .await
            .unwrap();
        let doc_ast = output.into_document_ast().expect("Should be DocumentAst");

        let original_id = crate::engine::content_processors::ORIGINAL_FILE_ID;
        let in_ast_context = doc_ast
            .ast_context
            .source_context
            .get_file(original_id)
            .expect("ast_context.source_context must have the original file registered");
        assert_eq!(in_ast_context.content.as_deref(), Some(ORIGINAL_CONTENT));

        let in_top_level = doc_ast
            .source_context
            .get_file(original_id)
            .expect("top-level source_context must have the original file registered");
        assert_eq!(in_top_level.content.as_deref(), Some(ORIGINAL_CONTENT));
    }

    /// **Plan 7c seam 2 — per-cell virtual files registered in both
    /// contexts** (success path). An ipynb conversion's `source.files`
    /// are ephemeral per-cell files its `Concat` pieces point at
    /// (plan decision 6: contiguous from `ORIGINAL_FILE_ID + 1`, in
    /// `Converted.files` order). `ParseDocumentStage` must register them
    /// in BOTH output contexts, lockstep with the original-file
    /// registration above; percent/spin (empty `files`) are unaffected.
    ///
    /// Named revert: remove the cell-registration loop in `run()` and
    /// this test goes RED (FileId(2)/FileId(3) absent from both
    /// contexts).
    #[tokio::test]
    async fn test_parse_document_a_plus_registers_per_cell_files() {
        use crate::engine::content_processors::ipynb;
        use crate::format::Format;
        use crate::project::{DocumentInfo, ProjectContext};
        use crate::stage::{ConversionProvenance, LoadedSource, StageContext};
        use quarto_source_map::{FileId, FileOrigin};
        use quarto_system_runtime::TempDir;
        use std::sync::Arc;

        const NOTEBOOK: &str = r#"{"cells":[{"cell_type":"markdown","id":"a1","metadata":{},"source":["first\n"]},{"cell_type":"markdown","id":"b2","metadata":{},"source":["second\n"]}],"metadata":{"kernelspec":{"name":"python3"}},"nbformat":4,"nbformat_minor":5}"#;

        struct MockRuntime;

        #[async_trait::async_trait]
        impl quarto_system_runtime::SystemRuntime for MockRuntime {
            fn file_read(
                &self,
                _path: &std::path::Path,
            ) -> quarto_system_runtime::RuntimeResult<Vec<u8>> {
                Ok(NOTEBOOK.as_bytes().to_vec())
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
            ) -> quarto_system_runtime::RuntimeResult<quarto_system_runtime::PathMetadata>
            {
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
            fn file_remove(
                &self,
                _path: &std::path::Path,
            ) -> quarto_system_runtime::RuntimeResult<()> {
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
                Ok(PathBuf::from("/"))
            }
            fn temp_dir(&self, _template: &str) -> quarto_system_runtime::RuntimeResult<TempDir> {
                Ok(TempDir::new(PathBuf::from("/tmp/test")))
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
            ) -> quarto_system_runtime::RuntimeResult<quarto_system_runtime::CommandOutput>
            {
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

        let runtime = Arc::new(MockRuntime);
        let project = ProjectContext {
            dir: PathBuf::from("/project"),
            config: crate::project::ProjectConfig::default(),
            is_single_file: true,
            output_dir: PathBuf::from("/project"),
            ..Default::default()
        };
        let doc = DocumentInfo::from_path("/project/nb.ipynb");
        let mut ctx = StageContext::new(runtime, Format::html(), project, doc).unwrap();

        let converted = ipynb::convert_notebook(std::path::Path::new("nb.ipynb"), NOTEBOOK)
            .expect("notebook must convert");
        let mut source = LoadedSource::new(
            PathBuf::from("/project/nb.ipynb"),
            converted.markdown.clone().into_bytes(),
        );
        source.conversion = Some(ConversionProvenance {
            engine: "jupyter".to_string(),
        });
        source.source_info = Some(converted.source_info.clone());
        source.files = converted.files.clone();

        let output = ParseDocumentStage::new()
            .run(PipelineData::LoadedSource(source), &mut ctx)
            .await
            .expect("two-markdown-cell notebook must parse");
        let doc_ast = output.into_document_ast().expect("Should be DocumentAst");

        let original_id = crate::engine::content_processors::ORIGINAL_FILE_ID;
        for (ctx_name, sc) in [
            ("ast_context", &doc_ast.ast_context.source_context),
            ("source_context", &doc_ast.source_context),
        ] {
            let orig = sc
                .get_file(original_id)
                .unwrap_or_else(|| panic!("{ctx_name}: original missing at ORIGINAL_FILE_ID"));
            assert_eq!(orig.content.as_deref(), Some(NOTEBOOK));
            for (i, cell_file) in converted.files.iter().enumerate() {
                let id = FileId(original_id.0 + 1 + i);
                let f = sc
                    .get_file(id)
                    .unwrap_or_else(|| panic!("{ctx_name}: cell {i} missing at {id:?}"));
                assert_eq!(f.path, cell_file.label, "{ctx_name}: cell {i} label");
                assert_eq!(
                    f.content.as_deref(),
                    Some(cell_file.text.as_str()),
                    "{ctx_name}: cell {i} content"
                );
                assert_eq!(
                    f.metadata.origin,
                    Some(FileOrigin::NotebookCell {
                        notebook_path: "/project/nb.ipynb".to_string(),
                        cell_index: i + 1,
                        cell_id: cell_file.cell_id.clone(),
                        cell_type: cell_file.cell_type.clone(),
                    }),
                    "{ctx_name}: cell {i} origin"
                );
            }
        }
    }
}
