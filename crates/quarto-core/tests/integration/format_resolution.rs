/*
 * tests/integration/format_resolution.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * F3a: the resolver through the whole entry points. The orchestrator
 * decides the format of a whole-book render once; every other render
 * lets each document choose its own (epic FR4).
 */

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tempfile::TempDir;

use quarto_core::format::Format;
use quarto_core::format_resolution::{ScopeRequest, resolve_document_format};
use quarto_core::overlay_runtime::RenderOverlay;
use quarto_core::project::ProjectContext;
use quarto_core::project::orchestrator::{ProjectPipeline, RenderMode, project_type_for};
use quarto_core::render_to_file::RenderToFileOptions;
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

fn canonical(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn write(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, contents).unwrap();
}

fn native() -> Arc<dyn SystemRuntime> {
    Arc::new(NativeRuntime::new())
}

fn with_metadata(json: serde_json::Value) -> Arc<dyn SystemRuntime> {
    Arc::new(RenderOverlay::with_metadata(json)).apply(native())
}

const BOOK: &str = "project:\n  type: book\nformat: html\nbook:\n  title: Fixture\n  chapters:\n    - index.qmd\n    - ch1.qmd\n    - ch2.qmd\n";
const DECK: &str = "---\nformat: revealjs\n---\n\n## Slide\n\n- one\n";
const TYPST_CHAPTER: &str = "---\nformat: typst\n---\n\n# Chapter\n\ntext\n";
const PLAIN: &str = "# Chapter\n\ntext\n";

fn book(root: &Path, ch1: &str) {
    write(&root.join("_quarto.yml"), BOOK);
    write(&root.join("index.qmd"), "# Home {.unnumbered}\n\nhome\n");
    write(&root.join("ch1.qmd"), ch1);
    write(&root.join("ch2.qmd"), PLAIN);
}

/// Render through the native pipeline and return the output paths.
fn render(
    root: &Path,
    runtime: Arc<dyn SystemRuntime>,
    mode: RenderMode,
    to: Option<&str>,
) -> Vec<PathBuf> {
    let mut project = ProjectContext::discover(root, runtime.as_ref()).unwrap();
    let project_type = project_type_for(&project);
    let options = RenderToFileOptions::default();
    let mut pipeline = ProjectPipeline::new(
        &mut project,
        project_type,
        Format::from_format_string("html").unwrap(),
        "html",
        &options,
        runtime,
    )
    .with_mode(mode)
    .with_format_override(to.map(str::to_string));
    let summary = pollster::block_on(pipeline.run_with_book_support())
        .unwrap_or_else(|e| panic!("render failed: {e}"));
    assert!(summary.pass2_failures.is_empty(), "{summary:?}");
    summary.outputs.into_iter().map(|o| o.output_path).collect()
}

fn output<'a>(outputs: &'a [PathBuf], stem: &str) -> &'a PathBuf {
    outputs
        .iter()
        .find(|p| p.file_stem().is_some_and(|s| s == stem))
        .unwrap_or_else(|| panic!("no output with stem {stem} in {outputs:?}"))
}

fn is_deck(path: &Path) -> bool {
    std::fs::read_to_string(path)
        .unwrap()
        .contains("class=\"reveal\"")
}

fn subset(root: &Path, names: &[&str]) -> RenderMode {
    RenderMode::Subset(names.iter().map(|n| root.join(n)).collect::<HashSet<_>>())
}

#[test]
fn a_full_html_book_keeps_a_typst_chapter_html() {
    let temp = TempDir::new().unwrap();
    let root = canonical(temp.path());
    book(&root, TYPST_CHAPTER);
    let outputs = render(&root, native(), RenderMode::Full, None);
    for out in &outputs {
        assert_eq!(
            out.extension().and_then(|e| e.to_str()),
            Some("html"),
            "{out:?}: every member renders in the project's format"
        );
    }
    assert!(
        outputs
            .iter()
            .any(|p| p.file_stem().is_some_and(|s| s == "ch1"))
    );
}

#[test]
fn a_full_html_book_keeps_a_deck_chapter_html() {
    let temp = TempDir::new().unwrap();
    let root = canonical(temp.path());
    book(&root, DECK);
    let outputs = render(&root, native(), RenderMode::Full, None);
    assert!(!is_deck(output(&outputs, "ch1")));
}

#[test]
fn a_full_website_keeps_a_deck_page() {
    let temp = TempDir::new().unwrap();
    let root = canonical(temp.path());
    write(
        &root.join("_quarto.yml"),
        "project:\n  type: website\nformat: html\nwebsite:\n  title: W\n",
    );
    write(&root.join("index.qmd"), PLAIN);
    write(&root.join("deck.qmd"), DECK);
    let outputs = render(&root, native(), RenderMode::Full, None);
    assert!(
        is_deck(output(&outputs, "deck")),
        "the page keeps its format"
    );
    assert!(!is_deck(output(&outputs, "index")));
}

