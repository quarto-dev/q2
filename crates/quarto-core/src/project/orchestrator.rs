/*
 * project/orchestrator.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * `ProjectType` trait + two-pass driver.
 */

//! Project orchestration.
//!
//! # Two passes
//!
//! The driver ([`ProjectPipeline`]) runs every file in a project
//! through **two passes**:
//!
//! - **Pass 1** advances each file as far as
//!   [`DocumentProfileStage`](crate::stage::DocumentProfileStage) —
//!   parse + metadata merge only. Pass 1 extracts each file's
//!   [`DocumentProfile`](crate::document_profile::DocumentProfile),
//!   collecting the full `Vec` into a
//!   [`ProjectIndex`](super::index::ProjectIndex).
//! - **Pass 2** runs the full per-file render, with the Pass-1
//!   `ProjectIndex` available on
//!   [`StageContext::project_index`](crate::stage::StageContext).
//!   Phase-1 stages do not consume it; Phase-2+ (sidebar generate,
//!   cross-doc link rewriting) will.
//!
//! Between the two passes, the project's [`ProjectType`]
//! implementation runs its `pre_render` hook. After Pass 2 it runs
//! `post_render`.
//!
//! # Pass-2 resumption (v1)
//!
//! Phase 1 v1 re-runs the head pipeline inside Pass 2. This wastes a
//! parse + metadata-merge per file versus resuming from the cloned
//! `PipelineData::AtProfile`. The re-work is accepted for v1 because
//! it keeps the CLI rewiring scoped to *orchestration* — threading a
//! pre-built `AtProfile` through `render_document_to_file` is a
//! separate refactor.  A follow-up beads issue tracks the
//! optimization.

use async_trait::async_trait;

use quarto_error_reporting::DiagnosticMessage;

use crate::Result;

use super::index::ProjectIndex;
use super::{ProjectContext, ProjectKind};

// Phase 9 sub-phase 9.1 lifted these from `cfg(not(wasm32))` so
// `ProjectPipeline` can drive Pass-1 / Pass-2 on WASM as well as
// natively. The orchestration logic is now platform-agnostic; only
// the disk-writing renderer (`RenderToFileRenderer`) and the
// disk-writing post-render hooks (`copy_favicon`, `write_sitemap`,
// `write_robots_txt`) stay native-only.
use std::sync::Arc;

use quarto_system_runtime::SystemRuntime;

use crate::error::QuartoError;

use crate::format::Format;

use super::DocumentInfo;

use super::pass2_renderer::Pass2Renderer;

// `RenderToFileOptions` and `RenderToFileRenderer` reference
// `render_document_to_file`, which is native-only. Stay gated to
// native; WASM callers wire their own renderer through
// `ProjectPipeline::with_renderer`.
#[cfg(not(target_arch = "wasm32"))]
use crate::render_to_file::{RenderToFileOptions, RenderToFileResult};

#[cfg(not(target_arch = "wasm32"))]
use super::pass2_renderer::RenderToFileRenderer;

// WASM-visible placeholder for `RenderToFileResult` so the
// `ProjectType::post_render` trait signature
// (`outputs: &[RenderToFileResult]`) compiles on WASM. The slice is
// always empty there because the WASM Pass-2 renderer's `Output` is
// a `WasmPassTwoOutput` (sub-phase 9.2) and the orchestrator's
// `run_wasm` driver substitutes `&[]` when calling `post_render`.
#[cfg(target_arch = "wasm32")]
#[derive(Debug)]
pub struct RenderToFileResult;

/// Orchestration hooks implemented by each project kind.
///
/// Phase-1 ships [`DefaultProjectType`] (no-op hooks used for
/// single-file and loose-directory renders) and
/// [`WebsiteProjectType`] (identical placeholder; Phase-2+ will fill
/// in the website-specific hooks).
///
/// Trait methods are `async` because future website hooks (sitemap
/// writing, favicon copying, remote resource fetches) want async I/O.
/// The no-op default implementations mean current callers pay zero
/// cost. The `?Send` bound matches the pipeline's own stage trait so
/// an eventual `rayon + pollster-per-worker` parallelism path does
/// not require migrating the rest of the stage graph.
#[async_trait(?Send)]
pub trait ProjectType {
    /// The tag this implementation serves.
    fn kind(&self) -> ProjectKind;

    /// Name of the project's shared "lib" directory (e.g.
    /// `"site_libs"` for websites). The empty string indicates
    /// the project type has **no** shared lib directory: in that
    /// case [`ArtifactScope::Project`] artifacts resolve under
    /// the same per-page resource directory as
    /// [`ArtifactScope::Page`] artifacts (preserving pre-Phase-5
    /// single-doc behavior).
    ///
    /// Returns an owned `String` rather than `&'static str` so
    /// implementations can later read the value from
    /// [`ProjectContext::config`] when the user-config override
    /// (`project.lib-dir:`) lands without churning this trait
    /// signature.
    ///
    /// See `claude-notes/plans/2026-04-24-websites-phase-5.md`
    /// Decision 4 for the design rationale.
    ///
    /// [`ArtifactScope::Project`]: crate::artifact::ArtifactScope::Project
    /// [`ArtifactScope::Page`]: crate::artifact::ArtifactScope::Page
    fn lib_dir(&self) -> String {
        String::new()
    }

    /// Called once per project, after Pass 1 and before Pass 2.
    /// Default: no-op.
    async fn pre_render(&self, _project: &mut ProjectContext, _index: &ProjectIndex) -> Result<()> {
        Ok(())
    }

