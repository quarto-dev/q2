//! Tests for bd-syntax-helper-parse-masking-w88mhedp: rules that need a
//! successful AST must not report an unparseable file as clean.
//!
//! Before the fix, `bracket_analysis::analyze` turned a parse failure into an
//! empty analysis, so `check -r literal-brackets -r reference-links` printed
//! "Success rate: 100.0%" for a file the rules never saw. The same masking
//! existed independently in q-2-30. The fix makes "unanalyzable" a distinct
//! state: `analyze()` and the requires-parse rules' `check()` return an error
//! on parse failure, and the check/convert drivers probe the file first, skip
//! requires-parse rules, and report the skip explicitly.

use qmd_syntax_helper::conversions::bracket_analysis::analyze;
use qmd_syntax_helper::rule::RuleRegistry;
use qmd_syntax_helper::utils::resources::ResourceManager;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Output;

/// Q-2-10 (stray apostrophe: a space before `'` makes it read as a closing
/// quote with no matching open) plus one undefined literal bracket and one
/// reference-style link with its definition — the two findings the AST rules
/// exist to produce, masked by the parse failure.
const UNPARSEABLE: &str = "---\ntitle: \"Repro\"\n---\n\n\
The admins ' console triggers Q-2-10.\n\n\
Diagram key: [1] is the gateway node.\n\n\
See [the docs][gcc] for details.\n\n\
[gcc]: https://example.com/gcc\n";

/// Identical, with the apostrophe escaped so the file parses.
const PARSEABLE_CONTROL: &str = "---\ntitle: \"Repro\"\n---\n\n\
The admins \\' console triggers no parse error.\n\n\
Diagram key: [1] is the gateway node.\n\n\
See [the docs][gcc] for details.\n\n\
[gcc]: https://example.com/gcc\n";

fn write_fixture(rm: &ResourceManager, name: &str, content: &str) -> PathBuf {
    let path = rm.temp_dir().join(name);
    fs::write(&path, content).unwrap();
    path
}

fn run_bin(args: &[&str], paths: &[&Path]) -> Output {
    let mut cmd = std::process::Command::new(env!("CARGO_BIN_EXE_qmd-syntax-helper"));
    cmd.args(args);
    for p in paths {
        cmd.arg(p);
    }
    cmd.output().unwrap()
}

fn stdout_of(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).to_string()
}

fn stderr_of(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).to_string()
}

// ---------------------------------------------------------------------
// Library level: parse failures are errors, not empty results
// ---------------------------------------------------------------------

#[test]
fn analyze_returns_err_on_unparseable_source() {
    let result = analyze(UNPARSEABLE, "test.qmd");
    let err = result.expect_err("analyze must not turn a parse failure into an empty analysis");
    assert!(
        err.to_string().contains("Q-2-10"),
        "error should carry the parse diagnostic codes, got: {err}"
    );
}

#[test]
fn analyze_succeeds_on_parseable_control() {
    let analysis = analyze(PARSEABLE_CONTROL, "test.qmd").unwrap();
    assert_eq!(
        analysis.findings.len(),
        2,
        "control fixture must yield the literal bracket and the reference link"
    );
}

#[test]
fn requires_parse_declarations() {
    let registry = RuleRegistry::new().unwrap();
    for name in ["literal-brackets", "reference-links", "q-2-30"] {
        assert!(
            registry.get(name).unwrap().requires_parse(),
            "{name} needs a successful AST and must declare requires_parse"
        );
    }
    // Diagnostic-driven and text-based rules read unparseable files on
    // purpose; they must keep running on them.
    for name in ["parse", "apostrophe-quotes", "grid-tables", "q-2-5"] {
        assert!(
            !registry.get(name).unwrap().requires_parse(),
            "{name} must not require a successful parse"
        );
    }
}

#[test]
fn requires_parse_rule_checks_err_on_unparseable() {
    let rm = ResourceManager::new().unwrap();
    let file = write_fixture(&rm, "unparseable.qmd", UNPARSEABLE);
    let registry = RuleRegistry::new().unwrap();

    for name in ["literal-brackets", "reference-links", "q-2-30"] {
        let result = registry.get(name).unwrap().check(&file, false);
        let err = result.expect_err(&format!(
            "{name}.check on an unparseable file must be an error, not a clean result"
        ));
        assert!(
            err.to_string().contains("Q-2-10"),
            "{name}'s error should carry the parse diagnostic codes, got: {err}"
        );
    }
}

