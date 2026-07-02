//! End-to-end CLI tests for `q2 render --strict` (bd-yjs54ptg, GH #220).
//!
//! Each test spawns the real `q2` binary as a subprocess, runs
//! `q2 render` against a fixture that produces a warning on an
//! otherwise-successful render (an unresolved crossref), and asserts
//! on severity labeling and exit codes.
//!
//! Contract under verification (per
//! `claude-notes/plans/2026-07-02-strict-mode-warnings-as-errors.md`):
//! - Without `--strict`, warnings print with warning severity and the
//!   command exits 0 (unchanged behavior).
//! - With `--strict`, warning diagnostics are promoted to errors
//!   *before* any output is produced: the text path labels them as
//!   errors, the `--json-errors` path emits `"kind": "error"`, the
//!   counts clause tallies them as errors, and the command exits
//!   non-zero.
//! - A clean render under `--strict` still exits 0.
//!
//! TDD note: these tests are written *before* the implementation and
//! must fail in expected ways before Phase 2 starts (clap rejects the
//! unknown `--strict` flag).

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;
use tempfile::TempDir;

const Q2_BIN: &str = env!("CARGO_BIN_EXE_q2");
const JSON_DIAGNOSTIC_SCHEMA_URL: &str = "https://quarto.org/schemas/v1/json-diagnostic.json";

/// Front matter + an unresolved crossref: renders successfully but
/// emits exactly one warning diagnostic ("unresolved crossref").
const WARNING_DOC: &str = "---\ntitle: Warn\n---\n\nSee @fig-nonexistent for details.\n";

/// A document that renders with no diagnostics at all.
const CLEAN_DOC: &str = "---\ntitle: Clean\n---\n\nNothing to report.\n";

fn write_file(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, contents).unwrap();
}

fn canonical(p: &Path) -> PathBuf {
    p.canonicalize().unwrap_or_else(|_| p.to_path_buf())
}

/// Run `q2 render <args...>` from `cwd`. Returns the exit status and
/// captured stdout / stderr.
fn run_q2_render(cwd: &Path, args: &[&str]) -> std::process::Output {
    let mut cmd = Command::new(Q2_BIN);
    cmd.current_dir(cwd);
    cmd.arg("render");
    for a in args {
        cmd.arg(a);
    }
    cmd.output().expect("spawn q2 binary")
}

/// Parse stderr as NDJSON, keeping only lines that parse as JSON
/// objects (tracing lines may share the channel).
fn parse_ndjson_lines(stderr: &str) -> Vec<Value> {
    stderr
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim_start();
            if !trimmed.starts_with('{') {
                return None;
            }
            serde_json::from_str::<Value>(trimmed).ok()
        })
        .collect()
}

fn is_diagnostic_shape(value: &Value) -> bool {
    value.get("$schema").and_then(|v| v.as_str()) == Some(JSON_DIAGNOSTIC_SCHEMA_URL)
}

// ====================================================================
// Tests
// ====================================================================

/// The flag exists and `--help` advertises it. Guards against silent
/// removal / rename.
#[test]
fn strict_flag_exists() {
    let output = Command::new(Q2_BIN)
        .args(["render", "--help"])
        .output()
        .expect("spawn q2 --help");
    assert!(output.status.success(), "q2 render --help should succeed");
    let help = String::from_utf8_lossy(&output.stdout);
    assert!(
        help.contains("--strict"),
        "q2 render --help should advertise --strict; got:\n{help}"
    );
}

/// Baseline (unchanged behavior): a warning-producing document
/// without `--strict` renders successfully, exits 0, and labels the
/// diagnostic with warning severity.
#[test]
fn warning_without_strict_exits_zero() {
    let temp = TempDir::new().unwrap();
    let dir = canonical(temp.path());
    write_file(&dir.join("warn.qmd"), WARNING_DOC);

    let out_path = dir.join("warn.html");
    let output = run_q2_render(&dir, &["-o", out_path.to_str().unwrap(), "warn.qmd"]);

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "expected exit 0 for a warning without --strict; stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("unresolved crossref"),
        "expected the crossref warning on stderr; got:\n{stderr}"
    );
    assert!(
        stderr.contains("Warning"),
        "expected warning severity labeling without --strict; got:\n{stderr}"
    );
    assert!(
        out_path.exists(),
        "the render itself should have succeeded and written output"
    );
}

