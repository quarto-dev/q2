/*
 * stage/context.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Stage execution context (the "activation frame" pattern).
 */

//! Stage execution context.
//!
//! The [`StageContext`] is the owned context passed to all pipeline stages.
//! It uses an "activation frame" pattern - all data is owned rather than
//! borrowed, eliminating lifetime complexity in async code.
//!
//! This design:
//! - Works cleanly with async (no lifetime complexity in futures)
//! - Supports potential task spawning for parallelization
//! - Allows stages to clone what they need without lifetime constraints

use std::path::PathBuf;
use std::sync::Arc;

use quarto_error_reporting::DiagnosticMessage;
use quarto_system_runtime::SystemRuntime;

use super::cancellation::Cancellation;
use super::error::PipelineError;
use super::observer::{NoopObserver, PipelineObserver};
use crate::artifact::ArtifactStore;
use crate::format::Format;
use crate::project::{DocumentInfo, ProjectContext};

/// Owned context passed to all pipeline stages.
///
/// This is the "activation frame" for stage execution - it contains
/// all the data a stage needs without lifetime parameters, making it
/// work cleanly with async and potential parallelization.
///
/// # Design Notes
///
/// The context is designed to be:
/// - **Owned**: No lifetime parameters, all data is either owned or `Arc`
/// - **Mutable**: Stages can modify artifacts and diagnostics
/// - **Observable**: Stages can emit events through the observer
/// - **Cancellable**: Long-running stages can check for cancellation
pub struct StageContext {
    // === Immutable shared data ===
    /// System runtime (filesystem, env, subprocesses).
    ///
    /// This is `Arc` for potential task spawning.
    pub runtime: Arc<dyn SystemRuntime>,

    // === Owned data (no lifetime complexity) ===
    /// Target format for this render
    pub format: Format,

    /// Project context (configuration, paths)
    pub project: ProjectContext,

    /// Information about the document being rendered
    pub document: DocumentInfo,

    /// Temporary directory for this pipeline run
    pub temp_dir: PathBuf,

    // === Mutable state ===
    /// Artifact store for dependencies and intermediates
    pub artifacts: ArtifactStore,

    /// Diagnostics (warnings, errors, info) collected during execution
    pub diagnostics: Vec<DiagnosticMessage>,

    // === Observation & Control ===
    /// Observer for tracing, progress reporting, and WASM callbacks
    pub observer: Arc<dyn PipelineObserver>,

    /// Cancellation token for graceful shutdown (Ctrl+C)
    pub cancellation: Cancellation,
}

impl StageContext {
    /// Create a new stage context.
    ///
    /// This creates a temporary directory for the pipeline run.
    ///
    /// # Errors
    ///
    /// Returns an error if the temporary directory cannot be created.
    pub fn new(
        runtime: Arc<dyn SystemRuntime>,
        format: Format,
        project: ProjectContext,
        document: DocumentInfo,
    ) -> Result<Self, PipelineError> {
        let temp_dir = runtime
            .temp_dir("quarto-pipeline")
            .map_err(|e| PipelineError::other(format!("Failed to create temp directory: {}", e)))?
            .into_path();

        Ok(Self {
            runtime,
            format,
            project,
            document,
            temp_dir,
            artifacts: ArtifactStore::new(),
            diagnostics: Vec::new(),
            observer: Arc::new(NoopObserver),
            cancellation: Cancellation::new(),
        })
    }

    /// Set a custom observer for tracing and progress.
    pub fn with_observer(mut self, observer: Arc<dyn PipelineObserver>) -> Self {
        self.observer = observer;
        self
    }

    /// Set a custom cancellation token (for CLI integration).
    pub fn with_cancellation(mut self, token: Cancellation) -> Self {
        self.cancellation = token;
        self
    }

    /// Set a custom temporary directory.
    pub fn with_temp_dir(mut self, temp_dir: PathBuf) -> Self {
        self.temp_dir = temp_dir;
        self
    }

    /// Check if cancellation has been requested.
    ///
    /// Stages should call this periodically during long-running
    /// operations to enable graceful cancellation.
    pub fn is_cancelled(&self) -> bool {
        self.cancellation.is_cancelled()
    }

    /// Add a diagnostic to the context.
    ///
    /// Diagnostics are issues (warnings, errors, info) discovered during stage execution.
    pub fn add_diagnostic(&mut self, diagnostic: DiagnosticMessage) {
        self.diagnostics.push(diagnostic);
    }

    /// Add multiple diagnostics to the context.
    pub fn add_diagnostics(&mut self, diagnostics: impl IntoIterator<Item = DiagnosticMessage>) {
        self.diagnostics.extend(diagnostics);
    }

