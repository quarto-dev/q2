/*
 * tests/link_rewriting_pipeline.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Phase 6 of the website-projects epic: end-to-end body-link
 * rewriting through `ProjectPipeline`. See
 * `claude-notes/plans/2026-04-24-websites-phase-6.md` (tests 39-49).
 */

//! End-to-end body-content link rewriting tests.
//!
//! Drives a real `ProjectPipeline::run` over fixture projects and
//! inspects the rendered HTML's `<a href>` attributes. The unit
//! tests in `transforms::link_rewrite` and
//! `transforms::navigation_href` cover the helper math; these tests
//! exist to catch wiring bugs (resolver not threaded through,
//! transform skipped, page-relative depth wrong, etc.).

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

/// Render a project fixture and return `(project_dir, outputs)`
/// where each output is `(relative_output_path, html_content)`.
/// Relative path is relative to the project's output dir,
/// forward-slash form (e.g. `"index.html"`, `"docs/api.html"`).
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

    // Keep the temp dir alive — outputs are inspected post-render.
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
        .collect();

    (project_dir, outputs, diagnostics)
}

fn find_html<'a>(outputs: &'a [(String, String)], rel: &str) -> &'a str {
    &outputs
        .iter()
        .find(|(p, _)| p == rel)
        .unwrap_or_else(|| {
            panic!(
                "no output for '{}'; got: {:?}",
                rel,
                outputs.iter().map(|(p, _)| p).collect::<Vec<_>>()
            )
        })
        .1
}

/// Body region of the rendered HTML — the `<main class="content">`
/// section that contains the user-authored content. Skips the
/// `<head>`, `<nav>` (navbar), and sidebar regions so assertions
/// about body links don't mistakenly match navigation links.
fn body_region(html: &str) -> &str {
    let start = html.find("<main").unwrap_or(0);
    let end = html[start..]
        .find("</main>")
        .map(|i| start + i)
        .unwrap_or(html.len());
    &html[start..end]
}

// === Plan test 39 =========================================================

/// Body link `[About](about.qmd)` from `index.qmd` rewrites to
/// `about.html`.
#[test]
fn pipeline_body_link_rewrites_simple_qmd() {
    let (_dir, outputs, _) = render_project(|project_dir| {
        write(
            &project_dir.join("_quarto.yml"),
            "project:\n  type: website\n  output-dir: _site\n",
        );
        write(
            &project_dir.join("index.qmd"),
            "---\ntitle: Home\n---\n\n[About me](about.qmd)\n",
        );
        write(
            &project_dir.join("about.qmd"),
            "---\ntitle: About\n---\n\nA.\n",
        );
    });
    let body = body_region(find_html(&outputs, "index.html"));
    assert!(
        body.contains("href=\"about.html\""),
        "expected rewritten body link to about.html, got:\n{}",
        body
    );
    // Negative: the raw .qmd href must not survive.
    assert!(
        !body.contains("href=\"about.qmd\""),
        "raw about.qmd href should not survive in body, got:\n{}",
        body
    );
}

// === Plan test 40 =========================================================

/// `[About](../about.qmd)` from `docs/api.qmd` rewrites to
/// `../about.html` (page-relative depth correct).
#[test]
fn pipeline_body_link_rewrites_doc_relative() {
    let (_dir, outputs, _) = render_project(|project_dir| {
        write(
            &project_dir.join("_quarto.yml"),
            "project:\n  type: website\n  output-dir: _site\n",
        );
        write(
            &project_dir.join("about.qmd"),
            "---\ntitle: About\n---\n\nA.\n",
        );
        write(
            &project_dir.join("docs").join("api.qmd"),
            "---\ntitle: API\n---\n\n[About](../about.qmd)\n",
        );
    });
    let body = body_region(find_html(&outputs, "docs/api.html"));
    assert!(
        body.contains("href=\"../about.html\""),
        "expected ../about.html in nested-page body, got:\n{}",
        body
    );
}

// === Plan test 41 =========================================================

/// `[API](docs/api.qmd)` from `index.qmd` rewrites to
/// `docs/api.html` (subdir descent).
#[test]
fn pipeline_body_link_rewrites_subdir() {
    let (_dir, outputs, _) = render_project(|project_dir| {
        write(
            &project_dir.join("_quarto.yml"),
            "project:\n  type: website\n  output-dir: _site\n",
        );
        write(
            &project_dir.join("index.qmd"),
            "---\ntitle: Home\n---\n\n[API](docs/api.qmd)\n",
        );
        write(
            &project_dir.join("docs").join("api.qmd"),
            "---\ntitle: API\n---\n\nDocs.\n",
        );
    });
    let body = body_region(find_html(&outputs, "index.html"));
    assert!(
        body.contains("href=\"docs/api.html\""),
        "expected docs/api.html in body, got:\n{}",
        body
    );
}

// === Plan test 42 =========================================================

