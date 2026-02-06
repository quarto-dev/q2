/*
 * engine/jupyter/execute.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Code execution via Jupyter kernels.
 */

//! Code execution via Jupyter kernels.
//!
//! This module handles sending execute requests to kernels and
//! collecting their outputs from the iopub channel.

use jupyter_protocol::{
    ExecuteRequest, ExecutionState, JupyterMessage, JupyterMessageContent, Media, MediaType,
    Status, Stdio,
};
use std::time::Duration;
use tokio::time::timeout;

use super::error::{JupyterError, Result};
use super::session::KernelSession;

/// Default timeout for execution (5 minutes).
const DEFAULT_EXECUTE_TIMEOUT: Duration = Duration::from_secs(300);

/// Result of executing code in a kernel.
#[derive(Debug, Clone)]
pub struct ExecuteResult {
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

impl KernelSession {
    /// Execute code in the kernel and collect outputs.
    ///
    /// This sends an execute_request, collects outputs from iopub until
    /// the kernel returns to idle state, then returns the collected outputs.
    pub async fn execute(&mut self, code: &str) -> Result<ExecuteResult> {
        self.execute_with_timeout(code, DEFAULT_EXECUTE_TIMEOUT)
            .await
    }

    /// Execute code with a custom timeout.
    pub async fn execute_with_timeout(
        &mut self,
        code: &str,
        exec_timeout: Duration,
    ) -> Result<ExecuteResult> {
        self.touch();
        let _execution_count = self.next_execution_count();

        // Build execute request
        let request = ExecuteRequest::new(code.to_string());
        let message: JupyterMessage = request.into();
        let msg_id = message.header.msg_id.clone();

        // Send the request
        self.shell_socket
            .send(message)
            .await
            .map_err(|e| JupyterError::SendError(e.to_string()))?;

        // Collect outputs
        let result = timeout(exec_timeout, self.collect_outputs(&msg_id)).await;

        match result {
            Ok(outputs) => outputs,
            Err(_) => Err(JupyterError::KernelStartupTimeout {
                seconds: exec_timeout.as_secs(),
            }),
        }
    }

    /// Collect outputs from iopub until execution completes.
    async fn collect_outputs(&mut self, request_id: &str) -> Result<ExecuteResult> {
        let mut outputs = Vec::new();
        let mut status = ExecuteStatus::Ok;
        let mut execution_count = None;

        loop {
            let message = self
                .iopub_socket
                .read()
                .await
                .map_err(|e| JupyterError::ReceiveError(e.to_string()))?;

            // Only process messages that are responses to our request
            let is_our_message = message
                .parent_header
                .as_ref()
                .map(|h| h.msg_id == request_id)
                .unwrap_or(false);

            if !is_our_message {
                continue;
            }

            match message.content {
                JupyterMessageContent::Status(Status {
                    execution_state: ExecutionState::Idle,
                }) => {
                    // Kernel is idle - execution complete
                    break;
                }
                JupyterMessageContent::StreamContent(stream) => {
                    let name = match stream.name {
                        Stdio::Stdout => "stdout",
                        Stdio::Stderr => "stderr",
                    };
                    outputs.push(CellOutput::Stream {
                        name: name.to_string(),
                        text: stream.text,
                    });
                }
                JupyterMessageContent::DisplayData(data) => {
                    outputs.push(CellOutput::DisplayData {
                        data: media_to_mime_bundle(&data.data),
                        metadata: serde_json::Value::Object(data.metadata),
                    });
                }
                JupyterMessageContent::ExecuteResult(result) => {
                    let count = result.execution_count.value() as u32;
                    execution_count = Some(count);
                    outputs.push(CellOutput::ExecuteResult {
                        execution_count: count,
                        data: media_to_mime_bundle(&result.data),
                        metadata: serde_json::Value::Object(result.metadata),
                    });
                }
                JupyterMessageContent::ErrorOutput(err) => {
                    status = ExecuteStatus::Error {
                        ename: err.ename.clone(),
                        evalue: err.evalue.clone(),
                        traceback: err.traceback.clone(),
                    };
                    outputs.push(CellOutput::Error {
                        ename: err.ename,
                        evalue: err.evalue,
                        traceback: err.traceback,
                    });
                }
                _ => {
                    // Ignore other message types (Status::Busy, ExecuteInput, etc.)
                }
            }
        }

        Ok(ExecuteResult {
            status,
            outputs,
            execution_count,
        })
    }

