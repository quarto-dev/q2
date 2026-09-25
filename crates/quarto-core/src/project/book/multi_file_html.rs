/*
 * multi_file_html.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * book-projects P5: `run_with_book_support()`'s multi-file-HTML branch
 * (pass 3). Every chapter renders through the
 * Normalization → Crossref → Navigation transforms and pauses; the paused
 * inventories aggregate into one project-wide registry; each chapter then
 * resumes from Finalization with the registry in reach, so cross-chapter
 * `@ref`s resolve to the owning chapter's scoped number and a working
 * relative link. See
 * `claude-notes/plans/2026-09-21-book-projects-P5-crossref-registry.md`.
 */

use std::path::PathBuf;
use std::sync::Arc;

use pampa::citeproc_filter::ChapterCitationManifest;
use quarto_error_reporting::{DiagnosticMessage, DiagnosticMessageBuilder};
use quarto_source_map::FileId;
use quarto_system_runtime::SystemRuntime;

use crate::artifact::ArtifactStore;
use crate::crossref::index::CrossrefIndex;
use crate::crossref::project_index::{ChapterCrossrefInventory, aggregate_chapter_inventories};
use crate::error::QuartoError;
use crate::format::{
    Format, format_key_from_config_value, format_key_from_frontmatter, resolve_format_key,
};
use crate::pipeline::{
    BookChapterPauseState, RenderOutput, build_html_pipeline_finishing_stages,
    render_qmd_to_ast_partial, run_pipeline_from_ast,
};
use crate::project::book::render_item::{BookRenderItem, BookRenderItemKind};
use crate::project::book::{aggregate_chapter_citations, build_merged_bibliography};
use crate::project::index::ProjectIndex;
use crate::project::orchestrator::{FileFailure, file_failure_from_error};
use crate::project::{DocumentInfo, ProjectContext};
use crate::render::{BinaryDependencies, ChapterSeed};
use crate::render_to_file::{
    RenderToFileOptions, RenderToFileResult, apply_project_output_dir_to_options,
    determine_output_paths, finalize_rendered_output, render_document_to_file,
};
use crate::resource_resolver::ResourceResolverContext;
use crate::stage::DocumentAst;
use crate::transform::TransformPhase;

/// Everything one chapter's pause leg produced that the resume leg needs:
/// the extracted [`BookChapterPauseState`] (artifacts, diagnostics,
/// includes, resource report, …), the paused AST, and the per-chapter
/// paths/format resolved once here so both legs see identical output
/// locations.
struct HeldChapter {
    input_path: PathBuf,
    doc_info: DocumentInfo,
    state: BookChapterPauseState,
    paused: DocumentAst,
    render_format: Format,
    pause_diagnostics: Vec<DiagnosticMessage>,
    output_path: PathBuf,
    resources_dir: PathBuf,
    resolver: ResourceResolverContext,
    chapter_seed: Option<ChapterSeed>,
    /// Book-projects P6: whether this is the designated
    /// `BookRenderItemKind::References` chapter — the one whose paused AST
    /// gets the project-wide merged bibliography spliced in before resume.
    is_references: bool,
}

/// `Q-23-1`: a book chapter's `filters:` config places the built-in
/// `citeproc` filter into `.post` (after the `quarto` sentinel), so its
/// citations resolve after this chapter pauses for project-wide
/// crossref/bibliography aggregation — its references would otherwise
/// silently never reach the merged bibliography (book-projects P6, plan
/// "Decisions" — Bibliography, fifteenth pass).
fn citeproc_post_ordering_diagnostic(input_path: &std::path::Path) -> DiagnosticMessage {
    DiagnosticMessageBuilder::error("Citeproc Resolves After Book Merge Point")
        .with_code("Q-23-1")
        .problem(format!(
            "The book chapter `{}` orders the built-in `citeproc` filter into \
             `.post` (after the `quarto` sentinel in `filters:`), so its citations \
             resolve after this chapter pauses for project-wide crossref/bibliography \
             aggregation. Its references will not appear in this book's merged \
             bibliography.",
            input_path.display()
        ))
        .add_hint(
            "Move `citeproc` before the `quarto` sentinel in this chapter's (or the \
             project's) `filters:` list so it resolves in `.pre`.",
        )
        .build()
}

