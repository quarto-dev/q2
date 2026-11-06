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

/// F8-D9 / 8.3c: a provider started with a `-M` executes with the overlay
/// (the engine receives the merged metadata, so the recorded output shows
/// it) but records an overlay-free `input_qmd`, the staleness key, so the
/// viewer's overlay-free recompute does not mark the capture stale.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_dash_m_reaches_execution_but_not_the_staleness_key() {
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

    let overlay = quarto_core::metadata_flag::overlay_from_metadata_flags(&[
        "subtitle:ZZOVERLAYSUB".to_string(),
    ])
    .unwrap();
    assert!(overlay.is_some());
    let provider = Provider::new_with_overlay(
        provider_repo,
        provider_index,
        "provider-actor",
        Arc::new(AlwaysAccept),
        Some(Arc::new(passthrough_registry())),
        overlay,
    );
    let ExecOutcome::Executed(doc_id) = provider
        .execute_once("doc.qmd")
        .await
        .expect("execution succeeds")
    else {
        panic!("expected Executed");
    };
    provider
        .flush_to_hub(&doc_id, Duration::from_secs(10))
        .await;

    let captures = fetch_captures(&server_repo, &doc_id).await;
    let markdown = captures[0]
        .result
        .get("markdown")
        .and_then(|v| v.as_str())
        .expect("recorded markdown");
    assert!(
        markdown.contains("ZZOVERLAYSUB"),
        "the engine did not receive the -M metadata: {markdown}"
    );
    assert!(
        !captures[0].input_qmd.contains("ZZOVERLAYSUB"),
        "input_qmd (the staleness key) must describe the source, not the -M: {}",
        captures[0].input_qmd
    );
    assert!(captures[0].input_qmd.contains("1 + 1"));
}

// ── EXR-5: the execution record under a `-M` overlay ────────────────────────

/// An engine that records every unit the stage listed, naming the languages it owns as
/// the Jupyter engine does (a record whose entry owns nothing cannot be judged fresh) (EXR-4's `RecordBuilder` over the stage's units and author texts).
struct RecordingEngine;

impl ExecutionEngine for RecordingEngine {
    fn name(&self) -> &str {
        "test-recording"
    }
    fn execute(
        &self,
        input: &str,
        ctx: &ExecutionContext,
    ) -> Result<ExecuteResult, ExecutionError> {
        use quarto_core::record::{EngineEntryInput, RecordBuilder, UnitResult};
        let mut result = ExecuteResult::passthrough(input);
        if ctx.want_record {
            let def = self.hash_definition();
            let err = |e: &dyn std::fmt::Display| {
                ExecutionError::execution_failed("test-recording", e.to_string())
            };
            let mut builder = RecordBuilder::new(self.name(), &def, &ctx.units, &ctx.author_texts)
                .map_err(|e| err(&e))?;
            for index in 0..ctx.units.len() {
                builder
                    .push(index, UnitResult::default())
                    .map_err(|e| err(&e))?;
            }
            result.record = Some(
                builder
                    .finish(EngineEntryInput {
                        owned_languages: ctx.owned_languages.clone(),
                        document_metadata: ctx.document_metadata_json(),
                        ..Default::default()
                    })
                    .map_err(|e| err(&e))?,
            );
        }
        Ok(result)
    }
    fn claims_language(&self, language: &str, _first_class: Option<&str>) -> LanguageClaim {
        if language == "test-recording" {
            LanguageClaim::Primary(1)
        } else {
            LanguageClaim::None
        }
    }
}

