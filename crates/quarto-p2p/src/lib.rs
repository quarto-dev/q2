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

mod client;
mod host;
mod ticket;

pub use client::{TunnelClient, TunnelClientConfig, TunnelClientHandle};
pub use host::{TunnelHost, TunnelHostConfig, TunnelHostHandle};
/// Typed error for [`PreviewShareTicket`] parsing (`FromStr`).
pub use iroh_tickets::ParseError as TicketParseError;
pub use ticket::PreviewShareTicket;

/// ALPN for the preview tunnel protocol.
///
/// The trailing `/0` is the protocol version tag; a breaking wire change
/// bumps it together with the ticket KIND.
pub const ALPN: &[u8] = b"q2/preview-tunnel/0";

/// Session-token length in bytes (256-bit).
pub const TOKEN_LEN: usize = 32;

/// Which iroh environment an endpoint binds into.
///
/// Production code uses [`EndpointPreset::N0`]; tests use
/// [`EndpointPreset::HermeticLoopback`] so no n0 infrastructure (relays,
/// pkarr, DNS) is touched in CI.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum EndpointPreset {
    /// n0 production defaults: pkarr publish/resolve + DNS lookup +
    /// default relays.
    #[default]
    N0,
    /// Hermetic mode for tests: crypto only, relays disabled, bound to
    /// loopback. Peers are reachable only via explicit ticket addresses.
    HermeticLoopback,
}

/// Which kind of network path currently carries the tunnel's traffic
/// (the selected path's `is_relay()` is the discriminator).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathKind {
    /// Direct IP path (LAN or hole-punched).
    Direct,
    /// Via a relay server (the designed fallback when hole-punching
    /// fails; traffic stays end-to-end encrypted).
    Relay,
    /// No selected path is visible right now (e.g. mid-migration).
    Unknown,
}

impl std::fmt::Display for PathKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            PathKind::Direct => "direct connection",
            PathKind::Relay => "relay",
            PathKind::Unknown => "unknown path",
        })
    }
}

/// Client connection status, surfaced to the CLI ("connected via relay",
/// "reconnecting…").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TunnelStatus {
    /// Tunnel is up; the payload says what kind of path carries it (and
    /// tracks upgrades, e.g. relay → direct once hole-punching lands).
    Connected(PathKind),
    /// Connection lost; the client is re-dialing with backoff.
    Reconnecting,
    /// The host closed the connection as unauthorized: this join
    /// string's token was rejected (the share session ended or the host
    /// restarted with a fresh token). Terminal — re-dialing with the
    /// same token cannot succeed, so the client stops trying.
    Rejected,
}

/// QUIC application error code the host closes with when a stream fails
/// token auth; the client maps it to [`TunnelStatus::Rejected`].
pub(crate) const ERROR_CODE_UNAUTHORIZED: u32 = 1;

pub(crate) type BoxedError = Box<dyn std::error::Error + Send + Sync + 'static>;

/// Bind an iroh endpoint for the given preset (shared by host and client).
///
/// `HermeticLoopback` builds a `presets::Minimal` endpoint bound to loopback
/// only, with relays explicitly disabled (Minimal already defaults to
/// `RelayMode::Disabled`; the explicit setting documents the hermetic
/// posture). `configure` lets callers add builder options (e.g. the
/// client's `MemoryLookup`).
pub(crate) async fn bind_endpoint(
    preset: EndpointPreset,
    secret_key: Option<iroh::SecretKey>,
    bind_addr: Option<std::net::SocketAddr>,
    configure: impl FnOnce(iroh::endpoint::Builder) -> iroh::endpoint::Builder,
) -> Result<iroh::Endpoint, TunnelError> {
    let mut builder = match preset {
        EndpointPreset::N0 => iroh::Endpoint::builder(iroh::endpoint::presets::N0),
        EndpointPreset::HermeticLoopback => {
            iroh::Endpoint::builder(iroh::endpoint::presets::Minimal)
                .relay_mode(iroh::RelayMode::Disabled)
                .clear_ip_transports()
        }
    };
    let bind_addr = match (preset, bind_addr) {
        (_, Some(addr)) => Some(addr),
        (EndpointPreset::HermeticLoopback, None) => {
            Some("127.0.0.1:0".parse().expect("loopback socket addr"))
        }
        (EndpointPreset::N0, None) => None,
    };
    if let Some(addr) = bind_addr {
        builder = builder
            .bind_addr(addr)
            .map_err(|e| TunnelError::Bind(Box::new(e)))?;
    }
    if let Some(secret_key) = secret_key {
        builder = builder.secret_key(secret_key);
    }
    configure(builder)
        .bind()
        .await
        .map_err(|e| TunnelError::Bind(Box::new(e)))
}

/// Errors from the tunnel API.
#[derive(Debug, thiserror::Error)]
pub enum TunnelError {
    /// Binding the iroh endpoint (or its UDP socket) failed.
    #[error("failed to bind tunnel endpoint")]
    Bind(#[source] BoxedError),
    /// The endpoint never reported a dialable address.
    #[error("tunnel endpoint has no dialable address")]
    NoAddress,
    /// Dialing the share host failed.
    #[error("could not reach the share host")]
    Connect(#[source] BoxedError),
    /// The local TCP proxy listener failed.
    #[error("local tunnel proxy error")]
    Proxy(#[source] std::io::Error),
    /// Graceful shutdown did not complete cleanly.
    #[error("tunnel shutdown failed")]
    Shutdown(#[source] BoxedError),
}
