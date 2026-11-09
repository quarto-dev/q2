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
use crate::engine::DEFAULT_EXECUTE_TIMEOUT;
// The kernel result types are target-neutral and live in the presenter (EXR-1).
use crate::presenter::output::{CellOutput, CellResult, ExecuteStatus, MimeBundle};

/// Flags of an `execute_request` that the engine chooses per call.
///
/// A cell uses the default (`store_history = true`, `silent = false`). An inline
/// expression runs with `store_history = false` and `silent = false`: `silent = true`
/// would suppress the `execute_result` whose value the span needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExecuteOpts {
    pub store_history: bool,
    pub silent: bool,
}

impl Default for ExecuteOpts {
    fn default() -> Self {
        Self {
            store_history: true,
            silent: false,
        }
    }
}

impl ExecuteOpts {
    /// The flags for an inline expression (EXR-3 design decision 1).
    pub const INLINE: Self = Self {
        store_history: false,
        silent: false,
    };
}

/// Build the `execute_request` for `code` with `opts`.
pub(crate) fn build_execute_request(code: &str, opts: ExecuteOpts) -> ExecuteRequest {
    let mut request = ExecuteRequest::new(code.to_string());
    request.store_history = opts.store_history;
    request.silent = opts.silent;
    request
}

impl KernelSession {
    /// Execute code in the kernel and collect outputs.
    ///
    /// This sends an execute_request, collects outputs from iopub until
    /// the kernel returns to idle state, then returns the collected outputs.
    pub async fn execute(&mut self, code: &str) -> Result<CellResult> {
        self.execute_with(code, ExecuteOpts::default()).await
    }

    /// Execute code with explicit request flags (see [`ExecuteOpts`]).
    pub async fn execute_with(&mut self, code: &str, opts: ExecuteOpts) -> Result<CellResult> {
        self.execute_with_opts_and_timeout(code, opts, DEFAULT_EXECUTE_TIMEOUT)
            .await
    }

    /// Execute code with a custom timeout.
    pub async fn execute_with_timeout(
        &mut self,
        code: &str,
        exec_timeout: Duration,
    ) -> Result<CellResult> {
        self.execute_with_opts_and_timeout(code, ExecuteOpts::default(), exec_timeout)
            .await
    }

    async fn execute_with_opts_and_timeout(
        &mut self,
        code: &str,
        opts: ExecuteOpts,
        exec_timeout: Duration,
    ) -> Result<CellResult> {
        let _execution_count = self.next_execution_count();

        // Build execute request
        let message: JupyterMessage = build_execute_request(code, opts).into();
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
    async fn collect_outputs(&mut self, request_id: &str) -> Result<CellResult> {
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
                .is_some_and(|h| h.msg_id == request_id);

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
                    push_stream(&mut outputs, name, stream.text);
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

        Ok(CellResult {
            status,
            outputs,
            execution_count,
        })
    }
}

/// Append stream text, merging it into the previous output when that is a stream of the same
/// name. The kernel flushes `print("x")` as `"x"` and `"\n"` in separate messages whenever its
/// flush timer fires between the two writes, so the split is timing-dependent; nbclient merges
/// such runs too, which keeps the recorded outputs deterministic.
fn push_stream(outputs: &mut Vec<CellOutput>, name: &str, text: String) {
    if let Some(CellOutput::Stream {
        name: last_name,
        text: last_text,
    }) = outputs.last_mut()
        && last_name == name
    {
        last_text.push_str(&text);
        return;
    }
    outputs.push(CellOutput::Stream {
        name: name.to_string(),
        text,
    });
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
        // `MediaType` is `#[non_exhaustive]` (jupyter-protocol 2.0). Future
        // variants we don't recognize fall through here; preserve the MIME
        // string so downstream rendering can still decide what to do.
        other => (other.mime_type().to_string(), serde_json::Value::Null),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stream_texts(outputs: &[CellOutput]) -> Vec<(String, String)> {
        outputs
            .iter()
            .map(|o| match o {
                CellOutput::Stream { name, text } => (name.clone(), text.clone()),
                other => panic!("not a stream: {other:?}"),
            })
            .collect()
    }

    #[test]
    fn push_stream_merges_adjacent_same_name_messages() {
        let mut outputs = Vec::new();
        push_stream(&mut outputs, "stdout", "cell 19".to_string());
        push_stream(&mut outputs, "stdout", "\n".to_string());
        push_stream(&mut outputs, "stderr", "warn\n".to_string());
        push_stream(&mut outputs, "stdout", "after\n".to_string());
        assert_eq!(
            stream_texts(&outputs),
            vec![
                ("stdout".to_string(), "cell 19\n".to_string()),
                ("stderr".to_string(), "warn\n".to_string()),
                ("stdout".to_string(), "after\n".to_string()),
            ]
        );
    }

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

#[cfg(test)]
mod request_tests {
    use super::*;

    #[test]
    fn default_opts_are_a_normal_cell() {
        let r = build_execute_request("1+1", ExecuteOpts::default());
        assert_eq!(r.code, "1+1");
        assert!(r.store_history);
        assert!(!r.silent);
    }

    #[test]
    fn inline_opts_do_not_store_history_and_are_not_silent() {
        let r = build_execute_request("x", ExecuteOpts::INLINE);
        assert!(!r.store_history);
        assert!(!r.silent, "silent would suppress the execute_result");
    }
}
