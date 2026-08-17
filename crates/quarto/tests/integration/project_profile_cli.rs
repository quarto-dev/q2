/*
 * tests/integration/project_profile_cli.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * CLI end-to-end tests for project profiles (bd-fu16z22k, Phase 2).
 */

//! `--profile` / `QUARTO_PROFILE` through the real `q2` binary.
//!
//! Every test scrubs `QUARTO_PROFILE` from the child environment so a
//! developer's shell cannot leak into assertions, then sets it
//! explicitly where the test wants it. (This is the end-to-end
//! coverage for the env-var glue noted in the Phase 1 plan entry.)

use std::path::Path;
use std::process::Command;

use tempfile::TempDir;

const Q2_BIN: &str = env!("CARGO_BIN_EXE_q2");

/// A project whose `winner` key differs per profile, plus overlays
/// `a`/`b` for ordering tests and a `title` used by the render test.
fn make_fixture() -> TempDir {
    let dir = TempDir::new().expect("temp dir");
    let root = dir.path();
    std::fs::write(
        root.join("_quarto.yml"),
        "project:\n  type: default\nwinner: base\ntitle: Base Title\n",
    )
    .unwrap();
    std::fs::write(
        root.join("_quarto-prod.yml"),
        "winner: prod\ntitle: Production Title\n",
    )
    .unwrap();
    std::fs::write(root.join("_quarto-a.yml"), "winner: from-a\n").unwrap();
    std::fs::write(root.join("_quarto-b.yml"), "winner: from-b\n").unwrap();
    std::fs::write(root.join("index.qmd"), "# Hello\n\nBody text.\n").unwrap();
    dir
}

/// Run `q2 get-config index.qmd winner` in `root` with the given
/// extra args and env; return (exit-ok, stdout, stderr).
fn get_winner(root: &Path, extra: &[&str], env: &[(&str, &str)]) -> (bool, String, String) {
    let mut cmd = Command::new(Q2_BIN);
    cmd.arg("get-config")
        .arg(root.join("index.qmd"))
        .arg("winner")
        .args(extra)
        .env_remove("QUARTO_PROFILE");
    for (k, v) in env {
        cmd.env(k, v);
    }
    let out = cmd.output().expect("q2 runs");
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).trim().to_string(),
        String::from_utf8_lossy(&out.stderr).to_string(),
    )
}

// ── get-config: activation plumbing ─────────────────────────────────

#[test]
fn get_config_without_profiles_sees_base() {
    let dir = make_fixture();
    let (ok, stdout, _) = get_winner(dir.path(), &[], &[]);
    assert!(ok);
    assert_eq!(stdout, "\"base\"");
}

#[test]
fn get_config_profile_flag_selects_overlay() {
    let dir = make_fixture();
    let (ok, stdout, _) = get_winner(dir.path(), &["--profile", "prod"], &[]);
    assert!(ok);
    assert_eq!(stdout, "\"prod\"");
}

#[test]
fn get_config_env_var_selects_overlay() {
    let dir = make_fixture();
    let (ok, stdout, _) = get_winner(dir.path(), &[], &[("QUARTO_PROFILE", "prod")]);
    assert!(ok);
    assert_eq!(stdout, "\"prod\"");
}

#[test]
fn profile_flag_replaces_env_var() {
    // Q1 parity: --profile REPLACES QUARTO_PROFILE, never merges.
    let dir = make_fixture();
    let (ok, stdout, _) = get_winner(dir.path(), &["--profile", "b"], &[("QUARTO_PROFILE", "a")]);
    assert!(ok);
    assert_eq!(stdout, "\"from-b\"");
}

#[test]
fn comma_form_and_repeated_flags_are_equivalent() {
    let dir = make_fixture();
    let (ok1, comma, _) = get_winner(dir.path(), &["--profile", "a,b"], &[]);
    let (ok2, repeated, _) = get_winner(dir.path(), &["--profile", "a", "--profile", "b"], &[]);
    assert!(ok1 && ok2);
    assert_eq!(comma, "\"from-a\"", "first-listed profile wins");
    assert_eq!(repeated, comma);
}

// ── render: e2e through the real pipeline ───────────────────────────

