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
//! - Shutting down idle kernels
//! - Cleaning up on shutdown

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use jupyter_protocol::ConnectionInfo;
use tokio::sync::RwLock;
use uuid::Uuid;

use super::error::{JupyterError, Result};
use super::kernelspec;
use super::session::{KernelSession, SessionKey};

/// Default timeout before idle kernels are shut down.
const DEFAULT_IDLE_TIMEOUT: Duration = Duration::from_secs(300); // 5 minutes

/// In-process daemon managing Jupyter kernel sessions.
///
/// The daemon maintains a pool of running kernels, indexed by
/// (kernel_name, working_dir). This allows kernel reuse across
/// multiple render operations on documents in the same directory.
pub struct JupyterDaemon {
    /// Active kernel sessions.
    sessions: RwLock<HashMap<SessionKey, KernelSession>>,
    /// Idle timeout before shutting down unused kernels.
    idle_timeout: Duration,
}

impl JupyterDaemon {
    /// Create a new daemon with default settings.
    pub fn new() -> Self {
        Self {
            sessions: RwLock::new(HashMap::new()),
            idle_timeout: DEFAULT_IDLE_TIMEOUT,
        }
    }

    /// Create a daemon with custom idle timeout.
    pub fn with_idle_timeout(idle_timeout: Duration) -> Self {
        Self {
            sessions: RwLock::new(HashMap::new()),
            idle_timeout,
        }
    }

    /// Get or start a kernel session for the given key.
    ///
    /// If a session already exists for this (kernel, working_dir) pair,
    /// it is reused. Otherwise, a new kernel is started.
    pub async fn get_or_start_session(
        &self,
        kernel_name: &str,
        working_dir: &PathBuf,
    ) -> Result<SessionKey> {
        let key = SessionKey::new(kernel_name, working_dir.clone());

        // Check if we have an existing session
        {
            let sessions = self.sessions.read().await;
            if sessions.contains_key(&key) {
                return Ok(key);
            }
        }

        // Start a new kernel
        self.start_kernel(&key).await?;

        Ok(key)
    }

    /// Start a new kernel for the given key.
    async fn start_kernel(&self, key: &SessionKey) -> Result<()> {
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

        let shell_socket =
            runtimelib::create_client_shell_connection(&connection_info, &session_id)
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
            last_used: Instant::now(),
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
        sessions.get_mut(key).map(|session| {
            session.touch();
            f(session)
        })
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
            session.touch();
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

    /// Cleanup idle sessions that have exceeded the timeout.
    pub async fn cleanup_idle_sessions(&self) {
        let now = Instant::now();
        let mut sessions = self.sessions.write().await;

        let idle_keys: Vec<SessionKey> = sessions
            .iter()
            .filter(|(_, session)| now.duration_since(session.last_used) > self.idle_timeout)
            .map(|(key, _)| key.clone())
            .collect();

        for key in idle_keys {
            if let Some(mut session) = sessions.remove(&key) {
                tracing::info!(kernel = %key.kernel_name, "Shutting down idle kernel");
                let _ = session.shutdown().await;
            }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_daemon_creation() {
        let daemon = JupyterDaemon::new();
        assert_eq!(daemon.idle_timeout, DEFAULT_IDLE_TIMEOUT);
    }

    #[test]
    fn test_daemon_custom_timeout() {
        let timeout = Duration::from_secs(60);
        let daemon = JupyterDaemon::with_idle_timeout(timeout);
        assert_eq!(daemon.idle_timeout, timeout);
    }

    #[tokio::test]
    async fn test_daemon_initial_state() {
        let daemon = JupyterDaemon::new();
        assert_eq!(daemon.session_count().await, 0);
    }
}
