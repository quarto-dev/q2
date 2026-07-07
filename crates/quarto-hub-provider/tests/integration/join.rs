//! End-to-end join test (bd-sfet3264, Phase 3B).
//!
//! Spins up a *bare* samod acceptor behind a tungstenite websocket server (no
//! auth — the BearerDialer's `Authorization` header is sent and harmlessly
//! ignored), seeds an index document with files, and verifies the provider's
//! [`join_and_list_files`] connects over the real `BearerDialer` transport,
//! syncs the index, and lists the files.
//!
//! This exercises the actual transport (BearerDialer -> ws -> samod acceptor ->
//! sync -> IndexDocument::load), which the unit tests can't. The
//! *authenticated* acceptance path (hub validates the Bearer) is covered by
//! `quarto-hub`'s `auth_bearer` tests on the hub side, and end-to-end by the
//! Phase 3C real-binary run.

use std::sync::Arc;
use std::time::Duration;

use quarto_hub::index::IndexDocument;
use quarto_hub_provider::{JoinConfig, StaticTokenSource, join_and_list_files};
use samod::Repo;
use tokio::net::TcpListener;

#[tokio::test]
async fn join_connects_and_lists_files_over_the_bearer_dialer() {
    // ── Server side: a samod repo with an index doc behind a ws acceptor ──
    let server_repo = Repo::build_tokio().load().await;

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let ws_url: url::Url = format!("ws://{addr}").parse().unwrap();

    let acceptor = server_repo
        .make_acceptor(ws_url.clone())
        .expect("make acceptor");

    // Seed the index document with two files. The file values are placeholder
    // doc ids — `join_and_list_files` only reads the path keys.
    let (index, index_doc_id) = IndexDocument::create(&server_repo)
        .await
        .expect("create index");
    index.add_file("a.qmd", "doc-a").unwrap();
    index.add_file("docs/b.qmd", "doc-b").unwrap();

    // Accept incoming websocket connections and hand them to the acceptor.
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                break;
            };
            let acceptor = acceptor.clone();
            tokio::spawn(async move {
                match tokio_tungstenite::accept_async(stream).await {
                    Ok(ws) => {
                        let _ = acceptor.accept_tungstenite(ws);
                    }
                    Err(e) => eprintln!("ws accept failed: {e}"),
                }
            });
        }
    });

    // ── Provider side: dial with the BearerDialer and list files ──
    let files = join_and_list_files(
        JoinConfig {
            server_ws_url: ws_url,
            index_doc_id,
            connect_timeout: Duration::from_secs(10),
        },
        Arc::new(StaticTokenSource::new("test-bearer-token")),
    )
    .await
    .expect("join + list files");

    assert_eq!(files, vec!["a.qmd".to_string(), "docs/b.qmd".to_string()]);
}
