//! bd-gy2ozix3 / GH #683 — a knitr document whose R output attaches an
//! HTML dependency must render, and the dependency must reach the page.
//!
//! Reported with `reactable::reactable(...)` and `DT::datatable(...)`, but
//! the trigger is any `htmltools::htmlDependency` — htmlwidgets are just
//! the common way to produce one. The fixture here uses `htmltools`
//! directly, which `rmarkdown` already requires, so the test needs no
//! widget package. (An `href`-only dependency is *not* usable: rmarkdown
//! rejects it with `Dependency ... is not disk-based`.)
//!
//! Failure mode being bound: `execute.R`'s `create_pandoc_includes` wraps
//! each include path in `I()`, so the results JSON carries
//! `"include-in-header": ["<path>"]` — an array, matching Quarto 1's
//! `string[]` type. Typing the slot as a single path in Rust makes every
//! such render die in `call_r` with
//! `Failed to parse R results: invalid type: sequence, expected path string`.
//!
//! Renders go through `render_document_to_file`, the entry `q2 render`
//! uses. Gate copied from `nested_cell_mask_render.rs` (binary + `knitr`
//! package); a skip is a signal about the machine, not a pass.

#![cfg(not(target_arch = "wasm32"))]

use std::path::Path;
use std::process::Command;
use std::sync::Arc;

use tempfile::TempDir;

use quarto_core::ProjectContext;
use quarto_core::render_to_file::{RenderToFileOptions, render_document_to_file};
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

fn rscript_available() -> bool {
    Command::new("Rscript")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

fn r_packages_available(packages: &[&str]) -> bool {
    let expr = packages
        .iter()
        .map(|p| format!("if (!requireNamespace(\"{p}\", quietly = TRUE)) quit(status = 1)"))
        .collect::<Vec<_>>()
        .join("; ");
    Command::new("Rscript")
        .args(["-e", &expr])
        .output()
        .is_ok_and(|o| o.status.success())
}

fn knitr_render_available() -> bool {
    rscript_available() && r_packages_available(&["knitr", "rmarkdown", "htmltools"])
}

/// A live `{r}` cell that attaches a disk-based `htmlDependency` named
/// `q2dep` (version 1.0, one script) to a plain `<div>`.
const FIXTURE: &str = "---\nformat: html\nengine: knitr\n---\n\n\
```{r}\n\
htmltools::attachDependencies(\n\
  htmltools::div(\"hi\"),\n\
  htmltools::htmlDependency(\"q2dep\", \"1.0\", src = \"deps\", script = \"q2dep.js\")\n\
)\n\
```\n";

/// Write the fixture and its `deps/q2dep.js` next to each other, render
/// through the real per-document path, and return the rendered HTML plus
/// the output path (the `_files` directory sits beside it).
fn render_fixture(tmp: &TempDir) -> Result<(String, std::path::PathBuf), String> {
    let input = tmp.path().join("htmldep.qmd");
    std::fs::write(&input, FIXTURE).unwrap();
    let deps = tmp.path().join("deps");
    std::fs::create_dir_all(&deps).unwrap();
    std::fs::write(deps.join("q2dep.js"), "window.q2dep = true;\n").unwrap();

    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let project = ProjectContext::discover(&input, runtime.as_ref())
        .expect("project discovery for the html-dependency fixture");

    let result = render_document_to_file(
        &input,
        "html",
        &RenderToFileOptions::default(),
        Some(&project),
        runtime.clone(),
        None,
        None,
        None,
    )
    .map_err(|e| e.to_string())?;

    let html = std::fs::read_to_string(&result.output_path).expect("read rendered HTML");
    Ok((html, result.output_path))
}

fn skip(name: &str) -> bool {
    if knitr_render_available() {
        return false;
    }
    eprintln!("SKIP: knitr (Rscript + knitr/rmarkdown/htmltools) not available — {name}");
    true
}

/// The render must succeed at all. Before the fix it fails inside
/// `call_r` while deserializing the results file.
#[test]
fn html_dependency_document_renders() {
    if skip("html_dependency_document_renders") {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let outcome = render_fixture(&tmp);
    assert!(
        outcome.is_ok(),
        "a document attaching an htmlDependency must render; error:\n{}",
        outcome.unwrap_err()
    );
}

/// The dependency's `<script>` tag must be in the page, pointing at the
/// `<stem>_files/<name>-<version>/<script>` path rmarkdown lays out, and
/// that file must exist beside the output so the reference resolves.
#[test]
fn html_dependency_script_reaches_the_page_and_disk() {
    if skip("html_dependency_script_reaches_the_page_and_disk") {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (html, output_path) = render_fixture(&tmp).unwrap_or_else(|e| panic!("render failed: {e}"));

    let expected_tag = "<script src=\"htmldep_files/q2dep-1.0/q2dep.js\"></script>";
    assert!(
        html.contains(expected_tag),
        "rendered HTML must carry the dependency script tag {expected_tag:?}; html:\n{html}"
    );

    let on_disk: &Path = &output_path
        .parent()
        .unwrap()
        .join("htmldep_files")
        .join("q2dep-1.0")
        .join("q2dep.js");
    assert!(
        on_disk.is_file(),
        "dependency script must be copied beside the output: {}",
        on_disk.display()
    );
}
