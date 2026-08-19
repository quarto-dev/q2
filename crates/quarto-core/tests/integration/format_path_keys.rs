/*
 * tests/integration/format_path_keys.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Declaration-site resolution for path-shaped format keys beyond
 * `css`: the `include-in-header` / `include-before-body` /
 * `include-after-body` slots and custom `theme` SCSS. bd-oejuizi9,
 * GH #455. Contract: claude-notes/designs/path-resolution-model.md.
 */

//! End-to-end tests for declaration-site path resolution of the
//! include slots and custom themes in project renders.
//!
//! Each test writes a small fixture to a temp dir and drives it
//! through `ProjectPipeline` (the same path `q2 render <project>`
//! uses) — the `render_project` harness mirrors
//! `tests/integration/format_css.rs`.
//!
//! What we pin (plan:
//! `claude-notes/plans/2026-08-19-bd-oejuizi9-declaration-site-path-keys.md`):
//!
//! - a project-level `include-in-header` reaches subdirectory pages
//!   (GH #455's main case), with no Q-5-4;
//! - a leading `/` on an include path anchors at the project root
//!   (contract rule 2 in filesystem space; bd-rdcvjy2s);
//! - a subdirectory `_metadata.yml` include resolves against *its*
//!   directory and applies only to that directory's documents;
//! - a document front-matter include keeps resolving against the
//!   document's own directory (regression guard);
//! - a project-level `theme: [cosmo, custom.scss]` compiles the
//!   custom layer into subdirectory pages' theme CSS.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use tempfile::TempDir;

use quarto_core::format::Format;
use quarto_core::project::ProjectContext;
use quarto_core::project::orchestrator::{ProjectPipeline, ProjectRenderSummary, project_type_for};
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

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {}", path.display(), e))
}

/// Drive a fixture through `ProjectPipeline`. Same harness as
/// `format_css.rs`: returns the project dir (kept alive past return)
/// and the full render summary.
fn render_project(fixture: impl FnOnce(&Path)) -> (PathBuf, ProjectRenderSummary) {
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
    assert!(
        summary.pass1_failures.is_empty() && summary.pass2_failures.is_empty(),
        "unexpected failures: pass1={:?} pass2={:?}",
        summary.pass1_failures,
        summary.pass2_failures
    );

    std::mem::forget(temp);
    (project_dir, summary)
}

/// All `(code, title)` pairs across per-document render diagnostics
/// and project-level diagnostics.
fn all_diagnostics(summary: &ProjectRenderSummary) -> Vec<(Option<String>, String)> {
    summary
        .outputs
        .iter()
        .flat_map(|o| o.render_output.diagnostics.iter())
        .chain(summary.project_diagnostics.iter())
        .map(|d| (d.code.clone(), d.title.clone()))
        .collect()
}

fn assert_no_include_warning(summary: &ProjectRenderSummary, context: &str) {
    let diags = all_diagnostics(summary);
    assert!(
        !diags
            .iter()
            .any(|(code, _)| code.as_deref() == Some("Q-5-4")),
        "{context}: expected no Q-5-4 include-not-found warning; got: {diags:?}"
    );
}

const HEADER_MARKER: &str = "<!-- hdr-455 -->";

/// The GH #455 fixture: project-level `include-in-header`, one root
/// document, one document in a subdirectory.
fn issue_455_fixture(include_entry: &str) -> impl FnOnce(&Path) + '_ {
    move |project_dir: &Path| {
        write(
            &project_dir.join("_quarto.yml"),
            &format!(
                "project:\n  type: website\n  output-dir: _site\n  render:\n    - index.qmd\n    - sub/index.qmd\n\
                 format:\n  html:\n    include-in-header:\n      - {include_entry}\n"
            ),
        );
        write(
            &project_dir.join("custom-header.html"),
            &format!("{HEADER_MARKER}\n"),
        );
        write(
            &project_dir.join("index.qmd"),
            "---\ntitle: Root\n---\n\nHello root.\n",
        );
        write(
            &project_dir.join("sub/index.qmd"),
            "---\ntitle: Sub\n---\n\nHello sub.\n",
        );
    }
}

// ═══════════════════════════════════════════════════════════════════
// GH #455: project-level include-in-header reaches subdir pages
// ═══════════════════════════════════════════════════════════════════

#[test]
fn project_include_in_header_reaches_subdir_pages() {
    let (project_dir, summary) = render_project(issue_455_fixture("custom-header.html"));

    let root_html = read(&project_dir.join("_site/index.html"));
    let sub_html = read(&project_dir.join("_site/sub/index.html"));
    assert!(
        root_html.contains(HEADER_MARKER),
        "root page must carry the project-level header include"
    );
    assert!(
        sub_html.contains(HEADER_MARKER),
        "subdirectory page must carry the project-level header include \
         (GH #455: it was resolved against the consuming doc's dir and dropped)"
    );
    assert_no_include_warning(&summary, "project-level include, both files present");
}

// ═══════════════════════════════════════════════════════════════════
// Leading `/`: project-root anchor in filesystem space (bd-rdcvjy2s)
// ═══════════════════════════════════════════════════════════════════

