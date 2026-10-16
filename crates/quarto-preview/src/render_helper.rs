//! The render helper and response contract shared by `POST /api/preview/pdf`
//! ([`crate::pdf_preview`]) and `POST /api/preview/render`
//! ([`crate::render_download`]) (epic E1, Plan 9).
//!
//! One blocking function, [`render_blocking`], does what both routes need:
//! a request directory under the discovered project (Plan 6), the editor's
//! text as the source, server-resolved captures spliced in place of running
//! kernels, the client's attribution payload, the typst target, child
//! cancellation and a deadline (Plan 1), scope resolution (Plan 3), and
//! structured diagnostics and stats. The routes only decode requests and
//! encode [`RenderOutcome`]s.
//!
//! Two process-wide seams, both statics (the preview router is built from a
//! bare fn pointer, so there is no extension slot; `re_execute.rs` does the
//! same): [`RenderSeam`] stands in for the real `quarto-core` render entry
//! points so route tests do not race real typst, and [`BookDownload`] is the
//! whole-book entry that Plan 10 fills in.
//!
//! The response contract is in [`outcome_response`]: 200 with
//! `X-Q2-Diagnostics` (warnings, percent-encoded JSON) and `X-Q2-Stats`
//! (percent-encoded `{"unexecuted_cells": N}`); 422 `{error?, diagnostics}`;
//! 409 `{"superseded": true}` or `{"busy": true}`; 501 for a seam that has no
//! book downloads. `X-Q2-Stats` also carries `book {scope, chapters}` when the
//! path belongs to a book.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::Duration;

use axum::{
    Json,
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
use quarto_core::book_download::{BookDownloadFailure, BookDownloadParams, render_book_download};
use quarto_core::engine::ExecutionPolicy;
use quarto_core::format::Format;
use quarto_core::format_resolution::{book_scope_in_json, resolve_render_scope};
use quarto_core::overlay_runtime::RenderOverlay;
use quarto_core::pandoc_request::formats::TYPST_PDF_KEY;
use quarto_core::preview_out::PreviewOutDir;
use quarto_core::project::ProjectContext;
use quarto_core::project::book::{ResolvedBookScope, format_consolidates_book, resolve_book_scope};
use quarto_core::project::orchestrator::RenderMode;
use quarto_core::render_to_file::{
    RenderToFileOptions, RenderToFileResult, TypstTarget, render_document_to_file,
};
use quarto_core::stage::Cancellation;
use quarto_core::{BookScope, QuartoError};
use quarto_error_reporting::{
    DiagnosticKind, DiagnosticMessage, JsonDiagnostic, diagnostic_to_json,
};
use quarto_hub::context::SharedContext;
use quarto_source_map::SourceContext;
use quarto_system_runtime::SystemRuntime;
use quarto_trace::EngineCapture;
use serde::Serialize;

/// Header carrying the render's warnings.
pub const DIAGNOSTICS_HEADER: &str = "x-q2-diagnostics";

/// Header carrying `{"unexecuted_cells": N}`.
pub const STATS_HEADER: &str = "x-q2-stats";

/// Wall-clock limit for each child process (typst, pandoc) of a route render.
/// The orange-book takes about 2 s; a stuck child must not hold a blocking
/// thread (and, for `/api/preview/pdf`, the path's slot) forever.
pub const CHILD_DEADLINE: Duration = Duration::from_secs(120);

/// Request body limit of both render routes. A `content` plus an
/// `attributionJson` (about 80 bytes per run: one run per contiguous stretch
/// of one author's text, so 8 MB holds a history of roughly 100 000 runs) is
/// far below it; axum's implicit 2 MB is not.
pub const BODY_LIMIT: usize = 8 * 1024 * 1024;

/// The request's `metadata` as the overlay's runtime metadata (F4-D1): absent
/// or `null` adds nothing, a JSON object is the metadata, anything else is a
/// 400 (the caller's message).
pub fn request_metadata(
    metadata: Option<serde_json::Value>,
) -> Result<Option<serde_json::Value>, String> {
    match metadata {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(value @ serde_json::Value::Object(_)) => Ok(Some(value)),
        Some(_) => Err("`metadata` must be a JSON object".to_string()),
    }
}

/// A body from a stale client may still carry the removed `scope` field. It is
/// ignored (no body denies unknown fields), so say so at debug level to keep a
/// stale embedded SPA diagnosable.
pub fn note_stale_scope(extra: &serde_json::Map<String, serde_json::Value>, route: &str) {
    if let Some(scope) = extra.get("scope") {
        tracing::debug!(
            route,
            %scope,
            "ignoring the removed `scope` request field; send `metadata: {{\"book-scope\": ...}}` (a stale client?)"
        );
    }
}

/// `{ error?, diagnostics }`: the body of a 422.
#[derive(Debug, Serialize)]
pub struct RenderFailed {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub diagnostics: Vec<JsonDiagnostic>,
}

impl RenderFailed {
    pub fn message(error: impl Into<String>) -> Self {
        Self {
            error: Some(error.into()),
            diagnostics: Vec::new(),
        }
    }
}

/// A finished render.
#[derive(Debug)]
pub struct RenderedFile {
    pub file_name: String,
    pub bytes: Vec<u8>,
    pub warnings: Vec<JsonDiagnostic>,
    pub unexecuted_cells: usize,
    /// Set when the path belongs to a book (`X-Q2-Stats` `book`).
    pub book: Option<BookStats>,
}

/// What `X-Q2-Stats` says about a book render: the scope it resolved to
/// (`book`, `chapter` or `none`) and the book's file-bearing chapters.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct BookStats {
    pub scope: &'static str,
    pub chapters: usize,
}

