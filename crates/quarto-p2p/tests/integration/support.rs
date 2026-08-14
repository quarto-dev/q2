//! Shared helpers for the quarto-p2p integration tests.

use std::net::SocketAddr;
use std::time::Duration;

use quarto_p2p::{EndpointPreset, TunnelClientConfig, TunnelHostConfig};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// Generous cap for individual awaits so a broken tunnel fails the test
/// instead of hanging it. Sized for CI, not for the happy path: iroh
/// endpoint operations inflate 5-10x under full-suite load (every
/// endpoint bind pays for netmon setup + handshake crypto on contended
/// cores), and ubuntu runners are the slowest we have. Never binds on a
/// healthy run — the uncontended cost of a step is milliseconds.
pub const STEP_TIMEOUT: Duration = Duration::from_secs(60);

pub fn hermetic_host_cfg() -> TunnelHostConfig {
    TunnelHostConfig {
        preset: EndpointPreset::HermeticLoopback,
        ..Default::default()
    }
}

pub fn hermetic_client_cfg() -> TunnelClientConfig {
    TunnelClientConfig {
        preset: EndpointPreset::HermeticLoopback,
    }
}

/// Serve `app` on a fresh loopback port; returns the bound address.
///
/// The server task is detached — it dies with the test process, which is
/// fine for these hermetic tests.
pub async fn spawn_http_target(app: axum::Router) -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind http target");
    let addr = listener.local_addr().expect("target local_addr");
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("axum serve");
    });
    addr
}

/// Raw TCP echo target: every accepted connection gets its own bytes
/// back. The `connect()` tests use it because `TunnelClient::connect`
/// is a transport seam — no HTTP involved. Detached like
/// [`spawn_http_target`].
pub async fn spawn_tcp_echo_target() -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind echo target");
    let addr = listener.local_addr().expect("echo local_addr");
    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else {
                return;
            };
            tokio::spawn(async move {
                let mut buf = [0u8; 8192];
                loop {
                    match sock.read(&mut buf).await {
                        Ok(0) | Err(_) => return,
                        Ok(n) => {
                            if sock.write_all(&buf[..n]).await.is_err() {
                                return;
                            }
                        }
                    }
                }
            });
        }
    });
    addr
}

/// Raw HTTP/1.1 GET with `Connection: close`; returns the full response
/// text (status line + headers + body). Panics on I/O failure.
pub async fn http_get_close(addr: SocketAddr, path: &str) -> String {
    try_http_get_close(addr, path)
        .await
        .expect("http_get_close roundtrip")
}

/// Non-panicking variant of [`http_get_close`] for eventually-succeeds
/// loops (e.g. while the client re-dials).
pub async fn try_http_get_close(addr: SocketAddr, path: &str) -> std::io::Result<String> {
    let mut stream = TcpStream::connect(addr).await?;
    stream
        .write_all(
            format!("GET {path} HTTP/1.1\r\nHost: tunnel-test\r\nConnection: close\r\n\r\n")
                .as_bytes(),
        )
        .await?;
    let mut buf = Vec::new();
    stream.read_to_end(&mut buf).await?;
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

/// Send a GET on an already-open connection *without* `Connection: close`
/// and read exactly one response (headers + `Content-Length` body).
/// Returns the full response text; the connection stays usable.
pub async fn http_get_keepalive(stream: &mut TcpStream, path: &str) -> String {
    stream
        .write_all(format!("GET {path} HTTP/1.1\r\nHost: tunnel-test\r\n\r\n").as_bytes())
        .await
        .expect("write keep-alive request");

    // Read until end of headers.
    let mut buf = Vec::new();
    let header_end = loop {
        let mut byte = [0u8; 1];
        let n = stream.read(&mut byte).await.expect("read response header");
        assert!(n > 0, "connection closed while reading response headers");
        buf.push(byte[0]);
        if buf.ends_with(b"\r\n\r\n") {
            break buf.len();
        }
        assert!(buf.len() < 64 * 1024, "response headers too large");
    };

    let headers = String::from_utf8_lossy(&buf[..header_end]).into_owned();
    let content_length: usize = headers
        .lines()
        .find_map(|l| {
            let (name, value) = l.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse().expect("content-length value"))
        })
        .expect("response has a Content-Length header");

    let mut body = vec![0u8; content_length];
    stream
        .read_exact(&mut body)
        .await
        .expect("read response body");
    format!("{headers}{}", String::from_utf8_lossy(&body))
}