    /// Called once per project, after Pass 2. Default: no-op.
    ///
    /// **Phase 5:** receives the orchestrator's project-wide
    /// artifact accumulator (filled by per-doc Pass-2 renders
    /// merging their drained Project-scoped artifacts). Project
    /// types with a non-empty [`Self::lib_dir`] (e.g. websites)
    /// flush this through `resolver.on_disk_path_for(...)`, which
    /// routes to either `{output_dir}/{lib_dir}/...` (native) or
    /// `{vfs_root}/...` (WASM hub-client).
    ///
    /// Project types with an empty `lib_dir` (default / book /
    /// manuscript today) receive an empty accumulator — Pass-2
    /// renderers detect the empty-lib-dir case and flush
    /// Project-scoped artifacts in-place via the per-page
    /// resolver instead of routing them through this hook
    /// (see `render_to_file::render_document_to_file` on native
    /// and `pass2_renderer::RenderToHtmlRenderer` on WASM).
    ///
    /// **Phase 7:** `diagnostics` is a project-level diagnostic
    /// channel surfaced through [`ProjectRenderSummary`]. The
    /// website hook uses it for non-fatal warnings (e.g.
    /// `website.favicon` references a missing source file).
    ///
    /// **Phase 9 sub-phase 9.2:** the `outputs` parameter became
    /// `output_paths: &[PathBuf]` (just the on-disk render targets,
    /// the only field native sitemap merge actually needed) so the
    /// trait remains cross-platform when WASM renderers (whose
    /// `Pass2Renderer::Output` is not `RenderToFileResult`) use it.
    /// Native renders extract paths via `R::output_path`; WASM
    /// renders pass an empty slice. The resolver argument is new
    /// — Phase 9 §Decision 4. It's the single source of truth for
    /// "where do Project-scope artifacts live on disk / VFS", so
    /// hooks like `flush_site_libs` no longer reconstruct the path
    /// math themselves.
    async fn post_render(
        &self,
        _project: &ProjectContext,
        _index: &ProjectIndex,
        _output_paths: &[std::path::PathBuf],
        _project_artifacts: &crate::artifact::ArtifactStore,
        _resolver: &crate::resource_resolver::ResourceResolverContext,
        _runtime: &dyn quarto_system_runtime::SystemRuntime,
        _diagnostics: &mut Vec<DiagnosticMessage>,
    ) -> Result<()> {
        Ok(())
    }
}

/// No-op orchestration used for the [`ProjectKind::Default`] tag.
///
/// Every single-file and loose-directory render in Phase 1 runs
/// through this type. The orchestrator invariant from Phase 0
/// ("no `is_project?` branch") is satisfied because even a bare file
/// is just a `DefaultProjectType` project with one entry in
/// `project.files`.
pub struct DefaultProjectType;

#[async_trait(?Send)]
impl ProjectType for DefaultProjectType {
    fn kind(&self) -> ProjectKind {
        ProjectKind::Default
    }
}

/// Placeholder website implementation for Phase 1.
///
/// Phase 1 only needs the tag to dispatch correctly. Phase 2 adds
/// sidebar / navbar generate transforms, Phase 5 adds `site_libs/`,
/// Phase 7 adds the post-render hooks (sitemap, favicon).
pub struct WebsiteProjectType;

#[async_trait(?Send)]
impl ProjectType for WebsiteProjectType {
    fn kind(&self) -> ProjectKind {
        ProjectKind::Website
    }

    fn lib_dir(&self) -> String {
        "site_libs".to_string()
    }

    /// Run the website post-render hooks.
    ///
    /// **Cross-platform** (Phase 9 sub-phase 9.2):
    /// 1. **`flush_site_libs`** (Phase 5; resolver-driven) — drain
    ///    Project-scoped artifacts to whatever destination the
    ///    resolver decides — `<output_dir>/site_libs/...` natively
    ///    or `/.quarto/project-artifacts/...` in the hub-client
    ///    VFS.
    ///
    /// **Native-only** (the rest write into the on-disk
    /// `<output_dir>` which doesn't exist in the in-browser
    /// preview):
    ///
    /// 2. **`copy_favicon`** (Phase 7) — copy `website.favicon`
    ///    from the project root to the output dir.
    /// 3. **`write_sitemap`** (Phase 7) — emit `sitemap.xml` when
    ///    `website.site-url` is set.
    /// 4. **`write_robots_txt`** (Phase 7) — emit `robots.txt`
    ///    (user-provided file wins; otherwise auto-generate when
    ///    `website.site-url` is set).
    ///
    /// Each hook short-circuits cleanly when its triggering config
    /// is absent, so a website project that opts in to none of
    /// these features just runs the site_libs flush.
    async fn post_render(
        &self,
        project: &ProjectContext,
        index: &ProjectIndex,
        output_paths: &[std::path::PathBuf],
        project_artifacts: &crate::artifact::ArtifactStore,
        resolver: &crate::resource_resolver::ResourceResolverContext,
        runtime: &dyn quarto_system_runtime::SystemRuntime,
        diagnostics: &mut Vec<DiagnosticMessage>,
    ) -> Result<()> {
        use super::website_post_render::flush_site_libs;
        flush_site_libs(project_artifacts, resolver, runtime)?;
        // The remaining hooks write to the on-disk output dir,
        // which only exists natively. WASM hub-client renders skip
        // them — see Phase 9 plan §Decision 4.
        #[cfg(not(target_arch = "wasm32"))]
        {
            use super::website_post_render::{copy_favicon, write_robots_txt, write_sitemap};
            copy_favicon(project, runtime, diagnostics)?;
            write_sitemap(project, index, output_paths, runtime)?;
            write_robots_txt(project, runtime)?;
            // L7 (`bd-qf7r`): replace listing description / image
            // placeholder envelopes with engine-rendered preview
            // content read from sibling outputs. Bracketed feature
            // — see `super::listing::post_render_upgrade` header
            // comment for the discipline this enforces.
            super::listing::post_render_upgrade::substitute_listing_placeholders(
                project,
                output_paths,
                runtime,
                diagnostics,
            )?;
            // L9 (`bd-o90m`): finalize staged RSS feeds. Walks
            // `project.output_dir` for `*.feed-{full|partial|metadata}-staged`
            // files emitted by `ListingFeedStageTransform`,
            // substitutes the description-element placeholder
            // envelopes against engine-rendered sibling HTML
            // (using a per-call sibling-read cache), writes the
            // final `.xml`, and removes the staged file. Runs
            // *after* L7 so any host-page HTML that L7 rewrote
            // is finalized before the L9 reader extractors read
            // sibling content.
            super::listing::feed::complete_staged_feeds(project, runtime, diagnostics)?;
        }
        // Suppress unused-warnings on WASM where the cfg block above
        // is empty. The signature is fixed by the trait, and these
        // arguments are real (just unused on this target).
        #[cfg(target_arch = "wasm32")]
        {
            let _ = (project, index, output_paths, diagnostics);
        }
        Ok(())
    }
}

