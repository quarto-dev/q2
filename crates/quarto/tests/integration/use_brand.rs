//! End-to-end CLI tests for `q2 use brand` (bd-1vlw8).
//!
//! Each test spawns the real `q2` binary as a subprocess and asserts on
//! the files it writes (and, just as importantly, the ones it does
//! *not*), its exit code, and its stdout/stderr contracts.
//!
//! Contract under verification (per
//! `claude-notes/plans/2026-07-28-q2-use-brand-command.md`):
//!
//! - **The declaration is the point.** Q2 has no `_brand.yml`
//!   auto-discovery, so dropping a brand file on disk without writing a
//!   `brand:` key into `_quarto.yml` would be a silent no-op at render
//!   time. `render_picks_up_the_declared_brand` is the test that proves
//!   the two halves connect; the rest are guardrails around it.
//! - **Pre-flight gates refuse before writing anything.** No
//!   `_quarto.yml` → refuse (and never synthesize one). A root brand
//!   file already present → refuse. A `brand:` already declared →
//!   refuse. A config we cannot safely append to → refuse.
//! - **Edits are append-only.** Everything above the insertion point in
//!   `_quarto.yml` stays byte-identical, comments included.
//! - **`--dry-run` reports the full plan and writes nothing.**
//! - **`--force` and `--trust` are distinct.** `--force` overrides the
//!   local-state gates; `--trust` waives the remote trust prompt.
//!   Neither implies the other.
//!
//! TDD note: written before the implementation, and observed failing
//! (the command returned "not implemented") before Phase 2.

use std::path::Path;
use std::process::{Command, Stdio};

use tempfile::TempDir;

const Q2_BIN: &str = env!("CARGO_BIN_EXE_q2");

/// Run `q2 use <args...>` from `cwd`.
fn run_q2_use(cwd: &Path, args: &[&str]) -> std::process::Output {
    let mut cmd = Command::new(Q2_BIN);
    cmd.current_dir(cwd);
    cmd.arg("use");
    for a in args {
        cmd.arg(a);
    }
    // Keep the prompt gate closed regardless of how the test runner is
    // attached; these tests assert non-interactive behavior.
    cmd.env("CI", "1");
    cmd.stdin(Stdio::null());
    cmd.output().expect("spawn q2 binary")
}

