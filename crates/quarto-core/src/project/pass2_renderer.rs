/*
 * project/pass2_renderer.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Pass-2 dispatch trait for `ProjectPipeline`.
 */

//! Pass-2 dispatch abstraction.
//!
//! Phase 9 introduces a trait so [`ProjectPipeline`] can drive
//! Pass-2 either natively (writing each rendered page to disk) or
//! in WASM (returning HTML + drained artifacts in-memory) without
//! duplicating the orchestration logic.
//!
//! # Implementations
//!
//! - [`RenderToFileRenderer`] (native): wraps
//!   [`crate::render_to_file::render_document_to_file`]. Returns
//!   [`crate::render_to_file::RenderToFileResult`].
//! - `RenderToHtmlRenderer` (Phase 9 sub-phase 9.2): wraps
//!   [`crate::pipeline::render_qmd_to_html`]. Returns
//!   `WasmPassTwoOutput` (HTML + diagnostics + drained artifacts).
//!
//! # Why a trait
//!
//! Q1's preview pipeline diverged from its on-disk render path,
//! which was a recurring source of subtle bugs. Q2 keeps a single
//! orchestrator and varies only the per-page Pass-2 step. New
//! consumers (e.g. the future `quarto preview` CLI) implement this
//! trait once and inherit the same Pass-1 caching, project-type
//! pre/post-render dispatch, and dependency-graph-based Mode B
//! filtering.
//!
//! [`ProjectPipeline`]: crate::project::orchestrator::ProjectPipeline

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;

use async_trait::async_trait;
use quarto_error_reporting::DiagnosticMessage;
use quarto_source_map::SourceContext;
use quarto_system_runtime::SystemRuntime;

use crate::Result;
use crate::artifact::ArtifactStore;
use crate::format::Format;
use crate::project::index::ProjectIndex;
use crate::project::{DocumentInfo, ProjectContext};
use crate::resource_resolver::ResourceResolverContext;

/// Per-page Pass-2 dispatch.
///
/// `ProjectPipeline` calls this once per document that survives
/// [`RenderMode`](crate::project::orchestrator::RenderMode) filtering.
/// The implementation drives whichever rendering function fits the
/// caller — disk-writing on native, in-memory on WASM.
///
/// # Threading state
///
/// Per-render state lives on `&mut self`. The orchestrator passes
/// the rest as method arguments — `doc_info`, `project`, `index`,
/// the runtime, and the project-wide artifact accumulator
/// (`project_artifacts`). Project-scoped artifacts produced by the
/// per-doc render are merged into this accumulator so the
/// [`ProjectType::post_render`](crate::project::orchestrator::ProjectType::post_render)
/// hook can flush them once for the whole project.
///
/// # `?Send`
///
/// Matches the rest of the pipeline's stage trait so a future
/// rayon-per-worker parallelism path doesn't require migrating
/// every renderer. Hub-client uses `wasm-bindgen-futures` which
/// is single-threaded, and on native we go through `pollster`.
#[async_trait(?Send)]
pub trait Pass2Renderer {
    /// Per-document output type.
    ///
    /// - Native ([`RenderToFileRenderer`]):
    ///   [`crate::render_to_file::RenderToFileResult`] (output paths
    ///   plus the full [`crate::pipeline::RenderOutput`]).
    /// - WASM ([`RenderToHtmlRenderer`]): [`WasmPassTwoOutput`]
    ///   carrying HTML + diagnostics + drained per-page artifacts.
    type Output;

    /// Render a single page in Pass-2.
    ///
    /// `project_artifacts` is the orchestrator-owned accumulator
    /// for project-scoped artifacts. Implementations must drain
    /// each per-doc render's project-scoped artifacts into it
    /// (so post_render can flush once).
    async fn render(
        &mut self,
        doc_info: &DocumentInfo,
        format: &Format,
        format_str: &str,
        project: &ProjectContext,
        index: Arc<ProjectIndex>,
        runtime: Arc<dyn SystemRuntime>,
        project_artifacts: &mut ArtifactStore,
    ) -> Result<Self::Output>;

