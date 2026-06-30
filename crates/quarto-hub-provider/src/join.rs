//! Join a hub as a client peer and list its files (Phase 3 deliverable).

use std::sync::Arc;
use std::time::Duration;

use samod::{BackoffConfig, Repo};

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
/// [`BearerDialer`], then `find()` the index document and return the sorted
/// list of project file paths.
///
/// This is the narrow Phase 3 success criterion: it exercises the full
/// authenticated sync path (BearerDialer handshake → samod sync → index doc)
/// without any execution.
pub async fn join_and_list_files(
    config: JoinConfig,
    token_source: Arc<dyn TokenSource>,
) -> Result<Vec<String>, ProviderError> {
    // A client peer keeps nothing on disk — the project lives on the hub, and
    // (Phase 4) we materialize to a temp dir per run.
    let repo = Repo::build_tokio().load().await;

    let dialer = Arc::new(BearerDialer::new(
        config.server_ws_url.clone(),
        token_source,
    ));
    let handle = repo
        .dial(BackoffConfig::default(), dialer)
        .map_err(|_| ProviderError::Repo("repo is stopped".into()))?;

    // Wait for the first connection (or a failure) within the timeout.
    tokio::time::timeout(config.connect_timeout, handle.established())
        .await
        .map_err(|_| ProviderError::Repo("timed out connecting to hub".into()))?
        .map_err(|_| ProviderError::Repo("hub connection failed (auth rejected?)".into()))?;

    let index = quarto_hub::index::IndexDocument::load(&repo, &config.index_doc_id)
        .await
        .map_err(|e| ProviderError::Index(e.to_string()))?
        .ok_or_else(|| ProviderError::Index("index document not found".into()))?;

    let mut files: Vec<String> = index.get_all_files().into_keys().collect();
    files.sort();
    Ok(files)
}
