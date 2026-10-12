//! Phase 1 (bd-mfapk5yl) of the UDS transport plan
//! (claude-notes/plans/2026-10-06-hub-unix-domain-socket-transport.md):
//! transport-parity tests — the *same* sync test bodies run over both
//! transports, so any behavioral delta between TCP and UDS fails one
//! arm. (Mirrored copies drift; fixtures get a transport knob instead.)
//!
//! The planned client work (a shipped `UnixDialer`, `--peer unix:///…`
//! peering, the upgrade-request capture-golden) was descoped
//! (bd-9mpnaslv): UDS mode is server-side only, fronted by an operator
//! proxy. The UDS arm below dials through the test-only
//! [`support::UnixWsDialer`].
#![cfg(unix)]

use std::sync::Arc;
use std::time::{Duration, Instant};

use automerge::{Automerge, ObjType, ROOT, ReadDoc, transaction::Transactable};
use samod::{BackoffConfig, DocHandle, DocumentId, Repo};
use url::Url;

use crate::support::{MockOidcProvider, TestHub, TestHubBuilder, Transport, UnixWsDialer};

// ── document helpers ──────────────────────────────────────────────

/// A document with a single text object at ROOT.text, mirroring
/// sync.rs's `create_doc_with_text`.
fn doc_with_text(content: &str) -> Automerge {
    let mut doc = Automerge::new();
    doc.transact::<_, _, automerge::AutomergeError>(|tx| {
        let text_obj = tx.put_object(ROOT, "text", ObjType::Text)?;
        tx.update_text(&text_obj, content)?;
        Ok(())
    })
    .unwrap();
    doc
}

fn read_text(handle: &DocHandle) -> String {
    handle.with_document(|doc| {
        let (_, text_obj) = doc.get(ROOT, "text").unwrap().unwrap();
        doc.text(&text_obj).unwrap()
    })
}

/// Poll `repo.find` until the document appears (sync propagation is
/// asynchronous) or the budget runs out.
async fn find_with_deadline(repo: &Repo, id: &DocumentId, budget: Duration) -> DocHandle {
    let deadline = Instant::now() + budget;
    loop {
        if let Ok(Some(handle)) = repo.find(id.clone()).await {
            return handle;
        }
        assert!(
            Instant::now() < deadline,
            "document {id} not found within {budget:?}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

// ── dial helper ───────────────────────────────────────────────────

/// Dial a hub from a fresh client repo over the hub's transport. The
/// UDS arm goes through the test-only `UnixWsDialer`, so every parity
/// test exercises the socket's sync path (not just its handshake).
async fn dial_hub(repo: &Repo, hub: &TestHub) {
    let handle = match hub.transport {
        Transport::Tcp => repo
            .dial_websocket(Url::parse(&hub.ws_url()).unwrap(), BackoffConfig::default())
            .unwrap(),
        Transport::Unix => {
            let dialer = Arc::new(UnixWsDialer::new(hub.socket_path.clone().unwrap()));
            repo.dial(BackoffConfig::default(), dialer).unwrap()
        }
    };
    tokio::time::timeout(Duration::from_secs(10), handle.established())
        .await
        .expect("dial established timeout")
        .expect("dial established");
}

// ── transport matrix: WS handshake + document sync ────────────────

/// The parity body, written once: a creator repo pushes a document
/// through the hub and a second, independent repo reads it back over
/// the same transport. Proves handshake AND bidirectional sync.
async fn ws_handshake_and_doc_sync_body(transport: Transport) {
    let provider = MockOidcProvider::start().await;
    let hub = TestHubBuilder::new()
        .auth_disabled()
        .transport(transport)
        .start(&provider)
        .await;

    let creator = Repo::build_tokio().load().await;
    dial_hub(&creator, &hub).await;
    let handle = creator.create(doc_with_text("parity ✓")).await.unwrap();
    let doc_id = handle.document_id();

    let reader = Repo::build_tokio().load().await;
    dial_hub(&reader, &hub).await;
    let found = find_with_deadline(&reader, doc_id, Duration::from_secs(10)).await;
    assert_eq!(read_text(&found), "parity ✓");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ws_handshake_and_doc_sync_over_tcp() {
    ws_handshake_and_doc_sync_body(Transport::Tcp).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ws_handshake_and_doc_sync_over_unix_socket() {
    ws_handshake_and_doc_sync_body(Transport::Unix).await;
}
