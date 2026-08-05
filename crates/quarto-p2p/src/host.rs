//! Host side of the tunnel: iroh `Router` accept loop → token check →
//! TCP splice onto the local preview server.

use std::fmt;
use std::net::SocketAddr;

use iroh::SecretKey;

use crate::{EndpointPreset, PreviewShareTicket, TOKEN_LEN, TunnelError};

/// Configuration for [`TunnelHost::spawn`].
///
/// The defaults are the production posture (n0 preset, random identity,
/// random token, default UDP binds). The overrides exist for hermetic tests
/// — a fixed identity + token + UDP port lets a test restart the host and
/// exercise the client's re-dial path against an unchanged ticket.
#[derive(Default)]
pub struct TunnelHostConfig {
    /// Endpoint environment (production n0 vs. hermetic loopback).
    pub preset: EndpointPreset,
    /// Fixed endpoint identity; `None` generates a fresh one.
    pub secret_key: Option<SecretKey>,
    /// Fixed session token; `None` generates a random one.
    pub token: Option<[u8; TOKEN_LEN]>,
    /// Fixed UDP bind address; `None` uses the preset's default binds.
    pub bind_addr: Option<SocketAddr>,
}

impl fmt::Debug for TunnelHostConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TunnelHostConfig")
            .field("preset", &self.preset)
            .field(
                "secret_key",
                &self.secret_key.as_ref().map(|_| "[redacted]"),
            )
            .field("token", &self.token.as_ref().map(|_| "[redacted]"))
            .field("bind_addr", &self.bind_addr)
            .finish()
    }
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
        _cfg: TunnelHostConfig,
        _target: SocketAddr,
    ) -> Result<(PreviewShareTicket, TunnelHostHandle), TunnelError> {
        todo!("Phase 1 (bd-v8mwzpmi)")
    }
}

/// Handle to a running [`TunnelHost`].
#[derive(Debug)]
pub struct TunnelHostHandle {}

impl TunnelHostHandle {
    /// Graceful shutdown: `Router::shutdown` closes the endpoint itself.
    pub async fn shutdown(self) -> Result<(), TunnelError> {
        todo!("Phase 1 (bd-v8mwzpmi)")
    }
}
