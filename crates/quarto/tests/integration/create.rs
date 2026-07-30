//! End-to-end CLI tests for `q2 create` (bd-oa5kd2yr).
//!
//! Each test spawns the real `q2` binary as a subprocess and asserts on
//! the files it writes, its exit code, and its stdout/stderr contracts.
//!
//! Contract under verification (per
//! `claude-notes/plans/2026-07-23-q2-create-command.md`):
//!
//! - Positional path: `q2 create project <choice> <dir> [title]`,
//!   non-interactive. Title defaults to the directory name (or the
//!   choice id for `.`) with a warning on stderr.
//! - Directory semantics: create-or-reuse dir; hard error iff the dir
//!   already contains `_quarto.yml`/`_quarto.yaml`; individual files
//!   that already exist are skipped, never overwritten.
//! - `.gitignore`: `/.quarto/` entry written; appended (not clobbered)
//!   when a `.gitignore` already exists.
//! - Machine path: `q2 create --json` reads one JSON directive from
//!   stdin and writes exactly one JSON result object to stdout;
//!   diagnostics (errors, warnings) go to stderr as JSON lines.
//!   `q2 create --list --json` emits the artifact/choice registry.
//! - `--dry-run` / `"dry_run": true`: full file plan reported, nothing
//!   written.
//!
//! TDD note: these tests are written *before* the implementation and
//! must fail (command returns "not implemented") before Phase 2.

use std::path::Path;
use std::process::{Command, Stdio};

use serde_json::Value;
use tempfile::TempDir;

const Q2_BIN: &str = env!("CARGO_BIN_EXE_q2");

/// Run `q2 create <args...>` from `cwd`.
fn run_q2_create(cwd: &Path, args: &[&str]) -> std::process::Output {
    let mut cmd = Command::new(Q2_BIN);
    cmd.current_dir(cwd);
    cmd.arg("create");
    for a in args {
        cmd.arg(a);
    }
    cmd.output().expect("spawn q2 binary")
}

/// Run `q2 create <args...>` from `cwd`, feeding `input` on stdin.
fn run_q2_create_stdin(cwd: &Path, args: &[&str], input: &str) -> std::process::Output {
    let mut cmd = Command::new(Q2_BIN);
    cmd.current_dir(cwd);
    cmd.arg("create");
    for a in args {
        cmd.arg(a);
    }
    cmd.stdin(Stdio::piped());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());
    let mut child = cmd.spawn().expect("spawn q2 binary");
    {
        use std::io::Write;
        let mut stdin = child.stdin.take().expect("child stdin");
        stdin.write_all(input.as_bytes()).expect("write stdin");
    }
    child.wait_with_output().expect("wait for q2 binary")
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

/// Parse the whole stdout as exactly one JSON value (the `--json`
/// stdout-purity contract: nothing on stdout but the result object).
fn parse_single_json(stdout: &str) -> Value {
    serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("stdout must be a single JSON value: {e}\nstdout:\n{stdout}"))
}

/// JSON lines found on stderr (diagnostics contract in `--json` mode).
fn stderr_json_lines(stderr: &str) -> Vec<Value> {
    stderr
        .lines()
        .filter_map(|line| {
            let t = line.trim_start();
            if !t.starts_with('{') {
                return None;
            }
            serde_json::from_str::<Value>(t).ok()
        })
        .collect()
}

/// The action recorded for `rel_path` in a JSON result's `files` array.
fn file_action<'a>(result: &'a Value, rel_path: &str) -> &'a str {
    result["files"]
        .as_array()
        .expect("result.files must be an array")
        .iter()
        .find(|f| f["path"] == rel_path)
        .unwrap_or_else(|| panic!("no files entry for {rel_path} in {result}"))["action"]
        .as_str()
        .expect("files[].action must be a string")
}

// ====================================================================
// Positional path: happy cases
// ====================================================================

