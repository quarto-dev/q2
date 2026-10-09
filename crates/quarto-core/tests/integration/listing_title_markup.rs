/*
 * tests/integration/listing_title_markup.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * End-to-end tests for bd-8a9eum6p: a listing item's title, subtitle
 * and description are markdown, and must reach the listing with their
 * markup intact. They used to be flattened to plain text and then
 * re-parsed as markdown, so formatting was lost and markdown-significant
 * plain text (`_scope`, `<anonymous>`) broke the re-parse and dropped the
 * whole listing. Plan:
 * `claude-notes/plans/2026-10-09-listing-title-reparse.md`.
 */

use std::path::PathBuf;
use std::sync::Arc;

use tempfile::TempDir;

use quarto_core::format::Format;
use quarto_core::project::ProjectContext;
use quarto_core::project::orchestrator::{ProjectPipeline, ProjectRenderSummary, project_type_for};
use quarto_core::render_to_file::RenderToFileOptions;
use quarto_error_reporting::{DiagnosticKind, DiagnosticMessage};
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

const PROJECT: &str = "project:\n  type: website\n";

/// One table listing and one default listing over the same items, so
/// both the pre-rendered table-row path and the `$title$` template path
/// are exercised.
const INDEX: &str = "---\ntitle: Home\nlisting:\n  - id: tbl\n    type: table\n    contents: \"p/*.qmd\"\n    fields: [title, subtitle, description]\n  - id: dflt\n    type: default\n    contents: \"p/*.qmd\"\n  - id: grd\n    type: grid\n    contents: \"p/*.qmd\"\n---\n";

fn write(path: &std::path::Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, contents).unwrap();
}

fn page(front_matter: &str) -> String {
    format!("---\n{front_matter}\n---\n\nBody.\n")
}

/// Render a fixture project; returns the summary (outputs *and*
/// failures — an error-severity diagnostic may land in either).
fn render(fixture: impl FnOnce(&std::path::Path)) -> ProjectRenderSummary {
    let temp = TempDir::new().unwrap();
    let project_dir = temp
        .path()
        .canonicalize()
        .unwrap_or_else(|_| temp.path().to_path_buf());
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
    summary
}

fn html_for(summary: &ProjectRenderSummary, relative_output: &str) -> String {
    let suffix: PathBuf = relative_output.split('/').collect();
    let out = summary
        .outputs
        .iter()
        .find(|o| o.output_path.ends_with(&suffix))
        .unwrap_or_else(|| panic!("no output ending in `{relative_output}`"));
    std::fs::read_to_string(&out.output_path).unwrap()
}

fn all_diags(summary: &ProjectRenderSummary) -> Vec<DiagnosticMessage> {
    summary
        .outputs
        .iter()
        .flat_map(|o| o.render_output.diagnostics.iter().cloned())
        .chain(
            summary
                .pass1_failures
                .iter()
                .chain(&summary.pass2_failures)
                .flat_map(|f| f.diagnostics.iter().cloned()),
        )
        .collect()
}

fn codes(diags: &[DiagnosticMessage]) -> Vec<String> {
    diags.iter().filter_map(|d| d.code.clone()).collect()
}

/// Split the host page into the table, default and grid listings' HTML.
fn listing_sections(html: &str) -> (&str, &str, &str) {
    let tbl = html.find("id=\"tbl\"").expect("table listing present");
    let dflt = html.find("id=\"dflt\"").expect("default listing present");
    let grd = html.find("id=\"grd\"").expect("grid listing present");
    assert!(tbl < dflt && dflt < grd, "listings out of order");
    (&html[tbl..dflt], &html[dflt..grd], &html[grd..])
}

fn write_markup_items(p: &std::path::Path) {
    write(&p.join("_quarto.yml"), PROJECT);
    write(&p.join("index.qmd"), INDEX);
    // Backslash-escaped underscore: the title's *plain text* is `_scope`,
    // which opens underscore emphasis when re-parsed as markdown.
    write(
        &p.join("p/a.qmd"),
        &page("title: 'Fix \\_scope: lexical regression'"),
    );
    // Code span and emphasis; the code span's content is markdown-significant.
    write(
        &p.join("p/b.qmd"),
        &page(
            "title: 'Plan for `_scope` and *emph*'\nsubtitle: 'Sub with `code`'\ndescription: 'Desc with **strong** and `<anonymous>`'",
        ),
    );
    // Raw-HTML-looking text inside a code span.
    write(
        &p.join("p/c.qmd"),
        &page("title: 'About `<anonymous>` frames'"),
    );
    // A pipe inside a code span must not split the table cell, and a
    // plain-text pipe must not either.
    write(&p.join("p/d.qmd"), &page("title: 'Pipes `a|b` and c|d'"));
}

