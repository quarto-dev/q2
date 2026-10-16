//! A book chapter rendered alone, as a standalone document: the one
//! mechanism behind the browser's chapter-scope request, `q2 render ch.qmd`
//! (a named chapter renders alone unless `-M book-scope:book` asks for the
//! book), and (via `render_chapter_alone`) the native preview routes.

use std::path::Path;

use crate::format::{Format, FormatIdentifier};
use crate::project::{ProjectContext, ProjectKind};

/// Make a cloned book project render `active` as a page of the project (a
/// page outside the chapter list, or a chapter requested alone, is not in the
/// render set by itself: keep it there, as the footer and 404 are,
/// `restrict_render_list`). The page skips the book merge, so typst also
/// gets the book's title, author and date as project metadata (the chapter's
/// own win).
///
/// `active` must be absolute. A no-op outside a book project.
pub(crate) fn prepare_page_in_book(project: &mut ProjectContext, active: &Path, format: &Format) {
    if project.project_kind() != ProjectKind::Book {
        return;
    }
    if let Ok(rel) = active.strip_prefix(&project.dir) {
        project.extra_render_files = vec![rel.to_path_buf()];
    }
    if format.identifier == FormatIdentifier::Typst {
        seed_book_title_metadata(project);
    }
}

/// Give a typst page rendered outside the book merge the book's title, author
/// and date as project metadata (the page's own win). The orchestrator's loop
/// over several named files calls it on the shared project around the
/// consolidating files only, so the other files do not see it.
pub(crate) fn seed_book_title_metadata(project: &mut ProjectContext) {
    if let Some(meta) = project.config.metadata.as_mut()
        && let Some(book) = meta.get("book").cloned()
    {
        super::merge::seed_missing_book_title_metadata(meta, &book);
    }
}

/// Render the chapter (or any page of the project) at `abs_path` alone, as a
/// standalone document in `format`, writing it as `options` direct.
///
/// Built on the browser's mechanism: the project's pass 1, then pass 2 over
/// the active page only (`RenderMode::ActivePage`), with the page kept in the
/// render set and the book's title metadata seeded. `project` is cloned; the
/// caller's copy is untouched. `format` is rendered whatever the project and
/// the page declare.
#[cfg(not(target_arch = "wasm32"))]
pub async fn render_chapter_alone(
    project: &ProjectContext,
    abs_path: &Path,
    format: &Format,
    options: &crate::render_to_file::RenderToFileOptions,
    runtime: std::sync::Arc<dyn quarto_system_runtime::SystemRuntime>,
) -> crate::Result<crate::render_to_file::RenderToFileResult> {
    use crate::error::{ParseError, QuartoError};

    let active = runtime
        .canonicalize(abs_path)
        .unwrap_or_else(|_| abs_path.to_path_buf());
    // `format` is the caller's explicit request, so it is the project's format
    // for this render too: Pass 1 (and the sibling output extensions its index
    // records) run under it, as they always did for this API. (The
    // orchestrator's own chapter-alone path passes the project's resolved
    // format instead, FR11.)
    let summary =
        render_chapter_alone_summary(project, &active, format, format, options, runtime).await;
    // The summary leaves the engine shutdown to its caller (the orchestrator
    // shuts down once per render); this is that caller.
    if let Err(e) = project.registry.shutdown_all() {
        tracing::warn!("engine registry shutdown_all failed after a chapter render: {e}");
    }
    let summary = summary?;
    if let Some(output) = summary.outputs.into_iter().next() {
        return Ok(output);
    }
    // Pass 1 drops a page that fails to parse and pass 2 never sees it:
    // surface that page's own failure.
    let failure = summary
        .pass1_failures
        .into_iter()
        .find(|f| f.input == active)
        .or_else(|| summary.pass2_failures.into_iter().next());
    Err(match failure {
        Some(f) => QuartoError::Parse(ParseError::new(
            f.diagnostics,
            f.source_context.unwrap_or_default(),
        )),
        None => QuartoError::Other(format!(
            "{} is not in the project's render set",
            active.display()
        )),
    })
}

/// [`render_chapter_alone`] with the whole project summary (failures and
/// project diagnostics included), for the orchestrator, whose report lists
/// them. `active` must be canonical. `format` is the page's own resolved
/// format; `project_format` is the project's, which Pass 1 runs under. The
/// engine registry is **not** shut down: the caller does that once.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) async fn render_chapter_alone_summary(
    project: &ProjectContext,
    active: &Path,
    format: &Format,
    project_format: &Format,
    options: &crate::render_to_file::RenderToFileOptions,
    runtime: std::sync::Arc<dyn quarto_system_runtime::SystemRuntime>,
) -> crate::Result<crate::project::orchestrator::ProjectRenderSummary> {
    use crate::project::orchestrator::{ProjectPipeline, RenderMode, project_type_for};

    let mut project = project.clone();
    prepare_page_in_book(&mut project, active, format);
    // The resolved key, not the base identifier: an extension format such as
    // `acm-typst` stays itself.
    let format_str = format.target_format.clone();
    let project_type = project_type_for(&project);
    let mut pipeline = ProjectPipeline::new(
        &mut project,
        project_type,
        format.clone(),
        format.identifier.to_string(),
        options,
        runtime,
    )
    .with_project_format(project_format.clone())
    .with_format_override(Some(format_str))
    .with_mode(RenderMode::ActivePage(active.to_path_buf()));
    pipeline.run_without_shutdown().await
}
