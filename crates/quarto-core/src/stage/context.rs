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

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use quarto_error_reporting::DiagnosticMessage;
use quarto_system_runtime::SystemRuntime;

use super::cancellation::Cancellation;
use super::data::PandocIncludes;
use super::error::PipelineError;
use super::observer::{NoopObserver, PipelineObserver};
use crate::artifact::ArtifactStore;
use crate::crossref::{CrossrefIndex, RefTypeRegistry};
use crate::extension::Extension;
use crate::format::Format;
use crate::project::index::ProjectIndex;
use crate::project::{DocumentInfo, ProjectContext};
use crate::resource_resolver::ResourceResolverContext;

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

    /// Temporary directory for this pipeline run, created lazily on
    /// first [`temp_dir`](Self::temp_dir) access (bd-tky36).
    ///
    /// Most documents (pure markdown — no execution engine) never need
    /// it: the engine-execution stage's "nothing to run" fast path
    /// returns before touching it. Creating it eagerly in
    /// [`new`](Self::new) cost a per-document `mkdir` (~3% of a website
    /// render) and leaked the directory. Holding it in a `OnceLock`
    /// preserves `StageContext: Send + Sync` (`PathBuf` is both).
    temp_dir: std::sync::OnceLock<PathBuf>,

    /// Extensions discovered for this document
    pub extensions: Vec<Extension>,

    /// Project variables from `_variables.yml` (project root), parsed as
    /// document metadata so markdown values render as markdown. `None` for
    /// single-file renders (Q1 parity: `var` is project-scoped) or when the
    /// file does not exist.
    pub variables: Option<quarto_pandoc_types::ConfigValue>,

    /// Project environment variables from `_environment` files
    /// (`_environment` + `_environment.local`; profile variants once
    /// bd-ev8mk1rp lands). File-defined values only — q2 never mutates
    /// the process environment, and consumers must check the real
    /// environment first so it always wins (see
    /// [`crate::project::environment`]). Empty for single-file renders
    /// (Q1 parity: env files are project-scoped).
    pub project_env: hashlink::LinkedHashMap<String, String>,

    // === Mutable state ===
    /// Artifact store for dependencies and intermediates
    pub artifacts: ArtifactStore,

    /// Text includes to inject into the output document.
    ///
    /// Populated by engine execution (knitr/jupyter), shortcode resolution,
    /// and Lua filters via `quarto.doc.include_text()`.
    pub includes: PandocIncludes,

    /// Diagnostics (warnings, errors, info) collected during execution
    pub diagnostics: Vec<DiagnosticMessage>,

    /// Ref-type registry: built-in + `crossref.custom` + promised-id prefixes.
    ///
    /// Populated by `PreEngineSugaringStage` after metadata merge and before
    /// engine execution. Frozen thereafter. See design plan D7.
    pub ref_type_registry: Option<RefTypeRegistry>,

    /// Per-document crossref index.
    ///
    /// Populated incrementally by transforms in the crossref phase of
    /// `AstTransformsStage` and consumed by later transforms and back-end
    /// renderers. Bridged to/from `RenderContext` by `AstTransformsStage`.
    pub crossref_index: Option<CrossrefIndex>,

    /// Per-document resource report (`bd-o8pr`). Engine stages and
    /// (Phase 3) Lua-filter post-drain push raw paths into this; the
    /// orchestrator drains it after Pass-2 render and resolves
    /// against the project root. Static-channel resources go through
    /// a different path (the profile + project config), so this
    /// holds only engine + filter contributions.
    pub resource_report: crate::project_resources::DocumentResourceReport,

    /// User-resource copy intents ([`crate::render::ResourceCopyIntent`])
    /// collected by AST transforms — currently
    /// [`crate::transforms::ResourceCollectorTransform`]. Bridged
    /// to/from the inner `RenderContext::resource_copies` by
    /// [`crate::stage::stages::AstTransformsStage`], then drained
    /// into the per-render [`crate::output_sink::OutputSink`] by
    /// the outer renderer (`render_document_to_file` /
    /// `RenderToHtmlRenderer` / `RenderToPreviewAstRenderer`).
    ///
    /// See bd-cfl67 for why this lives parallel to `artifacts`
    /// rather than inside it.
    pub resource_copies: Vec<crate::render::ResourceCopyIntent>,

    // === Observation & Control ===
    /// Observer for tracing, progress reporting, and WASM callbacks
    pub observer: Arc<dyn PipelineObserver>,

    /// Cancellation token for graceful shutdown (Ctrl+C)
    pub cancellation: Cancellation,

    /// Project-wide index of Pass-1 [`DocumentProfile`]s.
    ///
    /// Populated by
    /// [`ProjectPipeline::pass_two`](crate::project::orchestrator::ProjectPipeline)
    /// before each file's Pass-2 run. `None` for standalone renders
    /// and for Pass-1 runs themselves. Phase-1 does not read this
    /// anywhere; Phase-2+ stages (sidebar generate, cross-doc link
    /// rewriting) consume it.
    ///
    /// [`DocumentProfile`]: crate::document_profile::DocumentProfile
    pub project_index: Option<Arc<ProjectIndex>>,

    /// Per-page scope-aware resolver for HTML asset URLs and
    /// cross-document body links.
    ///
    /// Populated by the top-level render entry points
    /// ([`crate::render_to_file::render_document_to_file`] for the
    /// CLI; the equivalent setup in `wasm-quarto-hub-client` for
    /// the browser). Threaded through to the inner
    /// [`RenderContext`](crate::render::RenderContext) inside
    /// [`AstTransformsStage`](crate::stage::stages::AstTransformsStage)
    /// so AST transforms (Phase 6's `LinkRewriteTransform`,
    /// future body-link / footer-text consumers) can compute
    /// page-relative URLs without re-deriving the resolver.
    pub resource_resolver: Option<ResourceResolverContext>,

    /// Optional attribution-data producer, set by the CLI
    /// `--attribution` flag plumbing or the WASM
    /// `parse_qmd_to_ast_with_attribution` entry point. Consumed by
    /// [`crate::stage::stages::AttributionGenerateStage`], which
    /// runs *before* [`UserFiltersStage::pre`] so user filters can
    /// query attribution via the `quarto.attribution.*` Lua surface.
    /// `None` means attribution is off for this render — the
    /// generate stage short-circuits and `attribution_data` stays
    /// `None`.
    pub attribution_provider: Option<Arc<dyn crate::attribution::AttributionSourceProvider>>,

    /// Per-document attribution sidecar, populated by
    /// [`crate::stage::stages::AttributionGenerateStage`] before
    /// user filters run, then bridged into the inner `RenderContext`
    /// by [`crate::stage::stages::AstTransformsStage`] so
    /// `AttributionRenderTransform` can bake the writer-side lookup
    /// table. The Lua host binding registered by
    /// [`crate::stage::stages::UserFiltersStage`] reads this directly
    /// to back the `quarto.attribution.*` namespace. `None` when no
    /// provider was installed.
    pub attribution_data: Option<Arc<crate::attribution::AttributionData>>,

    /// Per-format writer-side options, populated by Render-phase
    /// transforms inside [`crate::stage::stages::AstTransformsStage`]
    /// and bridged back here after the inner pipeline runs. Consumed
    /// by downstream stages that build the writer config (e.g.
    /// [`crate::stage::stages::RenderHtmlBodyStage`] passes the
    /// HTML sub-bag to `pampa::writers::html`). Defaults to all
    /// `None` fields so existing stages and tests see no behaviour
    /// change.
    pub format_options: crate::render::FormatOptions,

    /// Optional provider of user-defined tree-sitter grammars, consulted
    /// by `CodeHighlightStage` before falling back to the built-in
    /// registry. Set by the top-level render entry points:
    ///
    /// - **Native CLI**: typically left `None` here; the stage builds a
    ///   native `UserGrammars` on-demand from `<project.dir>/_quarto/grammars/`.
    ///   (An advanced native caller could set this directly to override
    ///   the disk-scan behavior; no caller does so today.)
    /// - **Browser (hub-client)**: set to a `JsUserGrammars` handle that
    ///   forwards highlight requests to JS callbacks backed by
    ///   `web-tree-sitter`. See `crates/wasm-quarto-hub-client/src/lib.rs`.
    pub user_grammar_provider: Option<Rc<RefCell<dyn quarto_highlight::UserGrammarProvider>>>,
}

