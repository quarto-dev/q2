/*
 * core_types.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Ungated types shared by the single-file book render core
 * (`single_file_render.rs`) and its two callers: native `q2 render` (writes
 * files) and the browser's whole-book pandoc request. None of them names an
 * item of `crate::render_to_file`, which does not exist on wasm.
 */

use quarto_error_reporting::DiagnosticMessage;
use quarto_source_map::SourceContext;

/// What the chapter loop needs from the caller's render options. The native
/// caller builds it from its `RenderToFileOptions`; the browser sets
/// `execution_policy = ExecutionPolicy::None` (it has no engines).
#[derive(Clone, Default)]
pub struct BookRenderOptions {
    pub engine_registry_override: Option<std::sync::Arc<crate::engine::EngineRegistry>>,
    pub execution_policy: crate::engine::ExecutionPolicy,
}

/// One chapter's diagnostics with the `SourceContext` they were located
/// against. The merged document carries an empty `SourceContext` (a known
/// scope gap), so a diagnostic raised in chapter 3 can only be resolved
/// against chapter 3's own context. A context is kept only for a chapter that
/// has diagnostics, so the per-iteration drop of chapter state holds.
#[derive(Debug, Clone)]
pub struct ChapterDiagnostics {
    /// The chapter's project-relative file, as the book lists it.
    pub file: String,
    pub diagnostics: Vec<DiagnosticMessage>,
    pub source_context: SourceContext,
}

/// The loop was cancelled by the host (see [`BookRenderHooks`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cancelled;

/// Called by the chapter loop between chapters. Wasm futures with no real
/// await never yield to the event loop, so neither a status update nor an
/// `AbortSignal.abort()` could be delivered during the loop; the loop awaits
/// this instead. `SystemRuntime` stays unchanged. Native passes no hooks.
///
/// Called exactly once per file-bearing item, in book order, with a 1-based
/// `index` and the number of file-bearing items as `total`; never for
/// dividers, never for the tail (merge, Crossref, citeproc, prepare).
#[async_trait::async_trait(?Send)]
pub trait BookRenderHooks {
    async fn before_chapter(&self, index: usize, total: usize, file: &str)
    -> Result<(), Cancelled>;
}
