//! Unix domain socket transport (epic bd-kosgkxjt; plan:
//! claude-notes/plans/2026-10-06-hub-unix-domain-socket-transport.md).
//!
//! Exclusive, opt-in alternative to the TCP+WebSocket transport: when
//! `HubConfig::socket_path` is set, the hub serves the entire axum
//! router over a filesystem unix socket and binds no TCP listener.
//!
//! The module compiles on every platform — CLI validation helpers are
//! platform-neutral and only the listener machinery is `cfg(unix)` —
//! so the CLI rejects `--socket-path` with a clear error on Windows.

use std::path::{Path, PathBuf};

/// Soft limit for socket paths. `sun_path` is 104 bytes on macOS and
/// 108 on Linux; stay under both with margin.
pub const SUN_PATH_SOFT_LIMIT: usize = 100;

/// CLI validation: UDS mode exists on unix only. Compiled on every
/// platform so the Windows rejection runs through the same code path
/// as every other flag validation.
pub fn validate_socket_platform() -> Result<(), String> {
    #[cfg(unix)]
    {
        Ok(())
    }
    #[cfg(windows)]
    {
        Err("--socket-path is not supported on Windows; use TCP (--host/--port)".to_string())
    }
    #[cfg(not(any(unix, windows)))]
    {
        Err("--socket-path requires unix domain sockets (Linux and macOS only)".to_string())
    }
}

/// CLI validation: hub-enforced auth over UDS is out of scope for v1 —
/// UDS connections are unauthenticated local-trusted (filesystem
/// permissions plus the accept-time peer-uid check). A hub that needs
/// OIDC runs TCP behind its TLS proxy, as today.
pub fn validate_socket_args(
    socket_requested: bool,
    oidc_client_id: Option<&str>,
) -> Result<(), String> {
    if socket_requested && oidc_client_id.is_some() {
        return Err(
            "--socket-path cannot be combined with --oidc-client-id: hub-enforced auth \
             over a unix socket is out of scope for v1. A hub that needs OIDC runs TCP \
             behind its TLS proxy."
                .to_string(),
        );
    }
    Ok(())
}

/// Apply the `sun_path` guard to a requested socket path: paths beyond
/// [`SUN_PATH_SOFT_LIMIT`] fall back to a short hashed path under
/// `${TMPDIR}` and the effective path is recorded in a
/// `hub.sock.path` pointer file (mode `0600`) inside the convention
/// directory, so clients walking up to the project can still discover
/// the socket.
pub fn apply_sun_path_guard(requested: &Path, convention_dir: &Path) -> std::io::Result<PathBuf> {
    if requested.as_os_str().len() <= SUN_PATH_SOFT_LIMIT {
        return Ok(requested.to_path_buf());
    }

    use sha2::Digest as _;

    let hash = sha2::Sha256::digest(convention_dir.to_string_lossy().as_bytes());
    let fallback = std::env::temp_dir().join(format!("q2-hub-{}.sock", hex::encode(&hash[..8])));

    std::fs::create_dir_all(convention_dir)?;
    let pointer = convention_dir.join("hub.sock.path");
    std::fs::write(&pointer, fallback.to_string_lossy().as_bytes())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(&pointer, std::fs::Permissions::from_mode(0o600))?;
    }

    Ok(fallback)
}

#[cfg(unix)]
pub use unix_imp::*;

#[cfg(unix)]
mod unix_imp {
    use std::path::Path;

    use anyhow::{Context, anyhow};
    use tokio::net::{UnixListener, UnixStream};
    use tracing::{debug, info, warn};

    /// An axum [`Listener`](axum::serve::Listener) over a filesystem
    /// unix socket that enforces the peer-uid allowlist at accept
    /// time — defense-in-depth behind the `0600`/`0700` filesystem
    /// floor. Accept iff the peer's uid is the hub's own or is in
    /// `--socket-allow-uid`; mismatches are dropped and logged, so
    /// only allowlisted streams ever reach the router.
    pub struct UdsListener {
        inner: UnixListener,
        own_uid: u32,
        allowed_uids: Vec<u32>,
    }

    impl axum::serve::Listener for UdsListener {
        type Io = UnixStream;
        type Addr = tokio::net::unix::SocketAddr;