/// Factory: pick a [`ProjectType`] based on the project's tag.
///
/// Unknown / not-yet-implemented tags fall back to
/// [`DefaultProjectType`] so Phase 1 doesn't crash on `_quarto.yml`
/// files declaring `project.type: book` or `project.type: manuscript`
/// — those kinds are tracked by Phase-1 dispatch but have no behavior
/// yet.
pub fn project_type_for(project: &ProjectContext) -> Box<dyn ProjectType> {
    match project.project_kind() {
        ProjectKind::Default => Box::new(DefaultProjectType),
        ProjectKind::Website => Box::new(WebsiteProjectType),
        ProjectKind::Book | ProjectKind::Manuscript => Box::new(DefaultProjectType),
    }
}

/// Error reported for a single file whose Pass-1 profile or
/// Pass-2 render failed.
///
/// `diagnostics` and `source_context` are populated when the
/// underlying error is a structured [`crate::error::ParseError`]
/// — i.e. the parser attached source-located diagnostics to it.
/// For other error variants (`Io`, `Other`, …) `diagnostics` is
/// empty and `source_context` is `None`; the user-facing message
/// still surfaces through `error`.
///
/// The `error` string is `e.to_string()`, which for
/// `QuartoError::Parse` already includes the rendered ariadne
/// snippet — so the CLI's text path doesn't need to consult the
/// structured fields. The hub-client / WASM path uses the
/// structured fields to produce Monaco markers and the in-app
/// preview overlay (bd-mwtf, bd-rqba).
#[derive(Debug)]
pub struct FileFailure {
    pub input: std::path::PathBuf,
    pub error: String,
    pub diagnostics: Vec<DiagnosticMessage>,
    pub source_context: Option<quarto_source_map::SourceContext>,
}

/// Build a [`FileFailure`] from a [`QuartoError`], extracting
/// structured diagnostics + source context when the error is a
/// [`ParseError`](crate::error::ParseError).
fn file_failure_from_error(input: std::path::PathBuf, e: QuartoError) -> FileFailure {
    let (diagnostics, source_context) = match &e {
        QuartoError::Parse(pe) => (pe.diagnostics.clone(), Some(pe.source_context.clone())),
        _ => (Vec::new(), None),
    };
    FileFailure {
        error: e.to_string(),
        input,
        diagnostics,
        source_context,
    }
}

/// Result of a full project render, generic over the per-page
/// output type produced by the configured
/// [`Pass2Renderer`](super::pass2_renderer::Pass2Renderer).
///
/// The native `quarto render` path uses `O = RenderToFileResult`;
/// the WASM hub-client path (Phase 9 sub-phase 9.2) uses
/// `O = WasmPassTwoOutput`. Defaulting to `RenderToFileResult` on
/// native keeps existing call sites source-compatible.
#[derive(Debug, Default)]
pub struct ProjectRenderSummary<O = RenderToFileResult> {
    /// Successful per-file outputs (in `project.files` order).
    pub outputs: Vec<O>,
    /// Pass-1 files that could not be profiled. These are dropped
    /// from the index but do not abort the run.
    pub pass1_failures: Vec<FileFailure>,
    /// Pass-2 files that failed to render. The CLI decides whether
    /// this is a non-zero exit.
    pub pass2_failures: Vec<FileFailure>,
    /// Project-level diagnostics emitted by
    /// [`ProjectType::post_render`] (Phase 7+). These are
    /// non-fatal warnings (e.g. missing favicon source) — failures
    /// surface as a returned `Err` instead.
    pub project_diagnostics: Vec<DiagnosticMessage>,
    /// Compiled theme CSS fingerprint, recovered from the
    /// `css:theme:<fingerprint>` project-scoped artifact key
    /// produced by `CompileThemeCssStage` (Plan 2A item 11).
    /// `None` if no theme artifact was produced (e.g. q2-debug
    /// renders, projects with no `theme:` YAML key).
    ///
    /// Surfaced here so the WASM hub-client bridge can post it on
    /// `RenderResponse.theme_fingerprint` without needing access
    /// to the orchestrator's private artifact store.
    pub theme_fingerprint: Option<String>,
}

impl<O> ProjectRenderSummary<O> {
    /// True if any file (Pass 1 or Pass 2) failed.
    pub fn has_failures(&self) -> bool {
        !self.pass1_failures.is_empty() || !self.pass2_failures.is_empty()
    }
}

/// Two-pass project render driver (native only for Phase 1).
///
/// Wraps a [`ProjectContext`] and a [`ProjectType`] implementation,
/// runs Pass 1 over every file in `project.files`, builds a
/// [`ProjectIndex`], invokes `pre_render`, runs Pass 2, then
/// `post_render`.
///
/// Phase-1 restriction: **sequential**. A follow-up beads issue
/// tracks `rayon + pollster-per-worker` parallelism.
///
/// WASM note: the driver exists only on native targets — hub-client
/// orchestration is Phase 9 of the epic and will wire its own
/// VFS-aware entry points (`build_project_nav`, `render_page_in_project`).
/// What subset of the project to render.
///
/// - [`RenderMode::Full`] — render every page in the project.
///   This is what `quarto render` (no path arg) does and is the
///   default.
/// - [`RenderMode::Subset`] — render only the user-named pages
///   plus any always-render pages whose reverse dependencies
///   intersect them. Used by `quarto render foo.qmd`,
///   `quarto render foo/`, and `quarto render a.qmd b.qmd c.qmd`.
/// - [`RenderMode::ActivePage`] (Phase 9) — render exactly the
///   named page. No dependency-graph augmentation: the hub-client
///   live preview only ever has one page on screen, so always-
///   render-dependent siblings are out of scope (the user can't
///   see them). This is the default mode for the WASM
///   `render_page_in_project` entry point.
///
/// All modes still do a full Pass-1 over every project page —
/// the dependency graph builder needs every profile to derive
/// sidebar / body-link / nav-dependency edges correctly, and the
/// Phase 8 profile cache makes the warm-path Pass-1 cost
/// negligible. Mode B's optimization is in Pass-2: only the
/// augmented target set runs filters, engines, and rendering.
/// Mode `ActivePage` further restricts Pass-2 to a single file.
///
/// A later optimization may reduce Mode B's Pass-1 to a partial
/// walk (target → sibling closure), but doing so safely requires
/// resolving the sidebar-`auto:` chicken-and-egg (membership
/// resolution consults the index, which doesn't yet exist for
/// non-target pages). Filed as a Phase-8 follow-up.
#[derive(Debug, Clone)]
pub enum RenderMode {
    Full,
    Subset(std::collections::HashSet<std::path::PathBuf>),
    ActivePage(std::path::PathBuf),
}