#[test]
fn website_scaffold_written_with_defaulted_title() {
    let tmp = TempDir::new().unwrap();
    let out = run_q2_create(tmp.path(), &["project", "website", "mysite"]);
    assert!(
        out.status.success(),
        "exit: {:?}\nstderr: {}",
        out.status,
        stderr_str(&out)
    );

    let dir = tmp.path().join("mysite");
    for f in [
        "_quarto.yml",
        "index.qmd",
        "about.qmd",
        "styles.css",
        ".gitignore",
    ] {
        assert!(dir.join(f).is_file(), "missing {f}");
    }

    // Title defaults to the directory name and lands under `website:`
    // (what Q2's website pipeline reads).
    let yml = parse_yaml_file(&dir.join("_quarto.yml"));
    assert_eq!(yml["project"]["type"].as_str(), Some("website"));
    assert_eq!(yml["website"]["title"].as_str(), Some("mysite"));

    // The defaulting is surfaced as a warning on stderr.
    assert!(
        stderr_str(&out).contains("using \"mysite\" as the project title"),
        "stderr: {}",
        stderr_str(&out)
    );

    // Human stdout reports the created files and a render hint.
    let stdout = stdout_str(&out);
    assert!(stdout.contains("_quarto.yml"), "stdout: {stdout}");
    assert!(stdout.contains("q2 render"), "stdout: {stdout}");

    // .gitignore carries the Q2 scratch dir (and only entries Q2 can
    // actually produce — no `.quarto_ipynb` pattern).
    let gitignore = read(&dir.join(".gitignore"));
    assert!(gitignore.contains("/.quarto/"), "gitignore: {gitignore}");
    assert!(
        !gitignore.contains("quarto_ipynb"),
        "gitignore: {gitignore}"
    );
}

#[test]
fn explicit_title_positional_is_used_without_warning() {
    let tmp = TempDir::new().unwrap();
    let out = run_q2_create(tmp.path(), &["project", "website", "mysite", "My Site"]);
    assert!(out.status.success(), "stderr: {}", stderr_str(&out));

    let yml = parse_yaml_file(&tmp.path().join("mysite/_quarto.yml"));
    assert_eq!(yml["website"]["title"].as_str(), Some("My Site"));

    let index = read(&tmp.path().join("mysite/index.qmd"));
    assert!(index.contains("title: \"My Site\""));

    assert!(
        !stderr_str(&out).contains("as the project title"),
        "no defaulting warning expected; stderr: {}",
        stderr_str(&out)
    );
}

#[test]
fn special_character_title_produces_valid_yaml_on_disk() {
    let tmp = TempDir::new().unwrap();
    let title = r#"R & D "quoted" \ backslash"#;
    let out = run_q2_create(tmp.path(), &["project", "website", "mysite", title]);
    assert!(out.status.success(), "stderr: {}", stderr_str(&out));

    // The written config must round-trip through a YAML parser back to
    // the original title (raw `&`, escaped quotes/backslashes).
    let yml = parse_yaml_file(&tmp.path().join("mysite/_quarto.yml"));
    assert_eq!(yml["website"]["title"].as_str(), Some(title));
}

#[test]
fn default_project_scaffold_written() {
    let tmp = TempDir::new().unwrap();
    let out = run_q2_create(tmp.path(), &["project", "default", "myproj", "My Project"]);
    assert!(out.status.success(), "stderr: {}", stderr_str(&out));

    let dir = tmp.path().join("myproj");
    let yml = parse_yaml_file(&dir.join("_quarto.yml"));
    assert_eq!(yml["project"]["title"].as_str(), Some("My Project"));

    let index = read(&dir.join("index.qmd"));
    assert!(index.contains("title: \"My Project\""));
    assert!(index.contains("## Quarto"));
}

#[test]
fn dot_directory_scaffolds_into_cwd_with_choice_id_title() {
    let tmp = TempDir::new().unwrap();
    let out = run_q2_create(tmp.path(), &["project", "website", "."]);
    assert!(out.status.success(), "stderr: {}", stderr_str(&out));

    // Files land directly in cwd; the title defaults to the choice id
    // (a directory named "." would be a useless title).
    let yml = parse_yaml_file(&tmp.path().join("_quarto.yml"));
    assert_eq!(yml["website"]["title"].as_str(), Some("website"));
    assert!(tmp.path().join("index.qmd").is_file());
}

// ====================================================================
// Positional path: directory semantics
// ====================================================================

