/*
 * tests/integration/book_named_files.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * F5a of the format-resolution epic (F5a-D2, D3, D3b, D3d): the orchestrator
 * renders several named files of a book each on their own, in one run whose
 * head and tail happen once. Typst fixtures use `output-ext: typ`, which skips
 * the typst compile, so no typst binary or network is needed.
 */

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tempfile::TempDir;

use quarto_core::format::Format;
use quarto_core::project::ProjectContext;
use quarto_core::project::orchestrator::{
    ProjectPipeline, ProjectRenderSummary, RenderMode, project_type_for,
};
use quarto_core::render_to_file::{RenderToFileOptions, RenderToFileResult};
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

fn canonical(path: &Path) -> PathBuf {
    quarto_system_runtime::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn write(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, contents).unwrap();
}

const TYPST_PROJECT: &str = "project:\n  type: book\nformat:\n  typst:\n    output-ext: typ\nbook:\n  title: Named Book\n  chapters:\n    - index.qmd\n    - one.qmd\n    - two.qmd\n";

fn fixture(project_dir: &Path) {
    write(&project_dir.join("_quarto.yml"), TYPST_PROJECT);
    write(&project_dir.join("index.qmd"), "# Preface\n\nHello.\n");
    write(&project_dir.join("one.qmd"), "# One\n\nFirst.\n");
    // `two` says html: honored in chapter scope.
    write(
        &project_dir.join("two.qmd"),
        "---\nformat: html\n---\n\n# Two\n\nSecond.\n",
    );
    write(&project_dir.join("notes.qmd"), "# Notes\n\nOutside.\n");
}

struct Run {
    _temp: TempDir,
    dir: PathBuf,
    summary: ProjectRenderSummary<RenderToFileResult>,
    shutdowns: usize,
}

/// Render `named` (project-relative) of the typst book. The pipeline's format
/// is the project's (typst), as the CLI passes it for several files.
fn render(named: &[&str], fail_fast: bool, tweak: impl FnOnce(&Path)) -> Run {
    let temp = TempDir::new().unwrap();
    let dir = canonical(temp.path());
    fixture(&dir);
    tweak(&dir);
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let mut project = ProjectContext::discover(&dir, runtime.as_ref()).unwrap();
    let registry = project.registry.clone();
    let options = RenderToFileOptions::default();
    let project_type = project_type_for(&project);
    let set: HashSet<PathBuf> = named.iter().map(|n| dir.join(n)).collect();
    let mut pipeline = ProjectPipeline::new(
        &mut project,
        project_type,
        Format::from_format_string("typst").unwrap(),
        "typst",
        &options,
        runtime,
    )
    .with_fail_fast(fail_fast)
    .with_mode(RenderMode::Subset(set));
    let summary = pollster::block_on(pipeline.run_with_book_support())
        .unwrap_or_else(|e| panic!("render failed: {e}"));
    Run {
        _temp: temp,
        dir,
        summary,
        shutdowns: registry.shutdown_all_call_count(),
    }
}

fn names(run: &Run) -> Vec<String> {
    run.summary
        .outputs
        .iter()
        .map(|o| {
            o.output_path
                .strip_prefix(run.dir.join("_book"))
                .unwrap()
                .to_string_lossy()
                .into_owned()
        })
        .collect()
}

#[test]
fn a_chapter_rendered_alone_shuts_the_engines_down_once() {
    // It was twice: the nested pipeline's `run()` and the orchestrator's tail.
    let run = render(&["one.qmd"], false, |_| {});
    assert_eq!(names(&run), ["one.typ"]);
    assert_eq!(run.shutdowns, 1);
}

