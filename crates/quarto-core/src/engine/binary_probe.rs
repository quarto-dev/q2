//! Process-wide cached availability probe for engine binaries.
//!
//! A binary is available when it is found and, run with the probe arguments,
//! exits 0 within the probe deadline. Only the exit status is read; the
//! output is never inspected. A path that exists but does not run (a stale
//! shim, a broken install) is therefore reported unavailable.
//!
//! The probe runs in `cwd` when one is given and with the extra `env` pairs,
//! so a version-manager shim that picks its runtime from the working
//! directory or from a project environment variable is probed the way the
//! engine will run it.
//!
//! Engines are rebuilt per render, so a success or a timeout is cached per
//! `(path, arguments, cwd, env)` for the life of the process. A fast failure
//! (non-zero exit, spawn error) is not: the next check probes again, so a
//! runtime repaired during a long-lived process (`q2 preview`) is picked up
//! on the next render. A timeout stays cached so a hung shim is not run on
//! every check; on Windows the kill stops only the shim, not what it started.
//! A success replaces a cached timeout from an older probe of the same key;
//! a timeout never replaces a success.
//!
//! See `claude-notes/plans/2026-11-27-engine-binary-probe.md`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{LazyLock, Mutex};
use std::time::Duration;

use crate::child_runner::{ChildError, ChildLimits, PROBE_DEADLINE, run_status};

type ProbeKey = (PathBuf, Vec<String>, Option<PathBuf>, Vec<(String, String)>);

static PROBE_CACHE: LazyLock<Mutex<HashMap<ProbeKey, bool>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// True iff `path`, run with `args` (in `cwd` when given, else in this
/// process's working directory, and with `env` added to the inherited
/// environment), exits 0 within the probe deadline.
pub(crate) fn binary_runs(
    path: &Path,
    args: &[&str],
    cwd: Option<&Path>,
    env: &[(String, String)],
) -> bool {
    binary_runs_within(path, args, cwd, env, PROBE_DEADLINE)
}

