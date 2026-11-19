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

/// Rewrites a Windows verbatim path to its plain spelling: `\\?\C:\x` to `C:\x`, `\\?\UNC\h\s` to
/// `\\h\s`. Any other path is returned unchanged, on every OS. Git for Windows rejects a verbatim path as a
/// worktree or clone destination ("could not create leading directories ... Invalid argument"), and
/// `std::fs::canonicalize` returns one.
pub fn strip_verbatim(path: &Path) -> PathBuf {
    let text = path.to_string_lossy();
    if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        return PathBuf::from(format!(r"\\{rest}"));
    }
    if let Some(rest) = text.strip_prefix(r"\\?\") {
        let b = rest.as_bytes();
        if b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':' {
            return PathBuf::from(rest);
        }
    }
    path.to_path_buf()
}

/// A canonicalized temp directory (macOS `/var` is a symlink to `/private/var`), in its plain spelling.
pub fn temp_dir() -> (tempfile::TempDir, PathBuf) {
    let t = tempfile::tempdir().unwrap();
    let p = strip_verbatim(&t.path().canonicalize().unwrap());
    (t, p)
}

#[cfg(test)]
mod tests {
    use super::strip_verbatim;
    use std::path::Path;

    #[test]
    fn a_verbatim_drive_path_becomes_plain() {
        assert_eq!(
            strip_verbatim(Path::new(r"\\?\C:\Users\x\repo")),
            Path::new(r"C:\Users\x\repo")
        );
    }

    #[test]
    fn a_verbatim_unc_path_becomes_a_plain_share_path() {
        assert_eq!(
            strip_verbatim(Path::new(r"\\?\UNC\host\share\repo")),
            Path::new(r"\\host\share\repo")
        );
    }

    #[test]
    fn other_paths_are_unchanged() {
        for p in [r"C:\plain", "/var/x", r"\\?\Volume{1234}\x", "rel/p"] {
            assert_eq!(strip_verbatim(Path::new(p)), Path::new(p), "{p}");
        }
    }
}

/// Commits `rel` with LF content, then re-checks it out under `core.autocrlf=true`, so the working copy holds
/// CRLF the way a Windows checkout does. Returns the disk bytes (asserted to contain CRLF for text).
pub fn crlf_checkout(dir: &Path, rel: &str, blob: &[u8]) -> Vec<u8> {
    init_repo(dir);
    put(dir, rel, blob);
    commit_all(dir, "base");
    git(dir, &["config", "core.autocrlf", "true"]);
    std::fs::remove_file(dir.join(rel)).unwrap();
    git(dir, &["checkout", "--", rel]);
    std::fs::read(dir.join(rel)).unwrap()
}