#[test]
fn several_files_render_each_on_their_own_in_one_run() {
    // Named out of order, one consolidating and one html, plus a page outside
    // the book.
    let run = render(&["two.qmd", "notes.qmd", "one.qmd"], false, |_| {});
    // `ProjectRenderSummary` lists outputs in `project.files` order.
    assert_eq!(names(&run), ["one.typ", "two.html", "notes.typ"]);
    assert_eq!(run.shutdowns, 1, "one shutdown for the whole run");
    // One manifest lists every output.
    let manifest = std::fs::read_to_string(run.dir.join(".quarto/render-manifest.json")).unwrap();
    for name in ["one.typ", "two.html", "notes.typ"] {
        assert!(manifest.contains(name), "{name} missing from {manifest}");
    }
    // The typst chapter did not merge the book: just its own text.
    let one = std::fs::read_to_string(run.dir.join("_book/one.typ")).unwrap();
    assert!(one.contains("First") && !one.contains("Hello"), "{one}");
    // The typst file got the book's title as project metadata.
    assert!(one.contains("Named Book"), "{one}");
    assert!(!run.summary.has_failures(), "{:?}", run.summary);
}

#[test]
fn a_failing_file_does_not_discard_the_others_and_fail_fast_stops_the_loop() {
    let broke = |dir: &Path| {
        write(
            &dir.join("one.qmd"),
            "---\ntitle: One\n---\n\nLorem ipsum _dolor\n",
        )
    };
    // Best effort: the other typst chapter still renders.
    let run = render(&["index.qmd", "one.qmd"], false, broke);
    assert_eq!(names(&run), ["index.typ"]);
    assert!(run.summary.has_failures());

    // Fail fast: Pass 1 fails on `one`, so the run stops before Pass 2.
    let run = render(&["index.qmd", "one.qmd"], true, broke);
    assert!(run.summary.stopped_early);
    assert!(names(&run).is_empty());
    assert_eq!(run.shutdowns, 1);
}

/// The html pin (5a.5b), at the pipeline level (the CLI half is in
/// `crates/quarto/tests/integration/format_resolution_cli.rs`). The existing
/// `book_project_type::single_chapter_subset_render_keeps_one_file_render_set`
/// pins only the output count. An html chapter renders through the ordinary
/// per-document path and leaves a cross-chapter reference unresolved
/// (`bd-html-chapter-xref-szfxmvyg`); when that strand is fixed this is the
/// test to update.
#[test]
fn an_html_chapter_alone_leaves_a_cross_chapter_reference_unresolved() {
    let temp = TempDir::new().unwrap();
    let dir = canonical(temp.path());
    write(
        &dir.join("_quarto.yml"),
        "project:\n  type: book\nbook:\n  title: H\n  chapters:\n    - index.qmd\n    - one.qmd\n    - two.qmd\n",
    );
    write(
        &dir.join("index.qmd"),
        "# Preface {.unnumbered}\n\nHello.\n",
    );
    write(&dir.join("one.qmd"), "# One\n\nSee @fig-two.\n");
    write(
        &dir.join("two.qmd"),
        "# Two\n\n```{.python}\n#| label: fig-two\n#| fig-cap: Two\nx = 1\n```\n",
    );
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let mut project = ProjectContext::discover(&dir, runtime.as_ref()).unwrap();
    let options = RenderToFileOptions::default();
    let project_type = project_type_for(&project);
    let one = dir.join("one.qmd");
    let mut pipeline = ProjectPipeline::new(
        &mut project,
        project_type,
        Format::html(),
        "html",
        &options,
        runtime,
    )
    .with_mode(RenderMode::Subset(HashSet::from([one])));
    let summary = pollster::block_on(pipeline.run_with_book_support()).unwrap();
    assert_eq!(summary.outputs.len(), 1, "{summary:?}");
    let html = std::fs::read_to_string(dir.join("_book/one.html")).unwrap();
    assert!(
        html.contains("class=\"quarto-xref quarto-unresolved-ref\"") && html.contains("?fig-two?"),
        "{html}"
    );
    let titles: Vec<&str> = summary
        .outputs
        .iter()
        .flat_map(|o| o.render_output.diagnostics.iter())
        .map(|d| d.title.as_str())
        .collect();
    assert!(
        titles.iter().any(|t| t.contains("unresolved crossref")),
        "{titles:?}"
    );
}
