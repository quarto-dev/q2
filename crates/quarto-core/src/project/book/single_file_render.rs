/*
 * single_file_render.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * book-projects P2: `run_with_book_support()`'s single-file-merge branch.
 * Collects every chapter's post-Normalization AST, merges them into one
 * document, resolves cross-chapter links, applies citeproc once, and
 * runs the unmodified Crossref-onward pipeline over the merged whole —
 * see `claude-notes/plans/2026-09-21-book-projects-P2-single-file-merge.md`.
 */

use std::sync::Arc;

use quarto_error_reporting::DiagnosticMessage;
use quarto_pandoc_types::{ConfigValue, Pandoc};
use quarto_source_map::{By, SourceInfo};
use quarto_system_runtime::SystemRuntime;

use crate::Result;
use crate::artifact::ArtifactStore;
use crate::error::QuartoError;
use crate::format::{Format, FormatIdentifier};
use crate::pipeline::{
    ChapterPauseState, build_pandoc_pipeline_finishing_stages, render_qmd_to_ast_partial,
    run_pipeline_from_ast,
};
use crate::project::book::render_item::BookRenderItem;
use crate::project::{DocumentInfo, ProjectContext};
use crate::render::{BinaryDependencies, RenderContext};
use crate::render_to_file::{
    RenderToFileResult, apply_project_output_dir_to_options, determine_output_paths,
    finalize_rendered_output,
};
use crate::resource_resolver::ResourceResolverContext;
use crate::transform::TransformPhase;

fn generated_source_info() -> SourceInfo {
    SourceInfo::generated(By::programmatic_config())
}

/// Render one chapter through its own pipeline, paused after
/// Normalization — with citeproc deferred (Decision 1 of the plan): a
/// chapter that declares `filters: [citeproc]` must not build its own
/// separate bibliography before the merge, so this render's context
/// carries `defer_citeproc: true` and `UserFiltersStage::pre()` strips
/// `"citeproc"` from the chapter's resolved filters. The driver runs
/// citeproc once on the merged document instead.
///
/// Returns the chapter's paused `Pandoc` body, its extracted
/// [`ChapterPauseState`] (so the caller can merge artifacts/diagnostics
/// and carry the static-passthrough fields forward), and the paused
/// render's diagnostics.
#[allow(clippy::too_many_arguments)]
async fn render_chapter_paused(
    project: &ProjectContext,
    document: &DocumentInfo,
    format: &Format,
    binaries: &BinaryDependencies,
    content: &[u8],
    source_name: &str,
    runtime: Arc<dyn SystemRuntime>,
    pipeline_profile: crate::format::PipelineProfile,
    render_options: &crate::render_to_file::RenderToFileOptions,
) -> Result<(Pandoc, ChapterPauseState, Vec<DiagnosticMessage>)> {
    let mut state = ChapterPauseState::new(pipeline_profile);
    let mut ctx = state.build_context(project, document, format, binaries);
    ctx.defer_citeproc = true;
    // bd-sl79jjiq: thread the render's engine-registry override / execution
    // policy onto each chapter's context exactly like the single-document
    // path does (`render_qmd_to_html` from `HtmlRenderConfig`) — without
    // this a book render always executes every chapter with the default
    // registry, ignoring `q2 preview --static`/test overrides.
    ctx.engine_registry_override = render_options.engine_registry_override.clone();
    ctx.execution_policy = render_options.execution_policy.clone();

    let (paused, diagnostics) = render_qmd_to_ast_partial(
        content,
        source_name,
        &mut ctx,
        runtime,
        TransformPhase::Normalization,
    )
    .await?;

    state = ChapterPauseState::extract_from(&mut ctx);
    Ok((paused.ast, state, diagnostics))
}