// ---------------------------------------------------------------------
// Binary level: check driver — skip, synthesize, summarize
// ---------------------------------------------------------------------

#[test]
fn scoped_check_reports_unanalyzable_not_clean() {
    let rm = ResourceManager::new().unwrap();
    let file = write_fixture(&rm, "unparseable.qmd", UNPARSEABLE);

    let out = run_bin(
        &["check", "-r", "literal-brackets", "-r", "reference-links"],
        &[&file],
    );
    let stdout = stdout_of(&out);

    assert!(
        stdout.contains("Unanalyzable files:  1"),
        "summary must count the file as unanalyzable, got:\n{stdout}"
    );
    assert!(
        stdout.contains("Clean files:         0"),
        "an unanalyzable file must not be counted clean, got:\n{stdout}"
    );
    assert!(
        stdout.contains("Success rate:        0.0%"),
        "an unanalyzable file must not count toward the success rate, got:\n{stdout}"
    );
    assert!(
        stdout.contains("file does not parse (Q-2-10)"),
        "per-file output must say why the rules were skipped, got:\n{stdout}"
    );
    assert!(
        stdout.contains("not applied: literal-brackets, reference-links"),
        "per-file output must name the skipped rules, got:\n{stdout}"
    );
}

#[test]
fn scoped_check_control_still_finds_both_issues() {
    let rm = ResourceManager::new().unwrap();
    let file = write_fixture(&rm, "control.qmd", PARSEABLE_CONTROL);

    let out = run_bin(
        &["check", "-r", "literal-brackets", "-r", "reference-links"],
        &[&file],
    );
    let stdout = stdout_of(&out);

    assert!(
        stdout.contains("Files with issues:   1"),
        "control file must report its findings, got:\n{stdout}"
    );
    assert!(
        !stdout.contains("Unanalyzable"),
        "a parseable file must not produce unanalyzable output, got:\n{stdout}"
    );
}

#[test]
fn scoped_check_json_marks_unanalyzable() {
    let rm = ResourceManager::new().unwrap();
    let file = write_fixture(&rm, "unparseable.qmd", UNPARSEABLE);

    let out = run_bin(
        &[
            "check",
            "--json",
            "-r",
            "literal-brackets",
            "-r",
            "reference-links",
        ],
        &[&file],
    );
    let stdout = stdout_of(&out);

    let records: Vec<serde_json::Value> = stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(
        records.len(),
        1,
        "exactly one synthesized record for the file, got:\n{stdout}"
    );
    let rec = &records[0];
    assert_eq!(rec["rule_name"], "unanalyzable");
    assert_eq!(rec["has_issue"], false);
    assert_eq!(rec["unanalyzable"], true);
    assert_eq!(
        rec["skipped_rules"],
        serde_json::json!(["literal-brackets", "reference-links"]),
        "skipped_rules must list the skipped rules in sorted order"
    );
    assert_eq!(rec["error_codes"], serde_json::json!(["Q-2-10"]));
}

#[test]
fn check_all_skips_requires_parse_rules_but_runs_the_rest() {
    let rm = ResourceManager::new().unwrap();
    let file = write_fixture(&rm, "unparseable.qmd", UNPARSEABLE);

    let out = run_bin(&["check", "-r", "all", "--json"], &[&file]);
    let stdout = stdout_of(&out);

    let records: Vec<serde_json::Value> = stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();

    // The parse rule still reports the failure as its own finding.
    assert!(
        records
            .iter()
            .any(|r| r["rule_name"] == "parse" && r["has_issue"] == true),
        "parse rule must still report the failure under -r all, got:\n{stdout}"
    );
    // The requires-parse rules are skipped, recorded in one synthesized
    // per-file record (sorted for determinism — the registry iterates a
    // HashMap).
    let unanalyzable: Vec<_> = records
        .iter()
        .filter(|r| r["rule_name"] == "unanalyzable")
        .collect();
    assert_eq!(
        unanalyzable.len(),
        1,
        "exactly one synthesized record per file, got:\n{stdout}"
    );
    assert_eq!(
        unanalyzable[0]["skipped_rules"],
        serde_json::json!(["literal-brackets", "q-2-30", "reference-links"]),
    );
}

