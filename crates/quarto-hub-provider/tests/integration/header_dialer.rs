//! End-to-end handshake test for [`HeaderDialer`].
//!
//! The acceptor stands in for a hub behind a token-gated gateway, such as
//! the Connect quartohub satellite's, which refuses any upgrade that does not
//! carry its relay key. It answers those with 401 before samod sees the
//! socket, so a dialer that forgets the header never syncs.

use std::sync::Arc;
use std::time::Duration;

use quarto_hub::index::IndexDocument;
use quarto_hub_provider::{HeaderDialer, JoinConfig, join_with_dialer};
use samod::Repo;
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::handshake::server::{ErrorResponse, Request, Response};
use tokio_tungstenite::tungstenite::http::StatusCode;

const HEADER: &str = "X-Relay-Key";
const SECRET: &str = "relay-secret-1";

/// A samod acceptor that admits only upgrades carrying `HEADER: SECRET`.
/// Returns the server repo, the websocket URL, and the id of an index
/// document seeded with one file.
async fn spawn_gated_hub() -> (Repo, url::Url, String) {
    let server_repo = Repo::build_tokio().load().await;
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let ws_url: url::Url = format!("ws://{addr}/ws").parse().unwrap();
    let acceptor = server_repo.make_acceptor(ws_url.clone()).unwrap();

    let (index, index_doc_id) = IndexDocument::create(&server_repo).await.unwrap();
    index.add_file("index.qmd", "doc-a").unwrap();

    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            let acceptor = acceptor.clone();
            tokio::spawn(async move {
                let check = |req: &Request, resp: Response| -> Result<Response, ErrorResponse> {
                    if req.headers().get(HEADER).is_some_and(|v| v == SECRET) {
                        Ok(resp)
                    } else {
                        let mut refusal = ErrorResponse::new(None);
                        *refusal.status_mut() = StatusCode::UNAUTHORIZED;
                        Err(refusal)
                    }
                };
                if let Ok(ws) = tokio_tungstenite::accept_hdr_async(stream, check).await {
                    let _ = acceptor.accept_tungstenite(ws);
                }
            });
        }
    });

    (server_repo, ws_url, index_doc_id)
}

fn config(server_ws_url: url::Url, index_doc_id: String, timeout: Duration) -> JoinConfig {
    JoinConfig {
        server_ws_url,
        index_doc_id,
        connect_timeout: timeout,
    }
}

#[tokio::test]
async fn header_dialer_joins_a_gated_hub_when_it_sends_the_header() {
    let (_server, ws_url, index_doc_id) = spawn_gated_hub().await;
    let dialer = HeaderDialer::new(
        ws_url.clone(),
        vec![(HEADER.to_string(), SECRET.to_string())],
    )
    .unwrap();

    let (_repo, index) = join_with_dialer(
        config(ws_url, index_doc_id, Duration::from_secs(10)),
        Arc::new(dialer),
    )
    .await
    .expect("a dialer sending the relay key must join");

    let files: Vec<String> = index.get_all_files().into_keys().collect();
    assert_eq!(files, vec!["index.qmd".to_string()]);
}

#[tokio::test]
async fn header_dialer_is_refused_by_a_gated_hub_without_the_header() {
    let (_server, ws_url, index_doc_id) = spawn_gated_hub().await;
    let dialer = HeaderDialer::new(
        ws_url.clone(),
        vec![(HEADER.to_string(), "wrong-secret".to_string())],
    )
    .unwrap();

    let result = join_with_dialer(
        config(ws_url, index_doc_id, Duration::from_secs(2)),
        Arc::new(dialer),
    )
    .await;
    assert!(
        result.is_err(),
        "a dialer with the wrong relay key must never join"
    );
}
