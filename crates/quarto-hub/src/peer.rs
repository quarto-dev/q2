//! Peer connection management for syncing with external sync servers.
//!
//! This module handles outgoing WebSocket connections to sync servers like
//! sync.automerge.org. Connections are maintained with automatic reconnection
//! using exponential backoff.

use std::time::Duration;

use samod::{ConnDirection, Repo};
use tokio_tungstenite::connect_async;
use tracing::{debug, info, warn};

/// Minimum backoff duration between reconnection attempts.
const MIN_BACKOFF: Duration = Duration::from_secs(1);

/// Maximum backoff duration between reconnection attempts.
const MAX_BACKOFF: Duration = Duration::from_secs(60);

/// Spawn a background task that maintains a connection to a peer.
///
/// The task will:
/// 1. Attempt to connect to the peer via WebSocket
/// 2. If successful, sync documents until the connection closes
/// 3. Reconnect with exponential backoff on disconnection or error
///
/// The task runs until the repo is stopped.
pub fn spawn_peer_connection(repo: Repo, url: String) {
    tokio::spawn(async move {
        peer_connection_loop(repo, url).await;
    });
}

/// Main loop for maintaining a peer connection.
async fn peer_connection_loop(repo: Repo, url: String) {
    let mut backoff = MIN_BACKOFF;

    loop {
        info!(url = %url, "Connecting to peer");

        match connect_to_peer(&repo, &url).await {
            PeerConnectionResult::Connected => {
                // Connection succeeded and then closed normally
                info!(url = %url, "Peer connection closed");
                // Reset backoff on successful connection
                backoff = MIN_BACKOFF;
            }
            PeerConnectionResult::ConnectionFailed(err) => {
                warn!(url = %url, error = %err, "Failed to connect to peer");
            }
            PeerConnectionResult::RepoStopped => {
                info!(url = %url, "Repo stopped, exiting peer connection loop");
                return;
            }
        }

        // Wait before reconnecting
        debug!(url = %url, backoff_secs = backoff.as_secs(), "Waiting before reconnect");
        tokio::time::sleep(backoff).await;

        // Exponential backoff with cap
        backoff = (backoff * 2).min(MAX_BACKOFF);
    }
}

/// Result of a peer connection attempt.
enum PeerConnectionResult {
    /// Connected successfully, connection later closed
    Connected,
    /// Failed to establish connection
    ConnectionFailed(String),
    /// Repo was stopped
    RepoStopped,
}

/// Attempt to connect to a peer and sync until disconnection.
async fn connect_to_peer(repo: &Repo, url: &str) -> PeerConnectionResult {
    // Establish WebSocket connection
    let ws_stream = match connect_async(url).await {
        Ok((stream, response)) => {
            debug!(
                url = %url,
                status = %response.status(),
                "WebSocket connection established"
            );
            stream
        }
        Err(e) => {
            return PeerConnectionResult::ConnectionFailed(e.to_string());
        }
    };

    // Connect the WebSocket to the repo
    // Note: connect_tungstenite expects a stream that implements both Sink and Stream
    let connection = match repo.connect_tungstenite(ws_stream, ConnDirection::Outgoing) {
        Ok(conn) => conn,
        Err(samod::Stopped) => {
            return PeerConnectionResult::RepoStopped;
        }
    };

    info!(url = %url, peer_info = ?connection.info(), "Connected to peer");

    // Wait for the connection to finish (disconnect or error)
    let reason = connection.finished().await;
    debug!(url = %url, reason = ?reason, "Peer connection finished");

    PeerConnectionResult::Connected
}

#[cfg(test)]
mod tests {
    // Integration tests would require a test sync server
    // Manual testing instructions:
    // 1. Start a sync server (e.g., sync.automerge.org or local)
    // 2. Run hub with --peer ws://localhost:3030
    // 3. Verify connection in logs
}
