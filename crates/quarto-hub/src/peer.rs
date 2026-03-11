//! Peer connection management for syncing with external sync servers.
//!
//! This module handles outgoing WebSocket connections to sync servers like
//! sync.automerge.org. Connections are maintained with automatic reconnection
//! via samod's built-in dialer with exponential backoff.

use samod::{BackoffConfig, Repo};
use tracing::{info, warn};

/// Spawn a peer dialer that maintains a connection to a remote sync server.
///
/// Uses samod's built-in `dial_websocket` which handles:
/// - Initial connection via tungstenite
/// - Automatic reconnection with exponential backoff
/// - Document sync over the WebSocket transport
///
/// The dialer runs until the repo is stopped.
pub fn spawn_peer_connection(repo: Repo, url: String) {
    let ws_url: url::Url = match url.parse() {
        Ok(u) => u,
        Err(e) => {
            warn!(url = %url, error = %e, "Invalid peer URL, skipping");
            return;
        }
    };

    match repo.dial_websocket(ws_url.clone(), BackoffConfig::default()) {
        Ok(handle) => {
            info!(url = %ws_url, "Peer dialer started");
            // Spawn a task to log when the first connection is established
            tokio::spawn(async move {
                match handle.established().await {
                    Ok(peer_info) => {
                        info!(
                            url = %ws_url,
                            peer_id = %peer_info.peer_id,
                            "Peer connection established"
                        );
                    }
                    Err(_) => {
                        warn!(url = %ws_url, "Peer dialer failed permanently");
                    }
                }
            });
        }
        Err(samod::Stopped) => {
            warn!(url = %url, "Cannot dial peer: repo is stopped");
        }
    }
}

#[cfg(test)]
mod tests {
    // Integration tests would require a test sync server
    // Manual testing instructions:
    // 1. Start a sync server (e.g., sync.automerge.org or local)
    // 2. Run hub with --peer ws://localhost:3030
    // 3. Verify connection in logs
}