impl Default for RenderMode {
    fn default() -> Self {
        RenderMode::Full
    }
}

/// Two-pass project render driver.
///
/// Generic over [`Pass2Renderer`] so the same orchestration logic
/// drives both native renders (writing HTML to disk via
/// [`RenderToFileRenderer`]) and the WASM hub-client live preview
/// (returning HTML in-memory via `RenderToHtmlRenderer`,
/// sub-phase 9.2). Pass-1 caching, the dependency-graph
/// `RenderMode::Subset` augmentation, and `pre_render`/`post_render`
/// dispatch are platform-agnostic — only the per-doc Pass-2 step
/// varies between back-ends.
pub struct ProjectPipeline<'a, R: Pass2Renderer> {
    project: &'a mut ProjectContext,
    project_type: Box<dyn ProjectType>,
    format: Format,
    format_str: String,
    runtime: Arc<dyn SystemRuntime>,
    /// Render mode. Default `Full`; CLI subset args (Mode B) set
    /// this via [`with_mode`](Self::with_mode) before `run()`.
    mode: RenderMode,
    /// Project-wide artifact accumulator (Phase 5).
    ///
    /// Project-scoped artifacts produced by per-doc Pass-2
    /// renders are drained from the per-doc `StageContext` and
    /// merged into this store by the orchestrator. After Pass 2
    /// completes, [`ProjectType::post_render`] flushes the
    /// accumulated artifacts to disk.
    ///
    /// The orchestrator is the **only** owner that mutates this
    /// store; per-doc workers never touch it. This is what makes
    /// the design ready for future rayon-per-worker parallelism
    /// without redesign — see
    /// `claude-notes/plans/2026-04-24-websites-phase-5.md`
    /// Decision 2.
    project_artifacts: crate::artifact::ArtifactStore,
    /// Per-page Pass-2 dispatch (Phase 9 sub-phase 9.0).
    ///
    /// Native callers wire [`RenderToFileRenderer`] via
    /// [`Self::new`]; the WASM hub-client (Phase 9 sub-phase 9.2)
    /// supplies its own implementation via
    /// [`Self::with_renderer`].
    renderer: R,
}

#[cfg(not(target_arch = "wasm32"))]
impl<'a> ProjectPipeline<'a, RenderToFileRenderer<'a>> {
    /// Build a pipeline that writes per-page output to disk via
    /// [`crate::render_to_file::render_document_to_file`].
    ///
    /// This is the constructor every native call site (`quarto
    /// render`, every integration test) uses. For non-disk
    /// renderers (e.g. the Phase-9 WASM in-memory renderer) call
    /// [`Self::with_renderer`] instead.
    pub fn new(
        project: &'a mut ProjectContext,
        project_type: Box<dyn ProjectType>,
        format: Format,
        format_str: impl Into<String>,
        options: &'a RenderToFileOptions,
        runtime: Arc<dyn SystemRuntime>,
    ) -> Self {
        Self::with_renderer(
            project,
            project_type,
            format,
            format_str,
            runtime,
            RenderToFileRenderer::new(options),
        )
    }
}

impl<'a, R: Pass2Renderer> ProjectPipeline<'a, R> {
    /// Build a pipeline with an explicit Pass-2 renderer.
    ///
    /// Used by Phase 9 to wire the WASM in-memory renderer; native
    /// callers use [`Self::new`] which constructs a
    /// [`RenderToFileRenderer`] for them.
    pub fn with_renderer(
        project: &'a mut ProjectContext,
        project_type: Box<dyn ProjectType>,
        format: Format,
        format_str: impl Into<String>,
        runtime: Arc<dyn SystemRuntime>,
        renderer: R,
    ) -> Self {
        Self {
            project,
            project_type,
            format,
            format_str: format_str.into(),
            runtime,
            mode: RenderMode::Full,
            project_artifacts: crate::artifact::ArtifactStore::new(),
            renderer,
        }
    }

    /// Set the render mode. Default is [`RenderMode::Full`]; CLI
    /// subset args set this to [`RenderMode::Subset`] before
    /// calling [`run`](Self::run).
    pub fn with_mode(mut self, mode: RenderMode) -> Self {
        self.mode = mode;
        self
    }

    /// Read-only view of the project-wide artifact accumulator
    /// (Phase 5). Useful for tests and for `post_render`
    /// implementations that take `&self` on the trait.
    pub fn project_artifacts(&self) -> &crate::artifact::ArtifactStore {
        &self.project_artifacts
    }

