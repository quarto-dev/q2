//! samod [`Dialer`]s that add handshake headers.
//!
//! samod's built-in `dial_websocket` cannot set request headers, so a hub
//! behind an authenticating front is unreachable with it. These dialers
//! reimplement the (small) tungstenite connect path with headers added:
//!
//! - [`BearerDialer`] sends `Authorization: Bearer`, fetching a fresh token
//!   from a [`TokenSource`] on every (re)connect.
//! - [`HeaderDialer`] sends a fixed set of headers, such as the relay key a
//!   token-gated gateway in front of a hub requires.
//!
//! The byte-mapping mirrors samod's private `ws_to_bytes`: binary frames carry
//! sync bytes, Ping/Pong are transport-level and dropped, Close ends the
//! stream, and a Text frame is a protocol error. We replicate it here (it is
//! not part of samod's public API) rather than fork samod.

use std::sync::Arc;

use futures::future::BoxFuture;
use futures::{SinkExt, StreamExt};
use samod::{DialError, Dialer, Transport};
use tungstenite::Message;
use tungstenite::client::IntoClientRequest;
use tungstenite::handshake::client::Request;
use tungstenite::http::header::{AUTHORIZATION, HeaderName, HeaderValue};
use url::Url;

use crate::ProviderError;
use crate::token::TokenSource;

/// A samod [`Dialer`] that dials `url` over an authenticated websocket.
pub struct BearerDialer {
    url: Url,
    token_source: Arc<dyn TokenSource>,
}

impl BearerDialer {
    pub fn new(url: Url, token_source: Arc<dyn TokenSource>) -> Self {
        Self { url, token_source }
    }
}

/// A samod [`Dialer`] that dials `url` with a fixed set of handshake headers.
///
/// The headers are validated once, at construction: unlike a bearer token,
/// they never change between attempts, so a malformed one could never
/// succeed on retry.
pub struct HeaderDialer {
    url: Url,
    headers: Vec<(HeaderName, HeaderValue)>,
}

impl HeaderDialer {
    /// A dialer for `url` sending each `(name, value)` pair on every
    /// handshake. Fails with [`ProviderError::Handshake`] when a name or a
    /// value is not legal in an HTTP header.
    pub fn new(url: Url, headers: Vec<(String, String)>) -> Result<Self, ProviderError> {
        let headers = headers
            .into_iter()
            .map(|(name, value)| {
                let name = HeaderName::from_bytes(name.as_bytes()).map_err(|_| {
                    ProviderError::Handshake(format!("'{name}' is not a valid header name"))
                })?;
                let value = HeaderValue::from_str(&value).map_err(|_| {
                    ProviderError::Handshake(format!("the value of '{name}' is not valid"))
                })?;
                Ok((name, value))
            })
            .collect::<Result<Vec<_>, ProviderError>>()?;
        Ok(Self { url, headers })
    }

    /// The handshake request this dialer sends.
    pub(crate) fn build_request(&self) -> Result<Request, ProviderError> {
        let mut request = websocket_request(&self.url)?;
        for (name, value) in &self.headers {
            request.headers_mut().insert(name.clone(), value.clone());
        }
        Ok(request)
    }
}

/// A websocket client handshake request for `url`, with no extra headers.
fn websocket_request(url: &Url) -> Result<Request, ProviderError> {
    // tungstenite validates the scheme only at connect time; reject it here
    // so a bad --server URL is a permanent, nameable failure (bd-q96fhj3q).
    match url.scheme() {
        "ws" | "wss" => {}
        scheme => {
            return Err(ProviderError::InvalidUrl(format!(
                "unsupported scheme '{scheme}'"
            )));
        }
    }
    url.as_str()
        .into_client_request()
        .map_err(|e| ProviderError::InvalidUrl(e.to_string()))
}

