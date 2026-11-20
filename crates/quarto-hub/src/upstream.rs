//! Upstream mirroring: a standalone hub that serves an existing project
//! from another sync server (bd-yq3awhmw).
//!
//! `q2 preview --hub-url <ws> --project <id>` runs its embedded hub in
//! standalone mode and dials the remote hub as an *upstream* peer. The
//! browser keeps talking to the local `/ws`; the local hub fetches the
//! project's documents from the upstream on demand and relays changes in
//! both directions.
//!
//! The one non-obvious piece is the share policy. The hub's policy turns
//! `announce` off for every peer (bd-jvag5d4w), and in samod `announce` also
//! gates *requesting* a missing document (`can_request = can_write &&
//! announce`). With that policy the local hub would never ask the upstream
//! for anything, and never push documents the browser created. So
//! [`HubSharePolicy`] keeps `announce` off for every peer except the ones
//! recorded as the upstream. A hub with no upstream records none, so its
//! behaviour is unchanged.
//!
//! Peer ids are self-asserted by the transport, so a client that claims the
//! upstream's peer id gets announcements too. That only matters when the
//! hub listens beyond loopback; the preview binds loopback by default.
//!
//! See `claude-notes/plans/2026-11-13-preview-against-remote-hub.md`.

use std::collections::HashSet;
use std::str::FromStr;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use futures::StreamExt;
use samod::{BackoffConfig, DialerEvent, DocumentId, PeerId, Repo, ShareConfig, SharePolicy};
use tracing::{info, warn};

use crate::error::{Error, Result};
use crate::index::IndexDocument;

/// How long to wait for the first upstream handshake before giving up.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

/// An upstream sync server that a standalone hub mirrors.
#[derive(Debug, Clone)]
pub struct UpstreamConfig {
    /// `ws://` or `wss://` URL of the upstream sync endpoint.
    pub url: String,
    /// The upstream project's index document id, bare or `automerge:`-prefixed.
    pub index_doc_id: String,
}

/// The hub's samod share policy, aware of upstream peers.
///
/// Every peer gets [`crate::context::hub_share_policy`] (read-write,
/// announce off) except peers recorded as the upstream, which get
/// read-write with announce on, so the hub requests missing documents
/// from them and pushes local documents to them.
///
/// A read-only mirror ([`HubSharePolicy::read_only_clients`]) gives the
/// non-upstream peers (the browsers) read-only access instead: they receive
/// every change, and the hub ignores theirs, so nothing they type reaches the
/// upstream. That is `q2 preview --hub-url` without `--allow-edit`.
#[derive(Clone, Default)]
pub struct HubSharePolicy {
    upstream_peers: Arc<RwLock<HashSet<PeerId>>>,
    read_only_clients: bool,
}

impl HubSharePolicy {
    /// A policy whose non-upstream peers may read but not write.
    pub fn read_only_clients() -> Self {
        Self {
            read_only_clients: true,
            ..Self::default()
        }
    }

    /// Record `peer_id` as an upstream peer. Decisions are made per
    /// (document, peer) subscription, which samod restarts per connection,
    /// so a peer must be recorded before documents are requested over it.
    pub fn add_upstream_peer(&self, peer_id: PeerId) {
        self.upstream_peers
            .write()
            .expect("upstream peer set poisoned")
            .insert(peer_id);
    }

    fn decide(&self, peer_id: &PeerId) -> ShareConfig {
        let is_upstream = self
            .upstream_peers
            .read()
            .expect("upstream peer set poisoned")
            .contains(peer_id);
        if is_upstream {
            ShareConfig::read_write()
        } else if self.read_only_clients {
            ShareConfig::read_only().with_announce(false)
        } else {
            crate::context::hub_share_policy()
        }
    }
}

impl SharePolicy for HubSharePolicy {
    fn share(
        &self,
        _doc_id: DocumentId,
        peer_id: PeerId,
    ) -> impl futures::Stream<Item = ShareConfig> + Send + 'static {
        futures::stream::iter([self.decide(&peer_id)])
    }
}