fn scope_name(scope: ResolvedBookScope) -> &'static str {
    match scope {
        ResolvedBookScope::Book => "book",
        ResolvedBookScope::Chapter => "chapter",
        ResolvedBookScope::NoBook => "none",
    }
}

/// What a route sends back.
#[derive(Debug)]
pub enum RenderOutcome {
    Ok(RenderedFile),
    Failed(RenderFailed),
    /// A newer request for the same path replaced this one (`/pdf` only).
    Superseded,
    /// Another whole-book render holds the project (Plan 10).
    Busy,
    /// 501: this build cannot do it.
    Unavailable(String),
}

/// Everything [`render_blocking`] needs, resolved on the async side.
pub struct RenderJob {
    /// Absolute path of the document (hub root joined with the index path).
    pub abs_path: PathBuf,
    /// The format key: a `PANDOC_FORMATS` key (`docx`, `typst`, `typst-pdf`, ...).
    pub format_key: String,
    /// The editor's text; the disk copy is rendered when `None`.
    pub content: Option<String>,
    /// Runtime-layer metadata for this render (FR12): the request's
    /// `metadata` object; `None` adds nothing.
    pub metadata: Option<serde_json::Value>,
    pub attribution_json: Option<String>,
    /// The sidecar captures of this path (never a client-named id; E10).
    pub captures: Vec<EngineCapture>,
    /// Every sidecar entry of the hub, keyed by hub-root-relative path;
    /// filled only when the request may resolve to a whole book.
    pub all_captures: HashMap<String, Vec<EngineCapture>>,
    /// The canonical hub root the sidecar keys are relative to.
    pub hub_root: PathBuf,
    pub cancellation: Cancellation,
}

/// The render entry points of `quarto-core`, behind a seam.
pub trait RenderSeam: Send + Sync {
    /// `render_document_to_file` for the page at `input`, rendering `format`
    /// whatever the project and the page declare (what `render_to_file` does).
    fn render_document(
        &self,
        input: &Path,
        format: &str,
        options: &RenderToFileOptions,
        project: &ProjectContext,
        runtime: Arc<dyn SystemRuntime>,
    ) -> quarto_core::Result<RenderToFileResult>;

    /// `render_chapter_alone`.
    fn render_chapter(
        &self,
        project: &ProjectContext,
        abs_path: &Path,
        format: &Format,
        options: &RenderToFileOptions,
        runtime: Arc<dyn SystemRuntime>,
    ) -> quarto_core::Result<RenderToFileResult>;
}

struct RealRender;

