//! Size-capped LRU cache wrapper.
//!
//! Copyright (c) 2026 Posit, PBC
//!
//! This module layers least-recently-used eviction on top of the plain
//! `SystemRuntime::cache_*` key-value API. Instead of reaching into each
//! backend (native filesystem cache directory, WASM IndexedDB) to add
//! `last_accessed` metadata natively, it stores a small JSON index under a
//! reserved key inside each namespace that uses the wrapper. The result is
//! portable: any runtime that implements the existing trait methods gets
//! LRU for free.
//!
//! ## Reserved keys
//!
//! Callers that use [`cache_get_lru`] / [`cache_set_lru`] reserve the
//! following keys inside the managed namespace:
//!
//! - [`CACHE_LRU_INDEX_KEY`] — the LRU index itself (JSON blob).
//! - [`crate::cache_versioning::CACHE_VERSION_KEY`] — generational version
//!   sentinel written by [`crate::ensure_namespace_version`].
//!
//! Both reserved keys are skipped by eviction so the namespace's generational
//! purge remains the only authority that resets them.
//!
//! ## Index format
//!
//! The index is a JSON object of the form:
//!
//! ```json
//! { "entries": [ { "key": "…", "size": 12345, "accessed_ms": 1700000000000 }, … ] }
//! ```
//!
//! Simple enough to be hand-editable for debugging; compact enough that a
//! full ~33-entry sass cache index fits in well under 10 KB.
//!
//! ## Concurrency
//!
//! The index is read, mutated, and re-written as a single logical operation
//! but the underlying trait methods don't offer transactions. Under
//! contention — two tabs writing simultaneously — one write can lose. LRU
//! is only a hint, and eviction is self-healing (a wrongly-evicted hot
//! entry is recompiled on next access), so lost index updates are benign.
//! The target cache entry and its existence are tracked by the backend,
//! not by the index.

use crate::cache_versioning::CACHE_VERSION_KEY;
use crate::traits::{RuntimeError, RuntimeResult, SystemRuntime};
use serde::{Deserialize, Serialize};

/// Reserved key under which the LRU index is stored inside a namespace.
pub const CACHE_LRU_INDEX_KEY: &str = "_lru_index";

/// Byte budget used by the `sass` namespace (see plan
/// `claude-notes/plans/2026-04-18-wasm-scss-cache-regression.md`).
///
/// At ~305 KB per compiled theme this buys ~33 entries — comfortably more
/// than all 25 Bootswatch themes plus the default Bootstrap entry, with
/// slack for custom-theme churn.
pub const SASS_CACHE_BUDGET_BYTES: u64 = 10 * 1024 * 1024;

#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct LruIndex {
    entries: Vec<LruEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct LruEntry {
    key: String,
    size: u64,
    accessed_ms: u64,
}

impl LruIndex {
    fn load_bytes(bytes: &[u8]) -> Self {
        serde_json::from_slice(bytes).unwrap_or_default()
    }

    fn to_bytes(&self) -> Vec<u8> {
        // Stable ordering for deterministic serialization in tests; serde_json
        // preserves Vec order.
        serde_json::to_vec(self).unwrap_or_default()
    }

    fn position(&self, key: &str) -> Option<usize> {
        self.entries.iter().position(|e| e.key == key)
    }

    fn touch(&mut self, key: &str) {
        if let Some(i) = self.position(key) {
            self.entries[i].accessed_ms = now_ms();
        }
    }

    fn upsert(&mut self, key: &str, size: u64) {
        let now = now_ms();
        match self.position(key) {
            Some(i) => {
                self.entries[i].size = size;
                self.entries[i].accessed_ms = now;
            }
            None => self.entries.push(LruEntry {
                key: key.to_string(),
                size,
                accessed_ms: now,
            }),
        }
    }

    fn total_size(&self) -> u64 {
        self.entries.iter().map(|e| e.size).sum()
    }

    /// Pick victims (oldest `accessed_ms` first) whose removal brings the
    /// total at or below `target_bytes`, excluding `keep_key`. Returns the
    /// keys to evict, in order.
    fn victims_to_fit(&self, target_bytes: u64, keep_key: &str) -> Vec<String> {
        let mut ordered: Vec<&LruEntry> =
            self.entries.iter().filter(|e| e.key != keep_key).collect();
        ordered.sort_by_key(|e| e.accessed_ms);

        let mut remaining = self.total_size();
        let mut evict = Vec::new();
        for entry in ordered {
            if remaining <= target_bytes {
                break;
            }
            remaining = remaining.saturating_sub(entry.size);
            evict.push(entry.key.clone());
        }
        evict
    }

    fn remove(&mut self, key: &str) {
        if let Some(i) = self.position(key) {
            self.entries.swap_remove(i);
        }
    }
}

