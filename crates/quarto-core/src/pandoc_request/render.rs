//! The logic behind the browser's `render_pandoc_request` export: run the
//! pandoc-format pipeline with `PandocPrepareStage` as the tail and hand back
//! the request. The wasm wrapper (`wasm-quarto-hub-client`) only converts the
//! outcome to a JS object; everything testable lives here, so native tests
//! drive it with `NativeRuntime` over a directory.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use quarto_error_reporting::{DiagnosticKind, DiagnosticMessage};
use quarto_source_map::SourceContext;
use quarto_system_runtime::SystemRuntime;

use super::formats::pandoc_format;
use super::{PandocRequest, PrepareOptions, RequestPost, constants};
use crate::error::QuartoError;
use crate::format::Format;
use crate::pipeline::{build_pandoc_request_stages_fetching, run_pipeline};
use crate::project::{DocumentInfo, ProjectContext};
use crate::render::{BinaryDependencies, RenderContext, RenderOptions};
use crate::resource_resolver::ResourceResolverContext;

/// Which part of a book a request covers (`options.scope` of the export).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BookScope {
    /// The whole book when the project is a book, the format consolidates
    /// (typst, pdf, epub) and the active file is a chapter; the active page
    /// alone otherwise.
    #[default]
    Auto,
    /// The active page alone: the chapter-alone request.
    Chapter,
}

/// What actually happened, in `stats.book` of the export's envelope.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolvedBookScope {
    Book,
    Chapter,
}

/// Present when the project is a book.
#[derive(Debug)]
pub struct BookOutcomeInfo {
    pub scope: ResolvedBookScope,
    /// File-bearing items in the book's render list.
    pub chapters: usize,
    /// Each chapter's diagnostics with that chapter's own `SourceContext`
    /// (the outcome's own `diagnostics` hold only the unlocatable
    /// book-level ones).
    pub chapter_diagnostics: Vec<crate::project::book::ChapterDiagnostics>,
}

pub struct PandocRequestInput<'a> {
    pub path: &'a Path,
    pub content: &'a [u8],
    /// A `format:` key (`docx`, ...). Must be a downloadable format.
    pub format: &'a str,
    pub project: &'a ProjectContext,
    /// Seconds. Set as `SOURCE_DATE_EPOCH`; `None` leaves it unset.
    pub source_date_epoch: Option<i64>,
    pub captures: Vec<quarto_trace::EngineCapture>,
    /// Echoed into the request (`None` until the PDF work).
    pub typst_available_fonts: Option<Vec<String>>,
    /// The hub prelude installs the vfs-root resolver for every render;
    /// `None` leaves the pipeline's default.
    pub resolver: Option<ResourceResolverContext>,
    pub scope: BookScope,
    /// Every chapter's gzipped `EngineCapture[]`, keyed by sidecar key (see
    /// [`super::captures`]). Only the whole-book path (and, as the fallback
    /// when `captures` is empty, the active file's entry for a chapter-scope
    /// request) reads it.
    pub captures_by_path: std::collections::BTreeMap<String, Vec<u8>>,
    /// The error from parsing the active file's own capture blob, if it did
    /// not parse. Reported only when the request ends up covering the active
    /// page alone (as before); a whole-book request ignores that blob.
    pub capture_error: Option<String>,
    /// Awaited between chapters of a whole-book request (progress and
    /// cancel).
    pub hooks: Option<&'a dyn crate::project::book::BookRenderHooks>,
}

#[derive(Debug)]
pub struct PandocRequestOutcome {
    /// Absent when the document has errors or the request could not be built.
    pub request: Option<PandocRequest>,
    /// Warnings, and the errors that kept `request` from being built.
    pub diagnostics: Vec<DiagnosticMessage>,
    pub source_context: SourceContext,
    /// Why there is no request, when there is none.
    pub error: Option<String>,
    /// Code cells with no cached result, which the document shows as source.
    pub unexecuted_cells: usize,
    /// `None` outside a book project.
    pub book: Option<BookOutcomeInfo>,
}

impl PandocRequestOutcome {
    /// [`Self::failed`] for callers outside this module.
    pub(crate) fn failed_with(error: impl Into<String>) -> Self {
        Self::failed(error)
    }

    fn failed(error: impl Into<String>) -> Self {
        Self {
            request: None,
            diagnostics: Vec::new(),
            source_context: SourceContext::default(),
            error: Some(error.into()),
            unexecuted_cells: 0,
            book: None,
        }
    }
}