#[test]
fn rooted_include_in_header_anchors_at_project_root() {
    let (project_dir, summary) = render_project(issue_455_fixture("/custom-header.html"));

    let root_html = read(&project_dir.join("_site/index.html"));
    let sub_html = read(&project_dir.join("_site/sub/index.html"));
    assert!(
        root_html.contains(HEADER_MARKER) && sub_html.contains(HEADER_MARKER),
        "a leading `/` means project-root-relative (contract rule 2), \
         not filesystem-absolute; root={} sub={}",
        root_html.contains(HEADER_MARKER),
        sub_html.contains(HEADER_MARKER)
    );
    assert_no_include_warning(&summary, "rooted include entry");
}

// ═══════════════════════════════════════════════════════════════════
// Directory metadata: declaration-site resolution, directory scoping
// ═══════════════════════════════════════════════════════════════════

#[test]
fn dir_metadata_include_resolves_against_its_own_dir() {
    let (project_dir, summary) = render_project(|project_dir| {
        write(
            &project_dir.join("_quarto.yml"),
            "project:\n  type: website\n  output-dir: _site\n  render:\n    - index.qmd\n    - sub/index.qmd\n",
        );
        write(
            &project_dir.join("sub/_metadata.yml"),
            "format:\n  html:\n    include-in-header:\n      - sub-header.html\n",
        );
        write(
            &project_dir.join("sub/sub-header.html"),
            &format!("{HEADER_MARKER}\n"),
        );
        write(
            &project_dir.join("index.qmd"),
            "---\ntitle: Root\n---\n\nHello root.\n",
        );
        write(
            &project_dir.join("sub/index.qmd"),
            "---\ntitle: Sub\n---\n\nHello sub.\n",
        );
    });

    let root_html = read(&project_dir.join("_site/index.html"));
    let sub_html = read(&project_dir.join("_site/sub/index.html"));
    assert!(
        sub_html.contains(HEADER_MARKER),
        "sub/_metadata.yml include must resolve against sub/ and apply there"
    );
    assert!(
        !root_html.contains(HEADER_MARKER),
        "directory metadata must not leak to documents outside its directory"
    );
    assert_no_include_warning(&summary, "directory-metadata include");
}

// ═══════════════════════════════════════════════════════════════════
// Front matter: document-dir resolution keeps working (guard)
// ═══════════════════════════════════════════════════════════════════

#[test]
fn frontmatter_include_resolves_against_document_dir() {
    let (project_dir, summary) = render_project(|project_dir| {
        write(
            &project_dir.join("_quarto.yml"),
            "project:\n  type: website\n  output-dir: _site\n  render:\n    - index.qmd\n    - sub/index.qmd\n",
        );
        write(
            &project_dir.join("sub/local.html"),
            &format!("{HEADER_MARKER}\n"),
        );
        write(
            &project_dir.join("index.qmd"),
            "---\ntitle: Root\n---\n\nHello root.\n",
        );
        write(
            &project_dir.join("sub/index.qmd"),
            "---\ntitle: Sub\nformat:\n  html:\n    include-in-header:\n      - local.html\n---\n\nHello sub.\n",
        );
    });

    let sub_html = read(&project_dir.join("_site/sub/index.html"));
    assert!(
        sub_html.contains(HEADER_MARKER),
        "front-matter include must keep resolving against the document's dir"
    );
    assert_no_include_warning(&summary, "front-matter include");
}

// ═══════════════════════════════════════════════════════════════════
// Theme: project-level custom SCSS reaches subdir pages
// ═══════════════════════════════════════════════════════════════════

/// Recursively collect every `.css` file under `dir`.
fn css_files_under(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            css_files_under(&path, out);
        } else if path.extension().is_some_and(|e| e == "css") {
            out.push(path);
        }
    }
}

#[test]
fn project_theme_custom_scss_reaches_subdir_pages() {
    let (project_dir, _summary) = render_project(|project_dir| {
        write(
            &project_dir.join("_quarto.yml"),
            "project:\n  type: website\n  output-dir: _site\n  render:\n    - index.qmd\n    - sub/index.qmd\n\
             format:\n  html:\n    theme:\n      - cosmo\n      - custom.scss\n",
        );
        write(
            &project_dir.join("custom.scss"),
            "/*-- scss:rules --*/\nbody { --test-project-theme: 1; }\n",
        );
        write(
            &project_dir.join("index.qmd"),
            "---\ntitle: Root\n---\n\nHello root.\n",
        );
        write(
            &project_dir.join("sub/index.qmd"),
            "---\ntitle: Sub\n---\n\nHello sub.\n",
        );
    });

    // Pre-fix, the subdirectory page resolved `custom.scss` against
    // `sub/` and the theme compile failed the render outright (the
    // harness asserts no pass2 failures), so reaching this point at
    // all is most of the test; the content check pins the rule into
    // the compiled theme CSS.
    let mut css_files = Vec::new();
    css_files_under(&project_dir.join("_site"), &mut css_files);
    let themed: Vec<&PathBuf> = css_files
        .iter()
        .filter(|p| read(p).contains("--test-project-theme"))
        .collect();
    assert!(
        !themed.is_empty(),
        "expected the custom theme layer in some compiled theme CSS under _site/; \
         css files found: {css_files:#?}"
    );
}
