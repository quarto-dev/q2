//! Guest side of the tunnel: [`TunnelClient::connect`] builds a
//! supervised connection to the host (one token-prefixed QUIC bi-stream
//! per `open_stream` call, re-dial loop with backoff on connection
//! loss); [`TunnelClient::bind`] adds a local loopback TCP proxy that
//! splices each accepted connection onto one such stream.

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use futures::StreamExt;
use iroh::address_lookup::memory::MemoryLookup;
use iroh::endpoint::{Connection, ConnectionError, PathList, RecvStream, SendStream, VarInt};
use iroh::{Endpoint, EndpointAddr};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{RwLock, watch};
use tokio::task::JoinHandle;
use tokio::time::timeout;

use crate::{
    ALPN, ERROR_CODE_UNAUTHORIZED, EndpointPreset, PathKind, PreviewShareTicket, TOKEN_LEN,
    TunnelError, TunnelStatus,
};

/// Per-attempt cap on dialing the host (QUIC handshakes against a dead
/// UDP addr otherwise pend on retransmits for a long time).
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// Exponential re-dial backoff bounds.
const REDIAL_BACKOFF_INITIAL: Duration = Duration::from_millis(250);
const REDIAL_BACKOFF_MAX: Duration = Duration::from_secs(5);

/// How long an accepted local TCP connection waits for the tunnel to come
/// back before being dropped. Callers (browsers, the SPA's health
/// supervisor) retry with fresh connections, so failing one is cheap.
const STREAM_WAIT_TIMEOUT: Duration = Duration::from_secs(30);

/// Poll grain while waiting for the supervisor to notice a dead connection.
const RETRY_POLL_INTERVAL: Duration = Duration::from_millis(100);

/// Configuration for [`TunnelClient::bind`] and [`TunnelClient::connect`].
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
    ///
    /// Equivalent to [`TunnelClient::connect`] followed by
    /// [`TunnelConnection::bind`].
    pub async fn bind(
        cfg: TunnelClientConfig,
        ticket: PreviewShareTicket,
        local: SocketAddr,
    ) -> Result<(SocketAddr, TunnelClientHandle), TunnelError> {
        Self::connect(cfg, ticket).await?.bind(local).await
    }

    /// Dial the ticket's endpoint *without* binding a local listener
    /// (live-share payload plan Phase 3, design decision 1): the caller
    /// drives token-prefixed streams itself via
    /// [`TunnelConnection::open_stream`] — e.g. the join path's
    /// preflight (`/health` + `/api/preview/config` run before the
    /// local port exists) and the L7 join frontend's per-connection
    /// tunnel fallback. The initial dial happens here, same as
    /// [`bind`](Self::bind).
    pub async fn connect(
        cfg: TunnelClientConfig,
        ticket: PreviewShareTicket,
    ) -> Result<TunnelConnection, TunnelError> {
        // Seed a MemoryLookup with the ticket's addresses so re-dials
        // re-resolve without n0 infrastructure.
        let lookup = MemoryLookup::new();
        lookup.add_endpoint_info(ticket.addr.clone());
        let endpoint =
            crate::bind_endpoint(cfg.preset, None, None, |b| b.address_lookup(lookup.clone()))
                .await?;

        let conn = timeout(CONNECT_TIMEOUT, endpoint.connect(ticket.addr.clone(), ALPN))
            .await
            .map_err(|_| TunnelError::Connect("timed out dialing the share host".into()))?
            .map_err(|e| TunnelError::Connect(Box::new(e)))?;

        let (status_tx, status_rx) =
            watch::channel(TunnelStatus::Connected(selected_path_kind(&conn.paths())));
        let shared = Arc::new(Shared {
            endpoint: endpoint.clone(),
            remote: ticket.addr,
            token: ticket.token,
            conn: RwLock::new(conn.clone()),
            conn_generation: AtomicU64::new(0),
            status_tx,
            status_rx: status_rx.clone(),
        });
        tokio::spawn(watch_paths(conn, shared.clone(), 0));

        let supervisor = tokio::spawn(supervise_connection(shared.clone()));
        Ok(TunnelConnection {
            endpoint,
            shared,
            supervisor,
            status_rx,
        })
    }
}