pub async fn render_pandoc_request(
    input: PandocRequestInput<'_>,
    runtime: Arc<dyn SystemRuntime>,
) -> PandocRequestOutcome {
    let PandocRequestInput {
        path,
        content,
        format: format_key,
        project,
        source_date_epoch,
        captures,
        typst_available_fonts,
        resolver,
        scope,
        captures_by_path,
        capture_error,
        hooks,
    } = input;

    // D8: only the table's formats. `pdf` is the typst request plus a
    // `compile_typst` post step (the host compiles the `.typ`), so it is
    // built as typst.
    let Some(info) = pandoc_format(format_key) else {
        return PandocRequestOutcome::failed(format!(
            "{format_key} cannot be rendered by pandoc in the browser"
        ));
    };
    if !info.available {
        return PandocRequestOutcome::failed(format!(
            "{format_key} output is not available in the browser yet"
        ));
    }
    let compile_typst = format_key == "pdf";
    let format = match Format::from_format_string(if compile_typst { "typst" } else { format_key })
    {
        Ok(f) => f,
        Err(e) => return PandocRequestOutcome::failed(e),
    };

    let prepare_options = PrepareOptions {
        temp_root: PathBuf::from(&constants().share_root),
        source_date_epoch,
        collect_resources: true,
        typst_available_fonts: typst_available_fonts.clone(),
        post: if compile_typst {
            RequestPost::CompileTypst
        } else {
            RequestPost::None
        },
    };

    // A document inside a `_quarto.yml` project renders as the active page of
    // the project (R7), through the orchestrator's pass 1 (the index) and a
    // pandoc Pass2Renderer.
    if !project.is_single_file {
        return render_project_request(
            path,
            format,
            if compile_typst { "typst" } else { format_key },
            project,
            prepare_options,
            ProjectRequestInputs {
                scope,
                captures,
                capture_error,
                captures_by_path,
                hooks,
            },
            runtime,
        )
        .await;
    }

    // A single document: the active file's own capture blob, else (the
    // chapter-scope fallback) its entry in the per-path map.
    let captures = match active_file_captures(
        runtime.as_ref(),
        path,
        captures,
        capture_error,
        &captures_by_path,
    ) {
        Ok(captures) => captures,
        Err(message) => return PandocRequestOutcome::failed(message),
    };

    let doc = DocumentInfo::from_path(path);
    let binaries = BinaryDependencies::new();
    let mut ctx =
        RenderContext::new(project, &doc, &format, &binaries).with_options(RenderOptions {
            verbose: false,
            execute: false,
            use_freeze: false,
            output_path: None,
        });
    ctx.resource_resolver = resolver;
    ctx.prepare_options = Some(prepare_options);
    let source_name = path.to_string_lossy();
    build_request_in_context(
        &mut ctx,
        content,
        &source_name,
        runtime,
        captures,
        typst_available_fonts,
    )
    .await
}

/// Run the pandoc-request pipeline over `content` in a prepared context and
/// collect the outcome. `ctx.prepare_options` must be set. Shared by the
/// single-document path and [`RenderToPandocRequestRenderer`], so a project
/// document goes through exactly the stages a single document does.
///
/// [`RenderToPandocRequestRenderer`]: crate::project::pass2_renderer::RenderToPandocRequestRenderer
pub(crate) async fn build_request_in_context(
    ctx: &mut RenderContext<'_>,
    content: &[u8],
    source_name: &str,
    runtime: Arc<dyn SystemRuntime>,
    captures: Vec<quarto_trace::EngineCapture>,
    typst_available_fonts: Option<Vec<String>>,
) -> PandocRequestOutcome {
    // The browser has no engines: cells without a capture pass through
    // inert and silently (the response counts them), whatever the native
    // registry would have done with them.
    ctx.execution_policy = crate::engine::ExecutionPolicy::None;

    let (output, diagnostics) = match run_pipeline(
        content,
        source_name,
        ctx,
        runtime,
        build_pandoc_request_stages_fetching(captures),
    )
    .await
    {
        Ok(ok) => ok,
        Err(QuartoError::Parse(parse_error)) => {
            return PandocRequestOutcome {
                request: None,
                diagnostics: parse_error.diagnostics.clone(),
                source_context: parse_error.source_context.clone(),
                error: Some(QuartoError::Parse(parse_error).to_string()),
                unexecuted_cells: 0,
                book: None,
            };
        }
        Err(e) => return PandocRequestOutcome::failed(e.to_string()),
    };
    let source_context = output
        .into_rendered_output()
        .map(|r| r.source_context)
        .unwrap_or_default();

    // A document with errors does not produce a request (as natively).
    let first_error = diagnostics
        .iter()
        .find(|d| d.kind == DiagnosticKind::Error)
        .map(|d| d.title.clone());
    if let Some(title) = first_error {
        return PandocRequestOutcome {
            request: None,
            diagnostics,
            source_context,
            error: Some(title),
            unexecuted_cells: ctx.unexecuted_cells,
            book: None,
        };
    }

    let Some(mut request) = ctx.pandoc_request.take() else {
        return PandocRequestOutcome::failed("the pipeline produced no pandoc request");
    };
    if typst_available_fonts.is_some() {
        request.typst_available_fonts = typst_available_fonts;
    }
    request.job_id = request.compute_job_id();
    PandocRequestOutcome {
        request: Some(request),
        diagnostics,
        source_context,
        error: None,
        unexecuted_cells: ctx.unexecuted_cells,
        book: None,
    }
}

