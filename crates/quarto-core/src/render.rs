/*
 * render.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Render context for pipeline execution.
 */

//! Render context for pipeline execution.
//!
//! The `RenderContext` is the mutable state passed through all pipeline stages:
//! - Transforms can read and write to the artifact store
//! - Transforms can access project configuration and format settings
//! - Writers use the context to determine output paths

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use quarto_analysis::AnalysisContext;
use quarto_error_reporting::DiagnosticMessage;
use quarto_system_runtime::SystemRuntime;

use crate::artifact::ArtifactStore;
use std::collections::HashMap;

use crate::attribution::{
    AttributionData, AttributionRecord, AttributionSourceProvider, IdentityMap,
};
use crate::crossref::{CrossrefIndex, RefTypeRegistry};
use crate::format::Format;
use crate::project::index::ProjectIndex;
use crate::project::{DocumentInfo, ProjectContext};
use crate::resource_resolver::ResourceResolverContext;
use crate::stage::{NoopObserver, PandocIncludes, PipelineObserver};

/// Writer-side options populated by the Render-phase transforms and
/// read when constructing each writer's `*Config`. Per-format
/// sub-structs let HTML and JSON keep distinct lookup shapes without
/// either having to know about the other.
///
/// Defaults are `None` for every field so existing callers and tests
/// see no behaviour change.
#[derive(Debug, Clone, Default)]
pub struct FormatOptions {
    pub html: HtmlFormatOptions,
    pub json: JsonFormatOptions,
}

/// HTML writer-side options.
#[derive(Debug, Clone)]
pub struct HtmlFormatOptions {
    /// Walk-order slice of per-node `Option<AttributionRecord>`.
    /// `None` (outer) means "no attribution in scope" (off-path).
    /// `AttributionRecord.actor` is `Arc<str>` pointer-equal to the
    /// corresponding key in `attribution_identities`, preserving the
    /// Phase 1 interning invariant. Written by
    /// `AttributionRenderTransform`. Used as a regression invariant
    /// for the "lookup non-empty when attribution is on" contract;
    /// the writer queries `attribution_by_node` for per-node O(1)
    /// access.
    pub attribution_lookup: Option<Arc<[Option<AttributionRecord>]>>,

    /// Pointer-keyed map from AST node identity (`&Block` /
    /// `&Inline` cast through `*const ()` to `usize`) to the resolved
    /// `AttributionRecord`. The HTML writer's
    /// `write_block_source_attrs` / `write_inline_source_attrs` do a
    /// single `HashMap::get` to decide whether to emit
    /// `data-attr-*`. Pointer keys are stable because the transform
    /// is registered as the **last** Finalization-Phase entry — no
    /// later code mutates the AST.
    pub attribution_by_node: Option<Arc<HashMap<usize, AttributionRecord>>>,

    /// Identity table covering every distinct actor that appears in
    /// `runs`. Consumed by `AttributionViewerTransform` to emit one
    /// `[data-attr-actor="<id>"] { --attr-color: …; --attr-name: …; }`
    /// CSS rule per actor into `rendered.includes.header`. The HTML
    /// writer is identity-free; the browser paints colour via the
    /// cascade and `viewer.js` reads `--attr-name` from computed style
    /// for the hover badge.
    pub attribution_identities: Option<Arc<IdentityMap>>,

    /// Whether `AttributionViewerTransform` should auto-inject the
    /// default viewer CSS + JS pair (dotted underline + hover badge)
    /// into `rendered.includes.{header,after-body}`. Defaults to
    /// `true`; flipped to `false` only by the YAML opt-out
    /// `attribution: { source: git, viewer: false }`. The viewer
    /// transform additionally gates on `attribution_by_node.is_some()`,
    /// so the bool only matters when attribution is otherwise active.
    pub attribution_viewer_enabled: bool,
}

impl Default for HtmlFormatOptions {
    fn default() -> Self {
        Self {
            attribution_lookup: None,
            attribution_by_node: None,
            attribution_identities: None,
            attribution_viewer_enabled: true,
        }
    }
}

