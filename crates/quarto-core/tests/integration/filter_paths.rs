/*
 * tests/integration/filter_paths.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Declaration-site resolution for the `filters` key. GH #945,
 * bd-nqq41zgx. Contract: claude-notes/designs/path-resolution-model.md.
 */

//! End-to-end tests for declaration-site resolution of user filter
//! paths in project renders (the first consumer of
//! `quarto_core::config_paths::resolve_config_path`).
//!
//! Each test writes a small fixture to a temp dir and drives it
//! through `ProjectPipeline`, the same path `q2 render <project>`
//! uses; the `render_project` harness mirrors
//! `tests/integration/format_path_keys.rs`.
//!
//! What we pin (plan:
//! `claude-notes/plans/2026-10-28-config-path-late-binding.md`, Phase 0):
//!
//! - a project-level `filters: [hello.lua]` runs on subdirectory pages
//!   (GH #945's main case), in both the top-level and the
//!   `format.html.filters` spelling;
//! - a leading `/` anchors at the project root (contract rule 2 in
//!   filesystem space; bd-rdcvjy2s), for every page;
//! - a document front-matter filter keeps resolving against the
//!   document's own directory (regression guard);
//! - a subdirectory `_metadata.yml` filter resolves against *its*
//!   directory and applies only to that directory's documents;
//! - a bare extension name still resolves to the extension's filters
//!   (the existence probe moves, the name lookup stays);
//! - an explicit `!path` entry keeps working (it arrives already
//!   rebased by the merge, and the resolver honours that);
//! - a missing filter fails with the declaration-resolved path, not
//!   the consuming document's directory.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use tempfile::TempDir;

use quarto_core::format::Format;
use quarto_core::project::ProjectContext;
use quarto_core::project::orchestrator::{ProjectPipeline, ProjectRenderSummary, project_type_for};
use quarto_core::render_to_file::RenderToFileOptions;
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

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {}", path.display(), e))
}

/// Drive a fixture through `ProjectPipeline` and return the summary,
/// failures included (the caller decides what a failure means).
fn render_project_raw(fixture: impl FnOnce(&Path)) -> (PathBuf, ProjectRenderSummary) {
    let temp = TempDir::new().unwrap();
    let project_dir = canonical(temp.path());
    fixture(&project_dir);

    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let mut project = ProjectContext::discover(&project_dir, runtime.as_ref()).unwrap();

    let options = RenderToFileOptions::default();
    let project_type = project_type_for(&project);
    let mut pipeline = ProjectPipeline::new(
        &mut project,
        project_type,
        Format::html(),
        "html",
        &options,
        runtime.clone(),
    );
    let summary = pollster::block_on(pipeline.run()).expect("pipeline");

    std::mem::forget(temp);
    (project_dir, summary)
}

/// Same as [`render_project_raw`], asserting the render had no failures.
fn render_project(fixture: impl FnOnce(&Path)) -> (PathBuf, ProjectRenderSummary) {
    let (project_dir, summary) = render_project_raw(fixture);
    assert!(
        summary.pass1_failures.is_empty() && summary.pass2_failures.is_empty(),
        "unexpected failures: pass1={:?} pass2={:?}",
        summary
            .pass1_failures
            .iter()
            .map(|f| (&f.input, &f.error))
            .collect::<Vec<_>>(),
        summary
            .pass2_failures
            .iter()
            .map(|f| (&f.input, &f.error))
            .collect::<Vec<_>>()
    );
    (project_dir, summary)
}

/// The marker a filter appends; `marker("945")` → `FILTER-945-RAN`.
fn marker(tag: &str) -> String {
    format!("FILTER-{tag}-RAN")
}

/// A Lua filter that appends a paragraph carrying `marker(tag)`.
fn filter_source(tag: &str) -> String {
    format!(
        "function Pandoc(doc)\n  doc.blocks:insert(pandoc.Para(pandoc.Str(\"{}\")))\n  return doc\nend\n",
        marker(tag)
    )
}

/// Two-page website: `index.qmd` at the root, `sub/page.qmd` one level
/// down, `hello.lua` at the root. `config_tail` is appended to the
/// `project:` block of `_quarto.yml` verbatim.
fn two_page_site(config_tail: &str) -> impl FnOnce(&Path) + '_ {
    move |project_dir: &Path| {
        write(
            &project_dir.join("_quarto.yml"),
            &format!(
                "project:\n  type: website\n  output-dir: _site\n  render:\n    - index.qmd\n    - sub/page.qmd\n{config_tail}"
            ),
        );
        write(&project_dir.join("hello.lua"), &filter_source("945"));
        write(
            &project_dir.join("index.qmd"),
            "---\ntitle: Root\n---\n\nHello root.\n",
        );
        write(
            &project_dir.join("sub/page.qmd"),
            "---\ntitle: Sub\n---\n\nHello sub.\n",
        );
    }
}

fn assert_filter_ran_everywhere(project_dir: &Path, tag: &str, context: &str) {
    let root_html = read(&project_dir.join("_site/index.html"));
    let sub_html = read(&project_dir.join("_site/sub/page.html"));
    assert!(
        root_html.contains(&marker(tag)),
        "{context}: the root page must carry the filter's marker"
    );
    assert!(
        sub_html.contains(&marker(tag)),
        "{context}: the subdirectory page must carry the filter's marker \
         (GH #945: the project-level filter was resolved against the \
         consuming doc's dir and the page failed to render)"
    );
}