#[test]
fn check_without_requires_parse_rules_never_probes() {
    let rm = ResourceManager::new().unwrap();
    let file = write_fixture(&rm, "unparseable.qmd", UNPARSEABLE);

    // apostrophe-quotes is diagnostic-driven: the parse failure is its input.
    let out = run_bin(&["check", "-r", "apostrophe-quotes"], &[&file]);
    let stdout = stdout_of(&out);

    assert!(
        stdout.contains("Files with issues:   1"),
        "apostrophe-quotes must still run on (and flag) the unparseable file, got:\n{stdout}"
    );
    assert!(
        !stdout.contains("Unanalyzable"),
        "no requires-parse rule requested, so no unanalyzable accounting, got:\n{stdout}"
    );
}

#[test]
fn rule_error_files_are_not_counted_clean() {
    let rm = ResourceManager::new().unwrap();
    let path = rm.temp_dir().join("invalid-utf8.qmd");
    fs::write(&path, [0xff, 0xfe, 0x00, 0x41]).unwrap();

    let out = run_bin(&["check", "-r", "parse"], &[&path]);
    let stdout = stdout_of(&out);

    assert!(
        stdout.contains("Files with errors:   1"),
        "a file whose rules all error must be counted as an error file, got:\n{stdout}"
    );
    assert!(
        stdout.contains("Clean files:         0"),
        "a file whose rules all error must not be counted clean, got:\n{stdout}"
    );
}

// ---------------------------------------------------------------------
// Binary level: convert driver — per-file refusal, sweep continues
// ---------------------------------------------------------------------

#[test]
fn convert_refuses_unparseable_file_without_aborting_sweep() {
    let rm = ResourceManager::new().unwrap();
    let bad = write_fixture(&rm, "bad.qmd", UNPARSEABLE);
    let good = write_fixture(&rm, "good.qmd", PARSEABLE_CONTROL);

    let out = run_bin(&["convert", "-r", "literal-brackets", "-i"], &[&bad, &good]);
    let stderr = stderr_of(&out);

    assert!(
        out.status.success(),
        "a refusal must not fail the sweep, stderr:\n{stderr}"
    );
    assert_eq!(
        fs::read_to_string(&bad).unwrap(),
        UNPARSEABLE,
        "the unparseable file must be left untouched"
    );
    assert!(
        fs::read_to_string(&good).unwrap().contains("\\[1\\]"),
        "the sweep must continue past the refusal and convert the next file"
    );
    assert!(
        stderr.contains("bad.qmd") && stderr.contains("not applied"),
        "the refusal must be reported per file, got stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("literal-brackets") && stderr.contains("Q-2-10"),
        "the refusal must name the skipped rule and the parse codes, got stderr:\n{stderr}"
    );
    assert!(
        !stderr.contains("good.qmd"),
        "the parseable file must not be reported as refused, got stderr:\n{stderr}"
    );
}

/// Regression guard for the fix-then-apply compounding: an earlier rule in
/// the same run repairs the parse error, so the requires-parse rule applies
/// in a later iteration and no refusal is reported. (This worked before the
/// fix — via the very empty-analysis path being removed — and must keep
/// working with the driver-level skip.)
#[test]
fn convert_compounding_repairs_parse_then_applies_ast_rule() {
    let rm = ResourceManager::new().unwrap();
    let file = write_fixture(&rm, "compound.qmd", UNPARSEABLE);

    let out = run_bin(
        &[
            "convert",
            "-r",
            "apostrophe-quotes",
            "-r",
            "literal-brackets",
            "-i",
        ],
        &[&file],
    );
    let stderr = stderr_of(&out);
    let converted = fs::read_to_string(&file).unwrap();

    assert!(out.status.success(), "stderr:\n{stderr}");
    assert!(
        converted.contains("admins \\'"),
        "apostrophe-quotes must repair the parse error, got:\n{converted}"
    );
    assert!(
        converted.contains("\\[1\\]"),
        "literal-brackets must apply once the file parses, got:\n{converted}"
    );
    assert!(
        !stderr.contains("not applied"),
        "no refusal when the parse error is repaired mid-run, got stderr:\n{stderr}"
    );
}
