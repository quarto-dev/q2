//! Pipeline-level tests for `LanguageResolveStage` (localization, bd-llhlzd7p).
//!
//! The stage runs immediately after `metadata-merge`, resolves the document's
//! term table from `lang` + `language:` metadata (+ project-root
//! `_language.yml`), and injects it into `doc.ast.meta` under
//! `quarto.language` — the single transport consumed by transforms
//! (`LanguageTerms::from_meta`) and templates (`$quarto.language.<key>$`).

use std::path::PathBuf;
use std::sync::Arc;

use quarto_core::format::Format;
use quarto_core::language::LanguageTerms;
use quarto_core::pipeline::{build_html_pipeline_stages, build_wasm_html_pipeline};
use quarto_core::project::{DocumentInfo, ProjectConfig, ProjectContext};
use quarto_core::stage::{
    LoadedSource, Pipeline, PipelineData, PipelineDataKind, PipelineStage, StageContext,
};
use quarto_error_reporting::DiagnosticKind;
use quarto_pandoc_types::ConfigValue;

fn make_context(project_dir: PathBuf, doc_path: PathBuf, is_single_file: bool) -> StageContext {
    let runtime = Arc::new(quarto_system_runtime::NativeRuntime::new());
    let format = Format::html();
    let project = ProjectContext {
        dir: project_dir.clone(),
        config: ProjectConfig::default(),
        is_single_file,
        files: vec![DocumentInfo::from_path(doc_path.clone())],
        output_dir: project_dir,
        ..Default::default()
    };
    let document = DocumentInfo::from_path(doc_path);
    StageContext::new(runtime, format, project, document).expect("stage context")
}

/// Run the html pipeline stages up to and including `language-resolve`,
/// returning the post-stage metadata and the collected diagnostics.
async fn run_through_language_resolve(
    content: &[u8],
    ctx: &mut StageContext,
    doc_path: PathBuf,
) -> ConfigValue {
    let full = build_html_pipeline_stages();
    let pos = full
        .iter()
        .position(|s| s.name() == "language-resolve")
        .expect("language-resolve stage present in html pipeline");
    let head: Vec<Box<dyn PipelineStage>> = full.into_iter().take(pos + 1).collect();
    let pipeline = Pipeline::new(head).expect("head pipeline valid");
    let input = PipelineData::LoadedSource(LoadedSource::new(doc_path, content.to_vec()));
    let out = pipeline.run(input, ctx).await.expect("pipeline runs");
    assert_eq!(out.kind(), PipelineDataKind::DocumentAst);
    out.into_document_ast().unwrap().ast.meta
}

/// Convenience: run in a synthetic cwd-based single-file context.
async fn run_simple(content: &[u8]) -> (ConfigValue, StageContext) {
    let dir = std::env::current_dir().unwrap();
    let doc = dir.join("test.qmd");
    let mut ctx = make_context(dir, doc.clone(), true);
    let meta = run_through_language_resolve(content, &mut ctx, doc).await;
    (meta, ctx)
}

fn quarto_language_term(meta: &ConfigValue, key: &str) -> Option<String> {
    meta.get("quarto")?
        .get("language")?
        .get(key)?
        .as_plain_text()
}

// ── Stage placement ────────────────────────────────────────────────────────

#[test]
fn pipelines_run_language_resolve_right_after_metadata_merge() {
    let stages = build_html_pipeline_stages();
    let names: Vec<&str> = stages.iter().map(|s| s.name()).collect();
    let merge = names
        .iter()
        .position(|n| *n == "metadata-merge")
        .expect("metadata-merge present");
    assert_eq!(
        names.get(merge + 1).copied(),
        Some("language-resolve"),
        "language-resolve must directly follow metadata-merge: {names:?}"
    );

    let wasm_pipeline = build_wasm_html_pipeline();
    let wasm_names = wasm_pipeline.stage_names();
    let wasm_merge = wasm_names
        .iter()
        .position(|n| *n == "metadata-merge")
        .expect("metadata-merge present in wasm pipeline");
    assert_eq!(
        wasm_names.get(wasm_merge + 1).copied(),
        Some("language-resolve"),
        "wasm pipeline must include language-resolve after metadata-merge: {wasm_names:?}"
    );
}

// ── Meta injection ─────────────────────────────────────────────────────────

#[tokio::test]
async fn lang_es_injects_spanish_terms_into_meta() {
    let (meta, ctx) = run_simple(b"---\nlang: es\n---\n\nHello.\n").await;
    assert_eq!(
        quarto_language_term(&meta, "callout-note-title").as_deref(),
        Some("Nota")
    );
    assert_eq!(
        quarto_language_term(&meta, "toc-title-document").as_deref(),
        Some("Tabla de contenidos")
    );
    assert!(
        ctx.diagnostics.is_empty(),
        "no diagnostics expected: {:?}",
        ctx.diagnostics
    );
}

#[tokio::test]
async fn no_lang_defaults_to_english_terms() {
    let (meta, _) = run_simple(b"---\ntitle: T\n---\n\nHello.\n").await;
    assert_eq!(
        quarto_language_term(&meta, "callout-note-title").as_deref(),
        Some("Note")
    );
}

