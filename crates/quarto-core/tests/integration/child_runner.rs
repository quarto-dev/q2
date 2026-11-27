//! `quarto_core::child_runner::run_child` and `run_status`: deadline,
//! cancellation, output capture and exit status, driven through fake child
//! scripts.
//!
//! The scripts are built by `#[cfg(unix)]` / `#[cfg(not(unix))]` helper
//! pairs (see `.claude/rules/cross-platform.md`); only the `run_child`
//! grandchild test, which depends on process groups, is unix-only.

#![cfg(not(target_arch = "wasm32"))]

use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

use quarto_core::child_runner::{ChildError, ChildLimits, run_child, run_status};
use quarto_core::stage::Cancellation;

#[cfg(unix)]
fn script(body: &str) -> Command {
    let mut cmd = Command::new("sh");
    cmd.args(["-c", body]);
    cmd
}

#[cfg(not(unix))]
fn script(body: &str) -> Command {
    let mut cmd = Command::new("powershell");
    cmd.args(["-NoProfile", "-Command", body]);
    cmd
}

/// Prints `out` to stdout and `err` to stderr, then exits with `code`.
#[cfg(unix)]
fn emit_cmd(out: &str, err: &str, code: i32) -> Command {
    script(&format!("echo {out}; echo {err} >&2; exit {code}"))
}

#[cfg(not(unix))]
fn emit_cmd(out: &str, err: &str, code: i32) -> Command {
    script(&format!(
        "Write-Output {out}; [Console]::Error.WriteLine('{err}'); exit {code}"
    ))
}

/// Runs for `secs` seconds without output.
#[cfg(unix)]
fn sleep_cmd(secs: u64) -> Command {
    script(&format!("sleep {secs}"))
}

#[cfg(not(unix))]
fn sleep_cmd(secs: u64) -> Command {
    script(&format!("Start-Sleep -Seconds {secs}"))
}

/// Writes `n` bytes to stdout and `n` to stderr (well past a pipe buffer).
#[cfg(unix)]
fn flood_cmd(n: usize) -> Command {
    script(&format!("head -c {n} /dev/zero; head -c {n} /dev/zero >&2"))
}

#[cfg(not(unix))]
fn flood_cmd(n: usize) -> Command {
    script(&format!(
        "[Console]::Out.Write('x' * {n}); [Console]::Error.Write('x' * {n})"
    ))
}

#[test]
fn completes_and_captures_output() {
    let out = run_child(emit_cmd("hello", "oops", 0), ChildLimits::default()).unwrap();
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "hello");
    assert_eq!(String::from_utf8_lossy(&out.stderr).trim(), "oops");
}

#[test]
fn nonzero_exit_is_not_an_error() {
    let limits = ChildLimits {
        deadline: Some(Duration::from_secs(30)),
        cancellation: Some(Cancellation::new()),
    };
    let out = run_child(emit_cmd("x", "y", 3), limits).unwrap();
    assert_eq!(out.status.code(), Some(3));
}

#[test]
fn spawn_failure_is_io_error() {
    let err = run_child(
        Command::new("q2-no-such-binary-for-child-runner-test"),
        ChildLimits::default(),
    )
    .unwrap_err();
    assert!(matches!(err, ChildError::Io(_)), "got {err:?}");
}

#[test]
fn times_out_and_kills() {
    let limits = ChildLimits {
        deadline: Some(Duration::from_millis(200)),
        cancellation: None,
    };
    let start = Instant::now();
    let err = run_child(sleep_cmd(30), limits).unwrap_err();
    assert!(matches!(err, ChildError::TimedOut), "got {err:?}");
    assert!(start.elapsed() < Duration::from_secs(10));
}

#[test]
fn cancel_kills() {
    let token = Cancellation::new();
    let limits = ChildLimits {
        deadline: None,
        cancellation: Some(token.clone()),
    };
    let canceller = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(200));
        token.cancel();
    });
    let start = Instant::now();
    let err = run_child(sleep_cmd(30), limits).unwrap_err();
    canceller.join().unwrap();
    assert!(matches!(err, ChildError::Cancelled), "got {err:?}");
    assert!(start.elapsed() < Duration::from_secs(10));
}

#[test]
fn already_cancelled_token_never_lets_the_child_finish() {
    let token = Cancellation::new();
    token.cancel();
    let limits = ChildLimits {
        deadline: None,
        cancellation: Some(token),
    };
    let err = run_child(sleep_cmd(30), limits).unwrap_err();
    assert!(matches!(err, ChildError::Cancelled), "got {err:?}");
}

#[test]
fn output_larger_than_a_pipe_buffer_does_not_deadlock() {
    let n = 1_000_000;
    let out = run_child(flood_cmd(n), ChildLimits::default()).unwrap();
    assert!(out.status.success());
    assert!(out.stdout.len() >= n, "stdout {}", out.stdout.len());
    assert!(out.stderr.len() >= n, "stderr {}", out.stderr.len());
}

