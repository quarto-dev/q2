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
#[cfg(not(target_arch = "wasm32"))]
use crate::pipeline::build_pandoc_pipeline_finishing_stages;
use crate::pipeline::{
    ChapterPauseState, PartialKind, render_qmd_to_ast_partial, run_pipeline_from_ast,
};
use crate::project::book::core_types::{BookRenderOptions, ChapterDiagnostics};
use crate::project::book::render_item::BookRenderItem;
use crate::project::{DocumentInfo, ProjectContext};
use crate::render::{BinaryDependencies, RenderContext};
#[cfg(not(target_arch = "wasm32"))]
use crate::render_to_file::{
    RenderToFileResult, apply_project_output_dir_to_options, determine_output_paths,
    finalize_rendered_output,
};
use crate::resource_resolver::ResourceResolverContext;
use crate::transform::TransformPhase;

fn generated_source_info() -> SourceInfo {
    SourceInfo::generated(By::programmatic_config())
}

/// Rebase `meta["bibliography"]`/`meta["csl"]` entries from
/// `base_dir`-relative (their declaration site) to `root_dir`-relative
/// with a leading `/`, matching `typst_root_relative`'s convention
/// (`modules/mediabag.lua`). Only touches entries that are local files:
/// URLs are left verbatim, and an entry that doesn't exist as a file
/// under `base_dir` (e.g. a built-in CSL style name like `apa`) is left
/// verbatim too, mirroring `format_paths::mark_entry`'s
/// `ExistenceSilent` policy. Handles both the scalar and array forms
/// `bibliography`/`csl` accept.
fn rebase_typst_bibliography_paths(
    meta: &mut ConfigValue,
    base_dir: &std::path::Path,
    root_dir: &std::path::Path,
    runtime: &dyn SystemRuntime,
) {
    for key in ["bibliography", "csl"] {
        let Some(value) = meta.get_mut(key) else {
            continue;
        };
        match &mut value.value {
            quarto_pandoc_types::config_value::ConfigValueKind::Array(items) => {
                for item in items {
                    rebase_one_typst_path(item, base_dir, root_dir, runtime);
                }
            }
            _ => rebase_one_typst_path(value, base_dir, root_dir, runtime),
        }
    }
}

fn rebase_one_typst_path(
    entry: &mut ConfigValue,
    base_dir: &std::path::Path,
    root_dir: &std::path::Path,
    runtime: &dyn SystemRuntime,
) {
    let Some(declared) = entry.as_plain_text() else {
        return;
    };
    if quarto_util::is_external_url(&declared) {
        return;
    }
    let source = base_dir.join(&declared);
    if !runtime.is_file(&source).unwrap_or(false) {
        return;
    }
    let relative = pathdiff::diff_paths(&source, root_dir).unwrap_or(source);
    let rooted = format!("/{}", quarto_util::to_forward_slashes(&relative));
    *entry = ConfigValue::new_string(rooted, entry.source_info.clone());
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
/// and carry the static-passthrough fields forward), the paused render's
/// diagnostics, and the chapter's own `SourceContext` (the diagnostics are
/// located against it; the merged document has none that resolves them).
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
    render_options: &BookRenderOptions,
    partial_kind: PartialKind,
) -> Result<(
    Pandoc,
    ChapterPauseState,
    Vec<DiagnosticMessage>,
    quarto_source_map::SourceContext,
)> {
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
        partial_kind,
    )
    .await?;

    state = ChapterPauseState::extract_from(&mut ctx);
    Ok((paused.ast, state, diagnostics, paused.source_context))
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

/// What the caller's `setup` produces for the merged document: its synthetic
/// input path (the project root plus the book's output stem, so its document
/// directory is the project root, as natively), its output path, and the
/// resource resolver the merged document's own Finalization pass rewrites
/// links and images against. Native roots the resolver at `_book/`; the
/// browser request roots it at the project directory.
pub(crate) struct BookSetup {
    pub synthetic_input: std::path::PathBuf,
    pub output_path: std::path::PathBuf,
    pub resolver: ResourceResolverContext,
}

/// What the core knows once the finishing pass has run, handed to `finish`
/// and returned alongside its value.
pub(crate) struct BookTail {
    /// The finishing pass's `SourceContext` (the merged document's).
    pub source_context: quarto_source_map::SourceContext,
    /// Diagnostics of the book-level steps: author normalization, the
    /// Crossref phase, citeproc and the finishing pass. Unlocatable: the
    /// merged document carries an empty `SourceContext`.
    pub book_diagnostics: Vec<DiagnosticMessage>,
    /// Each chapter's diagnostics with that chapter's `SourceContext`, in
    /// book order (chapters with none are absent).
    pub chapter_diagnostics: Vec<ChapterDiagnostics>,
    /// True when ANY chapter's execution was skipped.
    pub execution_skipped: bool,
    /// Code cells left without a result, summed over every chapter.
    pub unexecuted_cells: usize,
}

