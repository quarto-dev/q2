//! Phase 1 (bd-mfapk5yl) of the UDS transport plan
//! (claude-notes/plans/2026-10-06-hub-unix-domain-socket-transport.md):
//! black-box tests of the `hub` binary's unix-domain-socket mode.
//!
//! Every test spawns the real `hub` binary and asserts from outside the
//! process — filesystem modes, TCP refusal, WS upgrades over the socket,
//! cross-uid probes — so the module compiles against the pre-feature
//! tree and fails at runtime: the binary does not yet accept
//! `--socket-path`. Phase 2 (bd-m49e3s5o) implements the flag and turns
//! these green.

use crate::support::SockDir;

/// `--socket-path` on Windows errors — and this test runs everywhere:
/// the flag must parse identically on every platform and be rejected by
/// validation only where UDS is unsupported. The unix arm doubles as a
/// smoke test that the flag is accepted here.
#[tokio::test]
async fn socket_path_errors_on_windows() {
    let dir = SockDir::new("platform");
    let sock = dir.sock();
    let data = dir.data("data");
    let sock_s = sock.to_string_lossy().into_owned();
    let data_s = data.to_string_lossy().into_owned();

    #[cfg(windows)]
    {
        use crate::support::run_hub_expect_failure;

        let (_status, stderr) =
            run_hub_expect_failure(&["--data-dir", &data_s, "--socket-path", &sock_s]);
        assert!(
            stderr.contains("Windows"),
            "expected a Windows-specific rejection of --socket-path, got:\n{stderr}"
        );
    }

    #[cfg(unix)]
    {
        use crate::support::{HubChild, wait_for_hub};
        use std::time::Duration;

        let mut hub = HubChild::spawn(&["--data-dir", &data_s, "--socket-path", &sock_s]);
        wait_for_hub(&mut hub, &sock, Duration::from_secs(15)).await;
    }
}

#[cfg(unix)]
mod unix_imp {
    use std::time::Duration;

    use crate::support::{
        HubChild, SockDir, command_stdout, free_port, run_hub_expect_failure, sigterm,
        unix_http_get, wait_for_hub,
    };

    /// The mismatched-uid accept tests re-execute this very test binary
    /// as a helper that drops privileges and probes the socket from
    /// another uid. These env vars drive the helper half.
    const HELPER_SOCK_ENV: &str = "Q2_UDS_HELPER_SOCK";
    const HELPER_UID_ENV: &str = "Q2_UDS_HELPER_UID";
    const HELPER_GID_ENV: &str = "Q2_UDS_HELPER_GID";
    const HELPER_EXPECT_ENV: &str = "Q2_UDS_HELPER_EXPECT";
    /// Cross-uid tests only run when explicitly enabled (CI runs them as
    /// root): `Q2_TEST_SETUID=1`.
    const SETUID_ENABLE_ENV: &str = "Q2_TEST_SETUID";

    // ── socket lifecycle ──────────────────────────────────────────

    /// A dead file where a crashed hub left its socket behind must be
    /// connect-probed, unlinked, and rebound — startup must not fail.
    #[tokio::test]
    async fn stale_socket_file_is_reclaimed() {
        let dir = SockDir::new("stale");
        let sock = dir.sock();
        std::fs::write(&sock, b"corpse of a previous hub").unwrap();

        let data = dir.data("data");
        let mut hub = HubChild::spawn(&[
            "--data-dir",
            &data.to_string_lossy(),
            "--socket-path",
            &sock.to_string_lossy(),
        ]);
        wait_for_hub(&mut hub, &sock, Duration::from_secs(15)).await;

        // The reclaimed socket is a live hub, not the stale file.
        let (status, _body) = unix_http_get(&sock, "/health").await;
        assert_eq!(status, 200);
    }

