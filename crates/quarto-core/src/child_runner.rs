/*
 * child_runner.rs
 * Copyright (c) 2025 Posit, PBC
 */

//! One helper for every native child process the render and preview paths
//! start (typst, pandoc for downloads, pandoc for import).
//!
//! [`run_child`] replaces `Command::output()` with the same captured-output
//! contract plus a wall-clock deadline and a [`Cancellation`] check. It uses
//! `std::process` and plain threads, so it can be driven from
//! `spawn_blocking` without needing tokio's `process` feature.
//!
//! When either limit is set the child is started in its own process group
//! (unix), and a timeout or cancel kills the whole group, so a grandchild
//! (a pandoc `--filter`) cannot keep the output pipes open. On Windows only
//! the child itself is killed; the post-kill grace period keeps the caller
//! from hanging on a surviving grandchild.

use std::io::Read;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

use crate::stage::Cancellation;

/// How often the wait loop re-checks the child, the deadline and the token.
const POLL: Duration = Duration::from_millis(10);

/// Deadline for the version and font-listing probes.
const PROBE_DEADLINE: Duration = Duration::from_secs(30);

/// After a kill, how long to wait for the pipe-drain threads before leaving
/// them to finish on their own.
const DRAIN_GRACE: Duration = Duration::from_millis(500);

/// Limits for one child. The default is unbounded, like `Command::output()`.
#[derive(Debug, Clone, Default)]
pub struct ChildLimits {
    /// Wall-clock budget from spawn. `None` means no deadline.
    pub deadline: Option<Duration>,
    /// Checked on every poll; once cancelled the child is killed.
    pub cancellation: Option<Cancellation>,
}

impl ChildLimits {
    fn is_bounded(&self) -> bool {
        self.deadline.is_some() || self.cancellation.is_some()
    }

    fn violation(&self, start: Instant) -> Option<ChildError> {
        if self.cancellation.as_ref().is_some_and(|c| c.is_cancelled()) {
            return Some(ChildError::Cancelled);
        }
        match self.deadline {
            Some(d) if start.elapsed() >= d => Some(ChildError::TimedOut),
            _ => None,
        }
    }
}

/// Captured result of a child that ran to completion.
#[derive(Debug)]
pub struct ChildOutput {
    pub status: ExitStatus,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

#[derive(Debug, thiserror::Error)]
pub enum ChildError {
    #[error("child process exceeded its deadline and was killed")]
    TimedOut,
    #[error("child process was cancelled and killed")]
    Cancelled,
    #[error("child process I/O error: {0}")]
    Io(#[from] std::io::Error),
}

/// Spawn `cmd` and wait for it, capturing stdout and stderr.
///
/// stdin is closed, as with `Command::output()`. Both pipes are drained on
/// threads so a child writing more than a pipe buffer cannot deadlock.
pub fn run_child(mut cmd: Command, limits: ChildLimits) -> Result<ChildOutput, ChildError> {
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    if limits.is_bounded() {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    let start = Instant::now();
    let mut child = cmd.spawn()?;
    let stdout_rx = drain(child.stdout.take());
    let stderr_rx = drain(child.stderr.take());
    let bounded = limits.is_bounded();

    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {}
            Err(e) => {
                kill(&mut child, bounded);
                return Err(e.into());
            }
        }
        if let Some(err) = limits.violation(start) {
            kill(&mut child, bounded);
            settle(&[&stdout_rx, &stderr_rx]);
            return Err(err);
        }
        std::thread::sleep(POLL);
    };

    // The child exited, but a grandchild may still hold a pipe open; keep
    // honouring the limits while the drains finish.
    let mut collected = Vec::with_capacity(2);
    for rx in [&stdout_rx, &stderr_rx] {
        loop {
            match rx.recv_timeout(POLL) {
                Ok(bytes) => {
                    collected.push(bytes);
                    break;
                }
                // The drain thread died without sending; treat as empty.
                Err(RecvTimeoutError::Disconnected) => {
                    collected.push(Vec::new());
                    break;
                }
                Err(RecvTimeoutError::Timeout) => {
                    if let Some(err) = limits.violation(start) {
                        kill_group_of(&child);
                        settle(&[&stdout_rx, &stderr_rx]);
                        return Err(err);
                    }
                }
            }
        }
    }
    let stderr = collected.pop().unwrap_or_default();
    let stdout = collected.pop().unwrap_or_default();
    Ok(ChildOutput {
        status,
        stdout,
        stderr,
    })
}

/// Run a short probe (`--version`, `fonts`) under a fixed deadline, returning
/// its output only when it ran to completion. A hung probe is killed instead
/// of pinning the caller.
pub(crate) fn run_probe(cmd: Command) -> Option<ChildOutput> {
    run_child(
        cmd,
        ChildLimits {
            deadline: Some(PROBE_DEADLINE),
            cancellation: None,
        },
    )
    .ok()
}

/// Map a [`ChildError`] to the stage error a pipeline stage reports. Timeouts
/// and cancels read differently from a failure to start the tool.
pub(crate) fn stage_error(stage: &str, tool: &str, err: ChildError) -> crate::stage::PipelineError {
    let msg = match err {
        ChildError::TimedOut => format!("{tool} timed out and was killed"),
        ChildError::Cancelled => format!("{tool} was cancelled"),
        ChildError::Io(e) => format!("failed to execute {tool}: {e}"),
    };
    crate::stage::PipelineError::stage_error(stage, msg)
}

fn drain<R: Read + Send + 'static>(pipe: Option<R>) -> Receiver<Vec<u8>> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(mut pipe) = pipe {
            let _ = pipe.read_to_end(&mut buf);
        }
        let _ = tx.send(buf);
    });
    rx
}

/// Kill the child (its whole group when it has one) and reap it.
fn kill(child: &mut Child, own_group: bool) {
    if own_group {
        kill_group_of(child);
    }
    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(unix)]
fn kill_group_of(child: &Child) {
    // Only called for children started with `process_group(0)`, so the
    // group id equals the child's pid.
    if let Ok(pid) = i32::try_from(child.id()) {
        // SAFETY: plain syscall; a negative pid addresses the process group.
        unsafe {
            libc::kill(-pid, libc::SIGKILL);
        }
    }
}

#[cfg(not(unix))]
fn kill_group_of(_child: &Child) {}

/// Give the drain threads a short grace period to finish after a kill, then
/// leave any straggler to end on its own.
fn settle(rxs: &[&Receiver<Vec<u8>>]) {
    let until = Instant::now() + DRAIN_GRACE;
    for rx in rxs {
        let left = until.saturating_duration_since(Instant::now());
        let _ = rx.recv_timeout(left);
    }
}
