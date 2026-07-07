//! The Node auth bridge: spawn the bundled `auth-stream` helper and expose its
//! streamed Bearer tokens as a [`TokenSource`] (bd-sfet3264, Phase 3C).
//!
//! Topology (D1=C): this Rust process is the long-running parent. It spawns the
//! Node helper (`auth-stream.mjs`, extracted from the embedded hub-mcp bundle)
//! as a child, reads newline-delimited token frames from its **stdout**, and
//! can write `{"type":"refresh"}` to its **stdin** to pull a fresh token. The
//! helper's logs + interactive sign-in URL go to its **stderr**, which we
//! inherit so the user sees them. stdio pipes are identical across platforms.

use std::process::Stdio;
use std::sync::Arc;
use std::sync::Mutex;

use serde::Deserialize;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::{Mutex as AsyncMutex, Notify};

use quarto_mcp_launcher::{
    Discovery, ExtractedBundle, bundled_defaults, content_hash, default_cache_root, embedded_files,
    extract_and_lock, find_node, injections, is_placeholder,
};

use crate::ProviderError;
use crate::token::{BearerFuture, TokenSource};

/// A frame on the helper's stdout. Matches the wire shape emitted by
/// `ts-packages/quarto-hub-mcp/src/auth-stream/protocol.ts`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub(crate) enum AuthFrame {
    Token {
        bearer: String,
        #[serde(rename = "expiresAt", default)]
        #[allow(dead_code)]
        expires_at: String,
    },
    Error {
        message: String,
    },
}

/// Parse one stdout line into a frame, or `None` for blank/unrecognized lines
/// (the helper writes logs to stderr, but be defensive about stray stdout).
pub(crate) fn parse_auth_frame(line: &str) -> Option<AuthFrame> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }
    serde_json::from_str::<AuthFrame>(trimmed).ok()
}

/// Latest-token cache shared between the stdout-reader task and waiters.
#[derive(Debug)]
enum TokenState {
    Pending,
    Ready(String),
    Failed(String),
}

#[derive(Debug)]
struct TokenCache {
    state: Mutex<TokenState>,
    notify: Notify,
}

impl TokenCache {
    fn new() -> Self {
        Self {
            state: Mutex::new(TokenState::Pending),
            notify: Notify::new(),
        }
    }

    fn set_ready(&self, bearer: String) {
        *self.state.lock().expect("token cache poisoned") = TokenState::Ready(bearer);
        self.notify.notify_waiters();
    }

    /// An error frame is fatal only while we have no token yet; an error after
    /// a good token is non-fatal (the existing token may still be valid).
    fn set_failed_if_pending(&self, message: String) {
        let mut state = self.state.lock().expect("token cache poisoned");
        if matches!(*state, TokenState::Pending) {
            *state = TokenState::Failed(message);
            self.notify.notify_waiters();
        }
    }

    /// Resolve to the current Bearer, waiting for the first one if the helper
    /// is still authenticating. Errors if the helper reported a fatal failure.
    async fn get(&self) -> Result<String, ProviderError> {
        loop {
            // Register for notification *before* inspecting the state so a
            // concurrent `notify_waiters` between the check and the await is
            // not lost.
            let notified = self.notify.notified();
            {
                let state = self.state.lock().expect("token cache poisoned");
                match &*state {
                    TokenState::Ready(token) => return Ok(token.clone()),
                    TokenState::Failed(message) => {
                        return Err(ProviderError::Token(message.clone()));
                    }
                    TokenState::Pending => {}
                }
            }
            notified.await;
        }
    }
}

/// A running Node auth bridge. Holds the child process and the extracted-bundle
/// lock for its lifetime; dropping it kills the child (`kill_on_drop`).
pub struct NodeBridge {
    _child: Child,
    // Async mutex: the write is held across `.await` points (a std Mutex
    // can't be, and would risk a deadlock).
    stdin: AsyncMutex<ChildStdin>,
    cache: Arc<TokenCache>,
    // Holds the lifetime shared lock on the extracted bundle dir.
    _extracted: ExtractedBundle,
}

