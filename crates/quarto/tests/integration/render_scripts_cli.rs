/*
 * render_scripts_cli.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * bd-w348iu63 — end-to-end CLI tests for project pre-render /
 * post-render scripts.
 */

//! End-to-end CLI tests for `project.pre-render` / `project.post-render`
//! scripts (bd-w348iu63).
//!
//! These spawn the real `q2` binary against fixture projects whose
//! scripts observe the `QUARTO_PROJECT_*` environment contract by
//! writing what they see to files the test then asserts on.
//!
//! Most fixtures use Python scripts (dispatched by the `.py`
//! extension) and skip gracefully when no Python is on PATH; the
//! direct-exec path gets platform-gated shell / batch variants.
//! Plan: claude-notes/plans/2026-07-29-pre-post-render-scripts.md

use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::TempDir;

const Q2_BIN: &str = env!("CARGO_BIN_EXE_q2");

fn write_file(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, contents).unwrap();
}

fn canonical(p: &Path) -> PathBuf {
    p.canonicalize().unwrap_or_else(|_| p.to_path_buf())
}

/// Run `q2 render <args...>` from `cwd` with extra environment
/// variables applied to the child.
fn run_q2_env(cwd: &Path, args: &[&str], env: &[(&str, &str)]) -> std::process::Output {
    let mut cmd = Command::new(Q2_BIN);
    cmd.current_dir(cwd);
    cmd.arg("render");
    for a in args {
        cmd.arg(a);
    }
    for (k, v) in env {
        cmd.env(k, v);
    }
    cmd.output().expect("spawn q2 binary")
}

fn run_q2(cwd: &Path, args: &[&str]) -> std::process::Output {
    run_q2_env(cwd, args, &[])
}

/// Find a Python interpreter for fixture scripts, mirroring the
/// candidate order the dispatcher uses. `None` ⇒ the test should
/// skip (prints a note so the skip is visible in test output).
fn find_python() -> Option<&'static str> {
    let candidates: &[&str] = if cfg!(windows) {
        &["python", "python3"]
    } else {
        &["python3", "python"]
    };
    for candidate in candidates {
        if let Ok(status) = Command::new(candidate)
            .arg("--version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            && status.success()
        {
            return Some(candidate);
        }
    }
    None
}

/// Skip macro: returns early from the test with a visible note when
/// no Python interpreter is available.
macro_rules! require_python {
    () => {
        match find_python() {
            Some(py) => py,
            None => {
                eprintln!("SKIP: no python interpreter on PATH");
                return;
            }
        }
    };
}

fn write_minimal_project(project: &Path, quarto_yml: &str) {
    write_file(&project.join("_quarto.yml"), quarto_yml);
    write_file(
        &project.join("index.qmd"),
        "---\ntitle: Home\n---\n\nHome body.\n",
    );
    write_file(&project.join("a.qmd"), "---\ntitle: A\n---\n\nA body.\n");
}

/// A Python script that dumps every `QUARTO_*` environment variable
/// to `<name>` as `KEY=VALUE` lines (values with newlines are
/// JSON-escaped so the dump stays line-oriented).
fn env_dump_script(name: &str) -> String {
    format!(
        r#"import os, json
with open({name:?}, "w") as f:
    for k in sorted(os.environ):
        if k.startswith("QUARTO_"):
            f.write(k + "=" + json.dumps(os.environ[k]) + "\n")
"#
    )
}

/// Parse an env-dump file produced by [`env_dump_script`] into
/// (key, decoded-value) pairs.
fn read_env_dump(path: &Path) -> Vec<(String, String)> {
    let content = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("read env dump {}: {e}", path.display()));
    content
        .lines()
        .map(|line| {
            let (k, v) = line.split_once('=').expect("KEY=VALUE line");
            let decoded: String = serde_json::from_str(v).expect("JSON-quoted value");
            (k.to_string(), decoded)
        })
        .collect()
}

fn dump_value<'a>(dump: &'a [(String, String)], key: &str) -> Option<&'a str> {
    dump.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
}

