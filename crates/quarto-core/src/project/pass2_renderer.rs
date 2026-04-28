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

use std::sync::Arc;

use async_trait::async_trait;
use quarto_system_runtime::SystemRuntime;

use crate::Result;
use crate::artifact::ArtifactStore;
use crate::format::Format;
use crate::project::index::ProjectIndex;
use crate::project::{DocumentInfo, ProjectContext};

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
    /// - WASM (Phase 9 sub-phase 9.2): a `WasmPassTwoOutput`
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
}
