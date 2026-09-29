/*
 * project/book/static_analyzer.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Pre-engine, cross-chapter-aware crossref sweep for book preview
 * (book-projects P8).
 */

//! `StaticProjectAnalyzer`: gives `q2 preview`/hub-client an approximate,
//! cross-chapter-aware crossref resolution without a real multi-chapter
//! render.
//!
//! A previewed chapter renders alone, through `RenderToPreviewAstRenderer` —
//! it never runs P2/P3's real `run_with_book_support()` orchestration, so a
//! `@ref` to a sibling chapter's target has nothing to resolve against.
//! This module sweeps every sibling chapter's *syntactically visible*
//! crossref targets (pre-engine — no code execution) and merges them into a
//! [`ProjectCrossrefIndex`], the same registry type P5's real Pass 3
//! produces from a full multi-chapter render. The previewed document's
//! `RenderContext.cross_chapter_crossref_registry` field is populated from
//! this sweep (see [`RenderToPreviewAstRenderer::render`]); P5's own
//! `CrossChapterCrossrefResolveTransform` reads it unmodified.
//!
//! ## Scope
//!
//! Sweeps exactly [`BookRenderItem`]'s file list — never
//! `ProjectContext::discover`'s full enumeration. A project can contain
//! non-chapter `.qmd` files (e.g. include-shortcode partials) that must
//! not be resolved as cross-chapter targets.
//!
//! ## Pipeline reuse
//!
//! Each sibling runs through [`build_analysis_pipeline`] — Parse +
//! MetadataMerge + LanguageResolve + IncludeExpansion + PreEngineSugaring +
//! AstTransforms bounded to Normalization+Crossref — the same
//! LSP-precedented, pre-engine subset `quarto-lsp-core`'s
//! `analyze_document_async` already uses for single-file analysis. Only
//! syntactically-visible crossref targets are resolvable pre-engine anyway
//! (single-file crossref design D2/D6), so nothing deeper is needed.
//!
//! ## Merge rule
//!
//! [`aggregate_chapter_inventories`] does the actual merge (first-in-book-
//! order wins, with a non-fatal `Q-15-2` diagnostic on a same-id collision
//! across chapters) — this module builds one [`ChapterCrossrefInventory`]
//! per swept sibling and hands the list to it, rather than reimplementing
//! any merge or number-composition logic itself.
//!
//! ## `owning_chapter_path`, not `owning_chapter_href`
//!
//! Preview has no real rendered output — there is no `_book/ch2.html` to
//! point at. Each inventory's `output_href` is left empty (inert: preview
//! navigation never reads `owning_chapter_href`) and `owning_chapter_path`
//! carries the project-relative *source* path instead, which
//! `CrossChapterCrossrefResolveTransform` patches onto the resolved node's
//! `plain_data` for hub-client's document-based navigation.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use quarto_error_reporting::DiagnosticMessage;
use quarto_source_map::FileId;
use quarto_system_runtime::SystemRuntime;

use crate::crossref::CrossrefIndex;
use crate::crossref::project_index::{
    ChapterCrossrefInventory, ProjectCrossrefIndex, aggregate_chapter_inventories,
};
use crate::format::Format;
use crate::pipeline::build_analysis_pipeline;
use crate::project::book::BookRenderItem;
use crate::project::{DocumentInfo, ProjectConfig, ProjectContext};
use crate::render::ChapterSeed;
use crate::stage::{LoadedSource, PipelineData, StageContext};

