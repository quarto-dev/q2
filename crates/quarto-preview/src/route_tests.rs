//! Route-level tests for Plan 9: `/api/preview/pdf` and `/api/preview/render`
//! driven against a real hub context with the render seam swapped for a fake,
//! so supersede and scope tests do not race real typst. The seams are
//! process-wide statics, so a test that installs one relies on nextest running
//! every test in its own process.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::Json;
use axum::extract::State;
use axum::http::{StatusCode, header};
use axum::response::Response;
use base64::Engine as _;
use quarto_core::QuartoError;
use quarto_core::engine::ExecutionPolicy;
use quarto_core::error::ParseError;
use quarto_core::format::Format;
use quarto_core::pipeline::RenderOutput;
use quarto_core::project::ProjectContext;
use quarto_core::render_to_file::{RenderToFileOptions, RenderToFileResult, TypstTarget};
use quarto_error_reporting::DiagnosticMessageBuilder;
use quarto_hub::HubContext;
use quarto_hub::context::{HubConfig, SharedContext};
use quarto_hub::index::{CaptureRef, CaptureState};
use quarto_hub::storage::StorageManager;
use quarto_source_map::SourceContext;
use quarto_system_runtime::SystemRuntime;
use quarto_trace::{CaptureFile, EngineCapture};
use tempfile::TempDir;

use crate::pdf_preview::{PdfRequest, pdf_handler};
use crate::render_download::{RenderRequest, render_handler};
use crate::render_helper::{
    BookDownload, BookDownloadError, BookDownloadOutput, BookDownloadRequest, RenderSeam,
    ScopeParam, set_book_download, set_render_seam,
};

// ---- fixtures ------------------------------------------------------------

async fn build_ctx(files: &[(&str, &str)]) -> (TempDir, SharedContext) {
    let project = TempDir::with_prefix("p9-test-").unwrap();
    let root = project.path().canonicalize().unwrap();
    for (name, content) in files {
        let path = root.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }
    let storage = StorageManager::new(&root).unwrap();
    let ctx = Arc::new(
        HubContext::new(storage, HubConfig::default())
            .await
            .unwrap(),
    );
    (project, ctx)
}

const BOOK: &[(&str, &str)] = &[
    (
        "_quarto.yml",
        "project:\n  type: book\nbook:\n  title: B\n  chapters:\n    - index.qmd\n    - one.qmd\n",
    ),
    ("index.qmd", "---\ntitle: Index\n---\n\nHome.\n"),
    ("one.qmd", "---\ntitle: One\n---\n\nChapter one.\n"),
    ("other.qmd", "---\ntitle: Other\n---\n\nNot in the book.\n"),
];

/// What the fake saw.
#[derive(Debug, Clone)]
struct Call {
    kind: &'static str,
    format: String,
    source: Option<Vec<u8>>,
    captures: usize,
    typst_target: Option<TypstTarget>,
    attribution: Option<String>,
    materialize_is_output: bool,
    policy_none: bool,
    has_cancellation: bool,
    has_deadline: bool,
    output_dir: PathBuf,
}

#[derive(Default)]
struct FakeSeam {
    calls: Mutex<Vec<Call>>,
    started: AtomicUsize,
    /// The first `hold_calls` renders wait for `release` (or cancellation).
    hold_calls: usize,
    release: AtomicBool,
    ignore_cancel: bool,
    unexecuted: usize,
    fail_parse: bool,
}