impl NodeBridge {
    /// Spawn the bundled auth helper. Must be called within a Tokio runtime
    /// (it spawns a background stdout-reader task).
    pub fn spawn() -> Result<Self, ProviderError> {
        if is_placeholder() {
            return Err(ProviderError::Token(
                "the hub-mcp bundle is not built; run `cargo xtask build-hub-mcp-bundle`".into(),
            ));
        }

        let files = embedded_files();
        let hash = content_hash(&files);
        let cache_root =
            default_cache_root().map_err(|e| ProviderError::Token(format!("cache dir: {e}")))?;
        let extracted = extract_and_lock(&cache_root, &files, &hash)
            .map_err(|e| ProviderError::Token(format!("extract bundle: {e}")))?;
        let entry = extracted.dir.join("auth-stream.mjs");

        let node = find_node(&Discovery::from_env())
            .map_err(|e| ProviderError::Token(format!("node discovery: {e}")))?;

        let mut command = Command::new(&node.path);
        command
            .arg(&entry)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            // Inherit stderr so the user sees the sign-in URL + helper logs.
            .stderr(Stdio::inherit())
            .kill_on_drop(true);

        // Inject the bundled OAuth client id/secret/server when the user's env
        // doesn't already set them (mirrors the `q2 mcp` launcher).
        for (var, value) in injections(&bundled_defaults(), |k| std::env::var(k).ok()) {
            command.env(var, value);
        }

        let mut child = command
            .spawn()
            .map_err(|e| ProviderError::Token(format!("spawn node auth helper: {e}")))?;

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| ProviderError::Token("auth helper has no stdout".into()))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| ProviderError::Token("auth helper has no stdin".into()))?;

        let cache = Arc::new(TokenCache::new());
        let reader_cache = cache.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(stdout).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                match parse_auth_frame(&line) {
                    Some(AuthFrame::Token { bearer, .. }) => reader_cache.set_ready(bearer),
                    Some(AuthFrame::Error { message }) => {
                        reader_cache.set_failed_if_pending(message)
                    }
                    None => {}
                }
            }
            // stdout closed: if we never got a token, surface it as a failure
            // so waiters don't hang forever.
            reader_cache.set_failed_if_pending("auth helper exited without a token".into());
        });

        Ok(Self {
            _child: child,
            stdin: AsyncMutex::new(stdin),
            cache,
            _extracted: extracted,
        })
    }

    /// Ask the helper to mint a fresh token (used before a reconnect once the
    /// cached token is near expiry). Best-effort; the new token arrives on the
    /// stdout stream and updates the cache.
    pub async fn request_refresh(&self) -> Result<(), ProviderError> {
        // Async mutex: the guard is safely held across the write/flush awaits.
        let mut stdin = self.stdin.lock().await;
        stdin
            .write_all(b"{\"type\":\"refresh\"}\n")
            .await
            .map_err(|e| ProviderError::Token(format!("write refresh: {e}")))?;
        stdin
            .flush()
            .await
            .map_err(|e| ProviderError::Token(format!("flush refresh: {e}")))?;
        Ok(())
    }
}

impl TokenSource for NodeBridge {
    fn fresh_bearer(&self) -> BearerFuture {
        let cache = self.cache.clone();
        Box::pin(async move { cache.get().await })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_token_frame() {
        let frame = parse_auth_frame(
            r#"{"type":"token","bearer":"abc.def.ghi","expiresAt":"2026-07-01T00:00:00Z"}"#,
        );
        assert_eq!(
            frame,
            Some(AuthFrame::Token {
                bearer: "abc.def.ghi".into(),
                expires_at: "2026-07-01T00:00:00Z".into(),
            })
        );
    }

    #[test]
    fn parses_a_token_frame_without_expiry() {
        let frame = parse_auth_frame(r#"{"type":"token","bearer":"t"}"#);
        assert_eq!(
            frame,
            Some(AuthFrame::Token {
                bearer: "t".into(),
                expires_at: String::new(),
            })
        );
    }

    #[test]
    fn parses_an_error_frame() {
        let frame = parse_auth_frame(r#"{"type":"error","message":"reauth required"}"#);
        assert_eq!(
            frame,
            Some(AuthFrame::Error {
                message: "reauth required".into(),
            })
        );
    }

    #[test]
    fn ignores_blank_and_non_frame_lines() {
        assert_eq!(parse_auth_frame(""), None);
        assert_eq!(parse_auth_frame("   "), None);
        assert_eq!(parse_auth_frame("[hub-mcp] some log line"), None);
        assert_eq!(parse_auth_frame(r#"{"type":"surprise"}"#), None);
    }
}