/// The active file's capture inputs for a request that covers it alone:
/// `captures` (the blob the caller gave) when there is one, a parse error
/// from that blob as the same failure as before, else the file's entry in
/// `captures_by_path` (the chapter-scope fallback).
fn active_file_captures(
    runtime: &dyn SystemRuntime,
    path: &Path,
    captures: Vec<quarto_trace::EngineCapture>,
    capture_error: Option<String>,
    captures_by_path: &std::collections::BTreeMap<String, Vec<u8>>,
) -> Result<Vec<quarto_trace::EngineCapture>, String> {
    if let Some(error) = capture_error {
        return Err(format!("Failed to parse capture: {error}"));
    }
    if !captures.is_empty() {
        return Ok(captures);
    }
    let key = super::captures::sidecar_key(runtime, path);
    match captures_by_path.get(&key) {
        Some(bytes) => super::captures::parse_capture_gz(Some(bytes))
            .map_err(|e| format!("Failed to parse capture: {e}")),
        None => Ok(Vec::new()),
    }
}

/// What `render_project_request` needs beyond the document: the scope and
/// the capture inputs.
struct ProjectRequestInputs<'a> {
    scope: BookScope,
    captures: Vec<quarto_trace::EngineCapture>,
    capture_error: Option<String>,
    captures_by_path: std::collections::BTreeMap<String, Vec<u8>>,
    hooks: Option<&'a dyn crate::project::book::BookRenderHooks>,
}

/// A book's render list as the whole-book dispatch needs it: how many items
/// bear a file and whether the active file is one of them. Cheap and
/// pass-free (a pure function of the `book:` config and the file system).
struct BookMembership {
    chapters: usize,
    active_is_chapter: bool,
}

/// `Err` carries the book's own error (`Q-5-34`/`Q-5-35`/`Q-5-36`).
fn book_membership(
    project: &ProjectContext,
    active: &Path,
    runtime: &dyn SystemRuntime,
) -> Result<BookMembership, QuartoError> {
    let files = crate::project::book::book_chapter_files(project, runtime)?;
    let active_key = active.strip_prefix(&project.dir).ok().map(|rel| {
        crate::project::book::links::normalize_book_path(&rel.to_string_lossy().replace('\\', "/"))
    });
    let mut chapters = 0;
    let mut active_is_chapter = false;
    for file in &files {
        chapters += 1;
        let key = crate::project::book::links::normalize_book_path(
            &file.to_string_lossy().replace('\\', "/"),
        );
        if active_key.as_deref() == Some(key.as_str()) {
            active_is_chapter = true;
        }
    }
    Ok(BookMembership {
        chapters,
        active_is_chapter,
    })
}