        async fn accept(&mut self) -> (Self::Io, Self::Addr) {
            loop {
                let (stream, addr) = match self.inner.accept().await {
                    Ok(pair) => pair,
                    Err(e) => {
                        warn!(error = %e, "unix socket accept failed; retrying");
                        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                        continue;
                    }
                };
                match stream.peer_cred() {
                    Ok(cred) if cred.uid() == self.own_uid => {
                        debug!(uid = cred.uid(), "UDS accept: local (hub's own uid)");
                        return (stream, addr);
                    }
                    Ok(cred) if self.allowed_uids.contains(&cred.uid()) => {
                        info!(uid = cred.uid(), "UDS accept: proxied (allowlisted uid)");
                        return (stream, addr);
                    }
                    Ok(cred) => {
                        warn!(
                            uid = cred.uid(),
                            "UDS connection dropped: peer uid is neither the hub's \
                             own nor in --socket-allow-uid"
                        );
                    }
                    Err(e) => {
                        warn!(error = %e, "UDS connection dropped: peer_cred() failed");
                    }
                }
            }
        }

        fn local_addr(&self) -> std::io::Result<Self::Addr> {
            self.inner.local_addr()
        }
    }

    /// Bind the UDS listener with the full socket lifecycle:
    ///
    /// - **stale-socket reclaim**: an existing path is connect-probed;
    ///   a live hub answers → fail with "already running"; a dead file
    ///   (connection refused) → unlinked and rebound;
    /// - **explicit permissions**: a hub-created parent dir is `0700`
    ///   and the socket `0600` by default, set explicitly (never
    ///   inherited from the umask). With `--socket-group`: `0750` /
    ///   `0660`, group-owned by that group — the trusted-proxy shape.
    ///   A pre-existing parent dir is left untouched (never chmod or
    ///   chgrp a directory the hub did not create — it could be
    ///   `/tmp`); under `--socket-group` it must already be traversable
    ///   by the group, or startup fails loudly (see
    ///   [`ensure_group_traversable`]);
    /// - **peer-uid allowlist**: see [`UdsListener`].
    pub async fn bind_uds_listener(
        path: &Path,
        group: Option<&str>,
        allowed_uids: &[u32],
    ) -> anyhow::Result<UdsListener> {
        use std::os::unix::fs::PermissionsExt as _;

        let (dir_mode, sock_mode) = if group.is_some() {
            (0o750, 0o660)
        } else {
            (0o700, 0o600)
        };

        let parent = path.parent().filter(|p| !p.as_os_str().is_empty());
        let mut created_parent = false;
        if let Some(parent) = parent
            && !parent.exists()
        {
            std::fs::create_dir_all(parent)?;
            std::fs::set_permissions(parent, std::fs::Permissions::from_mode(dir_mode))?;
            created_parent = true;
        }

        // Stale-socket reclaim: probe before unlinking so a live hub is
        // never clobbered.
        match UnixStream::connect(path).await {
            Ok(_) => {
                anyhow::bail!(
                    "a hub is already running on socket {} (refusing to unlink a live socket)",
                    path.display()
                );
            }
            Err(_) => match std::fs::remove_file(path) {
                Ok(()) => info!(socket = %path.display(), "Reclaimed stale socket file"),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e.into()),
            },
        }

