/*
 * engine/jupyter/daemon.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * In-process Jupyter kernel daemon.
 */

//! In-process daemon for managing Jupyter kernel sessions.
//!
//! The `JupyterDaemon` manages kernel lifecycle:
//! - Starting kernels on demand
//! - Reusing existing kernels for the same (kernel, working_dir) pair
//! - Cleaning up on shutdown
//!
//! # Kernel scopes (bd-hxhnnlzs)
//!
//! The daemon is a process-global static, and Rust never drops statics
//! — so nothing implicit ever shuts these kernels down. Left alone,
//! every spawned kernel outlives the process, reparents to PID 1, and
//! idles forever (2338 of them accumulated on one dev machine).
//!
//! Lifetime is therefore managed explicitly with refcounted **kernel
//! scopes**: every execution path that can spawn a kernel holds a
//! [`KernelScope`] (the jupyter engine's `execute_qmd` acquires one
//! around each engine run, so no caller can leak by accident), and
//! long-lived callers that want kernel reuse across engine runs — a
//! `q2 render` project invocation, the `q2 preview` server — hold an
//! outer scope for their own lifetime. When the last scope drops, all
//! sessions are shut down: `shutdown_request`, kill backstop,
//! connection-file removal.

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use jupyter_protocol::ConnectionInfo;
use tokio::sync::{Mutex, RwLock};
use uuid::Uuid;

use super::error::{JupyterError, Result};
use super::kernelspec;
use super::session::{KernelSession, SessionKey};

/// In-process daemon managing Jupyter kernel sessions.
///
/// The daemon maintains a pool of running kernels, indexed by
/// (kernel_name, working_dir). This allows kernel reuse across
/// multiple render operations on documents in the same directory.
pub struct JupyterDaemon {
    /// Active kernel sessions.
    sessions: RwLock<HashMap<SessionKey, KernelSession>>,
    /// Serializes kernel startup. Starting a kernel takes seconds and
    /// many await points; without this lock two concurrent renders of
    /// documents sharing a session key would both miss the sessions
    /// map and both spawn, with the loser's kernel evicted on insert
    /// (observed as duplicate ipykernel processes under project
    /// renders, bd-hxhnnlzs). Also keeps concurrent port allocations
    /// (`peek_ports`) from racing each other.
    start_lock: Mutex<()>,
}

impl JupyterDaemon {
    /// Create a new daemon with default settings.
    pub fn new() -> Self {
        Self {
            sessions: RwLock::new(HashMap::new()),
            start_lock: Mutex::new(()),
        }
    }

    /// Get or start a kernel session for the given key.
    ///
    /// If a session already exists for this (kernel, working_dir) pair,
    /// it is reused. Otherwise, a new kernel is started with
    /// `extra_env` applied on top of the inherited environment —
    /// project `_environment` pairs pre-filtered so the real
    /// environment wins. `extra_env` is a **spawn-time** input only:
    /// the session key deliberately excludes it, so a reused session
    /// keeps the env it was started with (sessions are keyed per
    /// working dir, and a render/preview serves one project, so the
    /// env is stable for a key's lifetime).
    pub async fn get_or_start_session(
        &self,
        kernel_name: &str,
        working_dir: &PathBuf,
        extra_env: &[(String, String)],
    ) -> Result<SessionKey> {
        let key = SessionKey::new(kernel_name, working_dir.clone());

        // Check if we have an existing session
        {
            let sessions = self.sessions.read().await;
            if sessions.contains_key(&key) {
                return Ok(key);
            }
        }

        if active_scopes() == 0 {
            // Not fatal — but this kernel will live until the process
            // exits (or until some other scope closes), which is
            // exactly the leak bd-hxhnnlzs is about. Callers should
            // hold a `kernel_scope()`.
            tracing::warn!(
                kernel = %key.kernel_name,
                "starting a Jupyter kernel outside any kernel scope; \
                 nothing will shut it down automatically"
            );
        }

        // Serialize startup and re-check: a concurrent caller may have
        // finished starting this very session while we awaited the
        // lock (the read-check/spawn/insert sequence is not atomic).
        let _starting = self.start_lock.lock().await;
        {
            let sessions = self.sessions.read().await;
            if sessions.contains_key(&key) {
                return Ok(key);
            }
        }
        self.start_kernel(&key, extra_env).await?;

        Ok(key)
    }