impl RenderSeam for RealRender {
    fn render_document(
        &self,
        input: &Path,
        format: &str,
        options: &RenderToFileOptions,
        project: &ProjectContext,
        runtime: Arc<dyn SystemRuntime>,
    ) -> quarto_core::Result<RenderToFileResult> {
        render_document_to_file(
            input,
            format,
            options,
            Some(project),
            runtime,
            None,
            None,
            Some(format),
        )
    }

    fn render_chapter(
        &self,
        project: &ProjectContext,
        abs_path: &Path,
        format: &Format,
        options: &RenderToFileOptions,
        runtime: Arc<dyn SystemRuntime>,
    ) -> quarto_core::Result<RenderToFileResult> {
        pollster::block_on(quarto_core::render_chapter_alone(
            project, abs_path, format, options, runtime,
        ))
    }
}

static RENDER_SEAM: RwLock<Option<Arc<dyn RenderSeam>>> = RwLock::new(None);

fn render_seam() -> Arc<dyn RenderSeam> {
    RENDER_SEAM
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
        .unwrap_or_else(|| Arc::new(RealRender))
}

/// Replace the render seam (`None` restores the real one). For tests: swap it
/// only in a test that runs in its own process (nextest's default).
pub fn set_render_seam(seam: Option<Arc<dyn RenderSeam>>) {
    *RENDER_SEAM.write().unwrap_or_else(|e| e.into_inner()) = seam;
}

/// What a whole-book download needs.
pub struct BookDownloadRequest {
    pub path: PathBuf,
    pub content: Option<String>,
    pub format: String,
    pub typst_target: Option<TypstTarget>,
    pub request_dir: PathBuf,
    pub hub_root: PathBuf,
    /// Every sidecar entry of the hub, keyed by hub-root-relative path.
    pub captures: HashMap<String, Vec<EngineCapture>>,
    pub cancellation: Cancellation,
    /// Runtime-layer metadata for this render (FR12).
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug)]
pub struct BookDownloadOutput {
    pub file: PathBuf,
    pub chapters: usize,
    pub unexecuted_cells: usize,
    pub diagnostics: Vec<JsonDiagnostic>,
}

#[derive(Debug)]
pub enum BookDownloadError {
    /// Another book render holds the project.
    Busy,
    Cancelled,
    Failed(RenderFailed),
    /// This build has no book downloads (a test seam can answer it).
    Unavailable(String),
}

/// The whole-book entry. Called inside `spawn_blocking`.
pub trait BookDownload: Send + Sync {
    fn render(&self, req: BookDownloadRequest) -> Result<BookDownloadOutput, BookDownloadError>;
}

/// The real whole-book entry: `quarto-core`'s native book pipeline into the
/// request directory (Plan 10).
struct NativeBookDownload;

impl BookDownload for NativeBookDownload {
    fn render(&self, req: BookDownloadRequest) -> Result<BookDownloadOutput, BookDownloadError> {
        let format = Format::from_format_string(render_format_and_target(&req.format).0)
            .map_err(|e| BookDownloadError::Failed(RenderFailed::message(e)))?;
        let result = render_book_download(BookDownloadParams {
            path: &req.path,
            content: req.content,
            format: &format,
            typst_target: req.typst_target,
            request_dir: &req.request_dir,
            hub_root: &req.hub_root,
            captures: &req.captures,
            cancellation: req.cancellation,
            child_deadline: Some(CHILD_DEADLINE),
            metadata: req.metadata,
            // The server's runtime, so a whole-book download resolves the
            // session's `--profile` like the single-page routes (F8-D6).
            runtime: Arc::new(crate::render_flags::native_runtime()),
        })
        .map_err(|e| match e {
            BookDownloadFailure::Busy => BookDownloadError::Busy,
            BookDownloadFailure::Cancelled => BookDownloadError::Cancelled,
            BookDownloadFailure::Failed(e) => BookDownloadError::Failed(failure_from(e)),
        })?;
        let diagnostics = json_all(&result.diagnostics, &result.source_context);
        if result
            .diagnostics
            .iter()
            .any(|d| matches!(d.kind, DiagnosticKind::Error))
        {
            return Err(BookDownloadError::Failed(RenderFailed {
                error: None,
                diagnostics,
            }));
        }
        Ok(BookDownloadOutput {
            file: result.file,
            chapters: result.chapters,
            unexecuted_cells: result.unexecuted_cells,
            diagnostics,
        })
    }
}