/// q2-debug JSON writer-side options.
#[derive(Debug, Clone, Default)]
pub struct JsonFormatOptions {
    /// Walk-order slice mirroring [`HtmlFormatOptions::attribution_lookup`].
    pub attribution_lookup: Option<Arc<[Option<AttributionRecord>]>>,

    /// Pointer-keyed map mirroring [`HtmlFormatOptions::attribution_by_node`].
    pub attribution_by_node: Option<Arc<HashMap<usize, AttributionRecord>>>,

    /// Actor → `(name, color)` table. Unlike the HTML path, the JSON
    /// wire dedupes — per-record entries carry only `{ s, actor, time }`
    /// and consumers join into this table for identity.
    pub attribution_actors: Option<Arc<IdentityMap>>,
}

/// Binary dependencies available for rendering
#[derive(Debug, Clone, Default)]
pub struct BinaryDependencies {
    /// dart-sass binary path (for SASS compilation)
    pub dart_sass: Option<PathBuf>,

    /// esbuild binary path (for JS bundling)
    pub esbuild: Option<PathBuf>,

    /// Pandoc binary path (for non-native formats)
    pub pandoc: Option<PathBuf>,

    /// Typst binary path
    pub typst: Option<PathBuf>,

    /// `git` binary path. Used by
    /// [`crate::attribution::GitBlameProvider`] to spawn
    /// `git blame --porcelain`. `None` when git isn't on PATH and
    /// `QUARTO_GIT` is unset; the provider degrades gracefully with
    /// a diagnostic warning in that case.
    pub git: Option<PathBuf>,
}

impl BinaryDependencies {
    /// Create empty binary dependencies
    pub fn new() -> Self {
        Self::default()
    }

    /// Discover binary dependencies from environment and PATH
    pub fn discover(runtime: &dyn SystemRuntime) -> Self {
        Self {
            dart_sass: runtime.find_binary("sass", "QUARTO_DART_SASS"),
            esbuild: runtime.find_binary("esbuild", "QUARTO_ESBUILD"),
            pandoc: runtime.find_binary("pandoc", "QUARTO_PANDOC"),
            typst: runtime.find_binary("typst", "QUARTO_TYPST"),
            git: runtime.find_binary("git", "QUARTO_GIT"),
        }
    }

    /// Check if dart-sass is available
    pub fn has_sass(&self) -> bool {
        self.dart_sass.is_some()
    }

    /// Check if Pandoc is available
    pub fn has_pandoc(&self) -> bool {
        self.pandoc.is_some()
    }
}

