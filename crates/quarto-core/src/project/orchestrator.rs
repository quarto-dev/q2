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

use quarto_error_reporting::{DiagnosticKind, DiagnosticMessage};

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

/// Process-wide accumulator of the OS thread IDs that have executed
/// at least one Pass-1 profile extraction. Backed by a `Mutex` over
/// `HashSet<ThreadId>` because the cardinality is low (one entry per
/// rayon worker thread; ~`available_parallelism()` total) and 574
/// lock acquisitions per render are invisible against tree-sitter
/// CPU cost.
///
/// Pre-parallelization (bd-m7x9s Phase 0) this is always `{current
/// thread}` ⇒ `len() == 1`. Post-Phase-2 it should grow to the size
/// of the rayon pool. Surfaced through [`pass1_threads_used()`] for
/// the `perf.pass1 threads_used=K` gauge. This is observability only —
/// realized thread count is a non-deterministic scheduler outcome, not
/// a behavioral contract, so it is intentionally not asserted in tests
/// (see bd-3klmk).
static PASS1_THREADS: std::sync::OnceLock<
    std::sync::Mutex<std::collections::HashSet<std::thread::ThreadId>>,
> = std::sync::OnceLock::new();

/// Cumulative count of documents processed by Pass-1 since process
/// start. Pairs with [`pass1_wall_nanos()`] to derive average per-doc
/// cost from `QUARTO_PERF_STATS=1` output.
static PASS1_DOCS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Cumulative wall-clock time spent in [`ProjectPipeline::pass_one`]
/// since process start, in nanoseconds. Only the outer pass_one
/// timer contributes — individual `profile_with_cache` calls don't
/// stop the clock — so under rayon this is wall time, not the sum
/// of per-doc CPU.
static PASS1_WALL_NANOS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn pass1_threads_record() {
    let mu = PASS1_THREADS.get_or_init(|| std::sync::Mutex::new(std::collections::HashSet::new()));
    if let Ok(mut set) = mu.lock() {
        set.insert(std::thread::current().id());
    }
}

/// Number of distinct OS threads that have executed at least one
/// Pass-1 profile extraction since the process started.
///
/// Reports `0` if no Pass-1 has run yet (the underlying set is
/// lazy-initialized on first record).
pub fn pass1_threads_used() -> usize {
    PASS1_THREADS
        .get()
        .map_or(0, |mu| mu.lock().map_or(0, |s| s.len()))
}

/// Number of documents Pass-1 has processed since process start.
pub fn pass1_docs_seen() -> usize {
    PASS1_DOCS.load(std::sync::atomic::Ordering::Relaxed)
}

/// Cumulative wall-clock time (nanoseconds) spent in
/// [`ProjectPipeline::pass_one`] since process start.
pub fn pass1_wall_nanos() -> u64 {
    PASS1_WALL_NANOS.load(std::sync::atomic::Ordering::Relaxed)
}

/// Print `perf.pass1 docs=N threads_used=K wall_ms=W` to stderr when
/// `QUARTO_PERF_STATS=1`. Call once at the end of a top-level command
/// (e.g. `q2 render`) — counterpart of
/// [`crate::engine::print_discovery_stats_if_enabled`].
pub fn print_pass1_stats_if_enabled() {
    if !std::env::var_os("QUARTO_PERF_STATS").is_some_and(|v| v == "1") {
        return;
    }
    let docs = pass1_docs_seen();
    let threads = pass1_threads_used();
    let wall_ms = pass1_wall_nanos() / 1_000_000;
    eprintln!("perf.pass1 docs={docs} threads_used={threads} wall_ms={wall_ms}");
}

// ── Pass-2 perf gauge (bd-3gj56) — mirrors the Pass-1 statics above ──

/// OS thread IDs that have rendered at least one Pass-2 document.
/// One entry per active rayon worker; `len()` is the realized
/// parallelism. Observability only — not a behavioral contract.
static PASS2_THREADS: std::sync::OnceLock<
    std::sync::Mutex<std::collections::HashSet<std::thread::ThreadId>>,
> = std::sync::OnceLock::new();
/// Cumulative count of documents rendered by Pass-2 since process start.
static PASS2_DOCS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
/// Cumulative wall-clock time (ns) spent in Pass-2 batch dispatch.
static PASS2_WALL_NANOS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Record the current thread as having executed Pass-2 work. Called
/// once per document (parallel: from each rayon worker; serial: from
/// the dispatch thread).
pub(crate) fn pass2_threads_record() {
    let mu = PASS2_THREADS.get_or_init(|| std::sync::Mutex::new(std::collections::HashSet::new()));
    if let Ok(mut set) = mu.lock() {
        set.insert(std::thread::current().id());
    }
}

/// Add to the cumulative Pass-2 doc count and wall time. Called once
/// per `render_batch` dispatch.
pub(crate) fn pass2_record(docs: usize, wall_nanos: u64) {
    PASS2_DOCS.fetch_add(docs, std::sync::atomic::Ordering::Relaxed);
    PASS2_WALL_NANOS.fetch_add(wall_nanos, std::sync::atomic::Ordering::Relaxed);
}

/// Number of distinct OS threads that rendered Pass-2 documents.
pub fn pass2_threads_used() -> usize {
    PASS2_THREADS
        .get()
        .map_or(0, |mu| mu.lock().map_or(0, |s| s.len()))
}