    /// A second hub on the same path must refuse to start while the
    /// first is alive — never unlink a live socket.
    #[tokio::test]
    async fn live_socket_refuses_second_hub() {
        let dir = SockDir::new("live");
        let sock = dir.sock();
        let sock_s = sock.to_string_lossy().into_owned();

        let data_a = dir.data("data-a");
        let mut first = HubChild::spawn(&[
            "--data-dir",
            &data_a.to_string_lossy(),
            "--socket-path",
            &sock_s,
        ]);
        wait_for_hub(&mut first, &sock, Duration::from_secs(15)).await;

        let data_b = dir.data("data-b");
        let (_status, stderr) = run_hub_expect_failure(&[
            "--data-dir",
            &data_b.to_string_lossy(),
            "--socket-path",
            &sock_s,
        ]);
        assert!(
            stderr.contains("already running"),
            "expected 'already running' refusal, got:\n{stderr}"
        );
    }

    /// Default posture: parent dir `0700`, socket `0600`, both set
    /// explicitly (never inherited from the umask); graceful shutdown
    /// removes the socket file.
    #[tokio::test]
    async fn socket_permissions_and_cleanup_on_graceful_shutdown() {
        use std::os::unix::fs::MetadataExt;

        let dir = SockDir::new("perms");
        // Explicit path under a not-yet-existing parent: the hub creates it.
        let parent = dir.child("hubdir");
        let sock = parent.join("hub.sock");
        let data = dir.data("data");

        let mut hub = HubChild::spawn(&[
            "--data-dir",
            &data.to_string_lossy(),
            "--socket-path",
            &sock.to_string_lossy(),
        ]);
        wait_for_hub(&mut hub, &sock, Duration::from_secs(15)).await;

        let dir_mode = std::fs::metadata(&parent).unwrap().mode() & 0o777;
        let sock_mode = std::fs::metadata(&sock).unwrap().mode() & 0o777;
        assert_eq!(dir_mode, 0o700, "socket parent dir mode");
        assert_eq!(sock_mode, 0o600, "socket file mode");

        sigterm(hub.pid());
        let status = hub.wait_for_exit(Duration::from_secs(10));
        assert!(
            status.success(),
            "graceful shutdown exit status: {status}; stderr:\n{}",
            hub.stderr_text()
        );
        assert!(
            !sock.exists(),
            "socket file must be removed on graceful shutdown"
        );
    }

    /// Trusted-proxy shape: `--socket-group` relaxes the filesystem
    /// floor to `0750`/`0660` owned by that group, so a different-uid
    /// proxy can connect.
    #[tokio::test]
    async fn socket_group_relaxes_permissions() {
        use std::os::unix::fs::MetadataExt;

        let group = command_stdout("id", &["-gn"]);
        let gid: u32 = command_stdout("id", &["-g"]).parse().unwrap();

        let dir = SockDir::new("group");
        let parent = dir.child("hubdir");
        let sock = parent.join("hub.sock");
        let data = dir.data("data");

        let mut hub = HubChild::spawn(&[
            "--data-dir",
            &data.to_string_lossy(),
            "--socket-path",
            &sock.to_string_lossy(),
            "--socket-group",
            &group,
        ]);
        wait_for_hub(&mut hub, &sock, Duration::from_secs(15)).await;

        let dir_md = std::fs::metadata(&parent).unwrap();
        let sock_md = std::fs::metadata(&sock).unwrap();
        assert_eq!(dir_md.mode() & 0o777, 0o750, "proxy-mode dir perms");
        assert_eq!(sock_md.mode() & 0o777, 0o660, "proxy-mode socket perms");
        assert_eq!(dir_md.gid(), gid, "dir group ownership");
        assert_eq!(sock_md.gid(), gid, "socket group ownership");
    }

    /// A `--socket-group` the hub cannot chgrp to must fail loudly at
    /// startup, not silently fall back to private permissions.
    #[test]
    fn socket_group_unknown_group_fails_loudly() {
        let dir = SockDir::new("badgroup");
        let sock = dir.sock();
        let data = dir.data("data");
        let (_status, stderr) = run_hub_expect_failure(&[
            "--data-dir",
            &data.to_string_lossy(),
            "--socket-path",
            &sock.to_string_lossy(),
            "--socket-group",
            "q2-no-such-group",
        ]);
        assert!(
            stderr.contains("q2-no-such-group"),
            "failure must name the unresolvable group, got:\n{stderr}"
        );
    }