#[test]
fn render_profile_flag_changes_output() {
    let dir = make_fixture();
    let out = Command::new(Q2_BIN)
        .arg("render")
        .arg(dir.path().join("index.qmd"))
        .args(["--profile", "prod", "--quiet"])
        .env_remove("QUARTO_PROFILE")
        .output()
        .expect("q2 runs");
    assert!(
        out.status.success(),
        "render failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let html = std::fs::read_to_string(dir.path().join("index.html")).expect("output exists");
    assert!(
        html.contains("Production Title"),
        "the overlay title must reach the rendered HTML"
    );
}

#[test]
fn render_project_dir_with_profile_flag() {
    let dir = make_fixture();
    let out = Command::new(Q2_BIN)
        .arg("render")
        .arg(dir.path())
        .args(["--profile", "prod", "--quiet"])
        .env_remove("QUARTO_PROFILE")
        .output()
        .expect("q2 runs");
    assert!(
        out.status.success(),
        "project render failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let html = std::fs::read_to_string(dir.path().join("index.html")).expect("output exists");
    assert!(html.contains("Production Title"));
}

#[test]
fn render_verbose_echoes_active_profiles() {
    let dir = make_fixture();
    let out = Command::new(Q2_BIN)
        .arg("render")
        .arg(dir.path().join("index.qmd"))
        .args(["--profile", "prod", "-v"])
        .env_remove("QUARTO_PROFILE")
        .output()
        .expect("q2 runs");
    assert!(out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("prod") && stderr.to_lowercase().contains("profile"),
        "-v must echo the active profile set; stderr: {stderr}"
    );
}

#[test]
fn render_without_verbose_stays_quiet_about_profiles() {
    // Q1 parity: normal output does not announce profiles.
    let dir = make_fixture();
    let out = Command::new(Q2_BIN)
        .arg("render")
        .arg(dir.path().join("index.qmd"))
        .args(["--profile", "prod"])
        .env_remove("QUARTO_PROFILE")
        .output()
        .expect("q2 runs");
    assert!(out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !stderr.to_lowercase().contains("active project profile"),
        "no profile echo without -v; stderr: {stderr}"
    );
}

// ── diagnostics through the binary ──────────────────────────────────

#[test]
fn unknown_profile_warns_but_renders() {
    let dir = make_fixture();
    let out = Command::new(Q2_BIN)
        .arg("render")
        .arg(dir.path().join("index.qmd"))
        .args(["--profile", "produciton"])
        .env_remove("QUARTO_PROFILE")
        .output()
        .expect("q2 runs");
    assert!(
        out.status.success(),
        "an unknown profile warns, it does not abort: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("Q-5-19") && stderr.contains("produciton"),
        "stderr must carry the Q-5-19 warning naming the profile: {stderr}"
    );
}

#[test]
fn invalid_profile_name_aborts_with_q_5_21() {
    let dir = make_fixture();
    let out = Command::new(Q2_BIN)
        .arg("render")
        .arg(dir.path().join("index.qmd"))
        .args(["--profile", "bad/name"])
        .env_remove("QUARTO_PROFILE")
        .output()
        .expect("q2 runs");
    assert!(!out.status.success(), "invalid profile names abort");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("Q-5-21") && stderr.contains("bad/name"),
        "stderr must carry Q-5-21 naming the offender: {stderr}"
    );
}

#[test]
fn single_file_render_accepts_profile_without_warning() {
    // No project ⇒ no overlays to match, so no Q-5-19 spam; the
    // selection still resolves (conditional content consumes it in
    // Phase 4).
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join("solo.qmd"), "# Solo\n").unwrap();
    let out = Command::new(Q2_BIN)
        .arg("render")
        .arg(dir.path().join("solo.qmd"))
        .args(["--profile", "prod", "--quiet"])
        .env_remove("QUARTO_PROFILE")
        .output()
        .expect("q2 runs");
    assert!(
        out.status.success(),
        "single-file render with --profile must work: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !stderr.contains("Q-5-19"),
        "no unknown-profile warning: {stderr}"
    );
}

#[test]
fn single_file_render_still_validates_profile_names() {
    // Strictness is not project-only: a bad name aborts even with no
    // `_quarto.yml` anywhere.
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join("solo.qmd"), "# Solo\n").unwrap();
    let out = Command::new(Q2_BIN)
        .arg("render")
        .arg(dir.path().join("solo.qmd"))
        .args(["--profile", "bad/name"])
        .env_remove("QUARTO_PROFILE")
        .output()
        .expect("q2 runs");
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("Q-5-21"), "got: {stderr}");
}

// ── environment-file integration (Phase 3, needs PR #486) ──────────

/// Render `doc.qmd` in `root` with args/env; return the produced HTML.
fn render_doc(root: &Path, extra: &[&str], env: &[(&str, &str)]) -> (bool, String, String) {
    let mut cmd = Command::new(Q2_BIN);
    cmd.arg("render")
        .arg(root.join("doc.qmd"))
        .args(extra)
        .env_remove("QUARTO_PROFILE");
    for (k, v) in env {
        cmd.env(k, v);
    }
    let out = cmd.output().expect("q2 runs");
    let html = std::fs::read_to_string(root.join("doc.html")).unwrap_or_default();
    (
        out.status.success(),
        html,
        String::from_utf8_lossy(&out.stderr).to_string(),
    )
}

