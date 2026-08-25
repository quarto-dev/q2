/*
 * tests/integration/listing_custom_template_diagnostics.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * End-to-end tests for bd-custom-template-not-templated-e5t6m0i0: a
 * `type: custom` listing whose template is a Quarto 1 EJS file must
 * warn (Q-12-9 at config time, Q-12-24 at render time) and must NOT
 * splice the raw template into the page. Mirrors the strand's repro
 * (`repro.qmd` with `welcome-card.ejs` vs. `control.qmd` with a real
 * doctemplate over the same items).
 */

use std::path::PathBuf;
use std::sync::Arc;

use tempfile::TempDir;

use quarto_core::format::Format;
use quarto_core::project::ProjectContext;
use quarto_core::project::orchestrator::{ProjectPipeline, project_type_for};
use quarto_core::render_to_file::{RenderToFileOptions, RenderToFileResult};
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

const PROJECT: &str = "project:\n  type: website\n  render:\n    - \"*.qmd\"\n";

/// Copied byte-for-byte in shape from the Positron site's
/// `welcome-card.ejs`: raw-HTML-wrapped EJS with no `$` anywhere,
/// which compiles as one literal and re-parses cleanly — the exact
/// silent case.
const EJS_TEMPLATE: &str = "```{=html}\n\
<div class=\"custom-card-grid\">\n\
  <% for (const item of items) { %>\n\
    <a href=\"<%= item.link %>\" class=\"custom-card-wrapper\">\n\
      <h3 class=\"custom-card-title\"><%= item.title %></h3>\n\
    </a>\n\
  <% } %>\n\
</div>\n```\n";

const DOCTEMPLATE: &str = "::: {.custom-card-grid}\n\
$for(items)$\n\
::: {.custom-card}\n\
### [$it.title$]($it.path$)\n\
:::\n\
\n\
$endfor$\n\
:::\n";

fn listing_page(template: &str) -> String {
    format!(
        "---\ntitle: Cards\nlisting:\n  id: cards\n  type: custom\n  template: {template}\n  contents: \"item-*.qmd\"\n---\n\nBefore the listing.\n\n::: {{#cards}}\n:::\n"
    )
}

fn write(path: &std::path::Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, contents).unwrap();
}

fn render_project(fixture: impl FnOnce(&std::path::Path)) -> (PathBuf, Vec<RenderToFileResult>) {
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
    assert!(
        summary.pass1_failures.is_empty() && summary.pass2_failures.is_empty(),
        "unexpected failures: pass1={:?} pass2={:?}",
        summary.pass1_failures,
        summary.pass2_failures,
    );
    std::mem::forget(temp);
    (project_dir, summary.outputs)
}

fn html_for(outputs: &[RenderToFileResult], relative_output: &str) -> String {
    let suffix: PathBuf = relative_output.split('/').collect();
    let out = outputs
        .iter()
        .find(|o| o.output_path.ends_with(&suffix))
        .unwrap_or_else(|| panic!("no output ending in `{relative_output}`"));
    std::fs::read_to_string(&out.output_path).unwrap()
}

fn all_diag_codes(outputs: &[RenderToFileResult]) -> Vec<String> {
    outputs
        .iter()
        .flat_map(|o| o.render_output.diagnostics.iter())
        .filter_map(|d| d.code.clone())
        .collect()
}

fn write_items(p: &std::path::Path) {
    write(&p.join("_quarto.yml"), PROJECT);
    write(
        &p.join("item-one.qmd"),
        "---\ntitle: First Item\ndescription: Description of the first item\n---\n\nOne.\n",
    );
    write(
        &p.join("item-two.qmd"),
        "---\ntitle: Second Item\ndescription: Description of the second item\n---\n\nTwo.\n",
    );
}

/// The strand's `repro.qmd`: a Quarto 1 EJS template must not reach
/// the reader, and both diagnostics must fire.
#[test]
fn ejs_custom_template_warns_and_is_not_spliced_into_the_page() {
    let (_dir, outputs) = render_project(|p| {
        write_items(p);
        write(&p.join("welcome-card.ejs"), EJS_TEMPLATE);
        write(&p.join("repro.qmd"), &listing_page("welcome-card.ejs"));
    });
    let html = html_for(&outputs, "repro.html");
    assert!(
        !html.contains("<%"),
        "raw EJS must not be spliced into the page: {html}"
    );
    assert!(
        !html.contains("custom-card-wrapper"),
        "the listing must be skipped, not rendered from the EJS file: {html}"
    );
    let codes = all_diag_codes(&outputs);
    assert!(
        codes.iter().any(|c| c == "Q-12-9"),
        "expected Q-12-9; got {codes:?}"
    );
    assert!(
        codes.iter().any(|c| c == "Q-12-24"),
        "expected Q-12-24; got {codes:?}"
    );
    assert!(
        !codes.iter().any(|c| c == "Q-12-10"),
        "no compile/re-parse diagnostic expected; got {codes:?}"
    );
}

/// The strand's `control.qmd`: a real doctemplate over the same items
/// renders cards and triggers neither diagnostic.
#[test]
fn doctemplate_custom_template_renders_cards_without_diagnostics() {
    let (_dir, outputs) = render_project(|p| {
        write_items(p);
        write(&p.join("card.template"), DOCTEMPLATE);
        write(&p.join("control.qmd"), &listing_page("card.template"));
    });
    let html = html_for(&outputs, "control.html");
    assert!(html.contains("custom-card-grid"), "{html}");
    assert!(
        html.contains("First Item") && html.contains("Second Item"),
        "{html}"
    );
    assert!(html.contains("href=\"item-one.html\""), "{html}");
    let codes = all_diag_codes(&outputs);
    for code in ["Q-12-9", "Q-12-24", "Q-12-10"] {
        assert!(
            !codes.iter().any(|c| c == code),
            "unexpected {code}; got {codes:?}"
        );
    }
}