/// A `-M` reaches execution, but the stored record's hashes describe the source:
/// after `overlay_free_run` they equal an overlay-free run's, and `quarto_overlay`
/// notes the overlay's execution-relevant keys.
#[test]
fn provide_hub_with_overlay_stores_overlay_free_record_hashes() {
    use quarto_core::engine::preview_record::{compute_source_inputs, record_capture_run};
    use quarto_core::project::ProjectContext;
    use quarto_hub_provider::overlay_free_run;
    use quarto_system_runtime::{NativeRuntime, SystemRuntime};

    let tmp = tempfile::TempDir::new().unwrap();
    let input = tmp.path().join("doc.qmd");
    std::fs::write(
        &input,
        "---\nengine: test-recording\n---\n\n```{test-recording}\n1 + 1\n```\n",
    )
    .unwrap();
    let registry = {
        let mut registry = EngineRegistry::new();
        registry.register(Arc::new(RecordingEngine));
        Some(Arc::new(registry))
    };
    let overlay = quarto_core::metadata_flag::overlay_from_metadata_flags(&[
        "params:{n: 2}".to_string(),
        "subtitle:ZZOVERLAYSUB".to_string(),
    ])
    .unwrap()
    .expect("an overlay");

    let plain: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let overlaid = overlay.apply(plain.clone());

    let source_project = ProjectContext::discover(&input, plain.as_ref()).unwrap();
    let overlaid_project = ProjectContext::discover(&input, overlaid.as_ref()).unwrap();

    let plain_run = pollster::block_on(record_capture_run(
        &input,
        &source_project,
        plain.clone(),
        registry.clone(),
    ))
    .unwrap();
    let overlaid_run = pollster::block_on(record_capture_run(
        &input,
        &overlaid_project,
        overlaid,
        registry,
    ))
    .unwrap();
    let plain_record = plain_run.record.as_ref().expect("plain record");
    let overlaid_record = overlaid_run.record.as_ref().expect("overlaid record");
    assert_ne!(
        plain_record.metadata.quarto_execution_hash, overlaid_record.metadata.quarto_execution_hash,
        "the -M params key reaches the document hash"
    );
    assert!(overlaid_run.captures[0].input_qmd.contains("ZZOVERLAYSUB"));

    let source = pollster::block_on(compute_source_inputs(&input, &source_project, plain)).unwrap();
    let stored = overlay_free_run(overlaid_run, &source, &overlay).unwrap();

    assert!(!stored.captures[0].input_qmd.contains("ZZOVERLAYSUB"));
    let record = stored.record.expect("record kept");
    // A viewer with no overlay recomputes from the source and finds the record fresh
    // (EXR-15), even though the execution ran under `params: {n: 2}`.
    assert!(
        !quarto_core::record::record_is_stale(
            &record,
            &String::from_utf8_lossy(&source.input_qmd),
            &source.sugared_blocks,
            &source.metadata,
        )
        .unwrap(),
        "an overlay-free viewer sees the record as fresh"
    );
    assert_eq!(
        record.metadata.quarto_execution_hash,
        plain_record.metadata.quarto_execution_hash
    );
    assert_eq!(
        record.metadata.quarto_engines[0].document_hash,
        plain_record.metadata.quarto_engines[0].document_hash
    );
    assert_eq!(
        record.cells, plain_record.cells,
        "unit hashes are the same either way"
    );
    let noted = record
        .metadata
        .quarto_overlay
        .expect("the overlay is noted");
    assert_eq!(noted.get("params"), Some(&serde_json::json!({ "n": 2 })));
    assert!(!noted.contains_key("subtitle"), "{noted:?}");
}

/// The provenance bytes depend on the overlay's meaning, not on how the flag was spelled,
/// and a non-execution key never reaches them (EXR-15; moved from EXR-4).
#[test]
fn overlay_provenance_is_independent_of_flag_spelling() {
    use quarto_core::metadata_flag::overlay_from_metadata_flags;
    use quarto_core::record::overlay_difference;

    let record = quarto_core::record::read_record(include_str!(
        "../../../quarto-core/tests/fixtures/records/basic.json"
    ))
    .unwrap();
    let noted = |flags: &[&str]| {
        let flags: Vec<String> = flags.iter().map(|f| f.to_string()).collect();
        let overlay = overlay_from_metadata_flags(&flags).unwrap().unwrap();
        let diff = overlay_difference(overlay.metadata.as_ref().unwrap(), &record);
        serde_json::to_string(&diff).unwrap()
    };
    let flow = noted(&["params:{n: 2}", "title:ZZ"]);
    let json = noted(&["title:ZZ", "params:{\"n\": 2}"]);
    assert_eq!(
        flow, json,
        "flag order and YAML flow vs JSON are not meaningful"
    );
    assert!(
        !flow.contains("title"),
        "a non-execution key is absent: {flow}"
    );
}