/// True if `key` is a reserved metadata key (index or version sentinel).
/// Reserved keys are skipped by LRU bookkeeping and eviction.
fn is_reserved(key: &str) -> bool {
    key == CACHE_LRU_INDEX_KEY || key == CACHE_VERSION_KEY
}

fn reject_reserved(key: &str) -> RuntimeResult<()> {
    if is_reserved(key) {
        Err(RuntimeError::CacheError(format!(
            "cache key {key:?} is reserved and cannot be used with LRU wrapper"
        )))
    } else {
        Ok(())
    }
}

async fn load_index(runtime: &dyn SystemRuntime, namespace: &str) -> RuntimeResult<LruIndex> {
    Ok(runtime
        .cache_get(namespace, CACHE_LRU_INDEX_KEY)
        .await?
        .map(|bytes| LruIndex::load_bytes(&bytes))
        .unwrap_or_default())
}

async fn store_index(
    runtime: &dyn SystemRuntime,
    namespace: &str,
    index: &LruIndex,
) -> RuntimeResult<()> {
    runtime
        .cache_set(namespace, CACHE_LRU_INDEX_KEY, &index.to_bytes())
        .await
}

/// LRU-aware `cache_get`.
///
/// Returns the cached value if present, and refreshes `accessed_ms` in the
/// index so future evictions treat this key as recently used.
///
/// Reserved keys ([`CACHE_LRU_INDEX_KEY`], [`CACHE_VERSION_KEY`]) are
/// rejected with an error — the caller should use
/// [`SystemRuntime::cache_get`] directly for those.
pub async fn cache_get_lru(
    runtime: &dyn SystemRuntime,
    namespace: &str,
    key: &str,
) -> RuntimeResult<Option<Vec<u8>>> {
    reject_reserved(key)?;
    let value = runtime.cache_get(namespace, key).await?;
    if value.is_some() {
        let mut index = load_index(runtime, namespace).await?;
        if index.position(key).is_some() {
            index.touch(key);
            // Index write failure is non-fatal; the cached value is still valid.
            let _ = store_index(runtime, namespace, &index).await;
        }
    }
    Ok(value)
}

/// LRU-aware `cache_set` with a per-namespace byte budget.
///
/// Writes the value, refreshes its `accessed_ms` entry in the index, and
/// evicts least-recently-used entries until the total tracked size is at
/// or below `budget_bytes`. The just-written entry is never evicted in
/// the same call.
///
/// Reserved keys are rejected; use [`SystemRuntime::cache_set`] for those.
pub async fn cache_set_lru(
    runtime: &dyn SystemRuntime,
    namespace: &str,
    key: &str,
    value: &[u8],
    budget_bytes: u64,
) -> RuntimeResult<()> {
    reject_reserved(key)?;

    // Write the new value first so a later crash doesn't leave the index
    // pointing to something that isn't there.
    runtime.cache_set(namespace, key, value).await?;

    let mut index = load_index(runtime, namespace).await?;
    index.upsert(key, value.len() as u64);

    if index.total_size() > budget_bytes {
        let victims = index.victims_to_fit(budget_bytes, key);
        for victim in &victims {
            // Best-effort: keep going on individual delete failures.
            let _ = runtime.cache_delete(namespace, victim).await;
            index.remove(victim);
        }
    }

    store_index(runtime, namespace, &index).await?;
    Ok(())
}

// ── Clock abstraction ───────────────────────────────────────────────────────

