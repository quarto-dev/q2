//! Execute-on-request end-to-end test (bd-sfet3264, Phase 4a).
//!
//! A bare samod acceptor stands in for the hub; a seeded index points at a
//! passthrough-engine `.qmd`. A provider joins as a client peer over the real
//! `BearerDialer` transport, then an editor-side broadcast of an `exec/request`
//! on the index handle drives the provider to materialize the project, run the
//! (passthrough) engine, and write a capture binary doc + `CaptureRef` sidecar
//! — which syncs back to the server, exactly as an editor peer would observe.
//!
//! A second test asserts a rejecting consent gate writes nothing.

use std::io::Read as _;
use std::str::FromStr as _;
use std::sync::Arc;
use std::time::{Duration, Instant};

use automerge::{Automerge, ObjType, ROOT, transaction::Transactable};
use flate2::read::GzDecoder;
use quarto_core::engine::{
    EngineRegistry, ExecuteResult, ExecutionContext, ExecutionEngine, ExecutionError, LanguageClaim,
};
use quarto_hub::index::{CaptureState, IndexDocument};
use quarto_hub::resource::read_binary_content;
use quarto_hub_provider::{
    AlwaysAccept, AlwaysReject, ExecMessage, ExecOutcome, JoinConfig, Provider, StaticTokenSource,
    join,
};
use quarto_trace::EngineCapture;
use samod::{DocumentId, Repo};
use tokio::net::TcpListener;

/// A stand-in engine so the test doesn't need a real knitr/jupyter runtime. It
/// passes the input markdown through with a marker appended, which is enough
/// for `EngineExecutionStage` to emit an `EngineCapture`.
struct PassthroughEngine;

impl ExecutionEngine for PassthroughEngine {
    fn name(&self) -> &str {
        "test-passthrough"
    }
    fn execute(
        &self,
        input: &str,
        _ctx: &ExecutionContext,
    ) -> Result<ExecuteResult, ExecutionError> {
        let mut out = String::from(input);
        out.push_str("\n<!-- executed by provider -->\n");
        Ok(ExecuteResult::passthrough(&out))
    }

    fn claims_language(&self, language: &str, _first_class: Option<&str>) -> LanguageClaim {
        if language == "test-passthrough" {
            LanguageClaim::Primary(1)
        } else {
            LanguageClaim::None
        }
    }
}

fn passthrough_registry() -> EngineRegistry {
    let mut registry = EngineRegistry::new();
    registry.register(Arc::new(PassthroughEngine));
    registry
}

const PASSTHROUGH_QMD: &str =
    "---\nengine: test-passthrough\n---\n\n```{test-passthrough}\n1 + 1\n```\n";

async fn create_text_doc(repo: &Repo, text: &str) -> String {
    let mut doc = Automerge::new();
    doc.transact::<_, _, automerge::AutomergeError>(|tx| {
        let obj = tx.put_object(ROOT, "text", ObjType::Text)?;
        tx.update_text(&obj, text)?;
        Ok(())
    })
    .unwrap();
    repo.create(doc).await.unwrap().document_id().to_string()
}

/// Spin up a bare samod acceptor behind a tungstenite ws server and return the
/// server repo + its ws URL.
async fn spawn_server() -> (Repo, url::Url) {
    let server_repo = Repo::build_tokio().load().await;
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let ws_url: url::Url = format!("ws://{addr}").parse().unwrap();
    let acceptor = server_repo.make_acceptor(ws_url.clone()).expect("acceptor");

    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                break;
            };
            let acceptor = acceptor.clone();
            tokio::spawn(async move {
                if let Ok(ws) = tokio_tungstenite::accept_async(stream).await {
                    let _ = acceptor.accept_tungstenite(ws);
                }
            });
        }
    });

    (server_repo, ws_url)
}