/// Print `perf.pass2 docs=N threads_used=K wall_ms=W` to stderr when
/// `QUARTO_PERF_STATS=1`. Counterpart of [`print_pass1_stats_if_enabled`].
pub fn print_pass2_stats_if_enabled() {
    if !std::env::var_os("QUARTO_PERF_STATS").is_some_and(|v| v == "1") {
        return;
    }
    let docs = PASS2_DOCS.load(std::sync::atomic::Ordering::Relaxed);
    let threads = pass2_threads_used();
    let wall_ms = PASS2_WALL_NANOS.load(std::sync::atomic::Ordering::Relaxed) / 1_000_000;
    eprintln!("perf.pass2 docs={docs} threads_used={threads} wall_ms={wall_ms}");
}

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
    /// hooks like the project-artifact flush no longer reconstruct
    /// the path math themselves.
    async fn post_render(
        &self,
        _project: &ProjectContext,
        index: &ProjectIndex,
        _output_paths: &[std::path::PathBuf],
        _project_artifacts: &crate::artifact::ArtifactStore,
        _resolver: &crate::resource_resolver::ResourceResolverContext,
        _runtime: &dyn quarto_system_runtime::SystemRuntime,
        diagnostics: &mut Vec<DiagnosticMessage>,
    ) -> Result<()> {
        // Only website projects write redirect stubs. Every other
        // project type says so rather than dropping the key in
        // silence — the silence *was* the original bug report
        // (bd-aliases-redirects-missing-sch7cd1g): a porting project
        // got no signal that its redirects had disappeared.
        super::aliases::warn_aliases_ignored(index, diagnostics);
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
    /// 1. **[`flush_project_artifacts`]** (Phase 5; resolver-driven) —
    ///    write the Project-scoped artifacts accumulated across every
    ///    Pass-2 render to whatever destination the resolver decides:
    ///    `<output_dir>/site_libs/...` natively, or
    ///    `/.quarto/project-artifacts/...` in the hub-client VFS.
    ///
    /// [`flush_project_artifacts`]: crate::artifact_flush::flush_project_artifacts
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
    /// these features just runs the project-artifact flush.
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
        crate::artifact_flush::flush_project_artifacts(project_artifacts, resolver, runtime)?;
        // The remaining hooks write to the on-disk output dir,
        // which only exists natively. WASM hub-client renders skip
        // them — see Phase 9 plan §Decision 4.
        #[cfg(not(target_arch = "wasm32"))]
        {
            use super::website_post_render::{
                copy_favicon, copy_footer_images, copy_navbar_logo, write_alias_redirects,
                write_robots_txt, write_sitemap,
            };
            copy_favicon(project, runtime, diagnostics)?;
            copy_navbar_logo(project, runtime, diagnostics)?;
            copy_footer_images(project, runtime, diagnostics)?;
            write_sitemap(project, index, output_paths, runtime)?;
            write_robots_txt(project, runtime)?;
            // `aliases:` redirect stubs
            // (bd-aliases-redirects-missing-sch7cd1g). Unlike its
            // neighbours this hook can *fail* the render: an alias
            // collision is an error, not a warning, because the
            // alternative is a redirect silently pointing at the
            // wrong page. See the `Q-5-23`..`Q-5-26` docs pages.
            write_alias_redirects(project, index, runtime)?;
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
pub(crate) fn file_failure_from_error(input: std::path::PathBuf, e: QuartoError) -> FileFailure {
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
    /// `true` when the render was cut short because `--fail-fast` was
    /// set and at least one per-document error was seen. The reported
    /// failures are then **not** exhaustive — remaining files were not
    /// checked. Always `false` without `--fail-fast`.
    pub stopped_early: bool,
}

impl<O> ProjectRenderSummary<O> {
    /// True if any file (Pass 1 or Pass 2) failed.
    pub fn has_failures(&self) -> bool {
        !self.pass1_failures.is_empty() || !self.pass2_failures.is_empty()
    }
}

/// Total error / warning counts aggregated across an entire render
/// (bd-ooleh).
///
/// These are **diagnostic-level** counts — an estimate of how many
/// distinct problems a user must resolve to reach a clean build, not a
/// count of failed files. Only [`DiagnosticKind::Error`] and
/// [`DiagnosticKind::Warning`] are tallied; `Info` / `Note` are
/// ignored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DiagnosticCounts {
    pub errors: usize,
    pub warnings: usize,
}

impl DiagnosticCounts {
    /// True when there is nothing to report (a clean run).
    pub fn is_empty(&self) -> bool {
        self.errors == 0 && self.warnings == 0
    }

    /// Tally a slice of diagnostics by severity, ignoring `Info` /
    /// `Note`.
    fn add_diagnostics(&mut self, diagnostics: &[DiagnosticMessage]) {
        for diagnostic in diagnostics {
            match diagnostic.kind {
                DiagnosticKind::Error => self.errors += 1,
                DiagnosticKind::Warning => self.warnings += 1,
                DiagnosticKind::Info | DiagnosticKind::Note => {}
            }
        }
    }
}

/// Expose the per-page diagnostics carried by a Pass-2 output so
/// [`ProjectRenderSummary::diagnostic_counts`] can tally warnings from
/// *successful* renders without knowing the concrete output type
/// (native [`RenderToFileResult`] vs the WASM `WasmPassTwoOutput`).
pub trait OutputDiagnostics {
    fn diagnostics(&self) -> &[DiagnosticMessage];

    /// Mutable access to the same diagnostics, so severity-promotion
    /// policies (`--strict`, bd-yjs54ptg) can rewrite `kind` in place.
    fn diagnostics_mut(&mut self) -> &mut [DiagnosticMessage];
}

#[cfg(not(target_arch = "wasm32"))]
impl OutputDiagnostics for RenderToFileResult {
    fn diagnostics(&self) -> &[DiagnosticMessage] {
        &self.render_output.diagnostics
    }

    fn diagnostics_mut(&mut self) -> &mut [DiagnosticMessage] {
        &mut self.render_output.diagnostics
    }
}

#[cfg(target_arch = "wasm32")]
impl OutputDiagnostics for RenderToFileResult {
    fn diagnostics(&self) -> &[DiagnosticMessage] {
        // The WASM placeholder never carries diagnostics (see the
        // `cfg(target_arch = "wasm32")` definition above).
        &[]
    }

    fn diagnostics_mut(&mut self) -> &mut [DiagnosticMessage] {
        &mut []
    }
}

impl<O: OutputDiagnostics> ProjectRenderSummary<O> {
    /// Aggregate every reported error / warning across all four
    /// diagnostic sources into a single [`DiagnosticCounts`].
    ///
    /// Counting rules (bd-ooleh):
    /// - A Pass-1 or Pass-2 [`FileFailure`] with **no** structured
    ///   diagnostics contributes **one error** — its top-level `error`
    ///   string is the single problem. (This is also why pass-1
    ///   failures read as errors rather than warnings: they almost
    ///   always land here, keeping the summary in agreement with the
    ///   non-zero exit code.)
    /// - A failure **with** structured diagnostics contributes those
    ///   diagnostics counted by [`DiagnosticKind`].
    /// - [`Self::project_diagnostics`] and the per-page diagnostics of
    ///   every successful render in [`Self::outputs`] are counted by
    ///   kind.
    /// - `Info` / `Note` severities are ignored.
    pub fn diagnostic_counts(&self) -> DiagnosticCounts {
        let mut counts = DiagnosticCounts::default();

        for failure in self.pass1_failures.iter().chain(&self.pass2_failures) {
            if failure.diagnostics.is_empty() {
                counts.errors += 1;
            } else {
                counts.add_diagnostics(&failure.diagnostics);
            }
        }

        counts.add_diagnostics(&self.project_diagnostics);

        for output in &self.outputs {
            counts.add_diagnostics(output.diagnostics());
        }

        counts
    }

    /// Promote every `Warning` diagnostic to `Error`, across all four
    /// diagnostic sources (strict mode, bd-yjs54ptg / GH #220).
    ///
    /// This is the single seam through which every structured
    /// diagnostic flows before anything is printed or counted, so a
    /// warning emitted anywhere in the pipeline — stages, transforms,
    /// Lua filters, project post-render — participates in strict mode
    /// with no per-call-site opt-in. `Info` / `Note` are left alone.
    ///
    /// The orchestrator itself stays policy-free: this method is only
    /// invoked by consumers that opt into strictness (the `--strict`
    /// CLI flag); it does not run during the render and cannot change
    /// what gets rendered.
    pub fn promote_warnings_to_errors(&mut self) {
        fn promote(diagnostics: &mut [DiagnosticMessage]) {
            for diagnostic in diagnostics {
                if diagnostic.kind == DiagnosticKind::Warning {
                    diagnostic.kind = DiagnosticKind::Error;
                }
            }
        }

        for failure in self
            .pass1_failures
            .iter_mut()
            .chain(&mut self.pass2_failures)
        {
            promote(&mut failure.diagnostics);
        }
        promote(&mut self.project_diagnostics);
        for output in &mut self.outputs {
            promote(output.diagnostics_mut());
        }
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
#[derive(Debug, Clone, Default)]
pub enum RenderMode {
    #[default]
    Full,
    Subset(std::collections::HashSet<std::path::PathBuf>),
    ActivePage(std::path::PathBuf),
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
    /// Stop the render as soon as the first per-document error is
    /// seen (Pass-1 or Pass-2). Default `false` (best-effort:
    /// render everything, report all failures at the end).
    fail_fast: bool,
    /// Explicit Pass-2 worker-count override (`with_jobs`). `None`
    /// ⇒ use the `QUARTO_JOBS`/auto default via [`worker_count`].
    jobs: Option<usize>,
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

    /// Set the CLI `--to` override (bd-l6itt34u minimal slice). When `Some`,
    /// it is merged as a top-priority `format: !prefer <to>` layer in every
    /// document's format resolution (an explicit `--to` wins for all docs).
    /// When `None`, each document's own front-matter `format:` — or a
    /// project-level `format:` — is honored, so a `format: revealjs` deck in
    /// an otherwise-HTML `type: default` project renders as a real deck.
    /// Defined on the concrete `RenderToFileRenderer` pipeline because only
    /// the disk renderer resolves per-document format; the WASM/preview
    /// renderers are unaffected.
    pub fn with_format_override(mut self, to: Option<String>) -> Self {
        self.renderer.format_override = to;
        self
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
            fail_fast: false,
            jobs: None,
        }
    }

    /// Set the render mode. Default is [`RenderMode::Full`]; CLI
    /// subset args set this to [`RenderMode::Subset`] before
    /// calling [`run`](Self::run).
    pub fn with_mode(mut self, mode: RenderMode) -> Self {
        self.mode = mode;
        self
    }

    /// Stop the render as soon as the first per-document error is seen
    /// (Pass-1 or Pass-2), instead of best-effort rendering everything.
    pub fn with_fail_fast(mut self, fail_fast: bool) -> Self {
        self.fail_fast = fail_fast;
        self
    }

    /// Pin the Pass-2 worker count, overriding the `QUARTO_JOBS`/auto
    /// default. `1` forces serial. Primarily for tests that need
    /// deterministic parallelism without mutating process-global env
    /// (which is `unsafe` in edition 2024 and racy across tests).
    pub fn with_jobs(mut self, jobs: usize) -> Self {
        self.jobs = Some(jobs);
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
        let mut initial_diagnostics = self.empty_render_set_diagnostic();
        // `project.render` patterns that contributed nothing
        // (bd-mt7a6uc4 D7). Computed here rather than inside
        // discovery so `ProjectContext::discover` keeps its
        // signature; the check is pure and reads the post-exclusion
        // file list, so it reports what actually happened.
        initial_diagnostics.extend(crate::project::discovery::render_pattern_diagnostics(
            &self.project.dir,
            &self.project.config.render_patterns,
            &self.project.files,
        ));

        let (profiles, pass1_failures) = self.pass_one().await;
        let index = Arc::new(ProjectIndex::new(profiles));

        // Fail-fast barrier: if Pass-1 surfaced any error, stop before
        // running the `pre_render` hook, Pass-2, `post_render`, resource
        // copy, and the manifest write. We report the failures we have
        // and flag the summary as truncated.
        if self.fail_fast && !pass1_failures.is_empty() {
            return Ok(ProjectRenderSummary {
                outputs: Vec::new(),
                pass1_failures,
                pass2_failures: Vec::new(),
                project_diagnostics: initial_diagnostics,
                stopped_early: true,
            });
        }

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
            // A hook that already produced structured diagnostics
            // passes through intact. Wrapping it in `other` would
            // flatten Ariadne spans into a string the caller can no
            // longer inspect, re-render, or serialize — the CLI would
            // print pre-rendered ANSI inside a generic message, and
            // `--to json` would lose the diagnostics entirely.
            // Opaque errors still get the context prefix.
            .map_err(|e| match e {
                QuartoError::Parse(_) => e,
                other => QuartoError::other(format!("post_render failed: {other}")),
            })?;

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
            let mut resource_diagnostics = Vec::new();
            let mut resolved = crate::project_resources::collect_static_resources_with_diagnostics(
                self.project,
                &index,
                self.runtime.as_ref(),
                &mut resource_diagnostics,
            )
            .map_err(QuartoError::Parse)?;
            project_diagnostics.extend(resource_diagnostics);
            for output in &outputs {
                if let Some(report) = R::extract_resource_report(output) {
                    if report.is_empty() {
                        continue;
                    }
                    let resolved_report = crate::project_resources::resolve_reported_resources(
                        &self.project.dir,
                        report,
                        self.runtime.as_ref(),
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

        let stopped_early = self.fail_fast && !pass2_failures.is_empty();
        Ok(ProjectRenderSummary {
            outputs,
            pass1_failures,
            pass2_failures,
            project_diagnostics,
            stopped_early,
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
            "Check `project.render` in `_quarto.yml` — its globs matched no renderable files."
        } else {
            "Add a `.qmd` file to the project, or remove `_quarto.yml` to render a single \
             standalone document."
        };
        diag.hints
            .push(quarto_error_reporting::MessageContent::from(hint));
        // `.md` files are render-list opt-in (bd-6d2wj4zp). When the
        // project has renderable `.md` files that no pattern matched,
        // the empty set is likely that policy at work — say so, or
        // the default reads as Quarto silently ignoring the files.
        let discovery_cfg = crate::project::discovery::DiscoveryConfig {
            project_dir: &self.project.dir,
            output_dir: &self.project.output_dir,
            render_patterns: &self.project.config.render_patterns,
        };
        if let Ok(md) =
            crate::project::discovery::unmatched_md_files(&discovery_cfg, self.runtime.as_ref())
            && !md.is_empty()
        {
            let n = md.len();
            let plural = if n == 1 { "" } else { "s" };
            diag.hints
                .push(quarto_error_reporting::MessageContent::from(format!(
                    "The project contains {n} `.md` file{plural}; `.md` files render \
                     only when matched by a `project.render` pattern such as \
                     `\"**/*.md\"`."
                )));
        }
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
    /// Test-only accessor that drives [`Self::pass_one`] and returns
    /// its raw output (profiles + per-file failures). Exists so
    /// integration tests in `crates/quarto-core/tests/` can assert
    /// invariants on Pass-1's collected ordering — checks that the
    /// public `run()` summary alone cannot make (Pass-2 re-iterates
    /// `project.files` so `summary.outputs` ordering is a Pass-2
    /// invariant, not a Pass-1 one).
    ///
    /// The name is deliberately unwieldy. Do not call this outside
    /// of tests.
    #[doc(hidden)]
    pub async fn __pass_one_for_test_only(
        &self,
    ) -> (
        Vec<crate::document_profile::DocumentProfile>,
        Vec<FileFailure>,
    ) {
        self.pass_one().await
    }

    async fn pass_one(
        &self,
    ) -> (
        Vec<crate::document_profile::DocumentProfile>,
        Vec<FileFailure>,
    ) {
        // bd-3y7ul: route the monotonic clock through SystemRuntime so the
        // WASM build doesn't hit `std::time::Instant::now()` (which panics
        // with "time not implemented on this platform" on
        // wasm32-unknown-unknown). Native uses `Instant`; WASM uses
        // `performance.now()`.
        let start_nanos = self.runtime.monotonic_now_nanos();
        // bd-3y7ul: WASM can't use `pollster::block_on` (no condvar), so it
        // routes through `pass_one_dispatch_async` and `.await`s each
        // per-doc future. Native keeps the sync `pass_one_dispatch` for
        // rayon compatibility.
        #[cfg(not(target_arch = "wasm32"))]
        let (profiles, failures) =
            pass_one_dispatch(&self.runtime, self.project, &self.format, self.fail_fast);
        #[cfg(target_arch = "wasm32")]
        let (profiles, failures) =
            pass_one_dispatch_async(&self.runtime, self.project, &self.format, self.fail_fast)
                .await;
        // bd-m7x9s Phase 0: feed the `perf.pass1` gauge with cumulative
        // doc count and wall time. `pass1_profile_with_cache`
        // increments the thread-set; here we add the per-`pass_one`
        // doc count and elapsed wall time. Outer-timer placement is
        // intentional — under rayon the dispatch's wall time is what
        // we want, not the sum of per-doc CPU.
        PASS1_DOCS.fetch_add(
            profiles.len() + failures.len(),
            std::sync::atomic::Ordering::Relaxed,
        );
        PASS1_WALL_NANOS.fetch_add(
            self.runtime
                .monotonic_now_nanos()
                .saturating_sub(start_nanos),
            std::sync::atomic::Ordering::Relaxed,
        );
        (profiles, failures)
    }

    /// Helper: project-relative source path for cache-key
    /// construction. Mirrors `DocumentProfileStage`'s computation
    /// (canonicalize when possible, fall back to file name).
    ///
    /// Thin `&self` wrapper around the free
    /// [`pass1_project_relative_source_path`] helper — kept because
    /// `compute_augmented_render_set` (Pass-2 setup) uses it from an
    /// `&self` context that already has the project context handy.
    fn project_relative_source_path(&self, input: &std::path::Path) -> String {
        pass1_project_relative_source_path(&self.project.dir, input)
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
        // Snapshot the file list to avoid borrowing `self.project`
        // while we also mutate `self.project_artifacts`.
        let files: Vec<crate::project::DocumentInfo> = self.project.files.clone();
        // Apply `skip` (Pass-1 failures) and Mode-B `render_set`
        // filtering up front, so the renderer batch only sees the
        // documents we actually mean to render.
        let docs: Vec<&crate::project::DocumentInfo> = files
            .iter()
            .filter(|d| !skip.contains(&d.input))
            .filter(|d| render_set.is_none_or(|set| set.contains(&d.input)))
            .collect();

        // Worker count: an explicit `with_jobs` override wins; else the
        // shared `QUARTO_JOBS`/auto default (capped at 16), same as Pass 1.
        let workers = self.jobs.unwrap_or_else(worker_count);

        // Phase 9 sub-phase 9.0: dispatch through `Pass2Renderer`.
        // The native `RenderToFileRenderer` fans `render_batch` out
        // across rayon workers (bd-3gj56); the WASM hub-client and the
        // default trait impl run it serially. Either way, outputs come
        // back in `docs` (= `project.files`) order.
        self.renderer
            .render_batch(
                &docs,
                &self.format,
                &self.format_str,
                self.project,
                index,
                self.runtime.clone(),
                &mut self.project_artifacts,
                workers,
                self.fail_fast,
            )
            .await
    }
}

// === Pass-1 free functions (bd-m7x9s Phase 1) =============================
//
// These are extracted from `ProjectPipeline` so the Phase-2 rayon
// fan-out can dispatch them across worker threads without sharing
// `&self`. Each call captures only `Send + Sync` data:
//
// - `Arc<dyn SystemRuntime>` — the trait is `Send + Sync` on native
//   (see `crates/quarto-system-runtime/src/traits.rs:243`).
// - `&ProjectContext` — pure data, no `Rc`/`RefCell`.
// - `&Format` — pure data, no interior mutability.
// - `&DocumentInfo` — pure data.
//
// The bodies are byte-for-byte the previous `&self` method bodies
// with `self.runtime`/`self.project`/`self.format` substituted for
// the explicit parameters; behavior is unchanged in Phase 1.

/// Resolve the worker-count knob shared by Pass-1
/// ([`pass_one_dispatch_parallel`]) and Pass-2
/// ([`RenderToFileRenderer::render_batch`]).
///
/// - `QUARTO_JOBS=<n>` if set and parses as a positive integer.
/// - Otherwise `std::thread::available_parallelism()`, capped at 16
///   to avoid pathological over-subscription on large servers.
/// - Falls back to 4 if `available_parallelism()` returns `Err`.
///
/// Returns `1` to mean "force sequential" — callers short-circuit
/// the rayon dispatch when `worker_count() == 1` or
/// `files.len() <= 1`.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn worker_count() -> usize {
    if let Some(raw) = std::env::var_os("QUARTO_JOBS")
        && let Some(s) = raw.to_str()
        && let Ok(n) = s.parse::<usize>()
        && n >= 1
    {
        return n;
    }
    std::thread::available_parallelism().map_or(4, |n| n.get().min(16))
}

/// WASM has no OS threads; Pass-2 always runs serially there (the
/// default [`Pass2Renderer::render_batch`] ignores this value anyway).
#[cfg(target_arch = "wasm32")]
pub(crate) fn worker_count() -> usize {
    1
}

/// Pass-1 fan-out: on native targets, dispatch each document through
/// the rayon work-stealing pool; on WASM, run sequentially.
///
/// The native parallel path uses `IndexedParallelIterator::collect()`
/// to preserve input order — see the `pass_one_preserves_input_order`
/// regression test. Each rayon worker runs `pollster::block_on` over
/// the (still `?Send`) Pass-1 future; this is safe because the
/// Pass-1 stage graph has no tokio runtime requirements (see the
/// Phase-0 pollster-compat audit in
/// `claude-notes/plans/2026-05-22-parallelize-pass-one.md`).
#[cfg(not(target_arch = "wasm32"))]
fn pass_one_dispatch(
    runtime: &Arc<dyn SystemRuntime>,
    project: &ProjectContext,
    format: &Format,
    fail_fast: bool,
) -> (
    Vec<crate::document_profile::DocumentProfile>,
    Vec<FileFailure>,
) {
    let workers = worker_count();
    if workers == 1 || project.files.len() <= 1 {
        return pass_one_dispatch_sequential(runtime, project, format, fail_fast);
    }
    pass_one_dispatch_parallel(runtime, project, format, workers, fail_fast)
}

/// Sequential fan-out — the pre-Phase-2 behavior. Used:
/// - on WASM (no OS threads, rayon doesn't compile);
/// - when `QUARTO_JOBS=1` is set;
/// - when the project has 0 or 1 files (parallelism has no payoff).
#[cfg(not(target_arch = "wasm32"))]
fn pass_one_dispatch_sequential(
    runtime: &Arc<dyn SystemRuntime>,
    project: &ProjectContext,
    format: &Format,
    fail_fast: bool,
) -> (
    Vec<crate::document_profile::DocumentProfile>,
    Vec<FileFailure>,
) {
    let mut profiles = Vec::with_capacity(project.files.len());
    let mut failures = Vec::new();
    for doc_info in &project.files {
        match pollster::block_on(pass1_profile_with_cache(runtime, project, format, doc_info)) {
            Ok(profile) => profiles.push(profile),
            Err(e) => {
                failures.push(file_failure_from_error(doc_info.input.clone(), e));
                // Fail-fast: stop at the first error in document order.
                // This path is deterministic (single-threaded).
                if fail_fast {
                    break;
                }
            }
        }
    }
    (profiles, failures)
}

/// WASM dispatch — async sequential fan-out, driven by `.await`.
///
/// `pollster::block_on` (used on native) calls `Condvar::wait()`
/// internally, which panics with "condvar wait not supported" on
/// `wasm32-unknown-unknown` (no OS threads). WASM is already
/// single-threaded, so we just await each per-doc future directly —
/// the caller (`ProjectPipeline::pass_one`) is already an `async fn`
/// driven by the host's `wasm_bindgen_futures` executor.
#[cfg(target_arch = "wasm32")]
async fn pass_one_dispatch_async(
    runtime: &Arc<dyn SystemRuntime>,
    project: &ProjectContext,
    format: &Format,
    fail_fast: bool,
) -> (
    Vec<crate::document_profile::DocumentProfile>,
    Vec<FileFailure>,
) {
    let mut profiles = Vec::with_capacity(project.files.len());
    let mut failures = Vec::new();
    for doc_info in &project.files {
        match pass1_profile_with_cache(runtime, project, format, doc_info).await {
            Ok(profile) => profiles.push(profile),
            Err(e) => {
                failures.push(file_failure_from_error(doc_info.input.clone(), e));
                if fail_fast {
                    break;
                }
            }
        }
    }
    (profiles, failures)
}

/// Native parallel fan-out via rayon.
///
/// Each per-doc closure captures `Send + Sync` data only
/// (`&Arc<dyn SystemRuntime>` clones the inner `Arc`,
/// `&ProjectContext` / `&Format` are immutable borrows of `Send +
/// Sync` data). Inside the closure we run `pollster::block_on` to
/// drive the `?Send` Pass-1 future on the rayon worker thread —
/// the future never crosses thread boundaries.
///
/// `std::panic::catch_unwind` per worker converts any panic to a
/// `FileFailure`, matching the per-file isolation we already have
/// for `Result::Err`. Without it a single misbehaving stage would
/// abort the entire render.
///
/// `IndexedParallelIterator::collect_into_vec` preserves input
/// order; downstream consumers (dependency graph builder,
/// diagnostic emission) see the same profile sequence pre and post
/// Phase 2.
#[cfg(not(target_arch = "wasm32"))]
fn pass_one_dispatch_parallel(
    runtime: &Arc<dyn SystemRuntime>,
    project: &ProjectContext,
    format: &Format,
    workers: usize,
    fail_fast: bool,
) -> (
    Vec<crate::document_profile::DocumentProfile>,
    Vec<FileFailure>,
) {
    use rayon::iter::{IndexedParallelIterator, IntoParallelRefIterator, ParallelIterator};

    enum DocOutcome {
        Profile(crate::document_profile::DocumentProfile),
        Failure(FileFailure),
        /// Fail-fast short-circuit: another worker already errored, so
        /// this document was never parsed. Folded away (no profile, no
        /// failure) when reducing outcomes.
        Skipped,
    }

    // Build a local rayon pool sized to `workers` so `QUARTO_JOBS`
    // actually constrains parallelism. Using the global pool with
    // `par_iter()` always spawns `available_parallelism()` workers
    // regardless of how the work is partitioned — fine for the
    // default case, but the env-var override would silently no-op.
    // Pool construction is a few-ms thread spawn; amortized over the
    // hundreds of docs Pass-1 typically processes, the cost is
    // invisible (and tests covering the small-N case run via the
    // sequential short-circuit, see `pass_one_dispatch`).
    let pool = match rayon::ThreadPoolBuilder::new()
        .num_threads(workers)
        .thread_name(|i| format!("quarto-pass1-{i}"))
        .build()
    {
        Ok(p) => p,
        // Pool construction failed (extremely rare — typically an
        // OOM or thread-limit signal from the OS). Degrade to the
        // sequential path rather than aborting the render.
        Err(_) => return pass_one_dispatch_sequential(runtime, project, format, fail_fast),
    };

    // Fail-fast abort flag. A plain local `AtomicBool` is `Sync`, so
    // `&aborted` can be captured by reference in the rayon closure
    // without an `Arc`. rayon does not cancel running tasks, so a few
    // in-flight workers may still error after this is set — accepted
    // per the fail-fast contract.
    let aborted = std::sync::atomic::AtomicBool::new(false);

    let mut outcomes: Vec<DocOutcome> = Vec::with_capacity(project.files.len());
    pool.install(|| {
        // `collect_into_vec` is the order-preserving sink for
        // `IndexedParallelIterator`. Calling it explicitly (rather
        // than `.collect::<Vec<_>>()`) documents the dependency on
        // indexed ordering and keeps the `IndexedParallelIterator`
        // import load-bearing — see the
        // `pass_one_preserves_input_order` regression test.
        project
            .files
            .par_iter()
            .map(|doc_info| {
                // Fail-fast: if another worker has already errored,
                // skip parsing this document entirely.
                if fail_fast && aborted.load(std::sync::atomic::Ordering::Relaxed) {
                    return DocOutcome::Skipped;
                }
                let input = doc_info.input.clone();
                // catch_unwind so a panic in
                // `pass1_profile_with_cache` (e.g. tree-sitter
                // assertion on a malformed input) maps to a
                // per-file failure rather than aborting the whole
                // render. The closure only borrows `Send` data so
                // it satisfies `UnwindSafe`'s practical
                // requirements; `AssertUnwindSafe` papers over the
                // strict checker.
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    pollster::block_on(pass1_profile_with_cache(runtime, project, format, doc_info))
                }));
                match result {
                    Ok(Ok(profile)) => DocOutcome::Profile(profile),
                    Ok(Err(e)) => {
                        if fail_fast {
                            aborted.store(true, std::sync::atomic::Ordering::Relaxed);
                        }
                        DocOutcome::Failure(file_failure_from_error(input, e))
                    }
                    Err(panic_payload) => {
                        if fail_fast {
                            aborted.store(true, std::sync::atomic::Ordering::Relaxed);
                        }
                        let msg = panic_message(&panic_payload);
                        DocOutcome::Failure(file_failure_from_error(
                            input,
                            QuartoError::other(format!(
                                "internal error during Pass-1 (panic): {msg}",
                            )),
                        ))
                    }
                }
            })
            .collect_into_vec(&mut outcomes);
    });

    let mut profiles = Vec::with_capacity(project.files.len());
    let mut failures = Vec::new();
    for outcome in outcomes {
        match outcome {
            DocOutcome::Profile(p) => profiles.push(p),
            DocOutcome::Failure(f) => failures.push(f),
            // Fail-fast skip: contributed neither a profile nor a
            // failure. Dropped here.
            DocOutcome::Skipped => {}
        }
    }
    (profiles, failures)
}

/// Extract a printable message from a `catch_unwind` payload —
/// `Box<dyn Any + Send + 'static>` — by trying the two canonical
/// concrete types panics carry (`&'static str` and `String`).
#[cfg(not(target_arch = "wasm32"))]
fn panic_message(payload: &Box<dyn std::any::Any + Send + 'static>) -> String {
    if let Some(s) = payload.downcast_ref::<&'static str>() {
        return (*s).to_string();
    }
    if let Some(s) = payload.downcast_ref::<String>() {
        return s.clone();
    }
    "<non-string panic payload>".to_string()
}

/// Compute the project-relative source path used as input to
/// [`cache_key::pass1_key`]. Mirrors `DocumentProfileStage`'s
/// computation: prefer canonical relative-to-project, fall back to
/// the file name, fall back to the raw input.
fn pass1_project_relative_source_path(
    project_dir: &std::path::Path,
    input: &std::path::Path,
) -> String {
    if let Ok(rel) = input.strip_prefix(project_dir) {
        rel.to_string_lossy().into_owned()
    } else if let Some(name) = input.file_name() {
        name.to_string_lossy().into_owned()
    } else {
        input.to_string_lossy().into_owned()
    }
}

/// Walk the layered `_metadata.yml` chain for a document (from the
/// project root down to the document's directory) and return each
/// file's raw bytes. Skips files that can't be read; the cache key
/// for that doc will reflect only the available subset, which is
/// also what the head pipeline sees. Returns an empty vec for
/// single-file pseudo-projects.
fn pass1_layered_metadata_raw_bytes(
    runtime: &dyn SystemRuntime,
    project: &ProjectContext,
    document_path: &std::path::Path,
) -> Vec<(std::path::PathBuf, Vec<u8>)> {
    if project.is_single_file {
        return Vec::new();
    }
    let document_path = runtime
        .canonicalize(document_path)
        .unwrap_or_else(|_| document_path.to_path_buf());
    let document_dir = match document_path.parent() {
        Some(p) => p,
        None => return Vec::new(),
    };
    let project_dir = &project.dir;
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
            if let Ok(bytes) = runtime.file_read(&path) {
                out.push((path.clone(), bytes));
                break; // prefer .yml; if both exist this picks .yml
            }
        }
    }
    out
}

