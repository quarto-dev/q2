//! Diagnostic probes (bd-sfet3264). Ignored by default — need network.
//!
//! probe_live_doc_sync: join an existing doc and watch its `files` map sync.
//!   PROBE_SERVER=wss://sync.automerge.org PROBE_DOC_ID=<id> [PROBE_NATIVE=1] \
//!     cargo nextest run -p quarto-hub-provider --test integration \
//!     probe_live_doc_sync --run-ignored all --no-capture
//!
//! probe_create_then_find: peer A (samod) creates + pushes a doc, peer B
//!   (samod) finds it — both against PROBE_SERVER. Isolates whether samod can
//!   round-trip a doc through the real server at all (vs. only failing on
//!   hub-client-created docs).
//!     PROBE_SERVER=wss://sync.automerge.org \
//!     cargo nextest run -p quarto-hub-provider --test integration \
//!     probe_create_then_find --run-ignored all --no-capture

use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use quarto_hub::index::IndexDocument;
use quarto_hub_provider::{BearerDialer, StaticTokenSource};
use samod::{BackoffConfig, DocumentId, Repo};

async fn dial_bearer(url: &url::Url) -> Repo {
    let repo = Repo::build_tokio().load().await;
    let dialer = Arc::new(BearerDialer::new(
        url.clone(),
        Arc::new(StaticTokenSource::new("dev")),
    ));
    let handle = repo.dial(BackoffConfig::default(), dialer).unwrap();
    tokio::time::timeout(Duration::from_secs(30), handle.established())
        .await
        .expect("established timeout")
        .expect("established failed");
    repo
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs network + a live PROBE_DOC_ID"]
async fn probe_live_doc_sync() {
    let server = std::env::var("PROBE_SERVER").expect("set PROBE_SERVER");
    let doc_id = std::env::var("PROBE_DOC_ID").expect("set PROBE_DOC_ID");
    let native = std::env::var("PROBE_NATIVE").is_ok();
    let url: url::Url = server.parse().unwrap();

    let repo = if native {
        eprintln!("dialing with samod native dial_websocket");
        let repo = Repo::build_tokio().load().await;
        let handle = repo.dial_websocket(url, BackoffConfig::default()).unwrap();
        tokio::time::timeout(Duration::from_secs(30), handle.established())
            .await
            .expect("established timeout")
            .expect("established failed");
        repo
    } else {
        eprintln!("dialing with BearerDialer");
        dial_bearer(&url).await
    };
    eprintln!("connection established");

    let id = DocumentId::from_str(&doc_id).unwrap();
    match repo.find(id).await.unwrap() {
        Some(_) => eprintln!("find returned Some(handle)"),
        None => {
            eprintln!("find returned None (doc not found)");
            return;
        }
    }
    let index = IndexDocument::load(&repo, &doc_id).await.unwrap().unwrap();
    for i in 0..40 {
        let files = index.get_all_files();
        eprintln!(
            "[t={:>5}ms] {} file(s): {:?}",
            i * 500,
            files.len(),
            files.keys().collect::<Vec<_>>()
        );
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs network (PROBE_SERVER)"]
async fn probe_create_then_find() {
    let server = std::env::var("PROBE_SERVER").expect("set PROBE_SERVER");
    let url: url::Url = server.parse().unwrap();

    // Peer A creates + pushes a doc, then stays connected.
    let repo_a = dial_bearer(&url).await;
    let (index_a, doc_id) = IndexDocument::create(&repo_a).await.unwrap();
    index_a.add_file("index.qmd", "file-doc-1").unwrap();
    index_a.add_file("about.qmd", "file-doc-2").unwrap();
    eprintln!("peer A created doc {doc_id} with 2 files; waiting to push…");
    tokio::time::sleep(Duration::from_secs(3)).await;

    // Peer B finds it.
    let repo_b = dial_bearer(&url).await;
    match repo_b
        .find(DocumentId::from_str(&doc_id).unwrap())
        .await
        .unwrap()
    {
        Some(_) => eprintln!("B: find returned Some(handle)"),
        None => eprintln!("B: find returned None"),
    }
    let index_b = IndexDocument::load(&repo_b, &doc_id)
        .await
        .unwrap()
        .unwrap();
    let mut last = 0;
    for i in 0..30 {
        last = index_b.get_all_files().len();
        eprintln!("[B t={:>5}ms] {} file(s)", i * 500, last);
        if last >= 2 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    eprintln!("RESULT: peer B saw {last} of A's 2 files via {server}");
}
