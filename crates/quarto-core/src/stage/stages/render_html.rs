/*
 * stage/stages/render_html.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Render AST to HTML body.
 */

//! Render AST to HTML body.
//!
//! This stage renders the Pandoc AST to HTML body content using pampa's
//! HTML writer.

use async_trait::async_trait;

use crate::stage::{
    EventLevel, PipelineData, PipelineDataKind, PipelineError, PipelineStage, RenderedOutput,
    StageContext,
};
use crate::trace_event;

/// Render AST to HTML body.
///
/// This stage:
/// 1. Takes a transformed DocumentAst
/// 2. Renders it to HTML body using pampa::writers::html::write
/// 3. Returns a RenderedOutput with the HTML content
///
/// # Input
///
/// - `DocumentAst` - Transformed Pandoc AST
///
/// # Output
///
/// - `RenderedOutput` - HTML body content (not a complete document yet)
///
/// # Errors
///
/// Returns an error if rendering fails.
pub struct RenderHtmlBodyStage;

impl RenderHtmlBodyStage {
    /// Create a new RenderHtmlBodyStage.
    pub fn new() -> Self {
        Self
    }
}

impl Default for RenderHtmlBodyStage {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl PipelineStage for RenderHtmlBodyStage {
    fn name(&self) -> &str {
        "render-html-body"
    }

    fn input_kind(&self) -> PipelineDataKind {
        PipelineDataKind::DocumentAst
    }

    fn output_kind(&self) -> PipelineDataKind {
        PipelineDataKind::RenderedOutput
    }

    async fn run(
        &self,
        input: PipelineData,
        ctx: &mut StageContext,
    ) -> Result<PipelineData, PipelineError> {
        let PipelineData::DocumentAst(doc) = input else {
            return Err(PipelineError::unexpected_input(
                self.name(),
                self.input_kind(),
                input.kind(),
            ));
        };

        trace_event!(
            ctx,
            EventLevel::Debug,
            "rendering HTML body from {} blocks",
            doc.ast.blocks.len()
        );

        // Render AST to HTML body
        let mut body_buf = Vec::new();
        pampa::writers::html::write(&doc.ast, &doc.ast_context, &mut body_buf).map_err(|e| {
            PipelineError::stage_error(self.name(), format!("Failed to write HTML body: {}", e))
        })?;

        let body = String::from_utf8(body_buf).map_err(|e| {
            PipelineError::stage_error(self.name(), format!("Invalid UTF-8 in HTML body: {}", e))
        })?;

        trace_event!(
            ctx,
            EventLevel::Debug,
            "rendered {} bytes of HTML body",
            body.len()
        );

        // Calculate output path
        let output_path = ctx.output_path();

        Ok(PipelineData::RenderedOutput(RenderedOutput {
            input_path: doc.path,
            output_path,
            format: ctx.format.clone(),
            content: body,
            is_intermediate: false, // HTML body is not intermediate for HTML output
            supporting_files: vec![],
            metadata: doc.ast.meta,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::Format;
    use crate::project::{DocumentInfo, ProjectContext};
    use crate::stage::DocumentAst;
    use quarto_pandoc_types::pandoc::Pandoc;
    use quarto_source_map::SourceContext;
    use quarto_system_runtime::TempDir;
    use std::path::PathBuf;
    use std::sync::Arc;

    // Mock runtime for testing
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
        fn fetch_url(&self, _url: &str) -> quarto_system_runtime::RuntimeResult<(Vec<u8>, String)> {
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

    #[tokio::test]
    async fn test_render_empty_document() {
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

        let stage = RenderHtmlBodyStage::new();

        let doc_ast = DocumentAst {
            path: PathBuf::from("/project/test.qmd"),
            ast: Pandoc::default(),
            ast_context: pampa::pandoc::ASTContext::default(),
            source_context: SourceContext::new(),
            warnings: vec![],
        };

        let input = PipelineData::DocumentAst(doc_ast);
        let output = stage.run(input, &mut ctx).await.unwrap();

        let rendered = output
            .into_rendered_output()
            .expect("Should be RenderedOutput");
        assert_eq!(rendered.input_path, PathBuf::from("/project/test.qmd"));
        assert_eq!(rendered.output_path, PathBuf::from("/project/test.html"));
        assert!(!rendered.is_intermediate);
    }
}
