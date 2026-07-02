//! `cargo xtask pandoc-check` — check the local `pandoc` binary against the
//! pampa oracle tests without touching the calibrated version gate.
//!
//! Print-only (decision 2 in bd-i9i5ad2t): on a green run past the current
//! ceiling, this prints the proposed new ceiling and where to bump it — it
//! never edits `crates/pampa/tests/integration/test.rs` itself. See the
//! ledger comment next to `PANDOC_ORACLE_MAX_VERSION` there.

use anyhow::{Context, Result, bail};
use std::process::Command;

use crate::dev_setup::parse_pandoc_version;
use crate::switch_task::current_worktree_root;
use crate::util::nested_command;

/// The oracle tests hard-gated on the calibrated pandoc range (each one
/// calls `assert_good_pandoc_version()` in `test.rs`). Hand-maintained, not
/// derived from that file — adding a new gated test there means adding its
/// name here too, or this tool will silently skip checking it.
const ORACLE_TEST_NAMES: &[&str] = &[
    "unit_test_corpus_matches_pandoc_markdown",
    "unit_test_corpus_matches_pandoc_commonmark",
    "test_json_writer",
    "test_html_writer",
];

const TEST_RS_RELATIVE_PATH: &str = "crates/pampa/tests/integration/test.rs";

/// Build the nextest filterset for exactly the oracle tests. `test(<name>)`
/// alone is a substring match against every binary in the crate — e.g.
/// `test(test_html_writer)` also matches the unrelated
/// `writers::html::tests::test_html_writer_context_*` unit tests in pampa's
/// lib and bin targets, silently inflating "4 oracle tests" into 8. Scoping
/// to `binary(integration)` (the single binary all pampa integration tests
/// compile into, per `.claude/rules/integration-tests.md`) excludes those.
fn oracle_test_filter() -> String {
    let names = ORACLE_TEST_NAMES
        .iter()
        .map(|name| format!("test({name})"))
        .collect::<Vec<_>>()
        .join(" + ");
    format!("binary(integration) & ({names})")
}

pub fn run() -> Result<()> {
    let root = current_worktree_root()?;

    let version_output = Command::new("pandoc")
        .arg("--version")
        .output()
        .context("running `pandoc --version` \u{2014} is pandoc installed?")?;
    let raw_version = String::from_utf8_lossy(&version_output.stdout).to_string();
    let detected = parse_pandoc_version(&raw_version);
    let first_line = raw_version.lines().next().unwrap_or("<no output>");

    let test_rs_path = root.join(TEST_RS_RELATIVE_PATH);
    let test_rs_source = std::fs::read_to_string(&test_rs_path)
        .with_context(|| format!("reading {}", test_rs_path.display()))?;
    let (min, _min_line) = find_version_const(&test_rs_source, "PANDOC_ORACLE_MIN_VERSION")?;
    let (max, max_line) = find_version_const(&test_rs_source, "PANDOC_ORACLE_MAX_VERSION")?;

    println!("Detected: {first_line}");
    println!(
        "Calibrated range: {}.{}\u{2013}{}.{}",
        min.0, min.1, max.0, max.1
    );

    let filter = oracle_test_filter();

    let mut cmd = nested_command("cargo");
    cmd.args(["nextest", "run", "-p", "pampa", "-E", &filter])
        .env("PAMPA_PANDOC_ORACLE_BYPASS_VERSION_GATE", "1")
        .current_dir(&root);
    let test_output = cmd
        .output()
        .context("running cargo nextest for the oracle tests")?;

    let combined = format!(
        "{}\n{}",
        String::from_utf8_lossy(&test_output.stdout),
        String::from_utf8_lossy(&test_output.stderr)
    );
    print!("{combined}");

    if test_output.status.success() {
        println!(
            "\nAll {} oracle tests pass against {first_line}.",
            ORACLE_TEST_NAMES.len()
        );
        if detected > max {
            println!(
                "Detected version is past the calibrated ceiling \u{2014} bump it:\n  \
                 {}:{max_line}\n  const PANDOC_ORACLE_MAX_VERSION: (u32, u32) = ({}, {});",
                test_rs_path.display(),
                detected.0,
                detected.1
            );
        }
        Ok(())
    } else {
        let broken = failed_test_names(&combined);
        if broken.is_empty() {
            bail!("oracle tests failed against {first_line} (see output above)");
        }
        bail!(
            "oracle tests failed against {first_line}: {}",
            broken.join(", ")
        );
    }
}