/// A live connection to the share host: the endpoint, the current QUIC
/// connection, and the supervisor that re-dials with backoff when it
/// dies. `open_stream` takes `&self`, so one connection can be shared
/// across many caller tasks (the L7 join frontend's per-connection
/// fallback does exactly that).
pub struct TunnelConnection {
    endpoint: Endpoint,
    shared: Arc<Shared>,
    supervisor: JoinHandle<()>,
    status_rx: watch::Receiver<TunnelStatus>,
}

impl TunnelConnection {
    /// Open one token-prefixed bi-stream to the host, waiting through a
    /// supervisor re-dial when the connection is down (budget:
    /// [`STREAM_WAIT_TIMEOUT`], matching the splice path's
    /// per-connection wait). `None` means the budget ran out or the
    /// host rejected the token (terminal — the same ticket can never
    /// succeed); callers drop the local connection they were serving.
    ///
    /// The token prefix is written before this returns: it
    /// authenticates the stream and satisfies iroh's write-first rule
    /// (the peer's `accept_bi` does not wake until bytes flow), and
    /// payload bytes pipeline right behind it — no extra RTT.
    ///
    /// Never cancel the returned future: dropping it mid-open can reset
    /// a stream before its token lands, which the host treats as an
    /// auth failure and answers by closing the whole connection. Pick a
    /// budget with [`open_stream_with_budget`](Self::open_stream_with_budget)
    /// instead of racing it against a timeout.
    pub async fn open_stream(&self) -> Option<(SendStream, RecvStream)> {
        self.open_stream_with_budget(STREAM_WAIT_TIMEOUT).await
    }

    /// [`open_stream`](Self::open_stream) with a caller-chosen wait
    /// budget — the join preflight probes with a short one so a wedged
    /// host can't park the readiness loop.
    pub async fn open_stream_with_budget(
        &self,
        budget: Duration,
    ) -> Option<(SendStream, RecvStream)> {
        open_stream(&self.shared, budget).await
    }

    /// Watch channel for CLI messaging ("connected via …",
    /// "reconnecting…", "rejected").
    pub fn status(&self) -> watch::Receiver<TunnelStatus> {
        self.status_rx.clone()
    }

    /// Bind the splice accept loop on `local` (port 0 allowed): one
    /// accepted TCP connection = one [`open_stream`](Self::open_stream).
    /// This is [`TunnelClient::bind`]'s accept half, split out so a
    /// caller that already connected (and maybe ran a preflight over
    /// raw streams) keeps the connection it dialed.
    pub async fn bind(
        self,
        local: SocketAddr,
    ) -> Result<(SocketAddr, TunnelClientHandle), TunnelError> {
        let listener = TcpListener::bind(local).await.map_err(TunnelError::Proxy)?;
        let local_addr = listener.local_addr().map_err(TunnelError::Proxy)?;
        let acceptor = tokio::spawn(accept_loop(listener, self.shared));
        Ok((
            local_addr,
            TunnelClientHandle {
                endpoint: self.endpoint,
                supervisor: self.supervisor,
                acceptor,
                status_rx: self.status_rx,
            },
        ))
    }

    /// Stop the re-dial supervisor and close the endpoint gracefully.
    /// Takes `&self` (unlike [`TunnelClientHandle::shutdown`]) so a
    /// shared connection — e.g. held by the join frontend's
    /// per-connection tasks — can still be shut down by its owner.
    pub async fn shutdown(&self) -> Result<(), TunnelError> {
        self.supervisor.abort();
        self.endpoint.close().await;
        Ok(())
    }
}

