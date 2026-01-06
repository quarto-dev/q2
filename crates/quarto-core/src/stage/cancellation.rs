/*
 * stage/cancellation.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Platform-agnostic cancellation abstraction.
 *
 * On native targets, wraps tokio_util::sync::CancellationToken.
 * On WASM, provides a simple AtomicBool-based implementation that
 * avoids the std::time::Instant dependency that tokio_util uses.
 */

//! Platform-agnostic cancellation for pipeline execution.
//!
//! This module provides [`Cancellation`], a cancellation token that works
//! in both native and WASM environments. The tokio_util CancellationToken
//! uses `std::time::Instant` internally, which panics on WASM.

#[cfg(target_arch = "wasm32")]
use std::sync::Arc;
#[cfg(target_arch = "wasm32")]
use std::sync::atomic::{AtomicBool, Ordering};

/// A cancellation token that works in both native and WASM environments.
///
/// On native targets, this wraps `tokio_util::sync::CancellationToken`.
/// On WASM, this uses a simple `AtomicBool` since WASM is single-threaded
/// and doesn't have signal handlers for Ctrl+C.
#[derive(Clone)]
pub struct Cancellation {
    #[cfg(not(target_arch = "wasm32"))]
    inner: tokio_util::sync::CancellationToken,

    #[cfg(target_arch = "wasm32")]
    cancelled: Arc<AtomicBool>,
}

impl Cancellation {
    /// Create a new cancellation token.
    pub fn new() -> Self {
        #[cfg(not(target_arch = "wasm32"))]
        {
            Self {
                inner: tokio_util::sync::CancellationToken::new(),
            }
        }

        #[cfg(target_arch = "wasm32")]
        {
            Self {
                cancelled: Arc::new(AtomicBool::new(false)),
            }
        }
    }

    /// Check if cancellation has been requested.
    pub fn is_cancelled(&self) -> bool {
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.inner.is_cancelled()
        }

        #[cfg(target_arch = "wasm32")]
        {
            self.cancelled.load(Ordering::Relaxed)
        }
    }

    /// Request cancellation.
    ///
    /// After this is called, `is_cancelled()` will return `true`.
    pub fn cancel(&self) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.inner.cancel()
        }

        #[cfg(target_arch = "wasm32")]
        {
            self.cancelled.store(true, Ordering::Relaxed)
        }
    }
}

impl Default for Cancellation {
    fn default() -> Self {
        Self::new()
    }
}

// Conversion from tokio_util CancellationToken (native only)
#[cfg(not(target_arch = "wasm32"))]
impl From<tokio_util::sync::CancellationToken> for Cancellation {
    fn from(token: tokio_util::sync::CancellationToken) -> Self {
        Self { inner: token }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_token_not_cancelled() {
        let token = Cancellation::new();
        assert!(!token.is_cancelled());
    }

    #[test]
    fn test_cancel_sets_flag() {
        let token = Cancellation::new();
        assert!(!token.is_cancelled());
        token.cancel();
        assert!(token.is_cancelled());
    }

    #[test]
    fn test_clone_shares_state() {
        let token1 = Cancellation::new();
        let token2 = token1.clone();

        assert!(!token1.is_cancelled());
        assert!(!token2.is_cancelled());

        token1.cancel();

        assert!(token1.is_cancelled());
        assert!(token2.is_cancelled());
    }
}
