//! `POST /api/preview/render` — "Download as" for the embedded hub
//! (pandoc-wasm H4b, design D7).
//!
//! The embed carries no pandoc.wasm, so it asks the native side, which has
//! pandoc already, to render. Body: `{ "path": "rel/doc.qmd", "format":
//! "docx", "content": "<editor text>" }`. `content` is the editor's current
//! text; the server renders it as the document's source (the disk copy can be
//! stale when edits are disabled), against the project on disk, so images,
//! `_quarto.yml` and includes resolve as they do for `q2 render`. Code cells
//! are not executed (D11: unexecuted by default).
//!
//! Responses:
//! - `200`: the file's bytes, `Content-Disposition: attachment`, and the
//!   render's warnings as `X-Q2-Diagnostics` (percent-encoded JSON array of
//!   `JsonDiagnostic`; absent when there are none). Warnings still download.
//! - `400`: the path is not in the project index, or the format is not one the
//!   download supports.
//! - `422`: the render failed or produced errors; the body is
//!   `{ "error"?: string, "diagnostics": [...] }` and nothing downloads.
//!
//! Auth is loopback-only, inherited from the server, as for `re_execute`.

use std::path::Path;
use std::sync::Arc;

use axum::{
    Json,
    extract::State,
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
use quarto_core::engine::ExecutionPolicy;
use quarto_core::render_to_file::{RenderToFileOptions, render_to_file};
use quarto_error_reporting::{DiagnosticKind, JsonDiagnostic, diagnostic_to_json};
use quarto_hub::context::SharedContext;
use quarto_system_runtime::{NativeRuntime, SystemRuntime};
use serde::{Deserialize, Serialize};

/// Header carrying the render's warnings.
pub const DIAGNOSTICS_HEADER: &str = "x-q2-diagnostics";

/// The formats the download supports: pandoc.wasm's table (D8) minus typst. Native
/// `format: typst` compiles to PDF; the `.typ`-only download D8 wants needs a
/// pipeline option that stops before `TypstCompileStage`, which no phase has yet.
const FORMATS: &[(&str, &str)] = &[
    (
        "docx",
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
    ),
    (
        "pptx",
        "application/vnd.openxmlformats-officedocument.presentationml.presentation",
    ),
    ("epub", "application/epub+zip"),
];

#[derive(Debug, Deserialize)]
pub struct RenderRequest {
    pub path: String,
    pub format: String,
    /// The editor's current text; the disk copy is rendered when absent.
    #[serde(default)]
    pub content: Option<String>,
}

#[derive(Debug, Serialize)]
struct RenderFailed {
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    diagnostics: Vec<JsonDiagnostic>,
}

struct Rendered {
    file_name: String,
    bytes: Vec<u8>,
    warnings: Vec<JsonDiagnostic>,
}

/// Axum handler for `POST /api/preview/render`.
pub async fn render_handler(
    State(ctx): State<SharedContext>,
    Json(body): Json<RenderRequest>,
) -> Response {
    let Some(&(_, mime)) = FORMATS.iter().find(|(f, _)| *f == body.format) else {
        return (
            StatusCode::BAD_REQUEST,
            format!("Format '{}' is not supported for download", body.format),
        )
            .into_response();
    };
    if !ctx.index().has_file(&body.path) {
        return (
            StatusCode::BAD_REQUEST,
            format!("Path '{}' is not in the project index", body.path),
        )
            .into_response();
    }
    let Some(project_root) = ctx.storage().project_root().map(Path::to_path_buf) else {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            "no project root (standalone mode?)",
        )
            .into_response();
    };
    let abs_path = project_root.join(&body.path);

    let outcome =
        tokio::task::spawn_blocking(move || render_blocking(&abs_path, &body.format, body.content))
            .await;
    match outcome {
        Ok(Ok(rendered)) => success_response(rendered, mime),
        Ok(Err(f)) => (StatusCode::UNPROCESSABLE_ENTITY, Json(f)).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("render task failed: {e}"),
        )
            .into_response(),
    }
}

