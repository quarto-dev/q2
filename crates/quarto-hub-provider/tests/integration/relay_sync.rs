//! Reproduce the real topology (bd-sfet3264): a relay server, peer A creates a
//! doc with files, peer B (like the provider) connects later and `find`s it.
//! This is different from `join.rs`, where the *server itself* creates the doc.
//!
//! Not ignored — fully local (bare samod acceptor + two client repos), no
//! network. If B ends up with 0 files, we've reproduced the provider's
//! "0 file(s)" bug in isolation.

use std::str::FromStr;
use std::time::Duration;

use quarto_hub::index::IndexDocument;
use samod::{BackoffConfig, DocumentId, NeverAnnounce, Repo};
use tokio::net::TcpListener;

async fn spawn_relay() -> (Repo, url::Url) {
    spawn_relay_with(Repo::build_tokio().load().await).await
}

async fn spawn_relay_never_announce() -> (Repo, url::Url) {
    spawn_relay_with(
        Repo::build_tokio()
            .with_announce_policy(NeverAnnounce)
            .load()
            .await,
    )
    .await
}

async fn spawn_relay_with(repo: Repo) -> (Repo, url::Url) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let ws_url: url::Url = format!("ws://{addr}").parse().unwrap();
    let acceptor = repo.make_acceptor(ws_url.clone()).expect("acceptor");
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
    (repo, ws_url)
}

async fn dial(url: &url::Url) -> Repo {
    let repo = Repo::build_tokio().load().await;
    let handle = repo
        .dial_websocket(url.clone(), BackoffConfig::default())
        .unwrap();
    tokio::time::timeout(Duration::from_secs(10), handle.established())
        .await
        .expect("established timeout")
        .expect("established failed");
    repo
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn peer_b_syncs_a_doc_created_by_peer_a_through_the_relay() {
    let (_relay, url) = spawn_relay().await;

    // Peer A: create the index + files, then keep its repo alive.
    let repo_a = dial(&url).await;
    let (index_a, doc_id) = IndexDocument::create(&repo_a).await.unwrap();
    index_a.add_file("index.qmd", "file-doc-1").unwrap();
    index_a.add_file("about.qmd", "file-doc-2").unwrap();

    // Give sync a moment to push A's doc to the relay.
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Peer B (like the provider): connect fresh and find A's doc.
    let repo_b = dial(&url).await;
    let _handle = repo_b
        .find(DocumentId::from_str(&doc_id).unwrap())
        .await
        .unwrap()
        .expect("B finds the doc");
    let index_b = IndexDocument::load(&repo_b, &doc_id)
        .await
        .unwrap()
        .unwrap();

    // Poll for the files to arrive.
    let mut last = 0;
    for _ in 0..40 {
        last = index_b.get_all_files().len();
        if last >= 2 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    assert_eq!(
        last, 2,
        "peer B never synced peer A's files through the relay (reproduces the provider bug)"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn peer_b_syncs_through_a_never_announce_relay() {
    // The real servers (quarto-hub, sync.automerge.org) do NOT proactively
    // announce docs to a connecting peer. If peer B still can't sync A's doc
    // here, we've pinned the provider's "0 file(s)" bug to a NeverAnnounce
    // relay — B must actively request/pull, not wait to be announced to.
    let (_relay, url) = spawn_relay_never_announce().await;

    let repo_a = dial(&url).await;
    let (index_a, doc_id) = IndexDocument::create(&repo_a).await.unwrap();
    index_a.add_file("index.qmd", "file-doc-1").unwrap();
    index_a.add_file("about.qmd", "file-doc-2").unwrap();
    tokio::time::sleep(Duration::from_millis(500)).await;

    let repo_b = dial(&url).await;
    let _handle = repo_b
        .find(DocumentId::from_str(&doc_id).unwrap())
        .await
        .unwrap()
        .expect("B finds the doc");
    let index_b = IndexDocument::load(&repo_b, &doc_id)
        .await
        .unwrap()
        .unwrap();

    let mut last = 0;
    for _ in 0..40 {
        last = index_b.get_all_files().len();
        if last >= 2 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    assert_eq!(last, 2, "peer B never synced through a NeverAnnounce relay");
}
