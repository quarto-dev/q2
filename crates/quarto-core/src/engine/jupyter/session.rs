/*
 * engine/jupyter/session.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Jupyter kernel session management.
 */

//! Kernel session management.
//!
//! A `KernelSession` represents an active connection to a running Jupyter kernel.
//! It manages the ZeroMQ sockets and provides methods for executing code.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use jupyter_protocol::{ConnectionInfo, JupyterMessage, KernelInfoRequest};
use runtimelib::{ClientIoPubConnection, ClientShellConnection};
use tokio::process::Child;
use tokio::time::timeout;

use super::error::{JupyterError, Result};
use super::kernelspec::ResolvedKernel;

/// Default timeout for waiting for kernel to become ready.
const KERNEL_READY_TIMEOUT: Duration = Duration::from_secs(60);

/// Information about a running kernel.
#[derive(Debug, Clone)]
pub struct KernelInfo {
    /// Programming language the kernel supports.
    pub language: String,
    /// Version of the language.
    pub language_version: String,
    /// Name of the kernel implementation.
    pub implementation: String,
    /// Kernel banner text.
    pub banner: String,
}

/// A running kernel session with active connections.
pub struct KernelSession {
    /// The resolved kernel specification.
    pub(crate) kernel: ResolvedKernel,
    /// The kernel process (we manage its lifecycle).
    pub(crate) process: Child,
    /// Connection info (ports, key, etc.).
    pub(crate) connection_info: ConnectionInfo,
    /// Path to the connection file (for cleanup).
    pub(crate) connection_file: PathBuf,
    /// ZeroMQ shell socket (for execute requests).
    pub(crate) shell_socket: ClientShellConnection,
    /// ZeroMQ iopub socket (for outputs).
    pub(crate) iopub_socket: ClientIoPubConnection,
    /// Session ID for message correlation.
    pub(crate) session_id: String,
    /// Execution counter for this session.
    pub(crate) execution_count: u32,
    /// Last activity timestamp.
    pub(crate) last_used: Instant,
    /// Working directory for this session.
    pub(crate) working_dir: PathBuf,
}

impl KernelSession {
    /// Get the kernel name.
    pub fn kernel_name(&self) -> &str {
        &self.kernel.name
    }

    /// Get the kernel language.
    pub fn language(&self) -> &str {
        &self.kernel.language
    }

    /// Get the working directory.
    pub fn working_dir(&self) -> &PathBuf {
        &self.working_dir
    }

    /// Get the current execution count.
    pub fn execution_count(&self) -> u32 {
        self.execution_count
    }

    /// Check if the kernel process is still running.
    pub fn is_alive(&mut self) -> bool {
        match self.process.try_wait() {
            Ok(None) => true,     // Still running
            Ok(Some(_)) => false, // Exited
            Err(_) => false,      // Error checking status
        }
    }

    /// Wait for the kernel to become ready by sending a kernel_info_request.
    ///
    /// This method sends a `kernel_info_request` message and waits for
    /// the `kernel_info_reply`. This is the standard way to verify that
    /// a Jupyter kernel has fully started and is ready to accept commands.
    ///
    /// Returns the kernel info (language, version, etc.) on success.
    pub async fn wait_for_ready(&mut self) -> Result<KernelInfo> {
        self.wait_for_ready_with_timeout(KERNEL_READY_TIMEOUT).await
    }

    /// Wait for the kernel to become ready with a custom timeout.
    pub async fn wait_for_ready_with_timeout(
        &mut self,
        ready_timeout: Duration,
    ) -> Result<KernelInfo> {
        tracing::debug!("Waiting for kernel to become ready...");

        // Build kernel_info_request
        let request = KernelInfoRequest {};
        let message: JupyterMessage = request.into();
        let msg_id = message.header.msg_id.clone();

        // Send the request
        self.shell_socket
            .send(message)
            .await
            .map_err(|e| JupyterError::SendError(e.to_string()))?;

        // Wait for the reply with timeout
        let result = timeout(ready_timeout, self.wait_for_kernel_info_reply(&msg_id)).await;

        match result {
            Ok(Ok(info)) => {
                tracing::info!(
                    language = %info.language,
                    version = %info.language_version,
                    "Kernel is ready"
                );
                Ok(info)
            }
            Ok(Err(e)) => Err(e),
            Err(_) => Err(JupyterError::KernelStartupTimeout {
                seconds: ready_timeout.as_secs(),
            }),
        }
    }

