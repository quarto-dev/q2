//! bd-yt2o2ovt: an `{r}` plot on Ark (R's Jupyter kernel) reaches the page.
//!
//! Ark sends `image/png` display data as base64 *without* the trailing `=`
//! padding. The presenter's decoder used to reject that and drop the figure
//! silently. The decoder itself is pinned by unit tests and by a fake-kernel
//! end-to-end test in the lib; this one runs the real kernel through the entry
//! `q2 render` uses.
//!
//! Skips when no `ark` kernelspec is registered, Jupyter is unavailable or `Rscript` is not on `PATH`
//! (CI has neither).

#![cfg(not(target_arch = "wasm32"))]

use std::sync::Arc;

use tempfile::TempDir;

use quarto_core::ProjectContext;
use quarto_core::engine::EngineRegistry;
use quarto_core::engine::jupyter::list_kernelspecs;
use quarto_core::render_to_file::{RenderToFileOptions, render_document_to_file};
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

fn rscript_on_path() -> bool {
    std::process::Command::new("Rscript")
        .arg("--version")
        .output()
        .is_ok()
}

#[test]
fn r_plot_on_ark_writes_a_figure_and_an_img() {
    // The render entry is synchronous and drives its own runtime, so no ambient one here.
    let ark = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(list_kernelspecs())
        .iter()
        .any(|k| k.name == "ark");
    let jupyter = EngineRegistry::default()
        .get("jupyter")
        .is_some_and(|e| e.is_available());
    if !ark || !jupyter || !rscript_on_path() {
        eprintln!(
            "SKIP: no `ark` kernelspec, no Jupyter runtime or no Rscript — r_plot_on_ark_writes_a_figure_and_an_img"
        );
        return;
    }
    let tmp = TempDir::new().unwrap();
    let input = tmp.path().join("ark.qmd");
    std::fs::write(
        &input,
        "---\ntitle: Ark plot\njupyter:\n  kernel: ark\n  claims:\n    r: 1\n---\n\n\
         ```{r}\nplot(1:3)\n```\n",
    )
    .unwrap();

    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let project = ProjectContext::discover(&input, runtime.as_ref()).expect("project discovery");
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
    .unwrap_or_else(|e| panic!("render must succeed: {e}"));

    let html = std::fs::read_to_string(&result.output_path).unwrap();
    assert!(html.contains("<img"), "no <img> in the page");
    assert!(
        html.contains("cell-1-output-1.png"),
        "figure not referenced:\n{html}"
    );
    let figure = tmp.path().join("ark_files/figure-html/cell-1-output-1.png");
    assert!(figure.is_file(), "missing {}", figure.display());
}
