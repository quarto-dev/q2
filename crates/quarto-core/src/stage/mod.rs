/*
 * stage/mod.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Unified render pipeline infrastructure.
 */

//! Unified render pipeline infrastructure.
//!
//! This module provides the core abstractions for building and executing
//! render pipelines. The pipeline is designed to:
//!
//! - Support both CLI and WASM environments
//! - Enable runtime-flexible pipeline construction
//! - Provide rich observability (tracing, progress, callbacks)
//! - Handle cancellation gracefully
//!
//! # Architecture
//!
//! The pipeline is composed of **stages**, each implementing [`PipelineStage`].
//! Data flows through stages via the [`PipelineData`] enum, which represents
//! all possible document states during rendering.
//!
//! ```text
//! LoadedSource → DocumentSource → DocumentAst → ExecutedDocument → RenderedOutput → FinalOutput
//!     ↑              ↑                ↑               ↑                 ↑              ↑
//!   Load          Convert          Parse          Execute           Render         Finalize
//! ```
//!
//! # Key Types
//!
//! - [`PipelineData`] - All data types that flow through the pipeline
//! - [`PipelineDataKind`] - Type tags for runtime validation
//! - [`PipelineStage`] - Trait for pipeline stages
//! - [`Pipeline`] - Validated sequence of stages with execution logic
//! - [`StageContext`] - Owned context passed to stages (no lifetime params)
//! - [`PipelineObserver`] - Trait for tracing/progress/callbacks
//! - [`PipelineError`] - Rich error type with stage context
//!
//! # Example
//!
//! ```ignore
//! use quarto_core::stage::{
//!     Pipeline, PipelineData, PipelineStage, StageContext,
//! };
//! use std::sync::Arc;
//!
//! // Create stages
//! let stages: Vec<Box<dyn PipelineStage>> = vec![
//!     Box::new(LoadSourceStage::new()),
//!     Box::new(ParseDocumentStage::new()),
//!     Box::new(AstTransformsStage::new(pipeline)),
//!     Box::new(RenderHtmlStage::new()),
//! ];
//!
//! // Create and validate pipeline
//! let pipeline = Pipeline::new(stages)?;
//!
//! // Create context
//! let mut ctx = StageContext::new(runtime, format, project, document)?;
//!
//! // Execute
//! let input = PipelineData::LoadedSource(source);
//! let output = pipeline.run(input, &mut ctx).await?;
//! ```
//!
//! # WASM Compatibility
//!
//! The pipeline is designed to work in WASM environments:
//!
//! - No OpenTelemetry dependency (feature-gated separately)
//! - `PipelineObserver` abstracts over different backends
//! - `SystemRuntime` provides platform-appropriate file operations
//! - Async via `async_trait` works with both tokio and wasm-bindgen-futures
//!
//! # Future: PipelinePlanner
//!
//! A future `PipelinePlanner` will construct pipelines based on:
//! - Document analysis (source type, metadata)
//! - Engine selection (Jupyter, Knitr, markdown)
//! - Format-specific stages (HTML, PDF, etc.)
//! - Project vs single-document mode

mod cancellation;
mod context;
mod data;
mod error;
mod observer;
mod pipeline;
pub mod stages;
mod traits;

// Re-export public types
pub use context::StageContext;
pub use data::{
    DocumentAst, DocumentSource, ExecutedDocument, FinalOutput, LoadedSource, PandocIncludes,
    PipelineData, PipelineDataKind, RenderedOutput, SourceType,
};
pub use error::{PipelineError, PipelineValidationError};
pub use observer::{EventLevel, NoopObserver, PipelineObserver, TracingObserver};
pub use pipeline::Pipeline;
pub use traits::PipelineStage;

// Re-export concrete stages for convenience
pub use stages::{
    ApplyTemplateStage, AstTransformsStage, EngineExecutionStage, ParseDocumentStage,
    RenderHtmlBodyStage,
};

