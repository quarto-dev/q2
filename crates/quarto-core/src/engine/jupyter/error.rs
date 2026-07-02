/*
 * engine/jupyter/error.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Jupyter engine error types.
 */

//! Error types specific to Jupyter kernel communication.

use std::path::PathBuf;
use thiserror::Error;

/// Errors that can occur during Jupyter kernel operations.
#[derive(Debug, Error)]
pub enum JupyterError {
    /// Kernel specification not found.
    ///
    /// Carries the directories that were searched and the kernel
    /// names that were discoverable in those directories so the
    /// `Display` impl can produce an actionable diagnostic.
    #[error("{}", format_kernelspec_not_found(name, searched, available))]
    KernelspecNotFound {
        name: String,
        searched: Vec<PathBuf>,
        available: Vec<String>,
    },

    /// No kernelspec matches the requested language.
    #[error("no kernel found for language '{language}'")]
    NoKernelForLanguage { language: String },

    /// Failed to allocate ports for kernel communication.
    #[error("failed to allocate ports: {0}")]
    PortAllocationFailed(String),

    /// Failed to write connection file.
    #[error("failed to write connection file to {path}: {message}")]
    ConnectionFileError { path: PathBuf, message: String },

    /// Failed to spawn kernel process.
    #[error("failed to spawn kernel process for '{kernel}': {message}")]
    ProcessSpawnError { kernel: String, message: String },

    /// Kernel process exited unexpectedly.
    #[error("kernel process exited unexpectedly with code {code:?}")]
    ProcessExited { code: Option<i32> },

    /// Failed to create ZeroMQ socket.
    #[error("failed to create {socket_type} socket: {message}")]
    SocketError {
        socket_type: String,
        message: String,
    },

    /// Timeout waiting for kernel to become ready.
    #[error("timeout waiting for kernel to become ready after {seconds}s")]
    KernelStartupTimeout { seconds: u64 },

    /// Error during code execution.
    #[error("execution error: {ename}: {evalue}")]
    ExecutionError {
        ename: String,
        evalue: String,
        traceback: Vec<String>,
    },

    /// A code cell raised an error and error output is not allowed —
    /// no `#| error: true` on the cell and no `execute: error: true`
    /// on the document (bd-ohvl879u, matching knitr/Q1 policy).
    #[error("{message}")]
    CellExecutionFailed { message: String },

    /// A cell's `#|` option lines are not valid YAML (bd-ohvl879u).
    #[error("{message}")]
    InvalidCellOptions { message: String },

    /// Error receiving message from kernel.
    #[error("failed to receive message: {0}")]
    ReceiveError(String),

    /// Error sending message to kernel.
    #[error("failed to send message: {0}")]
    SendError(String),

    /// Unexpected message type received.
    #[error("unexpected message type: expected {expected}, got {actual}")]
    UnexpectedMessageType { expected: String, actual: String },

    /// Kernel is not connected.
    #[error("kernel is not connected")]
    NotConnected,

    /// JSON serialization/deserialization error.
    #[error("JSON error: {0}")]
    JsonError(#[from] serde_json::Error),

    /// IO error.
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    /// Runtime library error.
    #[error("runtimelib error: {0}")]
    RuntimeLibError(String),
}

impl From<runtimelib::RuntimeError> for JupyterError {
    fn from(err: runtimelib::RuntimeError) -> Self {
        match err {
            runtimelib::RuntimeError::KernelNotFound {
                name,
                available,
                searched_paths,
            } => JupyterError::KernelspecNotFound {
                name,
                searched: searched_paths,
                available,
            },
            other => JupyterError::RuntimeLibError(other.to_string()),
        }
    }
}

/// Result type for Jupyter operations.
pub type Result<T> = std::result::Result<T, JupyterError>;

fn format_kernelspec_not_found(name: &str, searched: &[PathBuf], available: &[String]) -> String {
    use std::fmt::Write;
    let mut out = format!("kernelspec '{name}' not found\n\nsearched:\n");
    if searched.is_empty() {
        out.push_str("  (none)\n");
    } else {
        for path in searched {
            let _ = writeln!(out, "  {}", path.display());
        }
    }
    if available.is_empty() {
        out.push_str("\navailable kernels: (none)\n");
    } else {
        out.push_str("\navailable kernels:\n");
        for k in available {
            let _ = writeln!(out, "  {k}");
        }
    }
    out.push_str(
        "\nhint: if `jupyter kernelspec list` shows a kernel we missed, run quarto from the same shell where `jupyter` resolves to the kernel's environment, or set JUPYTER_PATH to include its parent directory.",
    );
    out
}