        let listener = UnixListener::bind(path)?;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(sock_mode))?;

        if let Some(group) = group {
            let gid = resolve_group(group)?;
            chgrp(path, gid).with_context(|| format!("cannot chgrp socket to '{group}'"))?;
            if let (true, Some(parent)) = (created_parent, parent) {
                chgrp(parent, gid).with_context(|| format!("cannot chgrp dir to '{group}'"))?;
            } else if let Some(parent) = parent {
                ensure_group_traversable(parent, gid, group)?;
            }
        }

        Ok(UdsListener {
            inner: listener,
            own_uid: current_uid(),
            allowed_uids: allowed_uids.to_vec(),
        })
    }

    fn current_uid() -> u32 {
        unsafe { libc::geteuid() }
    }

    /// The trusted-proxy shape needs the proxy's group to traverse the
    /// socket's parent dir. The hub never modifies directories it did
    /// not create (a `--socket-path /tmp/x.sock` must not chmod `/tmp`,
    /// and a chgrp'd data dir would leak the automerge store to the
    /// group), so a pre-existing parent must already be traversable —
    /// fail loudly with the remedy when it isn't, rather than letting
    /// the proxy discover EACCES at runtime.
    fn ensure_group_traversable(
        parent: &Path,
        gid: libc::gid_t,
        group: &str,
    ) -> anyhow::Result<()> {
        use std::os::unix::fs::MetadataExt as _;

        let md = std::fs::metadata(parent)?;
        let mode = md.mode() & 0o777;
        // Traversal needs the execute bit only: o+x, or the dir already
        // group-owned by the target group with g+x (the restart case —
        // a hub-created 0750 dir on a previous run).
        let traversable = mode & 0o001 != 0 || (md.gid() == gid && mode & 0o010 != 0);
        if !traversable {
            anyhow::bail!(
                "--socket-group '{group}': pre-existing parent dir {} (mode {mode:04o}) \
                 is not traversable by that group, so the proxy cannot reach the socket. \
                 The hub never modifies directories it did not create — fix the dir \
                 yourself (chgrp '{group}' it and chmod 0750) or choose a socket path \
                 in a dedicated directory.",
                parent.display()
            );
        }
        Ok(())
    }

    /// Resolve a group name (or a numeric gid) to a gid, failing
    /// loudly when it doesn't exist.
    fn resolve_group(name: &str) -> anyhow::Result<libc::gid_t> {
        if let Ok(gid) = name.parse::<u32>() {
            return Ok(gid);
        }
        let c_name = std::ffi::CString::new(name)?;
        let mut grp: libc::group = unsafe { std::mem::zeroed() };
        let mut result: *mut libc::group = std::ptr::null_mut();
        let mut buf = vec![0u8; 4096];
        let rc = unsafe {
            libc::getgrnam_r(
                c_name.as_ptr(),
                &mut grp,
                buf.as_mut_ptr() as *mut libc::c_char,
                buf.len(),
                &mut result,
            )
        };
        if rc != 0 || result.is_null() {
            return Err(anyhow!("unknown group '{name}' (--socket-group)"));
        }
        Ok(unsafe { (*result).gr_gid })
    }

    /// Set group ownership, leaving the owner unchanged.
    fn chgrp(path: &Path, gid: u32) -> std::io::Result<()> {
        use std::os::unix::ffi::OsStrExt as _;

        let c_path = std::ffi::CString::new(path.as_os_str().as_bytes())
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
        // (uid_t)-1 leaves the owner unchanged.
        let rc = unsafe { libc::chown(c_path.as_ptr(), u32::MAX, gid) };
        if rc != 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn socket_platform_validation_runs_everywhere() {
        #[cfg(unix)]
        assert!(validate_socket_platform().is_ok());
        #[cfg(windows)]
        assert!(validate_socket_platform().unwrap_err().contains("Windows"));
    }

    #[test]
    fn socket_path_plus_oidc_is_rejected() {
        let err = validate_socket_args(true, Some("client")).unwrap_err();
        assert!(err.contains("--socket-path"));
        assert!(err.contains("--oidc-client-id"));
        // No socket → no rule.
        assert!(validate_socket_args(false, Some("client")).is_ok());
        assert!(validate_socket_args(true, None).is_ok());
    }

    #[test]
    fn short_socket_path_passes_the_guard_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let requested = dir.path().join("hub.sock");
        let effective = apply_sun_path_guard(&requested, dir.path()).unwrap();
        assert_eq!(effective, requested);
        assert!(!dir.path().join("hub.sock.path").exists());
    }

    #[cfg(unix)]
    #[test]
    fn long_socket_path_falls_back_with_a_pointer_file() {
        use std::os::unix::fs::MetadataExt as _;

        let dir = tempfile::tempdir().unwrap();
        let long = dir
            .path()
            .join(format!("{}hub.sock", "a".repeat(SUN_PATH_SOFT_LIMIT)));
        assert!(long.as_os_str().len() > SUN_PATH_SOFT_LIMIT);

        let effective = apply_sun_path_guard(&long, dir.path()).unwrap();
        assert_ne!(effective, long);
        assert!(
            effective.as_os_str().len() <= SUN_PATH_SOFT_LIMIT,
            "fallback {} still too long",
            effective.display()
        );

        // The pointer file records the effective path, mode 0600.
        let pointer = dir.path().join("hub.sock.path");
        let recorded = std::fs::read_to_string(&pointer).unwrap();
        assert_eq!(recorded, effective.to_string_lossy());
        assert_eq!(std::fs::metadata(&pointer).unwrap().mode() & 0o777, 0o600);

        // Deterministic for the same convention dir.
        let again = apply_sun_path_guard(&long, dir.path()).unwrap();
        assert_eq!(effective, again);
    }
}
