//! A samod [`Dialer`] that injects an `Authorization: Bearer` header.
//!
//! samod's built-in `dial_websocket` cannot set request headers, so an
//! authenticated hub (`/ws` behind a Bearer JWT) is unreachable with it. This
//! dialer reimplements the (small) tungstenite connect path with the header
//! added, fetching a fresh token from a [`TokenSource`] on every (re)connect.
//!
//! The byte-mapping mirrors samod's private `ws_to_bytes`: binary frames carry
//! sync bytes, Ping/Pong are transport-level and dropped, Close ends the
//! stream, and a Text frame is a protocol error. We replicate it here (it is
//! not part of samod's public API) rather than fork samod.

use std::sync::Arc;

use futures::future::BoxFuture;
use futures::{SinkExt, StreamExt};
use samod::{Dialer, Transport};
use tungstenite::Message;
use tungstenite::client::IntoClientRequest;
use tungstenite::handshake::client::Request;
use tungstenite::http::header::AUTHORIZATION;
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

/// Build a websocket client handshake request for `url` carrying an
/// `Authorization: Bearer <bearer>` header.
pub(crate) fn build_auth_request(url: &Url, bearer: &str) -> Result<Request, ProviderError> {
    let mut request = url
        .as_str()
        .into_client_request()
        .map_err(|e| ProviderError::Handshake(format!("invalid websocket url: {e}")))?;
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
    fn url(&self) -> Url {
        self.url.clone()
    }

    fn connect(
        &self,
    ) -> BoxFuture<'static, Result<Transport, Box<dyn std::error::Error + Send + Sync + 'static>>>
    {
        let url = self.url.clone();
        let token_source = self.token_source.clone();
        Box::pin(async move {
            // Fresh token per (re)connect — the auth bridge may have refreshed
            // it since the last attempt.
            let bearer = token_source.fresh_bearer().await?;
            let request = build_auth_request(&url, &bearer)?;

            let (ws, _response) = tokio_tungstenite::connect_async(request).await?;
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
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