/// Handle to a running [`TunnelClient`].
#[derive(Debug)]
pub struct TunnelClientHandle {
    endpoint: Endpoint,
    supervisor: JoinHandle<()>,
    acceptor: JoinHandle<()>,
    status_rx: watch::Receiver<TunnelStatus>,
}

impl TunnelClientHandle {
    /// Watch channel for CLI messaging ("connected via …",
    /// "reconnecting…", "rejected").
    pub fn status(&self) -> watch::Receiver<TunnelStatus> {
        self.status_rx.clone()
    }

    /// Abort the accept loop (unbinding the local port) and close the
    /// endpoint gracefully.
    pub async fn shutdown(self) -> Result<(), TunnelError> {
        self.acceptor.abort();
        self.supervisor.abort();
        // Await the aborted tasks so the listener is guaranteed dropped
        // (port unbound) before we return.
        let _ = self.acceptor.await;
        let _ = self.supervisor.await;
        self.endpoint.close().await;
        Ok(())
    }
}

/// Shared state between the accept loop, per-connection tasks, and the
/// connection supervisor.
struct Shared {
    endpoint: Endpoint,
    remote: EndpointAddr,
    token: [u8; TOKEN_LEN],
    conn: RwLock<Connection>,
    /// Bumped on every re-dial; a path watcher only updates the status
    /// when its connection is still the current generation, so a
    /// straggling snapshot from a dying connection can't overwrite the
    /// fresh connection's path kind.
    conn_generation: AtomicU64,
    status_tx: watch::Sender<TunnelStatus>,
    status_rx: watch::Receiver<TunnelStatus>,
}

/// Classify the selected path of a connection snapshot.
fn selected_path_kind(paths: &PathList<'_>) -> PathKind {
    match paths.iter().find(|p| p.is_selected()) {
        Some(p) if p.is_relay() => PathKind::Relay,
        Some(p) if p.is_ip() => PathKind::Direct,
        // Custom transports don't occur here; selection may also simply
        // not have happened yet.
        _ => PathKind::Unknown,
    }
}

/// Follow one connection's path snapshots, updating a live `Connected`
/// status when the selected path changes kind (e.g. relay → direct once
/// hole-punching lands). Ends when the connection closes. Never touches
/// `Reconnecting`/`Rejected` — those belong to the supervisor.
async fn watch_paths(conn: Connection, shared: Arc<Shared>, generation: u64) {
    let mut stream = std::pin::pin!(conn.paths_stream());
    while let Some(paths) = stream.as_mut().next().await {
        let kind = selected_path_kind(&paths);
        if shared.conn_generation.load(Ordering::SeqCst) != generation {
            return; // a newer connection owns the status now
        }
        shared.status_tx.send_if_modified(|status| match status {
            TunnelStatus::Connected(old) if *old != kind => {
                *status = TunnelStatus::Connected(kind);
                true
            }
            _ => false,
        });
    }
}

/// Watches the current connection for death and re-dials with exponential
/// backoff, updating the status channel around the outage. A close carrying
/// the host's "unauthorized" code is terminal: the session token was
/// rejected, so re-dialing with the same ticket can never succeed.
async fn supervise_connection(shared: Arc<Shared>) {
    loop {
        let conn = shared.conn.read().await.clone();
        let reason = conn.closed().await;
        if let ConnectionError::ApplicationClosed(close) = &reason
            && close.error_code == VarInt::from_u32(ERROR_CODE_UNAUTHORIZED)
        {
            tracing::warn!("preview tunnel: the host rejected the session token");
            shared.status_tx.send_replace(TunnelStatus::Rejected);
            return;
        }
        tracing::info!(?reason, "preview tunnel: connection lost; re-dialing");
        shared.status_tx.send_replace(TunnelStatus::Reconnecting);

        let mut delay = REDIAL_BACKOFF_INITIAL;
        loop {
            match timeout(
                CONNECT_TIMEOUT,
                shared.endpoint.connect(shared.remote.clone(), ALPN),
            )
            .await
            {
                Ok(Ok(new_conn)) => {
                    let generation = shared.conn_generation.fetch_add(1, Ordering::SeqCst) + 1;
                    *shared.conn.write().await = new_conn.clone();
                    shared
                        .status_tx
                        .send_replace(TunnelStatus::Connected(selected_path_kind(
                            &new_conn.paths(),
                        )));
                    tokio::spawn(watch_paths(new_conn, shared.clone(), generation));
                    tracing::info!("preview tunnel: reconnected");
                    break;
                }
                Ok(Err(err)) => tracing::debug!(%err, "preview tunnel: re-dial failed"),
                Err(_) => tracing::debug!("preview tunnel: re-dial timed out"),
            }
            tokio::time::sleep(delay).await;
            delay = (delay * 2).min(REDIAL_BACKOFF_MAX);
        }
    }
}