/// Book-level fields carried forward from the first rendered chapter
/// into the merged document's own `RenderContext` (plan Decision 7: the
/// seven static-passthrough fields are project-level and never
/// chapter-mutated, so any one chapter's copy is the book-level value —
/// computed once here rather than re-derived per chapter).
struct BookLevelState {
    ref_type_registry: Option<crate::crossref::RefTypeRegistry>,
    options: crate::render::RenderOptions,
    includes: crate::stage::PandocIncludes,
    observer: Arc<dyn crate::stage::PipelineObserver>,
    user_grammar_provider:
        Option<Rc<std::cell::RefCell<dyn quarto_highlight::UserGrammarProvider>>>,
    resource_report: crate::project_resources::DocumentResourceReport,
    /// Base metadata for the merged document (Q1's
    /// `mergeExecutedFiles`: `safeCloneDeep(files[0].context)` — the
    /// first chapter's own already-fully-merged metadata, not a
    /// from-scratch project-metadata re-merge, since nothing downstream
    /// of the merge point re-runs `MetadataMergeStage`).
    meta: ConfigValue,
}

use std::rc::Rc;

impl BookLevelState {
    fn seed_from(chapter: &mut ChapterPauseState, meta: ConfigValue) -> Self {
        Self {
            ref_type_registry: chapter.ref_type_registry.take(),
            options: chapter.options.clone(),
            includes: std::mem::take(&mut chapter.includes),
            observer: chapter.observer.clone(),
            user_grammar_provider: chapter.user_grammar_provider.clone(),
            resource_report: std::mem::take(&mut chapter.resource_report),
            meta,
        }
    }
}