    /// Extract the output's on-disk (or synthetic-VFS) path for
    /// downstream hooks that key off "which pages were rendered
    /// this run" (e.g. the sitemap merge in
    /// [`crate::project::website_post_render::write_sitemap`]).
    ///
    /// Returns `None` for renderers that produce no path-shaped
    /// artifact (the WASM in-memory renderer is the only such
    /// case today).
    fn output_path(output: &Self::Output) -> Option<&Path>;

    /// Build a resolver suitable for project-level post-render
    /// hooks like
    /// [`crate::project::website_post_render::flush_site_libs`].
    ///
    /// - Native ([`RenderToFileRenderer`]): produces a
    ///   [`ResourceResolverContext::project_root`] resolver from
    ///   the project's `output_dir` and the project type's
    ///   `lib_dir` so `on_disk_path_for(Project, p)` returns
    ///   `{output_dir}/{lib_dir}/{p}`.
    /// - WASM ([`RenderToHtmlRenderer`]): produces a
    ///   [`ResourceResolverContext::vfs_root`] resolver pointing at
    ///   the synthetic project-artifacts root (e.g.
    ///   `/.quarto/project-artifacts`), matching the URLs Pass-2
    ///   already embedded in HTML.
    ///
    /// The construction-level invariant from Phase 9 §Decision 4
    /// applies: the URL embedded in HTML by `html_url_for(Project,
    /// p)` and the on-disk write path returned by
    /// `on_disk_path_for(Project, p)` must round-trip through this
    /// resolver. Otherwise `flush_site_libs` writes artifacts to a
    /// place the rendered HTML never references.
    fn build_project_resolver(
        &self,
        project: &ProjectContext,
        lib_dir: &str,
    ) -> ResourceResolverContext;

    /// Extract this output's per-document resource report for the
    /// orchestrator to drain (`bd-o8pr` Phase 2).
    ///
    /// Returns `None` for renderers that don't run engines (e.g. the
    /// WASM hub-client preview) — engine-emitted supporting files
    /// have nowhere meaningful to land in the in-browser path. Native
    /// renders return their per-doc report; the orchestrator resolves
    /// each entry against the project root and merges with the
    /// static-channel list.
    fn extract_resource_report(
        _output: &Self::Output,
    ) -> Option<&crate::project_resources::DocumentResourceReport> {
        None
    }
}

// ───────────────────────────────────────────────────────────────────
// Native impl: writes HTML+resources to disk via render_document_to_file.
// ───────────────────────────────────────────────────────────────────

/// Native Pass-2 renderer.
///
/// Wraps [`crate::render_to_file::render_document_to_file`]: each
/// per-doc call computes the output path under the project's
/// `output-dir`, writes Page-scoped artifacts under
/// `{stem}_files/`, drains Project-scoped artifacts into
/// `project_artifacts`, and writes the rendered HTML to disk.
///
/// This is what the `quarto render` CLI uses through every code
/// path that ends up in `ProjectPipeline`.
#[cfg(not(target_arch = "wasm32"))]
pub struct RenderToFileRenderer<'a> {
    /// Borrowed render options. The reference is stored so the
    /// renderer can be threaded through `ProjectPipeline` without
    /// re-cloning the options on every per-doc call.
    pub options: &'a crate::render_to_file::RenderToFileOptions,
}