/// Read the project's `_quarto.yml` (or `_quarto.yaml`) raw bytes
/// for cache-key hashing. Returns an empty vec when the project
/// has no config file (single-file pseudo-project) or it can't be
/// read.
fn pass1_read_quarto_yml_bytes(runtime: &dyn SystemRuntime, project: &ProjectContext) -> Vec<u8> {
    if project.is_single_file {
        return Vec::new();
    }
    for candidate in ["_quarto.yml", "_quarto.yaml"] {
        let path = project.dir.join(candidate);
        if let Ok(bytes) = runtime.file_read(&path) {
            return bytes;
        }
    }
    Vec::new()
}

/// Try the on-disk profile cache for `doc_info`, falling back to a
/// live head-pipeline run on miss. See `profile_cache::load` for the
/// include-verification semantics.
///
/// Pass-1 entry point — every call records the current OS thread in
/// the `PASS1_THREADS` set via [`pass1_threads_record`] so the
/// `perf.pass1 threads_used=K` gauge stays accurate when this
/// function is dispatched across rayon workers.
async fn pass1_profile_with_cache(
    runtime: &Arc<dyn SystemRuntime>,
    project: &ProjectContext,
    format: &Format,
    doc_info: &DocumentInfo,
) -> Result<crate::document_profile::DocumentProfile> {
    pass1_threads_record();

    // Read source bytes — used for both the cache key
    // computation and the live pipeline below.
    let source_bytes = runtime.file_read(&doc_info.input).map_err(|e| {
        QuartoError::other(format!(
            "Failed to read {} during pass 1: {}",
            doc_info.input.display(),
            e
        ))
    })?;

    // Compute the project-relative source path the same way
    // DocumentProfileStage does (canonicalize when possible, else
    // fall back to file_name).
    let source_path = pass1_project_relative_source_path(&project.dir, &doc_info.input);
    let format_id = format.target_format.clone();

    // Layered _metadata.yml raw bytes for the cache-key domain.
    // We re-read the raw bytes (not the parsed ConfigValue) because
    // byte-for-byte changes invalidate the key — a comment-only
    // edit to _metadata.yml correctly invalidates the cache.
    let metadata_files =
        pass1_layered_metadata_raw_bytes(runtime.as_ref(), project, &doc_info.input);

    // _quarto.yml raw bytes (project root). Empty when the project
    // has no config file (single-file render).
    let quarto_yml_bytes = pass1_read_quarto_yml_bytes(runtime.as_ref(), project);

    // Format extensions: TODO follow-up. For v1 we pass empty
    // contributions; extension changes require `--clean`. See
    // `claude-notes/plans/2026-04-27-websites-phase-8.md` §
    // "Sub-phase 8.4" for the user-facing escape hatch and
    // §Decision 2 footnote for the rationale.
    let extension_contributions: Vec<(String, Vec<u8>)> = Vec::new();

    // Project-profile inputs (bd-fu16z22k): active names plus raw
    // bytes of every merged overlay / `_quarto.yml.local`, so a
    // profile switch or overlay edit invalidates cached pass-1
    // DocumentProfiles. Paths are hashed project-relative for
    // machine-independence, same policy as `metadata_files`.
    let active_profile_names: Vec<String> = project
        .config
        .active_config_profiles
        .iter()
        .map(|p| p.name.clone())
        .collect();
    let profile_config_files: Vec<(std::path::PathBuf, Vec<u8>)> = project
        .config
        .profile_config_paths
        .iter()
        .map(|path| {
            let rel = path
                .strip_prefix(&project.dir)
                .map_or_else(|_| path.clone(), std::path::Path::to_path_buf);
            let bytes = runtime.file_read(path).unwrap_or_default();
            (rel, bytes)
        })
        .collect();

    let key_inputs = crate::project::cache_key::Pass1KeyInputs {
        format_id: &format_id,
        source_path: &source_path,
        source_bytes: &source_bytes,
        metadata_files: &metadata_files,
        quarto_yml_bytes: &quarto_yml_bytes,
        extension_contributions: &extension_contributions,
        active_config_profiles: &active_profile_names,
        profile_config_files: &profile_config_files,
    };
    let key_bytes = crate::project::cache_key::pass1_key(&key_inputs);
    let key_hex = crate::project::cache_key::hex_encode(&key_bytes);

    // Cache lookup. The include resolver reads each include's
    // current bytes and computes their SHA-256 to compare against
    // the cached profile's recorded content_hashes. A miss here
    // (load returns Ok(None)) just means we do a live extraction
    // and overwrite.
    let resolver_runtime = runtime.clone();
    let include_resolver = move |path: &std::path::Path| {
        let bytes = resolver_runtime.file_read(path)?;
        Ok(crate::document_profile::IncludeEntry::hash_bytes(&bytes))
    };

    // A cache miss / verification failure / runtime error (Ok(None) | Err(_))
    // degrades to a live extraction. We don't surface the error because the
    // orchestrator never wants a cache hiccup to abort an otherwise-fine render.
    if let Ok(Some(profile)) =
        crate::project::profile_cache::load(runtime.as_ref(), &key_hex, include_resolver).await
    {
        return Ok(profile);
    }

    let profile =
        pass1_profile_single_file_live(runtime.clone(), project, format, doc_info, &source_bytes)
            .await?;

    // Best-effort save. A save failure is also non-fatal — the
    // profile is already computed and downstream code can use it for
    // this run; the next run will just retry the cache write.
    let _ = crate::project::profile_cache::save(runtime.as_ref(), &key_hex, &profile).await;

    Ok(profile)
}