#[tokio::test]
async fn from_meta_round_trips_resolved_terms() {
    let (meta, _) = run_simple(b"---\nlang: pt-BR\n---\n\nHello.\n").await;
    let terms = LanguageTerms::from_meta(&meta).expect("quarto.language present");
    assert_eq!(terms.lang(), "pt-BR");
    assert_eq!(terms.get("code-links-title"), Some("Links de código"));
    assert_eq!(terms.crossref_prefix("fig"), Some("Figura"));
}

#[tokio::test]
async fn inline_language_map_overrides_shipped_terms() {
    let src = b"---\nlang: fr\nlanguage:\n  toc-title-document: \"Sommaire\"\n---\n\nBonjour.\n";
    let (meta, ctx) = run_simple(src).await;
    assert_eq!(
        quarto_language_term(&meta, "toc-title-document").as_deref(),
        Some("Sommaire")
    );
    // Untouched keys keep shipped fr values.
    assert_eq!(
        quarto_language_term(&meta, "title-block-published").as_deref(),
        Some("Date de publication")
    );
    assert!(ctx.diagnostics.is_empty());
}

#[tokio::test]
async fn unknown_language_key_warns_through_stage_diagnostics() {
    let src = b"---\nlanguage:\n  my-custom-term: \"Zap\"\n---\n\nHello.\n";
    let (meta, ctx) = run_simple(src).await;
    let warnings: Vec<_> = ctx
        .diagnostics
        .iter()
        .filter(|d| d.kind == DiagnosticKind::Warning)
        .collect();
    assert_eq!(warnings.len(), 1, "one warning expected: {warnings:?}");
    assert!(warnings[0].title.contains("my-custom-term"));
    // The custom term is still resolvable.
    assert_eq!(
        quarto_language_term(&meta, "my-custom-term").as_deref(),
        Some("Zap")
    );
}

// ── File-based language config ─────────────────────────────────────────────

fn scratch_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir()
        .join("q2-language-pipeline-tests")
        .join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

#[tokio::test]
async fn language_file_resolves_relative_to_document() {
    let dir = scratch_dir("file-form");
    std::fs::write(
        dir.join("custom.yml"),
        "toc-title-document: \"Custom TOC\"\n",
    )
    .unwrap();
    let doc = dir.join("test.qmd");
    let mut ctx = make_context(dir, doc.clone(), true);
    let meta =
        run_through_language_resolve(b"---\nlanguage: custom.yml\n---\n\nHello.\n", &mut ctx, doc)
            .await;
    assert_eq!(
        quarto_language_term(&meta, "toc-title-document").as_deref(),
        Some("Custom TOC")
    );
    assert!(
        ctx.diagnostics.is_empty(),
        "no diagnostics expected: {:?}",
        ctx.diagnostics
    );
}

#[tokio::test]
async fn missing_language_file_is_an_error_diagnostic() {
    let dir = scratch_dir("missing-file");
    let doc = dir.join("test.qmd");
    let mut ctx = make_context(dir, doc.clone(), true);
    let meta =
        run_through_language_resolve(b"---\nlanguage: nope.yml\n---\n\nHello.\n", &mut ctx, doc)
            .await;
    let errors: Vec<_> = ctx
        .diagnostics
        .iter()
        .filter(|d| d.kind == DiagnosticKind::Error)
        .collect();
    assert_eq!(errors.len(), 1, "one error expected: {:?}", ctx.diagnostics);
    assert!(
        errors[0].title.contains("nope.yml"),
        "error should name the file: {}",
        errors[0].title
    );
    // Shipped terms still resolve (render continues usable).
    assert_eq!(
        quarto_language_term(&meta, "callout-note-title").as_deref(),
        Some("Note")
    );
}

#[tokio::test]
async fn project_root_language_yml_is_autodetected() {
    let dir = scratch_dir("project-root");
    std::fs::write(
        dir.join("_language.yml"),
        "toc-title-document: \"Project TOC\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("_language-fr.yml"),
        "toc-title-document: \"TOC projet\"\n",
    )
    .unwrap();
    let doc = dir.join("test.qmd");

    // lang: fr picks the sibling _language-fr.yml value.
    let mut ctx = make_context(dir.clone(), doc.clone(), false);
    let meta =
        run_through_language_resolve(b"---\nlang: fr\n---\n\nBonjour.\n", &mut ctx, doc.clone())
            .await;
    assert_eq!(
        quarto_language_term(&meta, "toc-title-document").as_deref(),
        Some("TOC projet")
    );

    // Without lang, the project base file applies.
    let mut ctx2 = make_context(dir, doc.clone(), false);
    let meta2 =
        run_through_language_resolve(b"---\ntitle: T\n---\n\nHello.\n", &mut ctx2, doc).await;
    assert_eq!(
        quarto_language_term(&meta2, "toc-title-document").as_deref(),
        Some("Project TOC")
    );
}

#[tokio::test]
async fn single_file_render_ignores_project_root_language_yml() {
    // Auto-detection is a project feature (Q1: "alongside _quarto.yml").
    let dir = scratch_dir("single-file");
    std::fs::write(
        dir.join("_language.yml"),
        "toc-title-document: \"Should not apply\"\n",
    )
    .unwrap();
    let doc = dir.join("test.qmd");
    let mut ctx = make_context(dir, doc.clone(), true);
    let meta = run_through_language_resolve(b"---\ntitle: T\n---\n\nHello.\n", &mut ctx, doc).await;
    assert_eq!(
        quarto_language_term(&meta, "toc-title-document").as_deref(),
        Some("Table of contents")
    );
}
