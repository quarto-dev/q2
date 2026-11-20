/*
 * tests/integration/loose_list_render.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * End-to-end CLI test: loose list items keep their `<p>` (bd-nnvvomg6).
 */

//! A loose list (blank line between items) is `Para` items in the AST, and
//! Pandoc renders each as `<li><p>…</p></li>`. The HTML writer once unwrapped
//! any single-`Para` item, so `q2 render` lost the paragraph spacing that
//! Pandoc, Quarto 1 and `q2 preview` all show. A tight list must stay bare.

use std::process::Command;

use tempfile::TempDir;

const Q2_BIN: &str = env!("CARGO_BIN_EXE_q2");

#[test]
fn q2_render_keeps_paragraphs_in_loose_list_items() {
    let temp = TempDir::new().unwrap();
    let dir = temp.path();
    std::fs::write(
        dir.join("doc.qmd"),
        "---\ntitle: Lists\n---\n\n\
         Loose:\n\n- loose a\n\n- loose b\n\n\
         Tasks:\n\n- [ ] task a\n\n- [x] task b\n\n\
         Tight:\n\n- tight a\n- tight b\n",
    )
    .unwrap();

    let output = Command::new(Q2_BIN)
        .current_dir(dir)
        .args(["render", "doc.qmd"])
        .output()
        .expect("spawn q2 binary");
    assert!(
        output.status.success(),
        "render failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let html = std::fs::read_to_string(dir.join("doc.html")).expect("doc.html");

    assert!(html.contains("<li><p>loose a</p>"), "loose a: {html}");
    assert!(html.contains("<li><p>loose b</p>"), "loose b: {html}");
    assert!(
        html.contains("<li class=\"task-list-item\"><p><label>"),
        "loose task item keeps its <p>: {html}"
    );
    assert!(html.contains("<li>tight a</li>"), "tight a: {html}");
    assert!(html.contains("<li>tight b</li>"), "tight b: {html}");
}