// ═══════════════════════════════════════════════════════════════════
// GH #945: project-level filters reach subdirectory pages
// ═══════════════════════════════════════════════════════════════════

#[test]
fn project_filters_reach_subdir_pages() {
    let (project_dir, _) = render_project(two_page_site("filters:\n  - hello.lua\n"));
    assert_filter_ran_everywhere(&project_dir, "945", "top-level `filters:` in _quarto.yml");
}

#[test]
fn format_html_filters_reach_subdir_pages() {
    let (project_dir, _) = render_project(two_page_site(
        "format:\n  html:\n    filters:\n      - ./hello.lua\n",
    ));
    assert_filter_ran_everywhere(&project_dir, "945", "`format.html.filters` in _quarto.yml");
}

// ═══════════════════════════════════════════════════════════════════
// Leading `/`: project-root anchor in filesystem space (bd-rdcvjy2s)
// ═══════════════════════════════════════════════════════════════════

#[test]
fn rooted_filter_anchors_at_project_root() {
    let (project_dir, _) = render_project(two_page_site("filters:\n  - /hello.lua\n"));
    assert_filter_ran_everywhere(
        &project_dir,
        "945",
        "a leading `/` means project-root-relative (contract rule 2), not filesystem-absolute",
    );
}

// ═══════════════════════════════════════════════════════════════════
// Front matter: still document-relative (regression guard)
// ═══════════════════════════════════════════════════════════════════

#[test]
fn frontmatter_filter_resolves_against_document_dir() {
    let (project_dir, _) = render_project(|project_dir| {
        two_page_site("")(project_dir);
        write(
            &project_dir.join("sub/page.qmd"),
            "---\ntitle: Sub\nfilters:\n  - ../hello.lua\n---\n\nHello sub.\n",
        );
    });
    let root_html = read(&project_dir.join("_site/index.html"));
    let sub_html = read(&project_dir.join("_site/sub/page.html"));
    assert!(
        sub_html.contains(&marker("945")),
        "a front-matter `../hello.lua` resolves against the document's own directory"
    );
    assert!(
        !root_html.contains(&marker("945")),
        "a front-matter filter applies only to its own document"
    );
}

// ═══════════════════════════════════════════════════════════════════
// Directory metadata: declaration-site resolution, directory scoping
// ═══════════════════════════════════════════════════════════════════

#[test]
fn dir_metadata_filter_resolves_against_its_own_dir() {
    let (project_dir, _) = render_project(|project_dir| {
        two_page_site("")(project_dir);
        write(
            &project_dir.join("sub/_metadata.yml"),
            "filters:\n  - local.lua\n",
        );
        write(&project_dir.join("sub/local.lua"), &filter_source("dir"));
    });
    let root_html = read(&project_dir.join("_site/index.html"));
    let sub_html = read(&project_dir.join("_site/sub/page.html"));
    assert!(
        sub_html.contains(&marker("dir")),
        "a `sub/_metadata.yml` filter resolves against `sub/`"
    );
    assert!(
        !root_html.contains(&marker("dir")),
        "directory metadata applies only to that directory's documents"
    );
}

// ═══════════════════════════════════════════════════════════════════
// Extension names and explicit `!path` keep working
// ═══════════════════════════════════════════════════════════════════

#[test]
fn extension_name_filter_still_resolves() {
    let (project_dir, _) = render_project(|project_dir| {
        two_page_site("filters:\n  - fx\n")(project_dir);
        write(
            &project_dir.join("_extensions/fx/_extension.yml"),
            "title: FX\nauthor: Test\ncontributes:\n  filters:\n    - fx.lua\n",
        );
        write(
            &project_dir.join("_extensions/fx/fx.lua"),
            &filter_source("ext"),
        );
    });
    assert_filter_ran_everywhere(
        &project_dir,
        "ext",
        "a bare extension name falls through to the extension's filters on every page",
    );
}

#[test]
fn explicit_path_tag_filter_reaches_subdir_pages() {
    let (project_dir, _) = render_project(two_page_site("filters:\n  - !path hello.lua\n"));
    assert_filter_ran_everywhere(
        &project_dir,
        "945",
        "an explicit `!path` entry is rebased by the merge and honoured as such",
    );
}

// ═══════════════════════════════════════════════════════════════════
// A missing filter is reported at the declaration-resolved path
// ═══════════════════════════════════════════════════════════════════

#[test]
fn missing_project_filter_names_the_declaration_resolved_path() {
    let (project_dir, summary) = render_project_raw(two_page_site("filters:\n  - nope.lua\n"));
    let failures: Vec<&str> = summary
        .pass1_failures
        .iter()
        .chain(summary.pass2_failures.iter())
        .map(|f| f.error.as_str())
        .collect();
    assert!(
        !failures.is_empty(),
        "a filter that does not exist must fail the render"
    );
    let expected = project_dir.join("nope.lua").display().to_string();
    let wrong = project_dir
        .join("sub")
        .join("nope.lua")
        .display()
        .to_string();
    for error in &failures {
        assert!(
            error.contains(&expected) && !error.contains(&wrong),
            "the error must name the declaration-resolved path {expected}, not {wrong}; got: {error}"
        );
    }
}
