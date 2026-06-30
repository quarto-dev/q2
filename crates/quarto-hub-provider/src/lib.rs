//! Connect a native `q2` process to a hub's automerge session as a
//! code-execution provider (bd-sfet3264, Phase 3).
//!
//! The hybrid architecture (D1=C): a Node child owns the OAuth/keyring auth
//! and streams Bearer tokens; this Rust side owns the automerge sync + (later,
//! Phase 4) the engine execution. It joins the hub as a samod **client peer**
//! by dialing the remote `/ws` with a [`BearerDialer`] that injects an
//! `Authorization: Bearer <jwt>` header — re-fetched on every (re)connect from
//! a [`TokenSource`].
//!
//! Phase 3 deliverable is narrow: join an authenticated hub, `find()` the
//! index document, and list the project files. Execution, the capability
//! beacon, and temp-dir materialization land in Phase 4.

mod dialer;
mod join;
mod token;

pub use dialer::BearerDialer;
pub use join::{JoinConfig, join_and_list_files};
pub use token::{StaticTokenSource, TokenSource};

/// Errors from joining a hub as an execution provider.
#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    /// Failed to build or perform the websocket handshake (bad URL, header).
    #[error("websocket handshake error: {0}")]
    Handshake(String),

    /// A transport-level protocol violation (e.g. a text frame on the sync
    /// socket, or a send/receive error).
    #[error("sync transport error: {0}")]
    Protocol(String),

    /// The auth bridge could not provide a token.
    #[error("token error: {0}")]
    Token(String),

    /// The samod repo was stopped (or could not be reached).
    #[error("repo error: {0}")]
    Repo(String),

    /// The index document could not be found or loaded.
    #[error("index document error: {0}")]
    Index(String),
}