    /// Run Pass 1 → `pre_render` → Pass 2 → `post_render`.
    ///
    /// Cross-platform since Phase 9 sub-phase 9.2: every step
    /// dispatches through the [`Pass2Renderer`] (for per-doc
    /// rendering, output-path extraction, and project-resolver
    /// construction) and the [`ProjectType`] trait (for the
    /// pre/post hooks). Native and WASM share the same body — only
    /// the renderer and project-type implementations differ.
    pub async fn run(&mut self) -> Result<ProjectRenderSummary<R::Output>> {
        let initial_diagnostics = self.empty_render_set_diagnostic();

        let (profiles, pass1_failures) = self.pass_one().await;
        let index = Arc::new(ProjectIndex::new(profiles));

        // Map hook errors through so the caller sees exactly which
        // hook failed. The plan specifies hook failures abort the
        // project render entirely (unlike per-file failures).
        self.project_type
            .pre_render(self.project, &index)
            .await
            .map_err(|e| QuartoError::other(format!("pre_render failed: {e}")))?;

        // Phase 8.2: in Mode B, build the dependency graph from
        // the freshly-loaded profiles, augment the user-named
        // targets with always-render dependents, and tell pass_two
        // to render only that set. Mode A renders every page; the
        // augmented set is `None`.
        let augmented_render_set = self.compute_augmented_render_set(&index);

        // Skip Pass-2 on files that failed Pass 1 — Pass 2 does
        // strictly more work, so it can only produce duplicate errors.
        let skip: std::collections::HashSet<std::path::PathBuf> =
            pass1_failures.iter().map(|f| f.input.clone()).collect();
        let (outputs, pass2_failures) = self
            .pass_two(index.clone(), &skip, augmented_render_set.as_ref())
            .await;

        // Phase 9 sub-phase 9.2: extract on-disk paths from the
        // per-doc outputs (`None` for in-memory WASM renders) and
        // build a project-level resolver for the `post_render`
        // hook to consume.
        let output_paths: Vec<std::path::PathBuf> = outputs
            .iter()
            .filter_map(|o| R::output_path(o).map(|p| p.to_path_buf()))
            .collect();
        let lib_dir = self.project_type.lib_dir();
        let resolver = self.renderer.build_project_resolver(self.project, &lib_dir);

        let mut project_diagnostics: Vec<DiagnosticMessage> = initial_diagnostics;
        self.project_type
            .post_render(
                self.project,
                &index,
                &output_paths,
                &self.project_artifacts,
                &resolver,
                self.runtime.as_ref(),
                &mut project_diagnostics,
            )
            .await
            .map_err(|e| QuartoError::other(format!("post_render failed: {e}")))?;

        // bd-o8pr Phases 1 + 2: copy resources to the output dir.
        // - Phase 1 (static channel): project- and document-level
        //   YAML `resources:` declarations frozen on `ProjectConfig`
        //   and `DocumentProfile`.
        // - Phase 2 (engine channel): per-doc `DocumentResourceReport`
        //   populated by `EngineExecutionStage` from
        //   `ExecuteResult.supporting_files`. Drained here from each
        //   per-doc output via `R::extract_resource_report`.
        // Runs after every project type's post_render so the project
        // type doesn't need to implement this itself. Native only —
        // the WASM hub-client preview doesn't write to a real output
        // dir.
        #[cfg(not(target_arch = "wasm32"))]
        {
            let mut resolved =
                crate::project_resources::collect_static_resources(self.project, &index)
                    .map_err(|e| QuartoError::other(e.to_string()))?;
            for output in &outputs {
                if let Some(report) = R::extract_resource_report(output) {
                    if report.is_empty() {
                        continue;
                    }
                    let resolved_report = crate::project_resources::resolve_reported_resources(
                        &self.project.dir,
                        report,
                    )
                    .map_err(|e| QuartoError::other(e.to_string()))?;
                    resolved.extend(resolved_report);
                }
            }
            crate::project_resources::copy_resources_to_output_dir(
                &resolved,
                &self.project.output_dir,
                self.runtime.as_ref(),
            )?;

            // bd-o8pr Phase 4: emit `.quarto/render-manifest.json`
            // describing the rendered files + every published
            // resource (with `origin` for diagnostics). Becomes the
            // canonical input to `quarto publish`; the existing
            // dir-walk path remains as a fallback when no manifest
            // is present.
            let rendered_files: Vec<String> = output_paths
                .iter()
                .map(|p| {
                    p.strip_prefix(&self.project.output_dir)
                        .unwrap_or(p)
                        .to_string_lossy()
                        .replace('\\', "/")
                })
                .collect();
            let manifest = crate::project_resources::RenderManifest::new(
                &self.project.dir,
                rendered_files,
                &resolved,
            );
            crate::project_resources::write_render_manifest(
                &self.project.dir,
                &manifest,
                self.runtime.as_ref(),
            )?;
        }

        // Recover the theme fingerprint from the `css:theme:<fp>`
        // artifact key. The compile-theme-css stage stores its output
        // with this key, fingerprint-suffixed; the bytes also live
        // under a path the post_render flushes to disk / VFS.
        let theme_fingerprint = self
            .project_artifacts
            .get_by_prefix("css:theme:")
            .first()
            .and_then(|(key, _)| key.strip_prefix("css:theme:"))
            .map(|s| s.to_string());

        Ok(ProjectRenderSummary {
            outputs,
            pass1_failures,
            pass2_failures,
            project_diagnostics,
            theme_fingerprint,
        })
    }

    /// Detect "the project will render zero files" before Pass-1
    /// starts and emit a project-level diagnostic. Returns an empty
    /// vec when the situation does not apply.
    ///
    /// Fires under [`RenderMode::Full`] (CLI no-arg case) and
    /// [`RenderMode::ActivePage`] (hub-client live preview) when
    /// `project.files` is empty. The CLI dispatcher already
    /// guarantees `RenderMode::Subset` carries at least one explicit
    /// target, so we don't fire there.
    ///
    /// Without this check, the orchestrator returns a successful
    /// summary with empty `outputs` / `pass1_failures` /
    /// `pass2_failures`, and the CLI silently exits 0 — the
    /// confusing behavior reported in bd-h736.
    fn empty_render_set_diagnostic(&self) -> Vec<DiagnosticMessage> {
        if !self.project.files.is_empty() {
            return Vec::new();
        }
        if matches!(self.mode, RenderMode::Subset(_)) {
            return Vec::new();
        }
        let project_dir = self.project.dir.display().to_string();
        let mut diag = DiagnosticMessage::error("Project has no renderable files");
        diag.code = Some("Q-PROJECT-EMPTY".to_string());
        diag.problem = Some(quarto_error_reporting::MessageContent::from(format!(
            "Project at `{project_dir}` resolved to an empty render set.",
        )));
        let has_render_patterns = !self.project.config.render_patterns.is_empty();
        let hint = if has_render_patterns {
            "Check `project.render` in `_quarto.yml` — its globs matched no `.qmd` files."
        } else {
            "Add a `.qmd` file to the project, or remove `_quarto.yml` to render a single \
             standalone document."
        };
        diag.hints
            .push(quarto_error_reporting::MessageContent::from(hint));
        vec![diag]
    }

