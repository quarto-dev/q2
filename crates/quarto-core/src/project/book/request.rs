/*
 * request.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * The browser's whole-book pandoc request: the shared single-file-merge core
 * (`single_file_render::render_book_core`) with the request tail.
 */

use std::sync::Arc;

use quarto_error_reporting::{DiagnosticKind, DiagnosticMessage};
use quarto_system_runtime::SystemRuntime;

use super::core_types::{BookRenderOptions, ChapterDiagnostics};
use super::links::normalize_book_path;
use super::render_item::BookRenderItem;
use super::single_file_render::{
    BookCoreArgs, BookCoreOutcome, BookSetup, BookTail, render_book_core,
};
use crate::error::QuartoError;
use crate::format::Format;
use crate::pandoc_request::render::{BookOutcomeInfo, PandocRequestOutcome, ResolvedBookScope};
use crate::pipeline::{PartialKind, build_pandoc_request_finishing_stages};
use crate::project::ProjectContext;
use crate::project::orchestrator::{BookRequestParams, project_type_for};
use crate::render::BinaryDependencies;
use crate::resource_resolver::ResourceResolverContext;
use crate::transform::TransformPhase;

/// Render every chapter of `project`'s book and build one request from the
/// merged document, exactly as a single document's request is built. A
/// failing chapter fails the whole render with that chapter's own
/// diagnostics (native refuses to make such a book).
pub(crate) async fn render_book_request(
    project: &ProjectContext,
    book_items: &[BookRenderItem],
    format: &Format,
    runtime: Arc<dyn SystemRuntime>,
    params: BookRequestParams<'_>,
) -> PandocRequestOutcome {
    let BookRequestParams {
        prepare_options,
        captures_by_path,
        hooks,
    } = params;
    let chapters_total = book_items.iter().filter(|i| i.file.is_some()).count();
    let typst_available_fonts = prepare_options.typst_available_fonts.clone();

    // The browser has no engines: cells without a capture pass through
    // inert and silently (the response counts them).
    let options = BookRenderOptions {
        engine_registry_override: None,
        execution_policy: crate::engine::ExecutionPolicy::None,
    };
    let binaries = BinaryDependencies::new();
    let project_type = project_type_for(project);

    let runtime_for_kind = runtime.clone();
    let mut chapter_kind = |item: &BookRenderItem| {
        let Some(file) = &item.file else {
            return (
                PartialKind::Request {
                    captures: Vec::new(),
                },
                Vec::new(),
            );
        };
        let label = file.to_string_lossy().replace('\\', "/");
        // The sidecar key of this chapter: its `project.dir`-relative `href`
        // (`./intro.qmd`, `a/../b.qmd`) normalized, then made VFS-root
        // relative. Parsed here and dropped with the chapter.
        let abs = project.dir.join(normalize_book_path(&label));
        let key = crate::pandoc_request::captures::sidecar_key(runtime_for_kind.as_ref(), &abs);
        match captures_by_path.get(&key) {
            None => (
                PartialKind::Request {
                    captures: Vec::new(),
                },
                Vec::new(),
            ),
            Some(bytes) => match crate::pandoc_request::captures::parse_capture_gz(Some(bytes)) {
                Ok(captures) => (PartialKind::Request { captures }, Vec::new()),
                Err(e) => (
                    PartialKind::Request {
                        captures: Vec::new(),
                    },
                    vec![DiagnosticMessage::warning(format!(
                        "Could not read the recorded results for {label}: {e}; its code is shown as source"
                    ))],
                ),
            },
        }
    };

    let outcome = render_book_core(
        BookCoreArgs {
            project,
            book_items,
            format,
            runtime: runtime.clone(),
            binaries: &binaries,
            options: &options,
            finishing_stages: build_pandoc_request_finishing_stages(TransformPhase::Navigation),
            prepare_options: Some(prepare_options),
            hooks,
        },
        &mut chapter_kind,
        |stem| {
            // Nothing is written: the merged document pretends to be a page
            // at the project root, so its image targets and links are made
            // relative to the project root, the one thing pandoc can resolve
            // here (its resource path is the document directory).
            let synthetic_input = project.dir.join(format!("{stem}.qmd"));
            let output_path = synthetic_input.with_extension(&format.output_extension);
            let resolver = ResourceResolverContext::website(
                &project.dir,
                &output_path,
                project_type.lib_dir(),
                stem,
            );
            Ok((
                BookSetup {
                    synthetic_input,
                    output_path,
                    resolver,
                },
                (),
            ))
        },
        |ctx, _, _, _| {
            ctx.pandoc_request
                .take()
                .ok_or_else(|| QuartoError::other("the pipeline produced no pandoc request"))
        },
    )
    .await;

    let book = |chapter_diagnostics: Vec<ChapterDiagnostics>| BookOutcomeInfo {
        scope: ResolvedBookScope::Book,
        chapters: chapters_total,
        chapter_diagnostics,
    };

    match outcome {
        Err(QuartoError::Parse(parse_error)) => PandocRequestOutcome {
            request: None,
            diagnostics: parse_error.diagnostics.clone(),
            source_context: parse_error.source_context.clone(),
            error: Some(QuartoError::Parse(parse_error).to_string()),
            unexecuted_cells: 0,
            book: Some(book(Vec::new())),
        },
        Err(e) => PandocRequestOutcome {
            book: Some(book(Vec::new())),
            ..PandocRequestOutcome::failed_with(e.to_string())
        },
        Ok(BookCoreOutcome::Cancelled) => PandocRequestOutcome {
            book: Some(book(Vec::new())),
            ..PandocRequestOutcome::failed_with("cancelled")
        },
        Ok(BookCoreOutcome::ChapterFailed { file, error }) => {
            // Located in the failing chapter's own file: a `Parse` error
            // carries its diagnostics and its own `SourceContext`.
            let message = format!("Rendering {file} failed: {error}");
            let chapter_diagnostics = match error {
                QuartoError::Parse(parse_error) => vec![ChapterDiagnostics {
                    file,
                    diagnostics: parse_error.diagnostics.clone(),
                    source_context: parse_error.source_context.clone(),
                }],
                _ => Vec::new(),
            };
            PandocRequestOutcome {
                book: Some(book(chapter_diagnostics)),
                ..PandocRequestOutcome::failed_with(message)
            }
        }
        Ok(BookCoreOutcome::Done { value, tail }) => {
            finish_request(value, tail, typst_available_fonts, chapters_total)
        }
    }
}