/// A grandchild that inherits the pipes must not outlive a timeout: the
/// whole process group is killed.
#[cfg(unix)]
#[test]
fn timeout_kills_a_grandchild_holding_the_pipes() {
    let limits = ChildLimits {
        deadline: Some(Duration::from_millis(200)),
        cancellation: None,
    };
    let start = Instant::now();
    // The shell waits on a backgrounded sleep that inherits stdout/stderr.
    let err = run_child(script("sleep 30 & wait"), limits).unwrap_err();
    assert!(matches!(err, ChildError::TimedOut), "got {err:?}");
    assert!(
        start.elapsed() < Duration::from_secs(10),
        "returned after {:?}",
        start.elapsed()
    );
}

/// A child that exits while a grandchild still holds the pipes stays bound
/// by the deadline instead of hanging in the drain.
#[cfg(unix)]
#[test]
fn deadline_applies_while_a_grandchild_holds_the_pipes_after_exit() {
    let limits = ChildLimits {
        deadline: Some(Duration::from_millis(300)),
        cancellation: None,
    };
    let start = Instant::now();
    let err = run_child(script("sleep 30 &"), limits).unwrap_err();
    assert!(matches!(err, ChildError::TimedOut), "got {err:?}");
    assert!(start.elapsed() < Duration::from_secs(10));
}

/// Starts a background child that records its own PID in `marker` and sleeps
/// 30 s, waits until the marker exists, then exits 0.
#[cfg(unix)]
fn background_child_then_exit0_cmd(marker: &Path) -> Command {
    let m = marker.display();
    script(&format!(
        "sh -c 'echo $$ > \"{m}\"; exec sleep 30' & while [ ! -s \"{m}\" ]; do sleep 0.05; done; exit 0"
    ))
}

#[cfg(not(unix))]
fn background_child_then_exit0_cmd(marker: &Path) -> Command {
    let m = marker.display();
    script(&format!(
        "$p = Start-Process -PassThru -NoNewWindow powershell -ArgumentList '-NoProfile','-Command','Start-Sleep 30'; Set-Content -Path '{m}' -Value $p.Id; exit 0"
    ))
}

#[cfg(unix)]
fn kill_pid(pid: &str) {
    let _ = Command::new("kill").arg(pid).status();
}

#[cfg(not(unix))]
fn kill_pid(pid: &str) {
    let _ = Command::new("taskkill")
        .args(["/PID", pid, "/T", "/F"])
        .status();
}

#[test]
fn run_status_reports_exit_code() {
    let status = run_status(emit_cmd("o", "e", 3), ChildLimits::default()).unwrap();
    assert_eq!(status.code(), Some(3));
    let status = run_status(emit_cmd("o", "e", 0), ChildLimits::default()).unwrap();
    assert!(status.success());
}

#[test]
fn run_status_spawn_failure_is_io_error() {
    let err = run_status(
        Command::new("definitely-not-a-real-binary-q2"),
        ChildLimits::default(),
    )
    .unwrap_err();
    assert!(matches!(err, ChildError::Io(_)), "got {err:?}");
}

#[test]
fn run_status_times_out_and_kills() {
    let limits = ChildLimits {
        deadline: Some(Duration::from_millis(300)),
        cancellation: None,
    };
    let start = Instant::now();
    let err = run_status(sleep_cmd(30), limits).unwrap_err();
    assert!(matches!(err, ChildError::TimedOut), "got {err:?}");
    assert!(start.elapsed() < Duration::from_secs(10));
}

/// A grandchild that outlives the child does not hold `run_status` open:
/// there are no pipes to drain.
#[test]
fn run_status_ignores_background_grandchild() {
    let dir = std::env::temp_dir().join(format!(
        "q2-run-status-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let marker = dir.join("grandchild.pid");
    let limits = ChildLimits {
        deadline: Some(Duration::from_secs(20)),
        cancellation: None,
    };
    let start = Instant::now();
    let result = run_status(background_child_then_exit0_cmd(&marker), limits);
    let elapsed = start.elapsed();
    let pid = std::fs::read_to_string(&marker)
        .map(|s| s.trim().to_string())
        .unwrap_or_default();
    if !pid.is_empty() {
        kill_pid(&pid);
    }
    let _ = std::fs::remove_dir_all(&dir);

    let status = result.unwrap();
    assert!(status.success());
    assert!(
        elapsed < Duration::from_secs(10),
        "returned after {elapsed:?}"
    );
    assert!(
        !pid.is_empty() && pid.chars().all(|c| c.is_ascii_digit()),
        "marker held {pid:?}"
    );
}