/// Context for a single document render operation.
///
/// This is the mutable state passed through all pipeline stages.
/// It contains:
/// - References to project and document configuration (immutable borrows)
/// - The artifact store (mutable, for collecting dependencies and intermediates)
/// - The target format
/// - Binary dependencies
///
/// `RenderContext` implements [`AnalysisContext`], allowing analysis transforms
/// from `quarto-analysis` to be used directly in the render pipeline.
pub struct RenderContext<'a> {
    /// Artifact store for dependencies and intermediates
    pub artifacts: ArtifactStore,

    /// Project context (configuration, paths)
    pub project: &'a ProjectContext,

    /// Information about the document being rendered
    pub document: &'a DocumentInfo,

    /// Target format for this render
    pub format: &'a Format,

    /// Binary dependencies
    pub binaries: &'a BinaryDependencies,

    /// Render options
    pub options: RenderOptions,

    /// Text includes to inject into the output document.
    ///
    /// Populated by shortcode transforms via `quarto.doc.include_text()`.
    /// Bridged to/from `StageContext` by `AstTransformsStage`.
    pub includes: PandocIncludes,

    /// Diagnostics (warnings, errors, info) collected during transforms
    pub diagnostics: Vec<DiagnosticMessage>,

    /// Ref-type registry: built-in + `crossref.custom` + promised-id prefixes.
    ///
    /// Populated by `PreEngineSugaringStage` before the transform pipeline
    /// runs. Bridged from `StageContext` by `AstTransformsStage`. `None` when
    /// the pipeline is invoked directly without the pre-engine stage (e.g.
    /// some unit tests).
    pub ref_type_registry: Option<RefTypeRegistry>,

    /// Per-document crossref index.
    ///
    /// Populated by `CrossrefIndexTransform` during the crossref phase;
    /// consumed by `CrossrefResolveTransform` and later by back-end renderers.
    /// Bridged to/from `StageContext` by `AstTransformsStage`.
    pub crossref_index: Option<CrossrefIndex>,

    /// Project-wide index of Pass-1 profiles.
    ///
    /// Populated by
    /// [`ProjectPipeline`](crate::project::orchestrator::ProjectPipeline)
    /// before each file's Pass-2 render, transferred into
    /// [`StageContext::project_index`](crate::stage::StageContext) by
    /// [`run_pipeline`](crate::pipeline::run_pipeline). `None` for
    /// standalone renders.
    pub project_index: Option<Arc<ProjectIndex>>,

    /// Per-page scope-aware resolver for HTML asset URLs and
    /// cross-document body links.
    ///
    /// Populated alongside `project_index` in
    /// [`crate::render_to_file::render_document_to_file`] (Phase 5
    /// constructs the resolver for `HtmlRenderConfig`; Phase 6
    /// makes the same resolver available to AST transforms via this
    /// field). `None` when no per-page resolver has been built —
    /// e.g. unit tests that construct a `RenderContext` directly,
    /// or pipeline drivers that don't go through
    /// `render_document_to_file`.
    pub resource_resolver: Option<ResourceResolverContext>,

    /// Observer for pipeline tracing.
    ///
    /// Bridged from `StageContext` by `AstTransformsStage` so that
    /// inner transforms can emit data trace events.
    pub observer: Arc<dyn PipelineObserver>,

    /// Optional provider of user-defined tree-sitter grammars. Transferred
    /// to `StageContext` by `run_pipeline` before the pipeline starts.
    ///
    /// Callers: the native CLI typically leaves this `None` and lets
    /// `CodeHighlightStage` load grammars from `_quarto/grammars/`. The
    /// browser hub-client sets this to a `JsUserGrammars` (Phase 4.3 of
    /// the syntax-highlighting plan) so JS-backed user grammars flow
    /// through the same `CodeHighlightStage` code path.
    pub user_grammar_provider: Option<Rc<RefCell<dyn quarto_highlight::UserGrammarProvider>>>,

    /// Per-document resource report (`bd-o8pr`). Mirrors
    /// [`crate::stage::StageContext::resource_report`]; `run_pipeline`
    /// transfers entries from the inner stage context back here so
    /// the caller (`render_document_to_file`) can stuff them into
    /// the per-doc render result for the orchestrator to drain.
    pub resource_report: crate::project_resources::DocumentResourceReport,

    /// Resolved listings produced by `ListingGenerateTransform` and
    /// consumed by `ListingRenderTransform`. Populated only when the
    /// host page declares a `listing:` key. Both transforms run
    /// inside `AstTransformsStage` and share this context directly,
    /// so no `StageContext` bridge is needed.
    ///
    /// This is the impl-time revision of the L3 sub-plan's D2: the
    /// original design called for round-tripping through
    /// `meta.listings.<id>` for Lua-mutation forward-compat, but
    /// per D13 there is no Lua filter slot between generate and
    /// render today. When `bd-0fd0` (Lua injection slot) lands, the
    /// natural integration point is a meta serialize/deserialize
    /// bridge at the injection boundary; this typed in-memory shape
    /// stays unchanged.
    pub resolved_listings: Vec<crate::project::listing::ResolvedListing>,

    /// Code-block decorations produced by
    /// [`CodeBlockGenerateTransform`](crate::transforms::CodeBlockGenerateTransform)
    /// and consumed by
    /// [`CodeBlockRenderTransform`](crate::transforms::CodeBlockRenderTransform).
    /// Populated only for `CodeBlock`s that carry at least one
    /// decoration-triggering attribute (filename, code-copy, code-fold,
    /// …) or that are affected by a document-level default. Both
    /// transforms run inside `AstTransformsStage` and share this
    /// context directly, so no `StageContext` bridge is needed.
    ///
    /// Keyed by [`CodeBlockDecorationKey`](crate::transforms::CodeBlockDecorationKey)
    /// — a `(file_id, start_offset, end_offset)` triple derived from
    /// the block's `SourceInfo::Original`. See the key type's docs
    /// for the non-`Original` skip rule and the user-filter timing
    /// argument that makes the key stable in practice.
    ///
    /// Decision pinned in
    /// `claude-notes/plans/2026-05-19-code-block-features.md`
    /// (sideband map preferred over a `DecoratedCodeBlock` CustomNode
    /// to avoid the nested-CustomNode complexity Q1 ran into).
    pub code_block_decorations: std::collections::HashMap<
        crate::transforms::CodeBlockDecorationKey,
        crate::transforms::CodeBlockDecoration,
    >,

    /// Opt-in signal: when `Some`, the
    /// [`AttributionGenerateTransform`](crate::transforms::AttributionGenerateTransform)
    /// will call the provider's `build()` and merge the result with
    /// any user-authored `meta.attribution.identities`.
    ///
    /// Set by the CLI flag plumbing (Phase 3c) or the WASM entry
    /// point (Phase 3b). **Read by `AttributionGenerateTransform`
    /// only.** No other transform should consult this field.
    pub attribution_provider: Option<Arc<dyn AttributionSourceProvider>>,

    /// Sidecar carrying the canonical merged attribution form
    /// between the Generate and Render stages.
    ///
    /// Written by `AttributionGenerateTransform`; read by
    /// `AttributionRenderTransform`. **No other transform reads or
    /// writes this field.** The entire Finalization Phase runs
    /// between Generate and Render with this slot populated; future
    /// Finalization transforms must treat it as opaque.
    ///
    /// `Arc` so the value travels between transforms (and into the
    /// writer config via `format_options`) without re-copying.
    pub attribution_data: Option<Arc<AttributionData>>,

    /// Per-format writer-side options bag. Populated by Render-phase
    /// transforms and read when constructing each writer's `*Config`.
    pub format_options: FormatOptions,
}