// === Tests ============================================================

/// A pre-render script creates a new `.qmd`; the same `q2 render`
/// invocation renders it (the project is discovered after the script
/// runs). String config form.
#[test]
fn pre_render_script_creates_input_file() {
    require_python!();
    let temp = TempDir::new().unwrap();
    let project = canonical(temp.path());
    write_minimal_project(
        &project,
        "project:\n  type: website\n  output-dir: _site\n  pre-render: gen.py\n",
    );
    write_file(
        &project.join("gen.py"),
        r#"import os
if not os.path.exists("generated.qmd"):
    with open("generated.qmd", "w") as f:
        f.write("---\ntitle: Generated\n---\n\nGenerated body.\n")
"#,
    );

    let out = run_q2(&project, &[]);
    assert!(
        out.status.success(),
        "render should succeed; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        project.join("generated.qmd").exists(),
        "pre-render script should have created generated.qmd"
    );
    let html_path = project.join("_site/generated.html");
    assert!(
        html_path.exists(),
        "script-created input should be rendered in the same pass"
    );
    let html = std::fs::read_to_string(&html_path).unwrap();
    assert!(
        html.contains("Generated body."),
        "rendered HTML should contain the generated body; got:\n{html}"
    );
}

/// Post-render scripts receive `QUARTO_PROJECT_OUTPUT_FILES`
/// (newline-separated, project-relative) computed fresh from the
/// pipeline's actual outputs, plus the shared vars.
#[test]
fn post_render_script_receives_output_files() {
    require_python!();
    let temp = TempDir::new().unwrap();
    let project = canonical(temp.path());
    write_minimal_project(
        &project,
        "project:\n  type: website\n  output-dir: _site\n  post-render:\n    - capture.py\n",
    );
    write_file(
        &project.join("capture.py"),
        &env_dump_script("post-env.txt"),
    );

    let out = run_q2(&project, &[]);
    assert!(
        out.status.success(),
        "render should succeed; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let dump = read_env_dump(&project.join("post-env.txt"));
    let output_files =
        dump_value(&dump, "QUARTO_PROJECT_OUTPUT_FILES").expect("OUTPUT_FILES set for post-render");
    let listed: Vec<&str> = output_files.lines().collect();
    // Project-relative paths, one per line. Both pages must appear.
    let expect_index = Path::new("_site").join("index.html");
    let expect_a = Path::new("_site").join("a.html");
    for expected in [&expect_index, &expect_a] {
        assert!(
            listed.iter().any(|l| Path::new(l) == *expected),
            "OUTPUT_FILES should list {}; got: {listed:?}",
            expected.display()
        );
    }
    // Post-render must NOT see the pre-render-only var.
    assert!(
        dump_value(&dump, "QUARTO_PROJECT_INPUT_FILES").is_none(),
        "INPUT_FILES is pre-render-only"
    );
    // Shared vars.
    assert_eq!(
        dump_value(&dump, "QUARTO_PROJECT_DIR").map(Path::new),
        Some(project.as_path()),
        "QUARTO_PROJECT_DIR should be the absolute project dir"
    );
    assert_eq!(
        dump_value(&dump, "QUARTO_PROJECT_OUTPUT_DIR").map(Path::new),
        Some(project.join("_site").as_path()),
        "QUARTO_PROJECT_OUTPUT_DIR should be the absolute output dir"
    );
}

/// Full-project render: `QUARTO_PROJECT_RENDER_ALL` is `"1"` and
/// `QUARTO_PROJECT_INPUT_FILES` lists every input, project-relative.
#[test]
fn env_contract_full_render() {
    require_python!();
    let temp = TempDir::new().unwrap();
    let project = canonical(temp.path());
    write_minimal_project(
        &project,
        "project:\n  type: website\n  output-dir: _site\n  pre-render: dump.py\n",
    );
    write_file(&project.join("dump.py"), &env_dump_script("pre-env.txt"));

    let out = run_q2(&project, &[]);
    assert!(
        out.status.success(),
        "render should succeed; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let dump = read_env_dump(&project.join("pre-env.txt"));
    assert_eq!(
        dump_value(&dump, "QUARTO_PROJECT_RENDER_ALL"),
        Some("1"),
        "full render sets RENDER_ALL=1"
    );
    let input_files =
        dump_value(&dump, "QUARTO_PROJECT_INPUT_FILES").expect("INPUT_FILES set for pre-render");
    let listed: Vec<&str> = input_files.lines().collect();
    for expected in ["index.qmd", "a.qmd"] {
        assert!(
            listed.iter().any(|l| Path::new(l) == Path::new(expected)),
            "INPUT_FILES should list {expected}; got: {listed:?}"
        );
    }
    assert_eq!(
        dump_value(&dump, "QUARTO_PROJECT_DIR").map(Path::new),
        Some(project.as_path()),
    );
    // Post-render-only var must be absent during pre-render.
    assert!(
        dump_value(&dump, "QUARTO_PROJECT_OUTPUT_FILES").is_none(),
        "OUTPUT_FILES is post-render-only"
    );
}

/// Single-file render *inside* a project still runs the scripts
/// (Q1-compatible), but `QUARTO_PROJECT_RENDER_ALL` is absent and
/// `INPUT_FILES` names only the targeted file.
#[test]
fn env_contract_subset_render() {
    require_python!();
    let temp = TempDir::new().unwrap();
    let project = canonical(temp.path());
    write_minimal_project(
        &project,
        "project:\n  type: website\n  output-dir: _site\n  pre-render: dump.py\n",
    );
    write_file(&project.join("dump.py"), &env_dump_script("pre-env.txt"));

    let out = run_q2(&project, &["a.qmd"]);
    assert!(
        out.status.success(),
        "subset render should succeed; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let dump = read_env_dump(&project.join("pre-env.txt"));
    assert_eq!(
        dump_value(&dump, "QUARTO_PROJECT_RENDER_ALL"),
        None,
        "RENDER_ALL must be absent (not \"0\") on a partial render"
    );
    let input_files = dump_value(&dump, "QUARTO_PROJECT_INPUT_FILES").expect("INPUT_FILES set");
    let listed: Vec<&str> = input_files.lines().collect();
    assert_eq!(
        listed.len(),
        1,
        "subset render passes only the targeted file; got: {listed:?}"
    );
    assert_eq!(Path::new(listed[0]), Path::new("a.qmd"));
}

/// A failing pre-render script aborts the render with a diagnostic
/// naming the script and its exit code; later scripts do not run and
/// nothing is rendered.
#[test]
fn failing_pre_render_script_aborts() {
    require_python!();
    let temp = TempDir::new().unwrap();
    let project = canonical(temp.path());
    write_minimal_project(
        &project,
        "project:\n  type: website\n  output-dir: _site\n  pre-render:\n    - fail.py\n    - after.py\n",
    );
    write_file(
        &project.join("fail.py"),
        "import sys\nsys.stderr.write(\"boom from fail.py\\n\")\nsys.exit(3)\n",
    );
    write_file(
        &project.join("after.py"),
        "open(\"after-ran.txt\", \"w\").write(\"x\")\n",
    );

    let out = run_q2(&project, &[]);
    assert!(
        !out.status.success(),
        "failing pre-render script must abort the render"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("fail.py"),
        "diagnostic should name the failing script; got: {stderr}"
    );
    assert!(
        stderr.contains('3'),
        "diagnostic should report the exit code (3); got: {stderr}"
    );
    // The script's own stderr passes through.
    assert!(
        stderr.contains("boom from fail.py"),
        "script stderr should be visible; got: {stderr}"
    );
    assert!(
        !project.join("after-ran.txt").exists(),
        "scripts after the failing one must not run"
    );
    assert!(
        !project.join("_site/index.html").exists(),
        "pre-render failure must abort before any rendering"
    );
    // bd-m6wmztln control: the Q-5-8 snippet anchors in the file that
    // declared the script (`path:line:col` in the ariadne header).
    assert!(
        stderr.contains("_quarto.yml:"),
        "snippet should anchor in _quarto.yml for a directly-declared script; got: {stderr}"
    );
}

/// bd-m6wmztln: a failing pre-render script *contributed by an
/// extension* (`contributes.metadata.project.pre-render`) must anchor
/// its Q-5-8 snippet in the extension's `_extension.yml`. The script's
/// `SourceInfo` carries the manifest's filename-hash FileId; binding
/// `_quarto.yml`'s content to it (the old behavior) rendered the
/// manifest's byte offsets against the wrong file — a misleading span
/// when the offsets fit, a silently dropped snippet when they didn't.
#[test]
fn failing_extension_contributed_script_snippet_names_extension_yml() {
    require_python!();
    let temp = TempDir::new().unwrap();
    let project = canonical(temp.path());
    // Enough `_quarto.yml` content that the manifest's byte offsets
    // land inside it: on a regression this reproduces the worse
    // misleading-span variant, not just a dropped snippet.
    write_minimal_project(
        &project,
        "project:\n  type: website\n  output-dir: _site\n  render:\n    - \"**/*.qmd\"\n    - \"!drafts/\"\n",
    );
    write_file(
        &project.join("_extensions/acme/failing/_extension.yml"),
        "title: failing\nauthor: Acme\nversion: 0.0.1\ncontributes:\n  metadata:\n    project:\n      pre-render:\n        - fail.py\n",
    );
    // Lives in the extension dir, so the contributed entry is rebased
    // to `_extensions/acme/failing/fail.py` — rebasing must preserve
    // the manifest anchor.
    write_file(
        &project.join("_extensions/acme/failing/fail.py"),
        "import sys\nsys.exit(3)\n",
    );

    let out = run_q2(&project, &[]);
    assert!(
        !out.status.success(),
        "failing extension-contributed pre-render script must abort the render"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("[Q-5-8]"),
        "diagnostic should carry the Q-5-8 code; got: {stderr}"
    );
    assert!(
        stderr.contains("_extension.yml:"),
        "snippet should anchor in the extension manifest; got: {stderr}"
    );
    assert!(
        !stderr.contains("_quarto.yml:"),
        "snippet must not anchor in _quarto.yml (the script is not declared there); got: {stderr}"
    );
    assert!(
        stderr.contains("extension manifest"),
        "diagnostic should attribute the entry to the extension manifest; got: {stderr}"
    );
}

/// A pre-render script that changes `project.output-dir` in
/// `_quarto.yml` triggers the mutation guard: the render aborts with
/// a diagnostic.
#[test]
fn forbidden_output_dir_mutation_aborts() {
    require_python!();
    let temp = TempDir::new().unwrap();
    let project = canonical(temp.path());
    write_minimal_project(
        &project,
        "project:\n  type: website\n  output-dir: _site\n  pre-render: mutate.py\n",
    );
    write_file(
        &project.join("mutate.py"),
        r#"with open("_quarto.yml", "w") as f:
    f.write("project:\n  type: website\n  output-dir: _other\n  pre-render: mutate.py\n")
"#,
    );

    let out = run_q2(&project, &[]);
    assert!(
        !out.status.success(),
        "output-dir mutation by a pre-render script must abort the render; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("output-dir"),
        "diagnostic should name the forbidden key; got: {stderr}"
    );
    assert!(
        !project.join("_other").exists() && !project.join("_site/index.html").exists(),
        "no rendering should happen after a forbidden mutation"
    );
}

/// A pre-render script that changes `project.type` also trips the
/// mutation guard.
#[test]
fn forbidden_project_type_mutation_aborts() {
    require_python!();
    let temp = TempDir::new().unwrap();
    let project = canonical(temp.path());
    write_minimal_project(
        &project,
        "project:\n  type: website\n  output-dir: _site\n  pre-render: mutate.py\n",
    );
    write_file(
        &project.join("mutate.py"),
        r#"with open("_quarto.yml", "w") as f:
    f.write("project:\n  type: default\n  output-dir: _site\n  pre-render: mutate.py\n")
"#,
    );

    let out = run_q2(&project, &[]);
    assert!(
        !out.status.success(),
        "project.type mutation by a pre-render script must abort the render"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("type"),
        "diagnostic should name the forbidden key; got: {stderr}"
    );
}

/// List config form: multiple pre-render scripts run in declaration
/// order, each with the project root as cwd.
#[test]
fn list_form_runs_scripts_in_order() {
    require_python!();
    let temp = TempDir::new().unwrap();
    let project = canonical(temp.path());
    write_minimal_project(
        &project,
        "project:\n  type: website\n  output-dir: _site\n  pre-render:\n    - one.py\n    - two.py\n",
    );
    write_file(
        &project.join("one.py"),
        "open(\"order.log\", \"a\").write(\"one\\n\")\n",
    );
    write_file(
        &project.join("two.py"),
        "open(\"order.log\", \"a\").write(\"two\\n\")\n",
    );

    let out = run_q2(&project, &[]);
    assert!(
        out.status.success(),
        "render should succeed; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let log = std::fs::read_to_string(project.join("order.log")).expect("order.log written");
    assert_eq!(log, "one\ntwo\n", "scripts must run in declaration order");
}

/// An explicit-interpreter command line (`<python> script.py arg`)
/// bypasses extension dispatch; arguments (including double-quoted
/// ones) reach the script.
#[test]
fn explicit_interpreter_command_line_with_args() {
    let py = require_python!();
    let temp = TempDir::new().unwrap();
    let project = canonical(temp.path());
    write_minimal_project(
        &project,
        &format!(
            "project:\n  type: website\n  output-dir: _site\n  pre-render: {py} args.py --flag \"two words\"\n"
        ),
    );
    write_file(
        &project.join("args.py"),
        r#"import sys
with open("args.txt", "w") as f:
    f.write("\n".join(sys.argv[1:]))
"#,
    );

    let out = run_q2(&project, &[]);
    assert!(
        out.status.success(),
        "render should succeed; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let args = std::fs::read_to_string(project.join("args.txt")).expect("args.txt written");
    assert_eq!(
        args, "--flag\ntwo words",
        "quoted argument should arrive as a single argv entry"
    );
}

/// `--no-render-scripts` skips both script phases.
#[test]
fn no_render_scripts_flag_skips_scripts() {
    require_python!();
    let temp = TempDir::new().unwrap();
    let project = canonical(temp.path());
    write_minimal_project(
        &project,
        "project:\n  type: website\n  output-dir: _site\n  pre-render: pre.py\n  post-render: post.py\n",
    );
    write_file(
        &project.join("pre.py"),
        "open(\"pre-ran.txt\", \"w\").write(\"x\")\n",
    );
    write_file(
        &project.join("post.py"),
        "open(\"post-ran.txt\", \"w\").write(\"x\")\n",
    );

    let out = run_q2(&project, &["--no-render-scripts"]);
    assert!(
        out.status.success(),
        "render should succeed; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        !project.join("pre-ran.txt").exists(),
        "--no-render-scripts must skip pre-render scripts"
    );
    assert!(
        !project.join("post-ran.txt").exists(),
        "--no-render-scripts must skip post-render scripts"
    );
    assert!(
        project.join("_site/index.html").exists(),
        "the render itself still happens"
    );
}

/// `QUARTO_USE_FILE_FOR_PROJECT_INPUT_FILES=<path>` diverts the input
/// list to a file; the env var is then not set on the script.
#[test]
fn input_files_escape_hatch_writes_file() {
    require_python!();
    let temp = TempDir::new().unwrap();
    let project = canonical(temp.path());
    write_minimal_project(
        &project,
        "project:\n  type: website\n  output-dir: _site\n  pre-render: dump.py\n",
    );
    write_file(&project.join("dump.py"), &env_dump_script("pre-env.txt"));

    let list_file = project.join("input-list.txt");
    let out = run_q2_env(
        &project,
        &[],
        &[(
            "QUARTO_USE_FILE_FOR_PROJECT_INPUT_FILES",
            list_file.to_str().unwrap(),
        )],
    );
    assert!(
        out.status.success(),
        "render should succeed; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let dump = read_env_dump(&project.join("pre-env.txt"));
    assert!(
        dump_value(&dump, "QUARTO_PROJECT_INPUT_FILES").is_none(),
        "with the escape hatch active the env var must not be set"
    );
    let listing = std::fs::read_to_string(&list_file).expect("input list file written");
    let listed: Vec<&str> = listing.lines().collect();
    for expected in ["index.qmd", "a.qmd"] {
        assert!(
            listed.iter().any(|l| Path::new(l) == Path::new(expected)),
            "list file should contain {expected}; got: {listed:?}"
        );
    }
}

/// Direct-exec dispatch (no recognized extension): a shell script
/// with shebang + exec bit on Unix.
#[cfg(unix)]
#[test]
fn direct_exec_shell_script_runs() {
    use std::os::unix::fs::PermissionsExt;
    let temp = TempDir::new().unwrap();
    let project = canonical(temp.path());
    write_minimal_project(
        &project,
        "project:\n  type: website\n  output-dir: _site\n  pre-render: mark.sh\n",
    );
    let script = project.join("mark.sh");
    write_file(&script, "#!/bin/sh\necho marker > sh-ran.txt\n");
    let mut perms = std::fs::metadata(&script).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&script, perms).unwrap();

    let out = run_q2(&project, &[]);
    assert!(
        out.status.success(),
        "render should succeed; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        project.join("sh-ran.txt").exists(),
        "direct-exec shell script should have run from the project root"
    );
}

/// Direct-exec dispatch on Windows: a `.bat` script.
#[cfg(windows)]
#[test]
fn direct_exec_batch_script_runs() {
    let temp = TempDir::new().unwrap();
    let project = canonical(temp.path());
    write_minimal_project(
        &project,
        "project:\n  type: website\n  output-dir: _site\n  pre-render: mark.bat\n",
    );
    write_file(
        &project.join("mark.bat"),
        "@echo off\r\necho marker > bat-ran.txt\r\n",
    );

    let out = run_q2(&project, &[]);
    assert!(
        out.status.success(),
        "render should succeed; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        project.join("bat-ran.txt").exists(),
        "direct-exec batch script should have run from the project root"
    );
}

/// Underscore-typo guard: `project.pre_render` (wrong spelling) emits
/// a warning naming the correct key; the render still succeeds and no
/// script runs.
#[test]
fn underscore_typo_warns_and_renders() {
    let temp = TempDir::new().unwrap();
    let project = canonical(temp.path());
    write_minimal_project(
        &project,
        "project:\n  type: website\n  output-dir: _site\n  pre_render: nope.py\n",
    );
    // Deliberately no nope.py on disk — it must never be looked up.

    let out = run_q2(&project, &[]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "typo is a warning, not an error; stderr: {stderr}"
    );
    assert!(
        stderr.contains("pre_render") && stderr.contains("pre-render"),
        "warning should name both the typo and the correct spelling; got: {stderr}"
    );
    assert!(project.join("_site/index.html").exists());
}

/// A bare single-file render outside any project never runs scripts
/// (and the script wiring must not require a project config).
#[test]
fn no_project_renders_without_scripts() {
    let temp = TempDir::new().unwrap();
    let dir = canonical(temp.path());
    write_file(
        &dir.join("solo.qmd"),
        "---\ntitle: Solo\n---\n\nSolo body.\n",
    );

    let out = run_q2(&dir, &["solo.qmd"]);
    assert!(
        out.status.success(),
        "single-file render outside a project should succeed; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(dir.join("solo.html").exists());
}