// Re-export the trace_event macro
pub use crate::trace_event;

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use quarto_source_map::SourceInfo;
    use std::path::PathBuf;
    use std::sync::Arc;

    // A minimal mock runtime for tests
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

    // Test stages
    struct IdentityStage {
        name: &'static str,
        kind: PipelineDataKind,
    }

    #[async_trait]
    impl PipelineStage for IdentityStage {
        fn name(&self) -> &str {
            self.name
        }
        fn input_kind(&self) -> PipelineDataKind {
            self.kind
        }
        fn output_kind(&self) -> PipelineDataKind {
            self.kind
        }

        async fn run(
            &self,
            input: PipelineData,
            _ctx: &mut StageContext,
        ) -> Result<PipelineData, PipelineError> {
            Ok(input)
        }
    }

    struct TransformStage {
        name: &'static str,
        input: PipelineDataKind,
        output: PipelineDataKind,
        transform: Box<dyn Fn(PipelineData) -> PipelineData + Send + Sync>,
    }

    #[async_trait]
    impl PipelineStage for TransformStage {
        fn name(&self) -> &str {
            self.name
        }
        fn input_kind(&self) -> PipelineDataKind {
            self.input
        }
        fn output_kind(&self) -> PipelineDataKind {
            self.output
        }

        async fn run(
            &self,
            input: PipelineData,
            _ctx: &mut StageContext,
        ) -> Result<PipelineData, PipelineError> {
            Ok((self.transform)(input))
        }
    }

    fn make_test_context() -> StageContext {
        use crate::format::Format;
        use crate::project::{DocumentInfo, ProjectContext};

        let runtime = Arc::new(MockRuntime);
        let project = ProjectContext {
            dir: PathBuf::from("/project"),
            config: None,
            is_single_file: true,
            files: vec![],
            output_dir: PathBuf::from("/project"),
        };
        let document = DocumentInfo::from_path("/project/test.qmd");
        let format = Format::html();

        StageContext::new(runtime, format, project, document).unwrap()
    }

    #[test]
    fn test_integration_pipeline_creation() {
        let stages: Vec<Box<dyn PipelineStage>> = vec![Box::new(IdentityStage {
            name: "load",
            kind: PipelineDataKind::LoadedSource,
        })];

        let pipeline = Pipeline::new(stages).unwrap();
        assert_eq!(pipeline.len(), 1);
        assert_eq!(pipeline.expected_input(), PipelineDataKind::LoadedSource);
        assert_eq!(pipeline.expected_output(), PipelineDataKind::LoadedSource);
    }

    #[test]
    fn test_integration_stage_context() {
        let ctx = make_test_context();
        assert!(!ctx.is_cancelled());
        assert!(ctx.diagnostics.is_empty());
    }

    #[test]
    fn test_integration_pipeline_type_checking() {
        // Valid pipeline
        let valid_stages: Vec<Box<dyn PipelineStage>> = vec![
            Box::new(TransformStage {
                name: "load-to-source",
                input: PipelineDataKind::LoadedSource,
                output: PipelineDataKind::DocumentSource,
                transform: Box::new(|input| {
                    if let PipelineData::LoadedSource(s) = input {
                        let content = s.content_string();
                        PipelineData::DocumentSource(DocumentSource::new(
                            s.path,
                            content,
                            quarto_pandoc_types::ConfigValue::null(SourceInfo::default()),
                        ))
                    } else {
                        input
                    }
                }),
            }),
            Box::new(IdentityStage {
                name: "identity",
                kind: PipelineDataKind::DocumentSource,
            }),
        ];

        assert!(Pipeline::new(valid_stages).is_ok());

        // Invalid pipeline (type mismatch)
        let invalid_stages: Vec<Box<dyn PipelineStage>> = vec![
            Box::new(IdentityStage {
                name: "loaded",
                kind: PipelineDataKind::LoadedSource,
            }),
            Box::new(IdentityStage {
                name: "ast", // Wrong! LoadedSource output != DocumentAst input
                kind: PipelineDataKind::DocumentAst,
            }),
        ];

        assert!(Pipeline::new(invalid_stages).is_err());
    }
}
