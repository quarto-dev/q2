//! End-to-end CLI tests for `q2 render --dry-run`
//! (bd-render-dry-run-dyiq7d08).
//!
//! A dry run stops after Pass 1 (parse + metadata merge per file) and
//! the project type's `pre_render` hook, reports every diagnostic it
//! found, and writes nothing. Under `--json-errors` the diagnostics keep
//! the real render's NDJSON wire shapes on stderr, and one summary
//! object is printed on stdout.
//!
//! Plan: `claude-notes/plans/2026-11-13-render-dry-run.md`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;
use tempfile::TempDir;

const Q2_BIN: &str = env!("CARGO_BIN_EXE_q2");
const PASS1_FAILURE_SCHEMA: &str = "https://quarto.org/schemas/v1/json-pass1-failure.json";
const DIAGNOSTIC_SCHEMA: &str = "https://quarto.org/schemas/v1/json-diagnostic.json";
const SUMMARY_SCHEMA: &str = "https://quarto.org/schemas/v1/render-dry-run-summary.json";

/// A document whose attribute block never closes: Q-2-38, a hard parse
/// error raised in Pass 1.
const BROKEN_QMD: &str = "---\ntitle: Broken\n---\n\n::: {.callout\n\nText.\n";

/// A document whose front matter names a missing stylesheet: Q-5-29, a
/// warning raised by the metadata merge (Pass 1). (Most other common
/// warnings — an unresolved `[ref]`, say — come from Pass-2 transforms,
/// which a dry run deliberately does not run.)
const CSS_WARNING_QMD: &str = "---\ntitle: Warn\ncss: missing.css\n---\n\nHello.\n";

fn write_file(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, contents).unwrap();
}

fn canonical(p: &Path) -> PathBuf {
    p.canonicalize().unwrap_or_else(|_| p.to_path_buf())
}

fn run_q2(cwd: &Path, args: &[&str]) -> std::process::Output {
    Command::new(Q2_BIN)
        .current_dir(cwd)
        .arg("render")
        .args(args)
        // Keep the run deterministic (sequential Pass 1, so `--fail-fast`
        // stops at the first file in document order) and free of
        // ambient log filters.
        .env("QUARTO_JOBS", "1")
        .env_remove("RUST_LOG")
        .output()
        .expect("spawn q2 binary")
}

