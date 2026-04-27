/*
 * project/profile_cache.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! Phase 8 on-disk profile cache.
//!
//! Persists serialized [`DocumentProfile`]s under the `profiles`
//! namespace of `SystemRuntime::cache_get/set`, keyed by the
//! [`pass1_key`](crate::project::cache_key::pass1_key) hash of every
//! input that produced the profile.
//!
//! ## Layout
//!
//! On native systems with a cache directory wired in (i.e. the CLI's
//! `<project>/.quarto/cache/` via `NativeRuntime::with_cache_dir`),
//! profiles land at `cache/profiles/<64-char-hex-key>` with the
//! `DocumentProfile`'s JSON encoding inside.
//!
//! On WASM (default `cache_get` returns `Ok(None)`, default
//! `cache_set` is a no-op), the cache is a transparent miss — every
//! Pass-1 runs live, every save is silent. Phase 9 will wire a
//! WASM-side store (likely IndexedDB) behind the same interface.
//!
//! ## Error handling
//!
//! Cache errors are *never* fatal. The CLI's contract is:
//!
//! - `load`: any failure (missing entry, JSON parse error, version
//!   mismatch) → `Ok(None)`. The orchestrator falls through to a
//!   live Pass-1 and writes the fresh profile back.
//! - `save`: write errors are returned to the caller, who decides
//!   whether to surface them. Phase 8.2's orchestrator wraps these
//!   in a non-fatal warning so a transient I/O hiccup doesn't
//!   abort an otherwise-successful render.
//!
//! See `claude-notes/plans/2026-04-27-websites-phase-8.md`
//! Decision 14 for the policy.

use quarto_system_runtime::{RuntimeError, RuntimeResult, SystemRuntime};

use crate::document_profile::DocumentProfile;

/// Cache namespace for serialized profiles.
///
/// Lives alongside `sass` (the existing SCSS cache) under
/// `<project>/.quarto/cache/`. The namespace name appears as the
/// directory name on the native backend; ASCII-alphanumeric and the
/// short length keep us comfortably within the 128-char limit
/// `validate_cache_namespace` enforces.
pub const PROFILE_CACHE_NAMESPACE: &str = "profiles";

/// Try to load a `DocumentProfile` from the cache.
///
/// Returns `Ok(None)` for cache misses, version mismatches,
/// corrupted JSON, or runtimes without cache support. Returns
/// `Err(_)` only on lower-level runtime failures (e.g. cache I/O
/// permission errors that aren't a normal miss). Callers can treat
/// `Err` as "best to abort the cache lookup" — the orchestrator
/// degrades gracefully to a live Pass-1 either way.
///
/// `key` is the 64-char hex string from
/// [`hex_encode`](crate::project::cache_key::hex_encode); it must
/// satisfy `validate_cache_key` (ASCII-alphanumeric + `-` + `_`,
/// non-empty, ≤128 chars), which `hex_encode` guarantees.
pub async fn load(
    runtime: &dyn SystemRuntime,
    key: &str,
) -> RuntimeResult<Option<DocumentProfile>> {
    let raw = match runtime.cache_get(PROFILE_CACHE_NAMESPACE, key).await? {
        Some(bytes) => bytes,
        None => return Ok(None),
    };

    let json = match std::str::from_utf8(&raw) {
        Ok(s) => s,
        Err(_) => {
            // Corrupted entry — treat as a miss. The orchestrator
            // overwrites with a fresh profile on the next save.
            return Ok(None);
        }
    };

    match DocumentProfile::from_json(json) {
        Ok(profile) => Ok(Some(profile)),
        Err(_) => Ok(None),
    }
}

/// Persist a `DocumentProfile` to the cache.
///
/// Serialization errors map to [`RuntimeError::CacheError`] so the
/// caller sees a consistent error type. JSON serialization for
/// `DocumentProfile` cannot fail under normal use (every field is
/// straightforwardly serializable), but the API surface stays
/// fallible to keep callers honest if a future field changes that.
pub async fn save(
    runtime: &dyn SystemRuntime,
    key: &str,
    profile: &DocumentProfile,
) -> RuntimeResult<()> {
    let json = profile
        .to_json()
        .map_err(|e| RuntimeError::CacheError(format!("profile serialize: {e}")))?;
    runtime
        .cache_set(PROFILE_CACHE_NAMESPACE, key, json.as_bytes())
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document_profile::IncludeEntry;
    use std::path::PathBuf;
    use std::sync::Arc;

    /// Build a profile with non-default values across every field
    /// the orchestrator might cache, so round-trip tests exercise
    /// serialization comprehensively.
    fn rich_profile() -> DocumentProfile {
        DocumentProfile {
            source_path: PathBuf::from("docs/api.qmd"),
            output_href: "docs/api.html".to_string(),
            format_id: "html".to_string(),
            title: Some("API".to_string()),
            description: Some("Reference docs".to_string()),
            authors: vec!["Alice".to_string()],
            categories: vec!["docs".to_string()],
            includes: vec![IncludeEntry::new(
                PathBuf::from("snippets/header.qmd"),
                b"shared header",
            )],
            nav_dependencies: vec![PathBuf::from("../tutorial.qmd")],
            always_render: true,
            body_link_targets: vec![PathBuf::from("about.qmd")],
            ..DocumentProfile::default()
        }
    }

    /// `tempfile`-backed `NativeRuntime` so the test exercises the
    /// real on-disk cache layout without polluting the project
    /// directory.
    fn native_runtime_with_temp_cache() -> (Arc<dyn SystemRuntime>, quarto_system_runtime::TempDir)
    {
        let temp = quarto_system_runtime::NativeRuntime::new()
            .temp_dir("phase8-cache")
            .expect("temp_dir");
        let runtime: Arc<dyn SystemRuntime> = Arc::new(
            quarto_system_runtime::NativeRuntime::with_cache_dir(temp.path().to_path_buf()),
        );
        (runtime, temp)
    }

    #[tokio::test]
    async fn load_returns_none_for_missing_key() {
        let (runtime, _temp) = native_runtime_with_temp_cache();
        let result = load(runtime.as_ref(), "0000000000000000")
            .await
            .expect("load should not error on miss");
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn round_trip_preserves_profile() {
        let (runtime, _temp) = native_runtime_with_temp_cache();
        let profile = rich_profile();
        let key = "abc123def456";

        save(runtime.as_ref(), key, &profile).await.expect("save");
        let loaded = load(runtime.as_ref(), key)
            .await
            .expect("load")
            .expect("hit");

        assert_eq!(loaded, profile);
    }

    #[tokio::test]
    async fn load_rejects_corrupt_json_as_miss() {
        let (runtime, _temp) = native_runtime_with_temp_cache();
        let key = "corrupt_key";

        runtime
            .cache_set(PROFILE_CACHE_NAMESPACE, key, b"{ not valid json")
            .await
            .expect("set");

        let result = load(runtime.as_ref(), key)
            .await
            .expect("load should swallow the parse error");
        assert!(result.is_none(), "corrupt JSON should be a cache miss");
    }

    #[tokio::test]
    async fn load_rejects_version_mismatch_as_miss() {
        let (runtime, _temp) = native_runtime_with_temp_cache();
        let key = "wrong_version";

        // A v1 profile JSON (the pre-Phase-8 shape). After v2
        // bump, from_json rejects this and load() returns None.
        let v1_json = br#"{
            "profile_version": 1,
            "source_path": "x.qmd",
            "output_href": "x.html",
            "format_id": "html",
            "title": null,
            "subtitle": null,
            "description": null,
            "authors": [],
            "date": null,
            "categories": [],
            "keywords": [],
            "image": null,
            "draft": false,
            "outline": []
        }"#;

        runtime
            .cache_set(PROFILE_CACHE_NAMESPACE, key, v1_json)
            .await
            .expect("set");

        let result = load(runtime.as_ref(), key)
            .await
            .expect("load should swallow the version-mismatch error");
        assert!(result.is_none(), "v1 entry should be a cache miss");
    }

    #[tokio::test]
    async fn load_returns_none_when_runtime_has_no_cache() {
        // NativeRuntime::new() (no cache_dir) → all cache_get calls
        // return Ok(None) by trait default. profile_cache::load
        // honors that — Phase 8 caching is a no-op in that
        // configuration.
        let runtime: Arc<dyn SystemRuntime> = Arc::new(quarto_system_runtime::NativeRuntime::new());
        let result = load(runtime.as_ref(), "any_key")
            .await
            .expect("load should not error");
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn save_is_a_no_op_when_runtime_has_no_cache() {
        // NativeRuntime::new() (no cache_dir) → cache_set is a no-op.
        // No I/O occurs; the call returns Ok(()).
        let runtime: Arc<dyn SystemRuntime> = Arc::new(quarto_system_runtime::NativeRuntime::new());
        let profile = rich_profile();
        save(runtime.as_ref(), "any_key", &profile)
            .await
            .expect("save should be a silent no-op");
    }

    #[tokio::test]
    async fn save_then_overwrite_then_load_returns_latest() {
        let (runtime, _temp) = native_runtime_with_temp_cache();
        let key = "overwrite_me";

        let mut p1 = rich_profile();
        p1.title = Some("First".to_string());
        save(runtime.as_ref(), key, &p1).await.unwrap();

        let mut p2 = rich_profile();
        p2.title = Some("Second".to_string());
        save(runtime.as_ref(), key, &p2).await.unwrap();

        let loaded = load(runtime.as_ref(), key).await.unwrap().expect("hit");
        assert_eq!(loaded.title.as_deref(), Some("Second"));
    }

    #[tokio::test]
    async fn invalid_cache_key_propagates_error_from_runtime() {
        // The runtime validates keys; ours doesn't, so we don't
        // catch the error ourselves. This test just asserts that
        // a key violation surfaces (rather than being silently
        // swallowed by load/save).
        let (runtime, _temp) = native_runtime_with_temp_cache();
        let bogus = ""; // empty key fails validate_cache_key
        let err = save(runtime.as_ref(), bogus, &rich_profile()).await;
        assert!(err.is_err(), "empty key should be rejected by runtime");
    }
}
