//! Tests for `scripts/nightly-gate.sh`, the "does `main` have unreleased
//! changes?" decision behind the Nightly workflow (bd-p4ljdp2e; plan
//! claude-notes/plans/2026-09-19-nightly-release-workflow.md, Decision 4).
//!
//! The script runs inside a checkout and emits GitHub-Actions style
//! `key=value` outputs (to `$GITHUB_OUTPUT`, or stdout when unset):
//!
//!   build            true|false
//!   reason           released | nightly-current | unreleased | forced | no-tags
//!   version          <next-minor>-nightly.<YYYYMMDD>   (only when build=true)
//!   sha              full HEAD sha
//!   prev_release_tag nearest `v*` tag reachable from HEAD ("" if none)
//!
//! Skip iff HEAD is exactly the newest release tag (main is fully
//! released) or exactly the current `nightly` tag (last night already
//! built this tree). `NIGHTLY_FORCE=1` builds regardless; `NIGHTLY_DATE`
//! pins the date so the version is deterministic here.
//!
//! Unix-only like bootstrap_sh.rs: the script is bash.

#![cfg(unix)]

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

fn gate_script() -> PathBuf {
    repo_root().join("scripts").join("nightly-gate.sh")
}

/// A throwaway git repository with a workspace-shaped Cargo.toml.
struct TempRepo {
    tmp: tempfile::TempDir,
}

impl TempRepo {
    fn new(cargo_version: &str) -> TempRepo {
        let tmp = tempfile::tempdir().unwrap();
        let repo = TempRepo { tmp };
        repo.git(&["init", "-q", "-b", "main"]);
        repo.git(&["config", "user.email", "test@example.com"]);
        repo.git(&["config", "user.name", "Nightly Gate Test"]);
        repo.git(&["config", "commit.gpgsign", "false"]);
        repo.git(&["config", "tag.gpgsign", "false"]);
        // Same shape release.yml's preflight greps (`grep -m1 '^version'`).
        fs::write(
            repo.path().join("Cargo.toml"),
            format!(
                "[workspace]\nmembers = []\n\n[workspace.package]\nversion = \"{cargo_version}\"\nedition = \"2021\"\n"
            ),
        )
        .unwrap();
        repo.commit("initial");
        repo
    }

    fn path(&self) -> &Path {
        self.tmp.path()
    }

    fn git(&self, args: &[&str]) -> String {
        let out = Command::new("git")
            .args(args)
            .current_dir(self.path())
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap().trim().to_owned()
    }

    fn commit(&self, message: &str) {
        // Touch a file so every commit has content.
        let marker = self.path().join("CHANGES");
        let mut body = fs::read_to_string(&marker).unwrap_or_default();
        body.push_str(message);
        body.push('\n');
        fs::write(&marker, body).unwrap();
        self.git(&["add", "-A"]);
        self.git(&["commit", "-q", "-m", message]);
    }

    fn tag(&self, name: &str) {
        self.git(&["tag", "-a", name, "-m", name]);
    }

    fn head(&self) -> String {
        self.git(&["rev-parse", "HEAD"])
    }

    /// Run the gate with `NIGHTLY_DATE` pinned, plus any extra env.
    fn gate_env(&self, envs: &[(&str, &str)]) -> (Output, BTreeMap<String, String>) {
        let output_file = self.path().join("gh-output.txt");
        let _ = fs::remove_file(&output_file);
        let mut cmd = Command::new("bash");
        cmd.arg(gate_script())
            .current_dir(self.path())
            .env_clear()
            .env("PATH", std::env::var("PATH").unwrap_or_default())
            .env("HOME", self.path())
            .env("NIGHTLY_DATE", "20260919")
            .env("GITHUB_OUTPUT", &output_file);
        for (k, v) in envs {
            cmd.env(k, v);
        }
        let out = cmd.output().unwrap();
        assert!(
            out.status.success(),
            "gate exited {:?}\nstdout: {}\nstderr: {}",
            out.status,
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        let outputs = fs::read_to_string(&output_file)
            .unwrap_or_default()
            .lines()
            .filter_map(|l| l.split_once('='))
            .map(|(k, v)| (k.to_owned(), v.to_owned()))
            .collect();
        (out, outputs)
    }

    fn gate(&self) -> BTreeMap<String, String> {
        self.gate_env(&[]).1
    }
}

fn get<'a>(outputs: &'a BTreeMap<String, String>, key: &str) -> &'a str {
    outputs
        .get(key)
        .unwrap_or_else(|| panic!("gate did not emit `{key}`; outputs: {outputs:?}"))
}

#[test]
fn skips_when_head_is_the_newest_release() {
    let repo = TempRepo::new("0.32.0");
    repo.tag("v0.32.0");
    let o = repo.gate();
    assert_eq!(get(&o, "build"), "false");
    assert_eq!(get(&o, "reason"), "released");
    assert_eq!(get(&o, "prev_release_tag"), "v0.32.0");
    assert_eq!(get(&o, "sha"), repo.head());
    assert!(
        !o.contains_key("version"),
        "no version when skipping: {o:?}"
    );
}

#[test]
fn skips_when_head_is_the_current_nightly() {
    let repo = TempRepo::new("0.32.0");
    repo.tag("v0.32.0");
    repo.commit("feature");
    repo.tag("nightly");
    let o = repo.gate();
    assert_eq!(get(&o, "build"), "false");
    assert_eq!(get(&o, "reason"), "nightly-current");
    assert_eq!(get(&o, "prev_release_tag"), "v0.32.0");
}

