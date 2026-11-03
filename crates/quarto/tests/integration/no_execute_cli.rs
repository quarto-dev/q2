/*
 * tests/integration/no_execute_cli.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Issue #1008 (bd-2luq2ath): `q2 render --no-execute` through the real binary.
 */

//! `--no-execute` renders a document without running any of its code
//! cells, so a document whose cells would fail (or whose engine is not
//! installed) still renders. The fixture's cells fail when executed, which
//! keeps these tests independent of whether R or Python is installed: an
//! executing render fails either way (the cell errors, or the runtime is
//! missing), and only a render that skips execution succeeds.

use std::path::Path;
use std::process::{Command, Output};

use tempfile::TempDir;

const Q2_BIN: &str = env!("CARGO_BIN_EXE_q2");

const FAILING_CELLS: &str = "---\ntitle: No execute\n---\n\n\
```{r}\nstop(\"the r cell ran\")\n```\n\n\
```{python}\nraise SystemExit(\"the python cell ran\")\n```\n";

fn doc_with_failing_cells() -> TempDir {
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join("doc.qmd"), FAILING_CELLS).unwrap();
    dir
}

fn q2_render(dir: &Path, args: &[&str]) -> Output {
    Command::new(Q2_BIN)
        .current_dir(dir)
        .arg("render")
        .args(args)
        .env_remove("QUARTO_PROFILE")
        .output()
        .expect("q2 runs")
}

#[test]
fn no_execute_renders_without_running_cells() {
    let dir = doc_with_failing_cells();
    let out = q2_render(dir.path(), &["doc.qmd", "--no-execute"]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "q2 render failed:\n{stderr}");
    // A skipped engine is silent: no runtime-missing warning, no cell error.
    assert!(!stderr.contains("ran"), "a cell ran:\n{stderr}");
    assert!(
        !stderr.contains("not available"),
        "engine probed:\n{stderr}"
    );

    let html = std::fs::read_to_string(dir.path().join("doc.html")).unwrap();
    // The cells are shown as code, with no output.
    assert!(html.contains("stop(&quot;the r cell ran&quot;)"), "{html}");
    assert!(
        html.contains("raise SystemExit(&quot;the python cell ran&quot;)"),
        "{html}"
    );
    assert!(
        !html.contains("cell-output"),
        "cell output leaked into:\n{html}"
    );
}

#[test]
fn a_later_execute_cancels_no_execute() {
    // The last of the pair wins, so this render executes and fails.
    let dir = doc_with_failing_cells();
    let out = q2_render(dir.path(), &["doc.qmd", "--no-execute", "--execute"]);
    assert!(
        !out.status.success(),
        "expected the executing render to fail:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
}