/// Build a websocket client handshake request for `url` carrying an
/// `Authorization: Bearer <bearer>` header.
pub(crate) fn build_auth_request(url: &Url, bearer: &str) -> Result<Request, ProviderError> {
    let mut request = websocket_request(url)?;
    let value = format!("Bearer {bearer}")
        .parse()
        .map_err(|_| ProviderError::Handshake("bearer token is not a valid header value".into()))?;
    request.headers_mut().insert(AUTHORIZATION, value);
    Ok(request)
}

/// Map an inbound tungstenite message to the transport's byte protocol.
///
/// `Some(Ok(bytes))` for a binary sync frame, `None` to drop the frame
/// (Close / Ping / Pong / raw Frame), `Some(Err(_))` for a protocol violation
/// (an unexpected text frame on the sync socket).
pub(crate) fn inbound_to_bytes(msg: Message) -> Option<Result<Vec<u8>, ProviderError>> {
    match msg {
        Message::Binary(data) => Some(Ok(data.to_vec())),
        Message::Close(_) | Message::Ping(_) | Message::Pong(_) | Message::Frame(_) => None,
        Message::Text(_) => Some(Err(ProviderError::Protocol(
            "unexpected text message on sync websocket".into(),
        ))),
    }
}

/// Wrap outbound sync bytes as a binary websocket frame.
pub(crate) fn outbound_to_ws(bytes: Vec<u8>) -> Message {
    Message::Binary(bytes.into())
}

impl Dialer for BearerDialer {
    // URL-shaped failures can never succeed on retry — the URL is fixed at
    // construction — so they are the one permanent case (bd-q96fhj3q).
    // Everything else stays transient, matching samod 0.13: a fresh token is
    // fetched per attempt (auth rejections can clear), and connect errors
    // are network.
    type Error = ProviderError;

    fn url(&self) -> Url {
        self.url.clone()
    }

    fn connect(&self) -> BoxFuture<'static, Result<Transport, DialError<Self::Error>>> {
        let url = self.url.clone();
        let token_source = self.token_source.clone();
        Box::pin(async move {
            // Fresh token per (re)connect — the auth bridge may have refreshed
            // it since the last attempt.
            let bearer = token_source
                .fresh_bearer()
                .await
                .map_err(DialError::transient)?;
            let request = match build_auth_request(&url, &bearer) {
                Ok(request) => request,
                Err(err @ ProviderError::InvalidUrl(_)) => {
                    return Err(DialError::PermanentFailure(err));
                }
                Err(err) => return Err(DialError::transient(err)),
            };
            connect_transport(request).await
        })
    }
}

impl Dialer for HeaderDialer {
    // The headers were validated at construction, so the only permanent
    // failure left is the URL's, as for BearerDialer (bd-q96fhj3q).
    type Error = ProviderError;

    fn url(&self) -> Url {
        self.url.clone()
    }

    fn connect(&self) -> BoxFuture<'static, Result<Transport, DialError<Self::Error>>> {
        let request = self.build_request();
        Box::pin(async move {
            let request = request.map_err(|err| match err {
                ProviderError::InvalidUrl(_) => DialError::PermanentFailure(err),
                err => DialError::transient(err),
            })?;
            connect_transport(request).await
        })
    }
}