#[test]
fn existing_quarto_yml_is_a_hard_error() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path().join("mysite");
    std::fs::create_dir(&dir).unwrap();
    std::fs::write(dir.join("_quarto.yml"), "project:\n  title: \"keep\"\n").unwrap();

    let out = run_q2_create(tmp.path(), &["project", "website", "mysite"]);
    assert!(!out.status.success());
    assert!(
        stderr_str(&out).contains("already contains a Quarto project"),
        "stderr: {}",
        stderr_str(&out)
    );

    // Nothing else was written, and the existing config is untouched.
    assert_eq!(
        read(&dir.join("_quarto.yml")),
        "project:\n  title: \"keep\"\n"
    );
    assert!(!dir.join("index.qmd").exists());
}

#[test]
fn existing_quarto_yaml_extension_also_errors() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path().join("mysite");
    std::fs::create_dir(&dir).unwrap();
    std::fs::write(dir.join("_quarto.yaml"), "project: {}\n").unwrap();

    let out = run_q2_create(tmp.path(), &["project", "website", "mysite"]);
    assert!(!out.status.success());
    assert!(
        stderr_str(&out).contains("already contains a Quarto project"),
        "stderr: {}",
        stderr_str(&out)
    );
}

#[test]
fn existing_unrelated_file_is_skipped_not_overwritten() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path().join("mysite");
    std::fs::create_dir(&dir).unwrap();
    std::fs::write(dir.join("index.qmd"), "my precious content\n").unwrap();

    let out = run_q2_create(tmp.path(), &["project", "website", "mysite", "T"]);
    assert!(out.status.success(), "stderr: {}", stderr_str(&out));

    // Pre-existing file untouched; the rest of the scaffold written.
    assert_eq!(read(&dir.join("index.qmd")), "my precious content\n");
    assert!(dir.join("_quarto.yml").is_file());
    assert!(dir.join("about.qmd").is_file());
}

#[test]
fn existing_gitignore_is_appended_not_clobbered() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path().join("mysite");
    std::fs::create_dir(&dir).unwrap();
    std::fs::write(dir.join(".gitignore"), "node_modules/\n").unwrap();

    let out = run_q2_create(tmp.path(), &["project", "website", "mysite", "T"]);
    assert!(out.status.success(), "stderr: {}", stderr_str(&out));

    let gitignore = read(&dir.join(".gitignore"));
    assert!(
        gitignore.contains("node_modules/"),
        "gitignore: {gitignore}"
    );
    assert!(gitignore.contains("/.quarto/"), "gitignore: {gitignore}");
}

// ====================================================================
// Positional path: error cases
// ====================================================================

#[test]
fn unknown_artifact_type_lists_valid_types() {
    let tmp = TempDir::new().unwrap();
    let out = run_q2_create(tmp.path(), &["frobnicate"]);
    assert!(!out.status.success());
    let stderr = stderr_str(&out);
    assert!(stderr.contains("frobnicate"), "stderr: {stderr}");
    assert!(stderr.contains("project"), "stderr: {stderr}");
}

#[test]
fn bare_create_lists_valid_types() {
    let tmp = TempDir::new().unwrap();
    let out = run_q2_create(tmp.path(), &[]);
    assert!(!out.status.success());
    assert!(
        stderr_str(&out).contains("project"),
        "stderr: {}",
        stderr_str(&out)
    );
}

#[test]
fn unknown_choice_lists_valid_choices() {
    let tmp = TempDir::new().unwrap();
    let out = run_q2_create(tmp.path(), &["project", "frobnicate", "d"]);
    assert!(!out.status.success());
    let stderr = stderr_str(&out);
    assert!(stderr.contains("frobnicate"), "stderr: {stderr}");
    assert!(stderr.contains("default"), "stderr: {stderr}");
    assert!(stderr.contains("website"), "stderr: {stderr}");
}

#[test]
fn unimplemented_choice_says_so() {
    let tmp = TempDir::new().unwrap();
    let out = run_q2_create(tmp.path(), &["project", "manuscript", "mypaper"]);
    assert!(!out.status.success());
    let stderr = stderr_str(&out);
    // The error must name the choice, not just be a generic stub.
    assert!(stderr.contains("manuscript"), "stderr: {stderr}");
    assert!(stderr.contains("not yet implemented"), "stderr: {stderr}");
    assert!(!tmp.path().join("mypaper").exists());
}