    /// Start a new kernel for the given key.
    async fn start_kernel(&self, key: &SessionKey, extra_env: &[(String, String)]) -> Result<()> {
        tracing::info!(kernel = %key.kernel_name, dir = %key.working_dir.display(),
            "Starting Jupyter kernel");

        // 1. Find kernelspec
        let kernel = kernelspec::find_kernelspec(&key.kernel_name).await?;

        // 2. Allocate ports for ZeroMQ channels
        let ip = IpAddr::V4(Ipv4Addr::LOCALHOST);
        let ports = runtimelib::peek_ports(ip, 5)
            .await
            .map_err(|e| JupyterError::PortAllocationFailed(e.to_string()))?;

        // 3. Build connection info
        let connection_info = ConnectionInfo {
            transport: jupyter_protocol::connection_info::Transport::TCP,
            ip: ip.to_string(),
            stdin_port: ports[0],
            control_port: ports[1],
            hb_port: ports[2],
            shell_port: ports[3],
            iopub_port: ports[4],
            signature_scheme: "hmac-sha256".to_string(),
            key: Uuid::new_v4().to_string(),
            kernel_name: Some(key.kernel_name.clone()),
        };

        // 4. Write connection file
        let runtime_dir = runtimelib::dirs::runtime_dir();
        tokio::fs::create_dir_all(&runtime_dir).await.map_err(|e| {
            JupyterError::ConnectionFileError {
                path: runtime_dir.clone(),
                message: e.to_string(),
            }
        })?;

        let connection_file = runtime_dir.join(format!("kernel-{}.json", Uuid::new_v4()));
        let connection_json = serde_json::to_string(&connection_info)?;
        tokio::fs::write(&connection_file, &connection_json)
            .await
            .map_err(|e| JupyterError::ConnectionFileError {
                path: connection_file.clone(),
                message: e.to_string(),
            })?;

        // 5. Spawn kernel process
        let process = kernel
            .spec
            .clone()
            .command(&connection_file, None, None)
            .map_err(|e| JupyterError::ProcessSpawnError {
                kernel: key.kernel_name.clone(),
                message: e.to_string(),
            })?
            .current_dir(&key.working_dir)
            .envs(extra_env.iter().map(|(k, v)| (k, v)))
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .stdin(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| JupyterError::ProcessSpawnError {
                kernel: key.kernel_name.clone(),
                message: e.to_string(),
            })?;

        // 6. Create ZeroMQ socket connections
        let session_id = Uuid::new_v4().to_string();

        let peer_identity = runtimelib::peer_identity_for_session(&session_id).map_err(|e| {
            JupyterError::SocketError {
                socket_type: "shell".to_string(),
                message: e.to_string(),
            }
        })?;
        let shell_socket = runtimelib::create_client_shell_connection_with_identity(
            &connection_info,
            &session_id,
            peer_identity,
        )
        .await
        .map_err(|e| JupyterError::SocketError {
            socket_type: "shell".to_string(),
            message: e.to_string(),
        })?;

        let iopub_socket =
            runtimelib::create_client_iopub_connection(&connection_info, "", &session_id)
                .await
                .map_err(|e| JupyterError::SocketError {
                    socket_type: "iopub".to_string(),
                    message: e.to_string(),
                })?;

        // 7. Create session
        let session = KernelSession {
            kernel,
            process,
            connection_info,
            connection_file,
            shell_socket,
            iopub_socket,
            session_id,
            execution_count: 0,
            working_dir: key.working_dir.clone(),
        };

        // 8. Store session
        {
            let mut sessions = self.sessions.write().await;
            sessions.insert(key.clone(), session);
        }

        // 9. Wait for kernel to become ready
        {
            let mut sessions = self.sessions.write().await;
            if let Some(session) = sessions.get_mut(key) {
                match session.wait_for_ready().await {
                    Ok(info) => {
                        tracing::info!(
                            kernel = %key.kernel_name,
                            language = %info.language,
                            "Kernel is ready"
                        );
                    }
                    Err(e) => {
                        // If kernel fails to become ready, remove the session
                        tracing::error!(kernel = %key.kernel_name, error = %e, "Kernel failed to start");
                        if let Some(mut session) = sessions.remove(key) {
                            let _ = session.shutdown().await;
                        }
                        return Err(e);
                    }
                }
            }
        }

        tracing::info!(kernel = %key.kernel_name, "Kernel started successfully");

        Ok(())
    }