    /// Advance every file to the profile checkpoint, collecting
    /// profiles and any per-file failures.
    ///
    /// Phase 8 sub-phase 8.2: each file's profile lookup goes
    /// through the [`profile_cache`](crate::project::profile_cache)
    /// before the head pipeline runs. On hit, the cached profile is
    /// returned directly. On miss, the head pipeline runs and the
    /// resulting profile is saved.
    async fn pass_one(
        &self,
    ) -> (
        Vec<crate::document_profile::DocumentProfile>,
        Vec<FileFailure>,
    ) {
        let mut profiles = Vec::with_capacity(self.project.files.len());
        let mut failures = Vec::new();
        for doc_info in &self.project.files {
            match self.profile_with_cache(doc_info).await {
                Ok(profile) => profiles.push(profile),
                Err(e) => failures.push(file_failure_from_error(doc_info.input.clone(), e)),
            }
        }
        (profiles, failures)
    }

    /// Compute the [`pass1_key`](crate::project::cache_key::pass1_key)
    /// for a document, then try the cache, falling back to a live
    /// Pass-1 on miss.
    ///
    /// The cache load verifies the cached profile's
    /// [`includes`](crate::document_profile::DocumentProfile::includes)
    /// against current file bytes — see
    /// [`profile_cache::load`](crate::project::profile_cache::load).
    /// Any include drift triggers a miss.
    async fn profile_with_cache(
        &self,
        doc_info: &DocumentInfo,
    ) -> Result<crate::document_profile::DocumentProfile> {
        // Read source bytes — used for both the cache key
        // computation and the live pipeline below.
        let source_bytes = self.runtime.file_read(&doc_info.input).map_err(|e| {
            QuartoError::other(format!(
                "Failed to read {} during pass 1: {}",
                doc_info.input.display(),
                e
            ))
        })?;

        // Compute the project-relative source path the same way
        // DocumentProfileStage does. The math goes through
        // canonicalize when possible (matches the orchestrator's
        // file-discovery path); otherwise falls back to file_name.
        let source_path = self.project_relative_source_path(&doc_info.input);
        let format_id = self.format.target_format.clone();

        // Layered _metadata.yml raw bytes for the cache-key domain.
        // We re-read the raw bytes (not the parsed ConfigValue)
        // because byte-for-byte changes invalidate the key — a
        // comment-only edit to _metadata.yml correctly invalidates
        // the cache, which is intentional v1 behavior.
        let metadata_files = self.layered_metadata_raw_bytes(&doc_info.input);

        // _quarto.yml raw bytes (project root). Empty when the
        // project has no config file (single-file render).
        let quarto_yml_bytes = self.read_quarto_yml_bytes();

        // Format extensions: TODO follow-up. For v1 we pass empty
        // contributions; extension changes require `--clean`.
        // See plan §"Sub-phase 8.4" for the user-facing escape
        // hatch and §Decision 2 footnote for the rationale.
        let extension_contributions: Vec<(String, Vec<u8>)> = Vec::new();

        let key_inputs = crate::project::cache_key::Pass1KeyInputs {
            format_id: &format_id,
            source_path: &source_path,
            source_bytes: &source_bytes,
            metadata_files: &metadata_files,
            quarto_yml_bytes: &quarto_yml_bytes,
            extension_contributions: &extension_contributions,
        };
        let key_bytes = crate::project::cache_key::pass1_key(&key_inputs);
        let key_hex = crate::project::cache_key::hex_encode(&key_bytes);

        // Cache lookup. The include resolver reads each include's
        // current bytes and computes their SHA-256 to compare
        // against the cached profile's recorded content_hashes.
        // A miss here (load returns Ok(None)) just means we do a
        // live extraction and overwrite.
        let runtime = self.runtime.clone();
        let include_resolver = move |path: &std::path::Path| {
            let bytes = runtime.file_read(path)?;
            Ok(crate::document_profile::IncludeEntry::hash_bytes(&bytes))
        };

        match crate::project::profile_cache::load(self.runtime.as_ref(), &key_hex, include_resolver)
            .await
        {
            Ok(Some(profile)) => return Ok(profile),
            // Cache miss / verification failure / runtime error
            // (e.g. cache directory unwritable): degrade to a live
            // extraction. We don't surface the error because the
            // orchestrator never wants a cache hiccup to abort an
            // otherwise-fine render.
            Ok(None) | Err(_) => {}
        }

        // Cache miss → live extraction.
        let profile = self
            .profile_single_file_live(doc_info, &source_bytes)
            .await?;

        // Best-effort save. A save failure here is also non-fatal
        // — the profile is already computed and downstream code can
        // use it for this run; the next run will just retry the
        // cache write.
        let _ =
            crate::project::profile_cache::save(self.runtime.as_ref(), &key_hex, &profile).await;

        Ok(profile)
    }

    /// Run the head pipeline live (no cache lookup). Used by
    /// [`profile_with_cache`] when the cache misses.
    async fn profile_single_file_live(
        &self,
        doc_info: &DocumentInfo,
        source_bytes: &[u8],
    ) -> Result<crate::document_profile::DocumentProfile> {
        use crate::pipeline::run_pipeline;
        use crate::render::{BinaryDependencies, RenderContext};
        use crate::stage::{
            DocumentProfileStage, IncludeExpansionStage, LinkResolutionStage, MetadataMergeStage,
            ParseDocumentStage, PipelineStage,
        };

        let source_name = doc_info.input.to_string_lossy().to_string();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(self.project, doc_info, &self.format, &binaries);

        let stages: Vec<Box<dyn PipelineStage>> = vec![
            Box::new(ParseDocumentStage::new()),
            Box::new(MetadataMergeStage::new()),
            // Include-expansion threads child content through the
            // profile so transitive `{{< include … >}}` is visible
            // (bd-xfwx). Phase 8 sub-phase 8.0d's LinkResolutionStage
            // also depends on it: the AST walk must see post-include
            // content so a body link inside an included child counts
            // as a dependency edge of the parent.
            Box::new(IncludeExpansionStage::new()),
            Box::new(DocumentProfileStage::new()),
            // Pass-1 cross-doc body-link resolution. Reads the
            // post-include AST, writes
            // `profile.body_link_targets` for the dependency graph.
            Box::new(LinkResolutionStage::new()),
        ];

        let (output, _diagnostics) = run_pipeline(
            source_bytes,
            &source_name,
            &mut ctx,
            self.runtime.clone(),
            stages,
        )
        .await?;

        let profile = output.into_at_profile().ok_or_else(|| {
            QuartoError::other(
                "Pass 1 did not produce an AtProfile variant — pipeline shape unexpected",
            )
        })?;
        Ok(profile.profile)
    }

