//! End-to-end CLI tests for `q2 render` exit codes on per-document
//! error diagnostics (bd-zcjtaz78).
//!
//! A render can complete successfully (output file written, no
//! Pass-1/Pass-2 failure) while carrying a `DiagnosticKind::Error`
//! diagnostic — e.g. a duplicate crossref identifier (`Q-15-1`),
//! where the crossref indexer reports the error, skips the
//! duplicate, and continues. Before bd-zcjtaz78 such a render
//! printed `Error: ...` yet exited 0, so CI would report success on
//! output the user was told is broken.
//!
//! Contract under verification: any error-severity diagnostic,
//! wherever it lives in the summary, forces a non-zero exit — with
//! or without `--strict`. The render still completes (the output
//! file is written); only the exit code changes.
//!
//! TDD note: these tests are written *before* the fix and must fail
//! (observed exit 0) before the gate change lands.

use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::TempDir;

const Q2_BIN: &str = env!("CARGO_BIN_EXE_q2");

/// Two tables sharing one crossref id: renders successfully with
/// exactly one error diagnostic (`Q-15-1`) and no warnings.
const DUPLICATE_ID_DOC: &str = "---\ntitle: Duplicate ids\n---\n\n\
| a | b |\n|---|---|\n| 1 | 2 |\n\n: First {#tbl-dup}\n\n\
| c | d |\n|---|---|\n| 3 | 4 |\n\n: Second {#tbl-dup}\n";

fn write_file(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, contents).unwrap();
}

fn canonical(p: &Path) -> PathBuf {
    p.canonicalize().unwrap_or_else(|_| p.to_path_buf())
}

fn run_q2_render(cwd: &Path, args: &[&str]) -> std::process::Output {
    let mut cmd = Command::new(Q2_BIN);
    cmd.current_dir(cwd);
    cmd.arg("render");
    for a in args {
        cmd.arg(a);
    }
    cmd.output().expect("spawn q2 binary")
}

/// The core bd-zcjtaz78 contract: an error diagnostic on an
/// otherwise-successful single-document render exits non-zero even
/// without `--strict`, while the output file is still written.
#[test]
fn error_diagnostic_on_successful_render_exits_nonzero() {
    let temp = TempDir::new().unwrap();
    let dir = canonical(temp.path());
    write_file(&dir.join("dup.qmd"), DUPLICATE_ID_DOC);

    let out_path = dir.join("dup.html");
    let output = run_q2_render(&dir, &["-o", out_path.to_str().unwrap(), "dup.qmd"]);

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Q-15-1"),
        "expected the duplicate-id error diagnostic on stderr; got:\n{stderr}"
    );
    assert!(
        stderr.contains("1 error"),
        "expected the counts clause to report one error; got:\n{stderr}"
    );
    assert!(
        out_path.exists(),
        "the render itself completes; only the exit code changes"
    );
    assert!(
        !output.status.success(),
        "an error diagnostic must force a non-zero exit even without --strict; stderr:\n{stderr}"
    );
}

/// Project-render variant: one page with an error diagnostic in an
/// otherwise-clean project exits non-zero.
#[test]
fn error_diagnostic_in_project_render_exits_nonzero() {
    let temp = TempDir::new().unwrap();
    let dir = canonical(temp.path());
    write_file(
        &dir.join("_quarto.yml"),
        "project:\n  type: website\n  output-dir: _site\n",
    );
    write_file(
        &dir.join("index.qmd"),
        "---\ntitle: Home\n---\n\nAll good here.\n",
    );
    write_file(&dir.join("dup.qmd"), DUPLICATE_ID_DOC);

    let output = run_q2_render(&dir, &[]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Q-15-1"),
        "expected the duplicate-id error diagnostic on stderr; got:\n{stderr}"
    );
    assert!(
        !output.status.success(),
        "a per-page error diagnostic must force a non-zero project exit; stderr:\n{stderr}"
    );
}

/// Regression guard: a completely clean render still exits 0.
#[test]
fn clean_render_still_exits_zero() {
    let temp = TempDir::new().unwrap();
    let dir = canonical(temp.path());
    write_file(
        &dir.join("clean.qmd"),
        "---\ntitle: Clean\n---\n\nNothing to report.\n",
    );

    let out_path = dir.join("clean.html");
    let output = run_q2_render(&dir, &["-o", out_path.to_str().unwrap(), "clean.qmd"]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "clean renders must keep exiting 0; stderr:\n{stderr}"
    );
}