/// Options for rendering
#[derive(Debug, Clone, Default)]
pub struct RenderOptions {
    /// Whether to enable verbose/debug output
    pub verbose: bool,

    /// Whether to execute code cells (false for markdown-only engine)
    pub execute: bool,

    /// Whether to use cached execution results
    pub use_freeze: bool,

    /// Custom output path (overrides format-determined path)
    pub output_path: Option<PathBuf>,
}

impl<'a> RenderContext<'a> {
    /// Create a new render context
    pub fn new(
        project: &'a ProjectContext,
        document: &'a DocumentInfo,
        format: &'a Format,
        binaries: &'a BinaryDependencies,
    ) -> Self {
        Self {
            artifacts: ArtifactStore::new(),
            project,
            document,
            format,
            binaries,
            options: RenderOptions::default(),
            includes: PandocIncludes::default(),
            diagnostics: Vec::new(),
            ref_type_registry: None,
            crossref_index: None,
            project_index: None,
            resource_resolver: None,
            observer: Arc::new(NoopObserver),
            user_grammar_provider: None,
            resource_report: crate::project_resources::DocumentResourceReport::new(),
            resolved_listings: Vec::new(),
            code_block_decorations: std::collections::HashMap::new(),
            attribution_provider: None,
            attribution_data: None,
            format_options: FormatOptions::default(),
        }
    }

