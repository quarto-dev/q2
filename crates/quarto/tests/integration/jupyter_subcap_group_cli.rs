/*
 * tests/integration/jupyter_subcap_group_cli.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * OPR-7j: a stored notebook with a `fig-subcap` group, rendered through the real binary.
 */

//! `q2 render doc.ipynb` presents the stored outputs and needs no kernel, so this runs in CI.
//! A labeled `fig-cap` plus `fig-subcap` cell keeps its group caption, takes the label as its
//! id, and gives panel N the id `label-N`, so `@fig-two` and the panel references resolve.

use std::process::Command;

use tempfile::TempDir;

const Q2_BIN: &str = env!("CARGO_BIN_EXE_q2");

/// The parity corpus notebook: one figure, a two-panel group, a table, and the references.
const NOTEBOOK: &str =
    include_str!("../../../quarto-core/tests/fixtures/opr-parity/jupyter/fig-subcap/doc.ipynb");

#[test]
fn a_stored_notebook_group_keeps_its_caption_and_resolves_its_references() {
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join("doc.ipynb"), NOTEBOOK).unwrap();
    let out = Command::new(Q2_BIN)
        .current_dir(dir.path())
        .args(["render", "doc.ipynb", "--to", "html"])
        .env_remove("QUARTO_PROFILE")
        .output()
        .expect("q2 runs");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "q2 render failed:\n{stderr}");
    assert!(
        !stderr.contains("Unresolved") && !stderr.contains("unresolved"),
        "unresolved crossref:\n{stderr}"
    );

    let html = std::fs::read_to_string(dir.path().join("doc.html")).unwrap();
    for id in ["fig-two", "fig-two-1", "fig-two-2"] {
        assert!(html.contains(&format!("id=\"{id}\"")), "no #{id}:\n{html}");
    }
    assert!(html.contains("A group."), "group caption lost:\n{html}");
    assert!(!html.contains("?fig-two"), "unresolved reference:\n{html}");
}
