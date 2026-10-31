/*
 * tests/integration/knitr_chunk_records.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! EXR-11 (`claude-notes/plans/2026-10-28-exr-11-knitr-k1-chunk-records.md`):
//! knitr chunk records. Phase 11S, S1: an extra `#|` option reaches the
//! chunk hook under its own name, and the hook must not emit it as a
//! `name='value'` attribute on the cell div.
//!
//! Real R and knitr, no mocks; same gate as `knitr_label_reinject.rs`.

#![cfg(not(target_arch = "wasm32"))]

use std::process::Command;
use std::sync::Arc;

use tempfile::TempDir;

use quarto_core::ProjectContext;
use quarto_core::render_to_file::{RenderToFileOptions, render_document_to_file};
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

fn knitr_render_available() -> bool {
    Command::new("Rscript")
        .args([
            "-e",
            "if (!requireNamespace(\"knitr\", quietly = TRUE)) quit(status = 1)",
        ])
        .output()
        .is_ok_and(|o| o.status.success())
}

fn skip(name: &str) -> bool {
    if knitr_render_available() {
        return false;
    }
    eprintln!("SKIP: knitr (Rscript + knitr package) not available — {name}");
    true
}

fn render_html(tmp: &TempDir, source: &str) -> String {
    let input = tmp.path().join("doc.qmd");
    std::fs::write(&input, source).unwrap();
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let project = ProjectContext::discover(&input, runtime.as_ref())
        .expect("project discovery for the chunk-records fixture");
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
    std::fs::read_to_string(&result.output_path).expect("read rendered HTML")
}

/// S1: the id option is not emitted as a div attribute.
#[test]
fn knitr_chunk_id_is_not_emitted_as_an_attribute() {
    if skip("knitr_chunk_id_is_not_emitted_as_an_attribute") {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let html = render_html(
        &tmp,
        "---\ntitle: Id\nengine: knitr\n---\n\n\
```{r}\n\
#| quarto-unit-id: abc123\n\
#| echo: false\n\
1 + 1\n\
```\n",
    );
    assert!(
        !html.contains("quarto-unit-id") && !html.contains("abc123"),
        "id option leaked into the HTML:\n{html}"
    );
}
