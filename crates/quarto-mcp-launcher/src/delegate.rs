//! Hand control to `node <bundle>/index.mjs [args…]`.
//!
//! stdio passes through untouched — stdout belongs to the MCP
//! protocol, and the launcher itself never writes to it.
//!
//! Unix: `exec()` replaces the launcher entirely, so signals, exit
//! codes, and stdin-EOF semantics are node's with no middleman. The
//! lifetime shared lock (see cache.rs) must survive into node: file
//! descriptors persist across exec unless close-on-exec is set, and
//! Rust opens files with O_CLOEXEC — so we clear FD_CLOEXEC on the
//! lock fd first. The lock then releases exactly when the node process
//! exits, however it exits.
//!
//! Windows: no exec; spawn with inherited stdio, hold the lock in the
//! (still-alive) launcher, wait, and forward the exit code. MCP hosts
//! terminate stdio servers by closing stdin, which the server handles
//! (bd-9jq2a060), so the child exits and we follow.

use anyhow::Result;
use std::fs::File;
use std::path::Path;
use std::process::Command;

/// Returns only on failure to launch (Unix) or with the child's exit
/// code (Windows). The caller turns the code into `process::exit`.
pub fn delegate(node: &Path, entry: &Path, args: &[String], lock: File) -> Result<i32> {
    let mut cmd = Command::new(node);
    cmd.arg(entry).args(args);

    #[cfg(unix)]
    {
        use std::os::fd::AsRawFd;
        use std::os::unix::process::CommandExt;

        let fd = lock.as_raw_fd();
        // SAFETY: plain fcntl flag manipulation on an fd we own.
        unsafe {
            let flags = libc::fcntl(fd, libc::F_GETFD);
            if flags >= 0 {
                libc::fcntl(fd, libc::F_SETFD, flags & !libc::FD_CLOEXEC);
            }
        }
        let err = cmd.exec(); // only returns on failure
        Err(anyhow::Error::new(err).context(format!(
            "failed to exec {} {}",
            node.display(),
            entry.display()
        )))
    }

    #[cfg(not(unix))]
    {
        let status = cmd.status().map_err(|e| {
            anyhow::Error::new(e).context(format!(
                "failed to run {} {}",
                node.display(),
                entry.display()
            ))
        })?;
        // Keep the lock alive for the child's whole lifetime.
        drop(lock);
        Ok(status.code().unwrap_or(1))
    }
}