async fn accept_loop(listener: TcpListener, shared: Arc<Shared>) {
    loop {
        match listener.accept().await {
            Ok((tcp, _peer)) => {
                tokio::spawn(handle_local_conn(tcp, shared.clone()));
            }
            Err(err) => {
                tracing::warn!(%err, "preview tunnel: local proxy accept failed");
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        }
    }
}

/// One local TCP connection = one token-prefixed QUIC bi-stream.
async fn handle_local_conn(mut tcp: TcpStream, shared: Arc<Shared>) {
    let Some((send, recv)) = open_stream(&shared, STREAM_WAIT_TIMEOUT).await else {
        return; // budget exhausted or terminally rejected: drop the TCP conn
    };
    let mut quic = tokio::io::join(recv, send);
    match tokio::io::copy_bidirectional(&mut tcp, &mut quic).await {
        Ok((to_host, from_host)) => {
            tracing::debug!(to_host, from_host, "preview tunnel: local conn closed");
        }
        Err(err) => {
            tracing::debug!(%err, "preview tunnel: local conn ended with error");
        }
    }
}

/// Open one token-prefixed bi-stream on the current connection,
/// waiting through a supervisor re-dial when it is down. `None` when
/// the `budget` runs out or the host rejected the token (terminal —
/// re-dialing with the same ticket can never succeed).
async fn open_stream(shared: &Shared, budget: Duration) -> Option<(SendStream, RecvStream)> {
    let deadline = tokio::time::Instant::now() + budget;
    let (mut send, recv) = loop {
        let conn = shared.conn.read().await.clone();
        match conn.open_bi().await {
            Ok(pair) => break pair,
            Err(err) => {
                tracing::debug!(%err, "preview tunnel: open_bi failed; waiting for reconnect");
                // The supervisor notices the dead connection via
                // `closed()` and re-dials; wait for it (with a poll grain
                // in case it has not flipped the status yet), then retry
                // on the — possibly swapped — connection.
                tokio::time::sleep(RETRY_POLL_INTERVAL).await;
                let Some(remaining) = deadline.checked_duration_since(tokio::time::Instant::now())
                else {
                    return None; // budget exhausted
                };
                let mut status = shared.status_rx.clone();
                match timeout(
                    remaining,
                    status.wait_for(|s| !matches!(s, TunnelStatus::Reconnecting)),
                )
                .await
                {
                    // Reconnected: retry open_bi on the swapped connection.
                    Ok(Ok(current)) if matches!(*current, TunnelStatus::Connected(_)) => {}
                    // Rejected (terminal), channel closed, or budget
                    // exhausted.
                    _ => return None,
                }
            }
        }
    };

    // Token prefix: authenticates the stream and satisfies iroh's
    // write-first rule (the peer's accept_bi does not wake until bytes
    // flow). Payload bytes pipeline right behind it — no extra RTT.
    if send.write_all(&shared.token).await.is_err() {
        return None;
    }
    Some((send, recv))
}