/// Perform the websocket handshake for `request` and wrap the socket as a
/// samod [`Transport`].
async fn connect_transport(request: Request) -> Result<Transport, DialError<ProviderError>> {
    let (ws, _response) = match tokio_tungstenite::connect_async(request).await {
        Ok(ok) => ok,
        // A URL error at connect (e.g. a scheme tungstenite rejects)
        // is a property of the fixed URL, never of the network.
        Err(tungstenite::Error::Url(err)) => {
            return Err(DialError::PermanentFailure(ProviderError::InvalidUrl(
                err.to_string(),
            )));
        }
        Err(err) => return Err(DialError::transient(err)),
    };
    let (write, read) = ws.split();

    // Inbound: tungstenite frames -> sync bytes (dropping control frames).
    let msg_stream = read
        .filter_map(|res| async move {
            match res {
                Ok(msg) => inbound_to_bytes(msg),
                Err(e) => Some(Err(ProviderError::Protocol(format!(
                    "websocket receive error: {e}"
                )))),
            }
        })
        .boxed();

    // Outbound: sync bytes -> binary frames.
    let msg_sink = write
        .sink_map_err(|e| ProviderError::Protocol(format!("websocket send error: {e}")))
        .with(|bytes: Vec<u8>| {
            futures::future::ready(Ok::<Message, ProviderError>(outbound_to_ws(bytes)))
        });

    Ok(Transport::new(msg_stream, msg_sink))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::token::{BearerFuture, StaticTokenSource, TokenSource};

    #[test]
    fn auth_request_carries_bearer_header_and_target() {
        let url = Url::parse("wss://hub.example.com/ws").unwrap();
        let req = build_auth_request(&url, "tok-123").unwrap();

        let auth = req
            .headers()
            .get(AUTHORIZATION)
            .expect("Authorization header present");
        assert_eq!(auth, "Bearer tok-123");
        assert_eq!(req.uri().host(), Some("hub.example.com"));
        assert_eq!(req.uri().path(), "/ws");
    }

    #[test]
    fn auth_request_rejects_a_token_with_invalid_header_bytes() {
        let url = Url::parse("wss://hub.example.com/ws").unwrap();
        // A newline is not a legal header value byte.
        let err = build_auth_request(&url, "bad\ntoken").unwrap_err();
        assert!(matches!(err, ProviderError::Handshake(_)));
    }

    #[test]
    fn binary_frames_map_to_their_bytes() {
        let out = inbound_to_bytes(Message::Binary(vec![1, 2, 3].into()));
        assert_eq!(out.unwrap().unwrap(), vec![1, 2, 3]);
    }

    #[test]
    fn control_frames_are_dropped() {
        assert!(inbound_to_bytes(Message::Close(None)).is_none());
        assert!(inbound_to_bytes(Message::Ping(vec![].into())).is_none());
        assert!(inbound_to_bytes(Message::Pong(vec![].into())).is_none());
    }

    #[test]
    fn text_frames_are_a_protocol_error() {
        let out = inbound_to_bytes(Message::Text("hello".into()));
        assert!(matches!(out, Some(Err(ProviderError::Protocol(_)))));
    }

    #[test]
    fn outbound_bytes_become_a_binary_frame() {
        match outbound_to_ws(vec![4, 5, 6]) {
            Message::Binary(data) => assert_eq!(data.to_vec(), vec![4, 5, 6]),
            other => panic!("expected Binary frame, got {other:?}"),
        }
    }

    // bd-q96fhj3q: Url::parse accepts https://; tungstenite rejects it when
    // the handshake request is built. That case is a permanent
    // misconfiguration (the URL is fixed at construction) and must be
    // distinguishable from the bearer-header case, which stays transient
    // because a fresh token is fetched per attempt.
    #[test]
    fn auth_request_rejects_a_non_ws_scheme_as_invalid_url() {
        let url = Url::parse("https://hub.example.com/ws").unwrap();
        let err = build_auth_request(&url, "tok-123").unwrap_err();
        assert!(
            matches!(err, ProviderError::InvalidUrl(_)),
            "expected InvalidUrl, got {err:?}"
        );
    }

    #[tokio::test]
    async fn connect_with_a_non_ws_scheme_is_a_permanent_failure() {
        let dialer = BearerDialer::new(
            Url::parse("https://hub.example.com/ws").unwrap(),
            Arc::new(StaticTokenSource::new("tok-123")),
        );
        match dialer.connect().await {
            Err(err @ DialError::PermanentFailure(_)) => {
                let msg = err.to_string();
                assert!(
                    msg.contains("invalid websocket url"),
                    "permanent failure should name the URL as the cause, got: {msg}"
                );
            }
            Err(err) => panic!("expected a permanent failure, got transient: {err}"),
            Ok(_) => panic!("expected a permanent failure, got a live transport"),
        }
    }

    #[tokio::test]
    async fn connect_with_an_invalid_bearer_header_stays_transient() {
        // The bearer is refetched per attempt, so a header-invalid token can
        // clear on retry — this case must NOT become permanent.
        let dialer = BearerDialer::new(
            Url::parse("wss://hub.example.com/ws").unwrap(),
            Arc::new(StaticTokenSource::new("bad\ntoken")),
        );
        match dialer.connect().await {
            Err(DialError::TransientFailure(_)) => {}
            Err(err) => panic!("expected a transient failure, got permanent: {err}"),
            Ok(_) => panic!("expected a transient failure, got a live transport"),
        }
    }

    fn relay_headers() -> Vec<(String, String)> {
        vec![
            ("X-Relay-Key".to_string(), "secret-1".to_string()),
            ("X-Other".to_string(), "two".to_string()),
        ]
    }

    #[test]
    fn header_request_carries_every_configured_header_and_target() {
        let dialer = HeaderDialer::new(
            Url::parse("ws://127.0.0.1:40404/ws").unwrap(),
            relay_headers(),
        )
        .unwrap();
        let req = dialer.build_request().unwrap();

        assert_eq!(req.headers().get("x-relay-key").unwrap(), "secret-1");
        assert_eq!(req.headers().get("x-other").unwrap(), "two");
        assert_eq!(req.headers().get(AUTHORIZATION), None);
        assert_eq!(req.uri().host(), Some("127.0.0.1"));
        assert_eq!(req.uri().port_u16(), Some(40404));
        assert_eq!(req.uri().path(), "/ws");
    }

    #[test]
    fn header_dialer_rejects_an_invalid_header_name_at_construction() {
        let err = HeaderDialer::new(
            Url::parse("ws://127.0.0.1:40404/ws").unwrap(),
            vec![("not a header".to_string(), "v".to_string())],
        )
        .err()
        .expect("a header name with spaces must be rejected");
        assert!(matches!(err, ProviderError::Handshake(_)), "got {err:?}");
    }

    #[test]
    fn header_dialer_rejects_an_invalid_header_value_at_construction() {
        let err = HeaderDialer::new(
            Url::parse("ws://127.0.0.1:40404/ws").unwrap(),
            vec![("X-Relay-Key".to_string(), "bad\nvalue".to_string())],
        )
        .err()
        .expect("a header value with a newline must be rejected");
        assert!(matches!(err, ProviderError::Handshake(_)), "got {err:?}");
    }

    #[tokio::test]
    async fn header_dialer_connect_with_a_non_ws_scheme_is_a_permanent_failure() {
        let dialer = HeaderDialer::new(
            Url::parse("https://127.0.0.1:40404/ws").unwrap(),
            relay_headers(),
        )
        .unwrap();
        match dialer.connect().await {
            Err(err @ DialError::PermanentFailure(_)) => {
                assert!(
                    err.to_string().contains("invalid websocket url"),
                    "permanent failure should name the URL as the cause, got: {err}"
                );
            }
            Err(err) => panic!("expected a permanent failure, got transient: {err}"),
            Ok(_) => panic!("expected a permanent failure, got a live transport"),
        }
    }

    struct FailingTokenSource;

    impl TokenSource for FailingTokenSource {
        fn fresh_bearer(&self) -> BearerFuture {
            Box::pin(async { Err(ProviderError::Token("auth bridge is down".into())) })
        }
    }

    #[tokio::test]
    async fn connect_with_a_failing_token_source_stays_transient() {
        let dialer = BearerDialer::new(
            Url::parse("wss://hub.example.com/ws").unwrap(),
            Arc::new(FailingTokenSource),
        );
        match dialer.connect().await {
            Err(DialError::TransientFailure(_)) => {}
            Err(err) => panic!("expected a transient failure, got permanent: {err}"),
            Ok(_) => panic!("expected a transient failure, got a live transport"),
        }
    }
}