/// Dial the upstream, wait for the first handshake, and record its peer id
/// with `policy` (on this and every later reconnect).
///
/// Must complete before the upstream index is looked up: samod does not
/// revive a `find()` that completed while the upstream was still unknown.
pub async fn connect_upstream(
    repo: &Repo,
    upstream: &UpstreamConfig,
    policy: &HubSharePolicy,
) -> Result<()> {
    let url: samod::Url = upstream
        .url
        .parse()
        .map_err(|e| Error::Server(format!("invalid upstream URL {}: {e}", upstream.url)))?;
    if !matches!(url.scheme(), "ws" | "wss") {
        return Err(Error::Server(format!(
            "upstream URL must be ws:// or wss://, got {}",
            upstream.url
        )));
    }
    let handle = repo
        .dial_websocket(url.clone(), BackoffConfig::default())
        .map_err(|_| Error::Server("repo is stopped".to_string()))?;

    // The first handshake's peer id comes from `established()` below; this
    // task records the peer id of every later reconnect (an upstream
    // restart may present a new one). The handle moves into the task, and
    // dropping it would not stop the dialer anyway.
    let established = handle.established();
    let reconnect_policy = policy.clone();
    let log_url = url.clone();
    tokio::spawn(async move {
        let mut events = handle.events();
        while let Some(event) = events.next().await {
            match event {
                DialerEvent::Connected { peer_info } => {
                    info!(url = %log_url, peer_id = %peer_info.peer_id, "upstream connected");
                    reconnect_policy.add_upstream_peer(peer_info.peer_id);
                }
                DialerEvent::Disconnected { reason } => {
                    warn!(url = %log_url, %reason, "upstream disconnected; reconnecting");
                }
                _ => {}
            }
        }
    });

    let peer_info = match tokio::time::timeout(CONNECT_TIMEOUT, established).await {
        Ok(Ok(info)) => info,
        Ok(Err(e)) => {
            return Err(Error::Server(format!(
                "could not connect to upstream {url}: {e}"
            )));
        }
        Err(_) => {
            return Err(Error::Server(format!(
                "timed out after {}s connecting to upstream {url}",
                CONNECT_TIMEOUT.as_secs()
            )));
        }
    };
    policy.add_upstream_peer(peer_info.peer_id.clone());
    info!(url = %url, peer_id = %peer_info.peer_id, "upstream handshake complete");
    Ok(())
}

/// Fetch the upstream project's index document. Never creates one: a
/// missing index is an error, not an empty project.
pub async fn load_upstream_index(repo: &Repo, index_doc_id: &str) -> Result<IndexDocument> {
    let bare = index_doc_id
        .strip_prefix("automerge:")
        .unwrap_or(index_doc_id);
    IndexDocument::load(repo, bare).await?.ok_or_else(|| {
        Error::IndexDocument(format!(
            "the upstream hub does not have project index document {bare}"
        ))
    })
}

/// Fetch every file document listed in the index in the background, so
/// the browser's first requests find them already local instead of
/// racing the upstream fetch (the local hub answers "unavailable" while
/// it is still fetching).
pub fn prefetch_files(repo: &Repo, index: &IndexDocument) {
    let files = index.get_all_files();
    info!(count = files.len(), "prefetching upstream file documents");
    for (path, doc_id) in files {
        let bare = doc_id
            .strip_prefix("automerge:")
            .unwrap_or(&doc_id)
            .to_string();
        let Ok(id) = DocumentId::from_str(&bare) else {
            warn!(%path, %doc_id, "index entry has an invalid document id");
            continue;
        };
        let repo = repo.clone();
        tokio::spawn(async move {
            match repo.find(id).await {
                Ok(Some(_)) => {}
                Ok(None) => warn!(%path, "upstream file document unavailable"),
                Err(_) => {}
            }
        });
    }
}