#[test]
fn quarto_profile_in_environment_file_activates() {
    // Q1's dotenv bootstrap: QUARTO_PROFILE defined in _environment
    // selects profiles when neither --profile nor the env var do.
    let dir = make_fixture();
    std::fs::write(dir.path().join("_environment"), "QUARTO_PROFILE=prod\n").unwrap();
    let (ok, stdout, _) = get_winner(dir.path(), &[], &[]);
    assert!(ok);
    assert_eq!(stdout, "\"prod\"");
}

#[test]
fn environment_local_bootstrap_beats_base() {
    let dir = make_fixture();
    std::fs::write(dir.path().join("_environment"), "QUARTO_PROFILE=a\n").unwrap();
    std::fs::write(dir.path().join("_environment.local"), "QUARTO_PROFILE=b\n").unwrap();
    let (ok, stdout, _) = get_winner(dir.path(), &[], &[]);
    assert!(ok);
    assert_eq!(stdout, "\"from-b\"");
}

#[test]
fn real_env_and_cli_beat_environment_file_bootstrap() {
    let dir = make_fixture();
    std::fs::write(dir.path().join("_environment"), "QUARTO_PROFILE=prod\n").unwrap();
    // Real env var wins over the file…
    let (ok, stdout, _) = get_winner(dir.path(), &[], &[("QUARTO_PROFILE", "a")]);
    assert!(ok);
    assert_eq!(stdout, "\"from-a\"");
    // …and --profile wins over both.
    let (ok, stdout, _) = get_winner(dir.path(), &["--profile", "b"], &[("QUARTO_PROFILE", "a")]);
    assert!(ok);
    assert_eq!(stdout, "\"from-b\"");
}

#[test]
fn profile_environment_files_layer_first_listed_wins() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    std::fs::write(root.join("_quarto.yml"), "project:\n  type: default\n").unwrap();
    std::fs::write(
        root.join("_environment"),
        "GREETING=from-base\nBASE_ONLY=base\n",
    )
    .unwrap();
    std::fs::write(root.join("_environment-a"), "GREETING=from-a\n").unwrap();
    std::fs::write(root.join("_environment-b"), "GREETING=from-b\nB_ONLY=b\n").unwrap();
    std::fs::write(
        root.join("doc.qmd"),
        "G={{< env GREETING none >}} BASE={{< env BASE_ONLY none >}} B={{< env B_ONLY none >}}\n",
    )
    .unwrap();
    let (ok, html, stderr) = render_doc(root, &["--profile", "a,b", "--quiet"], &[]);
    assert!(ok, "render failed: {stderr}");
    assert!(
        html.contains("G=from-a"),
        "first-listed profile's env file must win: {html}"
    );
    assert!(
        html.contains("BASE=base"),
        "base _environment still applies: {html}"
    );
    assert!(
        html.contains("B=b"),
        "later profiles still contribute new keys: {html}"
    );
}

#[test]
fn environment_local_beats_profile_env_files() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    std::fs::write(root.join("_quarto.yml"), "project:\n  type: default\n").unwrap();
    std::fs::write(root.join("_environment-prod"), "GREETING=from-prod\n").unwrap();
    std::fs::write(root.join("_environment.local"), "GREETING=from-local\n").unwrap();
    std::fs::write(root.join("doc.qmd"), "G={{< env GREETING none >}}\n").unwrap();
    let (ok, html, stderr) = render_doc(root, &["--profile", "prod", "--quiet"], &[]);
    assert!(ok, "render failed: {stderr}");
    assert!(
        html.contains("G=from-local"),
        "_environment.local wins: {html}"
    );
}

#[test]
fn quarto_profile_in_profile_env_file_does_not_recurse() {
    // Q1 parity: the bootstrap reads _environment{,.local} only. A
    // QUARTO_PROFILE inside _environment-<name> must not activate
    // more profiles.
    let dir = make_fixture();
    std::fs::write(dir.path().join("_environment-a"), "QUARTO_PROFILE=b\n").unwrap();
    let (ok, stdout, _) = get_winner(dir.path(), &["--profile", "a"], &[]);
    assert!(ok);
    assert_eq!(
        stdout, "\"from-a\"",
        "profile b must NOT have been activated by _environment-a"
    );
}

// ── flag presence on sibling commands ───────────────────────────────

#[test]
fn commands_advertise_profile_flag() {
    // `preview` is deliberately absent: its flag form needs the
    // selection threaded through HubContext (bd-pfgc273f);
    // `QUARTO_PROFILE=x q2 preview` works today.
    for subcommand in ["publish", "render", "get-config"] {
        let out = Command::new(Q2_BIN)
            .args([subcommand, "--help"])
            .output()
            .expect("q2 runs");
        let help = String::from_utf8_lossy(&out.stdout);
        assert!(
            help.contains("--profile"),
            "`q2 {subcommand} --help` must document --profile; got:\n{help}"
        );
    }
}
