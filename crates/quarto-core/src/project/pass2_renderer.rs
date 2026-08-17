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
use crate::project::orchestrator::{FileFailure, file_failure_from_error};
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

    /// Render a batch of pre-filtered documents, returning outputs in
    /// `docs` order plus per-file failures.
    ///
    /// The orchestrator does `skip` / `render_set` filtering before
    /// calling this, so every entry in `docs` is meant to be rendered.
    /// Implementations must drain each render's Project-scoped
    /// artifacts into `project_artifacts` (directly, or via per-worker
    /// stores merged at the end — the merge is order-independent, see
    /// [`ArtifactStore::merge_into_project`]).
    ///
    /// `workers` is the desired parallelism (1 = force serial); serial
    /// renderers ignore it. `fail_fast` stops after the first failure
    /// (best-effort under parallelism — in-flight renders may still
    /// complete).
    ///
    /// **Default impl is serial** — it simply drives [`Self::render`]
    /// per document. This is what the WASM in-memory renderer and any
    /// other serial-only renderer use; they need no `Send`/`Sync`
    /// bounds. The native [`RenderToFileRenderer`] overrides this with
    /// a rayon fan-out (`Self::Output = RenderToFileResult` is `Send`
    /// there, so the threading bounds stay confined to that impl).
    async fn render_batch(
        &mut self,
        docs: &[&DocumentInfo],
        format: &Format,
        format_str: &str,
        project: &ProjectContext,
        index: Arc<ProjectIndex>,
        runtime: Arc<dyn SystemRuntime>,
        project_artifacts: &mut ArtifactStore,
        _workers: usize,
        fail_fast: bool,
    ) -> (Vec<Self::Output>, Vec<FileFailure>) {
        let mut outputs = Vec::with_capacity(docs.len());
        let mut failures = Vec::new();
        for doc_info in docs {
            match self
                .render(
                    doc_info,
                    format,
                    format_str,
                    project,
                    index.clone(),
                    runtime.clone(),
                    project_artifacts,
                )
                .await
            {
                Ok(result) => outputs.push(result),
                Err(e) => {
                    failures.push(file_failure_from_error(doc_info.input.clone(), e));
                    // Fail-fast: stop at the first error in document
                    // order. Deterministic (single-threaded).
                    if fail_fast {
                        break;
                    }
                }
            }
        }
        (outputs, failures)
    }

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
    /// [`crate::artifact_flush::flush_project_artifacts`].
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
    /// resolver. Otherwise the project-artifact flush writes to a
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
    /// The CLI `--to` value, if the user forced one (bd-l6itt34u minimal
    /// slice). It is merged as a top-priority `format: !prefer <to>` layer
    /// in each document's format resolution; `None` lets per-file / project
    /// `format:` declarations win. Set via
    /// `ProjectPipeline::with_format_override`.
    pub format_override: Option<String>,
}

