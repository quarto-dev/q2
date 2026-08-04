//! P2P tunnel for `q2 preview --share` / `--join` (live share over iroh).
//!
//! The host side ([`TunnelHost`]) exposes the local preview HTTP server
//! through token-authenticated iroh QUIC bi-streams; the guest side
//! ([`TunnelClient`]) is a local loopback TCP proxy that splices each
//! accepted connection onto one such stream. The join string
//! ([`PreviewShareTicket`]) carries the host's `EndpointAddr` and a random
//! 256-bit session token — possession of the string is the capability.
//!
//! Plan: `claude-notes/plans/2026-08-03-q2-preview-live-share-iroh.md`.
//! This is the Phase 0 scaffold (bd-9gam4jqe): public API stubs only.
//! Phase 1 (bd-v8mwzpmi) implements them tests-first.

use std::net::SocketAddr;

use iroh::EndpointAddr;

/// Join-string payload: the host's endpoint address plus the session token.
///
/// Phase 1 adds the `iroh_tickets::Ticket` impl (KIND `"q2preview"`),
/// `Display`/`FromStr`, and a manual `Debug` that redacts the token —
/// no derived `Debug` here, ever, or the token leaks into logs.
pub struct PreviewShareTicket {
    pub addr: EndpointAddr,
    pub token: [u8; 32],
}

/// Host side of the tunnel: an iroh `Router` whose accept loop verifies the
/// per-stream token prefix, then splices the stream onto a fresh TCP
/// connection to the local preview server.
pub struct TunnelHost;

impl TunnelHost {
    /// Bind an iroh endpoint, start the accept loop targeting `target`
    /// (the loopback-bound preview server), and return the join ticket
    /// plus a shutdown handle.
    pub async fn spawn(
        _target: SocketAddr,
    ) -> Result<(PreviewShareTicket, TunnelHostHandle), TunnelError> {
        todo!("Phase 1 (bd-v8mwzpmi)")
    }
}

/// Handle to a running [`TunnelHost`].
pub struct TunnelHostHandle;

impl TunnelHostHandle {
    /// Graceful shutdown: `Router::shutdown` closes the endpoint itself.
    pub async fn shutdown(self) -> Result<(), TunnelError> {
        todo!("Phase 1 (bd-v8mwzpmi)")
    }
}

/// Guest side of the tunnel: dials the ticket's endpoint and serves a local
/// TCP listener; one accepted connection = one token-prefixed QUIC
/// bi-stream. Owns a re-dial loop with backoff.
pub struct TunnelClient;

impl TunnelClient {
    /// Dial the ticket's endpoint and bind the local proxy on `local`
    /// (port 0 allowed). Returns the bound address and a handle.
    pub async fn bind(
        _ticket: PreviewShareTicket,
        _local: SocketAddr,
    ) -> Result<(SocketAddr, TunnelClientHandle), TunnelError> {
        todo!("Phase 1 (bd-v8mwzpmi)")
    }
}

/// Handle to a running [`TunnelClient`]. Phase 1 adds a status watch
/// channel ([`TunnelStatus`]) for CLI messaging.
pub struct TunnelClientHandle;

impl TunnelClientHandle {
    /// Abort the accept loop and close the endpoint.
    pub async fn shutdown(self) -> Result<(), TunnelError> {
        todo!("Phase 1 (bd-v8mwzpmi)")
    }
}

/// Client connection status, surfaced to the CLI ("connected via relay",
/// "reconnecting…").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TunnelStatus {
    Connected,
    Reconnecting,
}

/// Errors from the tunnel API. Variants are added by Phase 1 alongside the
/// behavior that produces them.
#[derive(Debug, thiserror::Error)]
pub enum TunnelError {}