#[cfg(not(target_arch = "wasm32"))]
impl<'a> RenderToFileRenderer<'a> {
    /// Build a renderer borrowing the given options for its
    /// lifetime.
    pub fn new(options: &'a crate::render_to_file::RenderToFileOptions) -> Self {
        Self { options }
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[async_trait(?Send)]
impl<'a> Pass2Renderer for RenderToFileRenderer<'a> {
    type Output = crate::render_to_file::RenderToFileResult;

    async fn render(
        &mut self,
        doc_info: &DocumentInfo,
        _format: &Format,
        format_str: &str,
        project: &ProjectContext,
        index: Arc<ProjectIndex>,
        runtime: Arc<dyn SystemRuntime>,
        project_artifacts: &mut ArtifactStore,
    ) -> Result<Self::Output> {
        // `render_document_to_file` is sync (it calls
        // `pollster::block_on` internally for the head pipeline);
        // we run it inside an `async fn` so callers can `await` the
        // dispatch uniformly with the future WASM impl.
        crate::render_to_file::render_document_to_file(
            &doc_info.input,
            format_str,
            self.options,
            Some(project),
            runtime,
            Some(index),
            Some(project_artifacts),
        )
    }

    fn output_path(output: &Self::Output) -> Option<&Path> {
        Some(&output.output_path)
    }

    fn build_project_resolver(
        &self,
        project: &ProjectContext,
        lib_dir: &str,
    ) -> ResourceResolverContext {
        ResourceResolverContext::project_root(project.output_dir.clone(), lib_dir.to_string())
    }

    fn extract_resource_report(
        output: &Self::Output,
    ) -> Option<&crate::project_resources::DocumentResourceReport> {
        Some(&output.resource_report)
    }
}

// ───────────────────────────────────────────────────────────────────
// WASM impl: keeps HTML + drained artifacts in memory, no disk I/O.
// ───────────────────────────────────────────────────────────────────

/// The rendered payload produced by a Pass-2 WASM renderer.
///
/// `RenderToHtmlRenderer` produces [`Pass2Payload::Html`]; the
/// q2-preview renderer (added in a later commit) produces
/// [`Pass2Payload::AstJson`]. Both variants share the rest of
/// [`WasmPassTwoOutput`]'s fields (source path, diagnostics,
/// source context, page artifacts) so the orchestrator can drive
/// either renderer through the same code path and only branch on
/// the payload at the response-building tail.
#[derive(Debug)]
pub enum Pass2Payload {
    /// Rendered HTML, ready for the iframe post-processor.
    Html(String),
    /// Serialized Pandoc AST JSON, ready for the React-side
    /// q2-preview renderer.
    AstJson(String),
}

impl Pass2Payload {
    /// Returns the HTML string when this is [`Pass2Payload::Html`],
    /// otherwise `None`. Convenience for callers that statically
    /// know they invoked the HTML renderer (e.g. native tests).
    pub fn as_html(&self) -> Option<&str> {
        match self {
            Pass2Payload::Html(s) => Some(s.as_str()),
            Pass2Payload::AstJson(_) => None,
        }
    }

    /// Returns the AST JSON string when this is
    /// [`Pass2Payload::AstJson`], otherwise `None`.
    pub fn as_ast_json(&self) -> Option<&str> {
        match self {
            Pass2Payload::AstJson(s) => Some(s.as_str()),
            Pass2Payload::Html(_) => None,
        }
    }
}

/// Output of a single Pass-2 render under
/// [`RenderToHtmlRenderer`] (or, in a later commit, the q2-preview
/// renderer). Carries everything the orchestrator (and ultimately
/// the WASM caller) needs to surface back to the hub-client preview.
///
/// Cross-platform on purpose: native code rarely wants this shape,
/// but the type's `RenderToHtmlRenderer` impl block is gated to
/// targets where [`crate::pipeline::render_qmd_to_html`] is
/// reachable. The struct itself is gate-free so tests on native
/// can construct fixtures.
#[derive(Debug)]
pub struct WasmPassTwoOutput {
    /// Source `.qmd` path (as the orchestrator received it).
    pub source_path: std::path::PathBuf,
    /// The rendered payload — HTML for `RenderToHtmlRenderer`,
    /// AST JSON for the q2-preview renderer.
    pub payload: Pass2Payload,
    /// Per-page diagnostics emitted by the head pipeline plus
    /// every Pass-2 stage.
    pub diagnostics: Vec<DiagnosticMessage>,
    /// Source-context handle for translating diagnostic offsets
    /// into line/column positions on the JS side.
    pub source_context: SourceContext,
    /// Per-page (Page-scoped) artifacts produced during the
    /// render. The orchestrator's project-scoped accumulator
    /// receives Project-scoped artifacts separately (Phase 5
    /// invariant).
    pub page_artifacts: ArtifactStore,
}

impl WasmPassTwoOutput {
    /// Returns the HTML payload, panicking if the payload is not
    /// [`Pass2Payload::Html`]. Convenience for callers (notably
    /// native test fixtures) that statically know they invoked
    /// the HTML renderer.
    pub fn html(&self) -> &str {
        self.payload
            .as_html()
            .expect("WasmPassTwoOutput::html() called on non-Html payload")
    }
}

/// In-memory Pass-2 renderer used by the WASM hub-client live
/// preview.
///
/// Wraps [`crate::pipeline::render_qmd_to_html`] with a per-page
/// VFS-root resolver and produces a [`WasmPassTwoOutput`] that the
/// orchestrator returns to JS through the
/// `render_page_in_project` entry point (sub-phase 9.3).
///
/// The renderer is `Send`-free in keeping with the rest of the
/// `?Send` pipeline; on WASM this matches the single-threaded
/// `wasm-bindgen-futures` executor.
pub struct RenderToHtmlRenderer {
    /// Synthetic VFS root under which every artifact (page and
    /// project scope alike) lives in WASM. `RenderResolverContext::vfs_root`
    /// will be constructed with this root.
    vfs_root: std::path::PathBuf,

    /// Optional user-grammar provider attached by the caller. Shared
    /// across every page the renderer touches (one
    /// `RenderToHtmlRenderer` may produce many pages in `ActivePage`
    /// mode plus future multi-page modes). The pipeline is `?Send`
    /// so `Rc<RefCell<…>>` is correct on both wasm32 and on the
    /// native single-task executor used by tests. (bd-izfv)
    user_grammars: Option<Rc<RefCell<dyn quarto_highlight::UserGrammarProvider>>>,
}

impl RenderToHtmlRenderer {
    /// Build a renderer that resolves artifacts under the given
    /// synthetic VFS root.
    pub fn new(vfs_root: impl Into<std::path::PathBuf>) -> Self {
        Self {
            vfs_root: vfs_root.into(),
            user_grammars: None,
        }
    }

    /// Attach a user-grammar provider. The renderer installs it on
    /// every per-page [`crate::render::RenderContext`] before
    /// running the pipeline, so `CodeHighlightStage` consults it
    /// in preference to the native disk loader. (bd-izfv)
    pub fn with_user_grammars(
        mut self,
        provider: Rc<RefCell<dyn quarto_highlight::UserGrammarProvider>>,
    ) -> Self {
        self.user_grammars = Some(provider);
        self
    }
}

#[async_trait(?Send)]
impl Pass2Renderer for RenderToHtmlRenderer {
    type Output = WasmPassTwoOutput;

    async fn render(
        &mut self,
        doc_info: &DocumentInfo,
        format: &Format,
        _format_str: &str,
        project: &ProjectContext,
        index: Arc<ProjectIndex>,
        runtime: Arc<dyn SystemRuntime>,
        project_artifacts: &mut ArtifactStore,
    ) -> Result<Self::Output> {
        use crate::pipeline::{HtmlRenderConfig, render_qmd_to_html};
        use crate::render::{BinaryDependencies, RenderContext, RenderOptions};

        // Read source bytes from the runtime (VFS in WASM, native
        // FS in native test runs).
        let input_bytes = runtime.file_read(&doc_info.input).map_err(|e| {
            crate::error::QuartoError::other(format!(
                "Failed to read {} for Pass-2 render: {}",
                doc_info.input.display(),
                e
            ))
        })?;

        // Build a per-page resolver. In the hub-client all artifact
        // URLs land under `/.quarto/project-artifacts/...` (the
        // post-processor reads from VFS at the matching path); see
        // Phase 5 sub-plan §"`ResourceResolverContext::vfs_root`".
        let resolver = ResourceResolverContext::vfs_root(self.vfs_root.clone());

        let binaries = BinaryDependencies::new();
        let options = RenderOptions {
            verbose: false,
            execute: false,
            use_freeze: false,
            output_path: None,
        };
        let mut ctx =
            RenderContext::new(project, doc_info, format, &binaries).with_options(options);
        ctx.project_index = Some(index);
        ctx.resource_resolver = Some(resolver.clone());
        // bd-izfv: forward the renderer-attached user-grammar provider
        // (if any) to the per-page context. `run_pipeline` clones the
        // `Rc` into the inner `StageContext`, so the same handle is
        // shared across every page this renderer renders.
        ctx.user_grammar_provider = self.user_grammars.clone();

        let config = HtmlRenderConfig::with_resolver(resolver.clone());
        let source_name = doc_info.input.to_string_lossy().to_string();

        let render_output = render_qmd_to_html(
            &input_bytes,
            &source_name,
            &mut ctx,
            &config,
            runtime.clone(),
        )
        .await?;

        // Drain Project-scoped artifacts. Where they go next mirrors
        // the native `render_document_to_file` lib_dir branch
        // (`render_to_file.rs:264-297`):
        //
        // - **Shared lib dir** (e.g. websites, `lib_dir == "site_libs"`):
        //   merge into the orchestrator's accumulator so
        //   `WebsiteProjectType::post_render` can `flush_site_libs`
        //   them once across the whole project.
        // - **No shared lib dir** (default projects, `lib_dir == ""`):
        //   flush in-place via the per-page (vfs_root) resolver.
        //   `DefaultProjectType::post_render` is a no-op, so anything
        //   we leave in the accumulator would silently disappear and
        //   the iframe would VFS-miss on the theme `<link>` URL the
        //   HTML embeds (bd-87fu).
        //
        // Page-scoped artifacts on `ctx.artifacts` travel back to JS
        // alongside the HTML regardless of which branch fires.
        let drained = ctx.artifacts.drain_project_scoped();
        let lib_dir = super::orchestrator::project_type_for(project).lib_dir();
        if lib_dir.is_empty() {
            super::website_post_render::flush_site_libs(&drained, &resolver, runtime.as_ref())?;
        } else {
            project_artifacts.merge_into_project(drained).map_err(|e| {
                crate::error::QuartoError::other(format!(
                    "Project-scoped artifact merge failed for {}: {}",
                    doc_info.input.display(),
                    e
                ))
            })?;
        }

        Ok(WasmPassTwoOutput {
            source_path: doc_info.input.clone(),
            payload: Pass2Payload::Html(render_output.html),
            diagnostics: render_output.diagnostics,
            source_context: render_output.source_context,
            page_artifacts: ctx.artifacts,
        })
    }

    fn output_path(_output: &Self::Output) -> Option<&Path> {
        // The WASM renderer doesn't write to disk — there's no
        // path for `write_sitemap` to key off of, and the hub-client
        // doesn't run the sitemap hook anyway.
        None
    }

    fn build_project_resolver(
        &self,
        _project: &ProjectContext,
        _lib_dir: &str,
    ) -> ResourceResolverContext {
        // The WASM resolver collapses every artifact under
        // `{vfs_root}/{path}` regardless of scope (Phase 5's
        // `vfs_root_mode` flag), which matches the URLs Pass-2
        // already embeds in HTML. `lib_dir` is intentionally
        // ignored — the post-processor just needs to find the
        // bytes at the URL's path.
        ResourceResolverContext::vfs_root(self.vfs_root.clone())
    }
}

// ───────────────────────────────────────────────────────────────────
// q2-preview impl: produces AST JSON (not HTML), shares the same
// page/project artifact handling and `WasmPassTwoOutput` shape via
// the [`Pass2Payload`] enum.
// ───────────────────────────────────────────────────────────────────

/// In-memory Pass-2 renderer for the q2-preview format (Plan 1).
///
/// Sibling of [`RenderToHtmlRenderer`]. Wraps
/// [`crate::pipeline::render_qmd_to_preview_ast`] with the same
/// per-page VFS-root resolver pattern, and produces a
/// [`WasmPassTwoOutput`] whose payload variant is
/// [`Pass2Payload::AstJson`]. The orchestrator dispatches at the
/// response tail; everything in between (artifact draining,
/// diagnostics, source context, page artifacts) is identical to
/// `RenderToHtmlRenderer`.
pub struct RenderToPreviewAstRenderer {
    /// Synthetic VFS root under which every artifact lives in WASM.
    /// Same semantics as [`RenderToHtmlRenderer::new`].
    vfs_root: std::path::PathBuf,
}

impl RenderToPreviewAstRenderer {
    /// Build a q2-preview renderer that resolves artifacts under the
    /// given synthetic VFS root.
    pub fn new(vfs_root: impl Into<std::path::PathBuf>) -> Self {
        Self {
            vfs_root: vfs_root.into(),
        }
    }
}

#[async_trait(?Send)]
impl Pass2Renderer for RenderToPreviewAstRenderer {
    type Output = WasmPassTwoOutput;

    async fn render(
        &mut self,
        doc_info: &DocumentInfo,
        format: &Format,
        _format_str: &str,
        project: &ProjectContext,
        index: Arc<ProjectIndex>,
        runtime: Arc<dyn SystemRuntime>,
        project_artifacts: &mut ArtifactStore,
    ) -> Result<Self::Output> {
        use crate::pipeline::render_qmd_to_preview_ast;
        use crate::render::{BinaryDependencies, RenderContext, RenderOptions};

        // Read source bytes from the runtime (VFS in WASM, native FS
        // for native test runs). Identical to `RenderToHtmlRenderer`.
        let input_bytes = runtime.file_read(&doc_info.input).map_err(|e| {
            crate::error::QuartoError::other(format!(
                "Failed to read {} for Pass-2 q2-preview render: {}",
                doc_info.input.display(),
                e
            ))
        })?;

        let resolver = ResourceResolverContext::vfs_root(self.vfs_root.clone());

        let binaries = BinaryDependencies::new();
        let options = RenderOptions {
            verbose: false,
            execute: false,
            use_freeze: false,
            output_path: None,
        };
        let mut ctx =
            RenderContext::new(project, doc_info, format, &binaries).with_options(options);
        ctx.project_index = Some(index);
        ctx.resource_resolver = Some(resolver.clone());

        let source_name = doc_info.input.to_string_lossy().to_string();

        let preview_output =
            render_qmd_to_preview_ast(&input_bytes, &source_name, &mut ctx, runtime.clone())
                .await?;

        // Drain Project-scoped artifacts. Identical branching to
        // `RenderToHtmlRenderer` — shared lib dir merges into the
        // accumulator (websites use `flush_site_libs` in
        // `post_render`), no-lib-dir flushes in-place. The choice
        // is artifact-flow, not payload-flow, so HTML and q2-preview
        // share it verbatim.
        let drained = ctx.artifacts.drain_project_scoped();
        let lib_dir = super::orchestrator::project_type_for(project).lib_dir();
        if lib_dir.is_empty() {
            super::website_post_render::flush_site_libs(&drained, &resolver, runtime.as_ref())?;
        } else {
            project_artifacts.merge_into_project(drained).map_err(|e| {
                crate::error::QuartoError::other(format!(
                    "Project-scoped artifact merge failed for {}: {}",
                    doc_info.input.display(),
                    e
                ))
            })?;
        }

        Ok(WasmPassTwoOutput {
            source_path: doc_info.input.clone(),
            payload: Pass2Payload::AstJson(preview_output.ast_json),
            diagnostics: preview_output.diagnostics,
            source_context: preview_output.source_context,
            page_artifacts: ctx.artifacts,
        })
    }

    fn output_path(_output: &Self::Output) -> Option<&Path> {
        None
    }

    fn build_project_resolver(
        &self,
        _project: &ProjectContext,
        _lib_dir: &str,
    ) -> ResourceResolverContext {
        // Same coordinate system as `RenderToHtmlRenderer` —
        // `vfs_root` collapses every artifact under
        // `{vfs_root}/{path}`. q2-preview's `ResourceCollectorTransform`
        // (which runs in the q2-preview pipeline) embeds image URLs
        // using this resolver, so the iframe sees URLs that resolve
        // to the matching VFS path.
        ResourceResolverContext::vfs_root(self.vfs_root.clone())
    }
}