/// [`binary_runs`] with an explicit deadline.
fn binary_runs_within(
    path: &Path,
    args: &[&str],
    cwd: Option<&Path>,
    env: &[(String, String)],
    deadline: Duration,
) -> bool {
    let key = (
        path.to_path_buf(),
        args.iter().map(|a| a.to_string()).collect::<Vec<_>>(),
        cwd.map(Path::to_path_buf),
        env.to_vec(),
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
    cmd.envs(env.iter().map(|(k, v)| (k, v)));
    let limits = ChildLimits {
        deadline: Some(deadline),
        cancellation: None,
    };
    let verdict = match run_status(cmd, limits) {
        Ok(status) if status.success() => Some(true),
        Err(ChildError::TimedOut) => Some(false),
        _ => None,
    };
    match verdict {
        Some(runs) => store(&mut PROBE_CACHE.lock().unwrap(), key, runs),
        None => false,
    }
}

/// Cache a finished probe's verdict and return the verdict for the caller.
/// A success always wins: it replaces a timeout an older probe stored, and a
/// timeout is stored only when the key has no verdict yet.
fn store(cache: &mut HashMap<ProbeKey, bool>, key: ProbeKey, runs: bool) -> bool {
    if runs {
        cache.insert(key, true);
        true
    } else {
        *cache.entry(key).or_insert(false)
    }
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

/// Write a stub that exits 0 only when the environment variable `var` is
/// set, like a shim pinned through a project `_environment` file.
#[cfg(test)]
pub(crate) fn write_env_sensitive_stub(dir: &Path, name: &str, var: &str) -> PathBuf {
    #[cfg(windows)]
    let body = format!("if \"%{var}%\"==\"\" exit /b 1\r\nexit /b 0");
    #[cfg(not(windows))]
    let body = format!("[ -n \"${var}\" ] || exit 1\nexit 0");
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
        assert!(!binary_runs(&stub, &["--version"], None, &[]));
    }

    #[test]
    fn binary_runs_true_for_working_binary() {
        let tmp = tempfile::tempdir().unwrap();
        #[cfg(windows)]
        let stub = write_stub(tmp.path(), "probe", "echo 1.0\r\nexit /b 0");
        #[cfg(not(windows))]
        let stub = write_stub(tmp.path(), "probe", "echo 1.0\nexit 0");
        assert!(binary_runs(&stub, &["--version"], None, &[]));
    }

    #[test]
    fn binary_runs_false_for_missing_path() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(!binary_runs(
            &tmp.path().join("no-such-binary"),
            &["--version"],
            None,
            &[]
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

        assert!(!binary_runs(&stub, &["--version"], Some(&elsewhere), &[]));
        assert!(binary_runs(&stub, &["--version"], Some(&project), &[]));
        assert!(!binary_runs(&stub, &["--version"], Some(&elsewhere), &[]));
    }

    /// The extra `env` pairs reach the probe, and each set keeps its own
    /// cached result.
    #[test]
    fn binary_runs_probes_and_caches_per_env() {
        let tmp = tempfile::tempdir().unwrap();
        let var = "Q2_BINARY_PROBE_TEST_PIN";
        let stub = write_env_sensitive_stub(tmp.path(), "probe", var);
        let pinned = [(var.to_string(), "1".to_string())];

        assert!(!binary_runs(&stub, &["--version"], None, &[]));
        assert!(binary_runs(&stub, &["--version"], None, &pinned));
        assert!(!binary_runs(&stub, &["--version"], None, &[]));
    }

    #[test]
    fn binary_runs_reuses_cached_result() {
        let tmp = tempfile::tempdir().unwrap();
        let (stub, counter) = counting_stub(tmp.path());
        assert!(binary_runs(&stub, &["--version"], None, &[]));
        assert!(binary_runs(&stub, &["--version"], None, &[]));
        assert!(binary_runs(&stub, &["--version"], None, &[]));
        assert_eq!(invocations(&counter), 1);
    }

    /// A failure is not cached: a runtime repaired at the same path (a
    /// fixed shim during `q2 preview`) is seen on the next probe.
    #[test]
    fn binary_runs_reprobes_after_a_failure() {
        let tmp = tempfile::tempdir().unwrap();
        #[cfg(windows)]
        let stub = write_stub(tmp.path(), "probe", "exit /b 1");
        #[cfg(not(windows))]
        let stub = write_stub(tmp.path(), "probe", "exit 1");
        assert!(!binary_runs(&stub, &["--version"], None, &[]));

        #[cfg(windows)]
        write_stub(tmp.path(), "probe", "exit /b 0");
        #[cfg(not(windows))]
        write_stub(tmp.path(), "probe", "exit 0");
        assert!(binary_runs(&stub, &["--version"], None, &[]));
    }

    /// A probe that times out is cached: a hung shim is not run again on
    /// every check (on Windows a timeout kills only the shim, not what it
    /// started). After the timeout the hung stub is replaced by a working
    /// one; the second check, with a deadline long enough for that stub to
    /// succeed, still returns the cached timeout, so it did not probe again.
    #[test]
    fn binary_runs_caches_a_timeout() {
        let tmp = tempfile::tempdir().unwrap();
        #[cfg(windows)]
        let stub = write_stub(tmp.path(), "probe", ":spin\r\ngoto spin");
        #[cfg(not(windows))]
        let stub = write_stub(tmp.path(), "probe", "sleep 5");
        assert!(!binary_runs_within(
            &stub,
            &["--version"],
            None,
            &[],
            std::time::Duration::from_millis(300)
        ));

        #[cfg(windows)]
        write_stub(tmp.path(), "probe", "exit /b 0");
        #[cfg(not(windows))]
        write_stub(tmp.path(), "probe", "exit 0");
        assert!(!binary_runs_within(
            &stub,
            &["--version"],
            None,
            &[],
            std::time::Duration::from_secs(10)
        ));
    }

    /// Two probes of one key finish out of order: a success replaces a
    /// timeout stored by an older probe, and a timeout never replaces a
    /// success.
    #[test]
    fn store_lets_a_success_replace_a_timeout() {
        let key: ProbeKey = (PathBuf::from("probe"), vec![], None, vec![]);

        let mut cache = HashMap::new();
        assert!(!store(&mut cache, key.clone(), false));
        assert!(store(&mut cache, key.clone(), true));
        assert_eq!(cache.get(&key), Some(&true));

        let mut cache = HashMap::new();
        assert!(store(&mut cache, key.clone(), true));
        assert!(store(&mut cache, key.clone(), false));
        assert_eq!(cache.get(&key), Some(&true));
    }

    #[test]
    fn binary_runs_caches_per_argument_list() {
        let tmp = tempfile::tempdir().unwrap();
        let (stub, counter) = counting_stub(tmp.path());
        assert!(binary_runs(&stub, &["--version"], None, &[]));
        assert!(binary_runs(&stub, &["--version"], None, &[]));
        assert!(binary_runs(&stub, &["-V"], None, &[]));
        assert_eq!(invocations(&counter), 2);
    }
}