/// Render a book project to multi-file HTML with project-wide crossref
/// resolution: pause each chapter at `..=Navigation`, harvest its
/// crossref inventory, aggregate the registry, then resume each chapter
/// from `Finalization..` with the registry installed.
///
/// Called from `run_with_book_support()`'s Html branch *instead of*
/// `pass_two()` — Pass 1 and `pre_render()` still run as normal, so
/// `book_items` here is `project.book_render_items` as `pre_render` left
/// it and `project.files` holds chapters + additions (page-footer href
/// items, `404.qmd`). Non-item `project.files` render through plain
/// [`render_document_to_file`] — P4-identical treatment — as does any
/// chapter whose resolved format does not drive the native HTML pipeline.
///
/// Per-file failures do not abort the book: they collect into the
/// returned `FileFailure` list (mirroring `pass_two`), with `fail_fast`
/// stopping the loops early. Returns the outputs, the failures, and the
/// aggregation diagnostics (`Q-15-2` duplicate ids across chapters).
#[allow(clippy::too_many_arguments)]
pub(crate) async fn render_book_multi_file_html(
    project: &ProjectContext,
    book_items: &[BookRenderItem],
    format_str: &str,
    index: Arc<ProjectIndex>,
    runtime: Arc<dyn SystemRuntime>,
    mut project_artifacts: Option<&mut ArtifactStore>,
    render_options: &RenderToFileOptions,
    format_override: Option<&str>,
    fail_fast: bool,
) -> crate::Result<(
    Vec<RenderToFileResult>,
    Vec<FileFailure>,
    Vec<DiagnosticMessage>,
)> {
    let binaries = BinaryDependencies::discover(runtime.as_ref());
    let project_type = crate::project::orchestrator::project_type_for(project);
    let seeds = crate::project::book::render_item::chapter_seed_map(&project.dir, book_items);
    // Per-document format resolution reads the project-level `format:`
    // declarations once; each chapter's front matter and the `--to`
    // override layer on top (the same prefer-merge
    // `render_document_to_file` applies).
    let project_format = project
        .config
        .metadata
        .as_ref()
        .and_then(|m| m.get("format"))
        .and_then(format_key_from_config_value);

    let mut held: Vec<HeldChapter> = Vec::with_capacity(book_items.len());
    let mut inventories: Vec<ChapterCrossrefInventory> = Vec::with_capacity(book_items.len());
    let mut citation_manifests: Vec<ChapterCitationManifest> = Vec::with_capacity(book_items.len());
    let mut outputs: Vec<RenderToFileResult> = Vec::new();
    let mut failures: Vec<FileFailure> = Vec::new();

    // ── Pause leg: every chapter through ..=Navigation ──────────────
    for item in book_items {
        let Some(rel_path) = &item.file else {
            continue;
        };
        let input_path = project.dir.join(rel_path);
        let content = match runtime.file_read(&input_path) {
            Ok(c) => c,
            Err(e) => {
                failures.push(file_failure_from_error(
                    input_path.clone(),
                    QuartoError::other(format!(
                        "Failed to read book chapter {}: {}",
                        input_path.display(),
                        e
                    )),
                ));
                if fail_fast {
                    break;
                }
                continue;
            }
        };
        // `chapter_seed_map` keys are canonicalized paths; match that
        // spelling for the lookup.
        let seed_key = input_path
            .canonicalize()
            .unwrap_or_else(|_| input_path.clone());
        let chapter_seed = seeds.get(&seed_key).copied();
        let doc_info = DocumentInfo::from_path(&input_path);
        // Book-projects P6: only the designated references chapter keeps
        // its own per-chapter citeproc-generated bibliography div; every
        // other chapter gets `suppress-bibliography: true` so it does not
        // also grow its own duplicate, unmerged local bibliography.
        let is_references = item.kind == BookRenderItemKind::References;

        let document_format = std::str::from_utf8(&content)
            .ok()
            .and_then(format_key_from_frontmatter);
        let resolved_format = resolve_format_key(
            project_format.as_deref(),
            document_format.as_deref(),
            format_override,
            format_str,
        );
        let render_format =
            Format::from_format_string(&resolved_format).map_err(QuartoError::other)?;

        // Chapters whose resolved format does not drive the native HTML
        // pipeline cannot pause/resume through the HTML stage list —
        // render them exactly as P4's pass_two did.
        if !render_format.identifier.is_native() {
            let mut doc_options = render_options.clone();
            doc_options.chapter_seed = chapter_seed;
            match render_document_to_file(
                &input_path,
                format_str,
                &doc_options,
                Some(project),
                runtime.clone(),
                Some(index.clone()),
                project_artifacts.as_deref_mut(),
                format_override,
            ) {
                Ok(r) => outputs.push(r),
                Err(e) => failures.push(file_failure_from_error(input_path.clone(), e)),
            }
            if fail_fast && !failures.is_empty() {
                break;
            }
            continue;
        }

        let effective_options =
            apply_project_output_dir_to_options(render_options, project, &input_path);
        let (output_path, output_dir, output_stem) =
            match determine_output_paths(&input_path, &resolved_format, &effective_options) {
                Ok(p) => p,
                Err(e) => {
                    failures.push(file_failure_from_error(input_path.clone(), e));
                    if fail_fast {
                        break;
                    }
                    continue;
                }
            };
        if let Err(e) = runtime.dir_create(&output_dir, true) {
            failures.push(file_failure_from_error(
                input_path.clone(),
                QuartoError::other(format!(
                    "Failed to create output directory {}: {}",
                    output_dir.display(),
                    e
                )),
            ));
            if fail_fast {
                break;
            }
            continue;
        }
        let resource_paths = match crate::resources::prepare_html_resources(
            &output_dir,
            &output_stem,
            runtime.as_ref(),
        ) {
            Ok(p) => p,
            Err(e) => {
                failures.push(file_failure_from_error(input_path.clone(), e));
                if fail_fast {
                    break;
                }
                continue;
            }
        };
        let output_href = output_path
            .strip_prefix(&project.output_dir)
            .unwrap_or(&output_path)
            .to_string_lossy()
            .replace('\\', "/");
        let resolver = ResourceResolverContext::website(
            &project.output_dir,
            &output_path,
            project_type.lib_dir(),
            &output_stem,
        );

        let pipeline_profile =
            crate::format::PipelineProfile::from_format(&render_format.target_format);
        let mut state = BookChapterPauseState::new(pipeline_profile);
        let mut ctx = state.build_context(project, &doc_info, &render_format, &binaries);
        if let Some(seed) = chapter_seed {
            ctx = ctx.with_chapter_seed(seed);
        }
        ctx.project_index = Some(index.clone());
        ctx.resource_resolver = Some(resolver.clone());
        ctx.suppress_book_bibliography = !is_references;
        // bd-sl79jjiq: thread the render's engine-registry override /
        // execution policy onto each chapter's context exactly like the
        // single-document path does.
        ctx.engine_registry_override = if let Some(reg) = &render_options.engine_registry_override {
            Some(reg.clone())
        } else if !render_options.replay_captures.is_empty() {
            Some(Arc::new(crate::engine::EngineRegistry::with_replay_many(
                render_options.replay_captures.clone(),
            )))
        } else {
            None
        };
        ctx.execution_policy = render_options.execution_policy.clone();
        if matches!(
            render_options.attribution,
            Some(crate::attribution::AttributionMode::Git)
        ) {
            ctx.attribution_provider = Some(Arc::new(crate::attribution::GitBlameProvider::new()));
        }

        let (paused_ast, mut pause_diagnostics) = match render_qmd_to_ast_partial(
            &content,
            &input_path.to_string_lossy(),
            &mut ctx,
            runtime.clone(),
            TransformPhase::Navigation,
        )
        .await
        {
            Ok(v) => v,
            Err(e) => {
                failures.push(file_failure_from_error(input_path.clone(), e));
                if fail_fast {
                    break;
                }
                continue;
            }
        };
        let state = BookChapterPauseState::extract_from(&mut ctx);

        // Book-projects P6: a chapter whose `filters:` placed `citeproc`
        // into `.post` never captured a citation manifest — diagnose it by
        // name rather than silently dropping its references from the
        // merged bibliography.
        if state.citeproc_filter_in_post {
            pause_diagnostics.push(citeproc_post_ordering_diagnostic(&input_path));
        }
        if let Some(manifest) = state.citation_manifest.clone() {
            citation_manifests.push(manifest);
        }

        // Harvest the chapter's crossref inventory at the pause point.
        // Clone, not take: the resume leg's `CrossrefRenderTransform` needs
        // the chapter's own index (carried via `build_context`) for the
        // chapter's local refs.
        let chapter_index = state
            .crossref_index
            .clone()
            .unwrap_or_else(|| CrossrefIndex::new(FileId(0)));
        inventories.push(ChapterCrossrefInventory {
            index: chapter_index,
            chapter_seed,
            output_href: output_href.clone(),
        });

        held.push(HeldChapter {
            input_path: input_path.clone(),
            doc_info,
            state,
            paused: paused_ast,
            render_format,
            pause_diagnostics,
            output_path,
            resources_dir: resource_paths.resource_dir,
            resolver,
            chapter_seed,
            is_references,
        });
        if fail_fast && !failures.is_empty() {
            break;
        }
    }

    // ── Aggregate: one project-wide registry from every chapter ─────
    let (registry_index, aggregate_diagnostics) = aggregate_chapter_inventories(&inventories);
    let registry = Arc::new(registry_index);

    // ── Bibliography aggregate: one merged, deduplicated bibliography,
    // spliced into the designated references chapter's paused AST before
    // it resumes (book-projects P6). No-op when no chapter cited anything,
    // or when the book declares no `book.references` chapter.
    if !citation_manifests.is_empty()
        && let Some(references_chapter) = held.iter_mut().find(|c| c.is_references)
    {
        let (cited_order, references) = aggregate_chapter_citations(&citation_manifests);
        let config = pampa::citeproc_filter::extract_config(&references_chapter.paused.ast);
        let base_dir = references_chapter
            .input_path
            .parent()
            .map_or_else(|| project.dir.clone(), |p| p.to_path_buf());
        match build_merged_bibliography(&config, &base_dir, &cited_order, references) {
            Ok(bib_blocks) => {
                pampa::citeproc_filter::insert_bibliography(
                    &mut references_chapter.paused.ast.blocks,
                    bib_blocks,
                );
            }
            Err(e) => {
                failures.push(file_failure_from_error(
                    references_chapter.input_path.clone(),
                    e,
                ));
                if fail_fast {
                    return Ok((outputs, failures, aggregate_diagnostics));
                }
            }
        }
    }

    // ── Resume leg: every held chapter from Finalization.. ──────────
    for chapter in held {
        let mut state = chapter.state;
        let mut ctx = state.build_context(
            project,
            &chapter.doc_info,
            &chapter.render_format,
            &binaries,
        );
        ctx.diagnostics.extend(chapter.pause_diagnostics);
        // The seed does not ride `BookChapterPauseState` (it is input-only
        // to the pause leg's numbering transforms) — re-apply it so the
        // Finalization display path composes the same scoped numbers.
        if let Some(seed) = chapter.chapter_seed {
            ctx = ctx.with_chapter_seed(seed);
        }
        ctx = ctx.with_cross_chapter_crossref_registry(registry.clone());

        let finishing_stages = build_html_pipeline_finishing_stages(TransformPhase::Finalization);
        let (finished, finishing_diagnostics) = match run_pipeline_from_ast(
            chapter.paused,
            &mut ctx,
            runtime.clone(),
            finishing_stages,
        )
        .await
        {
            Ok(v) => v,
            Err(e) => {
                failures.push(file_failure_from_error(chapter.input_path.clone(), e));
                if fail_fast {
                    break;
                }
                continue;
            }
        };
        ctx.diagnostics.extend(finishing_diagnostics);
        let rendered = match finished.into_rendered_output() {
            Some(r) => r,
            None => {
                failures.push(file_failure_from_error(
                    chapter.input_path.clone(),
                    QuartoError::Other(
                        "Book chapter finishing pipeline did not produce RenderedOutput"
                            .to_string(),
                    ),
                ));
                if fail_fast {
                    break;
                }
                continue;
            }
        };
        let render_output = RenderOutput {
            html: rendered.content,
            diagnostics: ctx.diagnostics.clone(),
            source_context: rendered.source_context,
            // No engine stage runs in Finalization, so the resume leg
            // never sets this; the pause leg's flag is the true value.
            execution_skipped: state.execution_skipped || ctx.execution_skipped,
        };
        match finalize_rendered_output(
            &chapter.input_path,
            chapter.output_path.clone(),
            chapter.resources_dir.clone(),
            render_output,
            &mut ctx,
            &chapter.resolver,
            project_type.as_ref(),
            project_artifacts.as_deref_mut(),
            &runtime,
            true,
        ) {
            Ok(r) => outputs.push(r),
            Err(e) => {
                failures.push(file_failure_from_error(chapter.input_path.clone(), e));
                if fail_fast {
                    break;
                }
            }
        }
        if fail_fast && !failures.is_empty() {
            break;
        }
    }

    // ── Non-item project files (additions: footer pages, 404) ───────
    // `pre_render` restricted `project.files` to chapter files plus
    // additions; anything not a book item renders P4-style, with no
    // registry involvement.
    let item_inputs: std::collections::HashSet<PathBuf> = book_items
        .iter()
        .filter_map(|item| item.file.as_ref())
        .map(|f| {
            let p = project.dir.join(f);
            p.canonicalize().unwrap_or(p)
        })
        .collect();
    for doc in &project.files {
        let key = doc
            .input
            .canonicalize()
            .unwrap_or_else(|_| doc.input.clone());
        if item_inputs.contains(&key) {
            continue;
        }
        match render_document_to_file(
            &doc.input,
            format_str,
            render_options,
            Some(project),
            runtime.clone(),
            Some(index.clone()),
            project_artifacts.as_deref_mut(),
            format_override,
        ) {
            Ok(r) => outputs.push(r),
            Err(e) => failures.push(file_failure_from_error(doc.input.clone(), e)),
        }
        if fail_fast && !failures.is_empty() {
            break;
        }
    }

    Ok((outputs, failures, aggregate_diagnostics))
}
