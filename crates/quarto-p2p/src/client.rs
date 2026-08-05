//! Guest side of the tunnel: local loopback TCP proxy, one accepted
//! connection = one token-prefixed QUIC bi-stream; re-dial loop with
//! backoff on connection loss.

use std::net::SocketAddr;

use tokio::sync::watch;

use crate::{EndpointPreset, PreviewShareTicket, TunnelError, TunnelStatus};

/// Configuration for [`TunnelClient::bind`].
#[derive(Debug, Default, Clone, Copy)]
pub struct TunnelClientConfig {
    /// Endpoint environment (production n0 vs. hermetic loopback).
    pub preset: EndpointPreset,
}

/// Guest side of the tunnel: dials the ticket's endpoint and serves a local
/// TCP listener; one accepted connection = one token-prefixed QUIC
/// bi-stream. Owns a re-dial loop with backoff.
pub struct TunnelClient;

impl TunnelClient {
    /// Dial the ticket's endpoint and bind the local proxy on `local`
    /// (port 0 allowed). Returns the bound address and a handle.
    ///
    /// The initial dial happens here: an unreachable host is an error at
    /// bind time (clear CLI UX), not a background retry.
    pub async fn bind(
        _cfg: TunnelClientConfig,
        _ticket: PreviewShareTicket,
        _local: SocketAddr,
    ) -> Result<(SocketAddr, TunnelClientHandle), TunnelError> {
        todo!("Phase 1 (bd-v8mwzpmi)")
    }
}

/// Handle to a running [`TunnelClient`].
#[derive(Debug)]
pub struct TunnelClientHandle {}

impl TunnelClientHandle {
    /// Watch channel for CLI messaging ("connected", "reconnecting…").
    pub fn status(&self) -> watch::Receiver<TunnelStatus> {
        todo!("Phase 1 (bd-v8mwzpmi)")
    }

    /// Abort the accept loop and close the endpoint.
    pub async fn shutdown(self) -> Result<(), TunnelError> {
        todo!("Phase 1 (bd-v8mwzpmi)")
    }
}