    /// Helper: project-relative source path for cache-key
    /// construction. Mirrors `DocumentProfileStage`'s computation
    /// (canonicalize when possible, fall back to file name).
    fn project_relative_source_path(&self, input: &std::path::Path) -> String {
        if let Ok(rel) = input.strip_prefix(&self.project.dir) {
            rel.to_string_lossy().into_owned()
        } else if let Some(name) = input.file_name() {
            name.to_string_lossy().into_owned()
        } else {
            input.to_string_lossy().into_owned()
        }
    }

    /// Helper: walk the layered `_metadata.yml` files for a doc
    /// and return their raw bytes for cache-key hashing. Skips any
    /// file that can't be read; the cache key for that doc just
    /// reflects the available subset (matching what the head
    /// pipeline would see).
    fn layered_metadata_raw_bytes(
        &self,
        document_path: &std::path::Path,
    ) -> Vec<(std::path::PathBuf, Vec<u8>)> {
        if self.project.is_single_file {
            return Vec::new();
        }
        let document_path = self
            .runtime
            .canonicalize(document_path)
            .unwrap_or_else(|_| document_path.to_path_buf());
        let document_dir = match document_path.parent() {
            Some(p) => p,
            None => return Vec::new(),
        };
        let project_dir = &self.project.dir;
        let relative_path = match document_dir.strip_prefix(project_dir) {
            Ok(rel) => rel,
            Err(_) => return Vec::new(),
        };

        let mut out = Vec::new();
        let mut current = project_dir.clone();
        for component in relative_path.components() {
            current = current.join(component);
            for candidate in ["_metadata.yml", "_metadata.yaml"] {
                let path = current.join(candidate);
                if let Ok(bytes) = self.runtime.file_read(&path) {
                    out.push((path.clone(), bytes));
                    break; // prefer .yml; if both exist this picks .yml
                }
            }
        }
        out
    }

    /// Helper: read the project's `_quarto.yml` (or `_quarto.yaml`)
    /// raw bytes for cache-key hashing. Empty `Vec` when the
    /// project has no config file.
    fn read_quarto_yml_bytes(&self) -> Vec<u8> {
        if self.project.is_single_file {
            return Vec::new();
        }
        for candidate in ["_quarto.yml", "_quarto.yaml"] {
            let path = self.project.dir.join(candidate);
            if let Ok(bytes) = self.runtime.file_read(&path) {
                return bytes;
            }
        }
        Vec::new()
    }

    /// Compute the absolute-path render set for Pass-2 dispatch.
    ///
    /// - [`RenderMode::Full`] → returns `None`, meaning "render
    ///   every page" (the existing default).
    /// - [`RenderMode::Subset(targets)`] → builds the dependency
    ///   graph, augments the user-named targets with always-render
    ///   pages whose reverse-closure intersects them, and returns
    ///   the result as absolute paths matching `DocumentInfo.input`
    ///   on each project file.
    /// - [`RenderMode::ActivePage(path)`] (Phase 9) → returns the
    ///   single named page, no graph augmentation. Hub-client live
    ///   preview only renders the page on screen; always-render
    ///   siblings aren't user-visible.
    ///
    /// `targets` are absolute paths (CLI args have been canonicalized
    /// by the caller). They're translated to project-relative paths
    /// for the graph query (which keys on
    /// [`DocumentProfile::source_path`]) and back to absolute paths
    /// for `pass_two` filtering.
    fn compute_augmented_render_set(
        &self,
        index: &Arc<ProjectIndex>,
    ) -> Option<std::collections::HashSet<std::path::PathBuf>> {
        // Phase 9: ActivePage skips graph augmentation entirely —
        // the hub-client preview renders exactly one page.
        if let RenderMode::ActivePage(target) = &self.mode {
            let mut set = std::collections::HashSet::new();
            set.insert(target.clone());
            return Some(set);
        }

        let RenderMode::Subset(target_abs_paths) = &self.mode else {
            return None;
        };

        // Translate absolute target paths to project-relative
        // forward-slash form (matches DocumentProfile.source_path).
        let mut target_relatives: Vec<std::path::PathBuf> = Vec::new();
        for abs in target_abs_paths {
            let rel = if let Ok(r) = abs.strip_prefix(&self.project.dir) {
                r.to_path_buf()
            } else if let Some(name) = abs.file_name() {
                std::path::PathBuf::from(name)
            } else {
                abs.clone()
            };
            target_relatives.push(rel);
        }

        // Build the dependency graph and augment.
        let merged_meta = self.project.config.metadata.clone().unwrap_or_default();
        let mut graph_diags: Vec<DiagnosticMessage> = Vec::new();
        let graph = crate::project::dependency_graph::ProjectDependencyGraph::build(
            index,
            &merged_meta,
            &mut graph_diags,
        );
        let augmented_relatives =
            graph.augment_targets_with_always_render(target_relatives.iter().map(|p| p.as_path()));

        // Translate back to the absolute paths Pass-2 filters on.
        // Look each augmented project-relative path up in
        // `self.project.files` (which carries absolute paths via
        // `DocumentInfo.input`).
        let mut out: std::collections::HashSet<std::path::PathBuf> =
            std::collections::HashSet::new();
        for doc_info in &self.project.files {
            let rel = self.project_relative_source_path(&doc_info.input);
            if augmented_relatives.contains(&std::path::PathBuf::from(&rel)) {
                out.insert(doc_info.input.clone());
            }
        }
        Some(out)
    }

