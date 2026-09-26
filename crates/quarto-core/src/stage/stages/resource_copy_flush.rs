/*
 * stage/stages/resource_copy_flush.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * book-projects P2c: flush queued resource copies before
 * `TypstCompileStage` reads them off disk.
 */

//! `ResourceCopyFlushStage` — flushes `ctx.resource_copies` through a real
//! [`crate::output_sink::OutputSink`] *before* [`super::TypstCompileStage`]
//! shells out to `typst compile`.
//!
//! Unlike docx/pptx (pandoc's own writer embeds a referenced image's bytes
//! directly into the document at *write* time, reading from the image's
//! already-resolved source path — it never touches the output directory),
//! a Typst render has a second, later subprocess (`TypstCompileStage`)
//! that reads the intermediate `.typ` file's referenced images directly
//! off disk, relative to the file's own location — i.e. from the output
//! directory. Draining `ctx.resource_copies` only at the very end of
//! `finalize_rendered_output` (after the whole pipeline, `TypstCompileStage`
//! included, has already run) is too late: `typst compile` fails with
//! "file not found" before the copy that would have supplied the image
//! ever runs. Confirmed not book-specific — any Pandoc-hybrid Typst-compile
//! render whose output directory differs from the source directory and
//! references a local image is affected.
//!
//! This stage performs the identical drain-and-copy
//! `finalize_rendered_output` does (same `OutputSink`/
//! `enqueue_resource_copies` machinery — the bd-cfl67 allowed-roots
//! validation is preserved, not bypassed), just earlier — inserted between
//! [`super::PandocWriteStage`] and [`super::TypstCompileStage`] in
//! [`crate::pipeline::build_pandoc_pipeline_stages`]. `finalize_rendered_output`'s
//! own drain becomes a no-op afterward (`ctx.resource_copies` is empty by
//! then, via `std::mem::take`), so nothing is copied twice.
//!
//! Native-only, like `PandocWriteStage`/`TypstCompileStage`: exists only on
//! the Pandoc-hybrid Typst leg, which is native-only end to end.

use async_trait::async_trait;

use crate::output_sink::OutputSink;
use crate::resource_copy_diagnostics::{copy_failure_error, enqueue_resource_copies};
use crate::stage::{PipelineData, PipelineDataKind, PipelineError, PipelineStage, StageContext};

pub struct ResourceCopyFlushStage;

impl ResourceCopyFlushStage {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ResourceCopyFlushStage {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait(?Send)]
impl PipelineStage for ResourceCopyFlushStage {
    fn name(&self) -> &str {
        "resource-copy-flush"
    }

    fn input_kind(&self) -> PipelineDataKind {
        PipelineDataKind::RenderedOutput
    }

    fn output_kind(&self) -> PipelineDataKind {
        PipelineDataKind::RenderedOutput
    }

    async fn run(
        &self,
        input: PipelineData,
        ctx: &mut StageContext,
    ) -> Result<PipelineData, PipelineError> {
        let PipelineData::RenderedOutput(rendered) = input else {
            return Err(PipelineError::unexpected_input(
                self.name(),
                self.input_kind(),
                input.kind(),
            ));
        };

        let resource_copies = std::mem::take(&mut ctx.resource_copies);
        if resource_copies.is_empty() {
            return Ok(PipelineData::RenderedOutput(rendered));
        }

        let allowed_roots = ctx
            .resource_resolver
            .as_ref()
            .map(|r| r.allowed_output_roots())
            .unwrap_or_default();
        let mut sink = OutputSink::new(allowed_roots);
        let warnings = enqueue_resource_copies(resource_copies, &mut sink, ctx.runtime.as_ref())
            .map_err(|e| PipelineError::stage_error(self.name(), e.to_string()))?;
        ctx.diagnostics.extend(warnings);

        sink.flush(ctx.runtime.as_ref())
            .map_err(|e| match copy_failure_error(&e) {
                crate::error::QuartoError::Parse(parse_err) => {
                    PipelineError::stage_error_with_diagnostics(self.name(), parse_err.diagnostics)
                }
                other => PipelineError::stage_error(self.name(), other.to_string()),
            })?;

        Ok(PipelineData::RenderedOutput(rendered))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::Format;
    use crate::project::{DocumentInfo, ProjectContext};
    use crate::stage::RenderedOutput;
    use quarto_pandoc_types::ConfigValue;
    use quarto_source_map::SourceInfo;
    use std::path::PathBuf;
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

    fn make_ctx() -> StageContext {
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
        let format = Format::docx();
        StageContext::new(runtime, format, project, doc).unwrap()
    }

    fn make_rendered_output() -> RenderedOutput {
        RenderedOutput {
            input_path: PathBuf::from("/project/test.qmd"),
            output_path: PathBuf::from("/project/test.typ"),
            format: Format::docx(),
            content: String::new(),
            is_intermediate: true,
            supporting_files: vec![],
            metadata: ConfigValue::new_map(vec![], SourceInfo::for_test()),
            source_context: quarto_source_map::SourceContext::new(),
        }
    }

    #[tokio::test]
    async fn empty_resource_copies_is_passthrough() {
        let mut ctx = make_ctx();
        assert!(ctx.resource_copies.is_empty());
        let stage = ResourceCopyFlushStage::new();
        let input = PipelineData::RenderedOutput(make_rendered_output());
        let output = stage.run(input, &mut ctx).await.unwrap();
        assert!(output.into_rendered_output().is_some());
    }

    #[test]
    fn stage_kinds_are_rendered_output() {
        let stage = ResourceCopyFlushStage::new();
        assert_eq!(stage.input_kind(), PipelineDataKind::RenderedOutput);
        assert_eq!(stage.output_kind(), PipelineDataKind::RenderedOutput);
    }

    /// The whole point of this stage: after it runs, `ctx.resource_copies`
    /// must be empty — proving `finalize_rendered_output`'s later drain
    /// really would be a no-op, not relying on the full compile-and-inspect
    /// integration test alone.
    #[tokio::test]
    async fn drains_resource_copies_to_empty() {
        let mut ctx = make_ctx();
        ctx.resource_resolver = Some(
            crate::resource_resolver::ResourceResolverContext::single_doc(
                PathBuf::from("/project/test.qmd"),
                "test",
            ),
        );
        ctx.resource_copies.push(crate::render::ResourceCopyIntent {
            src: PathBuf::from("/project/img/dot.png"),
            dest: PathBuf::from("/project/img/dot.png"),
            origin: SourceInfo::for_test(),
        });
        let stage = ResourceCopyFlushStage::new();
        let input = PipelineData::RenderedOutput(make_rendered_output());
        stage.run(input, &mut ctx).await.unwrap();
        assert!(
            ctx.resource_copies.is_empty(),
            "resource_copies must be drained so finalize_rendered_output's \
             later flush is a no-op, not a double-copy"
        );
    }
}
