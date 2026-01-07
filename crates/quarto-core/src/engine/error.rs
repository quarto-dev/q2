/*
 * engine/error.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Error types for execution engines.
 */

//! Error types for execution engines.

use std::path::PathBuf;
use thiserror::Error;

/// Errors that can occur during engine execution.
#[derive(Debug, Error)]
pub enum ExecutionError {
    /// The requested engine is not available in this build.
    #[error("Engine not available: {0}")]
    NotAvailable(String),

    /// The engine is available but the required runtime (R, Python, etc.) is not installed.
    #[error("Engine runtime not found: {engine} requires {runtime}")]
    RuntimeNotFound {
        /// The engine that requires the runtime
        engine: String,
        /// The runtime that was not found
        runtime: String,
    },

    /// A required package is not installed.
    #[error("Missing package: {package}")]
    MissingPackage {
        /// The engine that requires the package
        engine: String,
        /// The missing package name
        package: String,
        /// Suggested installation command
        suggestion: Option<String>,
    },

    /// A package version is too old.
    #[error("Package version too old: {package} >= {required_version} required")]
    PackageVersionTooOld {
        /// The engine that requires the package
        engine: String,
        /// The package name
        package: String,
        /// The required minimum version
        required_version: String,
        /// Suggested update command
        suggestion: Option<String>,
    },

    /// Code execution failed.
    #[error("Execution failed in {engine}: {message}")]
    ExecutionFailed {
        /// The engine that failed
        engine: String,
        /// Error message from the engine
        message: String,
    },

    /// Code execution failed at specific source lines.
    #[error("Execution failed in {engine} at lines {start_line}-{end_line}: {message}")]
    ExecutionFailedAtLines {
        /// The engine that failed
        engine: String,
        /// Error message from the engine
        message: String,
        /// Start line (1-indexed)
        start_line: usize,
        /// End line (1-indexed, inclusive)
        end_line: usize,
    },

    /// IO error during execution.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// Failed to write/read temporary files.
    #[error("Temporary file error: {message}")]
    TempFile {
        /// Description of what failed
        message: String,
        /// The path involved, if any
        path: Option<PathBuf>,
    },

    /// Execution was cancelled.
    #[error("Execution cancelled")]
    Cancelled,

    /// Engine-specific error with custom message.
    #[error("{0}")]
    Other(String),
}

impl ExecutionError {
    /// Create an "engine not available" error.
    pub fn not_available(engine: impl Into<String>) -> Self {
        Self::NotAvailable(engine.into())
    }

    /// Create a "runtime not found" error.
    pub fn runtime_not_found(engine: impl Into<String>, runtime: impl Into<String>) -> Self {
        Self::RuntimeNotFound {
            engine: engine.into(),
            runtime: runtime.into(),
        }
    }

    /// Create an "execution failed" error.
    pub fn execution_failed(engine: impl Into<String>, message: impl Into<String>) -> Self {
        Self::ExecutionFailed {
            engine: engine.into(),
            message: message.into(),
        }
    }

    /// Create an "execution failed at lines" error.
    pub fn execution_failed_at_lines(
        engine: impl Into<String>,
        message: impl Into<String>,
        start_line: usize,
        end_line: usize,
    ) -> Self {
        Self::ExecutionFailedAtLines {
            engine: engine.into(),
            message: message.into(),
            start_line,
            end_line,
        }
    }

    /// Create a "missing package" error.
    pub fn missing_package(
        engine: impl Into<String>,
        package: impl Into<String>,
        suggestion: Option<String>,
    ) -> Self {
        Self::MissingPackage {
            engine: engine.into(),
            package: package.into(),
            suggestion,
        }
    }

    /// Create a "package version too old" error.
    pub fn package_version_too_old(
        engine: impl Into<String>,
        package: impl Into<String>,
        required_version: impl Into<String>,
        suggestion: Option<String>,
    ) -> Self {
        Self::PackageVersionTooOld {
            engine: engine.into(),
            package: package.into(),
            required_version: required_version.into(),
            suggestion,
        }
    }

    /// Create a "temp file" error.
    pub fn temp_file(message: impl Into<String>, path: Option<PathBuf>) -> Self {
        Self::TempFile {
            message: message.into(),
            path,
        }
    }

    /// Create an "other" error with a custom message.
    pub fn other(message: impl Into<String>) -> Self {
        Self::Other(message.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_not_available_error() {
        let err = ExecutionError::not_available("jupyter");
        assert!(matches!(err, ExecutionError::NotAvailable(_)));
        let msg = format!("{}", err);
        assert!(msg.contains("jupyter"));
        assert!(msg.contains("not available"));
    }

    #[test]
    fn test_runtime_not_found_error() {
        let err = ExecutionError::runtime_not_found("knitr", "R");
        let msg = format!("{}", err);
        assert!(msg.contains("knitr"));
        assert!(msg.contains("R"));
    }

    #[test]
    fn test_execution_failed_error() {
        let err = ExecutionError::execution_failed("jupyter", "Kernel died unexpectedly");
        let msg = format!("{}", err);
        assert!(msg.contains("jupyter"));
        assert!(msg.contains("Kernel died"));
    }

    #[test]
    fn test_temp_file_error() {
        let err =
            ExecutionError::temp_file("Failed to write", Some(PathBuf::from("/tmp/test.qmd")));
        let msg = format!("{}", err);
        assert!(msg.contains("Failed to write"));
    }

    #[test]
    fn test_io_error_conversion() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let err: ExecutionError = io_err.into();
        assert!(matches!(err, ExecutionError::Io(_)));
    }

    #[test]
    fn test_other_error() {
        let err = ExecutionError::other("Something unexpected happened");
        let msg = format!("{}", err);
        assert!(msg.contains("Something unexpected"));
    }

    #[test]
    fn test_cancelled_error() {
        let err = ExecutionError::Cancelled;
        let msg = format!("{}", err);
        assert!(msg.contains("cancelled"));
    }
}