/// Render a book project's single-file-merge target (Typst, EPUB): the
/// per-chapter loop + merge + once-only Crossref-onward finishing pass.
///
/// Called from `run_with_book_support()`'s single-file-merge branch
/// *instead of* `pass_two()` — Pass 1 and `pre_render()` still run as
/// normal (plan Decision 2, corrected after reading `run_inner()`
/// directly: `pre_render` is where `book_render_items`/format defaults
/// get computed, and skipping it would leave every chapter's metadata
/// merge unprepared), so `book_items` here is `project.book_render_items`
/// as `pre_render` left it — not recomputed.
pub(crate) async fn render_book_single_file(
    project: &ProjectContext,
    book_items: &[BookRenderItem],
    format: &Format,
    runtime: Arc<dyn SystemRuntime>,
    project_artifacts: Option<&mut ArtifactStore>,
    render_options: &crate::render_to_file::RenderToFileOptions,
) -> Result<RenderToFileResult> {
    let binaries = BinaryDependencies::discover(runtime.as_ref());
    let pipeline_profile = crate::format::PipelineProfile::from_format(&format.target_format);

    let mut chapters: Vec<(BookRenderItem, Pandoc)> = Vec::with_capacity(book_items.len());
    let mut book_level: Option<BookLevelState> = None;
    let mut first_chapter_dir: Option<std::path::PathBuf> = None;
    let mut artifacts = ArtifactStore::default();
    let mut diagnostics: Vec<DiagnosticMessage> = Vec::new();
    // bd-sl79jjiq: book-level truth is "was ANY chapter's execution
    // skipped" — an accumulator, not a passthrough seeded once like the
    // `BookLevelState` fields (those are project-level and never
    // chapter-mutated; this is the opposite: chapter-mutated, needs OR).
    let mut execution_skipped = false;

    for item in book_items {
        let Some(rel_path) = &item.file else {
            // Part/appendix dividers carry no file; `merge_book_chapters`
            // handles them from `item.text` alone.
            chapters.push((
                item.clone(),
                Pandoc {
                    meta: ConfigValue::new_map(vec![], generated_source_info()),
                    blocks: vec![],
                },
            ));
            continue;
        };

        let input_path = project.dir.join(rel_path);
        let content = runtime.file_read(&input_path).map_err(|e| {
            QuartoError::other(format!(
                "Failed to read book chapter {}: {}",
                input_path.display(),
                e
            ))
        })?;
        let document = DocumentInfo::from_path(&input_path);

        let (chapter_pandoc, mut state, chapter_diagnostics) = render_chapter_paused(
            project,
            &document,
            format,
            &binaries,
            &content,
            &input_path.to_string_lossy(),
            runtime.clone(),
            pipeline_profile.clone(),
            render_options,
        )
        .await?;
        diagnostics.extend(chapter_diagnostics);
        execution_skipped |= state.execution_skipped;

        artifacts
            .merge_into_project(std::mem::take(&mut state.artifacts))
            .map_err(|e| {
                QuartoError::other(format!(
                    "Book-merge artifact conflict rendering {}: {}",
                    input_path.display(),
                    e
                ))
            })?;
        diagnostics.append(&mut state.diagnostics);

        if book_level.is_none() {
            book_level = Some(BookLevelState::seed_from(
                &mut state,
                chapter_pandoc.meta.clone(),
            ));
            // The merged document's meta is seeded from this chapter's
            // meta, so its marked `Path` values (`bibliography`, `csl`, …)
            // are rebased to *this* chapter's directory — that's the base
            // the deferred citeproc must resolve them against
            // (bd-oqoozmtr).
            first_chapter_dir = Some(
                input_path
                    .parent()
                    .map_or_else(|| std::path::PathBuf::from("."), |p| p.to_path_buf()),
            );
        }

        chapters.push((item.clone(), chapter_pandoc));
    }

    let book_level = book_level.ok_or_else(|| {
        QuartoError::other("Book project has no chapters with a file to render".to_string())
    })?;

    let book_config = project.config.metadata.as_ref().and_then(|m| m.get("book"));
    let mut merged =
        crate::project::book::merge_book_chapters(chapters, book_level.meta, book_config);
    // `apply_book_title_metadata` copies `book.author` (and friends) into
    // the merged meta *after* each chapter's own Normalization pass, so the
    // derived keys AuthorsNormalizeTransform would have produced from them
    // (`by-author`, `author-meta`, …) don't exist yet — and templates that
    // consume them (orange-book's `typst-show.typ` `$for(by-author)$`)
    // silently see nothing. Re-derive here; `normalize_authors_meta`
    // recomputes from raw `author` on every run, so the book config still
    // wins exactly as `apply_book_title_metadata` intends.
    for issue in crate::transforms::normalize_authors_meta(&mut merged.meta) {
        diagnostics.push(DiagnosticMessage::warning(issue));
    }
    crate::project::book::resolve_cross_chapter_links(&mut merged);

    // Plan Decision 4/6: the merge step is the first thing that knows
    // this render is a book single-file merge — hand that down to
    // `PandocWriteStage` (which registers `BookSingleFileContributor`)
    // and to `build_forwarded_args` (`top-level-division`) as plain
    // metadata, exactly like every other per-render flag this pipeline
    // already threads that way.
    merged.meta.insert_path(
        &["single-file-book"],
        ConfigValue::new_bool(true, generated_source_info()),
    );
    if matches!(
        format.identifier,
        FormatIdentifier::Typst | FormatIdentifier::Pdf
    ) {
        merged.meta.insert_path(
            &["top-level-division"],
            ConfigValue::new_string("chapter", generated_source_info()),
        );
    }
    // "orange-book generates its own" (Q1's `book.ts` `formatExtras`) —
    // Typst-only; EPUB's own toc default is untouched.
    if format.identifier == FormatIdentifier::Typst {
        merged.meta.insert_path(
            &["toc"],
            ConfigValue::new_bool(false, generated_source_info()),
        );
    }

    // Deferred citeproc (Decision 1): one call, on the assembled whole,
    // before the Crossref-onward pass — instead of once per chapter.
    // bd-oqoozmtr: resolve `bibliography`/`csl` against the first
    // file-chapter's directory — the directory the merged meta's marked
    // `Path` values were rebased to. `book_level` is seeded from the same
    // chapter, so the Option always has a value here.
    let citeproc_base_dir = first_chapter_dir.unwrap_or_else(|| project.dir.clone());
    let ast_context = pampa::pandoc::ASTContext::default();
    let (mut merged, _ast_context, citeproc_diagnostics) =
        pampa::citeproc_filter::apply_citeproc_filter(
            merged,
            ast_context,
            &format.target_format,
            &citeproc_base_dir,
        )
        .map_err(|e| QuartoError::other(format!("citeproc failed for merged book: {e}")))?;
    diagnostics.extend(citeproc_diagnostics);

    // The deferred citeproc just consumed `bibliography`/`csl`: every
    // citation is formatted and the bibliography Div is inserted. Left in
    // the merged meta, pandoc's Typst writer would *additionally* emit
    // native `#set bibliography(...)`/`#bibliography(...)` pointing at
    // doc-relative paths that don't exist under the book output dir —
    // a compile error plus a second, hayagriva-rendered bibliography.
    merged.meta.remove("bibliography");
    merged.meta.remove("csl");

    // Output path (Decision 5): `book_output_stem` + the same
    // output-dir-resolution policy every document uses, fed a synthetic
    // input path so `determine_output_paths` derives the same stem back
    // out without duplicating its fallback logic.
    let stem = super::config::book_output_stem(&project.dir, book_config);
    let synthetic_input = project.dir.join(format!("{stem}.qmd"));
    let options = crate::render_to_file::RenderToFileOptions::default();
    let effective_options =
        apply_project_output_dir_to_options(&options, project, &synthetic_input);
    let (output_path, output_dir, output_stem) =
        determine_output_paths(&synthetic_input, &format.target_format, &effective_options)?;
    runtime.dir_create(&output_dir, true).map_err(|e| {
        QuartoError::other(format!(
            "Failed to create output directory {}: {}",
            output_dir.display(),
            e
        ))
    })?;
    let resource_paths =
        crate::resources::prepare_html_resources(&output_dir, &output_stem, runtime.as_ref())?;

    let book_document = DocumentInfo::from_path(&synthetic_input).with_output(&output_path);
    let project_type = crate::project::orchestrator::project_type_for(project);
    let resolver = ResourceResolverContext::website(
        &project.output_dir,
        &output_path,
        project_type.lib_dir(),
        &output_stem,
    );

    let mut ctx = RenderContext::new(project, &book_document, format, &binaries);
    ctx.artifacts = artifacts;
    ctx.diagnostics = diagnostics;
    // Plan Decision 2/item 84: the merged document's own `RenderContext`
    // is built without a `ProjectIndex` — `LinkRewriteTransform` stays
    // inert as a defensive backstop.
    ctx.project_index = None;
    ctx.resource_resolver = Some(resolver.clone());
    ctx.ref_type_registry = book_level.ref_type_registry;
    ctx.options = book_level.options;
    ctx.includes = book_level.includes;
    ctx.observer = book_level.observer;
    ctx.user_grammar_provider = book_level.user_grammar_provider;
    ctx.resource_report = book_level.resource_report;

    let merged_doc = crate::stage::DocumentAst {
        path: synthetic_input.clone(),
        ast: merged,
        ast_context: pampa::pandoc::ASTContext::default(),
        // Known scope gap (not itemized in the P2 plan, and out of
        // scope for this landing): each chapter had its own
        // `SourceContext` from its own parse; there is no per-FileId
        // remapping merge here, so a diagnostic raised during the
        // Crossref-onward finishing pass against a *non-first* chapter
        // resolves its span against the wrong file. Does not affect
        // render correctness (crossref numbers/links/citations operate
        // on the AST directly) — only diagnostic *location* for
        // non-first chapters. Tracked for follow-up, not blocking here.
        source_context: quarto_source_map::SourceContext::new(),
        warnings: Vec::new(),
        recorded_includes: Vec::new(),
    };

    let finishing_stages =
        build_pandoc_pipeline_finishing_stages(TransformPhase::Crossref, format.identifier);
    let (finished, finishing_diagnostics) =
        run_pipeline_from_ast(merged_doc, &mut ctx, runtime.clone(), finishing_stages).await?;
    ctx.diagnostics.extend(finishing_diagnostics);
    let rendered = finished.into_rendered_output().ok_or_else(|| {
        QuartoError::Other(
            "Book single-file finishing pipeline did not produce RenderedOutput".to_string(),
        )
    })?;

    let render_output = crate::pipeline::RenderOutput {
        html: String::new(),
        diagnostics: ctx.diagnostics.clone(),
        source_context: rendered.source_context,
        // bd-sl79jjiq: true when ANY chapter's own partial render skipped a
        // code-executing engine excluded by the render's `ExecutionPolicy`
        // (accumulated above via `ChapterPauseState::execution_skipped`).
        execution_skipped,
    };

    finalize_rendered_output(
        &synthetic_input,
        output_path,
        resource_paths.resource_dir,
        render_output,
        &mut ctx,
        &resolver,
        project_type.as_ref(),
        project_artifacts,
        &runtime,
        false, // is_native: Pandoc-hybrid leg wrote output_path directly
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_project() -> ProjectContext {
        ProjectContext {
            dir: std::path::PathBuf::from("/project"),
            config: crate::project::ProjectConfig::default(),
            is_single_file: true,
            files: vec![DocumentInfo::from_path("/project/chapter.qmd")],
            output_dir: std::path::PathBuf::from("/project"),
            ..Default::default()
        }
    }

    fn make_test_runtime() -> Arc<dyn SystemRuntime> {
        Arc::new(quarto_system_runtime::NativeRuntime::new())
    }

    /// A chapter declaring `filters: [citeproc]` against a bibliography
    /// path that does not exist. If citeproc actually runs, the render
    /// fails (`load_bibliography`'s `BibliographyNotFound`) — so a
    /// **successful** `render_chapter_paused` is direct evidence
    /// citeproc never ran for this chapter, which is exactly the claim
    /// the deferral mechanism (Decision 1) makes.
    const CITEPROC_CHAPTER_QMD: &[u8] = b"---\n\
bibliography: /nonexistent/path/refs.json\n\
filters: [citeproc]\n\
---\n\n\
Some chapter text.\n";

    /// GREEN: with citeproc deferred (`render_chapter_paused` sets
    /// `ctx.defer_citeproc`), the render succeeds even though the
    /// declared bibliography file does not exist — because
    /// `UserFiltersStage::pre()` strips `"citeproc"` from the chapter's
    /// resolved filters, so `load_bibliography` is never called at all.
    #[test]
    fn citeproc_is_deferred_past_the_per_chapter_pause() {
        let project = make_test_project();
        let document = DocumentInfo::from_path("/project/chapter.qmd");
        let format = Format::from_format_string("typst").unwrap();
        let binaries = BinaryDependencies::new();
        let runtime = make_test_runtime();
        let pipeline_profile = crate::format::PipelineProfile::from_format(&format.target_format);

        let result = pollster::block_on(render_chapter_paused(
            &project,
            &document,
            &format,
            &binaries,
            CITEPROC_CHAPTER_QMD,
            "chapter.qmd",
            runtime,
            pipeline_profile,
            &crate::render_to_file::RenderToFileOptions::default(),
        ));

        assert!(
            result.is_ok(),
            "citeproc must not run per-chapter — a render that touched the \
             missing bibliography would fail, but got: {:?}",
            result.err()
        );
    }

    /// Negative control, proving the deferral flag is load-bearing and
    /// not vacuously true: the *naive* pause (the same
    /// `render_qmd_to_ast_partial(..=Normalization)` call but on a
    /// default context, without `defer_citeproc`) DOES run
    /// `UserFiltersStage::pre()` — and therefore DOES fail on the same
    /// missing bibliography.
    #[test]
    fn naive_one_shot_pause_runs_citeproc_and_fails_on_missing_bibliography() {
        let project = make_test_project();
        let document = DocumentInfo::from_path("/project/chapter.qmd");
        let format = Format::from_format_string("typst").unwrap();
        let binaries = BinaryDependencies::new();
        let runtime = make_test_runtime();
        let mut ctx = RenderContext::new(&project, &document, &format, &binaries);

        let result = pollster::block_on(render_qmd_to_ast_partial(
            CITEPROC_CHAPTER_QMD,
            "chapter.qmd",
            &mut ctx,
            runtime,
            TransformPhase::Normalization,
        ));

        assert!(
            result.is_err(),
            "the naive pause runs citeproc (UserFiltersStage::pre() precedes the \
             Normalization pause point), so without defer_citeproc it must fail on \
             the missing bibliography — a passing result here would mean this \
             negative control no longer exercises the deferral path"
        );
    }
}
