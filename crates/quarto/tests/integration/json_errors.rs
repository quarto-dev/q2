//! End-to-end CLI tests for `q2 render --json-errors` (bd-iey8o).
//!
//! Each test spawns the real `q2` binary as a subprocess, runs
//! `q2 render` against a fixture, and asserts on the structure of
//! the diagnostics emitted to stderr.
//!
//! Wire contract under verification (per the plan in
//! `claude-notes/plans/2026-05-22-q2-render-json-errors.md`):
//! - All diagnostics emitted as NDJSON on stderr (one JSON object
//!   per line).
//! - Stdout is unaffected by the flag (output files still go to
//!   disk).
//! - Each emitted line carries a `$schema` field pointing at the
//!   appropriate wire-shape schema URL.
//! - `JsonDiagnostic` shape for per-page and project-level
//!   diagnostics; `JsonPass1Failure` shape for sibling-page Pass-1
//!   failures (mixed line shapes are intentional — see plan).
//!
//! TDD note: these tests are written *before* the implementation
//! and must fail in expected ways before Phase 2 starts.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;
use tempfile::TempDir;

const Q2_BIN: &str = env!("CARGO_BIN_EXE_q2");
const JSON_DIAGNOSTIC_SCHEMA_URL: &str = "https://quarto.org/schemas/v1/json-diagnostic.json";
const JSON_PASS1_FAILURE_SCHEMA_URL: &str = "https://quarto.org/schemas/v1/json-pass1-failure.json";

fn write_file(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, contents).unwrap();
}

fn canonical(p: &Path) -> PathBuf {
    p.canonicalize().unwrap_or_else(|_| p.to_path_buf())
}

/// Run `q2 render <args...>` from `cwd`. Returns the exit status
/// and captured stdout / stderr.
fn run_q2_render(cwd: &Path, args: &[&str]) -> std::process::Output {
    let mut cmd = Command::new(Q2_BIN);
    cmd.current_dir(cwd);
    cmd.arg("render");
    for a in args {
        cmd.arg(a);
    }
    cmd.output().expect("spawn q2 binary")
}

/// Parse stderr as NDJSON. Returns one `serde_json::Value` per
/// line that successfully parses as a JSON object; non-JSON lines
/// (e.g. tracing lines) are skipped.
///
/// Under `--json-errors` we expect *all* diagnostic output to be
/// JSON objects, but logger output (tracing) may share the
/// channel — the contract is "JSON lines exist," not "every line
/// is JSON."
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

/// True if `value` claims the `JsonDiagnostic` schema via its
/// `$schema` field.
fn is_diagnostic_shape(value: &Value) -> bool {
    value.get("$schema").and_then(|v| v.as_str()) == Some(JSON_DIAGNOSTIC_SCHEMA_URL)
}

/// True if `value` claims the `JsonPass1Failure` schema.
fn is_pass1_failure_shape(value: &Value) -> bool {
    value.get("$schema").and_then(|v| v.as_str()) == Some(JSON_PASS1_FAILURE_SCHEMA_URL)
}

// ====================================================================
// Tests
// ====================================================================

/// Phase 1 test: the flag exists and `--help` mentions it.
///
/// This guards against silent removal / rename of the flag.
#[test]
fn json_errors_flag_exists() {
    let output = Command::new(Q2_BIN)
        .args(["render", "--help"])
        .output()
        .expect("spawn q2 --help");
    assert!(output.status.success(), "q2 render --help should succeed");
    let help = String::from_utf8_lossy(&output.stdout);
    assert!(
        help.contains("--json-errors"),
        "q2 render --help should advertise --json-errors; got:\n{help}"
    );
}