static BOOK_DOWNLOAD: RwLock<Option<Arc<dyn BookDownload>>> = RwLock::new(None);

fn book_download() -> Arc<dyn BookDownload> {
    BOOK_DOWNLOAD
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
        .unwrap_or_else(|| Arc::new(NativeBookDownload))
}

/// Replace the book seam (`None` restores the native default); tests swap it
/// in a process of their own.
pub fn set_book_download(seam: Option<Arc<dyn BookDownload>>) {
    *BOOK_DOWNLOAD.write().unwrap_or_else(|e| e.into_inner()) = seam;
}

/// The sidecar captures of `rel_path`, read on the async side from the
/// capture doc the sidecar names for exactly that path (E10, P9-D4). Absent,
/// unreadable or never-recorded entries give no captures, so the document
/// compiles unexecuted and reports the notice.
pub async fn resolve_captures(ctx: &SharedContext, rel_path: &str) -> Vec<EngineCapture> {
    let Some(entry) = ctx.index().get_capture(rel_path) else {
        return Vec::new();
    };
    match crate::capture_driver::read_capture_from_doc(ctx, &entry.capture_doc_id).await {
        Ok(captures) => captures,
        Err(e) => {
            tracing::debug!(path = rel_path, error = %e, "capture doc unreadable; rendering unexecuted");
            Vec::new()
        }
    }
}

/// Every sidecar entry of the hub, for a whole-book render.
pub async fn resolve_all_captures(ctx: &SharedContext) -> HashMap<String, Vec<EngineCapture>> {
    let mut all = HashMap::new();
    for (key, entry) in ctx.index().get_all_captures() {
        if let Ok(captures) =
            crate::capture_driver::read_capture_from_doc(ctx, &entry.capture_doc_id).await
        {
            all.insert(key, captures);
        }
    }
    all
}

/// Whether a request for `format_key` can resolve to a whole book (so the
/// every-entry capture read is worth doing): a consolidating format, unless the
/// request's own `metadata` says `book-scope: chapter` (F4-D3; the PDF pane).
///
/// A request with no `book-scope` on a consolidating format deliberately loads
/// the captures although the target's default for one page is `chapter`: a
/// `book-scope: book` in the document, directory or project layer, which this
/// async side cannot see without reading them, would otherwise render a book
/// with empty captures. The price is one full capture read for every such
/// request. The same value (through `book_scope_in_json`) drives the resolver.
pub fn may_render_book(format_key: &str, metadata: Option<&serde_json::Value>) -> bool {
    let (base_key, _) = render_format_and_target(format_key);
    let consolidates =
        Format::from_format_string(base_key).is_ok_and(|f| format_consolidates_book(&f));
    let explicit_chapter = metadata
        .and_then(|m| book_scope_in_json(m).ok().flatten())
        .is_some_and(|scope| scope == BookScope::Chapter);
    consolidates && !explicit_chapter
}

/// Split a table key into the render format and the typst target (E8): the
/// `typst-pdf` key is `typst` compiled to a PDF, the `typst` key is the source.
pub fn render_format_and_target(format_key: &str) -> (&str, Option<TypstTarget>) {
    if format_key == TYPST_PDF_KEY {
        ("typst", Some(TypstTarget::Pdf))
    } else if format_key == "typst" {
        ("typst", Some(TypstTarget::Source))
    } else {
        (format_key, None)
    }
}

fn json_all(diagnostics: &[DiagnosticMessage], ctx: &SourceContext) -> Vec<JsonDiagnostic> {
    diagnostics
        .iter()
        .map(|d| diagnostic_to_json(d, ctx))
        .collect()
}

/// A render error as a 422 body: a parse error keeps its structured
/// diagnostics, anything else is a message.
fn failure_from(error: QuartoError) -> RenderFailed {
    match error {
        QuartoError::Parse(parse) => RenderFailed {
            error: None,
            diagnostics: json_all(&parse.diagnostics, &parse.source_context),
        },
        other => RenderFailed::message(other.to_string()),
    }
}

