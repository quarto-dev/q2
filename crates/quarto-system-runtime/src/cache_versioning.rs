//! Generational cache-namespace purge.
//!
//! Copyright (c) 2026 Posit, PBC
//!
//! This module provides [`ensure_namespace_version`], a helper that stamps a
//! version sentinel at a reserved key inside a cache namespace and purges the
//! whole namespace when the caller-supplied version disagrees with the stored
//! one. It's intended for cache layers backed by `SystemRuntime::cache_*`
//! methods — native filesystem cache, WASM IndexedDB — where entries survive
//! across sessions and can accumulate indefinitely as the set of valid keys
//! evolves (e.g. when the bundled SCSS resources change).
//!
//! The helper is purely a layer on top of [`SystemRuntime`] methods and
//! doesn't need its own storage; it writes the version under a well-known
//! reserved key (`_version`). Callers that use this pattern must therefore
//! treat `_version` as reserved inside their namespace.

use crate::traits::{RuntimeResult, SystemRuntime};

/// Reserved key inside a cache namespace used to stamp the generational
/// version sentinel.
pub const CACHE_VERSION_KEY: &str = "_version";

/// Check the stored version sentinel in `namespace`; purge the namespace and
/// re-stamp when the stored version disagrees with `version`.
///
/// ## Behavior
///
/// - Stored version matches `version` → no-op, returns `Ok(())`.
/// - Stored version absent or mismatched → `cache_clear_namespace(namespace)`
///   followed by `cache_set(namespace, "_version", version)`. Returns
///   `Ok(())`.
///
/// ## Call frequency
///
/// This helper performs at least one cache read per invocation, so callers
/// should memoize the check per process/session (e.g. with a `OnceLock`)
/// rather than calling it on every cache access.
///
/// ## Concurrency
///
/// Idempotent under racing callers: two concurrent calls against the same
/// underlying storage converge on the same final state (the target version
/// stored, no other keys).
pub async fn ensure_namespace_version(
    runtime: &dyn SystemRuntime,
    namespace: &str,
    version: &[u8],
) -> RuntimeResult<()> {
    let stored = runtime.cache_get(namespace, CACHE_VERSION_KEY).await?;
    match stored {
        Some(bytes) if bytes == version => Ok(()),
        _ => {
            runtime.cache_clear_namespace(namespace).await?;
            runtime
                .cache_set(namespace, CACHE_VERSION_KEY, version)
                .await?;
            Ok(())
        }
    }
}

#[cfg(test)]
#[cfg(not(target_arch = "wasm32"))]
mod tests {
    use super::*;
    use crate::native::NativeRuntime;
    use tempfile::TempDir;

    fn setup() -> (NativeRuntime, TempDir) {
        let temp = TempDir::new().unwrap();
        let rt = NativeRuntime::with_cache_dir(temp.path().to_path_buf());
        (rt, temp)
    }

    #[test]
    fn fresh_namespace_sets_version_and_preserves_nothing() {
        let (rt, _tmp) = setup();
        pollster::block_on(ensure_namespace_version(&rt, "sass", b"v1")).unwrap();
        let stored = pollster::block_on(rt.cache_get("sass", CACHE_VERSION_KEY)).unwrap();
        assert_eq!(stored.as_deref(), Some(b"v1".as_ref()));
    }

    #[test]
    fn matching_version_is_noop() {
        let (rt, _tmp) = setup();
        pollster::block_on(rt.cache_set("sass", CACHE_VERSION_KEY, b"v1")).unwrap();
        pollster::block_on(rt.cache_set("sass", "entry1", b"hello")).unwrap();

        pollster::block_on(ensure_namespace_version(&rt, "sass", b"v1")).unwrap();

        // entry1 still present — no purge occurred.
        let entry = pollster::block_on(rt.cache_get("sass", "entry1")).unwrap();
        assert_eq!(entry.as_deref(), Some(b"hello".as_ref()));
    }

    #[test]
    fn mismatched_version_purges_everything_and_restamps() {
        let (rt, _tmp) = setup();
        pollster::block_on(rt.cache_set("sass", CACHE_VERSION_KEY, b"old")).unwrap();
        pollster::block_on(rt.cache_set("sass", "theme_a", b"AAA")).unwrap();
        pollster::block_on(rt.cache_set("sass", "theme_b", b"BBB")).unwrap();

        pollster::block_on(ensure_namespace_version(&rt, "sass", b"new")).unwrap();

        // Old entries gone.
        let a = pollster::block_on(rt.cache_get("sass", "theme_a")).unwrap();
        let b = pollster::block_on(rt.cache_get("sass", "theme_b")).unwrap();
        assert!(a.is_none(), "theme_a should be purged");
        assert!(b.is_none(), "theme_b should be purged");

        // New version sentinel in place.
        let v = pollster::block_on(rt.cache_get("sass", CACHE_VERSION_KEY)).unwrap();
        assert_eq!(v.as_deref(), Some(b"new".as_ref()));
    }

    #[test]
    fn affects_only_the_requested_namespace() {
        let (rt, _tmp) = setup();
        pollster::block_on(rt.cache_set("other_ns", CACHE_VERSION_KEY, b"x")).unwrap();
        pollster::block_on(rt.cache_set("other_ns", "key1", b"survives")).unwrap();
        pollster::block_on(rt.cache_set("sass", "old_entry", b"doomed")).unwrap();

        pollster::block_on(ensure_namespace_version(&rt, "sass", b"v1")).unwrap();

        // `other_ns` untouched.
        let survivor = pollster::block_on(rt.cache_get("other_ns", "key1")).unwrap();
        assert_eq!(survivor.as_deref(), Some(b"survives".as_ref()));
        // `sass` purged.
        let gone = pollster::block_on(rt.cache_get("sass", "old_entry")).unwrap();
        assert!(gone.is_none());
    }

    #[test]
    fn repeated_calls_with_same_version_stay_noop() {
        let (rt, _tmp) = setup();
        // First call: stamps version (no prior entries to purge).
        pollster::block_on(ensure_namespace_version(&rt, "sass", b"v1")).unwrap();
        pollster::block_on(rt.cache_set("sass", "entry", b"data")).unwrap();

        // Second call: version matches, must not purge.
        pollster::block_on(ensure_namespace_version(&rt, "sass", b"v1")).unwrap();
        let entry = pollster::block_on(rt.cache_get("sass", "entry")).unwrap();
        assert_eq!(entry.as_deref(), Some(b"data".as_ref()));
    }
}