/// A document inside a project: the project's pass 1 builds the index (so
/// cross-document links, the project's `format:` layers and its metadata all
/// resolve as they do natively), then pass 2 renders only the active page,
/// with a renderer that builds the request instead of writing a file. For a
/// book chapter whose format consolidates (typst, pdf, epub) and whose scope
/// is not `Chapter`, the whole book is rendered instead.
async fn render_project_request(
    path: &Path,
    format: Format,
    format_str: &str,
    project: &ProjectContext,
    prepare_options: PrepareOptions,
    inputs: ProjectRequestInputs<'_>,
    runtime: Arc<dyn SystemRuntime>,
) -> PandocRequestOutcome {
    use crate::project::ProjectKind;
    use crate::project::orchestrator::{ProjectPipeline, RenderMode, project_type_for};
    use crate::project::pass2_renderer::RenderToPandocRequestRenderer;

    let ProjectRequestInputs {
        scope,
        captures,
        capture_error,
        captures_by_path,
        hooks,
    } = inputs;

    // The pipeline fills in per-render state (the book's render list) and
    // takes the project by `&mut`; the caller's copy stays as it was.
    let mut project = project.clone();
    let project_type = project_type_for(&project);
    // `project.files` holds canonical paths and the active-page filter
    // compares by equality.
    let active = runtime
        .canonicalize(path)
        .unwrap_or_else(|_| path.to_path_buf());

    // Book: decide before running anything whether this is the whole book.
    let is_book = project.project_kind() == ProjectKind::Book;
    let consolidates = matches!(
        format.identifier,
        crate::format::FormatIdentifier::Typst | crate::format::FormatIdentifier::Epub
    );
    let mut membership: Option<BookMembership> = None;
    if is_book {
        match book_membership(&project, &active, runtime.as_ref()) {
            Ok(m) => membership = Some(m),
            // An `auto` download of a book whose list is broken fails with
            // that error, as a render does; a chapter-alone request leaves it
            // to the pipeline, which reports it as it always has.
            Err(e) if scope == BookScope::Auto && consolidates => {
                return match e {
                    QuartoError::Parse(parse_error) => PandocRequestOutcome {
                        request: None,
                        diagnostics: parse_error.diagnostics.clone(),
                        source_context: parse_error.source_context.clone(),
                        error: Some(QuartoError::Parse(parse_error).to_string()),
                        unexecuted_cells: 0,
                        book: None,
                    },
                    other => PandocRequestOutcome::failed(other.to_string()),
                };
            }
            Err(_) => {}
        }
    }
    let whole_book = scope == BookScope::Auto
        && consolidates
        && membership.as_ref().is_some_and(|m| m.active_is_chapter);

    let renderer_captures;
    if whole_book {
        // The active chapter is treated like every other: its captures come
        // from the map, and the active file's own blob (and any error parsing
        // it) is ignored, so the request does not depend on which chapter is
        // active.
        renderer_captures = Vec::new();
    } else {
        renderer_captures = match active_file_captures(
            runtime.as_ref(),
            path,
            captures,
            capture_error,
            &captures_by_path,
        ) {
            Ok(captures) => captures,
            Err(message) => return PandocRequestOutcome::failed(message),
        };
        // A page outside the chapter list (or a chapter requested alone)
        // renders as a page of the project: keep the active file in the
        // render set, as the footer and 404 are (`restrict_render_list`).
        if is_book && let Ok(rel) = active.strip_prefix(&project.dir) {
            project.extra_render_files = vec![rel.to_path_buf()];
        }
        // The page skips the book merge, so hand it the book's title,
        // author and date as project metadata (the chapter's own win).
        if is_book
            && format.identifier == crate::format::FormatIdentifier::Typst
            && let Some(meta) = project.config.metadata.as_mut()
            && let Some(book) = meta.get("book").cloned()
        {
            crate::project::book::merge::seed_missing_book_title_metadata(meta, &book);
        }
    }

    let renderer = RenderToPandocRequestRenderer::new(prepare_options.clone(), renderer_captures);
    let mut pipeline = ProjectPipeline::with_renderer(
        &mut project,
        project_type,
        format,
        format_str,
        runtime,
        renderer,
    );

    if whole_book {
        let mut outcome = pipeline
            .run_book_request(crate::project::orchestrator::BookRequestParams {
                prepare_options,
                captures_by_path: &captures_by_path,
                hooks,
            })
            .await;
        if let Some(book) = outcome.book.as_mut() {
            book.chapters = membership.as_ref().map_or(book.chapters, |m| m.chapters);
        }
        return outcome;
    }

    let mut pipeline = pipeline.with_mode(RenderMode::ActivePage(active.clone()));
    let book_info = membership.map(|m| BookOutcomeInfo {
        scope: ResolvedBookScope::Chapter,
        chapters: m.chapters,
        chapter_diagnostics: Vec::new(),
    });
    let summary = match pipeline.run().await {
        Ok(summary) => summary,
        Err(QuartoError::Parse(parse_error)) => {
            return PandocRequestOutcome {
                request: None,
                diagnostics: parse_error.diagnostics.clone(),
                source_context: parse_error.source_context.clone(),
                error: Some(QuartoError::Parse(parse_error).to_string()),
                unexecuted_cells: 0,
                book: book_info,
            };
        }
        Err(e) => return PandocRequestOutcome::failed(e.to_string()),
    };

    let Some(page) = summary.outputs.into_iter().next() else {
        // Pass 1 drops a page that fails to parse, and pass 2 never sees it:
        // report the page's own diagnostics (as the preview does).
        let failure = summary
            .pass1_failures
            .into_iter()
            .find(|f| f.input == active)
            .or_else(|| summary.pass2_failures.into_iter().next());
        return match failure {
            Some(failure) => PandocRequestOutcome {
                request: None,
                diagnostics: failure.diagnostics,
                source_context: failure.source_context.unwrap_or_default(),
                error: Some(format!(
                    "Rendering {} failed: {}",
                    failure.input.display(),
                    failure.error
                )),
                unexecuted_cells: 0,
                book: book_info,
            },
            None => PandocRequestOutcome {
                book: book_info,
                ..PandocRequestOutcome::failed(
                    "The project render produced no output for the active page",
                )
            },
        };
    };

    let mut outcome = page.outcome;
    outcome.diagnostics.extend(summary.project_diagnostics);
    outcome.book = book_info;
    outcome
}