    /// Get the output path for this render.
    ///
    /// Priority:
    /// 1. Document's explicit output path
    /// 2. Format-determined path from input
    pub fn output_path(&self) -> PathBuf {
        if let Some(ref path) = self.document.output {
            return path.clone();
        }

        // Determine from format
        let output = self.format.output_path(&self.document.input);

        // If project has output_dir, make path relative to that
        if self.project.output_dir != self.project.dir
            && let Ok(relative) = self.document.input.strip_prefix(&self.project.dir)
        {
            let mut result = self.project.output_dir.join(relative);
            result.set_extension(&self.format.output_extension);
            return result;
        }

        output
    }

    /// Get a metadata value from the format configuration.
    pub fn format_metadata(&self, key: &str) -> Option<&serde_json::Value> {
        if self.format.metadata.is_null() {
            return None;
        }
        self.format.metadata.get(key)
    }

    /// Check if this is a native Rust pipeline render.
    pub fn is_native(&self) -> bool {
        self.format.native_pipeline
    }
}

impl std::fmt::Debug for StageContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StageContext")
            .field("format", &self.format.identifier)
            .field("project_dir", &self.project.dir)
            .field("document", &self.document.input)
            .field("temp_dir", &self.temp_dir)
            .field("artifacts_count", &self.artifacts.len())
            .field("diagnostics_count", &self.diagnostics.len())
            .field("is_cancelled", &self.is_cancelled())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::ProjectContext;
    use std::sync::Arc;

    // Mock runtime for testing
    struct MockRuntime {
        temp_path: PathBuf,
    }

    impl MockRuntime {
        fn new() -> Self {
            Self {
                temp_path: PathBuf::from("/tmp/mock"),
            }
        }
    }

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
            Ok(PathBuf::from("/"))
        }

        fn temp_dir(
            &self,
            _template: &str,
        ) -> quarto_system_runtime::RuntimeResult<quarto_system_runtime::TempDir> {
            Ok(quarto_system_runtime::TempDir::new(self.temp_path.clone()))
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
                "fetch not implemented".to_string(),
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

    fn make_test_project() -> ProjectContext {
        ProjectContext {
            dir: PathBuf::from("/project"),
            config: None,
            is_single_file: true,
            files: vec![DocumentInfo::from_path("/project/test.qmd")],
            output_dir: PathBuf::from("/project"),
        }
    }

    #[test]
    fn test_context_creation() {
        let runtime = Arc::new(MockRuntime::new());
        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/test.qmd");
        let format = Format::html();

        let ctx = StageContext::new(runtime, format, project, doc).unwrap();

        assert_eq!(ctx.document.input, PathBuf::from("/project/test.qmd"));
        assert!(!ctx.is_cancelled());
        assert!(ctx.diagnostics.is_empty());
        assert!(ctx.artifacts.is_empty());
    }

    #[test]
    fn test_context_with_observer() {
        use super::super::observer::TracingObserver;

        let runtime = Arc::new(MockRuntime::new());
        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/test.qmd");
        let format = Format::html();

        let ctx = StageContext::new(runtime, format, project, doc)
            .unwrap()
            .with_observer(Arc::new(TracingObserver::new()));

        // Just verify it compiles and runs
        ctx.observer.on_pipeline_start(1);
    }

    #[test]
    fn test_context_cancellation() {
        use super::super::cancellation::Cancellation;

        let runtime = Arc::new(MockRuntime::new());
        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/test.qmd");
        let format = Format::html();

        let token = Cancellation::new();
        let ctx = StageContext::new(runtime, format, project, doc)
            .unwrap()
            .with_cancellation(token.clone());

        assert!(!ctx.is_cancelled());
        token.cancel();
        assert!(ctx.is_cancelled());
    }

    #[test]
    fn test_context_diagnostics() {
        let runtime = Arc::new(MockRuntime::new());
        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/test.qmd");
        let format = Format::html();

        let mut ctx = StageContext::new(runtime, format, project, doc).unwrap();

        assert!(ctx.diagnostics.is_empty());

        ctx.add_diagnostic(DiagnosticMessage::warning("Test warning".to_string()));
        assert_eq!(ctx.diagnostics.len(), 1);

        ctx.add_diagnostics([
            DiagnosticMessage::warning("Warning 2".to_string()),
            DiagnosticMessage::warning("Warning 3".to_string()),
        ]);
        assert_eq!(ctx.diagnostics.len(), 3);
    }

    #[test]
    fn test_context_output_path() {
        let runtime = Arc::new(MockRuntime::new());
        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/test.qmd");
        let format = Format::html();

        let ctx = StageContext::new(runtime, format, project, doc).unwrap();

        assert_eq!(ctx.output_path(), PathBuf::from("/project/test.html"));
    }

    #[test]
    fn test_context_debug() {
        let runtime = Arc::new(MockRuntime::new());
        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/test.qmd");
        let format = Format::html();

        let ctx = StageContext::new(runtime, format, project, doc).unwrap();

        let debug = format!("{:?}", ctx);
        assert!(debug.contains("StageContext"));
        assert!(debug.contains("Html")); // FormatIdentifier::Html in Debug format
    }
}