/// Fragment is preserved: `[Bio](about.qmd#bio)` → `about.html#bio`.
#[test]
fn pipeline_body_link_preserves_fragment() {
    let (_dir, outputs, _) = render_project(|project_dir| {
        write(
            &project_dir.join("_quarto.yml"),
            "project:\n  type: website\n  output-dir: _site\n",
        );
        write(
            &project_dir.join("index.qmd"),
            "---\ntitle: Home\n---\n\n[Bio](about.qmd#bio)\n",
        );
        write(
            &project_dir.join("about.qmd"),
            "---\ntitle: About\n---\n\nA.\n",
        );
    });
    let body = body_region(find_html(&outputs, "index.html"));
    assert!(
        body.contains("href=\"about.html#bio\""),
        "expected fragment preserved, got:\n{}",
        body
    );
}

// === Plan test 43 =========================================================

/// Query string is preserved: `[Search](search.qmd?q=foo)` →
/// `search.html?q=foo`.
#[test]
fn pipeline_body_link_preserves_query_string() {
    let (_dir, outputs, _) = render_project(|project_dir| {
        write(
            &project_dir.join("_quarto.yml"),
            "project:\n  type: website\n  output-dir: _site\n",
        );
        write(
            &project_dir.join("index.qmd"),
            "---\ntitle: Home\n---\n\n[Search](search.qmd?q=foo)\n",
        );
        write(
            &project_dir.join("search.qmd"),
            "---\ntitle: Search\n---\n\nS.\n",
        );
    });
    let body = body_region(find_html(&outputs, "index.html"));
    assert!(
        body.contains("href=\"search.html?q=foo\""),
        "expected query string preserved, got:\n{}",
        body
    );
}

// === Plan test 44 =========================================================

/// External URL passes through unchanged.
#[test]
fn pipeline_body_link_external_unchanged() {
    let (_dir, outputs, diags) = render_project(|project_dir| {
        write(
            &project_dir.join("_quarto.yml"),
            "project:\n  type: website\n  output-dir: _site\n",
        );
        write(
            &project_dir.join("index.qmd"),
            "---\ntitle: Home\n---\n\n[GitHub](https://github.com)\n",
        );
    });
    let body = body_region(find_html(&outputs, "index.html"));
    assert!(
        body.contains("href=\"https://github.com\""),
        "expected external URL untouched, got:\n{}",
        body
    );
    assert!(
        !diags.iter().any(|d| d.title.contains("github")
            || d.problem
                .as_ref()
                .map(|p| p.as_str().contains("github"))
                .unwrap_or(false)),
        "external URL should not produce a diagnostic"
    );
}

// === Plan test 45 =========================================================

/// Broken `.qmd` link: href stays as raw `.qmd` and a "Body link"
/// warning is emitted naming the missing target.
#[test]
fn pipeline_body_link_broken_qmd_emits_diagnostic() {
    let (_dir, outputs, diags) = render_project(|project_dir| {
        write(
            &project_dir.join("_quarto.yml"),
            "project:\n  type: website\n  output-dir: _site\n",
        );
        write(
            &project_dir.join("index.qmd"),
            "---\ntitle: Home\n---\n\n[Missing](nope.qmd)\n",
        );
    });
    let body = body_region(find_html(&outputs, "index.html"));
    assert!(
        body.contains("href=\"nope.qmd\""),
        "expected dangling .qmd href preserved, got:\n{}",
        body
    );
    let q_13_4 = diags.iter().find(|d| {
        d.code.as_deref() == Some("Q-13-4")
            && d.problem
                .as_ref()
                .map(|p| p.as_str().contains("nope.qmd"))
                .unwrap_or(false)
    });
    assert!(
        q_13_4.is_some(),
        "expected Q-13-4 'Body link' diagnostic naming nope.qmd; got: {:?}",
        diags
    );
    // bd-c05x6: Q-13-4 should carry a source location pointing at
    // the URL inside `index.qmd`.
    assert!(
        q_13_4.unwrap().location.is_some(),
        "expected Q-13-4 to carry a SourceInfo location; got: {:?}",
        q_13_4.unwrap()
    );
}

// === Plan test 46 =========================================================