/// The blocking half of both routes. Everything that touches the project
/// directory happens here, and the request directory is removed (its guard
/// drops) only after the output bytes are read.
pub fn render_blocking(job: RenderJob) -> RenderOutcome {
    // This function creates the runtime for the whole request, so it applies
    // the overlay here, once, before the scope resolver runs: the resolver,
    // `render_chapter` and the render seam (a fake included) all see the
    // wrapped runtime (FR12).
    //
    // The request's `metadata` is the overlay's runtime metadata and the
    // editor's text stands in for the file, keyed by its canonical path, so
    // the resolver reads the same front matter the render does (F4-D4).
    let native: Arc<dyn SystemRuntime> = Arc::new(crate::render_flags::native_runtime());
    let active = native
        .canonicalize(&job.abs_path)
        .unwrap_or_else(|_| job.abs_path.clone());
    let overlay = Arc::new(RenderOverlay {
        files: job
            .content
            .iter()
            .map(|text| (active.clone(), text.clone().into_bytes()))
            .collect(),
        metadata: job.metadata.clone(),
    });
    let runtime: Arc<dyn SystemRuntime> = overlay.apply(native);
    let (base_key, typst_target) = render_format_and_target(&job.format_key);
    let format = match Format::from_format_string(base_key) {
        Ok(f) => f,
        Err(e) => return RenderOutcome::Failed(RenderFailed::message(e)),
    };
    let guard = match PreviewOutDir::create(&job.abs_path, runtime.as_ref()) {
        Ok(g) => g,
        Err(e) => return RenderOutcome::Failed(RenderFailed::message(e.to_string())),
    };
    let project = guard.project();

    let request = resolve_render_scope(
        project,
        &RenderMode::ActivePage(active.clone()),
        runtime.as_ref(),
    );
    let resolution = match resolve_book_scope(project, &active, &request, &format, runtime.as_ref())
    {
        Ok(r) => r,
        Err(e) => return RenderOutcome::Failed(failure_from(e)),
    };

    let options = RenderToFileOptions {
        output_dir: Some(guard.path().to_path_buf()),
        quiet: true,
        execution_policy: ExecutionPolicy::None,
        source_override: job.content.clone().map(String::into_bytes),
        cancellation: Some(job.cancellation.clone()),
        child_deadline: Some(CHILD_DEADLINE),
        capture_materialize_dir: Some(guard.path().to_path_buf()),
        typst_target,
        captures: job.captures.clone(),
        attribution_json: job.attribution_json.clone(),
        ..Default::default()
    };

    let seam = render_seam();
    let mut notices = json_all(&resolution.diagnostics, &SourceContext::new());
    let mut book_chapters = resolution.membership.map(|m| m.chapters);
    let (file, unexecuted_cells, mut diagnostics) = match resolution.scope {
        ResolvedBookScope::Book => {
            let request = BookDownloadRequest {
                path: active.clone(),
                content: job.content,
                format: job.format_key.clone(),
                typst_target,
                request_dir: guard.path().to_path_buf(),
                hub_root: job.hub_root.clone(),
                captures: job.all_captures,
                cancellation: job.cancellation.clone(),
                metadata: job.metadata.clone(),
            };
            match book_download().render(request) {
                Ok(out) => {
                    book_chapters = Some(out.chapters);
                    (out.file, out.unexecuted_cells, out.diagnostics)
                }
                Err(BookDownloadError::Busy) => return RenderOutcome::Busy,
                Err(BookDownloadError::Cancelled) => return RenderOutcome::Superseded,
                Err(BookDownloadError::Failed(failed)) => return RenderOutcome::Failed(failed),
                Err(BookDownloadError::Unavailable(m)) => return RenderOutcome::Unavailable(m),
            }
        }
        scope => {
            let result = match scope {
                ResolvedBookScope::Chapter => {
                    seam.render_chapter(project, &active, &format, &options, runtime)
                }
                _ => seam.render_document(&active, base_key, &options, project, runtime),
            };
            let result = match result {
                Ok(r) => r,
                Err(e) => return RenderOutcome::Failed(failure_from(e)),
            };
            let out = &result.render_output;
            let json = json_all(&out.diagnostics, &out.source_context);
            let has_errors = out
                .diagnostics
                .iter()
                .any(|d| matches!(d.kind, DiagnosticKind::Error));
            if has_errors {
                return RenderOutcome::Failed(RenderFailed {
                    error: None,
                    diagnostics: json,
                });
            }
            (result.output_path, result.unexecuted_cells, json)
        }
    };
    notices.append(&mut diagnostics);

    let bytes = match std::fs::read(&file) {
        Ok(b) => b,
        Err(e) => {
            return RenderOutcome::Failed(RenderFailed::message(format!(
                "could not read the rendered file {}: {e}",
                file.display()
            )));
        }
    };
    let file_name = file.file_name().map_or_else(
        || format!("download.{base_key}"),
        |n| n.to_string_lossy().into_owned(),
    );
    drop(guard);
    RenderOutcome::Ok(RenderedFile {
        file_name,
        bytes,
        warnings: notices,
        unexecuted_cells,
        book: book_chapters.map(|chapters| BookStats {
            scope: scope_name(resolution.scope),
            chapters,
        }),
    })
}

