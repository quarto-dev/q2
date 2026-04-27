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

    /// A request exceeded its per-request timeout `window`. **Distinct from a
    /// user `Cancelled`** — the TS-engine host's poison policy keys on this
    /// specific variant (a timeout poisons the engine instance; a plain
    /// `ExecutionFailed` does not). See plan1a-host "Timeout/cancel."
    #[error("engine '{engine}' timed out during {operation}")]
    Timeout {
        /// The engine that timed out.
        engine: String,
        /// The operation that exceeded its window (e.g. "execute", "loadEngine").
        operation: String,
    },

    /// The engine-host subprocess exited unexpectedly (the protocol channel
    /// closed). Delivered to **every** in-flight request when the demux reader
    /// thread hits EOF. No Q1 equivalent (in-process engines can't crash
    /// independently); mirrors `jupyter::error::ProcessExited`, plus the captured
    /// stderr tail. See plan1a-host error category 6.
    #[error(
        "engine host subprocess crashed while serving '{engine}' (exit code: {code:?})\n{stderr}"
    )]
    ProcessCrashed {
        /// The engine whose in-flight request observed the crash.
        engine: String,
        /// Reaped process exit code; `None` on signal death (SIGKILL / segfault /
        /// OOM-kill / our own teardown SIGKILL all give `ExitStatus::code() ==
        /// None`).
        code: Option<i32>,
        /// Tail of the child's stderr (from the bounded ring) — the actionable
        /// part of the diagnostic.
        stderr: String,
    },

    /// An operation is not supported by this engine. Used by trait method defaults
    /// to signal "this engine doesn't implement X."
    #[error("operation not supported: {0}")]
    NotSupported(&'static str),

    /// The engine owns the language (via resolution) but has no handler for it.
    /// E.g. jupyter resolved as owner of `{sql}` but has no SQL kernel.
    /// This is a clean refusal — it does **not** poison the instance (only
    /// `Cancelled | Timeout` do). See plan1a-engine §10 case 4.
    #[error("engine `{engine}` has no handler for language `{language}`")]
    NoHandlerForLanguage {
        /// The engine that was asked to run the language.
        engine: String,
        /// The language the engine cannot handle.
        language: String,
    },

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

    /// Create a "timeout" error naming the engine and the operation that
    /// exceeded its window.
    pub fn timeout(engine: impl Into<String>, operation: impl Into<String>) -> Self {
        Self::Timeout {
            engine: engine.into(),
            operation: operation.into(),
        }
    }

    /// Create a "process crashed" error with the reaped exit code and stderr tail.
    pub fn process_crashed(
        engine: impl Into<String>,
        code: Option<i32>,
        stderr: impl Into<String>,
    ) -> Self {
        Self::ProcessCrashed {
            engine: engine.into(),
            code,
            stderr: stderr.into(),
        }
    }

    /// Create a "not supported" error for operations the engine does not implement.
    pub fn not_supported(msg: &'static str) -> Self {
        Self::NotSupported(msg)
    }

    /// Create a "no handler for language" error naming both the engine and the
    /// language it was asked to execute but cannot.
    pub fn no_handler_for_language(engine: impl Into<String>, language: impl Into<String>) -> Self {
        Self::NoHandlerForLanguage {
            engine: engine.into(),
            language: language.into(),
        }
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
        assert!(msg.contains('R'));
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

    #[test]
    fn test_timeout_error_distinct_from_cancelled() {
        let err = ExecutionError::timeout("julia", "execute");
        // The poison policy keys on the *specific* Timeout variant — it must not
        // collapse into Cancelled or ExecutionFailed.
        assert!(matches!(err, ExecutionError::Timeout { .. }));
        assert!(!matches!(err, ExecutionError::Cancelled));
        let msg = format!("{}", err);
        assert!(msg.contains("julia"));
        assert!(msg.contains("execute"));
    }

    #[test]
    fn test_process_crashed_error_carries_code_and_stderr() {
        let err = ExecutionError::process_crashed("julia", None, "panic: boom");
        assert!(matches!(
            err,
            ExecutionError::ProcessCrashed { code: None, .. }
        ));
        let msg = format!("{}", err);
        assert!(msg.contains("julia"));
        assert!(msg.contains("crashed"));
        assert!(msg.contains("panic: boom"));
    }

    #[test]
    fn test_not_supported_error() {
        let err = ExecutionError::not_supported("markdown_for_file");
        assert!(matches!(
            err,
            ExecutionError::NotSupported("markdown_for_file")
        ));
        let msg = format!("{}", err);
        assert!(msg.contains("not supported"));
        assert!(msg.contains("markdown_for_file"));
    }

    #[test]
    fn test_no_handler_for_language_error() {
        let err = ExecutionError::no_handler_for_language("jupyter", "sql");
        assert!(matches!(err, ExecutionError::NoHandlerForLanguage { .. }));
        // Must NOT be a poison variant (Cancelled | Timeout).
        assert!(!matches!(err, ExecutionError::Cancelled));
        assert!(!matches!(err, ExecutionError::Timeout { .. }));
        let msg = format!("{}", err);
        assert!(msg.contains("jupyter"), "message should name the engine");
        assert!(msg.contains("sql"), "message should name the language");
    }

    #[test]
    fn test_execution_error_is_send() {
        // ProcessCrashed/Timeout cross the demux slot channel, so the whole enum
        // must stay Send. Compile-time guard.
        fn assert_send<T: Send>() {}
        assert_send::<ExecutionError>();
    }
}
