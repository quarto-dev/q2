//! Lint: every npm-workspace package with a `test` script must be run by a
//! step in one of the CI workflows, or be listed in `EXCUSED` with a reason.
//!
//! Why this exists (GH #250, 2026-08-22): CI ran `hub-client`'s tests and
//! `engine-host-deno`'s, and nothing else. Roughly 2,000 assertions across a
//! dozen packages sat outside the merge gate for months — long enough for a
//! KaTeX class rename to redden `preview-renderer` on `main` unnoticed. The
//! wiring drifted because nothing reconciled "packages that have tests"
//! against "packages CI runs". This rule is that reconciliation.
//!
//! It parses the workflows and looks only at each step's `run:` script, rather
//! than substring-matching the file. Two holes that closes:
//!
//!   * A whole-file match would be satisfied by a *build* step:
//!     `ts-packages/quarto-hub-mcp` appears in the MCP smoke check and
//!     `hub-client` in the WASM build, so deleting either package's test step
//!     would still pass.
//!   * A line/chunk text match would let a step with no `name:` merge into its
//!     neighbour and inherit a test marker it never had.
//!
//! **Limit, deliberate for v1:** keys on `scripts.test` only. A package with
//! `test:integration` or `test:e2e` but no `test` is invisible here — e.g.
//! q2-preview-spa's Playwright specs. Widening to any `test*` script is
//! follow-up work; do not assume coverage this does not have.
//!
//! Like `error_docs` and `error_docs_sidebar`, this is a *repo-level* rule: it
//! compares two trees rather than grepping one Rust file, so it runs once per
//! lint invocation.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use super::Violation;

const RULE: &str = "ci-test-suite-unwired";

/// Workflows that may satisfy the rule. `sync-test-harness` lives in the e2e
/// workflow because its hub tier shells out to `cargo run --bin hub`, which
/// only that workflow pre-builds — hence a list, not a single file.
const WORKFLOWS: &[&str] = &[
    ".github/workflows/ts-test-suite.yml",
    ".github/workflows/hub-client-e2e.yml",
];

/// Substrings that mark a step's `run:` script as actually running tests.
const TEST_MARKERS: &[&str] = &["npm test", "npm run test", "deno test", "vitest"];

/// Packages deliberately not gated, with the reason. Keep this list short and
/// always name the tracking strand or issue — an entry here is a promise to
/// come back, not a place to park work.
pub(crate) const EXCUSED: &[(&str, &str)] = &[(
    "@quarto/annotated-qmd",
    "2 of 156 tests fail on a source-tracking off-by-one; tracked by bd-1d6io. \
     Wire it in once that strand closes.",
)];

/// Expand the root `package.json` `workspaces` globs into package directories.
///
/// Only the trailing-`*` form npm actually uses here is supported
/// (`ts-packages/*`, `q2-demos/*`); anything else is treated as a literal path.
fn workspace_dirs(root: &Path, globs: &[String]) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    for glob in globs {
        if let Some(prefix) = glob.strip_suffix("/*") {
            let parent = root.join(prefix);
            let Ok(entries) = std::fs::read_dir(&parent) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.join("package.json").is_file() {
                    dirs.push(path);
                }
            }
        } else {
            let path = root.join(glob);
            if path.join("package.json").is_file() {
                dirs.push(path);
            }
        }
    }
    dirs.sort();
    dirs
}

/// The `run:` script of every step that looks like it runs tests.
///
/// Parsed, not text-matched: `jobs.<job>.steps[].run`. A step without a `run:`
/// (a `uses:` action) contributes nothing, and a step without a `name:` is
/// still its own step — both are why this parses instead of splitting lines.
fn test_run_scripts(workflow_yaml: &str) -> Vec<String> {
    let Ok(doc) = serde_yaml::from_str::<serde_yaml::Value>(workflow_yaml) else {
        return Vec::new();
    };
    let Some(jobs) = doc.get("jobs").and_then(|j| j.as_mapping()) else {
        return Vec::new();
    };

    let mut scripts = Vec::new();
    for (_job_name, job) in jobs {
        let Some(steps) = job.get("steps").and_then(|s| s.as_sequence()) else {
            continue;
        };
        for step in steps {
            let Some(run) = step.get("run").and_then(|r| r.as_str()) else {
                continue;
            };
            if TEST_MARKERS.iter().any(|marker| run.contains(marker)) {
                scripts.push(run.to_string());
            }
        }
    }
    scripts
}

