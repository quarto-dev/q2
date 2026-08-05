//! Host side of the tunnel: iroh `Router` accept loop → token check →
//! TCP splice onto the local preview server.

use std::fmt;
use std::net::SocketAddr;
use std::time::Duration;

use iroh::endpoint::{Connection, RecvStream, SendStream, VarInt};
use iroh::protocol::{AcceptError, ProtocolHandler, Router};
use iroh::{Endpoint, EndpointAddr, SecretKey, Watcher};
use subtle::ConstantTimeEq;
use tokio::net::TcpStream;

use crate::{ALPN, EndpointPreset, PreviewShareTicket, TOKEN_LEN, TunnelError};

/// How long a freshly accepted stream may take to present its token.
const TOKEN_READ_TIMEOUT: Duration = Duration::from_secs(10);

/// How long to wait for the endpoint to come "online" (relay contact) with
/// the production preset before proceeding without one. Matches iroh's
/// (private) net-report budget; do NOT use `iroh::NET_REPORT_TIMEOUT`,
/// which is a bare `u64` docs constant, not a `Duration`.
const ONLINE_TIMEOUT: Duration = Duration::from_secs(10);

/// How long to wait for a freshly bound endpoint to report at least one
/// dialable address.
const ADDR_TIMEOUT: Duration = Duration::from_secs(10);

/// QUIC application error code for a stream that failed token auth.
const ERROR_CODE_UNAUTHORIZED: u32 = 1;
/// QUIC application error code for "the local target refused a connection".
const ERROR_CODE_TARGET_UNAVAILABLE: u32 = 2;

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
        cfg: TunnelHostConfig,
        target: SocketAddr,
    ) -> Result<(PreviewShareTicket, TunnelHostHandle), TunnelError> {
        let token = cfg.token.unwrap_or_else(rand::random);
        let endpoint =
            crate::bind_endpoint(cfg.preset, cfg.secret_key, cfg.bind_addr, |b| b).await?;

        // With relays in play, only a completed net-report makes the relay
        // URL part of `endpoint.addr()`. `online()` pends forever with no
        // relay reachable, so cap it and degrade to direct/LAN-only.
        if cfg.preset == EndpointPreset::N0
            && tokio::time::timeout(ONLINE_TIMEOUT, endpoint.online())
                .await
                .is_err()
        {
            tracing::warn!(
                "iroh relay unreachable after {}s — ticket will carry direct/LAN addresses only",
                ONLINE_TIMEOUT.as_secs()
            );
        }

        let addr = dialable_addr(&endpoint).await?;
        let router = Router::builder(endpoint)
            .accept(ALPN, TunnelProtocol { token, target })
            .spawn();

        let ticket = PreviewShareTicket { addr, token };
        Ok((ticket, TunnelHostHandle { router }))
    }
}

/// Waits until the endpoint reports at least one dialable transport addr.
async fn dialable_addr(endpoint: &Endpoint) -> Result<EndpointAddr, TunnelError> {
    let mut watcher = endpoint.watch_addr();
    let deadline = tokio::time::Instant::now() + ADDR_TIMEOUT;
    loop {
        let addr = watcher.get();
        if !addr.addrs.is_empty() {
            return Ok(addr);
        }
        let Some(remaining) = deadline.checked_duration_since(tokio::time::Instant::now()) else {
            return Err(TunnelError::NoAddress);
        };
        match tokio::time::timeout(remaining, watcher.updated()).await {
            Ok(Ok(_)) => continue,
            Ok(Err(_)) | Err(_) => return Err(TunnelError::NoAddress),
        }
    }
}

/// Handle to a running [`TunnelHost`].
#[derive(Debug)]
pub struct TunnelHostHandle {
    router: Router,
}

impl TunnelHostHandle {
    /// Graceful shutdown: `Router::shutdown` closes the endpoint itself
    /// (a trailing `Endpoint::close` would be an idempotent no-op).
    pub async fn shutdown(self) -> Result<(), TunnelError> {
        self.router
            .shutdown()
            .await
            .map_err(|e| TunnelError::Shutdown(Box::new(e)))
    }
}

/// The `q2/preview-tunnel/0` protocol: per connection, accept bi-streams
/// forever; per stream, check the token prefix and splice onto the target.
#[derive(Debug, Clone)]
struct TunnelProtocol {
    token: [u8; TOKEN_LEN],
    target: SocketAddr,
}

impl ProtocolHandler for TunnelProtocol {
    async fn accept(&self, connection: Connection) -> Result<(), AcceptError> {
        // Log joining peers for auditability (security model).
        let remote = connection.remote_id().fmt_short();
        tracing::info!(%remote, "preview tunnel: peer connected");
        // The loop ends when accept_bi errors: peer disconnected or we
        // shut down; either way this per-connection task is done.
        while let Ok((send, recv)) = connection.accept_bi().await {
            let proto = self.clone();
            let connection = connection.clone();
            tokio::spawn(async move { proto.handle_stream(connection, send, recv).await });
        }
        tracing::info!(%remote, "preview tunnel: peer disconnected");
        Ok(())
    }
}

impl TunnelProtocol {
    async fn handle_stream(
        &self,
        connection: Connection,
        mut send: SendStream,
        mut recv: RecvStream,
    ) {
        let remote = connection.remote_id().fmt_short();

        let mut presented = [0u8; TOKEN_LEN];
        let authorized =
            match tokio::time::timeout(TOKEN_READ_TIMEOUT, recv.read_exact(&mut presented)).await {
                Ok(Ok(())) => bool::from(presented.ct_eq(&self.token)),
                // Short read (stream finished early) or timeout: unauthorized.
                Ok(Err(_)) | Err(_) => false,
            };
        if !authorized {
            tracing::warn!(%remote, "preview tunnel: bad or missing token; dropping connection");
            let _ = send.reset(VarInt::from_u32(ERROR_CODE_UNAUTHORIZED));
            connection.close(VarInt::from_u32(ERROR_CODE_UNAUTHORIZED), b"unauthorized");
            return;
        }

        let mut tcp = match TcpStream::connect(self.target).await {
            Ok(tcp) => tcp,
            Err(err) => {
                tracing::warn!(
                    %remote, target = %self.target, %err,
                    "preview tunnel: local target refused connection"
                );
                let _ = send.reset(VarInt::from_u32(ERROR_CODE_TARGET_UNAVAILABLE));
                return;
            }
        };

        // Terminated splice. `copy_bidirectional` propagates read-EOF as a
        // write-shutdown on the opposite side: QUIC FIN → TCP FIN via
        // `TcpStream::poll_shutdown`, TCP FIN → QUIC FIN via
        // `SendStream::poll_shutdown` (which calls `finish()`).
        let mut quic = tokio::io::join(recv, send);
        match tokio::io::copy_bidirectional(&mut quic, &mut tcp).await {
            Ok((to_target, from_target)) => {
                tracing::debug!(%remote, to_target, from_target, "preview tunnel: stream closed");
            }
            Err(err) => {
                tracing::debug!(%remote, %err, "preview tunnel: stream ended with error");
            }
        }
    }
}
