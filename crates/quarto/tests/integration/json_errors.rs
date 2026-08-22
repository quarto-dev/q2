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

/// bd-y56u1gl7: a structured parse error from *project discovery*
/// (here: Q-5-17 unknown `project.type`) must surface its real code
/// and location under `--json-errors` — not the generic Q-7-8
/// "Project Discovery Failed" envelope with the ANSI-rendered text
/// buried in `problem`.
#[test]
fn discovery_parse_error_json_carries_real_code() {
    let temp = TempDir::new().unwrap();
    let dir = canonical(temp.path());
    write_file(&dir.join("_quarto.yml"), "project:\n  type: posit-docs\n");
    write_file(&dir.join("index.qmd"), "---\ntitle: x\n---\n\nhi\n");

    let output = run_q2_render(&dir, &["--json-errors"]);
    assert!(
        !output.status.success(),
        "expected non-zero exit on unknown project.type"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    let lines = parse_ndjson_lines(&stderr);
    assert!(
        !lines.is_empty(),
        "expected at least one JSON diagnostic on stderr; stderr was:\n{stderr}"
    );

    let codes: Vec<&str> = lines
        .iter()
        .filter_map(|l| l.get("code").and_then(|c| c.as_str()))
        .collect();
    assert!(
        codes.contains(&"Q-5-17"),
        "expected the real Q-5-17 code in the stream, got {codes:?}; stderr:\n{stderr}"
    );
    assert!(
        !codes.contains(&"Q-7-8"),
        "the generic Q-7-8 envelope must not wrap a structured parse error; got {codes:?}"
    );

    let diag = lines
        .iter()
        .find(|l| l.get("code").and_then(|c| c.as_str()) == Some("Q-5-17"))
        .unwrap();
    assert!(
        is_diagnostic_shape(diag),
        "Q-5-17 line must claim the JsonDiagnostic schema; got:\n{diag:#?}"
    );
    assert_eq!(diag.get("kind").and_then(|k| k.as_str()), Some("error"));
    assert!(
        diag.get("start_line").is_some(),
        "the diagnostic must carry its source location; got:\n{diag:#?}"
    );
    assert!(
        !stderr.contains('\u{1b}'),
        "no ANSI escapes may leak into --json-errors output; got:\n{stderr}"
    );
}

// ====================================================================
// C4a: YAML content provenance in the re-parse bases (both YAML paths)
//
// `Q-2-9` ("HTML element converted to raw HTML") warnings are emitted
// by the qmd re-parse `ConfigMarkdownTransform`/`meta.rs` trigger on
// blessed config strings. Their location is whatever `SourceInfo` was
// passed as the re-parse *base*: before this task, that base was the
// node's raw source span (`source_info`, which includes quote
// delimiters and the per-line indentation block-scalar decoding
// strips), paired with the already-*decoded* string being re-parsed —
// wrong by exactly the amount decoding removed. These tests assert
// exact `start_column`/`end_column` (not just `start_line.is_some()`,
// which is all older tests in this file check) to catch that drift.
// ====================================================================

/// Every top-level `JsonDiagnostic` line with the given error `code`,
/// in emission order.
fn diagnostics_with_code<'a>(lines: &'a [Value], code: &str) -> Vec<&'a Value> {
    lines
        .iter()
        .filter(|v| is_diagnostic_shape(v) && v.get("code").and_then(|c| c.as_str()) == Some(code))
        .collect()
}

/// Read a required integer field off a diagnostic, panicking with the
/// full diagnostic if it's missing — used for `start_line` /
/// `start_column` / `end_line` / `end_column`, which this module (unlike
/// the rest of this file) asserts exactly rather than just checking
/// presence.
fn field_i64(v: &Value, field: &str) -> i64 {
    v.get(field)
        .and_then(|x| x.as_i64())
        .unwrap_or_else(|| panic!("expected integer field {field:?} on diagnostic:\n{v:#?}"))
}

/// `(start_line, start_column, end_line, end_column)` for each diagnostic,
/// in emission order.
fn positions(diags: &[&Value]) -> Vec<(i64, i64, i64, i64)> {
    diags
        .iter()
        .map(|d| {
            (
                field_i64(d, "start_line"),
                field_i64(d, "start_column"),
                field_i64(d, "end_line"),
                field_i64(d, "end_column"),
            )
        })
        .collect()
}

