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

use std::path::Path;

use quarto_system_runtime::{RuntimeError, RuntimeResult, SystemRuntime};

use crate::document_profile::{DocumentProfile, IncludeEntry};

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
/// **Include verification**. The cache key (per
/// [`cache_key::pass1_key`](super::cache_key::pass1_key)) does
/// not include the transitive include set — at lookup time we
/// don't yet know which child files the document includes. After
/// loading, this function verifies each [`IncludeEntry`] in the
/// cached profile against the include's current bytes via
/// `include_resolver`. If `include_resolver` returns a different
/// `content_hash` than the one stored on the entry, the load
/// degrades to a miss.
///
/// `include_resolver` is given each [`IncludeEntry::path`] and
/// returns the file's current SHA-256 (matching
/// [`IncludeEntry::hash_bytes`]). It returns `Err` if the file
/// can't be read (deleted child file, permission error). Phase 8
/// treats unreadable includes as a miss too — the file may be
/// gone and a fresh Pass-1 will surface that as its own
/// diagnostic.
///
/// `key` is the 64-char hex string from
/// [`hex_encode`](crate::project::cache_key::hex_encode); it must
/// satisfy `validate_cache_key` (ASCII-alphanumeric + `-` + `_`,
/// non-empty, ≤128 chars), which `hex_encode` guarantees.
pub async fn load<F>(
    runtime: &dyn SystemRuntime,
    key: &str,
    include_resolver: F,
) -> RuntimeResult<Option<DocumentProfile>>
where
    F: Fn(&Path) -> RuntimeResult<[u8; 32]>,
{
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

    let profile = match DocumentProfile::from_json(json) {
        Ok(p) => p,
        Err(_) => return Ok(None),
    };

    // Verify includes. Any unreadable / changed file ⇒ miss.
    if !verify_includes(&profile.includes, &include_resolver) {
        return Ok(None);
    }

    Ok(Some(profile))
}

/// Compare each cached `IncludeEntry`'s recorded `content_hash`
/// against the include's current bytes (via `resolver`). Returns
/// `true` only when every include matches. Any read error or
/// hash mismatch is reported as `false` (a miss).
fn verify_includes<F>(includes: &[IncludeEntry], resolver: F) -> bool
where
    F: Fn(&Path) -> RuntimeResult<[u8; 32]>,
{
    for entry in includes {
        let current_hash = match resolver(&entry.path) {
            Ok(h) => h,
            Err(_) => return false,
        };
        if current_hash != entry.content_hash {
            return false;
        }
    }
    true
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

    /// Resolver that always returns the entry's own recorded
    /// content_hash, so verification trivially succeeds. Use in
    /// tests that don't care about include verification.
    fn passthrough_resolver(_p: &Path) -> RuntimeResult<[u8; 32]> {
        // We can't read entry.content_hash from inside the
        // resolver — the resolver only knows the path. The
        // simplest workable approach in tests is to compute the
        // hash from the same bytes the entry was constructed
        // with; tests below build entries with `IncludeEntry::new`
        // and then hand a resolver keyed on path → bytes.
        // This passthrough is only safe for profiles whose
        // includes list is empty (nothing to verify).
        Ok([0u8; 32])
    }

    /// Bytes for the "snippets/header.qmd" include used by
    /// `rich_profile`. Tests that verify include behavior key
    /// resolvers off this constant.
    const HEADER_BYTES: &[u8] = b"shared header";

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
                HEADER_BYTES,
            )],
            nav_dependencies: vec![PathBuf::from("../tutorial.qmd")],
            always_render: true,
            body_link_targets: vec![PathBuf::from("about.qmd")],
            ..DocumentProfile::default()
        }
    }

    /// Resolver matching `rich_profile`'s only include — returns
    /// the same hash the entry was built with, so verification
    /// passes.
    fn matching_resolver(p: &Path) -> RuntimeResult<[u8; 32]> {
        if p == Path::new("snippets/header.qmd") {
            Ok(IncludeEntry::hash_bytes(HEADER_BYTES))
        } else {
            Err(RuntimeError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("test resolver: unknown include {}", p.display()),
            )))
        }
    }

    /// Resolver that pretends the included file changed: returns
    /// a different hash for the header file.
    fn mismatched_resolver(p: &Path) -> RuntimeResult<[u8; 32]> {
        if p == Path::new("snippets/header.qmd") {
            Ok(IncludeEntry::hash_bytes(b"DIFFERENT"))
        } else {
            Err(RuntimeError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("test resolver: unknown include {}", p.display()),
            )))
        }
    }

    /// Resolver that fails to read the include (e.g. file deleted).
    fn unreadable_resolver(_p: &Path) -> RuntimeResult<[u8; 32]> {
        Err(RuntimeError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "test resolver: include disappeared",
        )))
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
        let result = load(runtime.as_ref(), "0000000000000000", passthrough_resolver)
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
        let loaded = load(runtime.as_ref(), key, matching_resolver)
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

        let result = load(runtime.as_ref(), key, passthrough_resolver)
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

        let result = load(runtime.as_ref(), key, passthrough_resolver)
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
        let result = load(runtime.as_ref(), "any_key", passthrough_resolver)
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

        let loaded = load(runtime.as_ref(), key, matching_resolver)
            .await
            .unwrap()
            .expect("hit");
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

    // === Phase 8.2 step 2: include verification ============================

    #[tokio::test]
    async fn load_misses_when_include_content_hash_changed() {
        // The cached profile records snippets/header.qmd with
        // hash(HEADER_BYTES). The mismatched_resolver returns a
        // different hash → verification fails → cache miss.
        let (runtime, _temp) = native_runtime_with_temp_cache();
        let profile = rich_profile();
        let key = "include_changed";

        save(runtime.as_ref(), key, &profile).await.unwrap();
        let result = load(runtime.as_ref(), key, mismatched_resolver)
            .await
            .expect("load should not error on verification mismatch");
        assert!(result.is_none(), "include hash mismatch should be a miss");
    }

    #[tokio::test]
    async fn load_misses_when_include_unreadable() {
        // The included file disappeared between runs.
        let (runtime, _temp) = native_runtime_with_temp_cache();
        let profile = rich_profile();
        let key = "include_gone";

        save(runtime.as_ref(), key, &profile).await.unwrap();
        let result = load(runtime.as_ref(), key, unreadable_resolver)
            .await
            .expect("load should swallow resolver errors");
        assert!(result.is_none(), "unreadable include should be a miss");
    }

    #[tokio::test]
    async fn load_hits_when_includes_unchanged() {
        let (runtime, _temp) = native_runtime_with_temp_cache();
        let profile = rich_profile();
        let key = "includes_match";

        save(runtime.as_ref(), key, &profile).await.unwrap();
        let result = load(runtime.as_ref(), key, matching_resolver)
            .await
            .expect("load should not error")
            .expect("verification should succeed");
        assert_eq!(result.includes.len(), 1);
    }

    #[tokio::test]
    async fn load_hits_for_profile_with_no_includes() {
        // A profile with empty `includes` skips verification
        // entirely — any resolver behavior is irrelevant.
        let (runtime, _temp) = native_runtime_with_temp_cache();
        let mut profile = rich_profile();
        profile.includes.clear();
        let key = "no_includes";

        save(runtime.as_ref(), key, &profile).await.unwrap();
        let result = load(runtime.as_ref(), key, unreadable_resolver)
            .await
            .expect("load")
            .expect("hit");
        assert_eq!(result.includes.len(), 0);
    }
}