#[cfg(not(target_arch = "wasm32"))]
impl<'a> RenderToFileRenderer<'a> {
    /// Build a renderer borrowing the given options for its
    /// lifetime. No `--to` override by default; the project pipeline sets
    /// one when the user forced `--to`.
    pub fn new(options: &'a crate::render_to_file::RenderToFileOptions) -> Self {
        Self {
            options,
            format_override: None,
        }
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
            self.format_override.as_deref(),
        )
    }

    /// Native Pass-2 fan-out (bd-3gj56).
    ///
    /// Renders the batch across rayon workers, each producing a
    /// **per-document** `ArtifactStore`. After the parallel section the
    /// per-doc stores are merged into `project_artifacts` **in document
    /// order**, so merge-conflict attribution and accumulation order
    /// match the serial path exactly (the merge itself is
    /// order-independent for the dedup case — see
    /// [`ArtifactStore::merge_into_project`]).
    ///
    /// Degrades to serial for `workers <= 1`, `docs.len() <= 1`, or if
    /// the rayon pool fails to build. Mirrors `pass_one_dispatch`.
    async fn render_batch(
        &mut self,
        docs: &[&DocumentInfo],
        _format: &Format,
        format_str: &str,
        project: &ProjectContext,
        index: Arc<ProjectIndex>,
        runtime: Arc<dyn SystemRuntime>,
        project_artifacts: &mut ArtifactStore,
        workers: usize,
        fail_fast: bool,
    ) -> (Vec<Self::Output>, Vec<FileFailure>) {
        if workers <= 1 || docs.len() <= 1 {
            return render_batch_serial(
                self.options,
                docs,
                format_str,
                project,
                &index,
                &runtime,
                project_artifacts,
                fail_fast,
                self.format_override.as_deref(),
            );
        }
        render_batch_parallel(
            self.options,
            docs,
            format_str,
            project,
            &index,
            &runtime,
            project_artifacts,
            workers,
            fail_fast,
            self.format_override.as_deref(),
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
// Native Pass-2 batch dispatch (bd-3gj56). Free functions so the
// rayon `Send`/`Sync` requirements stay confined to the concrete
// `RenderToFileResult` output and never leak onto the `Pass2Renderer`
// trait (which the non-`Send` WASM renderer also implements).
// ───────────────────────────────────────────────────────────────────

/// Serial batch render — the small-N / `QUARTO_JOBS=1` / pool-build
/// fallback path. Merges each doc's Project-scoped artifacts straight
/// into the shared accumulator, exactly as the pre-bd-3gj56 loop did.
#[cfg(not(target_arch = "wasm32"))]
#[allow(clippy::too_many_arguments)]
fn render_batch_serial(
    options: &crate::render_to_file::RenderToFileOptions,
    docs: &[&DocumentInfo],
    format_str: &str,
    project: &ProjectContext,
    index: &Arc<ProjectIndex>,
    runtime: &Arc<dyn SystemRuntime>,
    project_artifacts: &mut ArtifactStore,
    fail_fast: bool,
    format_override: Option<&str>,
) -> (
    Vec<crate::render_to_file::RenderToFileResult>,
    Vec<FileFailure>,
) {
    let start = std::time::Instant::now();
    crate::project::orchestrator::pass2_threads_record();
    let mut outputs = Vec::with_capacity(docs.len());
    let mut failures = Vec::new();
    for doc in docs {
        match crate::render_to_file::render_document_to_file(
            &doc.input,
            format_str,
            options,
            Some(project),
            runtime.clone(),
            Some(index.clone()),
            Some(project_artifacts),
            format_override,
        ) {
            Ok(result) => outputs.push(result),
            Err(e) => {
                failures.push(file_failure_from_error(doc.input.clone(), e));
                if fail_fast {
                    break;
                }
            }
        }
    }
    crate::project::orchestrator::pass2_record(docs.len(), start.elapsed().as_nanos() as u64);
    (outputs, failures)
}

/// Parallel batch render — rayon fan-out, mirroring
/// `pass_one_dispatch_parallel`.
///
/// Each rayon task renders one document into a **fresh per-document
/// `ArtifactStore`** (no shared mutable state during render). After the
/// parallel section the per-doc stores are merged into
/// `project_artifacts` in **document order**, so a cross-document
/// artifact-key conflict fails the later document — identical to the
/// serial path's behavior.
///
/// `collect_into_vec` preserves document order. Fail-fast uses a shared
/// `AtomicBool` to skip *starting* further renders (rayon does not
/// cancel in-flight tasks — accepted, same contract as Pass 1).
/// `catch_unwind` maps a panicking render to a per-file failure.
#[cfg(not(target_arch = "wasm32"))]
#[allow(clippy::too_many_arguments)]
fn render_batch_parallel(
    options: &crate::render_to_file::RenderToFileOptions,
    docs: &[&DocumentInfo],
    format_str: &str,
    project: &ProjectContext,
    index: &Arc<ProjectIndex>,
    runtime: &Arc<dyn SystemRuntime>,
    project_artifacts: &mut ArtifactStore,
    workers: usize,
    fail_fast: bool,
    format_override: Option<&str>,
) -> (
    Vec<crate::render_to_file::RenderToFileResult>,
    Vec<FileFailure>,
) {
    use rayon::iter::{IndexedParallelIterator, IntoParallelRefIterator, ParallelIterator};

    enum Outcome {
        /// Rendered output + that document's drained Project-scoped
        /// artifacts (merged into the master store after the section).
        Ok(
            Box<crate::render_to_file::RenderToFileResult>,
            ArtifactStore,
        ),
        Failure(FileFailure),
        /// Fail-fast short-circuit: another worker already errored, so
        /// this document was never rendered.
        Skipped,
    }

    let pool = match rayon::ThreadPoolBuilder::new()
        .num_threads(workers)
        .thread_name(|i| format!("quarto-pass2-{i}"))
        .build()
    {
        Ok(p) => p,
        // Pool construction failed (rare — OOM / thread-limit). Degrade
        // to the serial path rather than aborting the render.
        Err(_) => {
            return render_batch_serial(
                options,
                docs,
                format_str,
                project,
                index,
                runtime,
                project_artifacts,
                fail_fast,
                format_override,
            );
        }
    };

    let start = std::time::Instant::now();
    let aborted = std::sync::atomic::AtomicBool::new(false);
    let mut outcomes: Vec<Outcome> = Vec::with_capacity(docs.len());

    pool.install(|| {
        docs.par_iter()
            .map(|doc| {
                if fail_fast && aborted.load(std::sync::atomic::Ordering::Relaxed) {
                    return Outcome::Skipped;
                }
                // Record this worker thread for the `perf.pass2`
                // threads_used gauge.
                crate::project::orchestrator::pass2_threads_record();
                let mut doc_store = ArtifactStore::new();
                let rendered = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    crate::render_to_file::render_document_to_file(
                        &doc.input,
                        format_str,
                        options,
                        Some(project),
                        runtime.clone(),
                        Some(index.clone()),
                        Some(&mut doc_store),
                        format_override,
                    )
                }));
                match rendered {
                    Ok(Ok(result)) => Outcome::Ok(Box::new(result), doc_store),
                    Ok(Err(e)) => {
                        if fail_fast {
                            aborted.store(true, std::sync::atomic::Ordering::Relaxed);
                        }
                        Outcome::Failure(file_failure_from_error(doc.input.clone(), e))
                    }
                    Err(_) => {
                        if fail_fast {
                            aborted.store(true, std::sync::atomic::Ordering::Relaxed);
                        }
                        Outcome::Failure(file_failure_from_error(
                            doc.input.clone(),
                            crate::error::QuartoError::other(format!(
                                "render panicked for {}",
                                doc.input.display()
                            )),
                        ))
                    }
                }
            })
            .collect_into_vec(&mut outcomes);
    });

    // Reduce in document order: merge per-doc artifact stores into the
    // shared accumulator and collect outputs/failures. A merge conflict
    // is attributed to the document whose artifacts conflicted, matching
    // the serial path (where the later doc's render returns the error).
    let mut outputs = Vec::with_capacity(docs.len());
    let mut failures = Vec::new();
    for (doc, outcome) in docs.iter().zip(outcomes) {
        match outcome {
            Outcome::Ok(result, doc_store) => {
                match project_artifacts.merge_into_project(doc_store) {
                    Ok(_) => outputs.push(*result),
                    Err(e) => failures.push(file_failure_from_error(
                        doc.input.clone(),
                        crate::error::QuartoError::other(format!(
                            "Project-scoped artifact merge failed for {}: {}",
                            doc.input.display(),
                            e
                        )),
                    )),
                }
            }
            Outcome::Failure(f) => failures.push(f),
            Outcome::Skipped => {}
        }
    }
    crate::project::orchestrator::pass2_record(docs.len(), start.elapsed().as_nanos() as u64);
    (outputs, failures)
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
    /// Compiled theme CSS fingerprint, recovered from the
    /// `css:theme:<fingerprint>` Project-scoped artifact key
    /// produced by `CompileThemeCssStage` (Plan 2A item 11).
    /// Captured at the renderer level **before** the
    /// `drain_project_scoped` call so the value survives both the
    /// website-merge and default-project-flush paths. `None` if
    /// no theme artifact was produced.
    pub theme_fingerprint: Option<String>,
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

