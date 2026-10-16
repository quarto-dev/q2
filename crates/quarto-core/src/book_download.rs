//! A whole book rendered to one file for download (epic E3/E11, Plan 10).
//!
//! The preview server's "Download book as X": the book's own project
//! pipeline (Pass 1, `pre_render`, per-chapter render, merge, finishing,
//! typst or pandoc) run into a request directory instead of `_book/`. The
//! caller has already resolved the scope to [`ResolvedBookScope::Book`]
//! (`resolve_book_scope`), so this does not decide it again.
//!
//! - The project is cloned with `output_dir` set to the request directory, so
//!   the output, the resource flush and the image links all land there; no
//!   tail runs, so `_book/`, `render-manifest.json` and `llms-manifest.json`
//!   are never touched. (`.quarto/` is not untouched: Pass 1 writes
//!   `cache/profiles/*` and the crossref filter `crossref-index.json`.)
//! - The active chapter's text comes from the editor through an
//!   [`RenderOverlay`]; every other chapter is read from disk. Unsaved edits
//!   in other chapters are not part of the download.
//! - Captured cell outputs are resolved per chapter, server-side only (E10),
//!   from the sidecar entry of that chapter. A chapter without one compiles
//!   unexecuted and counts into the notice.
//! - One download per project at a time ([`BookDownloadFailure::Busy`]); a
//!   cancelled token stops the render at the next chapter boundary and
//!   interrupts the merge tail's children.
//! - Pre- and post-render scripts are CLI-only and do not run, and a book's
//!   own `output-ext` is ignored (the format decides: `typst_target`).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use quarto_error_reporting::DiagnosticMessage;
use quarto_source_map::SourceContext;
use quarto_system_runtime::SystemRuntime;
use quarto_trace::EngineCapture;

use crate::error::QuartoError;
use crate::format::Format;
use crate::overlay_runtime::{RenderOverlay, normalize_overlay_path};
use crate::pipeline::PartialKind;
use crate::project::book::links::normalize_book_path;
use crate::project::book::render_item::{BookRenderItem, book_chapter_files};
use crate::project::book::single_file_render::{BookFileExtras, BookFileOutcome};
use crate::project::book::{BookRenderHooks, Cancelled};
use crate::project::orchestrator::{ProjectPipeline, project_type_for};
use crate::project::{ProjectContext, ProjectKind};
use crate::render_to_file::{RenderToFileOptions, TypstTarget};
use crate::stage::Cancellation;

/// What a whole-book download needs.
pub struct BookDownloadParams<'a> {
    /// The active file (absolute). Any file of the book; with `content` it is
    /// the chapter whose text the editor holds.
    pub path: &'a Path,
    /// The editor's current text for `path`.
    pub content: Option<String>,
    /// The base format: `typst`, `typst-pdf` is `typst` with a target.
    pub format: &'a Format,
    /// `Pdf` compiles, `Source` returns the `.typ`; `None` keeps the
    /// format's own extension (epub).
    pub typst_target: Option<TypstTarget>,
    /// An existing empty directory inside the project (a [`PreviewOutDir`]).
    ///
    /// [`PreviewOutDir`]: crate::preview_out::PreviewOutDir
    pub request_dir: &'a Path,
    /// The hub root, canonical: sidecar keys are relative to it.
    pub hub_root: &'a Path,
    /// Every sidecar entry, keyed by hub-root-relative path.
    pub captures: &'a HashMap<String, Vec<EngineCapture>>,
    pub cancellation: Cancellation,
    /// Bound on each pandoc or typst child of the merge tail.
    pub child_deadline: Option<Duration>,
    /// Runtime-layer metadata for this render (FR12); `None` adds nothing.
    /// Merged into the one overlay the download builds.
    pub metadata: Option<serde_json::Value>,
    /// The native runtime the whole render uses, project discovery
    /// included. The live preview builds it from its `--profile`
    /// selection, so a whole-book download resolves the same profile
    /// as the single-page routes do.
    pub runtime: Arc<dyn SystemRuntime>,
}