impl StageContext {
    /// Create a new stage context.
    ///
    /// The pipeline's temporary directory is **not** created here — it
    /// is allocated lazily on the first [`temp_dir`](Self::temp_dir)
    /// call (bd-tky36), so documents that never run an execution engine
    /// pay no `mkdir` and leak no directory.
    ///
    /// # Errors
    ///
    /// Returns an error if extension discovery fails. (Temp-dir
    /// creation errors now surface from [`temp_dir`](Self::temp_dir).)
    pub fn new(
        runtime: Arc<dyn SystemRuntime>,
        format: Format,
        project: ProjectContext,
        document: DocumentInfo,
    ) -> Result<Self, PipelineError> {
        let builtin_ext_path = builtin_extensions_path(runtime.as_ref());
        let (extensions, mut startup_diagnostics) = crate::extension::discover_extensions(
            &document.input,
            if project.is_single_file {
                None
            } else {
                Some(&project.dir)
            },
            builtin_ext_path.as_deref(),
            runtime.as_ref(),
        );

        let variables =
            load_project_variables(runtime.as_ref(), &project, &mut startup_diagnostics);

        // Active profiles are always empty until bd-ev8mk1rp lands
        // render-profile support.
        let project_env = crate::project::environment::load_project_environment(
            runtime.as_ref(),
            &project,
            &[],
            &mut startup_diagnostics,
        );

        Ok(Self {
            runtime,
            format,
            project,
            document,
            temp_dir: std::sync::OnceLock::new(),
            extensions,
            variables,
            project_env,
            artifacts: ArtifactStore::new(),
            includes: PandocIncludes::default(),
            diagnostics: startup_diagnostics,
            ref_type_registry: None,
            crossref_index: None,
            resource_report: crate::project_resources::DocumentResourceReport::new(),
            resource_copies: Vec::new(),
            project_index: None,
            resource_resolver: None,
            observer: Arc::new(NoopObserver),
            cancellation: Cancellation::new(),
            attribution_provider: None,
            attribution_data: None,
            format_options: crate::render::FormatOptions::default(),
            user_grammar_provider: None,
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

    /// The pipeline's temporary directory, created on first access and
    /// cached for the lifetime of this context (bd-tky36).
    ///
    /// Only callers that genuinely need scratch space (execution
    /// engines) hit this; pure-markdown renders never do, so the
    /// directory is never created for them.
    ///
    /// # Errors
    ///
    /// Returns an error if the directory cannot be created.
    pub fn temp_dir(&self) -> Result<&Path, PipelineError> {
        if let Some(path) = self.temp_dir.get() {
            return Ok(path.as_path());
        }
        let path = self
            .runtime
            .temp_dir("quarto-pipeline")
            .map_err(|e| PipelineError::other(format!("Failed to create temp directory: {}", e)))?
            .into_path();
        // Each `StageContext` is single-threaded, so the only way the
        // cell is already set here is a benign race we don't expect;
        // `set` returning `Err` just means someone else won — use
        // whatever value ended up stored.
        let _ = self.temp_dir.set(path);
        Ok(self
            .temp_dir
            .get()
            .expect("temp_dir was just set")
            .as_path())
    }

    /// Set a custom temporary directory, pre-seeding the lazy cell so
    /// [`temp_dir`](Self::temp_dir) returns it without asking the
    /// runtime to create one.
    pub fn with_temp_dir(self, temp_dir: PathBuf) -> Self {
        // Replace any previously-cached value. `new()` leaves the cell
        // empty, so in the common builder usage this just seeds it.
        let _ = self.temp_dir.set(temp_dir);
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
            .field("temp_dir", &self.temp_dir.get())
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
        /// Number of times `temp_dir` has been called — lets tests
        /// assert lazy temp-dir creation (bd-tky36).
        temp_dir_calls: std::sync::atomic::AtomicUsize,
    }

    impl MockRuntime {
        fn new() -> Self {
            Self {
                temp_path: PathBuf::from("/tmp/mock"),
                temp_dir_calls: std::sync::atomic::AtomicUsize::new(0),
            }
        }

        fn temp_dir_calls(&self) -> usize {
            self.temp_dir_calls
                .load(std::sync::atomic::Ordering::Relaxed)
        }
    }

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
            Ok(PathBuf::from("/"))
        }

        fn temp_dir(
            &self,
            _template: &str,
        ) -> quarto_system_runtime::RuntimeResult<quarto_system_runtime::TempDir> {
            self.temp_dir_calls
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
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

        async fn fetch_url(
            &self,
            _url: &str,
        ) -> quarto_system_runtime::RuntimeResult<(Vec<u8>, String)> {
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
            config: crate::project::ProjectConfig::default(),
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

    /// bd-tky36: the per-document temp dir must be created lazily — only
    /// on first access, not in `new()`. Pure-markdown documents never
    /// touch it (the engine-execution fast path returns first), so they
    /// must not pay the per-page `mkdir` (or leak the directory).
    #[test]
    fn temp_dir_created_lazily_on_first_access() {
        let runtime = Arc::new(MockRuntime::new());
        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/test.qmd");
        let format = Format::html();

        let ctx = StageContext::new(runtime.clone(), format, project, doc).unwrap();
        assert_eq!(
            runtime.temp_dir_calls(),
            0,
            "StageContext::new must NOT create a temp dir eagerly",
        );

        let first = ctx.temp_dir().unwrap().to_path_buf();
        assert_eq!(
            runtime.temp_dir_calls(),
            1,
            "first temp_dir() access must create the dir exactly once",
        );

        let second = ctx.temp_dir().unwrap().to_path_buf();
        assert_eq!(
            runtime.temp_dir_calls(),
            1,
            "subsequent temp_dir() accesses must return the cached dir",
        );
        assert_eq!(first, second, "temp_dir() must be stable across calls");
    }

    /// `with_temp_dir` pre-seeds the cell: the accessor returns the
    /// explicit path and the runtime is never asked to create one.
    #[test]
    fn with_temp_dir_pins_path_without_runtime_call() {
        let runtime = Arc::new(MockRuntime::new());
        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/test.qmd");
        let format = Format::html();

        let explicit = PathBuf::from("/explicit/temp");
        let ctx = StageContext::new(runtime.clone(), format, project, doc)
            .unwrap()
            .with_temp_dir(explicit.clone());

        assert_eq!(ctx.temp_dir().unwrap(), explicit.as_path());
        assert_eq!(
            runtime.temp_dir_calls(),
            0,
            "with_temp_dir must not trigger runtime temp-dir creation",
        );
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

/// Resolve the path to built-in extensions.
///
/// - **Native**: extracts the embedded `ResourceBundle` to a temp dir.
/// - **WASM**: returns the VFS path `/__quarto_resources__/extensions`
///   if it exists (populated during WASM init).
/// Load `<project dir>/_variables.yml` for the `var` shortcode.
///
/// Project-scoped like Q1 (no variables in single-file mode). A missing
/// file is normal; a file that exists but fails to parse produces a
/// warning diagnostic naming the file.
fn load_project_variables(
    runtime: &dyn SystemRuntime,
    project: &crate::project::ProjectContext,
    diagnostics: &mut Vec<DiagnosticMessage>,
) -> Option<quarto_pandoc_types::ConfigValue> {
    if project.is_single_file {
        return None;
    }
    let path = project.dir.join("_variables.yml");
    let content = runtime.file_read_string(&path).ok()?;
    match quarto_yaml::parse_file(&content, &path.display().to_string()) {
        Ok(yaml) => {
            let mut collector = pampa::utils::diagnostic_collector::DiagnosticCollector::new();
            Some(pampa::pandoc::yaml_to_config_value(
                yaml,
                quarto_pandoc_types::InterpretationContext::DocumentMetadata,
                &mut collector,
            ))
        }
        Err(e) => {
            diagnostics.push(
                quarto_error_reporting::DiagnosticMessageBuilder::warning(
                    "Project variables not loaded",
                )
                .problem(format!(
                    "`{}` could not be parsed: {}; `var` shortcodes will not resolve",
                    path.display(),
                    e
                ))
                .build(),
            );
            None
        }
    }
}

use crate::extension::builtin_extensions_path;
