//! Generic "wait for deployment to be live" polling.
//!
//! Used by every provider that supports a `verify` step. Keeping
//! the polling loop here means each provider only writes its
//! provider-specific check (e.g. for gh-pages, "fetch
//! `<site>/.nojekyll` and compare to the deploy id") instead of
//! re-implementing the timeout/sleep/cancellation machinery.

use std::time::{Duration, Instant};

use async_trait::async_trait;

use crate::types::PublishError;

/// One probe of "is the deploy live yet?".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeployCheck {
    /// Definitively live — `wait_for_deploy` returns `Ok(true)`.
    Ready,
    /// Definitively *not yet* live (404, etc.) — keep polling.
    NotYet,
    /// Definitively broken (5xx, DNS failure) — `wait_for_deploy`
    /// returns `Ok(false)` and lets the caller decide. Q1 also
    /// stops on definitively-broken to avoid masking real
    /// problems.
    Failed,
}

/// Closure-trait pair for the per-iteration probe.
///
/// We use a trait rather than a closure so the type can be passed
/// as a `&dyn` reference, keeping `wait_for_deploy` dyn-compatible.
#[async_trait]
pub trait DeployProbe: Send + Sync {
    async fn check(&self) -> Result<DeployCheck, PublishError>;
}

/// Result of a `wait_for_deploy` invocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaitOutcome {
    /// The probe returned `Ready`.
    Verified,
    /// The probe returned `Failed` (deploy is broken or
    /// unreachable in a way that won't resolve by waiting).
    Broken,
    /// We hit `timeout` without ever seeing `Ready` or `Failed`.
    TimedOut,
}

/// Configuration for `wait_for_deploy`.
#[derive(Debug, Clone, Copy)]
pub struct WaitConfig {
    pub interval: Duration,
    pub timeout: Duration,
}

impl Default for WaitConfig {
    fn default() -> Self {
        // Q1 polls every 2s with a 5-minute timeout for gh-pages.
        // Same defaults here.
        Self {
            interval: Duration::from_secs(2),
            timeout: Duration::from_secs(300),
        }
    }
}

/// Poll `probe.check()` until it returns `Ready`/`Failed` or the
/// timeout elapses.
///
/// Sleeping uses `std::thread::sleep` because (a) we're inside an
/// `async fn` driven by `pollster` (single-threaded), and (b) the
/// gh-pages verify step has no concurrent work to await on while
/// we wait. A future provider that wants concurrent verify work
/// can use `tokio::time::sleep`; we'll switch then.
pub async fn wait_for_deploy(
    probe: &dyn DeployProbe,
    cfg: WaitConfig,
) -> Result<WaitOutcome, PublishError> {
    let start = Instant::now();
    loop {
        match probe.check().await? {
            DeployCheck::Ready => return Ok(WaitOutcome::Verified),
            DeployCheck::Failed => return Ok(WaitOutcome::Broken),
            DeployCheck::NotYet => {}
        }
        if start.elapsed() >= cfg.timeout {
            return Ok(WaitOutcome::TimedOut);
        }
        std::thread::sleep(cfg.interval);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Probe that returns `Ready` after `n` calls, `NotYet` before.
    struct ReadyAfter {
        n: usize,
        calls: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl DeployProbe for ReadyAfter {
        async fn check(&self) -> Result<DeployCheck, PublishError> {
            let n = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
            if n >= self.n {
                Ok(DeployCheck::Ready)
            } else {
                Ok(DeployCheck::NotYet)
            }
        }
    }

    /// Probe that always returns `NotYet`.
    struct AlwaysNotYet {
        calls: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl DeployProbe for AlwaysNotYet {
        async fn check(&self) -> Result<DeployCheck, PublishError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(DeployCheck::NotYet)
        }
    }

    /// Probe that returns `Failed` immediately.
    struct AlwaysBroken;

    #[async_trait]
    impl DeployProbe for AlwaysBroken {
        async fn check(&self) -> Result<DeployCheck, PublishError> {
            Ok(DeployCheck::Failed)
        }
    }

    /// Probe that returns an error.
    struct AlwaysError;

    #[async_trait]
    impl DeployProbe for AlwaysError {
        async fn check(&self) -> Result<DeployCheck, PublishError> {
            Err(PublishError::Other(anyhow::anyhow!("network down")))
        }
    }

    fn fast_cfg() -> WaitConfig {
        WaitConfig {
            interval: Duration::from_millis(5),
            timeout: Duration::from_millis(200),
        }
    }

    #[test]
    fn wait_returns_verified_when_probe_ready() {
        let calls = Arc::new(AtomicUsize::new(0));
        let probe = ReadyAfter {
            n: 1,
            calls: calls.clone(),
        };
        let outcome = pollster::block_on(wait_for_deploy(&probe, fast_cfg())).unwrap();
        assert_eq!(outcome, WaitOutcome::Verified);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn wait_polls_until_ready() {
        let calls = Arc::new(AtomicUsize::new(0));
        let probe = ReadyAfter {
            n: 3,
            calls: calls.clone(),
        };
        let outcome = pollster::block_on(wait_for_deploy(&probe, fast_cfg())).unwrap();
        assert_eq!(outcome, WaitOutcome::Verified);
        assert_eq!(calls.load(Ordering::SeqCst), 3);
    }

    #[test]
    fn wait_returns_broken_on_failed() {
        let outcome = pollster::block_on(wait_for_deploy(&AlwaysBroken, fast_cfg())).unwrap();
        assert_eq!(outcome, WaitOutcome::Broken);
    }

    #[test]
    fn wait_times_out_when_probe_never_ready() {
        let calls = Arc::new(AtomicUsize::new(0));
        let probe = AlwaysNotYet {
            calls: calls.clone(),
        };
        let outcome = pollster::block_on(wait_for_deploy(&probe, fast_cfg())).unwrap();
        assert_eq!(outcome, WaitOutcome::TimedOut);
        // Should have polled at least a couple of times.
        assert!(
            calls.load(Ordering::SeqCst) >= 2,
            "expected multiple polls, got {}",
            calls.load(Ordering::SeqCst)
        );
    }

    #[test]
    fn wait_propagates_probe_errors() {
        let err = pollster::block_on(wait_for_deploy(&AlwaysError, fast_cfg())).unwrap_err();
        assert!(err.to_string().contains("network down"));
    }
}