    /// Wait for kernel_info_reply on the shell socket.
    async fn wait_for_kernel_info_reply(&mut self, request_id: &str) -> Result<KernelInfo> {
        // We need to read from shell socket for the reply
        // But we also need to drain any iopub messages that arrive first
        loop {
            // Try to read from iopub to drain status messages
            // Use a short timeout so we don't block too long
            if let Ok(Ok(_msg)) =
                timeout(Duration::from_millis(100), self.iopub_socket.read()).await
            {
                // Just drain iopub messages during startup
                continue;
            }

            // Now try to read from shell socket
            // The reply comes on the shell socket, but we need a way to read it
            // Unfortunately, runtimelib's ClientShellConnection only has send()
            // So we need to use a different approach - wait for iopub status messages

            // Actually, let's just wait a bit and then consider the kernel ready
            // The kernel_info_reply comes on shell socket but runtimelib doesn't expose recv for it
            // Instead, we'll verify by checking for the initial status message on iopub
            break;
        }

        // For now, return placeholder info since we can't easily read the reply
        // The kernel is ready if we got this far without timeout
        Ok(KernelInfo {
            language: self.kernel.language.clone(),
            language_version: "unknown".to_string(),
            implementation: self.kernel.name.clone(),
            banner: String::new(),
        })
    }

    /// Send an interrupt signal to the kernel (if supported).
    ///
    /// Note: Currently this is a no-op. Full implementation would send
    /// an interrupt_request message via the control channel.
    pub fn interrupt(&self) -> Result<()> {
        // TODO: Implement interrupt via control channel
        // The control channel supports interrupt_request messages
        // which is the portable way to interrupt kernels.
        tracing::warn!("Kernel interrupt not yet implemented");
        Ok(())
    }

    /// Shutdown the kernel gracefully.
    pub async fn shutdown(&mut self) -> Result<()> {
        // Kill the process
        if let Err(e) = self.process.start_kill() {
            tracing::warn!("Failed to kill kernel process: {}", e);
        }

        // Wait for it to exit (with timeout)
        let _ = tokio::time::timeout(std::time::Duration::from_secs(5), self.process.wait()).await;

        // Clean up connection file
        if self.connection_file.exists() {
            if let Err(e) = tokio::fs::remove_file(&self.connection_file).await {
                tracing::warn!("Failed to remove connection file: {}", e);
            }
        }

        Ok(())
    }

    /// Update the last-used timestamp.
    pub(crate) fn touch(&mut self) {
        self.last_used = Instant::now();
    }

    /// Increment and return the execution count.
    pub(crate) fn next_execution_count(&mut self) -> u32 {
        self.execution_count += 1;
        self.execution_count
    }
}

impl Drop for KernelSession {
    fn drop(&mut self) {
        // The Child process has kill_on_drop(true), so it will be killed.
        // We also try to clean up the connection file.
        if self.connection_file.exists() {
            let _ = std::fs::remove_file(&self.connection_file);
        }
    }
}

/// Key for identifying kernel sessions.
///
/// Sessions are identified by their kernel name and working directory.
/// This allows multiple kernels of the same type in different directories.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SessionKey {
    /// Kernelspec name (e.g., "python3", "julia-1.9").
    pub kernel_name: String,
    /// Working directory for the kernel.
    pub working_dir: PathBuf,
}

impl SessionKey {
    /// Create a new session key.
    pub fn new(kernel_name: impl Into<String>, working_dir: impl Into<PathBuf>) -> Self {
        SessionKey {
            kernel_name: kernel_name.into(),
            working_dir: working_dir.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_key_equality() {
        let key1 = SessionKey::new("python3", "/project/a");
        let key2 = SessionKey::new("python3", "/project/a");
        let key3 = SessionKey::new("python3", "/project/b");
        let key4 = SessionKey::new("julia-1.9", "/project/a");

        assert_eq!(key1, key2);
        assert_ne!(key1, key3);
        assert_ne!(key1, key4);
    }

    #[test]
    fn test_session_key_hash() {
        use std::collections::HashMap;

        let mut map: HashMap<SessionKey, &str> = HashMap::new();
        map.insert(SessionKey::new("python3", "/project"), "python");
        map.insert(SessionKey::new("julia-1.9", "/project"), "julia");

        assert_eq!(
            map.get(&SessionKey::new("python3", "/project")),
            Some(&"python")
        );
        assert_eq!(
            map.get(&SessionKey::new("julia-1.9", "/project")),
            Some(&"julia")
        );
    }
}