    /// Attach a project-wide [`ProjectIndex`] to this context.
    ///
    /// Called by
    /// [`ProjectPipeline`](crate::project::orchestrator::ProjectPipeline)
    /// before each file's Pass-2 render.
    pub fn with_project_index(mut self, index: Arc<ProjectIndex>) -> Self {
        self.project_index = Some(index);
        self
    }

    /// Attach a [`ResourceResolverContext`] to this context.
    ///
    /// Production callers receive their resolver through the
    /// `StageContext` ↔ `RenderContext` bridge inside the
    /// pipeline; this builder is primarily for test scaffolding
    /// and out-of-band callers (e.g. unit tests that drive a
    /// single Render transform directly without standing up a
    /// full pipeline).
    pub fn with_resource_resolver(mut self, resolver: ResourceResolverContext) -> Self {
        self.resource_resolver = Some(resolver);
        self
    }

    /// Create with custom options
    pub fn with_options(mut self, options: RenderOptions) -> Self {
        self.options = options;
        self
    }

    /// Get the output path for this render
    ///
    /// Priority:
    /// 1. Custom output path from options
    /// 2. Document's output path
    /// 3. Format-determined path from input
    pub fn output_path(&self) -> PathBuf {
        if let Some(ref path) = self.options.output_path {
            return path.clone();
        }

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

    /// Check if this is a native Rust pipeline render
    pub fn is_native(&self) -> bool {
        self.format.native_pipeline
    }
}

impl AnalysisContext for RenderContext<'_> {
    fn add_diagnostic(&mut self, msg: DiagnosticMessage) {
        self.diagnostics.push(msg);
    }
}

/// Result of a render operation
#[derive(Debug)]
pub struct RenderResult {
    /// Primary output file
    pub output_file: PathBuf,

    /// Additional files produced (lib/, resources, etc.)
    pub supporting_files: Vec<PathBuf>,

    /// Warnings generated during rendering
    pub warnings: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::DocumentInfo;

    fn make_test_project() -> ProjectContext {
        ProjectContext {
            dir: PathBuf::from("/project"),
            config: crate::project::ProjectConfig::default(),
            is_single_file: true,
            files: vec![DocumentInfo::from_path("/project/doc.qmd")],
            output_dir: PathBuf::from("/project"),
        }
    }

    fn make_test_project_with_output_dir() -> ProjectContext {
        ProjectContext {
            dir: PathBuf::from("/project"),
            config: crate::project::ProjectConfig::default(),
            is_single_file: false,
            files: vec![DocumentInfo::from_path("/project/doc.qmd")],
            output_dir: PathBuf::from("/project/_site"),
        }
    }

    // === BinaryDependencies tests ===

    #[test]
    fn test_binary_dependencies_new() {
        let deps = BinaryDependencies::new();
        assert!(deps.dart_sass.is_none());
        assert!(deps.pandoc.is_none());
        assert!(!deps.has_sass());
        assert!(!deps.has_pandoc());
    }

    #[test]
    fn test_binary_dependencies_default() {
        let deps = BinaryDependencies::default();
        assert!(deps.dart_sass.is_none());
        assert!(deps.esbuild.is_none());
        assert!(deps.pandoc.is_none());
        assert!(deps.typst.is_none());
    }

    #[test]
    fn test_binary_dependencies_has_sass_with_path() {
        let deps = BinaryDependencies {
            dart_sass: Some(PathBuf::from("/usr/bin/sass")),
            ..Default::default()
        };
        assert!(deps.has_sass());
        assert!(!deps.has_pandoc());
    }

    #[test]
    fn test_binary_dependencies_has_pandoc_with_path() {
        let deps = BinaryDependencies {
            pandoc: Some(PathBuf::from("/usr/bin/pandoc")),
            ..Default::default()
        };
        assert!(!deps.has_sass());
        assert!(deps.has_pandoc());
    }

    #[test]
    fn test_binary_dependencies_clone() {
        let deps = BinaryDependencies {
            dart_sass: Some(PathBuf::from("/usr/bin/sass")),
            pandoc: Some(PathBuf::from("/usr/bin/pandoc")),
            ..Default::default()
        };
        let cloned = deps.clone();
        assert_eq!(deps.dart_sass, cloned.dart_sass);
        assert_eq!(deps.pandoc, cloned.pandoc);
    }