/// Find `const {name}: (u32, u32) = (MAJOR, MINOR);` in `source` and return
/// its parsed value plus its 1-based line number.
fn find_version_const(source: &str, name: &str) -> Result<((u32, u32), usize)> {
    let needle = format!("const {name}: (u32, u32) = (");
    for (idx, line) in source.lines().enumerate() {
        let Some(pos) = line.find(&needle) else {
            continue;
        };
        let rest = &line[pos + needle.len()..];
        let end = rest
            .find(')')
            .with_context(|| format!("malformed `{name}` line: {line}"))?;
        let mut parts = rest[..end].split(',').map(str::trim);
        let major: u32 = parts
            .next()
            .and_then(|s| s.parse().ok())
            .with_context(|| format!("bad major version in `{name}` line: {line}"))?;
        let minor: u32 = parts
            .next()
            .and_then(|s| s.parse().ok())
            .with_context(|| format!("bad minor version in `{name}` line: {line}"))?;
        return Ok(((major, minor), idx + 1));
    }
    bail!("could not find `const {name}: (u32, u32) = (...)` in {TEST_RS_RELATIVE_PATH}")
}

/// Parse nextest's human-readable output for `FAIL ... <test-name>` lines,
/// returning the trailing test names of the tests that broke.
fn failed_test_names(nextest_output: &str) -> Vec<String> {
    nextest_output
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim_start();
            trimmed
                .strip_prefix("FAIL ")
                .and_then(|rest| rest.rsplit(' ').next())
                .map(|name| name.to_string())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_SOURCE: &str = "const PANDOC_ORACLE_MIN_VERSION: (u32, u32) = (3, 6);\nconst PANDOC_ORACLE_MAX_VERSION: (u32, u32) = (3, 9);\n";

    #[test]
    fn find_version_const_locates_min_and_max() {
        let (min, min_line) =
            find_version_const(SAMPLE_SOURCE, "PANDOC_ORACLE_MIN_VERSION").unwrap();
        assert_eq!(min, (3, 6));
        assert_eq!(min_line, 1);

        let (max, max_line) =
            find_version_const(SAMPLE_SOURCE, "PANDOC_ORACLE_MAX_VERSION").unwrap();
        assert_eq!(max, (3, 9));
        assert_eq!(max_line, 2);
    }

    #[test]
    fn oracle_test_filter_scopes_to_integration_binary_and_all_oracle_names() {
        let filter = oracle_test_filter();
        assert!(
            filter.starts_with("binary(integration) & ("),
            "filter must scope to the integration binary to avoid matching \
             unrelated lib/bin tests with the same substring: {filter}"
        );
        for name in ORACLE_TEST_NAMES {
            assert!(
                filter.contains(&format!("test({name})")),
                "filter missing {name}: {filter}"
            );
        }
    }

    #[test]
    fn find_version_const_missing_name_errors() {
        assert!(find_version_const(SAMPLE_SOURCE, "NOT_THERE").is_err());
    }

    #[test]
    fn failed_test_names_extracts_names_from_nextest_output() {
        let output = "        FAIL [   0.211s] (1/1) pampa::integration test::unit_test_corpus_matches_pandoc_markdown\n        PASS [   0.100s] (2/2) pampa::integration test::test_json_writer\n";
        assert_eq!(
            failed_test_names(output),
            vec!["test::unit_test_corpus_matches_pandoc_markdown".to_string()]
        );
    }

    #[test]
    fn failed_test_names_empty_when_all_pass() {
        let output = "        PASS [   0.100s] (1/1) pampa::integration test::test_json_writer\n";
        assert!(failed_test_names(output).is_empty());
    }
}