impl BookTail {
    /// Every diagnostic in the order native reports them: each chapter's
    /// (in book order), then the book-level ones.
    pub(crate) fn all_diagnostics(&self) -> Vec<DiagnosticMessage> {
        self.chapter_diagnostics
            .iter()
            .flat_map(|c| c.diagnostics.iter().cloned())
            .chain(self.book_diagnostics.iter().cloned())
            .collect()
    }
}

pub(crate) enum BookCoreOutcome<T> {
    Done {
        value: T,
        tail: BookTail,
    },
    /// A [`BookRenderHooks`] call returned `Cancelled` before a chapter.
    Cancelled,
    /// A chapter's own stages returned `Err`; `file` is its project-relative
    /// path. (A chapter that finished `Ok` with an error-severity diagnostic
    /// is not a failure here: native keeps going, so the request tail scans.)
    ChapterFailed {
        file: String,
        error: QuartoError,
    },
}

/// What differs between callers of [`render_book_core`].
pub(crate) struct BookCoreArgs<'a> {
    pub project: &'a ProjectContext,
    pub book_items: &'a [BookRenderItem],
    pub format: &'a Format,
    pub runtime: Arc<dyn SystemRuntime>,
    pub binaries: &'a BinaryDependencies,
    pub options: &'a BookRenderOptions,
    /// The finishing list run over the merged document, from `Navigation`
    /// (native: the writer tail; browser: the request tail).
    pub finishing_stages: Vec<Box<dyn crate::stage::PipelineStage>>,
    /// Set on the merged document's context only (never a chapter's).
    pub prepare_options: Option<crate::pandoc_request::PrepareOptions>,
    pub hooks: Option<&'a dyn crate::project::book::BookRenderHooks>,
}

