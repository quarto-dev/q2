/*
 * presenter/output.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Output types the presenter consumes; no kernel or async-runtime dependencies.
 */

//! The shapes of a cell's execution result: [`CellResult`], [`ExecuteStatus`],
//! [`CellOutput`] and [`MimeBundle`].
//!
//! Defined here, not beside the kernel session code, so they compile on `wasm32`.
//! The conversion from `jupyter_protocol::Media` lives with the kernel code.

/// Result of executing one cell: what the presenter renders.
///
/// Named `CellResult` so it cannot collide with
/// [`crate::engine::context::ExecuteResult`] (the engine-level result).
#[derive(Debug, Clone)]
pub struct CellResult {
    /// Execution status.
    pub status: ExecuteStatus,
    /// Collected outputs.
    pub outputs: Vec<CellOutput>,
    /// Execution count from the kernel.
    pub execution_count: Option<u32>,
}

/// Execution status.
#[derive(Debug, Clone)]
pub enum ExecuteStatus {
    /// Execution completed successfully.
    Ok,
    /// Execution failed with an error.
    Error {
        ename: String,
        evalue: String,
        traceback: Vec<String>,
    },
    /// Execution was aborted.
    Aborted,
}

/// Output from a code cell.
#[derive(Debug, Clone)]
pub enum CellOutput {
    /// Stream output (stdout/stderr).
    Stream { name: String, text: String },
    /// Rich display data (images, HTML, etc.).
    DisplayData {
        data: MimeBundle,
        metadata: serde_json::Value,
    },
    /// Execute result (the value of the last expression).
    ExecuteResult {
        execution_count: u32,
        data: MimeBundle,
        metadata: serde_json::Value,
    },
    /// Error output.
    Error {
        ename: String,
        evalue: String,
        traceback: Vec<String>,
    },
}

/// MIME bundle mapping mime types to content.
pub type MimeBundle = std::collections::HashMap<String, serde_json::Value>;

/// Extract text content from a MIME-bundle JSON value. Jupyter can send text as
/// either a single string or an array of line strings (the nbformat multiline
/// convention).
pub fn extract_text_content(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Array(arr) => arr
            .iter()
            .filter_map(|v| v.as_str())
            .collect::<Vec<_>>()
            .join(""),
        _ => String::new(),
    }
}