fn stderr_of(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn stdout_of(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// Every non-empty stderr line must be a JSON object.
fn parse_ndjson_strict(stderr: &str) -> Vec<Value> {
    stderr
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| match serde_json::from_str::<Value>(line) {
            Ok(value) if value.is_object() => value,
            _ => {
                panic!("stderr must be pure NDJSON; non-JSON line:\n{line}\nfull stderr:\n{stderr}")
            }
        })
        .collect()
}

/// Stdout must hold exactly one JSON object: the dry-run summary.
fn parse_summary(stdout: &str) -> Value {
    let lines: Vec<&str> = stdout.lines().filter(|l| !l.trim().is_empty()).collect();
    assert_eq!(
        lines.len(),
        1,
        "expected one summary line on stdout:\n{stdout}"
    );
    let value: Value = serde_json::from_str(lines[0]).expect("summary must be JSON");
    assert_eq!(value["$schema"], SUMMARY_SCHEMA);
    assert_eq!(value["dry_run"], true);
    value
}

fn schema_of(v: &Value) -> &str {
    v.get("$schema").and_then(Value::as_str).unwrap_or("")
}

/// Every file under `dir` (relative path → bytes), for "nothing was
/// written" assertions.
fn snapshot_tree(dir: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn walk(root: &Path, dir: &Path, out: &mut BTreeMap<PathBuf, Vec<u8>>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(root, &path, out);
            } else {
                let rel = path.strip_prefix(root).unwrap().to_path_buf();
                out.insert(rel, std::fs::read(&path).unwrap());
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(dir, dir, &mut out);
    out
}

/// A website project: one clean page, one page with a Pass-1 warning,
/// one page that fails to parse.
fn write_mixed_project(dir: &Path) {
    write_file(&dir.join("_quarto.yml"), "project:\n  type: website\n");
    write_file(&dir.join("index.qmd"), "---\ntitle: Home\n---\n\nHello.\n");
    write_file(&dir.join("warn.qmd"), CSS_WARNING_QMD);
    write_file(&dir.join("broken.qmd"), BROKEN_QMD);
}

#[test]
fn dry_run_flag_is_documented_in_help() {
    let output = Command::new(Q2_BIN)
        .args(["render", "--help"])
        .output()
        .unwrap();
    assert!(
        stdout_of(&output).contains("--dry-run"),
        "render --help must document --dry-run"
    );
}

#[test]
fn dry_run_conflicts_with_clean_cache() {
    let temp = TempDir::new().unwrap();
    let dir = canonical(temp.path());
    write_file(&dir.join("index.qmd"), "---\ntitle: A\n---\n\nHi.\n");
    let output = run_q2(&dir, &["--dry-run", "--clean-cache"]);
    assert!(!output.status.success());
    assert!(
        stderr_of(&output).contains("cannot be used with"),
        "{}",
        stderr_of(&output)
    );
}

/// Errors and warnings are both reported, the exit code is non-zero
/// because of the error, and the project is untouched: no output dir,
/// no `.quarto/` (the profile cache is neither read nor saved).
#[test]
fn project_dry_run_reports_problems_and_writes_nothing() {
    let temp = TempDir::new().unwrap();
    let dir = canonical(temp.path());
    write_mixed_project(&dir);
    let before = snapshot_tree(&dir);

    let output = run_q2(&dir, &["--dry-run"]);
    let stderr = stderr_of(&output);

    assert_eq!(output.status.code(), Some(1), "stderr:\n{stderr}");
    assert!(stderr.contains("Q-2-38"), "parse error reported:\n{stderr}");
    assert!(
        stderr.contains("Q-5-29"),
        "Pass-1 warning reported:\n{stderr}"
    );
    assert!(
        stderr.contains("Checked 3 files — 1 error, 1 warning"),
        "summary line:\n{stderr}"
    );
    // The would-render list is `-v` only.
    assert!(!stderr.contains("Would render"), "{stderr}");
    assert!(
        stdout_of(&output).is_empty(),
        "text mode leaves stdout alone"
    );

    assert_eq!(before, snapshot_tree(&dir), "a dry run must write nothing");
    assert!(!dir.join("_site").exists());
    assert!(!dir.join(".quarto").exists());
}

#[test]
fn warning_only_dry_run_exits_zero_unless_strict() {
    let temp = TempDir::new().unwrap();
    let dir = canonical(temp.path());
    write_file(&dir.join("_quarto.yml"), "project:\n  type: website\n");
    write_file(&dir.join("index.qmd"), CSS_WARNING_QMD);

    let output = run_q2(&dir, &["--dry-run"]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr_of(&output));
    assert!(stderr_of(&output).contains("Q-5-29"));

    let strict = run_q2(&dir, &["--dry-run", "--strict"]);
    assert_eq!(strict.status.code(), Some(1), "{}", stderr_of(&strict));
}

/// The profile cache would hide warnings (a cached profile carries no
/// diagnostics), so a dry run must parse live even when a real render
/// warmed the cache — and must leave that cache as it found it.
#[test]
fn dry_run_reports_warnings_despite_a_warm_profile_cache() {
    let temp = TempDir::new().unwrap();
    let dir = canonical(temp.path());
    write_file(&dir.join("_quarto.yml"), "project:\n  type: website\n");
    write_file(&dir.join("index.qmd"), CSS_WARNING_QMD);

    let real = run_q2(&dir, &[]);
    assert!(real.status.success(), "{}", stderr_of(&real));
    assert!(
        dir.join(".quarto/cache/profiles").is_dir(),
        "the real render must warm the profile cache for this test to mean anything"
    );
    let before = snapshot_tree(&dir);

    let output = run_q2(&dir, &["--dry-run"]);
    assert!(output.status.success(), "{}", stderr_of(&output));
    assert!(
        stderr_of(&output).contains("Q-5-29"),
        "warning must survive a warm cache:\n{}",
        stderr_of(&output)
    );
    assert_eq!(before, snapshot_tree(&dir), "a dry run must write nothing");
}

/// No engine runs: a cell that would create a file leaves no trace, and
/// the dry run passes whether or not Python is installed.
#[test]
fn dry_run_executes_no_code() {
    let temp = TempDir::new().unwrap();
    let dir = canonical(temp.path());
    write_file(
        &dir.join("index.qmd"),
        "---\ntitle: Code\n---\n\n```{python}\nopen('SENTINEL', 'w').write('ran')\n```\n",
    );

    let output = run_q2(&dir, &["index.qmd", "--dry-run"]);
    let stderr = stderr_of(&output);
    assert!(output.status.success(), "{stderr}");
    assert!(!dir.join("SENTINEL").exists(), "the cell must not run");
    // A single file gets the summary line too, so a clean run says so.
    assert!(
        stderr.contains("Checked 1 file — no problems found"),
        "{stderr}"
    );
    assert!(!dir.join("index.html").exists());
}

#[test]
fn verbose_dry_run_lists_files_with_their_formats() {
    let temp = TempDir::new().unwrap();
    let dir = canonical(temp.path());
    write_mixed_project(&dir);
    write_file(
        &dir.join("slides.qmd"),
        "---\ntitle: Deck\nformat: revealjs\n---\n\n## One\n",
    );

    let output = run_q2(&dir, &["--dry-run", "-v"]);
    let stderr = stderr_of(&output);
    assert!(stderr.contains("Would render 4 files:"), "{stderr}");
    assert!(stderr.contains("  index.qmd (html)"), "{stderr}");
    assert!(stderr.contains("  slides.qmd (revealjs)"), "{stderr}");
    assert!(
        stderr.contains("  broken.qmd (html) — would be skipped: failed to parse"),
        "{stderr}"
    );
}

/// A book's `pre_render` hook is part of the dry run: a missing chapter
/// (Q-5-35) is reported next to the per-file results instead of
/// aborting.
#[test]
fn book_dry_run_reports_missing_chapter() {
    let temp = TempDir::new().unwrap();
    let dir = canonical(temp.path());
    write_file(
        &dir.join("_quarto.yml"),
        "project:\n  type: book\nbook:\n  title: B\n  chapters:\n    - index.qmd\n    - missing.qmd\n",
    );
    write_file(&dir.join("index.qmd"), CSS_WARNING_QMD);

    let output = run_q2(&dir, &["--dry-run"]);
    let stderr = stderr_of(&output);
    assert_eq!(output.status.code(), Some(1), "{stderr}");
    assert!(stderr.contains("Q-5-35"), "{stderr}");
    assert!(
        stderr.contains("Q-5-29"),
        "per-file results survive: {stderr}"
    );
    assert!(!dir.join("_book").exists());
}

#[test]
fn fail_fast_dry_run_stops_at_first_failure() {
    let temp = TempDir::new().unwrap();
    let dir = canonical(temp.path());
    write_file(&dir.join("_quarto.yml"), "project:\n  type: website\n");
    write_file(&dir.join("a.qmd"), BROKEN_QMD);
    write_file(&dir.join("b.qmd"), BROKEN_QMD);

    let output = run_q2(&dir, &["--dry-run", "--fail-fast", "--json-errors"]);
    assert_eq!(output.status.code(), Some(1));
    let summary = parse_summary(&stdout_of(&output));
    assert_eq!(summary["stopped_early"], true);
    assert_eq!(summary["files_checked"], 1);
    assert_eq!(summary["files_failed"], 1);
}

// ── --json-errors ──────────────────────────────────────────────────────

/// Stderr is pure NDJSON in the real render's wire shapes; stdout holds
/// one summary object whose counts agree with stderr.
#[test]
fn json_dry_run_stderr_is_ndjson_and_stdout_is_one_summary() {
    let temp = TempDir::new().unwrap();
    let dir = canonical(temp.path());
    write_mixed_project(&dir);

    let output = run_q2(&dir, &["--dry-run", "--json-errors"]);
    assert_eq!(output.status.code(), Some(1));
    let lines = parse_ndjson_strict(&stderr_of(&output));

    let failures: Vec<&Value> = lines
        .iter()
        .filter(|v| schema_of(v) == PASS1_FAILURE_SCHEMA)
        .collect();
    assert_eq!(failures.len(), 1, "{lines:#?}");
    assert!(
        failures[0]["source_file"]
            .as_str()
            .unwrap()
            .ends_with("broken.qmd")
    );

    let warnings: Vec<&Value> = lines
        .iter()
        .filter(|v| schema_of(v) == DIAGNOSTIC_SCHEMA && v["kind"] == "warning")
        .collect();
    assert_eq!(warnings.len(), 1, "{lines:#?}");
    assert_eq!(warnings[0]["code"], "Q-5-29");
    assert!(
        warnings[0]["source_file"]
            .as_str()
            .unwrap()
            .ends_with("warn.qmd"),
        "the Pass-1 warning is attributed to its file: {:#?}",
        warnings[0]
    );

    let summary = parse_summary(&stdout_of(&output));
    assert_eq!(summary["files_checked"], 3);
    assert_eq!(summary["files_failed"], 1);
    assert_eq!(summary["errors"], 1);
    assert_eq!(summary["warnings"], 1);
    assert_eq!(summary["strict"], false);
    assert_eq!(summary["stopped_early"], false);
    assert_eq!(summary["format"], "html");
    assert!(
        summary.get("files").is_none(),
        "`files` is -v only: {summary}"
    );
}

/// `-v` adds `files` to the summary and still leaves stderr pure NDJSON
/// (no log lines).
#[test]
fn json_dry_run_verbose_adds_files_and_keeps_stderr_pure() {
    let temp = TempDir::new().unwrap();
    let dir = canonical(temp.path());
    write_mixed_project(&dir);

    let output = run_q2(&dir, &["--dry-run", "--json-errors", "-v"]);
    parse_ndjson_strict(&stderr_of(&output));
    let summary = parse_summary(&stdout_of(&output));
    let files = summary["files"].as_array().expect("files with -v");
    assert_eq!(files.len(), 3, "{summary}");
    let status_of = |name: &str| {
        files
            .iter()
            .find(|f| f["source_file"].as_str().unwrap().ends_with(name))
            .unwrap_or_else(|| panic!("{name} listed: {summary}"))["status"]
            .clone()
    };
    assert_eq!(status_of("broken.qmd"), "failed");
    assert_eq!(status_of("index.qmd"), "ok");
    assert!(files.iter().all(|f| f["format"] == "html"));
}

#[test]
fn json_dry_run_strict_promotes_in_stderr_and_summary() {
    let temp = TempDir::new().unwrap();
    let dir = canonical(temp.path());
    write_file(&dir.join("_quarto.yml"), "project:\n  type: website\n");
    write_file(&dir.join("index.qmd"), CSS_WARNING_QMD);

    let output = run_q2(&dir, &["--dry-run", "--json-errors", "--strict"]);
    assert_eq!(output.status.code(), Some(1));
    let lines = parse_ndjson_strict(&stderr_of(&output));
    let q5_29 = lines
        .iter()
        .find(|v| v["code"] == "Q-5-29")
        .expect("Q-5-29 record");
    assert_eq!(q5_29["kind"], "error");
    let summary = parse_summary(&stdout_of(&output));
    assert_eq!(summary["errors"], 1);
    assert_eq!(summary["warnings"], 0);
    assert_eq!(summary["strict"], true);
}

/// The single-file path prints the same summary.
#[test]
fn json_dry_run_single_file() {
    let temp = TempDir::new().unwrap();
    let dir = canonical(temp.path());
    write_file(&dir.join("a.qmd"), BROKEN_QMD);

    let output = run_q2(&dir, &["a.qmd", "--dry-run", "--json-errors"]);
    assert_eq!(output.status.code(), Some(1));
    let lines = parse_ndjson_strict(&stderr_of(&output));
    assert!(
        lines.iter().any(|v| v.to_string().contains("Q-2-38")),
        "{lines:#?}"
    );
    let summary = parse_summary(&stdout_of(&output));
    assert_eq!(summary["files_checked"], 1);
    assert_eq!(summary["errors"], 1);
}

/// Parity: a dry run reports a file that fails Pass 1 with exactly the
/// record a real render emits for it, so one parser serves both.
#[test]
fn json_dry_run_pass1_failure_matches_real_render() {
    let temp = TempDir::new().unwrap();
    let dir = canonical(temp.path());
    write_file(&dir.join("_quarto.yml"), "project:\n  type: website\n");
    write_file(&dir.join("index.qmd"), "---\ntitle: Home\n---\n\nHello.\n");
    write_file(&dir.join("broken.qmd"), BROKEN_QMD);

    let pass1_failures = |args: &[&str]| -> Vec<Value> {
        let output = run_q2(&dir, args);
        parse_ndjson_strict(&stderr_of(&output))
            .into_iter()
            .filter(|v| schema_of(v) == PASS1_FAILURE_SCHEMA)
            .collect()
    };
    let dry = pass1_failures(&["--dry-run", "--json-errors"]);
    let real = pass1_failures(&["--json-errors"]);
    assert_eq!(dry.len(), 1);
    assert_eq!(dry, real);
}