/// Phase 1 test: a single-document parse error emits a structured
/// diagnostic with kind=error and a present location somewhere in
/// the NDJSON stream.
///
/// The error appears at one of two levels (both are accepted):
///   - top-level `JsonDiagnostic`, or
///   - nested in a `JsonPass1Failure.diagnostics[]` (the common
///     case for parse failures, since single-doc renders also flow
///     through Pass-1 via the project pipeline).
///
/// The contract under test is "agents can find a typed, located
/// error diagnostic in the stream" — the wrapper level is not
/// load-bearing for this assertion.
///
/// Uses an unclosed code fence — same trigger pampa's
/// test_json_errors_flag_with_error uses.
#[test]
fn single_doc_parse_error_json() {
    let temp = TempDir::new().unwrap();
    let dir = canonical(temp.path());
    let fixture = dir.join("bad.qmd");
    // Unclosed code fence; this should produce a parse error.
    write_file(&fixture, "---\ntitle: Bad\n---\n\n```{python\n");

    let out_path = dir.join("bad.html");
    let output = run_q2_render(
        &dir,
        &["--json-errors", "-o", out_path.to_str().unwrap(), "bad.qmd"],
    );
    assert!(
        !output.status.success(),
        "expected non-zero exit on parse error; stderr:\n{}",
        String::from_utf8_lossy(&output.stderr),
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    let lines = parse_ndjson_lines(&stderr);
    assert!(
        !lines.is_empty(),
        "expected at least one JSON diagnostic on stderr; stderr was:\n{stderr}"
    );

    let any_error = lines.iter().any(any_located_error_diagnostic);
    assert!(
        any_error,
        "expected at least one located error diagnostic (flat or nested in a JsonPass1Failure); got lines:\n{lines:#?}"
    );
}

/// True when `v` is a `JsonDiagnostic` with kind=error and a
/// `start_line` — either at the top level or nested inside a
/// `JsonPass1Failure.diagnostics[]`.
fn any_located_error_diagnostic(v: &Value) -> bool {
    let looks_like_error = |d: &Value| {
        d.get("kind").and_then(|k| k.as_str()) == Some("error") && d.get("start_line").is_some()
    };
    if is_diagnostic_shape(v) && looks_like_error(v) {
        return true;
    }
    if is_pass1_failure_shape(v)
        && let Some(arr) = v.get("diagnostics").and_then(|d| d.as_array())
    {
        return arr.iter().any(looks_like_error);
    }
    false
}

/// Phase 1 test: a single-document metadata warning emits JSON on
/// stderr with kind=warning, and the run still succeeds.
///
/// Uses an incomplete-link description — same pattern as pampa's
/// test_json_errors_flag_with_warning.
#[test]
fn single_doc_warning_json() {
    let temp = TempDir::new().unwrap();
    let dir = canonical(temp.path());
    let fixture = dir.join("warn.qmd");
    write_file(
        &fixture,
        "---\ntitle: Warn\ndescription: \"[incomplete link\"\n---\n\nBody.\n",
    );

    let out_path = dir.join("warn.html");
    let output = run_q2_render(
        &dir,
        &[
            "--json-errors",
            "-o",
            out_path.to_str().unwrap(),
            "warn.qmd",
        ],
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    let lines = parse_ndjson_lines(&stderr);
    let any_warning = lines.iter().any(|v| {
        is_diagnostic_shape(v) && v.get("kind").and_then(|k| k.as_str()) == Some("warning")
    });
    assert!(
        any_warning,
        "expected at least one JsonDiagnostic with kind=warning; stderr:\n{stderr}\nparsed: {lines:#?}"
    );
}

/// Phase 1 test: project mode with a Pass-1 failure in a sibling
/// page emits a `JsonPass1Failure` shape (with `source_file`
/// tagged), distinct from the `JsonDiagnostic` shape.
#[test]
fn project_pass1_failure_json() {
    let temp = TempDir::new().unwrap();
    let dir = canonical(temp.path());
    write_file(
        &dir.join("_quarto.yml"),
        "project:\n  type: website\n  output-dir: _site\n",
    );
    write_file(&dir.join("index.qmd"), "---\ntitle: Home\n---\n\nHome.\n");
    // Sibling with unclosed code fence — Pass-1 fails for this file.
    write_file(
        &dir.join("broken.qmd"),
        "---\ntitle: Broken\n---\n\n```{python\n",
    );

    let output = run_q2_render(&dir, &["--json-errors"]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let lines = parse_ndjson_lines(&stderr);

    let has_pass1 = lines.iter().any(|v| {
        is_pass1_failure_shape(v)
            && v.get("source_file")
                .and_then(|s| s.as_str())
                .is_some_and(|s| s.contains("broken.qmd"))
    });
    assert!(
        has_pass1,
        "expected a JsonPass1Failure line referring to broken.qmd; stderr:\n{stderr}\nparsed:\n{lines:#?}"
    );
}

/// Phase 1 test: project with no renderable files emits a
/// project-level `Q-PROJECT-EMPTY` diagnostic as JSON, with no
/// location fields.
#[test]
fn project_diagnostic_json() {
    let temp = TempDir::new().unwrap();
    let dir = canonical(temp.path());
    // Project config with a render-list pattern that matches nothing.
    write_file(
        &dir.join("_quarto.yml"),
        "project:\n  type: website\n  output-dir: _site\n  render:\n    - does-not-exist.qmd\n",
    );

    let output = run_q2_render(&dir, &["--json-errors"]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let lines = parse_ndjson_lines(&stderr);

    let has_q_project_empty = lines.iter().any(|v| {
        is_diagnostic_shape(v)
            && v.get("code").and_then(|c| c.as_str()) == Some("Q-PROJECT-EMPTY")
            && v.get("start_line").is_none()
    });
    assert!(
        has_q_project_empty,
        "expected a JsonDiagnostic with code Q-PROJECT-EMPTY and no start_line; stderr:\n{stderr}\nparsed:\n{lines:#?}"
    );
}

/// Phase 1 test: a `DispatchError` (here: path not found, Q-7-2)
/// emits a JSON diagnostic on stderr with the matching error code.
#[test]
fn dispatch_error_json() {
    let temp = TempDir::new().unwrap();
    let dir = canonical(temp.path());
    let output = run_q2_render(&dir, &["--json-errors", "does-not-exist.qmd"]);
    assert!(
        !output.status.success(),
        "expected non-zero exit when input path is missing"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    let lines = parse_ndjson_lines(&stderr);
    let has_q72 = lines.iter().any(|v| {
        is_diagnostic_shape(v)
            && v.get("kind").and_then(|k| k.as_str()) == Some("error")
            && v.get("code").and_then(|c| c.as_str()) == Some("Q-7-2")
    });
    assert!(
        has_q72,
        "expected a JsonDiagnostic with code Q-7-2 for path-not-found; stderr:\n{stderr}\nparsed:\n{lines:#?}"
    );
}

/// Phase 1 regression test: without `--json-errors`, the human
/// (ariadne) text path is unchanged — there should be NO JSON
/// objects on stderr for the same fixture.
#[test]
fn text_mode_unchanged_regression() {
    let temp = TempDir::new().unwrap();
    let dir = canonical(temp.path());
    let fixture = dir.join("bad.qmd");
    write_file(&fixture, "---\ntitle: Bad\n---\n\n```{python\n");

    let out_path = dir.join("bad.html");
    let output = run_q2_render(&dir, &["-o", out_path.to_str().unwrap(), "bad.qmd"]);
    assert!(
        !output.status.success(),
        "expected non-zero exit on parse error"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    let lines = parse_ndjson_lines(&stderr);
    // It's fine for ariadne to emit some lines starting with `{` (e.g.
    // a `{...}` literal in source); what we really want is "no line
    // parses as a JsonDiagnostic" — i.e. nothing claims our schema.
    let any_diagnostic_shape = lines.iter().any(is_diagnostic_shape);
    let any_pass1_shape = lines.iter().any(is_pass1_failure_shape);
    assert!(
        !any_diagnostic_shape,
        "text mode must not emit JsonDiagnostic; stderr:\n{stderr}"
    );
    assert!(
        !any_pass1_shape,
        "text mode must not emit JsonPass1Failure; stderr:\n{stderr}"
    );
}
