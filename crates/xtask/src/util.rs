//! Small shared utilities for xtask subcommands.

use std::path::{Path, PathBuf};

/// Return a copy of `path` with forward slashes replaced by the platform's
/// main separator. No-op on POSIX where `MAIN_SEPARATOR == '/'`.
///
/// External tools on Windows (git, gh) commonly emit paths with `/`. Once
/// those paths feed into [`PathBuf::join`], which uses `MAIN_SEPARATOR`,
/// the result mixes separators (`C:/Users/.../q2\.worktrees\foo`) — still
/// valid, but ugly when displayed to a user. Normalize once on entry so
/// every downstream `join` and `display` produces a consistent path.
pub fn with_native_separators(path: &Path) -> PathBuf {
    if std::path::MAIN_SEPARATOR == '/' {
        return path.to_path_buf();
    }
    let s = path.to_string_lossy();
    PathBuf::from(s.replace('/', std::path::MAIN_SEPARATOR_STR))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn with_native_separators_is_noop_on_posix_like_paths() {
        // The function must always return a path the OS can resolve; we
        // don't try to test the Windows-only branch from here (it's a pure
        // string replace, and main.rs wires it correctly).
        let input = Path::new("/tmp/foo/bar");
        let out = with_native_separators(input);
        #[cfg(not(windows))]
        assert_eq!(out, PathBuf::from("/tmp/foo/bar"));
        #[cfg(windows)]
        assert_eq!(out, PathBuf::from(r"\tmp\foo\bar"));
    }

    #[test]
    fn with_native_separators_preserves_already_native_paths() {
        // PathBuf with platform separators round-trips unchanged.
        let mut p = PathBuf::new();
        p.push("a");
        p.push("b");
        p.push("c");
        assert_eq!(with_native_separators(&p), p);
    }
}