/// Read + gunzip + deserialize a capture binary doc by id from `repo`.
async fn fetch_captures(repo: &Repo, doc_id: &str) -> Vec<EngineCapture> {
    let id = DocumentId::from_str(doc_id).unwrap();
    let handle = repo.find(id).await.unwrap().expect("capture doc synced");
    let gz = handle
        .with_document(|doc| read_binary_content(doc))
        .expect("capture doc has content");
    let mut json = Vec::new();
    GzDecoder::new(gz.as_slice())
        .read_to_end(&mut json)
        .unwrap();
    serde_json::from_slice(&json).unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn provider_executes_an_allowed_request_and_writes_a_capture() {
    let (server_repo, ws_url) = spawn_server().await;

    let file_id = create_text_doc(&server_repo, PASSTHROUGH_QMD).await;
    let (server_index, index_id) = IndexDocument::create(&server_repo).await.unwrap();
    server_index.add_file("doc.qmd", &file_id).unwrap();
    assert!(server_index.get_capture("doc.qmd").is_none());

    let (provider_repo, provider_index) = join(
        JoinConfig {
            server_ws_url: ws_url,
            index_doc_id: index_id.clone(),
            connect_timeout: Duration::from_secs(10),
        },
        Arc::new(StaticTokenSource::new("test-token")),
    )
    .await
    .expect("provider joins");

    let provider = Provider::new(
        provider_repo,
        provider_index,
        "provider-actor",
        Arc::new(AlwaysAccept),
        Some(Arc::new(passthrough_registry())),
    );
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let run_handle = tokio::spawn({
        let provider = Arc::clone(&provider);
        async move {
            provider
                .run(async move {
                    let _ = shutdown_rx.await;
                })
                .await;
        }
    });

    // Ephemeral messages are best-effort: re-broadcast the request until the
    // capture lands (or the deadline trips), the way a real "Run" button retry
    // would. The provider's in-flight guard collapses duplicate broadcasts.
    let request = ExecMessage::Request {
        path: "doc.qmd".into(),
        request_id: "req-1".into(),
        requester_actor_id: "editor-actor".into(),
    };
    let deadline = Instant::now() + Duration::from_secs(20);
    let capture = loop {
        if let Some(cap) = server_index.get_capture("doc.qmd")
            && cap.state == Some(CaptureState::Idle)
        {
            break cap;
        }
        assert!(Instant::now() < deadline, "capture never landed");
        server_index.handle().broadcast(request.to_cbor());
        tokio::time::sleep(Duration::from_millis(250)).await;
    };

    // The sidecar the editor observes: idle, fresh, pointing at a real doc.
    assert_eq!(capture.state, Some(CaptureState::Idle));
    assert_eq!(capture.staleness, Some(false));
    assert_eq!(capture.last_error, None);
    assert!(!capture.capture_doc_id.is_empty());

    // The capture binary doc synced back and holds the passthrough engine's
    // recorded output.
    let captures = fetch_captures(&server_repo, &capture.capture_doc_id).await;
    assert_eq!(captures.len(), 1);
    assert_eq!(captures[0].engine_name, "test-passthrough");

    let _ = shutdown_tx.send(());
    let _ = run_handle.await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn one_shot_executes_once_and_flushes_to_the_hub() {
    // The one-shot CLI path: execute_once → flush_to_hub (a real
    // they-have-our-changes confirmation, not a fixed sleep) → the capture doc
    // and sidecar are present on the server → stop cleanly.
    let (server_repo, ws_url) = spawn_server().await;

    let file_id = create_text_doc(&server_repo, PASSTHROUGH_QMD).await;
    let (server_index, index_id) = IndexDocument::create(&server_repo).await.unwrap();
    server_index.add_file("doc.qmd", &file_id).unwrap();
    assert!(server_index.get_capture("doc.qmd").is_none());

    let (provider_repo, provider_index) = join(
        JoinConfig {
            server_ws_url: ws_url,
            index_doc_id: index_id.clone(),
            connect_timeout: Duration::from_secs(10),
        },
        Arc::new(StaticTokenSource::new("test-token")),
    )
    .await
    .expect("provider joins");

    let provider = Provider::new(
        provider_repo,
        provider_index,
        "provider-actor",
        Arc::new(AlwaysAccept),
        Some(Arc::new(passthrough_registry())),
    );

    // Execute the single document once (no beacon, no request channel).
    let outcome = provider
        .execute_once("doc.qmd")
        .await
        .expect("execution succeeds");
    let ExecOutcome::Executed(capture_doc_id) = outcome else {
        panic!("expected Executed, got {outcome:?}");
    };

    // Block until the hub has our changes (the real flush), then confirm both
    // the sidecar and the capture binary doc are present on the server.
    provider
        .flush_to_hub(&capture_doc_id, Duration::from_secs(10))
        .await;

    // After the flush confirmation the server's index reflects the sidecar.
    // Allow a brief settle for the server handle to surface the merged change.
    let deadline = Instant::now() + Duration::from_secs(5);
    let cap = loop {
        if let Some(cap) = server_index.get_capture("doc.qmd") {
            break cap;
        }
        assert!(
            Instant::now() < deadline,
            "sidecar never reached the server"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    };
    assert_eq!(cap.state, Some(CaptureState::Idle));
    assert_eq!(cap.capture_doc_id, capture_doc_id);

    let captures = fetch_captures(&server_repo, &capture_doc_id).await;
    assert_eq!(captures.len(), 1);
    assert_eq!(captures[0].engine_name, "test-passthrough");

    // One-shot then stops the repo cleanly.
    provider.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn rejected_request_writes_no_capture() {
    let (server_repo, ws_url) = spawn_server().await;

    let file_id = create_text_doc(&server_repo, PASSTHROUGH_QMD).await;
    let (server_index, index_id) = IndexDocument::create(&server_repo).await.unwrap();
    server_index.add_file("doc.qmd", &file_id).unwrap();

    let (provider_repo, provider_index) = join(
        JoinConfig {
            server_ws_url: ws_url,
            index_doc_id: index_id.clone(),
            connect_timeout: Duration::from_secs(10),
        },
        Arc::new(StaticTokenSource::new("test-token")),
    )
    .await
    .expect("provider joins");

    let provider = Provider::new(
        provider_repo,
        provider_index,
        "provider-actor",
        Arc::new(AlwaysReject),
        Some(Arc::new(passthrough_registry())),
    );
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let run_handle = tokio::spawn({
        let provider = Arc::clone(&provider);
        async move {
            provider
                .run(async move {
                    let _ = shutdown_rx.await;
                })
                .await;
        }
    });

    // Broadcast the request repeatedly for a bounded window; a provider whose
    // consent gate rejects must never write a capture.
    let request = ExecMessage::Request {
        path: "doc.qmd".into(),
        request_id: "req-reject".into(),
        requester_actor_id: "editor-actor".into(),
    };
    let until = Instant::now() + Duration::from_secs(3);
    while Instant::now() < until {
        server_index.handle().broadcast(request.to_cbor());
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    assert!(
        server_index.get_capture("doc.qmd").is_none(),
        "a rejecting consent gate must not write a capture"
    );

    let _ = shutdown_tx.send(());
    let _ = run_handle.await;
}