#[test]
fn colon_form_routes_through_template_parser() {
    let tmp = TempDir::new().unwrap();
    // `website:solitaire` is valid grammar but no such template
    // exists — the error must be about implementation, not syntax.
    let out = run_q2_create(tmp.path(), &["project", "website:solitaire", "d"]);
    assert!(!out.status.success());
    let stderr = stderr_str(&out);
    // The error must name the parsed target, proving the colon form was
    // accepted as grammar and rejected only on implementation status.
    assert!(stderr.contains("website:solitaire"), "stderr: {stderr}");
    assert!(stderr.contains("not yet implemented"), "stderr: {stderr}");
}

// ====================================================================
// Blog scaffold (bd-r1by4u2a)
// ====================================================================

#[test]
fn create_project_blog_writes_full_scaffold_including_binaries() {
    let tmp = TempDir::new().unwrap();
    let out = run_q2_create(tmp.path(), &["project", "blog", "myblog", "My Blog"]);
    assert!(
        out.status.success(),
        "stderr: {}\nstdout: {}",
        stderr_str(&out),
        stdout_str(&out)
    );
    let dir = tmp.path().join("myblog");

    for rel in [
        "_quarto.yml",
        "index.qmd",
        "about.qmd",
        "styles.css",
        ".gitignore",
        "posts/_metadata.yml",
        "posts/welcome/index.qmd",
        "posts/welcome/thumbnail.jpg",
        "posts/post-with-code/index.qmd",
        "posts/post-with-code/image.jpg",
    ] {
        assert!(dir.join(rel).exists(), "missing {rel}");
    }

    // The binaries must round-trip byte-identical through the
    // ScaffoldContent::Binary path and the disk writer.
    let embedded = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../quarto-project-create/resources/templates/website/blog");
    for (written, source) in [
        ("posts/welcome/thumbnail.jpg", "posts/welcome/thumbnail.jpg"),
        (
            "posts/post-with-code/image.jpg",
            "posts/post-with-code/image.jpg",
        ),
    ] {
        let got = std::fs::read(dir.join(written)).unwrap();
        let want = std::fs::read(embedded.join(source)).unwrap();
        assert_eq!(got, want, "{written} must be byte-identical");
    }

    let yml = parse_yaml_file(&dir.join("_quarto.yml"));
    assert_eq!(yml["website"]["title"].as_str(), Some("My Blog"));
    assert_eq!(yml["project"]["type"].as_str(), Some("website"));

    // The listing page carries Q1's canonical listing config.
    let index = read(&dir.join("index.qmd"));
    assert!(index.contains("contents: posts"), "index.qmd:\n{index}");
    assert!(index.contains("feed: true"));

    // Posts are date-stamped today / today-minus-3-days.
    let post = read(&dir.join("posts/post-with-code/index.qmd"));
    assert!(post.contains("date: \"2"), "post front matter:\n{post}");
    assert!(!post.contains('$'), "template residue:\n{post}");
}

#[test]
fn missing_directory_argument_errors() {
    let tmp = TempDir::new().unwrap();
    let out = run_q2_create(tmp.path(), &["project", "website"]);
    assert!(!out.status.success());
    assert!(
        stderr_str(&out).contains("directory"),
        "stderr: {}",
        stderr_str(&out)
    );
}

// ====================================================================
// Dry run (positional path)
// ====================================================================

#[test]
fn dry_run_reports_plan_and_writes_nothing() {
    let tmp = TempDir::new().unwrap();
    let out = run_q2_create(
        tmp.path(),
        &["project", "website", "mysite", "T", "--dry-run"],
    );
    assert!(out.status.success(), "stderr: {}", stderr_str(&out));

    let stdout = stdout_str(&out);
    assert!(stdout.contains("_quarto.yml"), "stdout: {stdout}");
    assert!(stdout.contains("dry run"), "stdout: {stdout}");
    assert!(
        !tmp.path().join("mysite").exists(),
        "dry run must not write"
    );
}

#[test]
fn dry_run_still_errors_on_existing_project() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path().join("mysite");
    std::fs::create_dir(&dir).unwrap();
    std::fs::write(dir.join("_quarto.yml"), "project: {}\n").unwrap();

    let out = run_q2_create(
        tmp.path(),
        &["project", "website", "mysite", "T", "--dry-run"],
    );
    assert!(!out.status.success());
    assert!(
        stderr_str(&out).contains("already contains a Quarto project"),
        "stderr: {}",
        stderr_str(&out)
    );
}