    /// Get mutable access to a session.
    ///
    /// Returns None if no session exists for the key.
    pub async fn with_session<F, R>(&self, key: &SessionKey, f: F) -> Option<R>
    where
        F: FnOnce(&mut KernelSession) -> R,
    {
        let mut sessions = self.sessions.write().await;
        sessions.get_mut(key).map(f)
    }

    /// Execute code in a kernel session.
    ///
    /// This is a convenience method that handles the async execution properly.
    /// Returns None if no session exists for the key.
    pub async fn execute_in_session(
        &self,
        key: &SessionKey,
        code: &str,
    ) -> Option<Result<super::execute::ExecuteResult>> {
        let mut sessions = self.sessions.write().await;
        if let Some(session) = sessions.get_mut(key) {
            Some(session.execute(code).await)
        } else {
            None
        }
    }

    /// Shutdown a specific kernel session.
    pub async fn shutdown_session(&self, key: &SessionKey) -> Result<()> {
        let mut sessions = self.sessions.write().await;
        if let Some(mut session) = sessions.remove(key) {
            tracing::info!(kernel = %key.kernel_name, "Shutting down kernel");
            session.shutdown().await?;
        }
        Ok(())
    }

    /// Shutdown all kernel sessions.
    pub async fn shutdown_all(&self) -> Result<()> {
        let mut sessions = self.sessions.write().await;
        for (key, mut session) in sessions.drain() {
            tracing::info!(kernel = %key.kernel_name, "Shutting down kernel");
            let _ = session.shutdown().await;
        }
        Ok(())
    }

    /// Shutdown all kernel sessions without needing an async context.
    ///
    /// This is the teardown path of the last [`KernelScope`] (which
    /// runs in `Drop`, hence sync). `try_write` cannot fail there in
    /// practice — a held write lock would mean a kernel is mid-execute,
    /// and scopes outlive the executions they cover — but if it ever
    /// does, warn rather than block or panic; the per-`Child`
    /// `kill_on_drop` remains as the final backstop.
    pub fn shutdown_all_blocking(&self) {
        let Ok(mut sessions) = self.sessions.try_write() else {
            tracing::warn!(
                "kernel sessions locked during scope teardown; \
                 skipping shutdown (kill-on-drop remains as backstop)"
            );
            return;
        };
        for (key, mut session) in sessions.drain() {
            tracing::info!(kernel = %key.kernel_name, "Shutting down kernel");
            session.shutdown_blocking();
        }
    }

    /// Get the number of active sessions.
    pub async fn session_count(&self) -> usize {
        self.sessions.read().await.len()
    }

    /// Check if a session exists for the given key.
    pub async fn has_session(&self, key: &SessionKey) -> bool {
        self.sessions.read().await.contains_key(key)
    }
}

impl Default for JupyterDaemon {
    fn default() -> Self {
        Self::new()
    }
}

/// Global daemon instance for kernel management.
///
/// Using a lazy static ensures the daemon persists across multiple renders.
static DAEMON: std::sync::OnceLock<Arc<JupyterDaemon>> = std::sync::OnceLock::new();

