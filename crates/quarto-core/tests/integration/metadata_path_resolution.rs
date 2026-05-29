/*
 * tests/metadata_path_resolution.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Integration tests for bd-qor9a: navigation paths declared in a
 * document's frontmatter (or any non-project-root YAML location) must
 * be resolved relative to *where the YAML was written*, not always
 * relative to the project root.
 *
 * Plan: claude-notes/plans/2026-05-20-bd-qor9a-metadata-path-resolution.md
 */

//! End-to-end source-location-driven path resolution for navigation
//! YAML.
//!
//! The user-visible bug fixed here: a sidebar declared in
//! `docs/guide/index.qmd`'s frontmatter with `href: introduction.qmd`
//! used to emit a `Q-13-1` "missing document" warning, because the
//! sidebar generator treated the href as project-root-relative
//! (`<root>/introduction.qmd`) instead of file-relative
//! (`<root>/docs/guide/introduction.qmd`).
//!
//! The fix: navigation parsers retain `ConfigValue.source_info` on
//! every href, and the Generate transforms resolve hrefs against the
//! directory of the originating YAML file before storing
//! `navigation.*`.

use std::path::PathBuf;
use std::sync::Arc;

use tempfile::TempDir;

use quarto_core::format::Format;
use quarto_core::project::ProjectContext;
use quarto_core::project::orchestrator::{ProjectPipeline, project_type_for};
use quarto_core::render_to_file::RenderToFileOptions;
use quarto_error_reporting::DiagnosticMessage;
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

fn canonical(path: &std::path::Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn write(path: &std::path::Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, contents).unwrap();
}

fn html_format() -> Format {
    Format::html()
}

fn runtime_arc() -> Arc<dyn SystemRuntime> {
    Arc::new(NativeRuntime::new())
}

fn read(path: &std::path::Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {}", path.display(), e))
}

/// Render a fixture project and return (project_dir, outputs, diagnostics).
/// outputs is `[(rel_path, html), ...]` with forward-slash paths under
/// the output dir.
fn render_project(
    fixture: impl FnOnce(&std::path::Path),
) -> (PathBuf, Vec<(String, String)>, Vec<DiagnosticMessage>) {
    let temp = TempDir::new().unwrap();
    let project_dir = canonical(temp.path());
    fixture(&project_dir);

    let runtime = runtime_arc();
    let mut project = ProjectContext::discover(&project_dir, runtime.as_ref()).unwrap();

    let options = RenderToFileOptions::default();
    let project_type = project_type_for(&project);
    let output_dir = project.output_dir.clone();
    let mut pipeline = ProjectPipeline::new(
        &mut project,
        project_type,
        html_format(),
        "html",
        &options,
        runtime.clone(),
    );
    let summary = pollster::block_on(pipeline.run()).expect("pipeline");
    assert!(
        summary.pass1_failures.is_empty() && summary.pass2_failures.is_empty(),
        "unexpected failures: pass1={:?} pass2={:?}",
        summary.pass1_failures,
        summary.pass2_failures,
    );

    std::mem::forget(temp);

    let outputs: Vec<(String, String)> = summary
        .outputs
        .iter()
        .map(|out| {
            let rel = out
                .output_path
                .strip_prefix(&output_dir)
                .unwrap_or(&out.output_path)
                .to_string_lossy()
                .replace('\\', "/");
            let html = read(&out.output_path);
            (rel, html)
        })
        .collect();

    let diagnostics: Vec<DiagnosticMessage> = summary
        .outputs
        .iter()
        .flat_map(|o| o.render_output.diagnostics.iter().cloned())
        .chain(summary.project_diagnostics.iter().cloned())
        .collect();

    (project_dir, outputs, diagnostics)
}

fn find_html<'a>(outputs: &'a [(String, String)], rel: &str) -> &'a str {
    &outputs
        .iter()
        .find(|(p, _)| p == rel)
        .unwrap_or_else(|| {
            panic!(
                "no output for `{}`; got: {:?}",
                rel,
                outputs.iter().map(|(p, _)| p).collect::<Vec<_>>()
            )
        })
        .1
}

// === Reproducer test (docs/guide/index.qmd) ===============================