// ====================================================================
// JSON directive mode
// ====================================================================

#[test]
fn json_directive_creates_project_and_reports_result() {
    let tmp = TempDir::new().unwrap();
    let directive =
        r#"{"artifact":"project","directory":"mysite","choice":"website","title":"My Site"}"#;
    let out = run_q2_create_stdin(tmp.path(), &["--json"], directive);
    assert!(out.status.success(), "stderr: {}", stderr_str(&out));

    let result = parse_single_json(&stdout_str(&out));
    assert_eq!(result["version"], 1);
    assert_eq!(result["dry_run"], false);

    // Path is absolute and points at the created directory.
    let path = Path::new(result["path"].as_str().expect("result.path"));
    assert!(
        path.is_absolute(),
        "path must be absolute: {}",
        path.display()
    );
    assert!(path.ends_with("mysite"));

    for f in [
        "_quarto.yml",
        "index.qmd",
        "about.qmd",
        "styles.css",
        ".gitignore",
    ] {
        assert_eq!(file_action(&result, f), "created");
        assert!(tmp.path().join("mysite").join(f).is_file(), "missing {f}");
    }

    let yml = parse_yaml_file(&tmp.path().join("mysite/_quarto.yml"));
    assert_eq!(yml["website"]["title"].as_str(), Some("My Site"));
}

#[test]
fn json_directive_reports_skipped_existing() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path().join("mysite");
    std::fs::create_dir(&dir).unwrap();
    std::fs::write(dir.join("styles.css"), "/* mine */\n").unwrap();

    let directive = r#"{"artifact":"project","directory":"mysite","choice":"website","title":"T"}"#;
    let out = run_q2_create_stdin(tmp.path(), &["--json"], directive);
    assert!(out.status.success(), "stderr: {}", stderr_str(&out));

    let result = parse_single_json(&stdout_str(&out));
    assert_eq!(file_action(&result, "styles.css"), "skipped-existing");
    assert_eq!(file_action(&result, "index.qmd"), "created");
    assert_eq!(read(&dir.join("styles.css")), "/* mine */\n");
}

#[test]
fn json_directive_without_title_warns_on_stderr_stdout_stays_pure() {
    let tmp = TempDir::new().unwrap();
    let directive = r#"{"artifact":"project","directory":"mysite","choice":"website"}"#;
    let out = run_q2_create_stdin(tmp.path(), &["--json"], directive);
    assert!(out.status.success(), "stderr: {}", stderr_str(&out));

    // stdout is exactly one JSON value even with a warning in play.
    let result = parse_single_json(&stdout_str(&out));
    assert_eq!(result["version"], 1);

    // The warning is a JSON line on stderr mentioning the default.
    let warnings = stderr_json_lines(&stderr_str(&out));
    assert!(
        warnings.iter().any(|w| w.to_string().contains("mysite")),
        "expected JSON warning naming the defaulted title; stderr: {}",
        stderr_str(&out)
    );
}

#[test]
fn json_malformed_directive_errors_with_json_diagnostic() {
    let tmp = TempDir::new().unwrap();
    let out = run_q2_create_stdin(tmp.path(), &["--json"], "this is not json");
    assert!(!out.status.success());
    assert_eq!(
        stdout_str(&out).trim(),
        "",
        "stdout must stay empty on error"
    );
    assert!(
        !stderr_json_lines(&stderr_str(&out)).is_empty(),
        "expected JSON diagnostic on stderr; stderr: {}",
        stderr_str(&out)
    );
}

#[test]
fn json_unknown_field_is_rejected() {
    let tmp = TempDir::new().unwrap();
    let directive = r#"{"artifact":"project","directory":"d","choice":"website","title":"T","frobnicate":true}"#;
    let out = run_q2_create_stdin(tmp.path(), &["--json"], directive);
    assert!(!out.status.success());
    assert!(
        stderr_str(&out).contains("frobnicate"),
        "stderr: {}",
        stderr_str(&out)
    );
    assert!(!tmp.path().join("d").exists());
}

