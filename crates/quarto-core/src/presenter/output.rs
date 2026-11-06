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

impl CellOutput {
    /// This output as an nbformat 4 output object (EXR-5): lossless, the raw mime
    /// bundle travels as is and stream text is kept exactly as the kernel sent it
    /// (a string, not nbformat's optional line array). `metadata` is always an
    /// object, as nbformat requires. The inverse is `ipynb_replay`'s
    /// `stored_result_for_cell`.
    pub fn to_nbformat(&self) -> serde_json::Value {
        use serde_json::json;
        fn object(metadata: &serde_json::Value) -> serde_json::Value {
            match metadata {
                serde_json::Value::Object(_) => metadata.clone(),
                _ => json!({}),
            }
        }
        match self {
            CellOutput::Stream { name, text } => json!({
                "output_type": "stream",
                "name": name,
                "text": text,
            }),
            CellOutput::DisplayData { data, metadata } => json!({
                "output_type": "display_data",
                "data": data,
                "metadata": object(metadata),
            }),
            CellOutput::ExecuteResult {
                execution_count,
                data,
                metadata,
            } => json!({
                "output_type": "execute_result",
                "execution_count": execution_count,
                "data": data,
                "metadata": object(metadata),
            }),
            CellOutput::Error {
                ename,
                evalue,
                traceback,
            } => json!({
                "output_type": "error",
                "ename": ename,
                "evalue": evalue,
                "traceback": traceback,
            }),
        }
    }
}

impl CellResult {
    /// The outputs as nbformat 4 output objects, in order (EXR-5).
    pub fn nbformat_outputs(&self) -> Vec<serde_json::Value> {
        self.outputs.iter().map(CellOutput::to_nbformat).collect()
    }

    /// A cell's stored nbformat outputs as the result shape the presenter consumes
    /// (the inverse of [`CellResult::nbformat_outputs`]). Handled `output_type`s:
    /// `stream`, `execute_result`, `display_data`, `error`; legacy pre-nbformat-4
    /// types (`pyout`/`pyerr`) are skipped. The status is always `Ok`: a stored
    /// `error` output is content to show, not a status to abort on. Shared by the
    /// stored-notebook replay (`ipynb_replay`) and the execution record (EXR-5).
    pub fn from_nbformat(outputs: &[serde_json::Value], execution_count: Option<u64>) -> Self {
        let mut converted = Vec::new();
        for out in outputs {
            match out.get("output_type").and_then(|t| t.as_str()) {
                Some("stream") => converted.push(CellOutput::Stream {
                    name: out
                        .get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("stdout")
                        .to_string(),
                    text: out
                        .get("text")
                        .map(extract_text_content)
                        .unwrap_or_default(),
                }),
                Some("execute_result") => converted.push(CellOutput::ExecuteResult {
                    execution_count: out
                        .get("execution_count")
                        .and_then(|v| v.as_u64())
                        .and_then(|v| u32::try_from(v).ok())
                        .unwrap_or(0),
                    data: nbformat_mime_bundle(out.get("data")),
                    metadata: out
                        .get("metadata")
                        .cloned()
                        .unwrap_or(serde_json::Value::Null),
                }),
                Some("display_data") => converted.push(CellOutput::DisplayData {
                    data: nbformat_mime_bundle(out.get("data")),
                    metadata: out
                        .get("metadata")
                        .cloned()
                        .unwrap_or(serde_json::Value::Null),
                }),
                Some("error") => converted.push(CellOutput::Error {
                    ename: out
                        .get("ename")
                        .and_then(|v| v.as_str())
                        .unwrap_or("Error")
                        .to_string(),
                    evalue: out
                        .get("evalue")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string(),
                    traceback: out
                        .get("traceback")
                        .and_then(|v| v.as_array())
                        .map(|a| {
                            a.iter()
                                .filter_map(|s| s.as_str())
                                .map(str::to_string)
                                .collect()
                        })
                        .unwrap_or_default(),
                }),
                other => tracing::debug!(
                    output_type = other.unwrap_or("<missing>"),
                    "nbformat: skipping unsupported stored output"
                ),
            }
        }
        CellResult {
            status: ExecuteStatus::Ok,
            outputs: converted,
            execution_count: execution_count.and_then(|v| u32::try_from(v).ok()),
        }
    }
}

/// A nbformat mime bundle (`{"text/html": ...}`) as the kernel's [`MimeBundle`].
fn nbformat_mime_bundle(value: Option<&serde_json::Value>) -> MimeBundle {
    value
        .and_then(|v| v.as_object())
        .map(|obj| obj.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
        .unwrap_or_default()
}

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
