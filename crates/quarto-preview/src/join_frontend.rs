//! Phase 3 of the live-share payload plan (bd-tl2j8js8;
//! `claude-notes/plans/2026-08-13-live-share-local-spa-assets.md`,
//! design decisions 2–3 + 6): the L7 join frontend.
//!
//! A `--join` guest whose embedded SPA manifest hash matches the host's
//! advertised hash already carries the exact bytes the host would
//! serve, so asset requests are answered from the guest's own binary
//! and only the dynamic traffic (`/ws`, `/api/*`, `/auth/*`, `/health`,
//! `/.quarto/*`, unknown paths) crosses the tunnel.
//!
//! Routing is **per connection** via a bounded head-peek (64 KiB cap,
//! 5 s timeout): the frontend reads the request head and routes the
//! whole connection —
//!
//! - a `GET`/`HEAD` whose path — after `spa_handler`'s exact
//!   normalization (query stripped, leading `/` trimmed, empty →
//!   `index.html`, raw percent-encoded, no decoding) — is an **exact
//!   manifest entry** is served from the embedded bundle via the shared
//!   `asset_response` builder (header logic is never forked), plus
//!   `Connection: close`;
//! - anything else is spliced onto a token-prefixed tunnel stream:
//!   `open_stream()`, the consumed head bytes replayed, then
//!   `copy_bidirectional`. WebSocket upgrades pass byte-identical.
//!   Any other head gets `Connection: close` forced in (a hop-by-hop
//!   header a proxy owns) so the host closes the connection after one
//!   response: a browser otherwise reuses an idle tunneled keep-alive
//!   connection for later requests — manifest-hit assets included,
//!   which then cross the tunnel unrouted (Phase 4 e2e finding,
//!   bd-2mpka14m). With per-request connections, keep-alive follow-up
//!   requests simply never happen, and every request gets its own
//!   routing decision.
//!
//! There is deliberately **no local SPA-index fallback**: an unmatched
//! path tunnels to the host, which stays the single authority on what
//! is dynamic — a present-or-future host route can never be shadowed
//! by a locally synthesized `index.html`. `Connection: close` on local
//! responses keeps a browser from mixing local and tunneled requests
//! on one keep-alive connection.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::http::HeaderValue;
use quarto_p2p::{TunnelConnection, TunnelError, TunnelStatus};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch;
use tokio::task::JoinHandle;

use crate::{AssetRequestCtx, PreviewUi, asset_response_parts, embedded_gz, lookup_embedded};

/// Bounds on the per-connection head-peek (design decision 3): the
/// request head must complete within 64 KiB and 5 s, or the connection
/// is answered 431 / dropped. A head that never completes can never be
/// replayed verbatim, so nothing oversize or stalled ever tunnels.
const HEAD_PEEK_CAP: usize = 64 * 1024;
const HEAD_PEEK_TIMEOUT: Duration = Duration::from_secs(5);

/// What a join frontend serves locally; every other connection
/// tunnels. The mode decision itself is the caller's
/// (`decide_asset_mode` compared the hashes) — this is its operational
/// form, and the hermetic fixture knob for the mismatch cases.
pub enum LocalServing {
    /// Manifest hash match: `GET`/`HEAD` requests whose normalized path
    /// is an exact manifest entry are served from this binary's
    /// embedded bundle for `ui`; everything else tunnels.
    Embedded {
        ui: PreviewUi,
        /// This binary's own embedded manifest for `ui`
        /// (`asset_manifest::embedded_manifest`) — the entry set the
        /// hash comparison validated.
        manifest: spa_manifest::Manifest,
    },
    /// No local serving: every connection tunnels (hash mismatch,
    /// missing manifest, `SPA_DIR_OVERRIDE`).
    Tunnel,
}

/// Handle to a running join frontend (the join session's local end).
pub struct JoinFrontendHandle {
    acceptor: JoinHandle<()>,
    conn: Arc<TunnelConnection>,
    status_rx: watch::Receiver<TunnelStatus>,
}