/// Get the global daemon instance.
pub fn daemon() -> Arc<JupyterDaemon> {
    DAEMON
        .get_or_init(|| Arc::new(JupyterDaemon::new()))
        .clone()
}

/// Long-lived runtime that owns every kernel session's resources.
///
/// Kernel ZeroMQ sockets and the kernel `Child` are tokio resources
/// bound to the runtime they were created on. Sessions outlive a
/// single engine run (that's the point of the daemon), so they must
/// not be created on a per-call runtime: reusing a session whose
/// runtime was dropped fails with "A Tokio 1.x context was found, but
/// it is being shutdown". Before bd-hxhnnlzs this was masked by the
/// startup race — every "reuse" actually re-spawned a fresh kernel and
/// evicted (killed) the old one.
///
/// One worker thread is plenty: engine executions serialize on the
/// sessions lock anyway. Being a static, the runtime is never dropped;
/// kernels are shut down by the [`KernelScope`] machinery, not by
/// runtime teardown.
static ENGINE_RUNTIME: std::sync::OnceLock<tokio::runtime::Runtime> = std::sync::OnceLock::new();

/// The shared engine runtime. All daemon session operations —
/// spawning, executing, async shutdown — must run on it.
pub(crate) fn engine_runtime() -> &'static tokio::runtime::Runtime {
    ENGINE_RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .thread_name("jupyter-daemon")
            .enable_all()
            .build()
            .expect("building the shared Jupyter engine runtime")
    })
}

/// Number of currently-open [`KernelScope`]s.
static ACTIVE_SCOPES: AtomicUsize = AtomicUsize::new(0);

fn active_scopes() -> usize {
    ACTIVE_SCOPES.load(Ordering::SeqCst)
}

/// RAII handle keeping the global kernel pool alive (bd-hxhnnlzs).
///
/// When the last open scope drops, every session in the global daemon
/// is shut down ([`JupyterDaemon::shutdown_all_blocking`]). The
/// jupyter engine acquires one around each engine run, so kernels
/// never outlive the work that spawned them; callers that want kernel
/// reuse *across* engine runs (a project render, a preview server)
/// hold an additional scope for their own lifetime.
///
/// Scope drops must happen on the normal return path — `Drop` does not
/// run across `std::process::exit`, so hold scopes tightly around the
/// work rather than around exit-calling code.
#[must_use = "the kernel pool shuts down when the last scope drops; \
              bind to a named guard for the intended lifetime"]
pub struct KernelScope {
    _not_send_sync_constructible: (),
}

/// Open a [`KernelScope`].
pub fn kernel_scope() -> KernelScope {
    ACTIVE_SCOPES.fetch_add(1, Ordering::SeqCst);
    KernelScope {
        _not_send_sync_constructible: (),
    }
}

impl Drop for KernelScope {
    fn drop(&mut self) {
        let was = ACTIVE_SCOPES.fetch_sub(1, Ordering::SeqCst);
        debug_assert!(was > 0, "KernelScope refcount underflow");
        // Last scope out shuts down the pool. A concurrent
        // `kernel_scope()` call racing this transition could get its
        // fresh kernel torn down; in practice every spawning path runs
        // under a scope that is still open here, and the cost of the
        // race is a kernel restart, not a leak.
        if was == 1
            && let Some(daemon) = DAEMON.get()
        {
            daemon.shutdown_all_blocking();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_daemon_initial_state() {
        let daemon = JupyterDaemon::new();
        assert_eq!(daemon.session_count().await, 0);
    }

    #[test]
    fn test_kernel_scope_refcount() {
        // nextest runs each test in its own process, so the global
        // counter starts at 0 and nothing else touches it.
        assert_eq!(active_scopes(), 0);
        let outer = kernel_scope();
        {
            let _inner = kernel_scope();
            assert_eq!(active_scopes(), 2);
        }
        // Inner drop must not tear down while an outer scope is open.
        assert_eq!(active_scopes(), 1);
        drop(outer);
        assert_eq!(active_scopes(), 0);
    }
}