    /// Re-render every file under the built `ProjectIndex`,
    /// skipping files that failed Pass 1.
    ///
    /// `render_set` filters which files Pass-2 dispatches:
    /// - `None` (Mode A) → every file.
    /// - `Some(set)` (Mode B) → only files whose absolute input
    ///   path appears in the set.
    ///
    /// **Phase 5:** each per-doc render drains its
    /// Project-scoped artifacts into the orchestrator's
    /// `project_artifacts` accumulator. The merge is sequential
    /// (no shared mutable state during render) so the function
    /// composes with future rayon-per-worker parallelism — see
    /// `claude-notes/plans/2026-04-24-websites-phase-5.md`
    /// Decision 2.
    async fn pass_two(
        &mut self,
        index: Arc<ProjectIndex>,
        skip: &std::collections::HashSet<std::path::PathBuf>,
        render_set: Option<&std::collections::HashSet<std::path::PathBuf>>,
    ) -> (Vec<R::Output>, Vec<FileFailure>) {
        let mut outputs = Vec::with_capacity(self.project.files.len());
        let mut failures = Vec::new();
        // Snapshot the file list to avoid borrowing `self.project`
        // while we also mutate `self.project_artifacts`.
        let files: Vec<crate::project::DocumentInfo> = self.project.files.clone();
        for doc_info in &files {
            if skip.contains(&doc_info.input) {
                continue;
            }
            // Mode B: skip pages outside the augmented render set.
            // Their existing on-disk output is left untouched.
            if let Some(set) = render_set {
                if !set.contains(&doc_info.input) {
                    continue;
                }
            }
            // Phase 9 sub-phase 9.0: dispatch through `Pass2Renderer`
            // so the orchestrator no longer hard-codes the
            // disk-writing path. Native callers pass
            // `RenderToFileRenderer` (preserving today's behavior);
            // the WASM hub-client (sub-phase 9.2) supplies its own
            // in-memory implementation.
            match self
                .renderer
                .render(
                    doc_info,
                    &self.format,
                    &self.format_str,
                    self.project,
                    index.clone(),
                    self.runtime.clone(),
                    &mut self.project_artifacts,
                )
                .await
            {
                Ok(result) => outputs.push(result),
                Err(e) => failures.push(file_failure_from_error(doc_info.input.clone(), e)),
            }
        }
        (outputs, failures)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_system_runtime::NativeRuntime;
    use std::path::PathBuf;

    #[test]
    fn default_project_type_reports_kind() {
        let t = DefaultProjectType;
        assert_eq!(t.kind(), ProjectKind::Default);
    }

    #[test]
    fn website_project_type_reports_kind() {
        let t = WebsiteProjectType;
        assert_eq!(t.kind(), ProjectKind::Website);
    }

    #[tokio::test]
    async fn default_project_type_hooks_are_no_ops() {
        // Build a minimal `ProjectContext` so the trait methods type-check.
        let runtime = NativeRuntime::new();
        let mut project = ProjectContext {
            dir: PathBuf::from("/project"),
            config: crate::project::ProjectConfig::default(),
            is_single_file: true,
            files: Vec::new(),
            output_dir: PathBuf::from("/project"),
        };
        let t = DefaultProjectType;
        let index = ProjectIndex::default();
        let project_artifacts = crate::artifact::ArtifactStore::new();
        let resolver = crate::resource_resolver::ResourceResolverContext::project_root(
            PathBuf::from("/p"),
            "",
        );

        assert!(t.pre_render(&mut project, &index).await.is_ok());
        let mut diags: Vec<DiagnosticMessage> = Vec::new();
        assert!(
            t.post_render(
                &project,
                &index,
                &[],
                &project_artifacts,
                &resolver,
                &runtime,
                &mut diags,
            )
            .await
            .is_ok()
        );
        assert!(diags.is_empty());
    }

    #[test]
    fn project_kind_string_roundtrip_still_holds() {
        // Rename-regression guard: `ProjectKind::try_from` must still
        // accept all canonical strings and round-trip through
        // `as_str`.
        for expected in [
            ProjectKind::Default,
            ProjectKind::Website,
            ProjectKind::Book,
            ProjectKind::Manuscript,
        ] {
            let s = expected.as_str();
            let back = ProjectKind::try_from(s).unwrap();
            assert_eq!(back, expected);
        }
    }

    // === Phase 5 Decision 4: ProjectType::lib_dir ===

    /// Plan test 15: WebsiteProjectType reports `"site_libs"`.
    #[test]
    fn website_project_type_lib_dir_is_site_libs() {
        let t = WebsiteProjectType;
        assert_eq!(t.lib_dir(), "site_libs");
    }

    /// Plan test 16: DefaultProjectType reports the empty
    /// string — its [`ArtifactScope::Project`] artifacts fall
    /// back to the per-page resource directory.
    #[test]
    fn default_project_type_lib_dir_is_empty() {
        let t = DefaultProjectType;
        assert_eq!(t.lib_dir(), "");
    }

    #[test]
    fn factory_dispatches_by_kind() {
        let make = |kind: ProjectKind| ProjectContext {
            dir: PathBuf::from("/p"),
            config: crate::project::ProjectConfig {
                project_kind: kind,
                ..Default::default()
            },
            is_single_file: false,
            files: Vec::new(),
            output_dir: PathBuf::from("/p"),
        };
        assert_eq!(
            project_type_for(&make(ProjectKind::Default)).kind(),
            ProjectKind::Default
        );
        assert_eq!(
            project_type_for(&make(ProjectKind::Website)).kind(),
            ProjectKind::Website
        );
        // Book / Manuscript fall back to Default for Phase 1.
        assert_eq!(
            project_type_for(&make(ProjectKind::Book)).kind(),
            ProjectKind::Default
        );
        assert_eq!(
            project_type_for(&make(ProjectKind::Manuscript)).kind(),
            ProjectKind::Default
        );
    }
}