impl JoinFrontendHandle {
    /// Watch channel for CLI messaging ("connected via …",
    /// "reconnecting…", "rejected") — the tunnel connection's own
    /// status, forwarded.
    pub fn status(&self) -> watch::Receiver<TunnelStatus> {
        self.status_rx.clone()
    }

    /// Abort the accept loop (unbinding the local port) and shut the
    /// tunnel connection down gracefully.
    pub async fn shutdown(self) -> Result<(), TunnelError> {
        self.acceptor.abort();
        // Await the aborted task so the listener is guaranteed dropped
        // (port unbound) before we return.
        let _ = self.acceptor.await;
        self.conn.shutdown().await
    }
}

/// Bind the join frontend on `local` (port 0 allowed), serving per
/// `serving` and tunneling everything else over `conn`. Returns the
/// bound address and a handle.
pub async fn bind(
    conn: TunnelConnection,
    local: SocketAddr,
    serving: LocalServing,
) -> Result<(SocketAddr, JoinFrontendHandle), TunnelError> {
    let listener = TcpListener::bind(local).await.map_err(TunnelError::Proxy)?;
    let local_addr = listener.local_addr().map_err(TunnelError::Proxy)?;

    let status_rx = conn.status();
    let conn = Arc::new(conn);
    let serving = Arc::new(Serving::from(serving));

    let acceptor = tokio::spawn({
        let conn = conn.clone();
        async move {
            loop {
                match listener.accept().await {
                    Ok((tcp, _peer)) => {
                        tokio::spawn(handle_conn(tcp, conn.clone(), serving.clone()));
                    }
                    Err(err) => {
                        tracing::warn!(%err, "join frontend: accept failed");
                        tokio::time::sleep(Duration::from_millis(50)).await;
                    }
                }
            }
        }
    });

    Ok((
        local_addr,
        JoinFrontendHandle {
            acceptor,
            conn,
            status_rx,
        },
    ))
}

/// The resolved routing table: the manifest's entry set is the local
/// path set (lookup-only; never iterated or serialized).
enum Serving {
    Embedded {
        ui: PreviewUi,
        paths: std::collections::HashSet<String>,
    },
    Tunnel,
}

impl From<LocalServing> for Serving {
    fn from(serving: LocalServing) -> Self {
        match serving {
            LocalServing::Embedded { ui, manifest } => Serving::Embedded {
                ui,
                paths: manifest.entries.into_iter().map(|e| e.path).collect(),
            },
            LocalServing::Tunnel => Serving::Tunnel,
        }
    }
}

/// One accepted loopback connection: peek the head, then serve locally
/// or tunnel the whole connection.
async fn handle_conn(mut tcp: TcpStream, conn: Arc<TunnelConnection>, serving: Arc<Serving>) {
    match read_head(&mut tcp).await {
        HeadRead::Complete(consumed) => match route(&consumed, &serving) {
            Route::Local {
                rel,
                accept_encoding,
                is_head,
                ui,
            } => {
                serve_local(&mut tcp, &rel, accept_encoding, is_head, ui).await;
            }
            Route::Tunnel => {
                // Every accepted connection is accounted for in the
                // logs: local serves at trace in `serve_local`, tunnels
                // here — the request line names what crossed the tunnel.
                let request_line =
                    String::from_utf8_lossy(consumed.split(|&b| b == b'\r').next().unwrap_or(&[]))
                        .into_owned();
                tracing::debug!(%request_line, "join frontend: tunneling");
                tunnel(&mut tcp, &conn, with_connection_close(&consumed)).await;
            }
        },
        HeadRead::Oversize => {
            // The head never completed within the cap, so it cannot be
            // replayed verbatim — answer 431 and close; nothing tunnels.
            let _ = tcp
                .write_all(
                    b"HTTP/1.1 431 Request Header Fields Too Large\r\n\
                      Content-Length: 0\r\nConnection: close\r\n\r\n",
                )
                .await;
            // Close politely: FIN only after the response bytes, then
            // drain what the peer already sent — dropping the socket
            // with unread receive data makes the kernel RST, which can
            // discard the 431 before the peer reads it.
            let _ = tcp.shutdown().await;
            let _ = tokio::time::timeout(Duration::from_secs(1), async {
                let mut chunk = [0u8; 8192];
                let mut drained = 0usize;
                while drained <= 4 * HEAD_PEEK_CAP {
                    match tcp.read(&mut chunk).await {
                        Ok(0) | Err(_) => break,
                        Ok(n) => drained += n,
                    }
                }
            })
            .await;
        }
        // Stalled or hung-up mid-head: close without a response.
        HeadRead::Timeout | HeadRead::Closed => {}
    }
}

