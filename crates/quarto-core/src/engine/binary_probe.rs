//! Process-wide cached availability probe for engine binaries.
//!
//! A binary is available when it is found and, run with the probe arguments,
//! exits 0 within the probe deadline. Only the exit status is read; the
//! output is never inspected. A path that exists but does not run (a stale
//! shim, a broken install) is therefore reported unavailable.
//!
//! The probe runs in `cwd` when one is given, so a version-manager shim that
//! picks its runtime from the working directory is probed where the engine
//! will run it.
//!
//! Engines are rebuilt per render, so results are cached per
//! `(path, arguments, cwd)` for the life of the process. The first stored
//! result for a key wins: concurrent callers may each run the probe, but all
//! of them return the value that was stored first.
//!
//! See `claude-notes/plans/2026-11-27-engine-binary-probe.md`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{LazyLock, Mutex};

use crate::child_runner::run_status_probe;

type ProbeKey = (PathBuf, Vec<String>, Option<PathBuf>);

static PROBE_CACHE: LazyLock<Mutex<HashMap<ProbeKey, bool>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// True iff `path`, run with `args` (in `cwd` when given, else in this
/// process's working directory), exits 0 within the probe deadline.
pub(crate) fn binary_runs(path: &Path, args: &[&str], cwd: Option<&Path>) -> bool {
    let key = (
        path.to_path_buf(),
        args.iter().map(|a| a.to_string()).collect::<Vec<_>>(),
        cwd.map(Path::to_path_buf),
    );
    if let Some(&cached) = PROBE_CACHE.lock().unwrap().get(&key) {
        return cached;
    }
    // The probe runs without the lock held so a slow binary does not block
    // probes of other paths.
    let mut cmd = Command::new(path);
    cmd.args(args);
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
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

/// Write a stub that exits 0 only when its working directory contains
/// `marker`, like a version-manager shim that reads a per-project pin.
#[cfg(test)]
pub(crate) fn write_cwd_sensitive_stub(dir: &Path, name: &str, marker: &str) -> PathBuf {
    #[cfg(windows)]
    let body = format!("if not exist \"{marker}\" exit /b 1\r\nexit /b 0");
    #[cfg(not(windows))]
    let body = format!("[ -f '{marker}' ] || exit 1\nexit 0");
    write_stub(dir, name, &body)
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
        assert!(!binary_runs(&stub, &["--version"], None));
    }

    #[test]
    fn binary_runs_true_for_working_binary() {
        let tmp = tempfile::tempdir().unwrap();
        #[cfg(windows)]
        let stub = write_stub(tmp.path(), "probe", "echo 1.0\r\nexit /b 0");
        #[cfg(not(windows))]
        let stub = write_stub(tmp.path(), "probe", "echo 1.0\nexit 0");
        assert!(binary_runs(&stub, &["--version"], None));
    }

    #[test]
    fn binary_runs_false_for_missing_path() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(!binary_runs(
            &tmp.path().join("no-such-binary"),
            &["--version"],
            None
        ));
    }

    /// A shim that only works in a directory holding `marker` runs in the
    /// given `cwd`, and each `cwd` keeps its own cached result.
    #[test]
    fn binary_runs_probes_and_caches_per_cwd() {
        let tmp = tempfile::tempdir().unwrap();
        let stub = write_cwd_sensitive_stub(tmp.path(), "probe", "marker.txt");
        let project = tmp.path().join("project");
        let elsewhere = tmp.path().join("elsewhere");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::create_dir_all(&elsewhere).unwrap();
        std::fs::write(project.join("marker.txt"), "").unwrap();

        assert!(!binary_runs(&stub, &["--version"], Some(&elsewhere)));
        assert!(binary_runs(&stub, &["--version"], Some(&project)));
        assert!(!binary_runs(&stub, &["--version"], Some(&elsewhere)));
    }

    #[test]
    fn binary_runs_reuses_cached_result() {
        let tmp = tempfile::tempdir().unwrap();
        let (stub, counter) = counting_stub(tmp.path());
        assert!(binary_runs(&stub, &["--version"], None));
        assert!(binary_runs(&stub, &["--version"], None));
        assert!(binary_runs(&stub, &["--version"], None));
        assert_eq!(invocations(&counter), 1);
    }

    #[test]
    fn binary_runs_caches_per_argument_list() {
        let tmp = tempfile::tempdir().unwrap();
        let (stub, counter) = counting_stub(tmp.path());
        assert!(binary_runs(&stub, &["--version"], None));
        assert!(binary_runs(&stub, &["--version"], None));
        assert!(binary_runs(&stub, &["-V"], None));
        assert_eq!(invocations(&counter), 2);
    }
}