/// Test 1 (the canonical case): a multi-line block scalar
/// (`page-footer.center: |`) containing raw HTML on its third content
/// line. Decoding a block scalar strips each line's leading indent, so
/// the decoded content drifts from the raw source span by the
/// *accumulated* stripped indent — enough, on the third line, to also
/// misattribute the diagnostic to the wrong *line*, not just the wrong
/// column. That's why both line and column are asserted here: a
/// column-only assertion would still pass if only the column half of
/// the bug were fixed.
///
/// Fixture transcribed verbatim from task-C4a-brief.md. Truth values
/// (`9:7` / `9:26`) are measured there against released q2 0.24.0,
/// which (pre-fix) reports `8:10` / `9:14` instead — a drift of
/// exactly 12 = 2 preceding content lines × 6 bytes of stripped indent.
#[test]
fn block_scalar_content_provenance_line_and_column() {
    let temp = TempDir::new().unwrap();
    let dir = canonical(temp.path());
    write_file(
        &dir.join("_quarto.yml"),
        "project:\n  type: website\nwebsite:\n  title: \"T\"\n  page-footer:\n    center: |\n      line one\n      line two\n      <span id=\"y\">Footer</span>\n",
    );
    write_file(
        &dir.join("index.qmd"),
        "---\ntitle: \"Index\"\n---\n\nbody\n",
    );

    let output = run_q2_render(&dir, &["--json-errors"]);
    assert!(
        output.status.success(),
        "expected a clean render (Q-2-9 is a warning); stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    let lines = parse_ndjson_lines(&stderr);
    let warnings = diagnostics_with_code(&lines, "Q-2-9");
    assert_eq!(
        warnings.len(),
        2,
        "expected exactly two Q-2-9 warnings (open + close <span> tags); got:\n{lines:#?}"
    );

    let starts: Vec<(i64, i64)> = positions(&warnings)
        .into_iter()
        .map(|(sl, sc, _, _)| (sl, sc))
        .collect();
    assert_eq!(
        starts,
        vec![(9, 7), (9, 26)],
        "expected the open <span> tag at 9:7 and the close tag at 9:26 (not 8:10 / 9:14, \
         the pre-fix drifted values); got {starts:?}\nfull diagnostics:\n{warnings:#?}"
    );

    // Bonus rigor beyond the brief's minimum (start line+column only):
    // the exclusive end position, hand-derived from the fixture
    // (open tag `<span id=\"y\">` spans columns 7-19, so its exclusive
    // end is column 20; close tag `</span>` spans columns 26-32, so its
    // exclusive end is column 33).
    let ends: Vec<(i64, i64)> = positions(&warnings)
        .into_iter()
        .map(|(_, _, el, ec)| (el, ec))
        .collect();
    assert_eq!(ends, vec![(9, 20), (9, 33)], "unexpected end positions");
}

/// Test 2: a quoted scalar containing raw HTML, on the **deferred**
/// project-config path (`ConfigMarkdownTransform` re-parsing
/// `website.title` after load). A quoted scalar's decoded content
/// drifts exactly 1 byte from its raw span (the opening quote).
///
/// Fixture: `website.title: "A <b>B</b>"` on `_quarto.yml` line 4
/// (0-based columns shown, then converted to the 1-based wire
/// convention `diagnostic_to_json` uses — `map_offset(0)`/
/// `map_offset(length)`, each `row+1`/`col+1`):
///
/// ```text
/// col (0-based): 0123456789012345678901
/// text:            title: "A <b>B</b>"
///                  t=2  "=9 A=10 <=12 b=13 >=14 B=15 <=16 /=17 b=18 >=19 "=20
/// ```
///
/// Decoded content is `A <b>B</b>` (10 bytes); `content_source_info`
/// maps content offset 0 to the `A` at column 10 (0-based) — i.e. one
/// byte right of the opening quote at column 9, confirming the "quoted
/// scalar drifts 1 byte" rule. From there:
/// - `<b>` is content offsets 2..5 → columns 12..15 (0-based) → 1-based
///   start `(4, 13)`, exclusive end at content offset 5 → column 15
///   (0-based) → 1-based `(4, 16)`.
/// - `</b>` is content offsets 6..10 → columns 16..20 (0-based) →
///   1-based start `(4, 17)`, exclusive end at content offset 10 →
///   column 20 (0-based) → 1-based `(4, 21)`.
///
/// Pre-fix (pairing decoded content with the quote-inclusive raw span)
/// each of these drifts 1 column left: `(4, 12)`/`(4,15)` and
/// `(4,16)`/`(4,20)`.
#[test]
fn quoted_scalar_raw_html_project_config_provenance() {
    let temp = TempDir::new().unwrap();
    let dir = canonical(temp.path());
    write_file(
        &dir.join("_quarto.yml"),
        "project:\n  type: website\nwebsite:\n  title: \"A <b>B</b>\"\n",
    );
    write_file(
        &dir.join("index.qmd"),
        "---\ntitle: \"Index\"\n---\n\nbody\n",
    );

    let output = run_q2_render(&dir, &["--json-errors"]);
    assert!(
        output.status.success(),
        "expected a clean render; stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    let lines = parse_ndjson_lines(&stderr);
    let warnings = diagnostics_with_code(&lines, "Q-2-9");
    assert_eq!(
        warnings.len(),
        2,
        "expected exactly two Q-2-9 warnings (<b> and </b>); got:\n{lines:#?}"
    );

    assert_eq!(
        positions(&warnings),
        vec![(4, 13, 4, 16), (4, 17, 4, 21)],
        "expected <b> at 4:13-4:16 and </b> at 4:17-4:21 (not the 1-byte-left-drifted \
         4:12-4:15 / 4:16-4:20); got:\n{warnings:#?}"
    );
}

/// Test 3: a quoted `title:` in the document's **own front matter** —
/// the immediate re-parse path (`meta.rs`'s `DocumentMetadata` default
/// branch), as opposed to test 2's deferred project-config path.
///
/// Fixture: `title: "A <b>B</b>"` as `index.qmd`'s entire front
/// matter, line 2 (no leading indent this time, so every column below
/// is exactly 2 less than test 2's, i.e. `title: ` occupies columns
/// 0-6 (0-based) here instead of `  title: ` 's columns 0-8 there).
/// Same fixture string, same derivation method as test 2:
/// - `<b>`: content offsets 2..5 → 1-based start `(2, 11)`, end `(2, 14)`.
/// - `</b>`: content offsets 6..10 → 1-based start `(2, 15)`, end `(2, 19)`.
#[test]
fn quoted_scalar_raw_html_frontmatter_provenance() {
    let temp = TempDir::new().unwrap();
    let dir = canonical(temp.path());
    let out_path = dir.join("index.html");
    write_file(
        &dir.join("index.qmd"),
        "---\ntitle: \"A <b>B</b>\"\n---\n\nbody\n",
    );

    let output = run_q2_render(
        &dir,
        &[
            "--json-errors",
            "-o",
            out_path.to_str().unwrap(),
            "index.qmd",
        ],
    );
    assert!(
        output.status.success(),
        "expected a clean render; stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    let lines = parse_ndjson_lines(&stderr);
    let warnings = diagnostics_with_code(&lines, "Q-2-9");
    assert_eq!(
        warnings.len(),
        2,
        "expected exactly two Q-2-9 warnings (<b> and </b>); got:\n{lines:#?}"
    );

    assert_eq!(
        positions(&warnings),
        vec![(2, 11, 2, 14), (2, 15, 2, 19)],
        "expected <b> at 2:11-2:14 and </b> at 2:15-2:19 (not the 1-byte-left-drifted \
         2:10-2:13 / 2:14-2:18); got:\n{warnings:#?}"
    );
}

/// Test 4a (regression guard): a **plain** (unquoted) scalar has no
/// delimiter for decoding to strip, so its content and raw spans
/// coincide already — this must stay correct, not become correct.
/// Same fixture shape and derivation as test 3, minus the quotes.
#[test]
fn plain_scalar_raw_html_frontmatter_unaffected() {
    let temp = TempDir::new().unwrap();
    let dir = canonical(temp.path());
    let out_path = dir.join("index.html");
    write_file(
        &dir.join("index.qmd"),
        "---\ntitle: A <b>B</b>\n---\n\nbody\n",
    );

    let output = run_q2_render(
        &dir,
        &[
            "--json-errors",
            "-o",
            out_path.to_str().unwrap(),
            "index.qmd",
        ],
    );
    assert!(
        output.status.success(),
        "expected a clean render; stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    let lines = parse_ndjson_lines(&stderr);
    let warnings = diagnostics_with_code(&lines, "Q-2-9");
    assert_eq!(
        warnings.len(),
        2,
        "expected exactly two Q-2-9 warnings (<b> and </b>); got:\n{lines:#?}"
    );

    assert_eq!(
        positions(&warnings),
        vec![(2, 10, 2, 13), (2, 14, 2, 18)],
        "plain scalar positions must be unaffected by the content-provenance fix; got:\n{warnings:#?}"
    );
}

/// Test 4b (regression guard): a **single-line** block scalar has no
/// preceding content lines, so it has zero accumulated stripped indent
/// — the drift this whole fix addresses is exactly zero here. Same
/// column shape as test 1's canonical third line (`column 7` /
/// `column 26`), which is the point: it demonstrates the drift really
/// is a *per preceding line* effect, not something inherent to block
/// scalars as such.
#[test]
fn single_line_block_scalar_raw_html_unaffected() {
    let temp = TempDir::new().unwrap();
    let dir = canonical(temp.path());
    write_file(
        &dir.join("_quarto.yml"),
        "project:\n  type: website\nwebsite:\n  page-footer:\n    center: |\n      <span id=\"z\">Footer</span>\n",
    );
    write_file(
        &dir.join("index.qmd"),
        "---\ntitle: \"Index\"\n---\n\nbody\n",
    );

    let output = run_q2_render(&dir, &["--json-errors"]);
    assert!(
        output.status.success(),
        "expected a clean render; stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    let lines = parse_ndjson_lines(&stderr);
    let warnings = diagnostics_with_code(&lines, "Q-2-9");
    assert_eq!(
        warnings.len(),
        2,
        "expected exactly two Q-2-9 warnings (<span> and </span>); got:\n{lines:#?}"
    );

    assert_eq!(
        positions(&warnings),
        vec![(6, 7, 6, 20), (6, 26, 6, 33)],
        "single-line block scalar positions must be unaffected by the content-provenance fix; got:\n{warnings:#?}"
    );
}