/// In a website project where the link target genuinely doesn't
/// resolve (typo in the user's qmd), the helper preserves the
/// dangling href verbatim and emits a "Body link" diagnostic. This
/// is the test 45 contract from a different angle — confirms that
/// the user can see and fix the broken link.
///
/// (Plan test 46's stricter "no diagnostic at all when there's no
/// project context" contract is covered by the
/// `link_rewrite_skips_when_no_index` unit test in
/// `transforms::link_rewrite::tests` and by the
/// `body_href_no_index_passes_through` test in
/// `transforms::navigation_href::tests`. The orchestrator-driven
/// integration path always has a `ProjectIndex`, so a fixture-
/// only test cannot exercise the strictly-standalone branch.)
#[test]
fn pipeline_body_link_unresolvable_in_website_warns() {
    let (_dir, outputs, diags) = render_project(|project_dir| {
        write(
            &project_dir.join("_quarto.yml"),
            "project:\n  type: website\n  output-dir: _site\n",
        );
        write(
            &project_dir.join("doc.qmd"),
            "---\ntitle: Doc\n---\n\n[X](other.qmd)\n",
        );
    });
    let body = body_region(find_html(&outputs, "doc.html"));
    assert!(
        body.contains("href=\"other.qmd\""),
        "expected raw .qmd to survive when target is unknown, got:\n{}",
        body
    );
    let q_13_4 = diags.iter().find(|d| {
        d.code.as_deref() == Some("Q-13-4")
            && d.problem
                .as_ref()
                .map(|p| p.as_str().contains("other.qmd"))
                .unwrap_or(false)
    });
    assert!(
        q_13_4.is_some(),
        "expected Q-13-4 'Body link' diagnostic naming other.qmd; got: {:?}",
        diags
    );
    // bd-c05x6: Q-13-4 should carry a source location pointing at
    // the URL inside `doc.qmd`.
    assert!(
        q_13_4.unwrap().location.is_some(),
        "expected Q-13-4 to carry a SourceInfo location; got: {:?}",
        q_13_4.unwrap()
    );
}

// === Plan test 47 =========================================================

/// Absolute project-root path: `[Home](/index.qmd)` from
/// `docs/api.qmd` rewrites to `../index.html`.
#[test]
fn pipeline_body_link_absolute_path() {
    let (_dir, outputs, _) = render_project(|project_dir| {
        write(
            &project_dir.join("_quarto.yml"),
            "project:\n  type: website\n  output-dir: _site\n",
        );
        write(
            &project_dir.join("index.qmd"),
            "---\ntitle: Home\n---\n\nWelcome.\n",
        );
        write(
            &project_dir.join("docs").join("api.qmd"),
            "---\ntitle: API\n---\n\n[Home](/index.qmd)\n",
        );
    });
    let body = body_region(find_html(&outputs, "docs/api.html"));
    assert!(
        body.contains("href=\"../index.html\""),
        "expected ../index.html, got:\n{}",
        body
    );
}

// === Plan test 48 =========================================================

/// Body link inside a bullet list rewrites correctly. The walker
/// must recurse into list items.
#[test]
fn pipeline_body_link_in_list() {
    let (_dir, outputs, _) = render_project(|project_dir| {
        write(
            &project_dir.join("_quarto.yml"),
            "project:\n  type: website\n  output-dir: _site\n",
        );
        write(
            &project_dir.join("index.qmd"),
            "---\ntitle: Home\n---\n\n- [About](about.qmd)\n- [API](docs/api.qmd)\n",
        );
        write(
            &project_dir.join("about.qmd"),
            "---\ntitle: About\n---\n\nA.\n",
        );
        write(
            &project_dir.join("docs").join("api.qmd"),
            "---\ntitle: API\n---\n\nA.\n",
        );
    });
    let body = body_region(find_html(&outputs, "index.html"));
    assert!(
        body.contains("href=\"about.html\""),
        "list item link about should rewrite, got:\n{}",
        body
    );
    assert!(
        body.contains("href=\"docs/api.html\""),
        "list item link to docs/api should rewrite, got:\n{}",
        body
    );
}

// === Plan test 49 =========================================================

/// Cross-contamination guard: rendering `index.qmd` does not affect
/// `about.qmd`'s body link target. Each doc's rewrite uses its own
/// source-relative basis.
#[test]
fn pipeline_body_link_no_cross_contamination() {
    let (_dir, outputs, _) = render_project(|project_dir| {
        write(
            &project_dir.join("_quarto.yml"),
            "project:\n  type: website\n  output-dir: _site\n",
        );
        write(
            &project_dir.join("index.qmd"),
            "---\ntitle: Home\n---\n\n[About](about.qmd)\n",
        );
        // about.qmd uses a doc-relative `..` from a deeper dir to
        // make sure the source-relative basis is per-doc, not
        // shared with `index.qmd`'s.
        write(
            &project_dir.join("docs").join("api.qmd"),
            "---\ntitle: API\n---\n\n[Home](../index.qmd)\n",
        );
        write(
            &project_dir.join("about.qmd"),
            "---\ntitle: About\n---\n\n[Index](index.qmd)\n",
        );
    });
    let index_body = body_region(find_html(&outputs, "index.html"));
    let about_body = body_region(find_html(&outputs, "about.html"));
    let api_body = body_region(find_html(&outputs, "docs/api.html"));

    assert!(
        index_body.contains("href=\"about.html\""),
        "index.html body should link to about.html"
    );
    assert!(
        about_body.contains("href=\"index.html\""),
        "about.html body should link to index.html"
    );
    assert!(
        api_body.contains("href=\"../index.html\""),
        "docs/api.html body should link to ../index.html"
    );
}
