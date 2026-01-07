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
    #[error("kernelspec '{name}' not found")]
    KernelspecNotFound { name: String },

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
        JupyterError::RuntimeLibError(err.to_string())
    }
}

/// Result type for Jupyter operations.
pub type Result<T> = std::result::Result<T, JupyterError>;
