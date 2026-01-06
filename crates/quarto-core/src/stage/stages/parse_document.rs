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
    DocumentAst, EventLevel, PipelineData, PipelineDataKind, PipelineError, PipelineStage,
    StageContext,
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

#[async_trait]
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
        let source_name = source.path.display().to_string();
        source_context.add_file(source_name.clone(), Some(content_str));

        // Parse the QMD content
        let mut output_stream = std::io::sink();
        let parse_result = pampa::readers::qmd::read(
            &source.content,
            false,        // loose mode
            &source_name, // filename for error messages
            &mut output_stream,
            true, // track source locations
            None, // file_id
        );

        match parse_result {
            Ok((ast, ast_context, warnings)) => {
                // Log any warnings
                if !warnings.is_empty() {
                    trace_event!(
                        ctx,
                        EventLevel::Debug,
                        "parsing produced {} warnings",
                        warnings.len()
                    );
                    // Also add warnings to context for pipeline-level collection
                    ctx.add_warnings(warnings.clone());
                }

                Ok(PipelineData::DocumentAst(DocumentAst {
                    path: source.path,
                    ast,
                    ast_context,
                    source_context,
                    warnings,
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
            fn fetch_url(
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
            config: None,
            is_single_file: true,
            files: vec![],
            output_dir: PathBuf::from("/project"),
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
}