fn render_blocking(
    abs_path: &Path,
    format: &str,
    content: Option<String>,
) -> Result<Rendered, RenderFailed> {
    let failed = |error: String| RenderFailed {
        error: Some(error),
        diagnostics: Vec::new(),
    };
    let out_dir = tempfile::TempDir::with_prefix("q2-download-")
        .map_err(|e| failed(format!("could not create a temporary directory: {e}")))?;
    let options = RenderToFileOptions {
        output_dir: Some(out_dir.path().to_path_buf()),
        quiet: true,
        execution_policy: ExecutionPolicy::None,
        source_override: content.map(String::into_bytes),
        ..Default::default()
    };
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let result =
        render_to_file(abs_path, format, &options, runtime).map_err(|e| failed(e.to_string()))?;

    let diagnostics = &result.render_output.diagnostics;
    let ctx = &result.render_output.source_context;
    let has_errors = diagnostics
        .iter()
        .any(|d| matches!(d.kind, DiagnosticKind::Error));
    let json: Vec<JsonDiagnostic> = diagnostics
        .iter()
        .map(|d| diagnostic_to_json(d, ctx))
        .collect();
    if has_errors {
        return Err(RenderFailed {
            error: None,
            diagnostics: json,
        });
    }
    let bytes = std::fs::read(&result.output_path).map_err(|e| {
        failed(format!(
            "could not read the rendered file {}: {e}",
            result.output_path.display()
        ))
    })?;
    let file_name = result.output_path.file_name().map_or_else(
        || format!("download.{format}"),
        |n| n.to_string_lossy().into_owned(),
    );
    Ok(Rendered {
        file_name,
        bytes,
        warnings: json,
    })
}

fn success_response(rendered: Rendered, mime: &'static str) -> Response {
    let mut response = (StatusCode::OK, rendered.bytes).into_response();
    let headers = response.headers_mut();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(mime));
    if let Ok(v) = HeaderValue::from_str(&content_disposition(&rendered.file_name)) {
        headers.insert(header::CONTENT_DISPOSITION, v);
    }
    if !rendered.warnings.is_empty()
        && let Ok(json) = serde_json::to_string(&rendered.warnings)
        && let Ok(v) =
            HeaderValue::from_str(&utf8_percent_encode(&json, NON_ALPHANUMERIC).to_string())
    {
        headers.insert(DIAGNOSTICS_HEADER, v);
    }
    response
}