/// Statically sweep a book project's chapters and build an approximate,
/// cross-chapter-aware [`ProjectCrossrefIndex`] for preview.
///
/// `project_dir` and `book_items` come straight from the same
/// `pre_render`-populated state P4's real seed map uses; `seed_map` is
/// that seed map itself (built by
/// [`chapter_seed_map`](super::render_item::chapter_seed_map)), applied
/// per sibling so each chapter's approximate numbers are chapter-scoped —
/// a sibling's raw section counter can't produce a correct number without
/// its own seed. Returns the merged registry plus any diagnostics raised
/// while reading or analyzing a sibling (a read/parse failure degrades
/// that one chapter to "contributes nothing," not a preview crash) and by
/// the merge step itself (duplicate ids across chapters).
pub(crate) async fn analyze_book_project_statically(
    project_dir: &Path,
    book_items: &[BookRenderItem],
    seed_map: &HashMap<PathBuf, ChapterSeed>,
    runtime: Arc<dyn SystemRuntime>,
) -> (ProjectCrossrefIndex, Vec<DiagnosticMessage>) {
    let mut inventories = Vec::with_capacity(book_items.len());
    let mut diagnostics = Vec::new();

    for item in book_items {
        let Some(rel_path) = &item.file else {
            continue;
        };
        let abs_path = project_dir.join(rel_path);
        // Matches `chapter_seed_map`'s own key derivation exactly (`std`'s
        // `Path::canonicalize`, not the runtime's) so a lookup against
        // `seed_map` hits the same entry that map was keyed with.
        let seed_key = abs_path.canonicalize().unwrap_or_else(|_| abs_path.clone());
        let chapter_seed = seed_map.get(&seed_key).copied();

        let content = match runtime.file_read(&abs_path) {
            Ok(bytes) => bytes,
            Err(e) => {
                diagnostics.push(DiagnosticMessage::warning(format!(
                    "StaticProjectAnalyzer: could not read sibling chapter {} for cross-chapter \
                     crossref preview: {e}",
                    abs_path.display()
                )));
                continue;
            }
        };

        let (index, chapter_diags) =
            analyze_one_sibling(&abs_path, &content, chapter_seed, runtime.clone()).await;
        diagnostics.extend(chapter_diags);

        inventories.push(ChapterCrossrefInventory {
            index,
            chapter_seed,
            output_href: String::new(),
            owning_chapter_path: Some(rel_path.to_string_lossy().to_string()),
        });
    }

    let (project_index, merge_diagnostics) = aggregate_chapter_inventories(&inventories);
    diagnostics.extend(merge_diagnostics);
    (project_index, diagnostics)
}

/// Run the analysis pipeline over one sibling chapter and harvest its
/// [`CrossrefIndex`]. Mirrors `quarto-lsp-core`'s `analyze_document_async`
/// scaffolding (minimal single-file `ProjectContext`, direct
/// `StageContext` construction) rather than the full render pipeline —
/// only syntactically-visible targets are resolvable pre-engine anyway.
async fn analyze_one_sibling(
    doc_path: &Path,
    content: &[u8],
    chapter_seed: Option<ChapterSeed>,
    runtime: Arc<dyn SystemRuntime>,
) -> (CrossrefIndex, Vec<DiagnosticMessage>) {
    let dir = doc_path
        .parent()
        .map_or_else(|| PathBuf::from("/"), Path::to_path_buf);
    let project = ProjectContext {
        dir: dir.clone(),
        config: ProjectConfig::default(),
        is_single_file: true,
        files: vec![DocumentInfo::from_path(doc_path)],
        output_dir: dir,
        ..Default::default()
    };
    let document = DocumentInfo::from_path(doc_path);
    let format = Format::from_format_string("html").unwrap_or_else(|_| Format::default());

    let mut stage_ctx = match StageContext::new(runtime, format, project, document) {
        Ok(ctx) => ctx,
        Err(e) => {
            return (
                CrossrefIndex::new(FileId(0)),
                vec![DiagnosticMessage::warning(format!(
                    "StaticProjectAnalyzer: failed to set up analysis for {}: {e}",
                    doc_path.display()
                ))],
            );
        }
    };
    // Bridged one-way into the analysis transforms (mirrors the real
    // render path's `RenderContext::chapter_seed` → `StageContext::chapter_seed`
    // bridge) so `CrossrefIndexTransform` offsets the section counter and
    // this sibling's raw `Order`s come out already chapter-scoped.
    stage_ctx.chapter_seed = chapter_seed;

    let input =
        PipelineData::LoadedSource(LoadedSource::new(doc_path.to_path_buf(), content.to_vec()));

    let pipeline = build_analysis_pipeline();
    match pipeline.run(input, &mut stage_ctx).await {
        Ok(_) => {
            let index = stage_ctx
                .crossref_index
                .take()
                .unwrap_or_else(|| CrossrefIndex::new(FileId(0)));
            (index, std::mem::take(&mut stage_ctx.diagnostics))
        }
        Err(e) => (
            CrossrefIndex::new(FileId(0)),
            vec![DiagnosticMessage::warning(format!(
                "StaticProjectAnalyzer: failed to analyze sibling chapter {}: {e}",
                doc_path.display()
            ))],
        ),
    }
}