/// The shared core of the single-file-merge book render: the per-chapter
/// loop, the merge, link and image resolution, the book metadata, the
/// Crossref-only pass, citeproc and the finishing pass, up to and including
/// `finishing_stages`. Native `q2 render` wraps it with a tail that writes
/// files ([`render_book_single_file`]); the browser's whole-book request
/// wraps it with a tail that takes `ctx.pandoc_request`.
///
/// `chapter_kind` is asked once per file-bearing chapter, just before its
/// render, for the [`PartialKind`] (native: the native lists; browser: the
/// request lists with that chapter's own captures, parsed there and dropped
/// with the chapter) and any diagnostics raised choosing it.
///
/// `setup` runs after the chapter loop and the no-chapters error (so a
/// failing native render does not change when `_book/` is created) and
/// returns the [`BookSetup`] plus a caller state `S` that is handed to
/// `finish`, which runs with the merged context after the finishing pass.
/// The merged context borrows a `book_document` built inside this function,
/// so it never leaves it: the tail's work happens inside `finish`.
pub(crate) async fn render_book_core<S, T>(
    args: BookCoreArgs<'_>,
    chapter_kind: &mut dyn FnMut(&BookRenderItem) -> (PartialKind, Vec<DiagnosticMessage>),
    setup: impl FnOnce(&str) -> Result<(BookSetup, S)>,
    finish: impl for<'c> FnOnce(&mut RenderContext<'c>, S, &BookSetup, &BookTail) -> Result<T>,
) -> Result<BookCoreOutcome<T>> {
    let BookCoreArgs {
        project,
        book_items,
        format,
        runtime,
        binaries,
        options,
        finishing_stages,
        prepare_options,
        hooks,
    } = args;
    let pipeline_profile = crate::format::PipelineProfile::from_format(&format.target_format);

    let mut chapters: Vec<(BookRenderItem, Pandoc)> = Vec::with_capacity(book_items.len());
    let mut book_level: Option<BookLevelState> = None;
    let mut first_chapter_dir: Option<std::path::PathBuf> = None;
    let mut artifacts = ArtifactStore::default();
    // The book-level diagnostics; each chapter's go in `chapter_diagnostics`
    // with their own `SourceContext`.
    let mut diagnostics: Vec<DiagnosticMessage> = Vec::new();
    let mut chapter_diagnostics: Vec<ChapterDiagnostics> = Vec::new();
    // bd-sl79jjiq: book-level truth is "was ANY chapter's execution
    // skipped" — an accumulator, not a passthrough seeded once like the
    // `BookLevelState` fields (those are project-level and never
    // chapter-mutated; this is the opposite: chapter-mutated, needs OR).
    let mut execution_skipped = false;
    // `restore_render_context` resets the context's count on every pipeline
    // run, so the merged context's final value is not the sum: accumulate.
    let mut unexecuted_cells = 0usize;

    let total = book_items.iter().filter(|i| i.file.is_some()).count();
    let mut index = 0usize;

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
        index += 1;
        let file_label = rel_path.to_string_lossy().to_string();
        if let Some(hooks) = hooks
            && hooks
                .before_chapter(index, total, &file_label)
                .await
                .is_err()
        {
            return Ok(BookCoreOutcome::Cancelled);
        }

        let input_path = project.dir.join(rel_path);
        let content = runtime.file_read(&input_path).map_err(|e| {
            QuartoError::other(format!(
                "Failed to read book chapter {}: {}",
                input_path.display(),
                e
            ))
        })?;
        let document = DocumentInfo::from_path(&input_path);

        let (partial_kind, kind_diagnostics) = chapter_kind(item);
        let (chapter_pandoc, mut state, partial_diagnostics, chapter_source_context) =
            match render_chapter_paused(
                project,
                &document,
                format,
                binaries,
                &content,
                &input_path.to_string_lossy(),
                runtime.clone(),
                pipeline_profile.clone(),
                options,
                partial_kind,
            )
            .await
            {
                Ok(ok) => ok,
                Err(error) => {
                    return Ok(BookCoreOutcome::ChapterFailed {
                        file: file_label,
                        error,
                    });
                }
            };
        execution_skipped |= state.execution_skipped;
        unexecuted_cells += state.unexecuted_cells;

        artifacts
            .merge_into_project(std::mem::take(&mut state.artifacts))
            .map_err(|e| {
                QuartoError::other(format!(
                    "Book-merge artifact conflict rendering {}: {}",
                    input_path.display(),
                    e
                ))
            })?;
        let mut this_chapter = kind_diagnostics;
        this_chapter.extend(partial_diagnostics);
        this_chapter.append(&mut state.diagnostics);
        if !this_chapter.is_empty() {
            chapter_diagnostics.push(ChapterDiagnostics {
                file: file_label,
                diagnostics: this_chapter,
                source_context: chapter_source_context,
            });
        }

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
    crate::project::book::resolve_chapter_image_targets(&mut merged);

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

    // Deferred citeproc (Decision 1, revised — see the note at the
    // Crossref-phase split below): the actual `apply_citeproc_filter` call
    // now happens *after* the merged document's Crossref phase runs, not
    // here. `citeproc_base_dir` is captured here (bd-oqoozmtr: resolve
    // `bibliography`/`csl` against the first file-chapter's directory —
    // the directory the merged meta's marked `Path` values were rebased
    // to; `book_level` is seeded from the same chapter, so the Option
    // always has a value) for use down there.
    let citeproc_base_dir = first_chapter_dir.unwrap_or_else(|| project.dir.clone());

    // Derive Q2's `epub-cover-image` key from `book.cover-image` (EPUB
    // output only, and only when the author set no explicit
    // `epub-cover-image`). Q1's `epubBookExtension.onSingleFilePreRender`
    // instead writes a generic `cover-image` metadata key that Pandoc's
    // EPUB writer reads natively; Q2 forwards only the literally-named
    // `epub-cover-image` key (`pandoc_write.rs`'s `epub_extra_args`), so
    // this is a deliberate adaptation to Q2's architecture, not a port.
    // The merged doc's synthetic input sits at the project root, so its
    // document directory *is* the project root — the project-root-relative
    // `book.cover-image` value (a leading `/` in config context means
    // project root, never the filesystem root) carries over after stripping
    // that prefix, exactly as the path-resolution contract requires.
    if format.identifier == FormatIdentifier::Epub {
        let book_cover = book_config
            .and_then(|b| b.get("cover-image"))
            .and_then(|v| v.as_plain_text());
        if let Some(cover) = book_cover
            && merged.meta.get("epub-cover-image").is_none()
        {
            let declared = cover.strip_prefix('/').unwrap_or(&cover);
            merged.meta.insert_path(
                &["epub-cover-image"],
                ConfigValue::new_string(declared, SourceInfo::generated(By::programmatic_config())),
            );
        }
    }

    // Output path (Decision 5): `book_output_stem` is the merged document's
    // name; the caller's `setup` turns it into the output path and resolver
    // (native: `determine_output_paths` and `_book/`; browser: no file
    // effect).
    let stem = super::config::book_output_stem(&project.dir, book_config);
    let (book_setup, caller_state) = setup(&stem)?;
    let synthetic_input = book_setup.synthetic_input.clone();
    let book_document =
        DocumentInfo::from_path(&synthetic_input).with_output(&book_setup.output_path);
    let resolver = book_setup.resolver.clone();

    let mut ctx = RenderContext::new(project, &book_document, format, binaries);
    ctx.artifacts = artifacts;
    ctx.diagnostics = diagnostics;
    // Plan Decision 2/item 84: the merged document's own `RenderContext`
    // is built without a `ProjectIndex` — `LinkRewriteTransform` stays
    // inert as a defensive backstop.
    ctx.project_index = None;
    ctx.resource_resolver = Some(resolver);
    ctx.ref_type_registry = book_level.ref_type_registry;
    ctx.options = book_level.options;
    ctx.includes = book_level.includes;
    ctx.observer = book_level.observer;
    ctx.user_grammar_provider = book_level.user_grammar_provider;
    ctx.resource_report = book_level.resource_report;
    ctx.prepare_options = prepare_options;

    let merged_doc = crate::stage::DocumentAst {
        path: synthetic_input,
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

    // Run the merged document's Crossref phase (`crossref-index` +
    // `crossref-resolve`, *not* `crossref-render` — see
    // `TransformPhase::Crossref`'s doc comment) on its own, before
    // citeproc. In the single-document pipeline, citeproc resolves into
    // the `.post` filter bucket by default (`filter_resolve.rs`'s
    // `"citeproc" into .post`) and so always runs *after*
    // `AstTransformsStage` — after Crossref has already reclassified
    // reserved-prefix keys (`@sec-...`, `@fig-...`, `@tbl-...`, …) out of
    // plain `Inline::Cite` nodes. `apply_citeproc_filter` has no such
    // awareness (it treats every `Inline::Cite` as bibliographic), so
    // calling it *before* Crossref runs — the original Decision 1
    // mechanism — makes it try to look up crossref-only labels in the
    // bibliography and fail loudly (`Citation processing error: Reference
    // '<label>' not found`) the moment a book mixes citeproc with numbered
    // crossrefs in the same document. Splitting the Crossref phase out
    // here and running citeproc immediately after restores the
    // single-document ordering for the merged whole. See the P8 plan
    // (`claude-notes/plans/2026-09-27-typst-smoke-all-epic-P8-orange-book-base.md`)
    // for the fixture that surfaced this.
    let crossref_stages: Vec<Box<dyn crate::stage::PipelineStage>> =
        vec![Box::new(crate::stage::AstTransformsStage::for_range(
            TransformPhase::Crossref..=TransformPhase::Crossref,
        ))];
    let (crossref_result, crossref_diagnostics) =
        run_pipeline_from_ast(merged_doc, &mut ctx, runtime.clone(), crossref_stages).await?;
    ctx.diagnostics.extend(crossref_diagnostics);
    let mut crossref_doc = crossref_result.into_document_ast().ok_or_else(|| {
        QuartoError::Other(
            "Book single-file Crossref-phase pipeline did not produce DocumentAst".to_string(),
        )
    })?;

    // `citation-location: margin` needs every `Inline::Cite` node left
    // unresolved: pandoc's native Typst writer then emits `#cite`/`@key`
    // citations, and `quarto-post/typst.lua`'s existing Pass 0 (Cite
    // handler + `marginCitations()`) builds the margin note text itself —
    // exactly as it already does for single-document renders. Running
    // `apply_citeproc_filter` here would resolve every `Cite` node before
    // that Lua filter ever runs, which is the P9 orange-book-margin bug.
    // Leaving `meta.bibliography`/`csl` intact (skipping the removal below)
    // also lets the Typst template's own `$if(bibliography)$` block
    // (`biblio.typ`) emit `#bibliography(...)`, suppressed from display
    // via `suppress-bibliography` when set — no template change needed.
    let margin_citations = crossref_doc
        .ast
        .meta
        .get("citation-location")
        .and_then(|value| value.as_plain_text())
        .is_some_and(|location| location == "margin");

    let post_citeproc_doc = if margin_citations {
        // The declared `bibliography`/`csl` paths are relative to
        // `citeproc_base_dir` (the chapter directory they were declared
        // in), but the compiled `.typ` lives under `project.dir`'s
        // `_book/` — a different directory. Typst embeds this path as
        // literal source text and resolves it against `typst compile
        // --root <project.dir>` (`typst_compile.rs:204`), where a
        // leading `/` means root-relative, not filesystem-absolute. This
        // mirrors `modules/mediabag.lua`'s `typst_root_relative`, the
        // existing convention for the same problem with image paths;
        // there's no Lua-side equivalent for `bibliography`/`csl` since
        // Lua's Meta filter never sees `citeproc_base_dir`.
        rebase_typst_bibliography_paths(
            &mut crossref_doc.ast.meta,
            &citeproc_base_dir,
            &project.dir,
            runtime.as_ref(),
        );
        crossref_doc
    } else {
        let ast_context = pampa::pandoc::ASTContext::default();
        let (mut citeproc_ast, _ast_context, citeproc_diagnostics, _citation_manifest) =
            pampa::citeproc_filter::apply_citeproc_filter(
                crossref_doc.ast,
                ast_context,
                &format.target_format,
                &citeproc_base_dir,
                runtime.as_ref(),
            )
            .map_err(|e| QuartoError::other(format!("citeproc failed for merged book: {e}")))?;
        ctx.diagnostics.extend(citeproc_diagnostics);

        // The deferred citeproc just consumed `bibliography`/`csl`: every
        // citation is formatted and the bibliography Div is inserted. Left in
        // the merged meta, pandoc's Typst writer would *additionally* emit
        // native `#set bibliography(...)`/`#bibliography(...)` pointing at
        // doc-relative paths that don't exist under the book output dir —
        // a compile error plus a second, hayagriva-rendered bibliography.
        citeproc_ast.meta.remove("bibliography");
        citeproc_ast.meta.remove("csl");

        crate::stage::DocumentAst {
            path: crossref_doc.path,
            ast: citeproc_ast,
            ast_context: crossref_doc.ast_context,
            source_context: crossref_doc.source_context,
            warnings: crossref_doc.warnings,
            recorded_includes: crossref_doc.recorded_includes,
        }
    };

    let (finished, finishing_diagnostics) = run_pipeline_from_ast(
        post_citeproc_doc,
        &mut ctx,
        runtime.clone(),
        finishing_stages,
    )
    .await?;
    ctx.diagnostics.extend(finishing_diagnostics);
    let rendered = finished.into_rendered_output().ok_or_else(|| {
        QuartoError::Other(
            "Book single-file finishing pipeline did not produce RenderedOutput".to_string(),
        )
    })?;

    let tail = BookTail {
        source_context: rendered.source_context,
        book_diagnostics: std::mem::take(&mut ctx.diagnostics),
        chapter_diagnostics,
        execution_skipped,
        unexecuted_cells,
    };
    let value = finish(&mut ctx, caller_state, &book_setup, &tail)?;
    Ok(BookCoreOutcome::Done { value, tail })
}

/// Native `q2 render`'s single-file-merge book render: the shared core plus
/// a tail that creates `_book/`, writes the output files and the resource
/// copies (`finalize_rendered_output`).
#[cfg(not(target_arch = "wasm32"))]
pub(crate) async fn render_book_single_file(
    project: &ProjectContext,
    book_items: &[BookRenderItem],
    format: &Format,
    runtime: Arc<dyn SystemRuntime>,
    project_artifacts: Option<&mut ArtifactStore>,
    render_options: &crate::render_to_file::RenderToFileOptions,
) -> Result<RenderToFileResult> {
    let binaries = BinaryDependencies::discover(runtime.as_ref());
    let options = BookRenderOptions {
        engine_registry_override: render_options.engine_registry_override.clone(),
        execution_policy: render_options.execution_policy.clone(),
    };
    let project_type = crate::project::orchestrator::project_type_for(project);
    let runtime_for_tail = runtime.clone();
    let outcome = render_book_core(
        BookCoreArgs {
            project,
            book_items,
            format,
            runtime: runtime.clone(),
            binaries: &binaries,
            options: &options,
            finishing_stages: build_pandoc_pipeline_finishing_stages(
                TransformPhase::Navigation,
                format.identifier,
            ),
            prepare_options: None,
            hooks: None,
        },
        &mut |_| (PartialKind::Native, Vec::new()),
        |stem| {
            // `book_output_stem` + the same output-dir-resolution policy
            // every document uses, fed a synthetic input path so
            // `determine_output_paths` derives the same stem back out
            // without duplicating its fallback logic. Native ignores the
            // caller's options here (`default()`), exactly as before.
            let synthetic_input = project.dir.join(format!("{stem}.qmd"));
            let options = crate::render_to_file::RenderToFileOptions::default();
            let effective_options =
                apply_project_output_dir_to_options(&options, project, &synthetic_input);
            let (output_path, output_dir, output_stem) = determine_output_paths(
                &synthetic_input,
                &format.target_format,
                &effective_options,
                runtime.as_ref(),
            )?;
            runtime.dir_create(&output_dir, true).map_err(|e| {
                QuartoError::other(format!(
                    "Failed to create output directory {}: {}",
                    output_dir.display(),
                    e
                ))
            })?;
            let resource_paths = crate::resources::prepare_html_resources(
                &output_dir,
                &output_stem,
                runtime.as_ref(),
            )?;
            let resolver = ResourceResolverContext::website(
                &project.output_dir,
                &output_path,
                project_type.lib_dir(),
                &output_stem,
            );
            Ok((
                BookSetup {
                    synthetic_input,
                    output_path,
                    resolver,
                },
                resource_paths.resource_dir,
            ))
        },
        |ctx, resource_dir, setup, tail| {
            let render_output = crate::pipeline::RenderOutput {
                html: String::new(),
                diagnostics: tail.all_diagnostics(),
                source_context: tail.source_context.clone(),
                // bd-sl79jjiq: true when ANY chapter's own partial render
                // skipped a code-executing engine excluded by the render's
                // `ExecutionPolicy`.
                execution_skipped: tail.execution_skipped,
            };
            finalize_rendered_output(
                &setup.synthetic_input,
                setup.output_path.clone(),
                resource_dir,
                render_output,
                ctx,
                &setup.resolver,
                project_type.as_ref(),
                project_artifacts,
                &runtime_for_tail,
                false, // is_native: Pandoc-hybrid leg wrote output_path directly
            )
        },
    )
    .await?;
    match outcome {
        BookCoreOutcome::Done { value, .. } => Ok(value),
        BookCoreOutcome::ChapterFailed { error, .. } => Err(error),
        BookCoreOutcome::Cancelled => Err(QuartoError::other("book render cancelled".to_string())),
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
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
            &BookRenderOptions::default(),
            PartialKind::Native,
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
            PartialKind::Native,
        ));

        assert!(
            result.is_err(),
            "the naive pause runs citeproc (UserFiltersStage::pre() precedes the \
             Normalization pause point), so without defer_citeproc it must fail on \
             the missing bibliography — a passing result here would mean this \
             negative control no longer exercises the deferral path"
        );
    }

    /// Records every hook call and cancels before the `cancel_at`-th chapter.
    struct RecordingHooks {
        calls: std::sync::Mutex<Vec<(usize, usize, String)>>,
        cancel_at: Option<usize>,
    }

    #[async_trait::async_trait(?Send)]
    impl crate::project::book::BookRenderHooks for RecordingHooks {
        async fn before_chapter(
            &self,
            index: usize,
            total: usize,
            file: &str,
        ) -> std::result::Result<(), crate::project::book::Cancelled> {
            self.calls
                .lock()
                .unwrap()
                .push((index, total, file.to_string()));
            if self.cancel_at == Some(index) {
                Err(crate::project::book::Cancelled)
            } else {
                Ok(())
            }
        }
    }

    /// R9 "Abort and progress", core level: the hook sees `(1, N, file)`,
    /// `(2, N, file)`, ... in book order with the part divider skipped and `N`
    /// the number of file-bearing items; a `Cancelled` before the third
    /// chapter stops the render with no output and the third chapter is never
    /// rendered (its file is deleted after the items are computed: a render
    /// that reached it would fail reading it instead of cancelling).
    #[test]
    fn hooks_see_each_file_bearing_chapter_in_order_and_cancel_stops_the_loop() {
        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap();
        std::fs::write(
            root.join("_quarto.yml"),
            "project:\n  type: book\nbook:\n  title: B\n  chapters:\n    - index.qmd\n    - part: Part One\n      chapters:\n        - ch1.qmd\n        - ch2.qmd\n    - ch3.qmd\n",
        )
        .unwrap();
        for f in ["index", "ch1", "ch2", "ch3"] {
            std::fs::write(root.join(format!("{f}.qmd")), format!("# {f}\n\nBody.\n")).unwrap();
        }
        let runtime = make_test_runtime();
        let project = ProjectContext::discover(&root, runtime.as_ref()).unwrap();
        let book = project
            .config
            .metadata
            .as_ref()
            .and_then(|m| m.get("book"))
            .unwrap()
            .clone();
        let items =
            crate::project::book::book_render_items(&root, &book, "Appendices", runtime.as_ref())
                .unwrap();
        assert!(items.iter().any(|i| i.file.is_none()), "a part divider");
        std::fs::remove_file(root.join("ch2.qmd")).unwrap();

        let format = Format::from_format_string("typst").unwrap();
        let binaries = BinaryDependencies::new();
        let options = BookRenderOptions::default();
        let hooks = RecordingHooks {
            calls: Default::default(),
            cancel_at: Some(3),
        };
        let outcome = pollster::block_on(render_book_core(
            BookCoreArgs {
                project: &project,
                book_items: &items,
                format: &format,
                runtime: runtime.clone(),
                binaries: &binaries,
                options: &options,
                finishing_stages: Vec::new(),
                prepare_options: None,
                hooks: Some(&hooks),
            },
            &mut |_| (PartialKind::Native, Vec::new()),
            |_| -> Result<(BookSetup, ())> { unreachable!("cancelled before setup") },
            |_, _, _, _| -> Result<()> { unreachable!("cancelled before finish") },
        ))
        .unwrap();

        assert!(matches!(outcome, BookCoreOutcome::Cancelled));
        assert_eq!(
            hooks.calls.lock().unwrap().clone(),
            vec![
                (1, 4, "index.qmd".to_string()),
                (2, 4, "ch1.qmd".to_string()),
                (3, 4, "ch2.qmd".to_string()),
            ],
            "exactly the calls up to the cancel, never for the divider or later chapters"
        );
    }

    // ---- R9 task 5: chapter-relative resources through the shared core ----
    //
    // The core is driven the way the browser's whole-book request drives it:
    // the request pause/finishing lists, a website-mode resolver rooted at the
    // project directory, and `pandoc-prepare` as the tail. (The request-level
    // halves are in `pandoc_request_books.rs` once the driver exists.)

    const ONE_PIXEL_PNG: &[u8] = &[
        0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1f,
        0x15, 0xc4, 0x89, 0x00, 0x00, 0x00, 0x0a, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0x00,
        0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0d, 0x0a, 0x2d, 0xb4, 0x00, 0x00, 0x00, 0x00, 0x49,
        0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
    ];

    fn copy_tree(src: &std::path::Path, dst: &std::path::Path) {
        std::fs::create_dir_all(dst).unwrap();
        for entry in std::fs::read_dir(src).unwrap() {
            let entry = entry.unwrap();
            let to = dst.join(entry.file_name());
            if entry.path().is_dir() {
                copy_tree(&entry.path(), &to);
            } else {
                std::fs::copy(entry.path(), to).unwrap();
            }
        }
    }

    /// A scratch directory outside `/tmp` (request mounts reject it): under
    /// the workspace `target/tmp`, as the integration tests' `CARGO_TARGET_TMPDIR`.
    /// The built-in extension subtrees are extracted to the system temp dir,
    /// which is `/tmp` on Linux, so they are relocated under the scratch root
    /// too (each nextest test is its own process, so setting the env is safe).
    fn scratch_root() -> (tempfile::TempDir, std::path::PathBuf) {
        let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/tmp");
        std::fs::create_dir_all(&base).unwrap();
        let dir = tempfile::Builder::new()
            .prefix("q2-r9-core-")
            .tempdir_in(base)
            .unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap();
        let runtime = quarto_system_runtime::NativeRuntime::new();
        let extracted = crate::extension::builtin_extension_subtree_roots(&runtime);
        let src = extracted.first().expect("an extracted subtree root");
        let subtrees = root.join("subtrees");
        copy_tree(src, &subtrees);
        unsafe { std::env::set_var("QUARTO_EXTENSION_SUBTREES_DIR", &subtrees) };
        (dir, root)
    }

    fn put(root: &std::path::Path, rel: &str, bytes: &[u8]) {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    }

    /// A book with: a root chapter with a local image and a figure whose
    /// caption holds an image; a chapter in `sub/` with local, `../` and
    /// site-root images; an `href:` part page with an image; a cover image.
    fn images_book(root: &std::path::Path, extra_yml: &str) {
        put(
            root,
            "_quarto.yml",
            format!("project:\n  type: book\nbook:\n  title: B\n  cover-image: cover.png\n  chapters:\n    - index.qmd\n    - one.qmd\n    - sub/two.qmd\n    - part: Part Two\n      href: partpage.qmd\n      chapters:\n        - three.qmd\n{extra_yml}").as_bytes(),
        );
        put(root, "index.qmd", b"---\ntitle: Preface\n---\n\nHello\n");
        put(
            root,
            "one.qmd",
            b"# One\n\n![root local](rootlocal.png)\n\n![A caption with ![icon](icon.png) inside.](rootfig.png){#fig-one}\n",
        );
        put(
            root,
            "sub/two.qmd",
            b"# Two\n\n![local](local.png)\n\n![site root](/img/a.png)\n\n![up](../top.png)\n\n![Sub caption ![i](sub-icon.png).](subfig.png){#fig-two}\n",
        );
        put(
            root,
            "partpage.qmd",
            b"---\ntitle: Part Two Page\n---\n\n![part image](part.png)\n",
        );
        put(root, "three.qmd", b"# Three\n\nBody three.\n");
        for f in [
            "rootlocal.png",
            "icon.png",
            "rootfig.png",
            "top.png",
            "part.png",
            "cover.png",
            "img/a.png",
            "sub/local.png",
            "sub/sub-icon.png",
            "sub/subfig.png",
        ] {
            put(root, f, ONE_PIXEL_PNG);
        }
    }

    /// Run the shared core as the browser driver does and return the request.
    fn request_via_core(
        root: &std::path::Path,
        format_key: &str,
        post: crate::pandoc_request::RequestPost,
    ) -> (crate::pandoc_request::PandocRequest, BookTail) {
        let runtime = make_test_runtime();
        let project = ProjectContext::discover(root, runtime.as_ref()).unwrap();
        let book = project
            .config
            .metadata
            .as_ref()
            .and_then(|m| m.get("book"))
            .unwrap()
            .clone();
        let items =
            crate::project::book::book_render_items(root, &book, "Appendices", runtime.as_ref())
                .unwrap();
        let format = Format::from_format_string(format_key).unwrap();
        let binaries = BinaryDependencies::new();
        let options = BookRenderOptions {
            engine_registry_override: None,
            execution_policy: crate::engine::ExecutionPolicy::None,
        };
        let prepare = crate::pandoc_request::PrepareOptions {
            temp_root: std::path::PathBuf::from(&crate::pandoc_request::constants().share_root),
            source_date_epoch: Some(1_700_000_000),
            collect_resources: true,
            typst_available_fonts: None,
            post,
        };
        let outcome = pollster::block_on(render_book_core(
            BookCoreArgs {
                project: &project,
                book_items: &items,
                format: &format,
                runtime: runtime.clone(),
                binaries: &binaries,
                options: &options,
                finishing_stages: crate::pipeline::build_pandoc_request_finishing_stages(
                    TransformPhase::Navigation,
                ),
                prepare_options: Some(prepare),
                hooks: None,
            },
            &mut |_| {
                (
                    PartialKind::Request {
                        captures: Vec::new(),
                    },
                    Vec::new(),
                )
            },
            |stem| {
                let synthetic_input = project.dir.join(format!("{stem}.qmd"));
                let output_path = synthetic_input.with_extension(&format.output_extension);
                let resolver = ResourceResolverContext::website(
                    &project.dir,
                    &output_path,
                    crate::project::orchestrator::project_type_for(&project).lib_dir(),
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
                    .ok_or_else(|| QuartoError::other("no request".to_string()))
            },
        ))
        .unwrap();
        match outcome {
            BookCoreOutcome::Done { value, tail } => (value, tail),
            BookCoreOutcome::ChapterFailed { file, error } => {
                panic!("chapter {file} failed: {error}")
            }
            BookCoreOutcome::Cancelled => panic!("cancelled"),
        }
    }

    fn input_json_targets(request: &crate::pandoc_request::PandocRequest) -> Vec<String> {
        let input = request
            .files
            .iter()
            .find(|f| f.path.ends_with("/pandoc-input.json"))
            .expect("pandoc-input.json");
        let json: serde_json::Value = serde_json::from_slice(&input.bytes).unwrap();
        let mut out = Vec::new();
        fn walk(v: &serde_json::Value, out: &mut Vec<String>) {
            match v {
                serde_json::Value::Object(map) => {
                    if map.get("t").and_then(|t| t.as_str()) == Some("Image")
                        && let Some(target) = map["c"][2][0].as_str()
                    {
                        out.push(target.to_string());
                    }
                    map.values().for_each(|v| walk(v, out));
                }
                serde_json::Value::Array(items) => items.iter().for_each(|v| walk(v, out)),
                _ => {}
            }
        }
        walk(&json, &mut out);
        out
    }

    fn mounted(
        request: &crate::pandoc_request::PandocRequest,
        root: &std::path::Path,
    ) -> Vec<String> {
        let root = crate::pandoc_request::normalize_request_path(root);
        let mut v: Vec<String> = request
            .resource_refs
            .iter()
            .map(|f| {
                f.path
                    .strip_prefix(&format!("{root}/"))
                    .unwrap_or(&f.path)
                    .to_string()
            })
            .collect();
        v.sort();
        v
    }

    /// Every chapter's images, in the pdf request: the merged document holds
    /// project-relative targets and `resource_refs` holds each file under its
    /// own chapter's directory, caption images and the part page's image
    /// included, with no outside-the-project warning.
    #[test]
    fn pdf_request_mounts_chapter_relative_images_under_their_own_directories() {
        let (_guard, root) = scratch_root();
        images_book(&root, "");
        let (request, tail) = request_via_core(
            &root,
            "typst",
            crate::pandoc_request::RequestPost::CompileTypst,
        );
        assert_eq!(
            mounted(&request, &root),
            vec![
                "icon.png",
                "img/a.png",
                "part.png",
                "rootfig.png",
                "rootlocal.png",
                "sub/local.png",
                "sub/sub-icon.png",
                "sub/subfig.png",
                "top.png",
            ],
            "{:?}",
            tail.book_diagnostics
                .iter()
                .map(|d| &d.title)
                .collect::<Vec<_>>()
        );
        assert!(
            tail.book_diagnostics
                .iter()
                .all(|d| d.code.as_deref() != Some("Q-11-1")),
            "no outside-the-project warning"
        );
        let targets = input_json_targets(&request);
        for expected in [
            "rootlocal.png",
            "sub/local.png",
            "img/a.png",
            "top.png",
            "part.png",
            "icon.png",
            "sub/sub-icon.png",
        ] {
            assert!(
                targets.iter().any(|t| t == expected),
                "target {expected} in {targets:?}"
            );
        }
        assert!(
            !targets.iter().any(|t| t.starts_with('/')),
            "no root-absolute target is left: {targets:?}"
        );
    }

    /// A plain `typst` request names images in the `.typ` only and mounts none.
    #[test]
    fn typst_request_names_the_images_but_mounts_none() {
        let (_guard, root) = scratch_root();
        images_book(&root, "");
        let (request, _) =
            request_via_core(&root, "typst", crate::pandoc_request::RequestPost::None);
        assert!(
            request.resource_refs.is_empty(),
            "{:?}",
            mounted(&request, &root)
        );
        let targets = input_json_targets(&request);
        assert!(targets.iter().any(|t| t == "sub/local.png"), "{targets:?}");
        assert!(targets.iter().any(|t| t == "part.png"), "{targets:?}");
    }

    /// EPUB: the images and the `book.cover-image` reach `resource_refs`.
    #[test]
    fn epub_request_mounts_images_and_the_cover() {
        let (_guard, root) = scratch_root();
        images_book(&root, "");
        let (request, _) =
            request_via_core(&root, "epub", crate::pandoc_request::RequestPost::None);
        let got = mounted(&request, &root);
        for expected in [
            "cover.png",
            "sub/local.png",
            "top.png",
            "img/a.png",
            "part.png",
        ] {
            assert!(got.iter().any(|g| g == expected), "{expected} in {got:?}");
        }
    }

    /// Margin citations leave `bibliography`/`csl` in the metadata for typst
    /// to read at compile time, so the pdf request carries the files, and the
    /// plain `typst` request needs none.
    #[test]
    fn margin_citation_bibliography_and_csl_reach_the_pdf_request() {
        let (_guard, root) = scratch_root();
        images_book(
            &root,
            "bibliography: refs.bib\ncsl: style.csl\nreference-location: margin\ncitation-location: margin\nsuppress-bibliography: true\n",
        );
        put(
            &root,
            "refs.bib",
            b"@book{k, author={A}, title={T}, year={2000}}\n",
        );
        put(&root, "style.csl", b"<style/>\n");
        put(&root, "one.qmd", b"# One\n\nCite [@k].\n");
        let (pdf, _) = request_via_core(
            &root,
            "typst",
            crate::pandoc_request::RequestPost::CompileTypst,
        );
        let got = mounted(&pdf, &root);
        assert!(got.iter().any(|g| g == "refs.bib"), "{got:?}");
        assert!(got.iter().any(|g| g == "style.csl"), "{got:?}");
        let (typ, _) = request_via_core(&root, "typst", crate::pandoc_request::RequestPost::None);
        assert!(typ.resource_refs.is_empty());
    }
}
