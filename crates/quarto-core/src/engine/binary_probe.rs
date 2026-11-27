//! Process-wide cached availability probe for engine binaries.
//!
//! A binary is available when it is found and, run with the probe arguments,
//! exits 0 within the probe deadline. Only the exit status is read; the
//! output is never inspected. A path that exists but does not run (a stale
//! shim, a broken install) is therefore reported unavailable.
//!
//! Engines are rebuilt per render, so results are cached per
//! `(path, arguments)` for the life of the process. The first stored result
//! for a key wins: concurrent callers may each run the probe, but all of them
//! return the value that was stored first.
//!
//! See `claude-notes/plans/2026-11-27-engine-binary-probe.md`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{LazyLock, Mutex};

use crate::child_runner::run_status_probe;

static PROBE_CACHE: LazyLock<Mutex<HashMap<(PathBuf, Vec<String>), bool>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// True iff `path`, run with `args`, exits 0 within the probe deadline.
pub(crate) fn binary_runs(path: &Path, args: &[&str]) -> bool {
    let key = (
        path.to_path_buf(),
        args.iter().map(|a| a.to_string()).collect::<Vec<_>>(),
    );
    if let Some(&cached) = PROBE_CACHE.lock().unwrap().get(&key) {
        return cached;
    }
    // The probe runs without the lock held so a slow binary does not block
    // probes of other paths.
    let mut cmd = Command::new(path);
    cmd.args(args);
    let result = run_status_probe(cmd).is_some_and(|status| status.success());
    *PROBE_CACHE.lock().unwrap().entry(key).or_insert(result)
}

/// Write an executable stub named `name` into `dir` that runs `body`.
#[cfg(test)]
pub(crate) fn write_stub(dir: &Path, name: &str, body: &str) -> PathBuf {
    #[cfg(windows)]
    {
        let path = dir.join(format!("{name}.bat"));
        std::fs::write(&path, format!("@echo off\r\n{body}\r\n")).unwrap();
        path
    }
    #[cfg(not(windows))]
    {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A stub that appends one line to a counter file per invocation.
    fn counting_stub(dir: &Path) -> (PathBuf, PathBuf) {
        let counter = dir.join("counter.txt");
        #[cfg(windows)]
        let stub = write_stub(
            dir,
            "probe",
            &format!("echo x>> \"{}\"\r\nexit /b 0", counter.display()),
        );
        #[cfg(not(windows))]
        let stub = write_stub(
            dir,
            "probe",
            &format!("echo x >> '{}'\nexit 0", counter.display()),
        );
        (stub, counter)
    }

    fn invocations(counter: &Path) -> usize {
        std::fs::read_to_string(counter)
            .unwrap_or_default()
            .lines()
            .count()
    }

    #[test]
    fn binary_runs_false_for_failing_binary() {
        let tmp = tempfile::tempdir().unwrap();
        #[cfg(windows)]
        let stub = write_stub(tmp.path(), "probe", "exit /b 1");
        #[cfg(not(windows))]
        let stub = write_stub(tmp.path(), "probe", "exit 1");
        assert!(!binary_runs(&stub, &["--version"]));
    }

    #[test]
    fn binary_runs_true_for_working_binary() {
        let tmp = tempfile::tempdir().unwrap();
        #[cfg(windows)]
        let stub = write_stub(tmp.path(), "probe", "echo 1.0\r\nexit /b 0");
        #[cfg(not(windows))]
        let stub = write_stub(tmp.path(), "probe", "echo 1.0\nexit 0");
        assert!(binary_runs(&stub, &["--version"]));
    }

    #[test]
    fn binary_runs_false_for_missing_path() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(!binary_runs(
            &tmp.path().join("no-such-binary"),
            &["--version"]
        ));
    }

    #[test]
    fn binary_runs_reuses_cached_result() {
        let tmp = tempfile::tempdir().unwrap();
        let (stub, counter) = counting_stub(tmp.path());
        assert!(binary_runs(&stub, &["--version"]));
        assert!(binary_runs(&stub, &["--version"]));
        assert!(binary_runs(&stub, &["--version"]));
        assert_eq!(invocations(&counter), 1);
    }

    #[test]
    fn binary_runs_caches_per_argument_list() {
        let tmp = tempfile::tempdir().unwrap();
        let (stub, counter) = counting_stub(tmp.path());
        assert!(binary_runs(&stub, &["--version"]));
        assert!(binary_runs(&stub, &["--version"]));
        assert!(binary_runs(&stub, &["-V"]));
        assert_eq!(invocations(&counter), 2);
    }
}