/// Drain `(src, dest)` resource-copy intents collected during the
/// pipeline into the validated [`OutputSink`] and flush.
///
/// This is the WASM/preview-side equivalent of the `sink.copy(...)`
/// loop in `render_to_file::render_document_to_file`. Both
/// renderers in this module call it after their artifact-flush
/// branch, so user-resource copies are committed alongside the
/// rest of the render's destructive output, governed by the sink's
/// `allowed_roots` and `src == dest` checks (bd-cfl67).
/// Returns the `Q-5-6` warnings for any referenced resources whose
/// source was missing (skipped, not copied); the caller merges these
/// into the page's diagnostics. A genuine copy fault surfaces as the
/// `Q-5-7` error (bd-bxrkxblx).
fn flush_resource_copies(
    copies: Vec<crate::render::ResourceCopyIntent>,
    resolver: &ResourceResolverContext,
    runtime: &dyn quarto_system_runtime::SystemRuntime,
) -> Result<Vec<DiagnosticMessage>> {
    if copies.is_empty() {
        return Ok(Vec::new());
    }
    let mut sink = crate::output_sink::OutputSink::new(resolver.allowed_output_roots());
    let warnings =
        crate::resource_copy_diagnostics::enqueue_resource_copies(copies, &mut sink, runtime)?;
    // This sink holds only resource copies, so any flush failure is
    // unambiguously a resource-copy fault → Q-5-7.
    sink.flush(runtime).map_err(|e| match &e {
        crate::output_sink::OutputSinkError::Copy { .. } => {
            crate::resource_copy_diagnostics::copy_failure_error(&e)
        }
        _ => crate::error::QuartoError::from(e),
    })?;
    Ok(warnings)
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

    /// bd-rz2we: when set, the per-page resolver is built with
    /// [`ResourceResolverContext::vfs_root_with_url_root`] using
    /// this string as the URL prefix while `vfs_root` keeps acting
    /// as the disk-write root. `None` keeps today's behavior
    /// (URL root derived from `vfs_root`). Used by native test
    /// helpers so rendered URLs don't capture the host's tempdir.
    vfs_url_root: Option<String>,

    /// Optional user-grammar provider attached by the caller. Shared
    /// across every page the renderer touches (one
    /// `RenderToHtmlRenderer` may produce many pages in `ActivePage`
    /// mode plus future multi-page modes). The pipeline is `?Send`
    /// so `Rc<RefCell<…>>` is correct on both wasm32 and on the
    /// native single-task executor used by tests. (bd-izfv)
    user_grammars: Option<Rc<RefCell<dyn quarto_highlight::UserGrammarProvider>>>,

    /// bd-uy4uygha: server-recorded engine captures for the active page,
    /// spliced into the HTML so hub-client's default `format: html` preview
    /// shows the output of a document executed by a connected `q2 provide-hub`.
    /// Empty (the default) renders code cells as source. Mirrors
    /// [`RenderToPreviewAstRenderer`]'s `captures` for the AST path.
    captures: Vec<quarto_trace::EngineCapture>,
}