/// The outcome of the bounded head-peek.
enum HeadRead {
    /// The full head (through `\r\n\r\n`) plus any bytes already read
    /// past it — the verbatim replay for a tunneled connection.
    Complete(Vec<u8>),
    /// More than [`HEAD_PEEK_CAP`] bytes without a head terminator.
    Oversize,
    /// [`HEAD_PEEK_TIMEOUT`] elapsed before the head completed.
    Timeout,
    /// The peer hung up (or errored) before the head completed.
    Closed,
}

/// Read the request head: until the `\r\n\r\n` terminator, the byte
/// cap, or the timeout — whichever comes first.
async fn read_head(tcp: &mut TcpStream) -> HeadRead {
    let mut buf = Vec::with_capacity(4096);
    let outcome = tokio::time::timeout(HEAD_PEEK_TIMEOUT, async move {
        loop {
            // Complete heads are checked before the cap so a head
            // straddling the exact cap boundary still counts.
            if let Some(_end) = find_header_end(&buf) {
                return HeadRead::Complete(buf);
            }
            if buf.len() > HEAD_PEEK_CAP {
                return HeadRead::Oversize;
            }
            let mut chunk = [0u8; 8192];
            match tcp.read(&mut chunk).await {
                Ok(0) | Err(_) => return HeadRead::Closed,
                Ok(n) => buf.extend_from_slice(&chunk[..n]),
            }
        }
    })
    .await;
    outcome.unwrap_or(HeadRead::Timeout)
}

/// Index just past the `\r\n\r\n` terminating the request head.
fn find_header_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n").map(|i| i + 4)
}

/// Force `Connection: close` into a tunneled head (the tunnel
/// direction of the mixed-keep-alive mitigation — see the module
/// docs). Upgrade heads (WebSocket) pass byte-identical: their
/// connection must stay open. Any other head has its existing
/// `Connection` lines dropped and a single `Connection: close`
/// appended; bytes past the head terminator (a request body already
/// read) are preserved untouched.
fn with_connection_close(consumed: &[u8]) -> Vec<u8> {
    let Some(head_end) = find_header_end(consumed) else {
        return consumed.to_vec();
    };
    // Lines keep their trailing `\r`; the head ends with a `"\r"`
    // blank line plus a final empty segment.
    let mut lines = consumed[..head_end].split(|&b| b == b'\n');
    let request_line = lines.next().unwrap_or(&[]);
    let mut is_upgrade = false;
    let mut kept: Vec<&[u8]> = Vec::new();
    for line in lines {
        if line == b"\r" || line.is_empty() {
            break;
        }
        let name = match line.iter().position(|&b| b == b':') {
            Some(i) => &line[..i],
            None => line,
        };
        if name.eq_ignore_ascii_case(b"upgrade") {
            is_upgrade = true;
        }
        if !name.eq_ignore_ascii_case(b"connection") {
            kept.push(line);
        }
    }
    if is_upgrade {
        return consumed.to_vec();
    }
    let mut out = Vec::with_capacity(consumed.len() + 20);
    out.extend_from_slice(request_line);
    out.push(b'\n');
    for line in kept {
        out.extend_from_slice(line);
        out.push(b'\n');
    }
    out.extend_from_slice(b"Connection: close\r\n\r\n");
    out.extend_from_slice(&consumed[head_end..]);
    out
}