#[test]
fn listing_keeps_title_subtitle_and_description_markup() {
    let summary = render(write_markup_items);
    let diags = all_diags(&summary);
    let codes = codes(&diags);
    assert!(
        !codes.iter().any(|c| c == "Q-12-10"),
        "listing re-parse must be clean; got {diags:#?}"
    );
    let html = html_for(&summary, "index.html");
    let (tbl, dflt, grd) = listing_sections(&html);

    for (name, section) in [("table", tbl), ("default", dflt), ("grid", grd)] {
        for needle in [
            "Fix _scope: lexical regression",
            "Plan for <code>_scope</code> and <em>emph</em>",
            "About <code>&lt;anonymous&gt;</code> frames",
        ] {
            assert!(
                section.contains(needle),
                "{name} listing is missing `{needle}`:\n{section}"
            );
        }
        assert!(
            !section.contains("<anonymous>"),
            "{name} listing leaked `<anonymous>` as raw HTML:\n{section}"
        );
    }
    // Subtitles appear in the table (explicit field) and the default layout.
    assert!(tbl.contains("Sub with <code>code</code>"), "{tbl}");
    assert!(dflt.contains("Sub with <code>code</code>"), "{dflt}");
    assert!(grd.contains("Sub with <code>code</code>"), "{grd}");
    // Descriptions: checked in the table only. The default layout
    // currently replaces a document's explicit description with the
    // derived first paragraph (bd-listing-description-precedence-x4bh6w3m);
    // `inline_record_prose_keeps_markup` covers the template path.
    assert!(
        tbl.contains("Desc with <strong>strong</strong> and <code>&lt;anonymous&gt;</code>"),
        "{tbl}"
    );
    // The pipe inside the code span stays inside one cell.
    assert!(
        tbl.contains("Pipes <code>a|b</code> and c|d"),
        "pipe handling in table cell:\n{tbl}"
    );
}

/// Inline `contents:` records carry prose too, and are written through
/// the default template's `$title$` / `$subtitle$` / `$description$`.
#[test]
fn inline_record_prose_keeps_markup() {
    let summary = render(|p| {
        write(&p.join("_quarto.yml"), PROJECT);
        write(
            &p.join("index.qmd"),
            "---\ntitle: Home\nlisting:\n  id: recs\n  type: default\n  contents:\n    - title: 'Record `_r` and *emph*'\n      subtitle: 'Sub `<x>`'\n      description: 'Rec **desc** with \\_x'\n---\n",
        );
    });
    let diags = all_diags(&summary);
    assert!(
        !codes(&diags).iter().any(|c| c == "Q-12-10"),
        "listing re-parse must be clean; got {diags:#?}"
    );
    let html = html_for(&summary, "index.html");
    for needle in [
        "Record <code>_r</code> and <em>emph</em>",
        "Sub <code>&lt;x&gt;</code>",
        "Rec <strong>desc</strong> with _x",
    ] {
        assert!(html.contains(needle), "missing `{needle}`:\n{html}");
    }
}

/// D5: a multi-paragraph description is flattened into one run of
/// inlines (it used to vanish silently) and draws a warning.
#[test]
fn multi_paragraph_description_is_flattened_with_a_warning() {
    let summary = render(|p| {
        write(&p.join("_quarto.yml"), PROJECT);
        write(&p.join("index.qmd"), INDEX);
        write(
            &p.join("p/a.qmd"),
            &page("title: Two paras\ndescription: |\n  First *para*.\n\n  Second para."),
        );
    });
    let diags = all_diags(&summary);
    let html = html_for(&summary, "index.html");
    let (tbl, _, _) = listing_sections(&html);
    assert!(
        tbl.contains("First <em>para</em>. Second para."),
        "description flattened into one cell:\n{tbl}"
    );
    let warning = diags
        .iter()
        .find(|d| d.code.as_deref() == Some("Q-12-26"))
        .unwrap_or_else(|| panic!("expected Q-12-26; got {diags:#?}"));
    assert_eq!(warning.kind, DiagnosticKind::Warning);
}

/// D5 for inline records: the record is in the host page's own front
/// matter, so the warning can point at the value.
#[test]
fn multi_paragraph_record_description_warns_with_a_location() {
    let summary = render(|p| {
        write(&p.join("_quarto.yml"), PROJECT);
        write(
            &p.join("index.qmd"),
            "---\ntitle: Home\nlisting:\n  id: recs\n  type: default\n  contents:\n    - title: Card\n      description: |\n        First.\n\n        Second.\n---\n",
        );
    });
    let diags = all_diags(&summary);
    let warning = diags
        .iter()
        .find(|d| d.code.as_deref() == Some("Q-12-26"))
        .unwrap_or_else(|| panic!("expected Q-12-26; got {diags:#?}"));
    assert_eq!(warning.kind, DiagnosticKind::Warning);
    assert!(warning.location.is_some(), "{warning:#?}");
    let html = html_for(&summary, "index.html");
    assert!(html.contains("First. Second."), "{html}");
}

/// D4: a listing whose generated markdown fails to re-parse is an
/// error, not a warning — the listing is skipped, and that must not
/// pass silently. The failure here comes from the template text itself
/// (an unclosed `_`), since item data can no longer cause one.
#[test]
fn failed_listing_reparse_is_an_error() {
    let summary = render(|p| {
        write(&p.join("_quarto.yml"), PROJECT);
        write(
            &p.join("broken.template"),
            "::: {.cards}\n$for(items)$\n_$it.title$\n\n$endfor$\n:::\n",
        );
        write(
            &p.join("index.qmd"),
            "---\ntitle: Home\nlisting:\n  id: cards\n  type: custom\n  template: broken.template\n  contents: \"p/*.qmd\"\n---\n",
        );
        write(&p.join("p/a.qmd"), &page("title: Plain"));
    });
    let diags = all_diags(&summary);
    let reparse = diags
        .iter()
        .find(|d| d.code.as_deref() == Some("Q-12-10"))
        .unwrap_or_else(|| panic!("expected Q-12-10; got {diags:#?}"));
    assert_eq!(
        reparse.kind,
        DiagnosticKind::Error,
        "a failed listing re-parse must be an error: {reparse:#?}"
    );
}