/// The core strict-mode contract: the same document with `--strict`
/// exits non-zero, and the diagnostic is labeled as an error
/// everywhere — no warning-severity output remains.
#[test]
fn strict_promotes_warning_and_fails() {
    let temp = TempDir::new().unwrap();
    let dir = canonical(temp.path());
    write_file(&dir.join("warn.qmd"), WARNING_DOC);

    let out_path = dir.join("warn.html");
    let output = run_q2_render(
        &dir,
        &["--strict", "-o", out_path.to_str().unwrap(), "warn.qmd"],
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !output.status.success(),
        "expected non-zero exit under --strict with a warning; stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("unresolved crossref"),
        "the promoted diagnostic should still be reported; got:\n{stderr}"
    );
    assert!(
        stderr.contains("Error"),
        "expected error severity labeling under --strict; got:\n{stderr}"
    );
    assert!(
        !stderr.contains("Warning"),
        "no warning-severity labeling should remain under --strict; got:\n{stderr}"
    );
    assert!(
        stderr.contains("1 error"),
        "the counts clause should tally the promoted warning as an error; got:\n{stderr}"
    );
    assert!(
        !stderr.contains("1 warning"),
        "the counts clause should not report warnings under --strict; got:\n{stderr}"
    );
}

/// Strict mode does not abort the render: even though the command
/// fails, the output file is still produced (render-everything-then-
/// fail semantics; strict changes the outcome, not the control flow).
#[test]
fn strict_still_writes_output() {
    let temp = TempDir::new().unwrap();
    let dir = canonical(temp.path());
    write_file(&dir.join("warn.qmd"), WARNING_DOC);

    let out_path = dir.join("warn.html");
    let output = run_q2_render(
        &dir,
        &["--strict", "-o", out_path.to_str().unwrap(), "warn.qmd"],
    );

    assert!(!output.status.success(), "expected non-zero exit");
    assert!(
        out_path.exists(),
        "strict mode must not suppress the rendered output"
    );
}

/// A clean document under `--strict` exits 0 — strict mode only
/// bites when there is something to promote.
#[test]
fn strict_clean_run_exits_zero() {
    let temp = TempDir::new().unwrap();
    let dir = canonical(temp.path());
    write_file(&dir.join("clean.qmd"), CLEAN_DOC);

    let out_path = dir.join("clean.html");
    let output = run_q2_render(
        &dir,
        &["--strict", "-o", out_path.to_str().unwrap(), "clean.qmd"],
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "expected exit 0 for a clean render under --strict; stderr:\n{stderr}"
    );
}

/// `--strict --json-errors`: the promoted diagnostic crosses the wire
/// as `"kind": "error"` and the command exits non-zero.
#[test]
fn strict_json_errors_kind_is_error() {
    let temp = TempDir::new().unwrap();
    let dir = canonical(temp.path());
    write_file(&dir.join("warn.qmd"), WARNING_DOC);

    let out_path = dir.join("warn.html");
    let output = run_q2_render(
        &dir,
        &[
            "--strict",
            "--json-errors",
            "-o",
            out_path.to_str().unwrap(),
            "warn.qmd",
        ],
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !output.status.success(),
        "expected non-zero exit under --strict; stderr:\n{stderr}"
    );

    let lines = parse_ndjson_lines(&stderr);
    let crossref_diags: Vec<&Value> = lines
        .iter()
        .filter(|v| {
            is_diagnostic_shape(v)
                && v.get("title")
                    .and_then(|t| t.as_str())
                    .is_some_and(|t| t.contains("unresolved crossref"))
        })
        .collect();
    assert!(
        !crossref_diags.is_empty(),
        "expected the crossref diagnostic as a JsonDiagnostic; stderr:\n{stderr}"
    );
    for diag in &crossref_diags {
        assert_eq!(
            diag.get("kind").and_then(|k| k.as_str()),
            Some("error"),
            "promoted diagnostic must cross the wire as kind=error; got:\n{diag:#?}"
        );
    }
}

/// Project render: a warning in one page of an otherwise-clean
/// project exits 0 without `--strict` and non-zero with it.
#[test]
fn strict_project_render_fails_on_warning() {
    let temp = TempDir::new().unwrap();
    let dir = canonical(temp.path());
    write_file(
        &dir.join("_quarto.yml"),
        "project:\n  type: website\n  output-dir: _site\n",
    );
    write_file(&dir.join("index.qmd"), CLEAN_DOC);
    write_file(&dir.join("warn.qmd"), WARNING_DOC);

    let lenient = run_q2_render(&dir, &[]);
    let lenient_stderr = String::from_utf8_lossy(&lenient.stderr);
    assert!(
        lenient.status.success(),
        "expected exit 0 for the project render without --strict; stderr:\n{lenient_stderr}"
    );

    let strict = run_q2_render(&dir, &["--strict"]);
    let strict_stderr = String::from_utf8_lossy(&strict.stderr);
    assert!(
        !strict.status.success(),
        "expected non-zero exit for the project render under --strict; stderr:\n{strict_stderr}"
    );
    assert!(
        strict_stderr.contains("unresolved crossref"),
        "the promoted diagnostic should be reported; got:\n{strict_stderr}"
    );
    assert!(
        strict_stderr.contains("1 error"),
        "the summary line should tally the promoted warning as an error; got:\n{strict_stderr}"
    );
}