    /// Evaluate expressions and return their string representations.
    ///
    /// This uses the `user_expressions` feature of execute_request to
    /// evaluate multiple expressions in a single request without
    /// affecting execution count.
    pub async fn evaluate_expressions(
        &mut self,
        expressions: &[String],
    ) -> Result<Vec<Option<String>>> {
        if expressions.is_empty() {
            return Ok(Vec::new());
        }

        self.touch();

        // Build user_expressions map
        let user_expressions: std::collections::HashMap<String, String> = expressions
            .iter()
            .enumerate()
            .map(|(i, expr)| (i.to_string(), expr.clone()))
            .collect();

        // Build execute request with empty code but user_expressions
        let mut request = ExecuteRequest::new(String::new());
        request.silent = true;
        request.store_history = false;
        request.user_expressions = Some(user_expressions);

        let message: JupyterMessage = request.into();
        let msg_id = message.header.msg_id.clone();

        // Send the request
        self.shell_socket
            .send(message)
            .await
            .map_err(|e| JupyterError::SendError(e.to_string()))?;

        // Wait for idle
        let _ = self.collect_outputs(&msg_id).await?;

        // TODO: Extract user_expressions from execute_reply
        // For now, return empty results
        Ok(vec![None; expressions.len()])
    }
}

/// Convert jupyter_protocol::Media to a MimeBundle.
fn media_to_mime_bundle(media: &Media) -> MimeBundle {
    let mut bundle = MimeBundle::new();

    // Media stores content as Vec<MediaType>
    // We extract the MIME type string and content from each variant
    for media_type in &media.content {
        let (mime_str, value) = media_type_to_mime_entry(media_type);
        bundle.insert(mime_str, value);
    }

    bundle
}

/// Convert a MediaType variant to a (mime_string, value) pair.
fn media_type_to_mime_entry(media_type: &MediaType) -> (String, serde_json::Value) {
    match media_type {
        MediaType::Plain(s) => (
            "text/plain".to_string(),
            serde_json::Value::String(s.clone()),
        ),
        MediaType::Html(s) => (
            "text/html".to_string(),
            serde_json::Value::String(s.clone()),
        ),
        MediaType::Latex(s) => (
            "text/latex".to_string(),
            serde_json::Value::String(s.clone()),
        ),
        MediaType::Javascript(s) => (
            "application/javascript".to_string(),
            serde_json::Value::String(s.clone()),
        ),
        MediaType::Markdown(s) => (
            "text/markdown".to_string(),
            serde_json::Value::String(s.clone()),
        ),
        MediaType::Svg(s) => (
            "image/svg+xml".to_string(),
            serde_json::Value::String(s.clone()),
        ),
        MediaType::Png(s) => (
            "image/png".to_string(),
            serde_json::Value::String(s.clone()),
        ),
        MediaType::Jpeg(s) => (
            "image/jpeg".to_string(),
            serde_json::Value::String(s.clone()),
        ),
        MediaType::Gif(s) => (
            "image/gif".to_string(),
            serde_json::Value::String(s.clone()),
        ),
        MediaType::Json(v) => ("application/json".to_string(), v.clone()),
        MediaType::GeoJson(v) => ("application/geo+json".to_string(), v.clone()),
        MediaType::Plotly(v) => ("application/vnd.plotly.v1+json".to_string(), v.clone()),
        MediaType::WidgetView(v) => (
            "application/vnd.jupyter.widget-view+json".to_string(),
            v.clone(),
        ),
        MediaType::WidgetState(v) => (
            "application/vnd.jupyter.widget-state+json".to_string(),
            v.clone(),
        ),
        MediaType::VegaLiteV2(v) => ("application/vnd.vegalite.v2+json".to_string(), v.clone()),
        MediaType::VegaLiteV3(v) => ("application/vnd.vegalite.v3+json".to_string(), v.clone()),
        MediaType::VegaLiteV4(v) => ("application/vnd.vegalite.v4+json".to_string(), v.clone()),
        MediaType::VegaLiteV5(v) => ("application/vnd.vegalite.v5+json".to_string(), v.clone()),
        MediaType::VegaLiteV6(v) => ("application/vnd.vegalite.v6+json".to_string(), v.clone()),
        MediaType::VegaV3(v) => ("application/vnd.vega.v3+json".to_string(), v.clone()),
        MediaType::VegaV4(v) => ("application/vnd.vega.v4+json".to_string(), v.clone()),
        MediaType::VegaV5(v) => ("application/vnd.vega.v5+json".to_string(), v.clone()),
        MediaType::Vdom(v) => ("application/vdom.v1+json".to_string(), v.clone()),
        MediaType::DataTable(table) => (
            "application/vnd.dataresource+json".to_string(),
            serde_json::to_value(table.as_ref()).unwrap_or(serde_json::Value::Null),
        ),
        MediaType::Other((mime, v)) => (mime.clone(), v.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_execute_status_variants() {
        let ok = ExecuteStatus::Ok;
        assert!(matches!(ok, ExecuteStatus::Ok));

        let error = ExecuteStatus::Error {
            ename: "NameError".to_string(),
            evalue: "name 'x' is not defined".to_string(),
            traceback: vec!["line 1".to_string()],
        };
        assert!(matches!(error, ExecuteStatus::Error { .. }));
    }

    #[test]
    fn test_cell_output_variants() {
        let stream = CellOutput::Stream {
            name: "stdout".to_string(),
            text: "Hello".to_string(),
        };
        assert!(matches!(stream, CellOutput::Stream { .. }));

        let mut data = MimeBundle::new();
        data.insert("text/plain".to_string(), serde_json::json!("42"));

        let result = CellOutput::ExecuteResult {
            execution_count: 1,
            data,
            metadata: serde_json::json!({}),
        };
        assert!(matches!(result, CellOutput::ExecuteResult { .. }));
    }
}
