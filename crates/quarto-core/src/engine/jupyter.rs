/*
 * engine/jupyter.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Jupyter engine for Python/Julia code execution.
 */

//! Jupyter engine for Python/Julia code execution.
//!
//! This engine executes code cells using Jupyter kernels. It communicates
//! with kernels to run Python, Julia, or other supported languages.
//!
//! # Availability
//!
//! This engine is only available in native builds (not WASM).
//! It requires jupyter to be installed.
//!
//! # Current Status
//!
//! This is a placeholder implementation. The actual Jupyter integration
//! will be implemented in a future phase.

#![cfg(not(target_arch = "wasm32"))]

use std::path::{Path, PathBuf};

use super::context::{ExecuteResult, ExecutionContext};
use super::error::ExecutionError;
use super::traits::ExecutionEngine;

/// Jupyter engine for Python/Julia code execution.
///
/// This engine communicates with Jupyter kernels to execute code cells.
///
/// # Requirements
///
/// - Jupyter must be installed (`jupyter` command accessible)
/// - Appropriate kernel for the language (e.g., `ipykernel` for Python)
///
/// # Supported Languages
///
/// - Python (via ipykernel)
/// - Julia (via IJulia)
/// - Other Jupyter-compatible kernels
pub struct JupyterEngine {
    /// Path to jupyter executable
    jupyter_path: Option<PathBuf>,
}

impl JupyterEngine {
    /// Create a new jupyter engine, attempting to find jupyter.
    pub fn new() -> Self {
        Self {
            jupyter_path: Self::find_jupyter(),
        }
    }

    /// Try to find jupyter executable on the system.
    ///
    /// Uses `command -v` (Unix) or `where` (Windows) to locate the executable.
    fn find_jupyter() -> Option<PathBuf> {
        Self::find_executable("jupyter")
    }

    /// Find an executable in PATH.
    fn find_executable(name: &str) -> Option<PathBuf> {
        #[cfg(unix)]
        {
            use std::process::Command;
            let output = Command::new("sh")
                .args(["-c", &format!("command -v {}", name)])
                .output()
                .ok()?;

            if output.status.success() {
                let path = String::from_utf8_lossy(&output.stdout);
                let path = path.trim();
                if !path.is_empty() {
                    return Some(PathBuf::from(path));
                }
            }
        }

        #[cfg(windows)]
        {
            use std::process::Command;
            let output = Command::new("where").arg(name).output().ok()?;

            if output.status.success() {
                let path = String::from_utf8_lossy(&output.stdout);
                // `where` returns multiple lines; take the first
                if let Some(first_line) = path.lines().next() {
                    let path = first_line.trim();
                    if !path.is_empty() {
                        return Some(PathBuf::from(path));
                    }
                }
            }
        }

        None
    }

    /// Get the path to jupyter, if found.
    pub fn jupyter_path(&self) -> Option<&Path> {
        self.jupyter_path.as_deref()
    }
}

impl Default for JupyterEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl ExecutionEngine for JupyterEngine {
    fn name(&self) -> &str {
        "jupyter"
    }

    fn execute(
        &self,
        _input: &str,
        _ctx: &ExecutionContext,
    ) -> Result<ExecuteResult, ExecutionError> {
        // Check if jupyter is available
        if self.jupyter_path.is_none() {
            return Err(ExecutionError::runtime_not_found("jupyter", "jupyter"));
        }

        // TODO: Implement actual jupyter execution
        // For now, return a "not implemented" error
        Err(ExecutionError::not_available(
            "jupyter engine execution not yet implemented",
        ))
    }

    fn can_freeze(&self) -> bool {
        true
    }

    fn is_available(&self) -> bool {
        self.jupyter_path.is_some()
    }

    fn intermediate_files(&self, input_path: &Path) -> Vec<PathBuf> {
        // Jupyter may produce {input}_files/ directory for outputs
        let stem = input_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("");

        if stem.is_empty() {
            return Vec::new();
        }

        let parent = input_path.parent().unwrap_or(Path::new("."));
        let files_dir = parent.join(format!("{}_files", stem));

        vec![files_dir]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_jupyter_engine_name() {
        let engine = JupyterEngine::new();
        assert_eq!(engine.name(), "jupyter");
    }

    #[test]
    fn test_jupyter_engine_can_freeze() {
        let engine = JupyterEngine::new();
        assert!(engine.can_freeze());
    }

    #[test]
    fn test_jupyter_engine_intermediate_files() {
        let engine = JupyterEngine::new();

        let files = engine.intermediate_files(Path::new("/project/notebook.qmd"));
        assert_eq!(files.len(), 1);
        assert_eq!(files[0], PathBuf::from("/project/notebook_files"));
    }

    #[test]
    fn test_jupyter_engine_is_available_depends_on_jupyter() {
        let engine = JupyterEngine::new();
        // Availability depends on whether jupyter is installed
        // We just verify it doesn't panic
        let _ = engine.is_available();
    }

    #[test]
    fn test_jupyter_engine_execute_requires_jupyter() {
        // Create engine with no jupyter path to test the error case
        let engine = JupyterEngine { jupyter_path: None };

        let ctx = ExecutionContext::new(
            PathBuf::from("/tmp"),
            PathBuf::from("/project"),
            PathBuf::from("/project/doc.qmd"),
            "html",
        );

        let result = engine.execute("# Test", &ctx);
        assert!(result.is_err());

        let err = result.unwrap_err();
        let msg = format!("{}", err);
        assert!(msg.contains("jupyter"));
    }

    #[test]
    fn test_jupyter_engine_default() {
        let engine = JupyterEngine::default();
        assert_eq!(engine.name(), "jupyter");
    }

    #[test]
    fn test_jupyter_engine_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<JupyterEngine>();
    }
}