#[test]
fn named_files_each_use_their_own_format() {
    let temp = TempDir::new().unwrap();
    let root = canonical(temp.path());
    book(&root, DECK);
    let outputs = render(
        &root,
        native(),
        subset(&root, &["ch1.qmd", "ch2.qmd"]),
        None,
    );
    assert!(is_deck(output(&outputs, "ch1")));
    assert!(!is_deck(output(&outputs, "ch2")));
    // One named file likewise.
    let outputs = render(&root, native(), subset(&root, &["ch1.qmd"]), None);
    assert!(is_deck(output(&outputs, "ch1")));
}

#[test]
fn an_explicit_book_scope_with_one_file_uses_the_projects_format() {
    let temp = TempDir::new().unwrap();
    let root = canonical(temp.path());
    book(&root, DECK);
    let runtime = with_metadata(serde_json::json!({"book-scope": "book"}));
    let outputs = render(&root, runtime, subset(&root, &["ch1.qmd"]), None);
    assert!(!is_deck(output(&outputs, "ch1")));
}

#[test]
fn an_explicit_to_applies_to_every_member() {
    let temp = TempDir::new().unwrap();
    let root = canonical(temp.path());
    book(&root, DECK);
    // `--to html` over a chapter that says revealjs, in a chapter render.
    let outputs = render(&root, native(), subset(&root, &["ch1.qmd"]), Some("html"));
    assert!(!is_deck(output(&outputs, "ch1")));
}

// ── profiles and `_metadata.yml` through the resolver ────────────────────

fn format_of(project: &ProjectContext, runtime: &dyn SystemRuntime, doc: &Path) -> String {
    resolve_document_format(project, doc, None, &ScopeRequest::chapter(), None, runtime).key
}

#[test]
fn a_project_only_format_supplied_by_a_profile_is_seen() {
    let temp = TempDir::new().unwrap();
    let root = canonical(temp.path());
    write(&root.join("_quarto.yml"), "project:\n  type: website\n");
    write(&root.join("_quarto-print.yml"), "format: typst\n");
    write(&root.join("index.qmd"), PLAIN);
    let runtime = NativeRuntime::new();
    let doc = root.join("index.qmd");

    let plain = ProjectContext::discover_with_profile(&doc, &runtime, None).unwrap();
    assert_eq!(format_of(&plain, &runtime, &doc), "html");

    // `--profile print`
    let selected = vec!["print".to_string()];
    let profiled = ProjectContext::discover_with_profile(&doc, &runtime, Some(&selected)).unwrap();
    assert_eq!(format_of(&profiled, &runtime, &doc), "typst");
}

#[test]
fn a_project_only_format_from_default_profile_or_environment_file_is_seen() {
    for (name, quarto, extra) in [
        (
            "profile.default",
            "project:\n  type: website\nprofile:\n  default: print\n",
            None,
        ),
        (
            "_environment",
            "project:\n  type: website\n",
            Some(("_environment", "QUARTO_PROFILE=print\n")),
        ),
    ] {
        let temp = TempDir::new().unwrap();
        let root = canonical(temp.path());
        write(&root.join("_quarto.yml"), quarto);
        write(&root.join("_quarto-print.yml"), "format: typst\n");
        write(&root.join("index.qmd"), PLAIN);
        if let Some((file, content)) = extra {
            write(&root.join(file), content);
        }
        let runtime = NativeRuntime::new();
        let doc = root.join("index.qmd");
        let project = ProjectContext::discover(&doc, &runtime).unwrap();
        assert_eq!(format_of(&project, &runtime, &doc), "typst", "{name}");
    }
}

#[test]
fn a_nested_metadata_format_is_a_document_level_key() {
    let temp = TempDir::new().unwrap();
    let root = canonical(temp.path());
    write(&root.join("_quarto.yml"), BOOK);
    write(&root.join("index.qmd"), "# Home\n");
    write(&root.join("ch1.qmd"), PLAIN);
    write(&root.join("ch2.qmd"), PLAIN);
    write(&root.join("part/_metadata.yml"), "format: revealjs\n");
    write(&root.join("part/doc.qmd"), PLAIN);
    let runtime = NativeRuntime::new();
    let project = ProjectContext::discover(root.join("index.qmd"), &runtime).unwrap();
    let doc = root.join("part/doc.qmd");

    // Chapter scope: the directory's key is a document-level key.
    assert_eq!(format_of(&project, &runtime, &doc), "revealjs");
    // Book scope: only the project's keys select.
    let book_request = ScopeRequest {
        value: quarto_core::BookScope::Book,
        ..ScopeRequest::chapter()
    };
    let r = resolve_document_format(&project, &doc, None, &book_request, None, &runtime);
    assert_eq!(r.key, "html");
}

#[test]
fn a_runtime_scalar_selects_and_to_wins_through_the_entry_point() {
    let temp = TempDir::new().unwrap();
    let root = canonical(temp.path());
    write(&root.join("_quarto.yml"), "project:\n  type: website\n");
    write(
        &root.join("index.qmd"),
        "---\nformat:\n  html: default\n  revealjs: default\n---\n\n# H\n",
    );
    let doc = root.join("index.qmd");
    let runtime = with_metadata(serde_json::json!({"format": "revealjs"}));
    let project = ProjectContext::discover(&doc, runtime.as_ref()).unwrap();
    assert_eq!(format_of(&project, runtime.as_ref(), &doc), "revealjs");
    let r = resolve_document_format(
        &project,
        &doc,
        None,
        &ScopeRequest::chapter(),
        Some("typst-pdf"),
        runtime.as_ref(),
    );
    assert_eq!(r.key, "typst", "`--to` wins, and typst-pdf is normalised");
}