    // === RenderOptions tests ===

    #[test]
    fn test_render_options_default() {
        let options = RenderOptions::default();
        assert!(!options.verbose);
        assert!(!options.execute);
        assert!(!options.use_freeze);
        assert!(options.output_path.is_none());
    }

    #[test]
    fn test_render_options_clone() {
        let options = RenderOptions {
            verbose: true,
            execute: true,
            use_freeze: false,
            output_path: Some(PathBuf::from("/output")),
        };
        let cloned = options.clone();
        assert_eq!(options.verbose, cloned.verbose);
        assert_eq!(options.execute, cloned.execute);
        assert_eq!(options.output_path, cloned.output_path);
    }

    // === RenderContext tests ===

    #[test]
    fn test_render_context_output_path() {
        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();

        let ctx = RenderContext::new(&project, &doc, &format, &binaries);
        let output = ctx.output_path();

        assert_eq!(output, PathBuf::from("/project/doc.html"));
    }

    #[test]
    fn test_render_context_custom_output() {
        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();

        let options = RenderOptions {
            output_path: Some(PathBuf::from("/custom/output.html")),
            ..Default::default()
        };

        let ctx = RenderContext::new(&project, &doc, &format, &binaries).with_options(options);
        let output = ctx.output_path();

        assert_eq!(output, PathBuf::from("/custom/output.html"));
    }

    #[test]
    fn test_render_context_document_output() {
        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/doc.qmd").with_output("/project/custom.html");
        let format = Format::html();
        let binaries = BinaryDependencies::new();

        let ctx = RenderContext::new(&project, &doc, &format, &binaries);
        let output = ctx.output_path();

        // Document output takes priority when no custom option is set
        assert_eq!(output, PathBuf::from("/project/custom.html"));
    }

    #[test]
    fn test_render_context_output_path_with_output_dir() {
        let project = make_test_project_with_output_dir();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();

        let ctx = RenderContext::new(&project, &doc, &format, &binaries);
        let output = ctx.output_path();

        // When project has output_dir different from dir, output goes there
        assert_eq!(output, PathBuf::from("/project/_site/doc.html"));
    }

    #[test]
    fn test_render_context_is_native_html() {
        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();

        let ctx = RenderContext::new(&project, &doc, &format, &binaries);
        assert!(ctx.is_native());
    }

    #[test]
    fn test_render_context_is_native_pdf() {
        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let format = Format::pdf();
        let binaries = BinaryDependencies::new();

        let ctx = RenderContext::new(&project, &doc, &format, &binaries);
        assert!(!ctx.is_native());
    }

    #[test]
    fn test_render_context_add_diagnostic() {
        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();

        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);
        assert!(ctx.diagnostics.is_empty());

        ctx.add_diagnostic(DiagnosticMessage::warning("Test warning".to_string()));
        assert_eq!(ctx.diagnostics.len(), 1);
    }

    // === RenderResult tests ===

    #[test]
    fn test_render_result() {
        let result = RenderResult {
            output_file: PathBuf::from("/output/doc.html"),
            supporting_files: vec![
                PathBuf::from("/output/lib/styles.css"),
                PathBuf::from("/output/lib/script.js"),
            ],
            warnings: vec!["Warning 1".to_string()],
        };

        assert_eq!(result.output_file, PathBuf::from("/output/doc.html"));
        assert_eq!(result.supporting_files.len(), 2);
        assert_eq!(result.warnings.len(), 1);
    }

    #[test]
    fn test_render_result_debug() {
        let result = RenderResult {
            output_file: PathBuf::from("/output.html"),
            supporting_files: vec![],
            warnings: vec![],
        };

        let debug = format!("{:?}", result);
        assert!(debug.contains("RenderResult"));
        assert!(debug.contains("output_file"));
    }
}
