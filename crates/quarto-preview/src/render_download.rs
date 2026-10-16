//! `POST /api/preview/render`: "Download as" for the embedded hub (pandoc-wasm
//! H4b, design D7), grown by Plan 9 of the hub-parity epic into the general
//! download route.
//!
//! The embed carries no pandoc.wasm, so it asks the native side to render.
//! Body: `{ "path": "rel/doc.qmd", "format": "docx", "content": "<editor
//! text>", "metadata"?: { ... }, "attributionJson"?: "<json>" }`.
//! `format` is a key of `PANDOC_FORMATS` (docx, odt, pptx, epub, typst,
//! typst-pdf), looked up in that table so the client's menu and this route
//! cannot drift; the caller's key beats the document's own `format:`. `typst`
//! is the typst source (no compile; a bare `.typ`: images are not bundled, so
//! one taken out of the request directory has broken image paths) and
//! `typst-pdf` the compiled PDF. `content` is the editor's current text (the
//! disk copy can be stale when edits are disabled); the path anchors project
//! discovery and relative resources. Code cells are never executed: recorded
//! captures of the path are spliced in, others compile unexecuted and are
//! counted in `X-Q2-Stats`.
//!
//! The render, response codes and headers are the shared contract of
//! [`crate::render_helper`]; this route adds the attachment headers and never
//! supersedes. Auth is loopback-only, inherited from the server, as for
//! `re_execute`.

use axum::{
    Json,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use quarto_core::pandoc_request::formats::{PandocFormatInfo, pandoc_format};
use quarto_core::stage::Cancellation;
use quarto_hub::context::SharedContext;
use serde::Deserialize;

pub use crate::render_helper::DIAGNOSTICS_HEADER;
use crate::render_helper::{
    CancelOnDrop, RenderJob, locate, may_render_book, note_stale_scope, outcome_response,
    render_blocking, request_metadata, resolve_all_captures, resolve_captures,
};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderRequest {
    pub path: String,
    pub format: String,
    /// The editor's current text; the disk copy is rendered when absent (an
    /// explicit empty string is an empty document).
    #[serde(default)]
    pub content: Option<String>,
    /// Runtime-layer metadata for this render (a JSON object), e.g.
    /// `{"book-scope": "chapter"}`; absent adds nothing.
    #[serde(default)]
    pub metadata: Option<serde_json::Value>,
    /// Client-built attribution transport JSON (Automerge history).
    #[serde(default)]
    pub attribution_json: Option<String>,
    /// Fields no longer (or not yet) read: kept to log a stale client's `scope`.
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// The table row for a downloadable key.
fn download_format(key: &str) -> Option<&'static PandocFormatInfo> {
    pandoc_format(key).filter(|f| f.available && !f.hidden)
}

/// Axum handler for `POST /api/preview/render`.
pub async fn render_handler(
    State(ctx): State<SharedContext>,
    Json(body): Json<RenderRequest>,
) -> Response {
    let Some(info) = download_format(&body.format) else {
        return (
            StatusCode::BAD_REQUEST,
            format!("Format '{}' is not supported for download", body.format),
        )
            .into_response();
    };
    let (abs_path, hub_root) = match locate(&ctx, &body.path) {
        Ok(located) => located,
        Err(response) => return response,
    };
    note_stale_scope(&body.extra, "render");
    let metadata = match request_metadata(body.metadata) {
        Ok(m) => m,
        Err(message) => return (StatusCode::BAD_REQUEST, message).into_response(),
    };
    let captures = resolve_captures(&ctx, &body.path).await;
    let all_captures = if may_render_book(info.key, metadata.as_ref()) {
        resolve_all_captures(&ctx).await
    } else {
        Default::default()
    };

    let cancel = Cancellation::new();
    let _client_gone = CancelOnDrop(cancel.clone());
    let job = RenderJob {
        abs_path,
        format_key: info.key.to_string(),
        content: body.content,
        metadata,
        attribution_json: body.attribution_json,
        captures,
        all_captures,
        hub_root,
        cancellation: cancel,
    };
    match tokio::task::spawn_blocking(move || render_blocking(job)).await {
        Ok(outcome) => outcome_response(outcome, info.mime, true),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("render task failed: {e}"),
        )
            .into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render_helper::content_disposition;
    use axum::http::header;
    use quarto_hub::HubContext;
    use quarto_hub::context::HubConfig;
    use quarto_hub::storage::StorageManager;
    use std::sync::Arc;
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
            metadata: None,
            attribution_json: None,
            extra: Default::default(),
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
        let response = render_handler(State(ctx), request("doc.qmd", "html", None)).await;
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