/// Run the head pipeline live (no cache lookup). Used by
/// [`pass1_profile_with_cache`] when the cache misses.
async fn pass1_profile_single_file_live(
    runtime: Arc<dyn SystemRuntime>,
    project: &ProjectContext,
    format: &Format,
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
    let mut ctx = RenderContext::new(project, doc_info, format, &binaries);

    let stages: Vec<Box<dyn PipelineStage>> = vec![
        Box::new(ParseDocumentStage::new()),
        Box::new(MetadataMergeStage::new()),
        // Include-expansion threads child content through the
        // profile so transitive `{{< include … >}}` is visible
        // (bd-xfwx). Phase 8 sub-phase 8.0d's LinkResolutionStage
        // also depends on it: the AST walk must see post-include
        // content so a body link inside an included child counts as
        // a dependency edge of the parent.
        Box::new(IncludeExpansionStage::new()),
        Box::new(DocumentProfileStage::new()),
        // Pass-1 cross-doc body-link resolution. Reads the
        // post-include AST, writes `profile.body_link_targets` for
        // the dependency graph.
        Box::new(LinkResolutionStage::new()),
    ];

    let (output, _diagnostics) =
        run_pipeline(source_bytes, &source_name, &mut ctx, runtime, stages).await?;

    let profile = output.into_at_profile().ok_or_else(|| {
        QuartoError::other(
            "Pass 1 did not produce an AtProfile variant — pipeline shape unexpected",
        )
    })?;
    Ok(profile.profile)
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

    // === bd-ooleh: error/warning summary counts ===

    use quarto_error_reporting::DiagnosticKind;

    /// Minimal Pass-2 output stub so the counter tests don't have to
    /// build a full `RenderToFileResult`. Carries only the per-page
    /// diagnostics the counter reads through `OutputDiagnostics`.
    #[derive(Debug, Default)]
    struct StubOutput {
        diagnostics: Vec<DiagnosticMessage>,
    }

    impl OutputDiagnostics for StubOutput {
        fn diagnostics(&self) -> &[DiagnosticMessage] {
            &self.diagnostics
        }

        fn diagnostics_mut(&mut self) -> &mut [DiagnosticMessage] {
            &mut self.diagnostics
        }
    }

    fn failure(diagnostics: Vec<DiagnosticMessage>) -> FileFailure {
        FileFailure {
            input: PathBuf::from("doc.qmd"),
            error: "boom".to_string(),
            diagnostics,
            source_context: None,
        }
    }

    fn output_with(diagnostics: Vec<DiagnosticMessage>) -> StubOutput {
        StubOutput { diagnostics }
    }

    #[test]
    fn diagnostic_counts_empty_summary_is_zero() {
        let summary: ProjectRenderSummary<StubOutput> = ProjectRenderSummary::default();
        let counts = summary.diagnostic_counts();
        assert_eq!(counts, DiagnosticCounts::default());
        assert_eq!(counts.errors, 0);
        assert_eq!(counts.warnings, 0);
        assert!(counts.is_empty());
    }

    #[test]
    fn diagnostic_counts_legacy_pass2_failure_is_one_error() {
        // A render failure with no structured diagnostics counts as a
        // single error (its top-level `error` string is the one
        // problem).
        let summary: ProjectRenderSummary<StubOutput> = ProjectRenderSummary {
            pass2_failures: vec![failure(Vec::new())],
            ..Default::default()
        };
        let counts = summary.diagnostic_counts();
        assert_eq!(counts.errors, 1);
        assert_eq!(counts.warnings, 0);
        assert!(!counts.is_empty());
    }

    #[test]
    fn diagnostic_counts_pass2_failure_counts_structured_diagnostics_by_kind() {
        let summary: ProjectRenderSummary<StubOutput> = ProjectRenderSummary {
            pass2_failures: vec![failure(vec![
                DiagnosticMessage::error("e1"),
                DiagnosticMessage::error("e2"),
                DiagnosticMessage::warning("w1"),
            ])],
            ..Default::default()
        };
        let counts = summary.diagnostic_counts();
        assert_eq!(counts.errors, 2);
        assert_eq!(counts.warnings, 1);
    }

    #[test]
    fn diagnostic_counts_pass1_failure_without_diagnostics_is_one_error() {
        // D1 = option (a): a pass-1 failure with no structured
        // diagnostics reads as an error so the summary agrees with the
        // non-zero exit code (not the legacy `warning:` display prefix).
        let summary: ProjectRenderSummary<StubOutput> = ProjectRenderSummary {
            pass1_failures: vec![failure(Vec::new())],
            ..Default::default()
        };
        let counts = summary.diagnostic_counts();
        assert_eq!(counts.errors, 1);
        assert_eq!(counts.warnings, 0);
    }

    #[test]
    fn diagnostic_counts_project_diagnostics_ignore_info_and_note() {
        let summary: ProjectRenderSummary<StubOutput> = ProjectRenderSummary {
            project_diagnostics: vec![
                DiagnosticMessage::error("e"),
                DiagnosticMessage::warning("w"),
                DiagnosticMessage::info("i"),
                DiagnosticMessage::new(DiagnosticKind::Note, "n"),
            ],
            ..Default::default()
        };
        let counts = summary.diagnostic_counts();
        assert_eq!(counts.errors, 1);
        assert_eq!(counts.warnings, 1);
    }

    #[test]
    fn diagnostic_counts_successful_render_warnings_are_counted() {
        let summary: ProjectRenderSummary<StubOutput> = ProjectRenderSummary {
            outputs: vec![
                output_with(vec![DiagnosticMessage::warning("w1")]),
                output_with(vec![
                    DiagnosticMessage::warning("w2"),
                    DiagnosticMessage::error("e1"),
                ]),
            ],
            ..Default::default()
        };
        let counts = summary.diagnostic_counts();
        assert_eq!(counts.errors, 1);
        assert_eq!(counts.warnings, 2);
    }

    #[test]
    fn diagnostic_counts_aggregates_all_four_sources() {
        let summary: ProjectRenderSummary<StubOutput> = ProjectRenderSummary {
            // 1 error (legacy, no diagnostics)
            pass1_failures: vec![failure(Vec::new())],
            // 1 error + 1 warning (structured)
            pass2_failures: vec![failure(vec![
                DiagnosticMessage::error("e"),
                DiagnosticMessage::warning("w"),
            ])],
            // 1 error + 1 warning (+ ignored info)
            project_diagnostics: vec![
                DiagnosticMessage::error("pe"),
                DiagnosticMessage::warning("pw"),
                DiagnosticMessage::info("pi"),
            ],
            // 1 warning from a successful render
            outputs: vec![output_with(vec![DiagnosticMessage::warning("ow")])],
            stopped_early: false,
        };
        let counts = summary.diagnostic_counts();
        assert_eq!(counts.errors, 3);
        assert_eq!(counts.warnings, 3);
        assert!(!counts.is_empty());
    }

    // === bd-yjs54ptg (GH #220): strict-mode warning promotion ===

    #[test]
    fn promote_warnings_covers_all_four_sources() {
        let mut summary: ProjectRenderSummary<StubOutput> = ProjectRenderSummary {
            pass1_failures: vec![failure(vec![DiagnosticMessage::warning("p1w")])],
            pass2_failures: vec![failure(vec![
                DiagnosticMessage::error("p2e"),
                DiagnosticMessage::warning("p2w"),
            ])],
            project_diagnostics: vec![DiagnosticMessage::warning("pw")],
            outputs: vec![output_with(vec![DiagnosticMessage::warning("ow")])],
            stopped_early: false,
        };
        summary.promote_warnings_to_errors();

        let counts = summary.diagnostic_counts();
        assert_eq!(counts.warnings, 0, "no warnings may survive promotion");
        assert_eq!(counts.errors, 5, "every former warning counts as an error");

        // Kind is rewritten on the diagnostics themselves (printing
        // reads `kind` directly, not the counts).
        assert!(
            summary
                .pass1_failures
                .iter()
                .chain(&summary.pass2_failures)
                .flat_map(|f| &f.diagnostics)
                .chain(&summary.project_diagnostics)
                .chain(summary.outputs.iter().flat_map(|o| o.diagnostics()))
                .all(|d| d.kind == DiagnosticKind::Error)
        );
    }

    #[test]
    fn promote_warnings_leaves_info_and_note_alone() {
        let mut summary: ProjectRenderSummary<StubOutput> = ProjectRenderSummary {
            outputs: vec![output_with(vec![
                DiagnosticMessage::info("i"),
                DiagnosticMessage::new(DiagnosticKind::Note, "n"),
                DiagnosticMessage::warning("w"),
            ])],
            ..Default::default()
        };
        summary.promote_warnings_to_errors();

        let kinds: Vec<DiagnosticKind> = summary.outputs[0]
            .diagnostics()
            .iter()
            .map(|d| d.kind)
            .collect();
        assert_eq!(
            kinds,
            vec![
                DiagnosticKind::Info,
                DiagnosticKind::Note,
                DiagnosticKind::Error
            ]
        );
    }

    #[test]
    fn promote_warnings_on_clean_summary_is_a_no_op() {
        let mut summary: ProjectRenderSummary<StubOutput> = ProjectRenderSummary::default();
        summary.promote_warnings_to_errors();
        assert!(summary.diagnostic_counts().is_empty());
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
