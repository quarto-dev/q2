//! Bearer-token supply for the [`BearerDialer`](crate::BearerDialer).

use std::future::Future;
use std::pin::Pin;

use crate::ProviderError;

/// A future yielding a fresh Bearer token (Send: the dialer runs on the
/// multi-threaded runtime and samod's `Dialer::connect` returns a Send future).
pub type BearerFuture = Pin<Box<dyn Future<Output = Result<String, ProviderError>> + Send>>;

/// Supplies a fresh Bearer token for each (re)connect.
///
/// Called by [`BearerDialer::connect`](crate::BearerDialer) on the initial
/// dial and every reconnection, so the implementation can hand back the latest
/// token the Node auth bridge has streamed (Phase 3C) — keeping reconnections
/// authenticated across token refreshes without restarting the process.
pub trait TokenSource: Send + Sync + 'static {
    fn fresh_bearer(&self) -> BearerFuture;
}

/// A fixed token. Used by tests and the dev `--token` path before the Node
/// auth bridge exists (Phase 3B).
pub struct StaticTokenSource(pub String);

impl StaticTokenSource {
    pub fn new(token: impl Into<String>) -> Self {
        Self(token.into())
    }
}

impl TokenSource for StaticTokenSource {
    fn fresh_bearer(&self) -> BearerFuture {
        let token = self.0.clone();
        Box::pin(async move { Ok(token) })
    }
}