/// The request tail: a book with an error-severity diagnostic anywhere does
/// not produce a request (native does not stop on one, since its writer still
/// runs, so the scan belongs here, not in the shared core), as the
/// single-document path does.
fn finish_request(
    mut request: crate::pandoc_request::PandocRequest,
    tail: BookTail,
    typst_available_fonts: Option<Vec<String>>,
    chapters_total: usize,
) -> PandocRequestOutcome {
    let first_error = tail
        .chapter_diagnostics
        .iter()
        .flat_map(|c| c.diagnostics.iter())
        .chain(tail.book_diagnostics.iter())
        .find(|d| d.kind == DiagnosticKind::Error)
        .map(|d| d.title.clone());
    let BookTail {
        source_context,
        book_diagnostics,
        chapter_diagnostics,
        unexecuted_cells,
        ..
    } = tail;
    let book = BookOutcomeInfo {
        scope: ResolvedBookScope::Book,
        chapters: chapters_total,
        chapter_diagnostics,
    };
    if let Some(title) = first_error {
        return PandocRequestOutcome {
            request: None,
            diagnostics: book_diagnostics,
            source_context,
            error: Some(title),
            unexecuted_cells,
            book: Some(book),
        };
    }
    if typst_available_fonts.is_some() {
        request.typst_available_fonts = typst_available_fonts;
    }
    request.job_id = request.compute_job_id();
    PandocRequestOutcome {
        request: Some(request),
        diagnostics: book_diagnostics,
        source_context,
        error: None,
        unexecuted_cells,
        book: Some(book),
    }
}
