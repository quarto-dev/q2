//! A bare in-process hub: a samod repo behind a tungstenite acceptor, with
//! helpers to author documents on the hub side, the way an editor would.

use std::future::Future;
use std::time::Duration;

use automerge::{Automerge, ObjType, ROOT, ReadDoc, transaction::Transactable};
use quarto_hub::index::IndexDocument;
use samod::{DocHandle, DocumentId, Repo};
use tokio::net::TcpListener;

/// A samod repo accepting websocket peers at the returned `ws://` URL.
pub async fn spawn_hub() -> (Repo, url::Url) {
    let repo = Repo::build_tokio().load().await;
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let ws_url: url::Url = format!("ws://{addr}/ws").parse().unwrap();
    let acceptor = repo.make_acceptor(ws_url.clone()).unwrap();
    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            let acceptor = acceptor.clone();
            tokio::spawn(async move {
                if let Ok(ws) = tokio_tungstenite::accept_async(stream).await {
                    let _ = acceptor.accept_tungstenite(ws);
                }
            });
        }
    });
    (repo, ws_url)
}

/// Create a text file document holding `text`; returns its document id.
pub async fn create_text_doc(repo: &Repo, text: &str) -> String {
    let mut doc = Automerge::new();
    doc.transact::<_, _, automerge::AutomergeError>(|tx| {
        let obj = tx.put_object(ROOT, "text", ObjType::Text)?;
        tx.update_text(&obj, text)?;
        Ok(())
    })
    .unwrap();
    repo.create(doc).await.unwrap().document_id().to_string()
}

/// Replace the text of the file document `doc_id` on the hub side.
pub async fn set_text(repo: &Repo, doc_id: &str, text: &str) {
    let handle = find(repo, doc_id).await;
    let text = text.to_string();
    handle.with_document(|doc| {
        doc.transact::<_, _, automerge::AutomergeError>(|tx| {
            let (_, obj) = tx.get(ROOT, "text")?.expect("a text document");
            tx.update_text(&obj, &text)?;
            Ok(())
        })
        .unwrap();
    });
}

/// A project: an index document listing `files` as `(path, text)` pairs.
/// Returns the index and each file's document id, in the order given.
pub async fn create_project(repo: &Repo, files: &[(&str, &str)]) -> (IndexDocument, Vec<String>) {
    let (index, _) = IndexDocument::create(repo).await.unwrap();
    let mut ids = Vec::new();
    for (path, text) in files {
        let id = create_text_doc(repo, text).await;
        index.add_file(path, &id).unwrap();
        ids.push(id);
    }
    (index, ids)
}

pub async fn find(repo: &Repo, doc_id: &str) -> DocHandle {
    let id: DocumentId = doc_id.parse().unwrap();
    repo.find(id)
        .await
        .unwrap()
        .expect("document exists on the hub")
}

/// Poll `check` until it returns `Some`, or panic after `timeout`.
/// Sync is eventually consistent, so a test waits for the state it expects
/// rather than sleeping for a guessed duration.
pub async fn eventually<T, F, Fut>(timeout: Duration, what: &str, mut check: F) -> T
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Option<T>>,
{
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        if let Some(value) = check().await {
            return value;
        }
        if tokio::time::Instant::now() > deadline {
            panic!("timed out waiting for {what}");
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}