/// A finished download.
#[derive(Debug)]
pub struct BookDownloadResult {
    /// The output file, inside the request directory.
    pub file: PathBuf,
    /// File-bearing chapters rendered.
    pub chapters: usize,
    /// Code cells left without a result, over every chapter.
    pub unexecuted_cells: usize,
    /// Every diagnostic of the render (chapters first, then book level).
    pub diagnostics: Vec<DiagnosticMessage>,
    /// Always empty: the merged document has no single source map, so the
    /// diagnostics are reported without locations.
    pub source_context: SourceContext,
}

#[derive(Debug)]
pub enum BookDownloadFailure {
    /// Another download of this project is running.
    Busy,
    /// The token was cancelled.
    Cancelled,
    Failed(QuartoError),
}

/// Projects with a download in flight, by discovered project directory.
static IN_FLIGHT: Mutex<Option<HashSet<PathBuf>>> = Mutex::new(None);

struct InFlight(PathBuf);

impl InFlight {
    fn acquire(project_dir: &Path) -> Option<Self> {
        let mut guard = IN_FLIGHT.lock().unwrap_or_else(|e| e.into_inner());
        guard
            .get_or_insert_with(HashSet::new)
            .insert(project_dir.to_path_buf())
            .then(|| Self(project_dir.to_path_buf()))
    }
}

impl Drop for InFlight {
    fn drop(&mut self) {
        let mut guard = IN_FLIGHT.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(set) = guard.as_mut() {
            set.remove(&self.0);
        }
    }
}

/// Stops the chapter loop before the next chapter once the token is set.
struct CancelHook(Cancellation);

#[async_trait::async_trait(?Send)]
impl BookRenderHooks for CancelHook {
    async fn before_chapter(
        &self,
        _index: usize,
        _total: usize,
        _file: &str,
    ) -> Result<(), Cancelled> {
        if self.0.is_cancelled() {
            Err(Cancelled)
        } else {
            Ok(())
        }
    }
}