#[test]
fn json_unknown_choice_errors_with_json_diagnostic() {
    let tmp = TempDir::new().unwrap();
    let directive = r#"{"artifact":"project","directory":"d","choice":"nope","title":"T"}"#;
    let out = run_q2_create_stdin(tmp.path(), &["--json"], directive);
    assert!(!out.status.success());
    assert_eq!(stdout_str(&out).trim(), "");
    let diags = stderr_json_lines(&stderr_str(&out));
    assert!(
        diags.iter().any(|d| d.to_string().contains("nope")),
        "expected JSON diagnostic naming the bad choice; stderr: {}",
        stderr_str(&out)
    );
}

#[test]
fn json_dry_run_reports_plan_and_writes_nothing() {
    let tmp = TempDir::new().unwrap();
    let directive = r#"{"artifact":"project","directory":"mysite","choice":"website","title":"T","dry_run":true}"#;
    let out = run_q2_create_stdin(tmp.path(), &["--json"], directive);
    assert!(out.status.success(), "stderr: {}", stderr_str(&out));

    let result = parse_single_json(&stdout_str(&out));
    assert_eq!(result["dry_run"], true);
    assert_eq!(file_action(&result, "_quarto.yml"), "created");
    assert!(
        !tmp.path().join("mysite").exists(),
        "dry run must not write"
    );
}

#[test]
fn json_with_positionals_is_rejected() {
    let tmp = TempDir::new().unwrap();
    let out = run_q2_create_stdin(tmp.path(), &["project", "--json"], "{}");
    assert!(!out.status.success());
}

// ====================================================================
// Capability discovery (--list)
// ====================================================================

#[test]
fn list_json_emits_registry_with_implemented_flags() {
    let tmp = TempDir::new().unwrap();
    let out = run_q2_create(tmp.path(), &["--list", "--json"]);
    assert!(out.status.success(), "stderr: {}", stderr_str(&out));

    let listing = parse_single_json(&stdout_str(&out));
    assert_eq!(listing["version"], 1);

    let artifacts = listing["artifacts"].as_array().expect("artifacts array");
    let project = artifacts
        .iter()
        .find(|a| a["type"] == "project")
        .expect("project artifact in listing");

    let choices = project["choices"].as_array().expect("choices array");
    let by_id = |id: &str| -> &Value {
        choices
            .iter()
            .find(|c| c["id"] == id)
            .unwrap_or_else(|| panic!("choice {id} missing from listing"))
    };
    assert_eq!(by_id("website")["implemented"], true);
    assert_eq!(by_id("default")["implemented"], true);
    assert_eq!(by_id("blog")["implemented"], true);
    assert_eq!(by_id("manuscript")["implemented"], false);
}

#[test]
fn list_human_output_names_choices() {
    let tmp = TempDir::new().unwrap();
    let out = run_q2_create(tmp.path(), &["--list"]);
    assert!(out.status.success(), "stderr: {}", stderr_str(&out));
    let stdout = stdout_str(&out);
    assert!(stdout.contains("website"), "stdout: {stdout}");
    assert!(stdout.contains("default"), "stdout: {stdout}");
}

// ====================================================================
// Interactive-prompt gating (bd-hh1erpfx)
// ====================================================================
//
// All tests in this file run with piped stdio, so nothing here can
// ever legitimately prompt — these tests pin the non-interactive
// contract and the explicit opt-outs.

#[test]
fn no_prompt_flag_with_missing_args_errors() {
    let tmp = TempDir::new().unwrap();
    let out = run_q2_create(tmp.path(), &["project", "website", "--no-prompt"]);
    assert!(!out.status.success());
    assert!(
        stderr_str(&out).contains("directory"),
        "stderr: {}",
        stderr_str(&out)
    );
}

#[test]
fn ci_env_with_missing_args_errors_without_prompting() {
    let tmp = TempDir::new().unwrap();
    let mut cmd = Command::new(Q2_BIN);
    cmd.current_dir(tmp.path());
    cmd.env("CI", "1");
    cmd.args(["create", "project", "website"]);
    let out = cmd.output().expect("spawn q2 binary");
    assert!(!out.status.success());
    assert!(
        stderr_str(&out).contains("directory"),
        "stderr: {}",
        stderr_str(&out)
    );
}