/// The head-peek's routing decision for one connection.
enum Route {
    /// Serve `rel` from the embedded bundle for `ui`.
    Local {
        rel: String,
        accept_encoding: Option<HeaderValue>,
        is_head: bool,
        ui: PreviewUi,
    },
    /// Splice the whole connection onto a tunnel stream.
    Tunnel,
}

/// Route a complete head (design decision 3). Local serving requires a
/// cleanly parsed `GET`/`HEAD` whose normalized path is an exact
/// manifest entry that the embed actually resolves; anything else —
/// non-GET methods, malformed heads, unknown paths — tunnels, so the
/// host's HTTP stack answers exactly as it would without the frontend.
fn route(consumed: &[u8], serving: &Serving) -> Route {
    let Serving::Embedded { ui, paths } = serving else {
        return Route::Tunnel;
    };
    let Some(parsed) = parse_head(consumed) else {
        return Route::Tunnel;
    };
    if !paths.contains(parsed.rel) {
        return Route::Tunnel;
    }
    // A manifest entry always resolves against this binary's own embed
    // (the manifest is its post-resolution view); a miss must tunnel,
    // never 404 locally.
    if lookup_embedded(*ui, parsed.rel).is_none() {
        return Route::Tunnel;
    }
    Route::Local {
        rel: parsed.rel.to_string(),
        accept_encoding: parsed.accept_encoding,
        is_head: parsed.is_head,
        ui: *ui,
    }
}

/// A cleanly parsed request head, normalized the way `spa_handler`
/// normalizes.
struct ParsedHead<'a> {
    /// `spa_handler`'s exact normalization of the request target:
    /// query stripped, leading `/` trimmed, empty → `index.html`, raw
    /// percent-encoded, no decoding.
    rel: &'a str,
    accept_encoding: Option<HeaderValue>,
    is_head: bool,
}

/// Parse a complete request head strictly enough that local serving
/// never diverges from what the host's HTTP stack would do: a
/// three-token `HTTP/1.x` request line, `GET` or `HEAD`, and
/// well-formed `Name: value` header lines. Anything unusual is `None`
/// (→ tunnel).
fn parse_head(consumed: &[u8]) -> Option<ParsedHead<'_>> {
    let end = find_header_end(consumed)?;
    let text = std::str::from_utf8(consumed.get(..end)?).ok()?;
    let mut lines = text.split("\r\n");
    let request_line = lines.next()?;
    let mut parts = request_line.split(' ');
    let method = parts.next()?;
    let target = parts.next()?;
    let version = parts.next()?;
    if parts.next().is_some() || !version.starts_with("HTTP/1") {
        return None;
    }
    let is_head = match method {
        "GET" => false,
        "HEAD" => true,
        _ => return None,
    };
    let mut accept_encoding = None;
    for line in lines {
        // The head terminator leaves trailing empty segments.
        if line.is_empty() {
            continue;
        }
        let (name, value) = line.split_once(':')?;
        if name.is_empty() || name.bytes().any(|b| b.is_ascii_whitespace()) {
            return None;
        }
        if name.eq_ignore_ascii_case("accept-encoding") {
            accept_encoding = HeaderValue::from_str(value.trim()).ok();
        }
    }
    let path = target.split('?').next().unwrap_or(target);
    let rel = path.trim_start_matches('/');
    let rel = if rel.is_empty() { "index.html" } else { rel };
    Some(ParsedHead {
        rel,
        accept_encoding,
        is_head,
    })
}