impl RenderToHtmlRenderer {
    /// Build a renderer that resolves artifacts under the given
    /// synthetic VFS root.
    pub fn new(vfs_root: impl Into<std::path::PathBuf>) -> Self {
        Self {
            vfs_root: vfs_root.into(),
            vfs_url_root: None,
            user_grammars: None,
            captures: Vec::new(),
        }
    }

    /// Attach server-recorded engine captures to splice into the active page's
    /// HTML (bd-uy4uygha). Mirrors
    /// [`RenderToPreviewAstRenderer::with_captures`].
    pub fn with_captures(mut self, captures: Vec<quarto_trace::EngineCapture>) -> Self {
        self.captures = captures;
        self
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

    /// bd-rz2we: override the URL prefix used for resolved-asset
    /// links/srcs. Disk writes still go through `vfs_root` (a real
    /// tempdir in native test runs); only the URL strings embedded
    /// in HTML change. Used by native test helpers so rendered
    /// output doesn't leak the host's tempdir.
    pub fn with_url_root(mut self, url_root: impl Into<String>) -> Self {
        self.vfs_url_root = Some(url_root.into());
        self
    }

    fn build_resolver(&self) -> ResourceResolverContext {
        match &self.vfs_url_root {
            Some(url) => {
                ResourceResolverContext::vfs_root_with_url_root(self.vfs_root.clone(), url.clone())
            }
            None => ResourceResolverContext::vfs_root(self.vfs_root.clone()),
        }
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
        // bd-rz2we: native test helpers can override the URL prefix
        // via `with_url_root` to keep rendered URLs path-independent.
        let resolver = self.build_resolver();

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

        // bd-uy4uygha: thread the active page's captures into the HTML render
        // so recorded engine output appears without re-running the engine.
        let config =
            HtmlRenderConfig::with_resolver(resolver.clone()).with_captures(self.captures.clone());
        let source_name = doc_info.input.to_string_lossy().to_string();

        let mut render_output = render_qmd_to_html(
            &input_bytes,
            &source_name,
            &mut ctx,
            &config,
            runtime.clone(),
        )
        .await?;

        // Drain Project-scoped artifacts. Where they go next is decided
        // by `route_drained_project_artifacts`, shared with the other
        // Pass-2 renderer and with native `render_document_to_file`
        // (bd-gdhk) — accumulate for a once-per-project flush when
        // there is a shared lib dir, write in place otherwise.
        //
        // Page-scoped artifacts on `ctx.artifacts` travel back to JS
        // alongside the HTML regardless of which branch fires.
        //
        // Plan 2A item 11: capture the theme fingerprint **before**
        // the drain. After the drain the artifact is gone on the
        // default-project (write-in-place) path — neither
        // `ctx.artifacts` nor `project_artifacts` retain it. Stashing
        // on the output makes the value visible to both project types.
        let theme_fingerprint = ctx
            .artifacts
            .get_by_prefix("css:theme:")
            .first()
            .and_then(|(key, _)| key.strip_prefix("css:theme:"))
            .map(|s| s.to_string());

        let drained = ctx.artifacts.drain_project_scoped();
        let lib_dir = super::orchestrator::project_type_for(project).lib_dir();
        let mut sink = crate::output_sink::OutputSink::new(resolver.allowed_output_roots());
        crate::artifact_flush::route_drained_project_artifacts(
            drained,
            Some(project_artifacts),
            !lib_dir.is_empty(),
            &resolver,
            &mut sink,
            &doc_info.input,
        )?;
        // A no-op when the accumulate branch fired: `OutputSink::flush`
        // short-circuits on empty ops before touching the filesystem.
        sink.flush(runtime.as_ref())
            .map_err(crate::error::QuartoError::from)?;

        // bd-cfl67: drain user-resource copy intents (images etc.
        // collected by `ResourceCollectorTransform`) into the
        // validated sink so the bytes land at the destination the
        // rendered HTML's `<img src>` URL points to (in WASM
        // hub-client mode that's `{vfs_root}/<url>`; in a native
        // website that's `{output_dir}/<page_relative>/<url>`).
        let copy_warnings = flush_resource_copies(
            std::mem::take(&mut ctx.resource_copies),
            &resolver,
            runtime.as_ref(),
        )?;
        render_output.diagnostics.extend(copy_warnings);

        Ok(WasmPassTwoOutput {
            source_path: doc_info.input.clone(),
            payload: Pass2Payload::Html(render_output.html),
            diagnostics: render_output.diagnostics,
            source_context: render_output.source_context,
            page_artifacts: ctx.artifacts,
            theme_fingerprint,
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
        self.build_resolver()
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
    /// bd-rz2we: when set, the per-page resolver is built with
    /// [`ResourceResolverContext::vfs_root_with_url_root`] using
    /// this string as the URL prefix while `vfs_root` keeps acting
    /// as the disk-write root. `None` keeps today's behavior
    /// (URL root derived from `vfs_root`). Used by native test
    /// helpers (idempotence harness) so rendered URLs don't
    /// capture the host's tempdir.
    vfs_url_root: Option<String>,
    /// bd-lucp / bd-5yff4: ordered engine-execution captures used to
    /// splice recorded engine output into the AST at preview time (one
    /// per engine that ran server-side). Plumbed through to
    /// [`crate::pipeline::render_qmd_to_preview_ast`] on every per-doc
    /// `render` call. Empty by default (no splice; engine cells render as
    /// raw source — same as the pre-bd-lucp path).
    captures: Vec<quarto_trace::EngineCapture>,
    /// Optional transport JSON payload (a serialized
    /// [`crate::attribution::types::TransportAttributionData`]). When
    /// `Some`, the renderer installs a
    /// [`crate::attribution::PreBuiltAttributionProvider`] on the
    /// per-page `RenderContext` before the pipeline runs. The
    /// multi-doc WASM entry point reaches this slot through
    /// [`Self::with_attribution`]; the active-page ctx is constructed
    /// *inside* [`Self::render`] so direct ctx-side install isn't
    /// possible from the WASM call site.
    attribution_json: Option<String>,
}

impl RenderToPreviewAstRenderer {
    /// Build a q2-preview renderer that resolves artifacts under the
    /// given synthetic VFS root.
    pub fn new(vfs_root: impl Into<std::path::PathBuf>) -> Self {
        Self {
            vfs_root: vfs_root.into(),
            vfs_url_root: None,
            attribution_json: None,
            captures: Vec::new(),
        }
    }

    /// Attach the recorded [`EngineCapture`](quarto_trace::EngineCapture)
    /// sequence (one per engine that ran, in order). The renderer threads
    /// them into every per-doc `render_qmd_to_preview_ast` call so the
    /// [`CaptureSpliceStage`](crate::stage::CaptureSpliceStage) can fold
    /// the captured engine output blocks onto the document's engine code
    /// cells. Used by the WASM `render_page_for_preview` entry point when
    /// the SPA hands in a capture binary doc from the IndexDocument
    /// sidecar; native test callers either pass an empty vec (no splice)
    /// or hand in synthetic captures.
    pub fn with_captures(mut self, captures: Vec<quarto_trace::EngineCapture>) -> Self {
        self.captures = captures;
        self
    }

    /// Attach a transport JSON attribution payload. The renderer
    /// installs a [`crate::attribution::PreBuiltAttributionProvider`]
    /// on the per-page [`crate::render::RenderContext`] before
    /// running the pipeline, so [`crate::transforms::AttributionGenerateTransform`]
    /// builds `AttributionData` and
    /// [`crate::transforms::AttributionRenderTransform`] populates
    /// `ctx.format_options.json.attribution_*`. Mirrors
    /// [`RenderToHtmlRenderer::with_user_grammars`].
    pub fn with_attribution(mut self, json: String) -> Self {
        self.attribution_json = Some(json);
        self
    }

    /// bd-rz2we: override the URL prefix used for resolved-asset
    /// links/srcs and cross-page links. Disk writes still go
    /// through `vfs_root` (a real tempdir in native test runs);
    /// only the URL strings embedded in the rendered AST change.
    /// Used by native test helpers so rendered AST is
    /// path-independent across runs.
    pub fn with_url_root(mut self, url_root: impl Into<String>) -> Self {
        self.vfs_url_root = Some(url_root.into());
        self
    }

    fn build_resolver(&self) -> ResourceResolverContext {
        match &self.vfs_url_root {
            Some(url) => {
                ResourceResolverContext::vfs_root_with_url_root(self.vfs_root.clone(), url.clone())
            }
            None => ResourceResolverContext::vfs_root(self.vfs_root.clone()),
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

        // bd-rz2we: native test helpers can override the URL prefix
        // via `with_url_root` so rendered AST link/asset URLs stay
        // path-independent across runs in different tempdirs.
        let resolver = self.build_resolver();

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
        // Install the pre-built attribution provider when the renderer
        // was configured with a transport JSON payload. JSON parse +
        // interning is lazy inside `build()`, so this is cheap and
        // infallible at construction time; any payload error surfaces
        // through `AttributionGenerateTransform`'s diagnostics path.
        if let Some(json) = self.attribution_json.clone() {
            ctx.attribution_provider = Some(Arc::new(
                crate::attribution::PreBuiltAttributionProvider::new(json),
            ));
        }

        let source_name = doc_info.input.to_string_lossy().to_string();

        let mut preview_output = render_qmd_to_preview_ast(
            &input_bytes,
            &source_name,
            &mut ctx,
            runtime.clone(),
            None,
            // bd-lucp / bd-5yff4: forward the renderer-attached capture
            // sequence (if any) so `CaptureSpliceStage` can fold recorded
            // engine output blocks onto the live AST. Cloned per-doc so
            // every page in a project pipeline run sees the same
            // captures; future per-page capture maps would replace this
            // with an index-lookup.
            self.captures.clone(),
        )
        .await?;

        // Drain Project-scoped artifacts. Same routing as
        // `RenderToHtmlRenderer`, via the shared
        // `route_drained_project_artifacts` (bd-gdhk): shared lib dir
        // merges into the accumulator for `post_render` to flush,
        // no-lib-dir writes in place. The choice is artifact-flow, not
        // payload-flow, so HTML and q2-preview share it verbatim.
        //
        // Plan 2A item 11: capture the theme fingerprint **before**
        // the drain (see `RenderToHtmlRenderer::render` for the
        // rationale). q2-preview also stamps the theme bytes at
        // the stable `styles.css` path so the hub-client iframe can
        // read them regardless of single-doc / project layout —
        // `compile_theme_css` puts the artifact at
        // `quarto/quarto-theme-<fp>.css` for multi-doc projects, but
        // q2-preview previews one document at a time so the
        // fingerprint-suffixed path adds nothing for this consumer.
        // Honors Plan 1's stated contract that "RenderToPreviewAstRenderer
        // writes the compiled theme CSS to
        // /.quarto/project-artifacts/styles.css on every q2-preview render."
        let theme_artifact_entry = ctx
            .artifacts
            .get_by_prefix("css:theme:")
            .first()
            .map(|(k, a)| (k.to_string(), a.content.clone()));

        let theme_fingerprint = theme_artifact_entry
            .as_ref()
            .and_then(|(k, _)| k.strip_prefix("css:theme:"))
            .map(|s| s.to_string());

        if let Some((_, content)) = &theme_artifact_entry {
            // Compute the iframe-readable path directly from the VFS
            // root rather than via `resolver.on_disk_path_for` —
            // websites would route Project-scoped paths through
            // `site_libs/`, but the iframe wrapper expects the
            // unsuffixed location at `{vfs_root}/styles.css`.
            let iframe_path = self.vfs_root.join("styles.css");
            if let Some(parent) = iframe_path.parent() {
                let _ = runtime.dir_create(parent, true);
            }
            runtime.file_write(&iframe_path, content).map_err(|e| {
                crate::error::QuartoError::other(format!(
                    "Failed to write q2-preview theme CSS to iframe path {}: {}",
                    iframe_path.display(),
                    e
                ))
            })?;
        }

        let drained = ctx.artifacts.drain_project_scoped();
        let lib_dir = super::orchestrator::project_type_for(project).lib_dir();
        let mut sink = crate::output_sink::OutputSink::new(resolver.allowed_output_roots());
        crate::artifact_flush::route_drained_project_artifacts(
            drained,
            Some(project_artifacts),
            !lib_dir.is_empty(),
            &resolver,
            &mut sink,
            &doc_info.input,
        )?;
        // A no-op when the accumulate branch fired: `OutputSink::flush`
        // short-circuits on empty ops before touching the filesystem.
        sink.flush(runtime.as_ref())
            .map_err(crate::error::QuartoError::from)?;

        // bd-cfl67: see the matching comment in the HTML renderer.
        let copy_warnings = flush_resource_copies(
            std::mem::take(&mut ctx.resource_copies),
            &resolver,
            runtime.as_ref(),
        )?;
        preview_output.diagnostics.extend(copy_warnings);

        Ok(WasmPassTwoOutput {
            source_path: doc_info.input.clone(),
            payload: Pass2Payload::AstJson(preview_output.ast_json),
            diagnostics: preview_output.diagnostics,
            source_context: preview_output.source_context,
            page_artifacts: ctx.artifacts,
            theme_fingerprint,
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
        self.build_resolver()
    }
}
