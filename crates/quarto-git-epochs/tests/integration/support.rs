//! Shared helpers: fixture paths and temporary repositories.

use std::path::{Path, PathBuf};
use std::process::Command;

/// `ts-packages/quarto-git-epochs/fixtures`, shared with the TypeScript package.
pub fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ts-packages/quarto-git-epochs/fixtures")
}

pub fn read_fixture_json(rel: &str) -> serde_json::Value {
    let text = std::fs::read_to_string(fixtures_dir().join(rel)).unwrap();
    serde_json::from_str(&text).unwrap()
}

/// Runs git in `dir`; panics with stderr on failure. Returns trimmed stdout.
pub fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

pub fn init_repo(dir: &Path) {
    git(dir, &["init", "-q", "-b", "main"]);
    git(dir, &["config", "user.email", "t@example.com"]);
    git(dir, &["config", "user.name", "T"]);
    git(dir, &["config", "core.autocrlf", "false"]);
}

pub fn put(dir: &Path, rel: &str, content: &[u8]) {
    let p = dir.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, content).unwrap();
}

pub fn commit_all(dir: &Path, msg: &str) -> String {
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-q", "-m", msg]);
    git(dir, &["rev-parse", "HEAD"])
}

/// A canonicalized temp directory (macOS `/var` is a symlink to `/private/var`).
pub fn temp_dir() -> (tempfile::TempDir, PathBuf) {
    let t = tempfile::tempdir().unwrap();
    let p = t.path().canonicalize().unwrap();
    (t, p)
}
