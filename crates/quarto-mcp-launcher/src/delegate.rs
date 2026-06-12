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

/// Assemble the child command: `node <entry> [args…]`, inheriting the
/// parent environment plus `extra_env` (the bundled hub defaults — see
/// `defaults.rs`; the caller has already resolved user-env precedence,
/// so everything in `extra_env` is set unconditionally). Split from
/// [`delegate`] because the Unix path execs and can never be observed
/// by a test.
pub(crate) fn build_command(
    node: &Path,
    entry: &Path,
    args: &[String],
    extra_env: &[(&str, &str)],
) -> Command {
    let mut cmd = Command::new(node);
    cmd.arg(entry).args(args);
    for (var, value) in extra_env {
        cmd.env(var, value);
    }
    cmd
}

/// Returns only on failure to launch (Unix) or with the child's exit
/// code (Windows). The caller turns the code into `process::exit`.
pub fn delegate(
    node: &Path,
    entry: &Path,
    args: &[String],
    extra_env: &[(&str, &str)],
    lock: File,
) -> Result<i32> {
    let mut cmd = build_command(node, entry, args, extra_env);

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

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;
    use std::path::PathBuf;

    fn env_of(cmd: &Command) -> Vec<(String, String)> {
        cmd.get_envs()
            .filter_map(|(k, v)| {
                Some((
                    k.to_string_lossy().into_owned(),
                    v?.to_string_lossy().into_owned(),
                ))
            })
            .collect()
    }

    #[test]
    fn build_command_shape_is_node_entry_args() {
        let cmd = build_command(
            &PathBuf::from("/usr/bin/node"),
            &PathBuf::from("/cache/index.mjs"),
            &["--server".to_string(), "ws://x".to_string()],
            &[],
        );
        assert_eq!(cmd.get_program(), OsStr::new("/usr/bin/node"));
        let args: Vec<_> = cmd.get_args().map(|a| a.to_string_lossy()).collect();
        assert_eq!(args, vec!["/cache/index.mjs", "--server", "ws://x"]);
        assert!(env_of(&cmd).is_empty(), "no injections requested");
    }

    #[test]
    fn build_command_sets_extra_env_on_the_child() {
        let cmd = build_command(
            &PathBuf::from("node"),
            &PathBuf::from("index.mjs"),
            &[],
            &[
                ("QUARTO_HUB_MCP_CLIENT_ID", "id-123"),
                ("QUARTO_HUB_SERVER", "wss://quarto-hub.com/ws"),
            ],
        );
        assert_eq!(
            env_of(&cmd),
            vec![
                ("QUARTO_HUB_MCP_CLIENT_ID".to_string(), "id-123".to_string()),
                (
                    "QUARTO_HUB_SERVER".to_string(),
                    "wss://quarto-hub.com/ws".to_string()
                ),
            ]
        );
        // Inherited environment is untouched: no env_clear, no removals.
        assert!(
            cmd.get_envs().all(|(_, v)| v.is_some()),
            "no variable removals expected"
        );
    }
}
