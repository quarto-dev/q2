//! The numbers of the contract ("Numbers"), in one place so the hub, the status route, and the tests read
//! the same values. Times are milliseconds.

/// Preview periodic sync.
pub const PREVIEW_PERIODIC_SYNC_MS: u64 = 5_000;
/// Quiet window before a decision.
pub const QUIET_WINDOW_MS: u64 = 1_000;
/// Decision cap.
pub const DECISION_CAP_MS: u64 = 30_000;
/// A lock file older than this is stale; state markers pause indefinitely.
pub const STALE_LOCK_MS: u64 = 60_000;
/// Client freeze and unavailable timeout.
pub const CLIENT_UNAVAILABLE_TIMEOUT_MS: u64 = 5_000;
/// Wait for an unavailable canonical doc.
pub const UNAVAILABLE_CANONICAL_MS: u64 = 5_000;
/// Editor-only wait for a flip.
pub const EDITOR_ONLY_FLIP_WAIT_MS: u64 = 10_000;
/// Proxy readiness.
pub const PROXY_READINESS_MS: u64 = 15_000;
/// Publish cap per file.
pub const PUBLISH_CAP_BYTES_PER_FILE: u64 = 10 * 1024 * 1024;
/// Retired handles kept per path.
pub const RETIRED_HANDLES_KEPT_PER_PATH: usize = 8;
/// Controller tick while a decision is pending.
pub const CONTROLLER_TICK_PENDING_MS: u64 = 250;
/// A file modified within this window of a lock release is skipped in the periodic loop.
pub const MTIME_SKIP_MS: u64 = 500;
/// Registry soft limit, in tracked paths.
pub const REGISTRY_SOFT_LIMIT_PATHS: usize = 15_000;
/// Controller tick while nothing is pending (the controller option default).
pub const IDLE_TICK_MS: u64 = 5_000;