/// Render the book that `params.path` belongs to into `params.request_dir`.
/// Blocking: call it from `spawn_blocking`.
pub fn render_book_download(
    params: BookDownloadParams<'_>,
) -> Result<BookDownloadResult, BookDownloadFailure> {
    let runtime = Arc::clone(&params.runtime);
    let failed = BookDownloadFailure::Failed;

    let mut project = ProjectContext::discover(params.path, runtime.as_ref()).map_err(failed)?;
    if project.project_kind() != ProjectKind::Book {
        return Err(failed(QuartoError::other(format!(
            "{} is not part of a book project",
            params.path.display()
        ))));
    }
    let Some(_in_flight) = InFlight::acquire(&project.dir) else {
        return Err(BookDownloadFailure::Busy);
    };
    if params.cancellation.is_cancelled() {
        return Err(BookDownloadFailure::Cancelled);
    }

    // The editor's text for the active chapter only.
    let mut overlay = HashMap::new();
    if let Some(content) = &params.content {
        let active = normalize_overlay_path(params.path);
        let chapters = book_chapter_files(&project, runtime.as_ref()).map_err(failed)?;
        let is_chapter = chapters.iter().any(|file| {
            let rel = normalize_book_path(&file.to_string_lossy());
            normalize_overlay_path(&project.dir.join(rel)) == active
        });
        if is_chapter {
            overlay.insert(active, content.clone().into_bytes());
        }
    }
    // One overlay for the path, applied once: the chapter's text plus the
    // request's metadata.
    let runtime: Arc<dyn SystemRuntime> = Arc::new(RenderOverlay {
        files: overlay,
        metadata: params.metadata.clone(),
    })
    .apply(runtime);

    // P10-D2: the target decides the extension; the book's own is ignored.
    let mut format = params.format.clone();
    match params.typst_target {
        Some(TypstTarget::Source) => format.output_extension = "typ".to_string(),
        Some(TypstTarget::Pdf) => format.output_extension = "pdf".to_string(),
        None => {}
    }

    project.output_dir = params.request_dir.to_path_buf();
    let options = RenderToFileOptions {
        quiet: true,
        execution_policy: crate::engine::ExecutionPolicy::None,
        cancellation: Some(params.cancellation.clone()),
        child_deadline: params.child_deadline,
        capture_materialize_dir: Some(params.request_dir.to_path_buf()),
        typst_target: params.typst_target,
        ..Default::default()
    };

    let project_dir = project.dir.clone();
    let hub_root = params.hub_root.to_path_buf();
    let materialize_dir = params.request_dir.to_path_buf();
    let captures = params.captures;
    let mut chapter_kind = |item: &BookRenderItem| {
        let none = || PartialKind::RequestMaterialized {
            captures: Vec::new(),
            materialize_dir: Some(materialize_dir.clone()),
        };
        let Some(file) = &item.file else {
            return (none(), Vec::new());
        };
        let rel = normalize_book_path(&file.to_string_lossy());
        let abs = project_dir.join(&rel);
        let key = crate::pandoc_request::captures::sidecar_key_from(&hub_root, &abs);
        match captures.get(&key) {
            Some(captures) => (
                PartialKind::RequestMaterialized {
                    captures: captures.clone(),
                    materialize_dir: Some(materialize_dir.clone()),
                },
                Vec::new(),
            ),
            None => (none(), Vec::new()),
        }
    };
    let hook = CancelHook(params.cancellation.clone());

    let project_type = project_type_for(&project);
    let format_str = format.identifier.to_string();
    let outcome = {
        let mut pipeline = ProjectPipeline::new(
            &mut project,
            project_type,
            format.clone(),
            format_str,
            &options,
            runtime.clone(),
        );
        pollster::block_on(pipeline.run_book_download(BookFileExtras {
            chapter_kind: Some(&mut chapter_kind),
            hooks: Some(&hook),
        }))
    };
    let result = match outcome.map_err(failed)? {
        BookFileOutcome::Cancelled => return Err(BookDownloadFailure::Cancelled),
        BookFileOutcome::Done(result) => result,
    };
    if params.cancellation.is_cancelled() {
        return Err(BookDownloadFailure::Cancelled);
    }
    let chapters = project
        .book_render_items
        .as_ref()
        .map_or(0, |items| items.iter().filter(|i| i.file.is_some()).count());
    Ok(BookDownloadResult {
        file: result.output_path,
        chapters,
        unexecuted_cells: result.unexecuted_cells,
        diagnostics: result.render_output.diagnostics,
        source_context: SourceContext::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_system_runtime::NativeRuntime;

    #[test]
    fn a_second_download_of_the_same_project_is_busy_until_the_first_ends() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        std::fs::write(
            root.join("_quarto.yml"),
            "project:\n  type: book\nbook:\n  title: B\n  chapters:\n    - a.qmd\n",
        )
        .unwrap();
        std::fs::write(root.join("a.qmd"), "# A\n").unwrap();
        let format = Format::from_format_string("typst").unwrap();
        let captures = HashMap::new();
        let a = root.join("a.qmd");
        let params = || BookDownloadParams {
            path: &a,
            content: None,
            format: &format,
            typst_target: Some(TypstTarget::Source),
            request_dir: &root,
            hub_root: &root,
            captures: &captures,
            cancellation: {
                let c = Cancellation::new();
                c.cancel();
                c
            },
            child_deadline: None,
            metadata: None,
            runtime: Arc::new(NativeRuntime::new()),
        };
        let held = InFlight::acquire(&root).expect("free");
        assert!(matches!(
            render_book_download(params()),
            Err(BookDownloadFailure::Busy)
        ));
        drop(held);
        // Free again: the pre-cancelled token now stops it instead.
        assert!(matches!(
            render_book_download(params()),
            Err(BookDownloadFailure::Cancelled)
        ));
    }
}