fn stdout_str(out: &std::process::Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr_str(out: &std::process::Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

fn parse_yaml_file(path: &Path) -> serde_yaml::Value {
    serde_yaml::from_str(&read(path))
        .unwrap_or_else(|e| panic!("{} must be valid YAML: {e}", path.display()))
}

fn assert_failed(out: &std::process::Output, expect_in_stderr: &str) {
    assert!(
        !out.status.success(),
        "expected failure, got success.\nstdout: {}\nstderr: {}",
        stdout_str(out),
        stderr_str(out)
    );
    let stderr = stderr_str(out);
    assert!(
        stderr.contains(expect_in_stderr),
        "stderr should mention {expect_in_stderr:?}; got:\n{stderr}"
    );
}

fn assert_succeeded(out: &std::process::Output) {
    assert!(
        out.status.success(),
        "expected success.\nstdout: {}\nstderr: {}",
        stdout_str(out),
        stderr_str(out)
    );
}

/// A project directory containing a `_quarto.yml` with `content`.
fn project_with_config(content: &str) -> TempDir {
    let tmp = TempDir::new().unwrap();
    std::fs::write(tmp.path().join("_quarto.yml"), content).unwrap();
    tmp
}

const MINIMAL_CONFIG: &str = "project:\n  type: website\n";

/// No brand file of any spelling exists at `root`.
fn assert_no_brand_files(root: &Path) {
    for f in ["_brand.yml", "_brand.yaml"] {
        assert!(
            !root.join(f).exists(),
            "{f} must not have been written to {}",
            root.display()
        );
    }
    assert!(
        !root.join("_brand").exists(),
        "_brand/ must not have been created"
    );
}

// ====================================================================
// Case 1 — the project gate
// ====================================================================

#[test]
fn no_quarto_yml_refuses_and_creates_nothing() {
    let tmp = TempDir::new().unwrap();
    let out = run_q2_use(tmp.path(), &["brand"]);

    assert_failed(&out, "_quarto.yml");
    // The command must never synthesize a project config.
    assert!(
        !tmp.path().join("_quarto.yml").exists(),
        "q2 use brand must not create _quarto.yml"
    );
    assert!(!tmp.path().join("_quarto.yaml").exists());
    assert_no_brand_files(tmp.path());
}

#[test]
fn no_quarto_yml_error_points_at_q2_create() {
    let tmp = TempDir::new().unwrap();
    let out = run_q2_use(tmp.path(), &["brand"]);
    let stderr = stderr_str(&out);
    assert!(
        stderr.contains("q2 create"),
        "the error should tell the user how to get a project; got:\n{stderr}"
    );
}

#[test]
fn project_root_is_found_by_walking_up() {
    let tmp = project_with_config(MINIMAL_CONFIG);
    let nested = tmp.path().join("posts").join("2026");
    std::fs::create_dir_all(&nested).unwrap();

    let out = run_q2_use(&nested, &["brand"]);
    assert_succeeded(&out);

    // The brand lands at the project root, not in the cwd.
    assert!(tmp.path().join("_brand.yml").is_file());
    assert!(!nested.join("_brand.yml").exists());
}

// ====================================================================
// Case 4 — the existing-brand-file gate
// ====================================================================

#[test]
fn existing_root_brand_yml_refuses_before_writing() {
    let tmp = project_with_config(MINIMAL_CONFIG);
    std::fs::write(tmp.path().join("_brand.yml"), "color:\n  primary: red\n").unwrap();

    let out = run_q2_use(tmp.path(), &["brand"]);

    assert_failed(&out, "_brand.yml");
    // The user's file is untouched...
    assert_eq!(
        read(&tmp.path().join("_brand.yml")),
        "color:\n  primary: red\n"
    );
    // ...and the config was not edited.
    assert_eq!(read(&tmp.path().join("_quarto.yml")), MINIMAL_CONFIG);
}

#[test]
fn existing_root_brand_yaml_spelling_also_refuses() {
    let tmp = project_with_config(MINIMAL_CONFIG);
    std::fs::write(tmp.path().join("_brand.yaml"), "color:\n").unwrap();

    let out = run_q2_use(tmp.path(), &["brand"]);
    assert_failed(&out, "_brand.yaml");
    assert_eq!(read(&tmp.path().join("_quarto.yml")), MINIMAL_CONFIG);
}

#[test]
fn rerunning_is_a_hard_error_not_a_silent_noop() {
    // Deliberate departure from `q2 create`'s skip-existing merge
    // semantics: the second run must say why it is refusing, so
    // "run it again" is obviously not the fix.
    let tmp = project_with_config(MINIMAL_CONFIG);
    assert_succeeded(&run_q2_use(tmp.path(), &["brand"]));

    let second = run_q2_use(tmp.path(), &["brand"]);
    assert!(
        !second.status.success(),
        "a second run must fail, not silently succeed"
    );
}

// ====================================================================
// Case 9 — the editability gate
// ====================================================================

#[test]
fn multi_document_config_is_refused_not_corrupted() {
    let original = "project:\n  type: website\n---\nproject:\n  type: book\n";
    let tmp = project_with_config(original);

    let out = run_q2_use(tmp.path(), &["brand"]);

    assert!(!out.status.success(), "a multi-doc config must be refused");
    assert_eq!(
        read(&tmp.path().join("_quarto.yml")),
        original,
        "the config must be left byte-identical"
    );
    assert_no_brand_files(tmp.path());
}

#[test]
fn sequence_at_top_level_is_refused_not_corrupted() {
    let original = "- one\n- two\n";
    let tmp = project_with_config(original);

    let out = run_q2_use(tmp.path(), &["brand"]);

    assert!(
        !out.status.success(),
        "a non-mapping config must be refused"
    );
    assert_eq!(read(&tmp.path().join("_quarto.yml")), original);
    assert_no_brand_files(tmp.path());
}

// ====================================================================
// Cases 2, 3 — the happy path and the declaration
// ====================================================================

#[test]
fn scaffold_writes_brand_file_and_declares_it() {
    let tmp = project_with_config(MINIMAL_CONFIG);
    let out = run_q2_use(tmp.path(), &["brand"]);
    assert_succeeded(&out);

    let brand = tmp.path().join("_brand.yml");
    assert!(brand.is_file(), "_brand.yml must be created");
    // The scaffold must itself be valid, parseable brand YAML.
    let parsed = parse_yaml_file(&brand);
    assert!(
        parsed.is_mapping(),
        "the scaffolded brand must be a YAML mapping"
    );

    // The declaration — the half Q1 does not need and Q2 cannot skip.
    let config = parse_yaml_file(&tmp.path().join("_quarto.yml"));
    assert_eq!(
        config["brand"].as_str(),
        Some("_brand.yml"),
        "_quarto.yml must declare the brand"
    );
}

#[test]
fn config_bytes_above_the_insertion_point_are_preserved() {
    let original = "# My site config\nproject:\n  type: website  # trailing comment\n\n# spacing\nformat:\n  html:\n    toc: true\n";
    let tmp = project_with_config(original);

    assert_succeeded(&run_q2_use(tmp.path(), &["brand"]));

    let updated = read(&tmp.path().join("_quarto.yml"));
    assert!(
        updated.starts_with(original),
        "comments and key order above the insertion point must survive byte-for-byte; got:\n{updated}"
    );
    // And the result is still valid YAML with both the old and new keys.
    let parsed = parse_yaml_file(&tmp.path().join("_quarto.yml"));
    assert_eq!(parsed["format"]["html"]["toc"].as_bool(), Some(true));
    assert_eq!(parsed["brand"].as_str(), Some("_brand.yml"));
}

#[test]
fn config_without_trailing_newline_is_not_spliced() {
    // The failure this guards: appending to a file whose last line has
    // no newline would produce `type: websitebrand: _brand.yml`.
    let tmp = project_with_config("project:\n  type: website");
    assert_succeeded(&run_q2_use(tmp.path(), &["brand"]));

    let parsed = parse_yaml_file(&tmp.path().join("_quarto.yml"));
    assert_eq!(parsed["project"]["type"].as_str(), Some("website"));
    assert_eq!(parsed["brand"].as_str(), Some("_brand.yml"));
}

#[test]
fn quarto_yaml_alternate_extension_is_honored() {
    let tmp = TempDir::new().unwrap();
    std::fs::write(tmp.path().join("_quarto.yaml"), MINIMAL_CONFIG).unwrap();

    assert_succeeded(&run_q2_use(tmp.path(), &["brand"]));

    assert!(tmp.path().join("_brand.yml").is_file());
    let config = parse_yaml_file(&tmp.path().join("_quarto.yaml"));
    assert_eq!(config["brand"].as_str(), Some("_brand.yml"));
    assert!(
        !tmp.path().join("_quarto.yml").exists(),
        "the .yml spelling must not be created alongside .yaml"
    );
}

// ====================================================================
// Cases 5, 6 — the existing-declaration gate
// ====================================================================

#[test]
fn existing_top_level_brand_declaration_refuses() {
    let original = "project:\n  type: website\nbrand: other.yml\n";
    let tmp = project_with_config(original);

    let out = run_q2_use(tmp.path(), &["brand"]);

    assert_failed(&out, "brand");
    assert_eq!(read(&tmp.path().join("_quarto.yml")), original);
    assert_no_brand_files(tmp.path());
}

#[test]
fn existing_declaration_error_quotes_the_declaration() {
    let tmp = project_with_config("project:\n  type: website\nbrand: other.yml\n");
    let out = run_q2_use(tmp.path(), &["brand"]);
    let stderr = stderr_str(&out);
    assert!(
        stderr.contains("other.yml"),
        "the error should show what is already declared; got:\n{stderr}"
    );
}

#[test]
fn existing_format_scoped_brand_declaration_refuses() {
    let original = "project:\n  type: website\nformat:\n  html:\n    brand: other.yml\n";
    let tmp = project_with_config(original);

    let out = run_q2_use(tmp.path(), &["brand"]);

    assert!(!out.status.success(), "a format-scoped brand must be found");
    let stderr = stderr_str(&out);
    assert!(
        stderr.contains("format") && stderr.contains("html"),
        "the error should name where the declaration lives; got:\n{stderr}"
    );
    assert_eq!(read(&tmp.path().join("_quarto.yml")), original);
}

#[test]
fn inline_brand_block_declaration_refuses() {
    let original = "project:\n  type: website\nbrand:\n  color:\n    primary: red\n";
    let tmp = project_with_config(original);

    let out = run_q2_use(tmp.path(), &["brand"]);

    assert!(!out.status.success(), "an inline brand block must be found");
    assert_eq!(read(&tmp.path().join("_quarto.yml")), original);
}

// ====================================================================
// Case 7 — --force scope
// ====================================================================

#[test]
fn force_overrides_the_existing_declaration_gate() {
    let tmp = project_with_config("project:\n  type: website\nbrand: other.yml\n");
    let out = run_q2_use(tmp.path(), &["brand", "--force"]);
    assert_succeeded(&out);
    assert!(tmp.path().join("_brand.yml").is_file());
}

#[test]
fn force_overrides_the_existing_brand_file_gate() {
    let tmp = project_with_config(MINIMAL_CONFIG);
    std::fs::write(tmp.path().join("_brand.yml"), "color:\n  primary: red\n").unwrap();

    let out = run_q2_use(tmp.path(), &["brand", "--force"]);
    assert_succeeded(&out);

    let config = parse_yaml_file(&tmp.path().join("_quarto.yml"));
    assert_eq!(config["brand"].as_str(), Some("_brand.yml"));
}

#[test]
fn trust_does_not_override_local_state_gates() {
    // The flags have distinct scopes: --trust is about fetched content,
    // never about clobbering the user's own files.
    let tmp = project_with_config(MINIMAL_CONFIG);
    std::fs::write(tmp.path().join("_brand.yml"), "color:\n").unwrap();

    let out = run_q2_use(tmp.path(), &["brand", "--trust"]);
    assert_failed(&out, "_brand.yml");
    assert_eq!(read(&tmp.path().join("_quarto.yml")), MINIMAL_CONFIG);
}

// ====================================================================
// Case 8 — --dry-run
// ====================================================================

#[test]
fn dry_run_reports_the_plan_and_writes_nothing() {
    let tmp = project_with_config(MINIMAL_CONFIG);
    let out = run_q2_use(tmp.path(), &["brand", "--dry-run"]);
    assert_succeeded(&out);

    let stdout = stdout_str(&out);
    assert!(
        stdout.contains("_brand.yml"),
        "the dry run must report the brand file; got:\n{stdout}"
    );
    assert!(
        stdout.contains("_quarto.yml"),
        "the dry run must report the config edit; got:\n{stdout}"
    );

    assert_no_brand_files(tmp.path());
    assert_eq!(read(&tmp.path().join("_quarto.yml")), MINIMAL_CONFIG);
}

#[test]
fn dry_run_still_reports_gate_failures() {
    let tmp = project_with_config("project:\n  type: website\nbrand: other.yml\n");
    let out = run_q2_use(tmp.path(), &["brand", "--dry-run"]);
    assert!(
        !out.status.success(),
        "a dry run must report the same refusal a real run would"
    );
}

#[test]
fn force_with_dry_run_is_rejected() {
    let tmp = project_with_config(MINIMAL_CONFIG);
    let out = run_q2_use(tmp.path(), &["brand", "--force", "--dry-run"]);
    assert!(!out.status.success(), "--force --dry-run must be rejected");
}

#[test]
fn trust_with_dry_run_is_rejected() {
    let tmp = project_with_config(MINIMAL_CONFIG);
    let out = run_q2_use(tmp.path(), &["brand", "--trust", "--dry-run"]);
    assert!(!out.status.success(), "--trust --dry-run must be rejected");
}

// ====================================================================
// Case 21 — the machine front door
// ====================================================================

#[test]
fn json_emits_one_result_object_on_stdout() {
    let tmp = project_with_config(MINIMAL_CONFIG);
    let out = run_q2_use(tmp.path(), &["brand", "--json"]);
    assert_succeeded(&out);

    let stdout = stdout_str(&out);
    let result: serde_json::Value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("stdout must be a single JSON value: {e}\nstdout:\n{stdout}"));

    assert_eq!(result["version"], 1);
    assert_eq!(result["dry_run"], false);
    let files = result["files"].as_array().expect("files array");
    let paths: Vec<&str> = files.iter().filter_map(|f| f["path"].as_str()).collect();
    assert!(paths.contains(&"_brand.yml"), "got {paths:?}");
    assert!(paths.contains(&"_quarto.yml"), "got {paths:?}");
}

#[test]
fn json_failure_puts_the_diagnostic_on_stderr_and_leaves_stdout_empty() {
    let tmp = TempDir::new().unwrap(); // no _quarto.yml
    let out = run_q2_use(tmp.path(), &["brand", "--json"]);

    assert!(!out.status.success());
    assert!(
        stdout_str(&out).trim().is_empty(),
        "stdout is reserved for the result object; got: {}",
        stdout_str(&out)
    );
    let stderr = stderr_str(&out);
    let line = stderr
        .lines()
        .find(|l| l.trim_start().starts_with('{'))
        .unwrap_or_else(|| panic!("expected a JSON diagnostic line; got:\n{stderr}"));
    serde_json::from_str::<serde_json::Value>(line.trim())
        .unwrap_or_else(|e| panic!("stderr diagnostic must be JSON: {e}\nline: {line}"));
}

// ====================================================================
// Unknown subcommands
// ====================================================================

#[test]
fn unknown_use_type_is_rejected() {
    let tmp = project_with_config(MINIMAL_CONFIG);
    let out = run_q2_use(tmp.path(), &["nonsense"]);
    assert!(
        !out.status.success(),
        "an unknown use type must be rejected"
    );
}

// ====================================================================
// Case 22 — the end-to-end proof
// ====================================================================
//
// This is the test the whole design exists for. A Q1-faithful port
// would copy a brand file and stop; because Q2 has no auto-discovery,
// the render would be unaffected and every other test here would still
// pass. Only rendering catches that.

#[test]
fn render_picks_up_the_declared_brand() {
    let tmp = project_with_config(MINIMAL_CONFIG);
    std::fs::write(
        tmp.path().join("index.qmd"),
        "---\ntitle: Test\n---\n\nHello.\n",
    )
    .unwrap();

    assert_succeeded(&run_q2_use(tmp.path(), &["brand"]));

    // Give the scaffolded brand a value we can find in the compiled CSS.
    let brand_path = tmp.path().join("_brand.yml");
    std::fs::write(
        &brand_path,
        "color:\n  palette:\n    q2probe: \"#abcdef\"\n  primary: q2probe\n",
    )
    .unwrap();

    let render = Command::new(Q2_BIN)
        .current_dir(tmp.path())
        .args(["render", "index.qmd", "--to", "html"])
        .output()
        .expect("spawn q2 render");
    assert!(
        render.status.success(),
        "render failed.\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&render.stdout),
        String::from_utf8_lossy(&render.stderr)
    );

    // The theme CSS is emitted as a separate file under `_site/`, not
    // inlined, so search the whole output tree rather than the HTML.
    let site = tmp.path().join("_site");
    assert!(site.is_dir(), "render should have produced _site/");
    assert!(
        tree_contains(&site, "abcdef"),
        "the declared brand's primary color must reach the compiled CSS under {}. \
         If this fails while every other test here passes, the brand file landed \
         on disk but was never declared — exactly the Q1-port trap this command \
         exists to avoid.",
        site.display()
    );
}

#[test]
fn the_shipped_starter_brand_renders_without_editing() {
    // Distinct from the test above, which overwrites `_brand.yml` with a
    // probe value and so never exercises the template we actually ship.
    // A typo in the starter brand — an unknown key, a bad font block —
    // would only surface here, at render time, for the user.
    let tmp = project_with_config(MINIMAL_CONFIG);
    std::fs::write(
        tmp.path().join("index.qmd"),
        "---\ntitle: Test\n---\n\nHello.\n",
    )
    .unwrap();

    assert_succeeded(&run_q2_use(tmp.path(), &["brand"]));

    let render = Command::new(Q2_BIN)
        .current_dir(tmp.path())
        .args(["render", "index.qmd", "--to", "html"])
        .output()
        .expect("spawn q2 render");
    assert!(
        render.status.success(),
        "the starter _brand.yml must render as shipped.\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&render.stdout),
        String::from_utf8_lossy(&render.stderr)
    );

    // The starter's accent color should reach the compiled CSS — proof
    // the brand was parsed and applied, not merely tolerated.
    assert!(
        tree_contains(&tmp.path().join("_site"), "2c6fbb"),
        "the starter brand's accent color should appear in the compiled CSS"
    );
}

/// Does any file under `dir` contain `needle` (case-insensitively)?
fn tree_contains(dir: &Path, needle: &str) -> bool {
    let needle = needle.to_lowercase();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if tree_contains(&path, &needle) {
                return true;
            }
        } else if let Ok(text) = std::fs::read_to_string(&path)
            && text.to_lowercase().contains(&needle)
        {
            return true;
        }
    }
    false
}