impl FakeSeam {
    fn run(
        &self,
        kind: &'static str,
        input: &Path,
        format: &str,
        options: &RenderToFileOptions,
    ) -> quarto_core::Result<RenderToFileResult> {
        let n = self.started.fetch_add(1, Ordering::SeqCst);
        let output_dir = options.output_dir.clone().expect("output dir");
        self.calls.lock().unwrap().push(Call {
            kind,
            format: format.to_string(),
            source: options.source_override.clone(),
            captures: options.captures.len(),
            typst_target: options.typst_target,
            attribution: options.attribution_json.clone(),
            materialize_is_output: options.capture_materialize_dir.as_deref()
                == Some(output_dir.as_path()),
            policy_none: matches!(options.execution_policy, ExecutionPolicy::None),
            has_cancellation: options.cancellation.is_some(),
            has_deadline: options.child_deadline.is_some(),
            output_dir: output_dir.clone(),
        });
        if n < self.hold_calls {
            loop {
                let cancelled = options
                    .cancellation
                    .as_ref()
                    .is_some_and(|c| c.is_cancelled());
                if cancelled && !self.ignore_cancel {
                    return Err(QuartoError::other("cancelled"));
                }
                if self.release.load(Ordering::SeqCst) {
                    break;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
        }
        if self.fail_parse {
            let diagnostic = DiagnosticMessageBuilder::error("boom")
                .problem("the document is broken")
                .build();
            return Err(QuartoError::Parse(ParseError::new(
                vec![diagnostic],
                SourceContext::new(),
            )));
        }
        let ext = match options.typst_target {
            Some(TypstTarget::Pdf) => "pdf",
            Some(TypstTarget::Source) => "typ",
            None => match format {
                "typst" => "pdf",
                other => other,
            },
        };
        let stem = input.file_stem().unwrap().to_string_lossy().into_owned();
        let output_path = output_dir.join(format!("{stem}.{ext}"));
        let bytes: &[u8] = if ext == "pdf" { b"%PDF-fake" } else { b"fake" };
        std::fs::write(&output_path, bytes).unwrap();
        Ok(RenderToFileResult {
            input_path: input.to_path_buf(),
            output_path,
            resources_dir: output_dir,
            render_output: RenderOutput {
                html: String::new(),
                diagnostics: Vec::new(),
                source_context: SourceContext::new(),
                execution_skipped: false,
            },
            resource_report: Default::default(),
            unexecuted_cells: self.unexecuted,
        })
    }
}

impl RenderSeam for Arc<FakeSeam> {
    fn render_document(
        &self,
        input: &Path,
        format: &str,
        options: &RenderToFileOptions,
        _project: &ProjectContext,
        _runtime: Arc<dyn SystemRuntime>,
    ) -> quarto_core::Result<RenderToFileResult> {
        self.run("document", input, format, options)
    }

    fn render_chapter(
        &self,
        _project: &ProjectContext,
        abs_path: &Path,
        format: &Format,
        options: &RenderToFileOptions,
        _runtime: Arc<dyn SystemRuntime>,
    ) -> quarto_core::Result<RenderToFileResult> {
        self.run("chapter", abs_path, &format.identifier.to_string(), options)
    }
}

fn install(seam: FakeSeam) -> Arc<FakeSeam> {
    let seam = Arc::new(seam);
    set_render_seam(Some(Arc::new(seam.clone())));
    seam
}

fn pdf_req(path: &str, content: Option<&str>, scope: Option<ScopeParam>) -> Json<PdfRequest> {
    Json(PdfRequest {
        path: path.to_string(),
        content: content.map(str::to_string),
        scope,
        attribution_json: None,
    })
}

fn render_req(
    path: &str,
    format: &str,
    content: Option<&str>,
    scope: Option<ScopeParam>,
) -> Json<RenderRequest> {
    Json(RenderRequest {
        path: path.to_string(),
        format: format.to_string(),
        content: content.map(str::to_string),
        scope,
        attribution_json: None,
    })
}

async fn body_bytes(response: Response) -> Vec<u8> {
    axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap()
        .to_vec()
}

fn decoded_header(response: &Response, name: &str) -> Option<String> {
    let value = response.headers().get(name)?.to_str().unwrap();
    Some(
        percent_encoding::percent_decode_str(value)
            .decode_utf8()
            .unwrap()
            .into_owned(),
    )
}

fn preview_out_is_empty(root: &Path) -> bool {
    let dir = root.join(".quarto/preview-out");
    !dir.exists()
        || std::fs::read_dir(dir)
            .unwrap()
            .filter_map(Result::ok)
            .all(|e| e.file_name() == ".gitignore")
}

fn root_of(project: &TempDir) -> PathBuf {
    project.path().canonicalize().unwrap()
}

async fn wait_until(what: &str, mut cond: impl FnMut() -> bool) {
    for _ in 0..1000 {
        if cond() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    panic!("timed out waiting for {what}");
}

// ---- /api/preview/pdf ----------------------------------------------------

#[tokio::test]
async fn pdf_is_inline_with_stats_and_the_typst_pdf_target() {
    let (project, ctx) = build_ctx(&[("doc.qmd", "---\ntitle: T\n---\n\nHi\n")]).await;
    let fake = install(FakeSeam {
        unexecuted: 3,
        ..Default::default()
    });
    let response = pdf_handler(State(ctx), pdf_req("doc.qmd", None, None)).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CONTENT_TYPE], "application/pdf");
    assert!(
        response
            .headers()
            .get(header::CONTENT_DISPOSITION)
            .is_none(),
        "the inline route has no Content-Disposition"
    );
    assert_eq!(
        decoded_header(&response, "x-q2-stats").as_deref(),
        Some(r#"{"unexecuted_cells":3}"#)
    );
    assert_eq!(&body_bytes(response).await[..4], b"%PDF");

    let calls = fake.calls.lock().unwrap();
    assert_eq!(calls.len(), 1);
    let call = &calls[0];
    assert_eq!(call.kind, "document");
    assert_eq!(call.format, "typst", "typst-pdf renders the typst format");
    assert_eq!(call.typst_target, Some(TypstTarget::Pdf));
    assert!(call.policy_none, "no kernel launches");
    assert!(call.has_cancellation && call.has_deadline);
    assert!(call.materialize_is_output, "plots go to the request dir");
    assert!(
        call.output_dir
            .starts_with(root_of(&project).join(".quarto/preview-out")),
        "the request directory is under the project: {:?}",
        call.output_dir
    );
    drop(calls);
    assert!(
        preview_out_is_empty(&root_of(&project)),
        "the request directory is gone after the render"
    );
}

#[tokio::test]
async fn content_is_the_editor_text_and_a_missing_field_is_not_blank() {
    let (_project, ctx) = build_ctx(&[("doc.qmd", "disk\n")]).await;
    let fake = install(FakeSeam::default());
    for content in [Some("editor text"), None, Some("")] {
        let response = pdf_handler(State(ctx.clone()), pdf_req("doc.qmd", content, None)).await;
        assert_eq!(response.status(), StatusCode::OK);
    }
    let calls = fake.calls.lock().unwrap();
    assert_eq!(calls[0].source.as_deref(), Some(&b"editor text"[..]));
    assert_eq!(calls[1].source, None, "omitted content reads the disk copy");
    assert_eq!(
        calls[2].source.as_deref(),
        Some(&b""[..]),
        "an explicit empty string is an empty document"
    );
}

#[tokio::test]
async fn pdf_unknown_path_is_400() {
    let (_project, ctx) = build_ctx(&[("doc.qmd", "hi\n")]).await;
    for path in ["nonexistent.qmd", "../doc.qmd", "/etc/passwd"] {
        let response = pdf_handler(State(ctx.clone()), pdf_req(path, None, None)).await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{path}");
    }
}

#[tokio::test]
async fn a_broken_document_is_422_with_structured_diagnostics() {
    let (_project, ctx) = build_ctx(&[("doc.qmd", "hi\n")]).await;
    install(FakeSeam {
        fail_parse: true,
        ..Default::default()
    });
    let response = pdf_handler(State(ctx), pdf_req("doc.qmd", None, None)).await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let body: serde_json::Value = serde_json::from_slice(&body_bytes(response).await).unwrap();
    let diagnostics = body["diagnostics"].as_array().expect("diagnostics array");
    assert_eq!(diagnostics.len(), 1, "{body}");
    assert!(
        body.get("error").is_none(),
        "structured, not a message: {body}"
    );
}

#[tokio::test]
async fn the_pdf_route_is_mounted_and_has_an_8mb_body_limit() {
    use tower::ServiceExt;
    let (_project, ctx) = build_ctx(&[("doc.qmd", "hi\n")]).await;
    install(FakeSeam::default());
    let post = |uri: &str, content_len: usize| {
        let body = serde_json::json!({ "path": "doc.qmd", "content": "x".repeat(content_len) });
        axum::http::Request::builder()
            .method("POST")
            .uri(uri)
            .header(header::CONTENT_TYPE, "application/json")
            .body(axum::body::Body::from(body.to_string()))
            .unwrap()
    };
    let app = crate::extend_with_preview(axum::Router::new()).with_state(ctx);
    // Above axum's implicit 2 MB, below the route's 8 MB.
    let response = app
        .clone()
        .oneshot(post("/api/preview/pdf", 3 * 1024 * 1024))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let response = app
        .clone()
        .oneshot(post("/api/preview/pdf", 9 * 1024 * 1024))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    // The download route shares the limit.
    let big = serde_json::json!({
        "path": "doc.qmd", "format": "typst", "content": "x".repeat(3 * 1024 * 1024)
    });
    let request = axum::http::Request::builder()
        .method("POST")
        .uri("/api/preview/render")
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(big.to_string()))
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

// ---- supersede ------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn a_newer_request_supersedes_the_running_render() {
    let (_project, ctx) = build_ctx(&[("doc.qmd", "hi\n")]).await;
    let fake = install(FakeSeam {
        hold_calls: 1,
        ..Default::default()
    });
    let a = tokio::spawn(pdf_handler(
        State(ctx.clone()),
        pdf_req("doc.qmd", Some("a"), None),
    ));
    wait_until("the first render to start", || {
        fake.started.load(Ordering::SeqCst) == 1
    })
    .await;
    let b = tokio::spawn(pdf_handler(
        State(ctx.clone()),
        pdf_req("doc.qmd", Some("b"), None),
    ));

    let a = a.await.unwrap();
    assert_eq!(a.status(), StatusCode::CONFLICT);
    let body: serde_json::Value = serde_json::from_slice(&body_bytes(a).await).unwrap();
    assert_eq!(body, serde_json::json!({ "superseded": true }));
    let b = b.await.unwrap();
    assert_eq!(b.status(), StatusCode::OK);
    assert_eq!(fake.started.load(Ordering::SeqCst), 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_request_replaced_while_waiting_never_renders() {
    let (_project, ctx) = build_ctx(&[("doc.qmd", "hi\n")]).await;
    // The first render ignores its cancellation, so the slot stays held.
    let fake = install(FakeSeam {
        hold_calls: 1,
        ignore_cancel: true,
        ..Default::default()
    });
    let a = tokio::spawn(pdf_handler(
        State(ctx.clone()),
        pdf_req("doc.qmd", Some("a"), None),
    ));
    wait_until("the first render to start", || {
        fake.started.load(Ordering::SeqCst) == 1
    })
    .await;
    let b = tokio::spawn(pdf_handler(
        State(ctx.clone()),
        pdf_req("doc.qmd", Some("b"), None),
    ));
    tokio::time::sleep(Duration::from_millis(100)).await;
    let c = tokio::spawn(pdf_handler(
        State(ctx.clone()),
        pdf_req("doc.qmd", Some("c"), None),
    ));

    // B is told at once, while A still holds the slot.
    let b = tokio::time::timeout(Duration::from_secs(5), b)
        .await
        .expect("a replaced waiter answers without waiting for the running render")
        .unwrap();
    assert_eq!(b.status(), StatusCode::CONFLICT);

    fake.release.store(true, Ordering::SeqCst);
    assert_eq!(a.await.unwrap().status(), StatusCode::CONFLICT);
    assert_eq!(c.await.unwrap().status(), StatusCode::OK);
    let calls = fake.calls.lock().unwrap();
    let sources: Vec<_> = calls.iter().map(|c| c.source.clone().unwrap()).collect();
    assert_eq!(
        sources,
        vec![b"a".to_vec(), b"c".to_vec()],
        "B never rendered"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn different_paths_do_not_supersede_each_other() {
    let (_project, ctx) = build_ctx(&[("a.qmd", "a\n"), ("b.qmd", "b\n")]).await;
    let fake = install(FakeSeam {
        hold_calls: 1,
        ignore_cancel: true,
        ..Default::default()
    });
    let a = tokio::spawn(pdf_handler(
        State(ctx.clone()),
        pdf_req("a.qmd", None, None),
    ));
    wait_until("the first render to start", || {
        fake.started.load(Ordering::SeqCst) == 1
    })
    .await;
    let b = pdf_handler(State(ctx.clone()), pdf_req("b.qmd", None, None)).await;
    assert_eq!(b.status(), StatusCode::OK, "b renders while a is held");
    fake.release.store(true, Ordering::SeqCst);
    assert_eq!(a.await.unwrap().status(), StatusCode::OK);
}

// ---- /api/preview/render --------------------------------------------------

#[tokio::test]
async fn the_download_keys_map_to_formats_and_typst_targets() {
    let (_project, ctx) = build_ctx(&[("doc.qmd", "hi\n")]).await;
    let fake = install(FakeSeam::default());
    let mut seen = Vec::new();
    for key in ["typst", "typst-pdf", "odt", "docx"] {
        let response =
            render_handler(State(ctx.clone()), render_req("doc.qmd", key, None, None)).await;
        assert_eq!(response.status(), StatusCode::OK, "{key}");
        let disposition = response.headers()[header::CONTENT_DISPOSITION]
            .to_str()
            .unwrap()
            .to_string();
        let mime = response.headers()[header::CONTENT_TYPE]
            .to_str()
            .unwrap()
            .to_string();
        seen.push((key, disposition, mime));
    }
    assert!(seen[0].1.contains("filename=\"doc.typ\""), "{:?}", seen[0]);
    assert_eq!(seen[0].2, "text/plain");
    assert!(seen[1].1.contains("filename=\"doc.pdf\""), "{:?}", seen[1]);
    assert_eq!(seen[1].2, "application/pdf");
    assert!(seen[2].1.contains("filename=\"doc.odt\""), "{:?}", seen[2]);
    assert_eq!(seen[2].2, "application/vnd.oasis.opendocument.text");

    let calls = fake.calls.lock().unwrap();
    assert_eq!(calls[0].typst_target, Some(TypstTarget::Source));
    assert_eq!(calls[0].format, "typst");
    assert_eq!(calls[1].typst_target, Some(TypstTarget::Pdf));
    assert_eq!(calls[1].format, "typst");
    assert_eq!(calls[2].typst_target, None);
    assert_eq!(calls[2].format, "odt");
}

#[tokio::test]
async fn unknown_and_preview_formats_are_400() {
    let (_project, ctx) = build_ctx(&[("doc.qmd", "hi\n")]).await;
    install(FakeSeam::default());
    for key in ["html", "bogus", "pdf"] {
        let response =
            render_handler(State(ctx.clone()), render_req("doc.qmd", key, None, None)).await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{key}");
    }
}

#[tokio::test]
async fn the_attribution_payload_reaches_the_render() {
    let (_project, ctx) = build_ctx(&[("doc.qmd", "hi\n")]).await;
    let fake = install(FakeSeam::default());
    let mut request = render_req("doc.qmd", "docx", None, None);
    request.0.attribution_json = Some(r#"{"runs":[],"identities":{}}"#.to_string());
    let response = render_handler(State(ctx), request).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        fake.calls.lock().unwrap()[0].attribution.as_deref(),
        Some(r#"{"runs":[],"identities":{}}"#)
    );
}

// ---- captures (E10) -------------------------------------------------------

fn capture(input: &str) -> EngineCapture {
    EngineCapture {
        engine_name: "r".to_string(),
        input_qmd: input.to_string(),
        result: serde_json::json!({ "markdown": input }),
        files: vec![CaptureFile {
            path: "plot.png".to_string(),
            contents_base64: base64::engine::general_purpose::STANDARD.encode(b"png"),
        }],
    }
}

async fn record(ctx: &SharedContext, path: &str, captures: &[EngineCapture]) {
    let id = crate::capture_driver::write_capture_doc(ctx, path, captures)
        .await
        .expect("capture doc");
    ctx.index()
        .set_capture(
            path,
            &CaptureRef {
                capture_doc_id: id,
                staleness: None,
                state: Some(CaptureState::Idle),
                last_error: None,
            },
        )
        .unwrap();
}

#[tokio::test]
async fn captures_come_from_the_sidecar_entry_of_the_requested_path_only() {
    let (_project, ctx) = build_ctx(&[("a.qmd", "a\n"), ("b.qmd", "b\n")]).await;
    record(&ctx, "a.qmd", &[capture("a\n"), capture("a2\n")]).await;
    let fake = install(FakeSeam::default());
    for path in ["a.qmd", "b.qmd"] {
        let response = pdf_handler(State(ctx.clone()), pdf_req(path, None, None)).await;
        assert_eq!(response.status(), StatusCode::OK);
    }
    let calls = fake.calls.lock().unwrap();
    assert_eq!(calls[0].captures, 2, "a.qmd has its sidecar entry");
    assert_eq!(calls[1].captures, 0, "b.qmd must not use a.qmd's captures");
}

// ---- scope (E11) ----------------------------------------------------------

fn codes(response: &Response) -> String {
    decoded_header(response, "x-q2-diagnostics").unwrap_or_default()
}

#[tokio::test]
async fn chapter_scope_renders_the_chapter_alone_and_a_non_member_renders_as_a_page() {
    let (_project, ctx) = build_ctx(BOOK).await;
    let fake = install(FakeSeam::default());

    let response = pdf_handler(
        State(ctx.clone()),
        pdf_req("one.qmd", None, Some(ScopeParam::Chapter)),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(!codes(&response).contains("Q-5-41"));

    // Asking for the chapter of a non-member is just that page, no notice.
    let response = pdf_handler(
        State(ctx.clone()),
        pdf_req("other.qmd", None, Some(ScopeParam::Chapter)),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(!codes(&response).contains("Q-5-41"));

    // Nothing chosen: the non-member is delivered standalone with the notice.
    let response = pdf_handler(State(ctx.clone()), pdf_req("other.qmd", None, None)).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(codes(&response).contains("Q-5-41"), "{}", codes(&response));

    let calls = fake.calls.lock().unwrap();
    assert_eq!(calls[0].kind, "chapter");
    assert_eq!(calls[1].kind, "document");
    assert_eq!(calls[2].kind, "document");
}

#[tokio::test]
async fn the_book_seam_defaults_to_501() {
    let (_project, ctx) = build_ctx(BOOK).await;
    install(FakeSeam::default());
    // A member with no scope chosen consolidates the book.
    let response = pdf_handler(State(ctx), pdf_req("one.qmd", None, None)).await;
    assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED);
    let body: serde_json::Value = serde_json::from_slice(&body_bytes(response).await).unwrap();
    assert_eq!(
        body["error"],
        "book downloads are not available in this build"
    );
}

#[tokio::test]
async fn a_book_request_reaches_the_book_seam_with_every_capture() {
    struct Recorder(
        Mutex<
            Option<(
                PathBuf,
                PathBuf,
                PathBuf,
                String,
                Option<TypstTarget>,
                Vec<String>,
            )>,
        >,
    );
    impl BookDownload for Arc<Recorder> {
        fn render(
            &self,
            req: BookDownloadRequest,
        ) -> Result<BookDownloadOutput, BookDownloadError> {
            let mut keys: Vec<String> = req.captures.keys().cloned().collect();
            keys.sort();
            assert!(req.request_dir.is_dir());
            *self.0.lock().unwrap() = Some((
                req.path.clone(),
                req.request_dir.clone(),
                req.hub_root.clone(),
                req.format.clone(),
                req.typst_target,
                keys,
            ));
            let file = req.request_dir.join("book.pdf");
            std::fs::write(&file, b"%PDF-book").unwrap();
            Ok(BookDownloadOutput {
                file,
                chapters: 2,
                unexecuted_cells: 4,
                diagnostics: Vec::new(),
            })
        }
    }
    let (project, ctx) = build_ctx(BOOK).await;
    record(&ctx, "one.qmd", &[capture("one\n")]).await;
    install(FakeSeam::default());
    let recorder = Arc::new(Recorder(Mutex::new(None)));
    set_book_download(Some(Arc::new(recorder.clone())));

    let response = render_handler(
        State(ctx),
        render_req("one.qmd", "typst-pdf", None, Some(ScopeParam::Book)),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        decoded_header(&response, "x-q2-stats").as_deref(),
        Some(r#"{"unexecuted_cells":4}"#)
    );
    assert_eq!(body_bytes(response).await, b"%PDF-book");

    let root = root_of(&project);
    let (path, request_dir, hub_root, format, target, keys) =
        recorder.0.lock().unwrap().clone().expect("the seam ran");
    assert_eq!(path, root.join("one.qmd"));
    assert!(request_dir.starts_with(root.join(".quarto/preview-out")));
    assert_eq!(hub_root, root);
    assert_eq!(format, "typst-pdf");
    assert_eq!(target, Some(TypstTarget::Pdf));
    assert_eq!(keys, vec!["one.qmd".to_string()]);
    assert!(preview_out_is_empty(&root));
}

#[tokio::test]
async fn an_explicit_scope_for_a_page_format_is_ignored_with_a_notice() {
    let (_project, ctx) = build_ctx(BOOK).await;
    let fake = install(FakeSeam::default());

    let response = render_handler(
        State(ctx.clone()),
        render_req("one.qmd", "docx", None, Some(ScopeParam::Book)),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(codes(&response).contains("Q-5-42"), "{}", codes(&response));

    let response = render_handler(State(ctx), render_req("one.qmd", "docx", None, None)).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        !codes(&response).contains("Q-5-42"),
        "an absent scope is silent"
    );

    let calls = fake.calls.lock().unwrap();
    assert!(calls.iter().all(|c| c.kind == "chapter"), "{calls:?}");
}

#[test]
fn the_route_contract_constants() {
    assert_eq!(crate::render_helper::BODY_LIMIT, 8 * 1024 * 1024);
    assert_eq!(
        crate::render_helper::CHILD_DEADLINE,
        Duration::from_secs(120)
    );
}