/// `attachment` with an ASCII fallback `filename` and the exact name as RFC 5987 `filename*`.
fn content_disposition(file_name: &str) -> String {
    let ascii: String = file_name
        .chars()
        .map(|c| {
            if c.is_ascii_graphic() && c != '"' && c != '\\' && c != '%' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let encoded = utf8_percent_encode(file_name, NON_ALPHANUMERIC);
    format!("attachment; filename=\"{ascii}\"; filename*=UTF-8''{encoded}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_hub::HubContext;
    use quarto_hub::context::HubConfig;
    use quarto_hub::storage::StorageManager;
    use tempfile::TempDir;

    async fn build_ctx(files: &[(&str, &str)]) -> (TempDir, SharedContext) {
        let project = TempDir::with_prefix("h4b-test-").unwrap();
        let root = project.path().canonicalize().unwrap();
        for (name, content) in files {
            std::fs::write(root.join(name), content).unwrap();
        }
        let storage = StorageManager::new(&root).unwrap();
        let ctx = Arc::new(
            HubContext::new(storage, HubConfig::default())
                .await
                .unwrap(),
        );
        (project, ctx)
    }

    fn request(path: &str, format: &str, content: Option<&str>) -> Json<RenderRequest> {
        Json(RenderRequest {
            path: path.to_string(),
            format: format.to_string(),
            content: content.map(str::to_string),
        })
    }

    /// Fail with the response body (the 422 JSON names the diagnostics).
    async fn expect_ok(response: Response) -> Response {
        if response.status() == StatusCode::OK {
            return response;
        }
        let status = response.status();
        let body = String::from_utf8_lossy(&body_bytes(response).await).into_owned();
        panic!("expected 200, got {status}: {body}");
    }

    async fn body_bytes(response: Response) -> Vec<u8> {
        axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap()
            .to_vec()
    }

    /// The text of a docx's `word/document.xml`.
    fn docx_text(bytes: Vec<u8>) -> String {
        use std::io::Read;
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).expect("a docx is a zip");
        let mut xml = String::new();
        zip.by_name("word/document.xml")
            .expect("word/document.xml")
            .read_to_string(&mut xml)
            .unwrap();
        xml
    }

    #[tokio::test]
    async fn renders_the_editor_content_not_the_disk_copy() {
        let (_tmp, ctx) =
            build_ctx(&[("doc.qmd", "---\ntitle: T\n---\n\nSTALE disk text.\n")]).await;
        let response = render_handler(
            State(ctx),
            request(
                "doc.qmd",
                "docx",
                Some("---\ntitle: T\n---\n\nFRESH editor text.\n"),
            ),
        )
        .await;
        let response = expect_ok(response).await;
        assert_eq!(
            response.headers()[header::CONTENT_TYPE],
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
        );
        let disposition = response.headers()[header::CONTENT_DISPOSITION]
            .to_str()
            .unwrap()
            .to_string();
        assert!(
            disposition.contains("filename=\"doc.docx\""),
            "{disposition}"
        );
        let text = docx_text(body_bytes(response).await);
        assert!(text.contains("FRESH editor text."), "{text}");
        assert!(!text.contains("STALE"), "{text}");
    }

    #[tokio::test]
    async fn falls_back_to_the_disk_copy_without_content() {
        let (_tmp, ctx) = build_ctx(&[("doc.qmd", "---\ntitle: T\n---\n\nDisk text.\n")]).await;
        let response = render_handler(State(ctx), request("doc.qmd", "docx", None)).await;
        let response = expect_ok(response).await;
        let text = docx_text(body_bytes(response).await);
        assert!(text.contains("Disk text."), "{text}");
    }

    #[tokio::test]
    async fn unsupported_format_is_400() {
        let (_tmp, ctx) = build_ctx(&[("doc.qmd", "hi\n")]).await;
        let response = render_handler(State(ctx), request("doc.qmd", "typst", None)).await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn path_outside_the_index_is_400() {
        let (_tmp, ctx) = build_ctx(&[("doc.qmd", "hi\n")]).await;
        for path in ["nonexistent.qmd", "../doc.qmd", "/etc/passwd"] {
            let response = render_handler(State(ctx.clone()), request(path, "docx", None)).await;
            assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{path}");
        }
    }

    #[tokio::test]
    async fn a_render_with_errors_is_422_and_does_not_download() {
        let (_tmp, ctx) = build_ctx(&[("doc.qmd", "hi\n")]).await;
        let response = render_handler(
            State(ctx),
            request(
                "doc.qmd",
                "docx",
                Some("---\ntitle: [unclosed\n---\n\nBody.\n"),
            ),
        )
        .await;
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let body: serde_json::Value = serde_json::from_slice(&body_bytes(response).await).unwrap();
        assert!(
            body["error"].is_string() || !body["diagnostics"].as_array().unwrap().is_empty(),
            "an error or diagnostics must explain the failure: {body}"
        );
    }

    #[tokio::test]
    async fn the_route_is_mounted_on_the_preview_router() {
        use tower::ServiceExt;
        let (_tmp, ctx) = build_ctx(&[("doc.qmd", "hi\n")]).await;
        let app = crate::extend_with_preview(axum::Router::new()).with_state(ctx);
        let req = axum::http::Request::builder()
            .method("POST")
            .uri("/api/preview/render")
            .header(header::CONTENT_TYPE, "application/json")
            .body(axum::body::Body::from(
                r#"{"path":"doc.qmd","format":"docx"}"#,
            ))
            .unwrap();
        let response = app.oneshot(req).await.unwrap();
        let response = expect_ok(response).await;
        assert_eq!(&body_bytes(response).await[..2], b"PK");
    }

    #[test]
    fn content_disposition_keeps_the_exact_name_and_a_safe_fallback() {
        let d = content_disposition("café \"x\".docx");
        assert!(
            d.starts_with("attachment; filename=\"caf___x_.docx\""),
            "{d}"
        );
        assert!(
            d.contains("filename*=UTF-8''caf%C3%A9%20%22x%22%2Edocx"),
            "{d}"
        );
    }
}