fn encoded_header(value: &str) -> Option<HeaderValue> {
    HeaderValue::from_str(&utf8_percent_encode(value, NON_ALPHANUMERIC).to_string()).ok()
}

/// `attachment` with an ASCII fallback `filename` and the exact name as RFC 5987 `filename*`.
pub fn content_disposition(file_name: &str) -> String {
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

/// Encode an outcome. `mime` is the 200's content type; `attachment` adds
/// `Content-Disposition` (the `/render` route; `/pdf` is inline).
pub fn outcome_response(outcome: RenderOutcome, mime: &'static str, attachment: bool) -> Response {
    match outcome {
        RenderOutcome::Ok(rendered) => {
            let mut response = (StatusCode::OK, rendered.bytes).into_response();
            let headers = response.headers_mut();
            headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(mime));
            if attachment
                && let Ok(v) = HeaderValue::from_str(&content_disposition(&rendered.file_name))
            {
                headers.insert(header::CONTENT_DISPOSITION, v);
            }
            if !rendered.warnings.is_empty()
                && let Ok(json) = serde_json::to_string(&rendered.warnings)
                && let Some(v) = encoded_header(&json)
            {
                headers.insert(DIAGNOSTICS_HEADER, v);
            }
            let mut stats = serde_json::json!({ "unexecuted_cells": rendered.unexecuted_cells });
            if let Some(book) = rendered.book {
                stats["book"] = serde_json::json!(book);
            }
            if let Some(v) = encoded_header(&stats.to_string()) {
                headers.insert(STATS_HEADER, v);
            }
            response
        }
        RenderOutcome::Failed(f) => (StatusCode::UNPROCESSABLE_ENTITY, Json(f)).into_response(),
        RenderOutcome::Superseded => (
            StatusCode::CONFLICT,
            Json(serde_json::json!({ "superseded": true })),
        )
            .into_response(),
        RenderOutcome::Busy => (
            StatusCode::CONFLICT,
            Json(serde_json::json!({ "busy": true })),
        )
            .into_response(),
        RenderOutcome::Unavailable(message) => (
            StatusCode::NOT_IMPLEMENTED,
            Json(serde_json::json!({ "error": message })),
        )
            .into_response(),
    }
}

/// Cancels the render when the handler future is dropped (the client went away).
pub struct CancelOnDrop(pub Cancellation);

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.cancel();
    }
}

/// The path-and-hub half every route shares: the index must know the path (a
/// 400 otherwise) and the hub must have a project root. `Err` is the response.
pub fn locate(ctx: &SharedContext, rel_path: &str) -> Result<(PathBuf, PathBuf), Response> {
    if !ctx.index().has_file(rel_path) {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("Path '{rel_path}' is not in the project index"),
        )
            .into_response());
    }
    let Some(project_root) = ctx.storage().project_root().map(Path::to_path_buf) else {
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            "no project root (standalone mode?)",
        )
            .into_response());
    };
    // The sidecar keys are relative to the hub root: a canonical project dir
    // against a non-canonical root would silently produce a wrong key.
    let hub_root = project_root.canonicalize().unwrap_or(project_root);
    let abs_path = hub_root.join(rel_path);
    Ok((abs_path, hub_root))
}