#[cfg(not(target_arch = "wasm32"))]
fn now_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(target_arch = "wasm32")]
fn now_ms() -> u64 {
    // Safe: js_sys::Date::now() returns ms since epoch as f64. Clamped to
    // u64 because realistic timestamps don't overflow and future timestamps
    // are orderable.
    js_sys::Date::now() as u64
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

    fn get(rt: &NativeRuntime, ns: &str, key: &str) -> Option<Vec<u8>> {
        pollster::block_on(rt.cache_get(ns, key)).unwrap()
    }

    fn load(rt: &NativeRuntime, ns: &str) -> LruIndex {
        pollster::block_on(load_index(rt, ns)).unwrap()
    }

    #[test]
    fn set_lru_stores_target_and_updates_index() {
        let (rt, _tmp) = setup();
        pollster::block_on(cache_set_lru(&rt, "sass", "a", b"AAA", 1024)).unwrap();

        assert_eq!(get(&rt, "sass", "a").as_deref(), Some(b"AAA".as_ref()));
        let index = load(&rt, "sass");
        assert_eq!(index.entries.len(), 1);
        assert_eq!(index.entries[0].key, "a");
        assert_eq!(index.entries[0].size, 3);
    }

    #[test]
    fn get_lru_returns_value_and_touches_access_time() {
        let (rt, _tmp) = setup();
        pollster::block_on(cache_set_lru(&rt, "sass", "a", b"AAA", 1024)).unwrap();
        // Force the access timestamp to a small value so we can detect the
        // touch's update.
        let mut primed = load(&rt, "sass");
        primed.entries[0].accessed_ms = 0;
        pollster::block_on(store_index(&rt, "sass", &primed)).unwrap();

        let got = pollster::block_on(cache_get_lru(&rt, "sass", "a")).unwrap();
        assert_eq!(got.as_deref(), Some(b"AAA".as_ref()));

        let index = load(&rt, "sass");
        assert!(
            index.entries[0].accessed_ms > 0,
            "accessed_ms should be refreshed by a successful get"
        );
    }

    #[test]
    fn get_lru_missing_key_returns_none_and_does_not_touch_index() {
        let (rt, _tmp) = setup();
        pollster::block_on(cache_set_lru(&rt, "sass", "a", b"AAA", 1024)).unwrap();
        let before = load(&rt, "sass");

        let got = pollster::block_on(cache_get_lru(&rt, "sass", "missing")).unwrap();
        assert!(got.is_none());

        let after = load(&rt, "sass");
        assert_eq!(
            before.entries[0].accessed_ms, after.entries[0].accessed_ms,
            "accessed_ms of existing entries should be untouched on miss"
        );
    }

    #[test]
    fn over_budget_write_evicts_lru_entry() {
        let (rt, _tmp) = setup();
        // Budget of 10 bytes: three 4-byte entries won't fit.
        pollster::block_on(cache_set_lru(&rt, "sass", "a", b"aaaa", 10)).unwrap();
        pollster::block_on(cache_set_lru(&rt, "sass", "b", b"bbbb", 10)).unwrap();
        pollster::block_on(cache_set_lru(&rt, "sass", "c", b"cccc", 10)).unwrap();

        // Oldest-accessed `a` should have been evicted to get total to 8 bytes.
        assert!(get(&rt, "sass", "a").is_none(), "a should be evicted");
        assert!(get(&rt, "sass", "b").is_some(), "b should survive");
        assert!(get(&rt, "sass", "c").is_some(), "c should survive");

        let index = load(&rt, "sass");
        assert_eq!(index.entries.len(), 2);
    }

    #[test]
    fn getting_old_entry_rescues_it_from_next_eviction() {
        let (rt, _tmp) = setup();
        pollster::block_on(cache_set_lru(&rt, "sass", "a", b"aaaa", 10)).unwrap();
        pollster::block_on(cache_set_lru(&rt, "sass", "b", b"bbbb", 10)).unwrap();

        // Touch `a` so it becomes the more-recently-accessed of the two.
        std::thread::sleep(std::time::Duration::from_millis(2));
        pollster::block_on(cache_get_lru(&rt, "sass", "a")).unwrap();

        // New write that forces an eviction: `b` (oldest) should go, not `a`.
        std::thread::sleep(std::time::Duration::from_millis(2));
        pollster::block_on(cache_set_lru(&rt, "sass", "c", b"cccc", 10)).unwrap();

        assert!(get(&rt, "sass", "a").is_some(), "touched a should survive");
        assert!(get(&rt, "sass", "b").is_none(), "untouched b should evict");
        assert!(get(&rt, "sass", "c").is_some(), "new c should be present");
    }

    #[test]
    fn just_written_entry_is_never_evicted_in_its_own_call() {
        let (rt, _tmp) = setup();
        pollster::block_on(cache_set_lru(&rt, "sass", "a", b"aaaa", 10)).unwrap();
        // A single write that exceeds the whole budget: the entry itself
        // mustn't be chosen as a victim in the same call.
        pollster::block_on(cache_set_lru(&rt, "sass", "big", &vec![0u8; 20], 10)).unwrap();
        assert!(get(&rt, "sass", "big").is_some(), "fresh write preserved");
        assert!(get(&rt, "sass", "a").is_none(), "a evicted to make room");

        let index = load(&rt, "sass");
        assert_eq!(index.entries.len(), 1);
        assert_eq!(index.entries[0].key, "big");
    }

    #[test]
    fn reserved_keys_rejected() {
        let (rt, _tmp) = setup();
        let err = pollster::block_on(cache_set_lru(&rt, "sass", CACHE_LRU_INDEX_KEY, b"oops", 10));
        assert!(err.is_err());

        let err = pollster::block_on(cache_get_lru(&rt, "sass", CACHE_VERSION_KEY));
        assert!(err.is_err());
    }

    #[test]
    fn reserved_keys_not_tracked_when_set_directly() {
        // Using the raw API to write a reserved key must not pollute the
        // index; the LRU wrapper should behave as if they don't exist.
        let (rt, _tmp) = setup();
        pollster::block_on(rt.cache_set("sass", CACHE_VERSION_KEY, b"v1")).unwrap();
        pollster::block_on(cache_set_lru(&rt, "sass", "a", b"AAA", 1024)).unwrap();
        let index = load(&rt, "sass");
        assert_eq!(
            index.entries.iter().map(|e| &e.key).collect::<Vec<_>>(),
            vec![&"a".to_string()],
        );
    }
}