// ── F3b: the requested scope reaches filters; notices come once ────────────

const PROBE_FILTER: &str = "function Pandoc(doc)\n  local v = doc.meta['book-scope']\n  local seen = v and pandoc.utils.stringify(v) or 'nothing'\n  return pandoc.Pandoc({ pandoc.Para({ pandoc.Str('SCOPE-SEEN=' .. seen) }) }, doc.meta)\nend\n";

fn probe_book(root: &Path) {
    write(&root.join("_quarto.yml"), BOOK);
    write(&root.join("index.qmd"), "# Home {.unnumbered}\n\nhome\n");
    write(
        &root.join("ch1.qmd"),
        "---\nfilters:\n  - probe.lua\n---\n\n# One\n\ntext\n",
    );
    write(&root.join("ch2.qmd"), PLAIN);
    write(&root.join("probe.lua"), PROBE_FILTER);
}

fn rendered_text(root: &Path, runtime: Arc<dyn SystemRuntime>) -> String {
    let outputs = render(root, runtime, subset(root, &["ch1.qmd"]), None);
    std::fs::read_to_string(output(&outputs, "ch1")).unwrap()
}

#[test]
fn a_filter_sees_the_requested_book_scope_and_nothing_when_none_was_requested() {
    let temp = TempDir::new().unwrap();
    let root = canonical(temp.path());
    probe_book(&root);
    // Asked for: the filter reads the requested value from the metadata.
    let asked = rendered_text(
        &root,
        with_metadata(serde_json::json!({"book-scope": "chapter"})),
    );
    assert!(asked.contains("SCOPE-SEEN=chapter"), "{asked}");
    // Not asked for: only the target's default applies, and it is not
    // written back into the metadata.
    let default = rendered_text(&root, native());
    assert!(default.contains("SCOPE-SEEN=nothing"), "{default}");
}

fn project_notices(
    root: &Path,
    runtime: Arc<dyn SystemRuntime>,
    mode: RenderMode,
    format: &str,
) -> Vec<String> {
    let mut project = ProjectContext::discover(root, runtime.as_ref()).unwrap();
    let project_type = project_type_for(&project);
    let options = RenderToFileOptions::default();
    let mut pipeline = ProjectPipeline::new(
        &mut project,
        project_type,
        Format::from_format_string(format).unwrap(),
        format,
        &options,
        runtime,
    )
    .with_mode(mode)
    .with_format_override(Some(format.to_string()));
    let summary = pollster::block_on(pipeline.run_with_book_support())
        .unwrap_or_else(|e| panic!("render failed: {e}"));
    summary
        .project_diagnostics
        .iter()
        .filter_map(|d| d.code.clone())
        .filter(|c| matches!(c.as_str(), "Q-5-41" | "Q-5-42" | "Q-5-48"))
        .collect()
}

#[test]
fn the_scope_notices_are_emitted_once_on_both_dispatch_paths() {
    let temp = TempDir::new().unwrap();
    let root = canonical(temp.path());
    book(&root, PLAIN);
    write(&root.join("notes.qmd"), PLAIN);
    let asked_book = || with_metadata(serde_json::json!({"book-scope": "book"}));
    let asked_chapter = || with_metadata(serde_json::json!({"book-scope": "chapter"}));

    // html: the orchestrator's tail path (`resolve_book_scope` is never called).
    let html = |rt: Arc<dyn SystemRuntime>, file: &str| {
        project_notices(&root, rt, subset(&root, &[file]), "html")
    };
    assert_eq!(html(native(), "notes.qmd"), ["Q-5-41"]);
    assert_eq!(html(asked_chapter(), "notes.qmd"), ["Q-5-41"]);
    assert_eq!(html(asked_book(), "notes.qmd"), ["Q-5-48"]);
    // A member with an explicit runtime `book` on html: the Q-5-42 warning, once.
    assert_eq!(html(asked_book(), "ch1.qmd"), ["Q-5-42"]);
    assert!(html(native(), "ch1.qmd").is_empty());
    assert!(html(asked_chapter(), "ch1.qmd").is_empty());

    // epub: `run_book_scoped` emits them, and the tail does not repeat them.
    let epub = |rt: Arc<dyn SystemRuntime>, file: &str| {
        project_notices(&root, rt, subset(&root, &[file]), "epub")
    };
    if quarto_system_runtime::NativeRuntime::new()
        .find_binary("pandoc", "QUARTO_PANDOC")
        .is_some()
    {
        assert_eq!(epub(native(), "notes.qmd"), ["Q-5-41"]);
        assert_eq!(epub(asked_book(), "notes.qmd"), ["Q-5-48"]);
        assert!(epub(native(), "ch1.qmd").is_empty());
    }
}