/// 1-indexed line of the `"test":` key in a package.json, or 1 if not found.
fn test_script_line(manifest: &str) -> usize {
    manifest
        .lines()
        .position(|line| line.contains("\"test\":"))
        .map_or(1, |i| i + 1)
}

pub fn check(workspace_root: &Path) -> Result<Vec<Violation>> {
    let root_manifest_path = workspace_root.join("package.json");
    let Ok(root_manifest) = std::fs::read_to_string(&root_manifest_path) else {
        // No npm workspace here (e.g. a Rust-only checkout) — nothing to check.
        return Ok(Vec::new());
    };
    let root: serde_json::Value = serde_json::from_str(&root_manifest)
        .with_context(|| format!("Failed to parse {}", root_manifest_path.display()))?;

    let globs: Vec<String> = root
        .get("workspaces")
        .and_then(|w| w.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();

    // Every test-running `run:` script across every scanned workflow.
    //
    // Bail only if *no* workflow file could be read (a partial checkout);
    // workflows that exist but run no tests must still produce violations,
    // otherwise the rule would go quiet exactly when everything is unwired.
    let mut test_scripts: Vec<String> = Vec::new();
    let mut found_a_workflow = false;
    for wf in WORKFLOWS {
        if let Ok(body) = std::fs::read_to_string(workspace_root.join(wf)) {
            found_a_workflow = true;
            test_scripts.extend(test_run_scripts(&body));
        }
    }
    if !found_a_workflow {
        return Ok(Vec::new());
    }

    let mut violations = Vec::new();

    for dir in workspace_dirs(workspace_root, &globs) {
        let manifest_path = dir.join("package.json");
        let manifest = std::fs::read_to_string(&manifest_path)
            .with_context(|| format!("Failed to read {}", manifest_path.display()))?;
        let pkg: serde_json::Value = serde_json::from_str(&manifest)
            .with_context(|| format!("Failed to parse {}", manifest_path.display()))?;

        let has_test = pkg
            .get("scripts")
            .and_then(|s| s.get("test"))
            .and_then(|t| t.as_str())
            .is_some_and(|t| !t.trim().is_empty());
        if !has_test {
            continue;
        }

        let name = pkg.get("name").and_then(|n| n.as_str()).unwrap_or_default();
        if EXCUSED.iter().any(|(excused, _)| *excused == name) {
            continue;
        }

        // The workspace path as npm's `-w` flag spells it, e.g.
        // `ts-packages/quarto-api`.
        let rel = dir
            .strip_prefix(workspace_root)
            .unwrap_or(&dir)
            .to_string_lossy()
            .replace('\\', "/");

        let wired = test_scripts
            .iter()
            .any(|script| script.contains(&rel) || (!name.is_empty() && script.contains(name)));
        if wired {
            continue;
        }

        violations.push(Violation {
            file: manifest_path.clone(),
            line: test_script_line(&manifest),
            column: 1,
            rule: RULE,
            message: format!(
                "{} has a `test` script but no CI step runs it — its tests are \
                 not gated (scanned: {})",
                if name.is_empty() { &rel } else { name },
                WORKFLOWS.join(", ")
            ),
            suggestion: Some(format!(
                "Add a step running `npm test -w {}` to one of {}, or add the \
                 package to EXCUSED in crates/xtask/src/lint/ci_test_wiring.rs \
                 with a reason and a tracking strand.",
                rel,
                WORKFLOWS.join(" / ")
            )),
        });
    }

    Ok(violations)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Build a fake repo: root package.json with `workspaces`, one package per
    /// (path, name, has_test), and the two workflow files the rule scans.
    fn scaffold(
        globs: &[&str],
        packages: &[(&str, &str, bool)],
        ts_suite: &str,
        e2e: &str,
    ) -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();

        let globs_json = globs
            .iter()
            .map(|g| format!("\"{}\"", g))
            .collect::<Vec<_>>()
            .join(",");
        fs::write(
            root.join("package.json"),
            format!("{{\"workspaces\":[{}]}}", globs_json),
        )
        .unwrap();

        for (path, name, has_test) in packages {
            let dir = root.join(path);
            fs::create_dir_all(&dir).unwrap();
            // Multi-line on purpose: the violation's line anchor points at the
            // `"test":` line, which a single-line manifest cannot exercise.
            let manifest = if *has_test {
                format!(
                    "{{\n  \"name\": \"{}\",\n  \"scripts\": {{\n    \"build\": \"tsc\",\n    \"test\": \"vitest run\"\n  }}\n}}\n",
                    name
                )
            } else {
                format!(
                    "{{\n  \"name\": \"{}\",\n  \"scripts\": {{\n    \"build\": \"tsc\"\n  }}\n}}\n",
                    name
                )
            };
            fs::write(dir.join("package.json"), manifest).unwrap();
        }

        let wf_dir = root.join(".github/workflows");
        fs::create_dir_all(&wf_dir).unwrap();
        fs::write(wf_dir.join("ts-test-suite.yml"), ts_suite).unwrap();
        fs::write(wf_dir.join("hub-client-e2e.yml"), e2e).unwrap();

        tmp
    }

    /// A workflow with no steps at all — valid YAML, zero test scripts.
    const NO_STEPS: &str = "jobs:\n  a-job:\n    steps: []\n";

    #[test]
    fn flags_a_tested_package_absent_from_the_workflows() {
        let tmp = scaffold(
            &["ts-packages/*"],
            &[("ts-packages/lonely", "@quarto/lonely", true)],
            NO_STEPS,
            NO_STEPS,
        );
        let violations = check(tmp.path()).unwrap();
        assert_eq!(violations.len(), 1, "{:?}", violations);
        assert_eq!(violations[0].rule, "ci-test-suite-unwired");
        assert!(
            violations[0].message.contains("@quarto/lonely"),
            "{}",
            violations[0].message
        );
        assert!(
            violations[0]
                .file
                .ends_with("ts-packages/lonely/package.json")
        );
        // The scaffolded manifest puts `"test":` on line 5.
        assert_eq!(violations[0].line, 5);
    }

    #[test]
    fn accepts_a_package_whose_test_step_names_its_path() {
        let tmp = scaffold(
            &["ts-packages/*"],
            &[("ts-packages/wired", "@quarto/wired", true)],
            "jobs:\n  a-job:\n    steps:\n      - name: Run wired\n        run: npm test -w ts-packages/wired\n",
            NO_STEPS,
        );
        assert!(check(tmp.path()).unwrap().is_empty());
    }

    #[test]
    fn accepts_a_package_matched_by_npm_name() {
        let tmp = scaffold(
            &["ts-packages/*"],
            &[("ts-packages/deno-host", "@quarto/engine-host-deno", true)],
            "jobs:\n  a-job:\n    steps:\n      - name: Run deno host\n        run: npm run test -w @quarto/engine-host-deno\n",
            NO_STEPS,
        );
        assert!(check(tmp.path()).unwrap().is_empty());
    }

    #[test]
    fn accepts_a_package_wired_in_the_e2e_workflow() {
        let tmp = scaffold(
            &["ts-packages/*"],
            &[("ts-packages/harness", "@quarto/harness", true)],
            NO_STEPS,
            "jobs:\n  e2e-tests:\n    steps:\n      - name: Run harness\n        run: npm test -w ts-packages/harness\n",
        );
        assert!(check(tmp.path()).unwrap().is_empty());
    }

    #[test]
    fn accepts_a_multi_line_run_that_cds_then_tests() {
        // hub-client's real shape: `cd hub-client` and `npm run test:ci` are
        // two lines of one step's `run:` block.
        let tmp = scaffold(
            &["hub-client"],
            &[("hub-client", "hub-client", true)],
            "jobs:\n  a-job:\n    steps:\n      - name: Run hub-client tests\n        run: |\n          cd hub-client\n          npm run test:ci\n",
            NO_STEPS,
        );
        assert!(check(tmp.path()).unwrap().is_empty());
    }

    #[test]
    fn accepts_a_step_with_no_name() {
        // What parsing buys over text-chunking: a nameless step is still a
        // step, and its run script still counts.
        let tmp = scaffold(
            &["ts-packages/*"],
            &[("ts-packages/nameless", "@quarto/nameless", true)],
            "jobs:\n  a-job:\n    steps:\n      - run: npm test -w ts-packages/nameless\n",
            NO_STEPS,
        );
        assert!(check(tmp.path()).unwrap().is_empty());
    }

    #[test]
    fn a_build_step_mention_does_not_count_as_wired() {
        // The regression this rule almost shipped with: Task 3's smoke check
        // names ts-packages/quarto-hub-mcp in a *build* step.
        let tmp = scaffold(
            &["ts-packages/*"],
            &[("ts-packages/quarto-hub-mcp", "@quarto/hub-mcp", true)],
            "jobs:\n  a-job:\n    steps:\n      - name: Smoke-check\n        run: node ts-packages/quarto-hub-mcp/dist/index.js --help\n",
            NO_STEPS,
        );
        let violations = check(tmp.path()).unwrap();
        assert_eq!(violations.len(), 1, "{:?}", violations);
        assert!(violations[0].message.contains("@quarto/hub-mcp"));
    }

    #[test]
    fn a_uses_step_cannot_inherit_a_neighbours_marker() {
        // A `uses:` step has no `run:`, so it can never satisfy the rule --
        // even sitting immediately after a real test step.
        let tmp = scaffold(
            &["ts-packages/*"],
            &[("ts-packages/sneaky", "@quarto/sneaky", true)],
            "jobs:\n  a-job:\n    steps:\n      - name: Run other\n        run: npm test -w ts-packages/other\n      - uses: actions/upload-artifact@v4\n        with:\n          path: ts-packages/sneaky\n",
            NO_STEPS,
        );
        let violations = check(tmp.path()).unwrap();
        assert_eq!(violations.len(), 1, "{:?}", violations);
        assert!(violations[0].message.contains("@quarto/sneaky"));
    }

    #[test]
    fn ignores_packages_without_a_test_script() {
        let tmp = scaffold(
            &["ts-packages/*"],
            &[("ts-packages/typesonly", "@quarto/types", false)],
            NO_STEPS,
            NO_STEPS,
        );
        assert!(check(tmp.path()).unwrap().is_empty());
    }

    #[test]
    fn ignores_excused_packages() {
        // Skips itself if the excuse list has been emptied (see Task 7).
        let Some(&(excused_name, _)) = EXCUSED.first() else {
            return;
        };
        let tmp = scaffold(
            &["ts-packages/*"],
            &[("ts-packages/excused-pkg", excused_name, true)],
            NO_STEPS,
            NO_STEPS,
        );
        assert!(check(tmp.path()).unwrap().is_empty());
    }

    #[test]
    fn expands_literal_and_glob_workspace_entries() {
        let tmp = scaffold(
            &["ts-packages/*", "trace-viewer"],
            &[
                ("ts-packages/a", "@quarto/a", true),
                ("trace-viewer", "@quarto/trace-viewer", true),
            ],
            "jobs:\n  a-job:\n    steps:\n      - name: Run a\n        run: npm test -w ts-packages/a\n",
            NO_STEPS,
        );
        let violations = check(tmp.path()).unwrap();
        assert_eq!(violations.len(), 1, "{:?}", violations);
        assert!(violations[0].message.contains("@quarto/trace-viewer"));
    }
}