/// Serve a manifest-hit asset from the embedded bundle, byte-identical
/// to what the host's `asset_response` produces for the same request
/// (the shared builder — header logic is never forked), plus
/// `Connection: close` (the mixed-keep-alive mitigation, design
/// decision 3). The connection closes with the response.
async fn serve_local(
    tcp: &mut TcpStream,
    rel: &str,
    accept_encoding: Option<HeaderValue>,
    is_head: bool,
    ui: PreviewUi,
) {
    let Some(file) = lookup_embedded(ui, rel) else {
        return; // route() already checked; defensive
    };
    let parts = asset_response_parts(
        rel,
        file.to_vec(),
        embedded_gz(ui, rel),
        AssetRequestCtx {
            accept_encoding: accept_encoding.as_ref(),
            is_head,
        },
    );
    let mut out = Vec::with_capacity(parts.body.len() + 512);
    out.extend_from_slice(
        format!(
            "HTTP/1.1 {} {}\r\n",
            parts.status.as_u16(),
            parts.status.canonical_reason().unwrap_or("OK")
        )
        .as_bytes(),
    );
    for (name, value) in &parts.headers {
        out.extend_from_slice(name.as_str().as_bytes());
        out.extend_from_slice(b": ");
        out.extend_from_slice(value.as_bytes());
        out.extend_from_slice(b"\r\n");
    }
    out.extend_from_slice(b"connection: close\r\n\r\n");
    out.extend_from_slice(&parts.body);
    let _ = tcp.write_all(&out).await;
    tracing::trace!(rel, "join frontend: served locally");
}

/// Tunnel a whole connection: one token-prefixed stream, the consumed
/// head bytes replayed verbatim, then a bidirectional splice.
/// WebSocket upgrades and keep-alive follow-up requests flow through
/// untouched.
async fn tunnel(tcp: &mut TcpStream, conn: &TunnelConnection, consumed: Vec<u8>) {
    let Some((send, recv)) = conn.open_stream().await else {
        return; // budget exhausted or terminally rejected: drop the conn
    };
    let mut quic = tokio::io::join(recv, send);
    if quic.write_all(&consumed).await.is_err() {
        return;
    }
    match tokio::io::copy_bidirectional(tcp, &mut quic).await {
        Ok((to_host, from_host)) => {
            tracing::debug!(to_host, from_host, "join frontend: tunneled conn closed");
        }
        Err(err) => {
            tracing::debug!(%err, "join frontend: tunneled conn ended with error");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::with_connection_close;

    #[test]
    fn injects_close_into_a_headerless_head() {
        let head = b"GET /health HTTP/1.1\r\n\r\n";
        assert_eq!(
            with_connection_close(head),
            b"GET /health HTTP/1.1\r\nConnection: close\r\n\r\n"
        );
    }

    #[test]
    fn replaces_keep_alive_and_preserves_the_other_headers() {
        let head = b"GET / HTTP/1.1\r\nHost: x\r\nConnection: keep-alive\r\nAccept: */*\r\n\r\n";
        assert_eq!(
            with_connection_close(head),
            b"GET / HTTP/1.1\r\nHost: x\r\nAccept: */*\r\nConnection: close\r\n\r\n"
        );
    }

    #[test]
    fn upgrade_heads_pass_verbatim() {
        let head = b"GET /ws HTTP/1.1\r\nConnection: Upgrade\r\nUpgrade: websocket\r\nSec-WebSocket-Key: abc\r\n\r\n";
        assert_eq!(with_connection_close(head), head);
    }

    #[test]
    fn bytes_past_the_terminator_are_preserved() {
        let mut head = b"POST /api/x HTTP/1.1\r\nContent-Length: 4\r\n\r\n".to_vec();
        head.extend_from_slice(b"BODY");
        let mut want =
            b"POST /api/x HTTP/1.1\r\nContent-Length: 4\r\nConnection: close\r\n\r\n".to_vec();
        want.extend_from_slice(b"BODY");
        assert_eq!(with_connection_close(&head), want);
    }

    #[test]
    fn a_head_without_terminator_passes_verbatim() {
        let head = b"GET /never-terminated";
        assert_eq!(with_connection_close(head), head);
    }
}
