//! Join a hub as a client peer and list its files (Phase 3 deliverable).

use std::sync::Arc;
use std::time::Duration;

use quarto_hub::index::IndexDocument;
use samod::{BackoffConfig, DialerFailed, Repo};

use crate::ProviderError;
use crate::dialer::BearerDialer;
use crate::token::TokenSource;

/// What to connect to.
pub struct JoinConfig {
    /// The hub's websocket endpoint, e.g. `wss://quarto-hub.com/ws`.
    pub server_ws_url: url::Url,
    /// The project's automerge index document id.
    pub index_doc_id: String,
    /// How long to wait for the peer connection to establish.
    pub connect_timeout: Duration,
}

/// Join the hub as an ephemeral (memory-storage) client peer, dialing with a
/// [`BearerDialer`], then `find()` the index document. Returns the live
/// [`Repo`] and [`IndexDocument`] so the caller can keep syncing — materialize
/// files, watch the ephemeral channel, and write captures back (Phase 4).
///
/// A client peer keeps nothing on disk: the project lives on the hub, and each
/// execution materializes to a temp dir per run.
pub async fn join(
    config: JoinConfig,
    token_source: Arc<dyn TokenSource>,
) -> Result<(Repo, IndexDocument), ProviderError> {
    let repo = Repo::build_tokio().load().await;

    let dialer = Arc::new(BearerDialer::new(
        config.server_ws_url.clone(),
        token_source,
    ));
    let handle = repo
        .dial(BackoffConfig::default(), dialer)
        .map_err(|_| ProviderError::Repo("repo is stopped".into()))?;

    // Wait for the first connection (or a failure) within the timeout. A
    // permanent dial failure (e.g. a non-ws(s) --server URL) surfaces its
    // own cause; anything else keeps the generic wording (bd-q96fhj3q).
    tokio::time::timeout(config.connect_timeout, handle.established())
        .await
        .map_err(|_| ProviderError::Repo("timed out connecting to hub".into()))?
        .map_err(|e| match e {
            DialerFailed::PermanentFailure(err) => {
                ProviderError::Repo(format!("hub connection failed: {err}"))
            }
            e => ProviderError::Repo(format!("hub connection failed: {e}")),
        })?;

    let index = IndexDocument::load(&repo, &config.index_doc_id)
        .await
        .map_err(|e| ProviderError::Index(e.to_string()))?
        .ok_or_else(|| ProviderError::Index("index document not found".into()))?;

    Ok((repo, index))
}

/// Join the hub and return the sorted list of project file paths.
///
/// This is the narrow Phase 3 success criterion: it exercises the full
/// authenticated sync path (BearerDialer handshake → samod sync → index doc)
/// without any execution.
pub async fn join_and_list_files(
    config: JoinConfig,
    token_source: Arc<dyn TokenSource>,
) -> Result<Vec<String>, ProviderError> {
    let (_repo, index) = join(config, token_source).await?;
    let mut files: Vec<String> = index.get_all_files().into_keys().collect();
    files.sort();
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::token::StaticTokenSource;

    // bd-q96fhj3q: a non-ws(s) --server URL is a permanent misconfiguration.
    // join must fail fast and name the URL as the cause — not burn the whole
    // connect timeout and then blame a timeout or auth.
    #[tokio::test]
    async fn join_with_a_non_ws_server_url_fails_fast_naming_the_url() {
        let start = std::time::Instant::now();
        let result = join(
            JoinConfig {
                server_ws_url: url::Url::parse("https://127.0.0.1:1/ws").unwrap(),
                index_doc_id: "irrelevant".to_string(),
                connect_timeout: Duration::from_secs(3),
            },
            Arc::new(StaticTokenSource::new("tok-123")),
        )
        .await;
        let err = match result {
            Ok(_) => panic!("join must fail for an https:// server URL"),
            Err(err) => err,
        };
        let msg = err.to_string();
        assert!(
            msg.contains("invalid websocket url"),
            "error should name the URL as the cause, got: {msg}"
        );
        assert!(
            start.elapsed() < Duration::from_secs(3),
            "permanent failure should surface before the connect timeout"
        );
    }
}