#[test]
fn builds_when_head_is_past_both_tags() {
    let repo = TempRepo::new("0.32.0");
    repo.tag("v0.32.0");
    repo.commit("feature");
    repo.tag("nightly");
    repo.commit("another feature");
    let o = repo.gate();
    assert_eq!(get(&o, "build"), "true");
    assert_eq!(get(&o, "reason"), "unreleased");
    assert_eq!(get(&o, "version"), "0.33.0-nightly.20260919");
    assert_eq!(get(&o, "sha"), repo.head());
    assert_eq!(get(&o, "prev_release_tag"), "v0.32.0");
}

#[test]
fn builds_when_there_is_a_release_but_no_nightly_yet() {
    let repo = TempRepo::new("0.32.0");
    repo.tag("v0.32.0");
    repo.commit("feature");
    let o = repo.gate();
    assert_eq!(get(&o, "build"), "true");
    assert_eq!(get(&o, "reason"), "unreleased");
    assert_eq!(get(&o, "version"), "0.33.0-nightly.20260919");
}

#[test]
fn builds_when_there_are_no_tags_at_all() {
    let repo = TempRepo::new("0.32.0");
    let o = repo.gate();
    assert_eq!(get(&o, "build"), "true");
    assert_eq!(get(&o, "reason"), "no-tags");
    assert_eq!(get(&o, "prev_release_tag"), "");
    assert_eq!(get(&o, "version"), "0.33.0-nightly.20260919");
}

#[test]
fn a_stale_nightly_tag_does_not_block_a_newer_head() {
    let repo = TempRepo::new("0.32.0");
    repo.commit("feature");
    repo.tag("nightly");
    repo.commit("later feature");
    let o = repo.gate();
    assert_eq!(get(&o, "build"), "true");
}

#[test]
fn force_builds_a_released_head() {
    let repo = TempRepo::new("0.32.0");
    repo.tag("v0.32.0");
    let (_, o) = repo.gate_env(&[("NIGHTLY_FORCE", "1")]);
    assert_eq!(get(&o, "build"), "true");
    assert_eq!(get(&o, "reason"), "forced");
    assert_eq!(get(&o, "version"), "0.33.0-nightly.20260919");
    assert_eq!(get(&o, "prev_release_tag"), "v0.32.0");
}

#[test]
fn force_is_only_honoured_when_set_to_1() {
    let repo = TempRepo::new("0.32.0");
    repo.tag("v0.32.0");
    let (_, o) = repo.gate_env(&[("NIGHTLY_FORCE", "false")]);
    assert_eq!(get(&o, "build"), "false");
}

#[test]
fn version_is_the_next_minor_of_the_manifest_version() {
    let repo = TempRepo::new("1.4.7");
    let o = repo.gate();
    assert_eq!(get(&o, "version"), "1.5.0-nightly.20260919");
}

#[test]
fn prev_release_is_the_nearest_v_tag_not_the_nightly_tag() {
    let repo = TempRepo::new("0.32.0");
    repo.tag("v0.31.0");
    repo.commit("bump");
    repo.tag("v0.32.0");
    repo.commit("feature");
    repo.tag("nightly");
    repo.commit("more");
    let o = repo.gate();
    assert_eq!(get(&o, "prev_release_tag"), "v0.32.0");
}

#[test]
fn a_non_release_tag_on_head_does_not_read_as_released() {
    // Only `v*` tags mean "released"; an unrelated tag on HEAD must not
    // suppress the build.
    let repo = TempRepo::new("0.32.0");
    repo.tag("v0.32.0");
    repo.commit("feature");
    repo.tag("some-marker");
    let o = repo.gate();
    assert_eq!(get(&o, "build"), "true");
    assert_eq!(get(&o, "prev_release_tag"), "v0.32.0");
}

#[test]
fn date_defaults_to_today_utc_when_not_pinned() {
    let repo = TempRepo::new("0.32.0");
    let output_file = repo.path().join("gh-output.txt");
    let out = Command::new("bash")
        .arg(gate_script())
        .current_dir(repo.path())
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("HOME", repo.path())
        .env("GITHUB_OUTPUT", &output_file)
        .output()
        .unwrap();
    assert!(out.status.success());
    let text = fs::read_to_string(&output_file).unwrap();
    let version = text
        .lines()
        .find_map(|l| l.strip_prefix("version="))
        .expect("version output");
    let (base, date) = version.split_once("-nightly.").unwrap();
    assert_eq!(base, "0.33.0");
    assert_eq!(date.len(), 8, "date should be YYYYMMDD: {date}");
    assert!(date.bytes().all(|b| b.is_ascii_digit()), "{date}");
}

#[test]
fn writes_outputs_to_stdout_when_github_output_is_unset() {
    let repo = TempRepo::new("0.32.0");
    let out = Command::new("bash")
        .arg(gate_script())
        .current_dir(repo.path())
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("HOME", repo.path())
        .env("NIGHTLY_DATE", "20260919")
        .output()
        .unwrap();
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("build=true\n"), "stdout: {text}");
    assert!(
        text.contains("version=0.33.0-nightly.20260919\n"),
        "stdout: {text}"
    );
}

#[test]
fn fails_loudly_outside_a_git_repository() {
    let tmp = tempfile::tempdir().unwrap();
    let out = Command::new("bash")
        .arg(gate_script())
        .current_dir(tmp.path())
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("HOME", tmp.path())
        .output()
        .unwrap();
    assert!(!out.status.success());
}

#[test]
fn gate_script_passes_shellcheck() {
    if Command::new("shellcheck")
        .arg("--version")
        .output()
        .is_err()
    {
        eprintln!("shellcheck not installed; skipping");
        return;
    }
    let out = Command::new("shellcheck")
        .arg("--severity=style")
        .arg(gate_script())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "shellcheck findings:\n{}",
        String::from_utf8_lossy(&out.stdout)
    );
}