/// The headline reproducer from bd-qor9a: a sidebar declared in a
/// doc's frontmatter, referencing sibling and parent-relative
/// `.qmd` paths. Today these emit Q-13-1; post-fix they resolve
/// correctly and no diagnostic fires.
#[test]
fn frontmatter_sidebar_resolves_sibling_relative_qmd() {
    let (_dir, outputs, diags) = render_project(|project_dir| {
        write(
            &project_dir.join("_quarto.yml"),
            "project:\n  type: website\n  output-dir: _site\n",
        );
        write(
            &project_dir.join("docs/guide/index.qmd"),
            "---\ntitle: Guide\nsidebar:\n  contents:\n    - text: Introduction\n      href: introduction.qmd\n    - text: Markdown\n      href: ../authoring/markdown/index.qmd\n---\n\nGuide intro.\n",
        );
        write(
            &project_dir.join("docs/guide/introduction.qmd"),
            "---\ntitle: Introduction\n---\n\nIntro text.\n",
        );
        write(
            &project_dir.join("docs/authoring/markdown/index.qmd"),
            "---\ntitle: Markdown\n---\n\nMarkdown text.\n",
        );
    });

    // No Q-13-* missing-document diagnostics for the frontmatter
    // sidebar entries.
    let nav_misses: Vec<_> = diags
        .iter()
        .filter(|d| {
            matches!(
                d.code.as_deref(),
                Some("Q-13-1") | Some("Q-13-2") | Some("Q-13-3") | Some("Q-13-7")
            )
        })
        .collect();
    assert!(
        nav_misses.is_empty(),
        "expected no missing-document warnings; got: {:?}",
        nav_misses
    );

    // Sidebar in the rendered guide/index.html points at the resolved
    // sibling and parent-relative paths. The exact href shape depends
    // on page-relativization (the page is `docs/guide/index.html`):
    //   - sibling `introduction.qmd` → `introduction.html` (same dir)
    //   - parent  `../authoring/markdown/index.qmd` →
    //     `../authoring/markdown/index.html` (one up, then descend)
    let guide_html = find_html(&outputs, "docs/guide/index.html");
    assert!(
        guide_html.contains("href=\"introduction.html\""),
        "expected sibling-resolved sidebar link to introduction.html; \
         got body excerpt: {}",
        snippet(guide_html, "quarto-sidebar")
    );
    assert!(
        guide_html.contains("href=\"../authoring/markdown/index.html\""),
        "expected parent-relative sidebar link to ../authoring/markdown/index.html; \
         got body excerpt: {}",
        snippet(guide_html, "quarto-sidebar")
    );
}

// === Regression guard: _quarto.yml-rooted sidebar still works ============

/// `_quarto.yml`-rooted sidebar entries continue to resolve as
/// project-root-relative (today's behaviour). The fix must not
/// over-relativize for the project-config case.
#[test]
fn project_yml_sidebar_keeps_project_root_relative_resolution() {
    let (_dir, _outputs, diags) = render_project(|project_dir| {
        write(
            &project_dir.join("_quarto.yml"),
            "project:\n  type: website\n  output-dir: _site\n\
             website:\n  sidebar:\n    contents:\n      - index.qmd\n      - docs/guide/index.qmd\n",
        );
        write(
            &project_dir.join("index.qmd"),
            "---\ntitle: Home\n---\n\nHello.\n",
        );
        write(
            &project_dir.join("docs/guide/index.qmd"),
            "---\ntitle: Guide\n---\n\nGuide.\n",
        );
    });

    let nav_misses: Vec<_> = diags
        .iter()
        .filter(|d| d.code.as_deref() == Some("Q-13-1"))
        .collect();
    assert!(
        nav_misses.is_empty(),
        "_quarto.yml sidebar should resolve project-root-relative; \
         got Q-13-1 diagnostics: {:?}",
        nav_misses
    );
}

// === Deliberately-broken case: Q-13-1 fires with source location ========

/// When a frontmatter sidebar references a *genuinely* missing
/// document (typo / deleted file), the Q-13-1 warning still fires —
/// but its `location` now points at the YAML scalar that introduced
/// the broken reference (bd-qor9a Phase 4).
#[test]
fn frontmatter_sidebar_missing_document_diagnostic_carries_location() {
    let (project_dir, _outputs, diags) = render_project(|project_dir| {
        write(
            &project_dir.join("_quarto.yml"),
            "project:\n  type: website\n  output-dir: _site\n",
        );
        write(
            &project_dir.join("docs/guide/index.qmd"),
            "---\ntitle: Guide\nsidebar:\n  contents:\n    - text: Missing\n      href: not-here.qmd\n---\n\nGuide.\n",
        );
    });

    let q13_1: Vec<_> = diags
        .iter()
        .filter(|d| d.code.as_deref() == Some("Q-13-1"))
        .collect();
    assert_eq!(
        q13_1.len(),
        1,
        "expected exactly one Q-13-1 for the missing href; got: {:?}",
        diags
    );
    let d = q13_1[0];
    // Location must be populated (was None pre-bd-qor9a).
    assert!(
        d.location.is_some(),
        "Q-13-1 must carry a SourceInfo location pointing at the YAML; \
         got: {:?}",
        d
    );
    // The problem text should still name the missing path.
    let _ = project_dir; // path of the guide file is in source_context, not asserted here
    assert!(
        d.problem
            .as_ref()
            .map(|p| p.as_str().contains("not-here.qmd"))
            .unwrap_or(false),
        "Q-13-1 problem must mention the missing path; got {:?}",
        d.problem
    );
}

// ----- helpers ------------------------------------------------------------

fn snippet<'a>(html: &'a str, needle: &str) -> &'a str {
    match html.find(needle) {
        Some(i) => {
            let start = i.saturating_sub(40);
            let end = (i + 800).min(html.len());
            &html[start..end]
        }
        None => &html[..html.len().min(400)],
    }
}