/// A `jupyter` engine that claims `python` and panics if it is ever asked to run.
struct MustNotRun;

impl ExecutionEngine for MustNotRun {
    fn name(&self) -> &str {
        "jupyter"
    }
    fn execute(
        &self,
        _input: &str,
        _ctx: &ExecutionContext,
    ) -> Result<ExecuteResult, ExecutionError> {
        panic!("publishing an imported record must never run an engine");
    }
    fn claims_language(&self, language: &str, _first_class: Option<&str>) -> LanguageClaim {
        if language == "python" {
            LanguageClaim::Primary(1)
        } else {
            LanguageClaim::None
        }
    }
}

const PYTHON_QMD: &str = "---\ntitle: T\n---\n\n```{python}\nx = 1\n```\n";

/// A directory holding one Positron-format cache for `x = 1`. The document the provider sees
/// is materialized to a random temp path, so only the content-hash tier can find this file.
fn positron_cache_dir() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let file = serde_json::json!({
        "nbformat": 4, "nbformat_minor": 5,
        "metadata": {"quarto_source": "file:///somewhere/else.qmd", "quarto_cache_version": 1},
        "cells": [{
            "cell_type": "code", "execution_count": null,
            "metadata": {"quarto_content_hash": quarto_core::record::content_hash("x = 1")},
            "outputs": [{"output_type": "stream", "name": "stdout", "text": ["from positron\n"]}],
            "source": []
        }]
    });
    std::fs::write(dir.path().join("cache.ipynb"), file.to_string()).unwrap();
    dir
}

/// RIU-3a Phase D: publishing an imported record never calls an engine and never reaches the
/// consent gate. The provider is built with `AlwaysReject` (which would refuse any engine run)
/// and an engine that panics if run; the record still lands. With the writer gate off the seam
/// refuses, naming the variable, and nothing is written.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn publishing_an_imported_record_needs_no_consent_and_runs_no_engine() {
    let (server_repo, ws_url) = spawn_server().await;
    let file_id = create_text_doc(&server_repo, PYTHON_QMD).await;
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
    let mut registry = EngineRegistry::empty();
    registry.register(Arc::new(MustNotRun));
    let provider = Provider::new(
        provider_repo,
        provider_index,
        "provider-actor",
        Arc::new(AlwaysReject),
        Some(Arc::new(registry)),
    );
    let cache = positron_cache_dir();
    let source = quarto_core::record::RecordSource::Dir(cache.path().to_path_buf());

    // Gate off: refused, naming the variable; nothing written.
    let err = provider
        .publish_record_source("doc.qmd", &source, false)
        .await
        .unwrap_err();
    assert!(err.contains("QUARTO_ENGINE_RECORD_WRITE"), "{err}");
    assert!(server_index.get_capture("doc.qmd").is_none());

    // Gate on: published as a record document, fresh, with no engine run.
    let outcome = provider
        .publish_record_source("doc.qmd", &source, true)
        .await
        .unwrap();
    let ExecOutcome::Executed(doc_id) = outcome else {
        panic!("expected the record to be published")
    };
    let deadline = Instant::now() + Duration::from_secs(20);
    let cap = loop {
        if let Some(cap) = server_index.get_capture("doc.qmd") {
            break cap;
        }
        assert!(Instant::now() < deadline, "the sidecar never synced");
        tokio::time::sleep(Duration::from_millis(100)).await;
    };
    assert_eq!(cap.capture_doc_id, doc_id);
    assert_eq!(cap.staleness, Some(false));
    assert_eq!(cap.state, Some(CaptureState::Idle));

    // A document the cache does not cover has nothing to publish: an error, not an empty page.
    let empty = tempfile::tempdir().unwrap();
    let none = quarto_core::record::RecordSource::Dir(empty.path().to_path_buf());
    assert!(
        provider
            .publish_record_source("doc.qmd", &none, true)
            .await
            .is_err()
    );
    provider.stop().await;
}