    /// Fail-loud safety (bd-m49e3s5o review): with `--socket-group`, a
    /// pre-existing parent dir the proxy group cannot traverse is a
    /// startup error naming the dir — the hub never modifies
    /// directories it did not create, so it must not start a shape the
    /// proxy can't reach.
    #[tokio::test]
    async fn socket_group_preexisting_untraversable_parent_fails_loudly() {
        use std::os::unix::fs::PermissionsExt;

        let group = command_stdout("id", &["-gn"]);
        let dir = SockDir::new("badparent");
        // Pre-existing parent the proxy group cannot traverse: even
        // though its group == the socket group, 0700 grants no g+x.
        let parent = dir.child("hubdir");
        std::fs::create_dir_all(&parent).unwrap();
        std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o700)).unwrap();
        let sock = parent.join("hub.sock");
        let data = dir.data("data");

        let mut hub = HubChild::spawn(&[
            "--data-dir",
            &data.to_string_lossy(),
            "--socket-path",
            &sock.to_string_lossy(),
            "--socket-group",
            &group,
        ]);
        let status = hub.wait_for_exit(Duration::from_secs(10));
        assert!(
            !status.success(),
            "hub must refuse to start when the proxy group cannot traverse the parent dir"
        );
        let stderr = hub.stderr_text();
        assert!(
            stderr.contains(parent.to_string_lossy().as_ref()),
            "error must name the parent dir, got:\n{stderr}"
        );
    }

    /// The other half of the pre-existing-parent rule: a parent that is
    /// already traversable (e.g. a storage-created data dir at 0755)
    /// passes the check — and stays byte-for-byte untouched: the hub
    /// never modifies directories it did not create.
    #[tokio::test]
    async fn socket_group_preexisting_traversable_parent_is_untouched() {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};

        let group = command_stdout("id", &["-gn"]);
        let gid: u32 = command_stdout("id", &["-g"]).parse().unwrap();
        let dir = SockDir::new("okparent");
        let parent = dir.child("hubdir");
        std::fs::create_dir_all(&parent).unwrap();
        std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o755)).unwrap();
        let sock = parent.join("hub.sock");
        let data = dir.data("data");

        let mut hub = HubChild::spawn(&[
            "--data-dir",
            &data.to_string_lossy(),
            "--socket-path",
            &sock.to_string_lossy(),
            "--socket-group",
            &group,
        ]);
        wait_for_hub(&mut hub, &sock, Duration::from_secs(15)).await;

        let parent_md = std::fs::metadata(&parent).unwrap();
        assert_eq!(
            parent_md.mode() & 0o777,
            0o755,
            "pre-existing parent left untouched"
        );
        let sock_md = std::fs::metadata(&sock).unwrap();
        assert_eq!(sock_md.mode() & 0o777, 0o660, "socket mode");
        assert_eq!(sock_md.gid(), gid, "socket group ownership");
    }

    /// Exclusive-UDS mode: no TCP socket exists to reach by any means.
    #[tokio::test]
    async fn uds_mode_binds_no_tcp_listener() {
        let dir = SockDir::new("notcp");
        let sock = dir.sock();
        let data = dir.data("data");
        let port = free_port();

        let mut hub = HubChild::spawn(&[
            "--data-dir",
            &data.to_string_lossy(),
            "--socket-path",
            &sock.to_string_lossy(),
            "-P",
            &port.to_string(),
        ]);
        wait_for_hub(&mut hub, &sock, Duration::from_secs(15)).await;

        let refused = tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .is_err();
        assert!(
            refused,
            "hub in UDS mode must not listen on 127.0.0.1:{port}"
        );
    }

    // ── peer-uid accept check ─────────────────────────────────────

    /// Defense-in-depth accept check: a same-uid client completes the
    /// websocket upgrade.
    #[tokio::test]
    async fn peer_cred_same_uid_is_accepted() {
        let dir = SockDir::new("sameuid");
        let sock = dir.sock();
        let data = dir.data("data");

        let mut hub = HubChild::spawn(&[
            "--data-dir",
            &data.to_string_lossy(),
            "--socket-path",
            &sock.to_string_lossy(),
        ]);
        wait_for_hub(&mut hub, &sock, Duration::from_secs(15)).await;

        let (_ws, response) = ws_upgrade_over(&sock).await.expect("same-uid upgrade");
        assert_eq!(response.status().as_u16(), 101);
    }

    /// websocket upgrade over a unix socket, returning the 101 response.
    async fn ws_upgrade_over(
        sock: &std::path::Path,
    ) -> Result<
        (
            tokio_tungstenite::WebSocketStream<tokio::net::UnixStream>,
            tungstenite::handshake::client::Response,
        ),
        tungstenite::Error,
    > {
        use tungstenite::client::IntoClientRequest;

        let stream = tokio::net::UnixStream::connect(sock).await?;
        let request = "ws://localhost/ws".into_client_request()?;
        tokio_tungstenite::client_async(request, stream).await
    }

    /// The peer-uid allowlist, probed from a second uid. The hub runs
    /// with `--socket-group <nobody's group>` so filesystem permissions
    /// deliberately *allow* the connection — what rejects the peer is
    /// the accept-time uid check, not DAC. With `--socket-allow-uid`,
    /// the same probe must succeed.
    ///
    /// CI-only: needs root to setuid down; enable with `Q2_TEST_SETUID=1`.
    /// Skips loudly otherwise.
    #[tokio::test]
    async fn peer_cred_other_uid_rejected_unless_allowlisted() {
        // ── helper half (re-executed binary, drops privileges) ──
        if let (Ok(sock), Ok(uid), Ok(gid), Ok(expect)) = (
            std::env::var(HELPER_SOCK_ENV),
            std::env::var(HELPER_UID_ENV),
            std::env::var(HELPER_GID_ENV),
            std::env::var(HELPER_EXPECT_ENV),
        ) {
            let uid: u32 = uid.parse().unwrap();
            let gid: u32 = gid.parse().unwrap();
            unsafe {
                libc::setgid(gid);
                libc::setuid(uid);
            }
            let probe = ws_upgrade_over(std::path::Path::new(&sock));
            let accepted = tokio::time::timeout(Duration::from_secs(10), probe)
                .await
                .ok()
                .and_then(|r| r.ok())
                .is_some();
            let code = match (expect.as_str(), accepted) {
                ("accept", true) | ("reject", false) => 0,
                ("accept", false) => 11,
                ("reject", true) => 12,
                _ => 13,
            };
            std::process::exit(code);
        }

        // ── parent half ──
        if std::env::var(SETUID_ENABLE_ENV).is_err() || unsafe { libc::geteuid() } != 0 {
            eprintln!(
                "skipping: cross-uid accept tests need root and {SETUID_ENABLE_ENV}=1 \
                 (CI-only; see the plan's Testing section)"
            );
            return;
        }

        let nobody_uid = command_stdout("id", &["-u", "nobody"]);
        let nobody_gid = command_stdout("id", &["-g", "nobody"]);
        let nobody_group = command_stdout("id", &["-gn", "nobody"]);
        let this_test = "uds_server::unix_imp::peer_cred_other_uid_rejected_unless_allowlisted";

        // Case 1: DAC permits (group-relaxed socket), the accept-time
        // uid check must still reject the unlisted uid.
        let dir = SockDir::new("xuid-rej");
        let sock = dir.sock();
        let data = dir.data("data");
        let sock_s = sock.to_string_lossy().into_owned();
        let mut hub = HubChild::spawn(&[
            "--data-dir",
            &data.to_string_lossy(),
            "--socket-path",
            &sock_s,
            "--socket-group",
            &nobody_group,
        ]);
        wait_for_hub(&mut hub, &sock, Duration::from_secs(15)).await;
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", this_test, "--nocapture"])
            .env(HELPER_SOCK_ENV, &sock_s)
            .env(HELPER_UID_ENV, &nobody_uid)
            .env(HELPER_GID_ENV, &nobody_gid)
            .env(HELPER_EXPECT_ENV, "reject")
            .status()
            .unwrap();
        assert!(
            status.success(),
            "unlisted uid must be rejected by the accept check (helper exit {status})"
        );
        drop(hub);

        // Case 2: with --socket-allow-uid, the allowlisted uid connects.
        let dir = SockDir::new("xuid-ok");
        let sock = dir.sock();
        let data = dir.data("data");
        let sock_s = sock.to_string_lossy().into_owned();
        let mut hub = HubChild::spawn(&[
            "--data-dir",
            &data.to_string_lossy(),
            "--socket-path",
            &sock_s,
            "--socket-group",
            &nobody_group,
            "--socket-allow-uid",
            &nobody_uid,
        ]);
        wait_for_hub(&mut hub, &sock, Duration::from_secs(15)).await;
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", this_test, "--nocapture"])
            .env(HELPER_SOCK_ENV, &sock_s)
            .env(HELPER_UID_ENV, &nobody_uid)
            .env(HELPER_GID_ENV, &nobody_gid)
            .env(HELPER_EXPECT_ENV, "accept")
            .status()
            .unwrap();
        assert!(
            status.success(),
            "allowlisted uid must be accepted (helper exit {status})"
        );
    }

    // ── CLI validation ────────────────────────────────────────────

    /// Hub-enforced auth over UDS is out of scope for v1: the
    /// combination fails CLI validation.
    #[test]
    fn socket_path_with_oidc_client_id_fails_validation() {
        let dir = SockDir::new("oidc");
        let sock = dir.sock();
        let data = dir.data("data");
        let (_status, stderr) = run_hub_expect_failure(&[
            "--data-dir",
            &data.to_string_lossy(),
            "--socket-path",
            &sock.to_string_lossy(),
            "--oidc-client-id",
            "test-client",
            "--allow-insecure-auth",
        ]);
        assert!(
            stderr.contains("--socket-path") && stderr.contains("--oidc-client-id"),
            "validation error must name both flags, got:\n{stderr}"
        );
    }

    /// `--behind-tls-proxy` is inert without OIDC on any transport —
    /// the trusted-proxy deployment passes nothing special.
    #[tokio::test]
    async fn socket_path_with_behind_tls_proxy_is_accepted() {
        let dir = SockDir::new("tlsproxy");
        let sock = dir.sock();
        let data = dir.data("data");
        let mut hub = HubChild::spawn(&[
            "--data-dir",
            &data.to_string_lossy(),
            "--socket-path",
            &sock.to_string_lossy(),
            "--behind-tls-proxy",
        ]);
        wait_for_hub(&mut hub, &sock, Duration::from_secs(15)).await;
    }

    /// Bare `--socket-path` (no value) in project mode resolves to the
    /// convention path — what makes zero-config client discovery work.
    #[tokio::test]
    async fn bare_socket_path_resolves_convention_in_project_mode() {
        // Keep the *canonicalized* path well under the 100-byte sun_path
        // guard: the hub canonicalizes the project root, and on macOS
        // per-user tempdirs resolve to long /var/folders (or /private/tmp)
        // paths that would push the convention socket past the guard and
        // into the hashed fallback. /tmp is short on every unix.
        let base = std::path::PathBuf::from(format!("/tmp/q2p-{}", std::process::id()));
        let project = base.join("proj");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(project.join("_quarto.yml"), "project: website\n").unwrap();
        let convention = project.join(".quarto").join("hub").join("hub.sock");

        let mut hub = HubChild::spawn(&["--project", &project.to_string_lossy(), "--socket-path"]);
        wait_for_hub(&mut hub, &convention, Duration::from_secs(15)).await;
        drop(hub);
        let _ = std::fs::remove_dir_all(&base);
    }

    /// Bare `--socket-path` in standalone mode resolves under the data dir.
    #[tokio::test]
    async fn bare_socket_path_resolves_convention_in_standalone_mode() {
        let dir = SockDir::new("convstd");
        let data = dir.data("data");
        let convention = data.join("hub.sock");

        let mut hub = HubChild::spawn(&["--data-dir", &data.to_string_lossy(), "--socket-path"]);
        wait_for_hub(&mut hub, &convention, Duration::from_secs(15)).await;
    }
}
