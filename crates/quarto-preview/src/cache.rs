//! Per-doc filesystem cache for `EngineCapture`s (Phase C.7, bd-kw93.7).
//!
//! Cache layout: `<cache_dir>/<sha256(input_qmd)>.bin`, where the file
//! contents are a gzip stream of the JSON-serialized [`EngineCapture`]
//! — identical wire format to the samod binary doc Phase C.1 writes,
//! and to the gzipped capture the WASM side ungzips in Phase C.4. The
//! shared format means the same readers/writers work for both the
//! authoritative automerge store and the local fast-path cache.
//!
//! Cache key: hex SHA-256 of the canonical `input_qmd` bytes — the
//! same QMD shape `EngineExecutionStage` hands to the engine, and the
//! same shape Phase C.2's staleness check uses. Drift between the two
//! would mean a staleness flip without a cache miss (or vice versa),
//! so both call into [`crate::config`]-flavoured shared helpers and
//! their canonical-input computation lives in
//! [`quarto_core::engine::preview_record::compute_input_qmd`].
//!
//! Atomic write: serialize → gzip → write to a sibling `<key>.bin.tmp`
//! → fsync → rename. Concurrent writers race on the rename, which is
//! safe on POSIX + Windows (last-writer-wins; the cached payload is
//! the same regardless of which gzipped stream lands).
//!
//! Scope: per-session (the directory is rooted under `data_dir`, which
//! is the ephemeral hub tempdir for `q2 preview`). Per-project reuse
//! across sessions is a Phase D follow-up — see plan §C.7
//! "Open follow-up".

use std::fmt::Write as _;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use quarto_core::engine::EngineRegistry;
use quarto_core::engine::preview_record::{compute_input_qmd, record_capture};
use quarto_core::project::ProjectContext;
use quarto_core::stage::PipelineError;
use quarto_system_runtime::SystemRuntime;
use quarto_trace::EngineCapture;
use sha2::{Digest, Sha256};

/// File extension for cache entries. Stable so a future tooling
/// pass (`q2 preview --purge-cache`, etc.) can sweep by glob.
pub const CACHE_FILE_EXT: &str = "bin";

/// Compute the SHA-256 hex digest of `input_qmd`. Public so tests
/// (and a future debugging surface) can derive the same key the cache
/// uses.
pub fn compute_key(input_qmd: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input_qmd);
    hex_encode(&hasher.finalize())
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        // `write!` to String never fails — unwrap is acceptable here.
        write!(s, "{:02x}", b).expect("writing to String never fails");
    }
    s
}

/// The on-disk path for a given cache key.
pub fn cache_file_path(cache_dir: &Path, key: &str) -> PathBuf {
    cache_dir.join(format!("{key}.{CACHE_FILE_EXT}"))
}

/// Look up a cached capture sequence (bd-5yff4 — one per engine, in
/// order). Returns `Ok(None)` for a clean miss (no file at that path);
/// `Ok(Some(vec))` on hit; `Err` if the file is present but unreadable
/// or unparseable. Callers in the driver downgrade `Err` to a miss + a
/// `tracing::warn!` so a corrupt cache entry doesn't take the preview
/// server down.
///
/// For robustness against a stale pre-bd-5yff4 entry (a single capture
/// object), a failed array parse falls back to a single-object parse
/// wrapped in a vec.
pub fn read_cached(cache_dir: &Path, key: &str) -> std::io::Result<Option<Vec<EngineCapture>>> {
    let path = cache_file_path(cache_dir, key);
    let file = match std::fs::File::open(&path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    let mut decoder = flate2::read::GzDecoder::new(file);
    let mut json_bytes = Vec::new();
    decoder.read_to_end(&mut json_bytes)?;
    match serde_json::from_slice::<Vec<EngineCapture>>(&json_bytes) {
        Ok(captures) => Ok(Some(captures)),
        Err(array_err) => match serde_json::from_slice::<EngineCapture>(&json_bytes) {
            Ok(single) => Ok(Some(vec![single])),
            Err(_) => Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                array_err,
            )),
        },
    }
}

/// Persist a capture sequence to the cache. Creates `cache_dir` (and
/// parents) if necessary. The write is atomic: temp file → fsync →
/// rename. The payload is a gzipped JSON **array** of captures.
pub fn write_cached(
    cache_dir: &Path,
    key: &str,
    captures: &[EngineCapture],
) -> std::io::Result<()> {
    std::fs::create_dir_all(cache_dir)?;

    let tmp_path = cache_dir.join(format!("{key}.{CACHE_FILE_EXT}.tmp"));
    let final_path = cache_file_path(cache_dir, key);

    let json = serde_json::to_vec(captures)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

    // Wrap the file in a scope so the encoder + file close before
    // rename — Windows requires the source file to be closed.
    {
        let file = std::fs::File::create(&tmp_path)?;
        let mut encoder = flate2::write::GzEncoder::new(file, flate2::Compression::default());
        encoder.write_all(&json)?;
        let file = encoder.finish()?;
        file.sync_data()?;
    }

    std::fs::rename(&tmp_path, &final_path)?;
    Ok(())
}

/// Cached wrapper around
/// [`quarto_core::engine::preview_record::record_capture`].
///
/// 1. Compute the canonical `input_qmd` via the existing
///    [`compute_input_qmd`] helper. (Same canonicalization Phase C.2
///    uses for staleness — see plan §Q-C6.)
/// 2. Hash → cache key.
/// 3. If the cache has a hit, return the cached capture directly. **No
///    engine invocation happens** — that's the whole point of the
///    cache, and it's what makes revert-edit cycles cheap.
/// 4. On miss, fall through to the real `record_capture`, then write
///    the resulting capture (if any) to the cache for next time.
///
/// A cache-write failure is logged but does not propagate — the
/// authoritative capture is still emitted to the caller, and a future
/// invocation simply re-runs the engine.
///
/// Returns an empty vec exactly when `record_capture` would (no engine
/// in play for this doc); the recorded sequence on either path; `Err` on
/// pipeline failures (cache read failures are downgraded to a miss).
pub async fn record_capture_cached(
    cache_dir: &Path,
    path: &Path,
    project: &ProjectContext,
    runtime: Arc<dyn SystemRuntime>,
    engine_registry: Option<Arc<EngineRegistry>>,
) -> Result<Vec<EngineCapture>, PipelineError> {
    // Compute the same input_qmd the engine would receive — this
    // doubles as the cache key derivation and as the source of truth
    // for what we hash. compute_input_qmd already runs the pre-engine
    // pipeline; on a cache hit, we save the much-more-expensive
    // engine invocation (Jupyter/knitr boot + cell execution).
    let input_qmd = compute_input_qmd(path, project, runtime.clone()).await?;
    let key = compute_key(&input_qmd);

    match read_cached(cache_dir, &key) {
        Ok(Some(captures)) => {
            tracing::debug!(path = %path.display(), key = %key, "engine capture cache hit");
            return Ok(captures);
        }
        Ok(None) => {}
        Err(e) => {
            tracing::warn!(
                path = %path.display(),
                key = %key,
                error = %e,
                "engine capture cache read failed; falling through to engine",
            );
        }
    }

    let captures = record_capture(path, project, runtime, engine_registry).await?;
    if !captures.is_empty()
        && let Err(e) = write_cached(cache_dir, &key, &captures)
    {
        tracing::warn!(
            path = %path.display(),
            key = %key,
            error = %e,
            "engine capture cache write failed; capture still emitted",
        );
    }
    Ok(captures)
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::atomic::{AtomicUsize, Ordering};

    use quarto_core::engine::{ExecuteResult, ExecutionContext, ExecutionEngine, ExecutionError};
    use quarto_system_runtime::NativeRuntime;
    use serde_json::json;
    use tempfile::TempDir;

    fn sample_capture() -> EngineCapture {
        EngineCapture {
            engine_name: "test-passthrough".to_string(),
            input_qmd: "---\nengine: test-passthrough\n---\n\n```{test-passthrough}\n1+1\n```\n"
                .to_string(),
            result: json!({
                "markdown": "1+1\n<!-- test -->\n",
                "supporting_files": [],
                "filters": [],
                "includes": [],
                "needs_postprocess": false,
            }),
            files: Vec::new(),
        }
    }

    /// Engine that counts how many times `execute` was called.
    /// Used to prove a cache hit truly avoids the engine invocation
    /// — not just that the returned capture matches.
    struct CountingPassthroughEngine {
        calls: Arc<AtomicUsize>,
    }

    impl ExecutionEngine for CountingPassthroughEngine {
        fn name(&self) -> &str {
            "test-passthrough"
        }
        fn execute(
            &self,
            input: &str,
            _ctx: &ExecutionContext,
        ) -> Result<ExecuteResult, ExecutionError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let mut out = String::from(input);
            out.push_str("\n<!-- counted-pass -->\n");
            Ok(ExecuteResult::passthrough(&out))
        }
        fn claims_language(
            &self,
            language: &str,
            _first_class: Option<&str>,
        ) -> quarto_core::engine::LanguageClaim {
            if language == "test-passthrough" {
                quarto_core::engine::LanguageClaim::Primary(1)
            } else {
                quarto_core::engine::LanguageClaim::None
            }
        }
    }

    fn counting_registry() -> (Arc<EngineRegistry>, Arc<AtomicUsize>) {
        let calls = Arc::new(AtomicUsize::new(0));
        let mut reg = EngineRegistry::new();
        reg.register(Arc::new(CountingPassthroughEngine {
            calls: calls.clone(),
        }));
        (Arc::new(reg), calls)
    }

    fn write_fixture(
        temp: &TempDir,
        content: &str,
    ) -> (std::path::PathBuf, ProjectContext, Arc<dyn SystemRuntime>) {
        let path = temp.path().join("doc.qmd");
        std::fs::write(&path, content).unwrap();
        let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
        let project = ProjectContext::discover(&path, runtime.as_ref()).unwrap();
        (path, project, runtime)
    }

    #[test]
    fn compute_key_is_stable_for_same_input() {
        let bytes = b"---\nengine: x\n---\nhello";
        let k1 = compute_key(bytes);
        let k2 = compute_key(bytes);
        assert_eq!(k1, k2);
        assert_eq!(k1.len(), 64, "SHA-256 hex should be 64 chars");
        assert!(
            k1.chars().all(|c| c.is_ascii_hexdigit()),
            "key should be ASCII hex"
        );
    }

    #[test]
    fn compute_key_differs_for_different_inputs() {
        assert_ne!(compute_key(b"foo"), compute_key(b"bar"));
        assert_ne!(compute_key(b"a"), compute_key(b"a\n"));
    }

    #[test]
    fn write_then_read_round_trips_capture() {
        // Plan §C.7 acceptance #1: "round-trip a cached capture through
        // <tempdir>/captures/."
        let tmp = TempDir::with_prefix("c7-cache-roundtrip-").unwrap();
        let cache_dir = tmp.path().join("captures");
        let capture = sample_capture();
        let key = compute_key(capture.input_qmd.as_bytes());

        write_cached(&cache_dir, &key, std::slice::from_ref(&capture)).expect("write succeeds");

        let read = read_cached(&cache_dir, &key)
            .expect("read succeeds")
            .expect("entry exists");

        assert_eq!(read.len(), 1);
        assert_eq!(read[0].engine_name, capture.engine_name);
        assert_eq!(read[0].input_qmd, capture.input_qmd);
        assert_eq!(read[0].result, capture.result);
    }

    #[test]
    fn read_returns_none_for_missing_key() {
        let tmp = TempDir::with_prefix("c7-cache-miss-").unwrap();
        let cache_dir = tmp.path().join("captures");
        // Cache dir doesn't even exist yet; that's still a clean miss.
        let result = read_cached(&cache_dir, "deadbeef").expect("not an error");
        assert!(result.is_none());
    }

    #[test]
    fn read_returns_none_when_dir_exists_but_key_missing() {
        let tmp = TempDir::with_prefix("c7-cache-empty-").unwrap();
        let cache_dir = tmp.path().join("captures");
        std::fs::create_dir_all(&cache_dir).unwrap();
        let result = read_cached(&cache_dir, "nokey").expect("not an error");
        assert!(result.is_none());
    }

    #[test]
    fn write_creates_parent_directory() {
        // The driver hands us a path that may not exist yet (the
        // first-write-during-this-session case). The writer must
        // create it.
        let tmp = TempDir::with_prefix("c7-cache-mkdir-").unwrap();
        let cache_dir = tmp.path().join("nested").join("captures");
        let capture = sample_capture();
        let key = compute_key(capture.input_qmd.as_bytes());
        write_cached(&cache_dir, &key, std::slice::from_ref(&capture))
            .expect("write creates parent");
        assert!(cache_dir.exists());
        assert!(cache_file_path(&cache_dir, &key).exists());
    }

    #[test]
    fn write_overwrites_existing_entry() {
        // Two write_cached calls for the same key — second one wins,
        // no error. Models the cross-session-with-same-input scenario.
        let tmp = TempDir::with_prefix("c7-cache-overwrite-").unwrap();
        let cache_dir = tmp.path().join("captures");
        let key = "samekey";

        let mut first = sample_capture();
        first.result = json!({"markdown": "first\n"});
        write_cached(&cache_dir, key, std::slice::from_ref(&first)).unwrap();

        let mut second = sample_capture();
        second.result = json!({"markdown": "second\n"});
        write_cached(&cache_dir, key, std::slice::from_ref(&second)).unwrap();

        let read = read_cached(&cache_dir, key).unwrap().unwrap();
        assert_eq!(read.len(), 1);
        assert_eq!(
            read[0].result.get("markdown").unwrap().as_str(),
            Some("second\n"),
            "second write must win"
        );
    }

    #[test]
    fn corrupt_cache_entry_surfaces_as_error() {
        // A truncated/garbled cache file must produce an Err — the
        // driver downgrades to a miss + log so a corrupt entry
        // can't take the server down, but the cache module itself
        // is honest about the corruption.
        let tmp = TempDir::with_prefix("c7-cache-corrupt-").unwrap();
        let cache_dir = tmp.path().join("captures");
        std::fs::create_dir_all(&cache_dir).unwrap();
        std::fs::write(
            cache_file_path(&cache_dir, "broken"),
            b"this is not a gzip stream",
        )
        .unwrap();
        assert!(read_cached(&cache_dir, "broken").is_err());
    }

    // ──────────────────────────────────────────────────────────────
    // record_capture_cached — the cache wrapper around record_capture
    // ──────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn cached_wrapper_invokes_engine_once_for_identical_input() {
        // Plan §C.7 acceptance #2: "edit a code cell back to its
        // original content; assert the second run uses the cache (no
        // engine invocation — verified by registry inspection)."
        //
        // We model this directly: two record_capture_cached calls
        // against the same file content. The counting engine asserts
        // exactly one execute() call across both.
        let tmp = TempDir::with_prefix("c7-cached-hit-").unwrap();
        let cache_dir = tmp.path().join("captures");
        let (path, project, runtime) = write_fixture(
            &tmp,
            "---\nengine: test-passthrough\n---\n\n```{test-passthrough}\nSENTINEL\n```\n",
        );

        let (registry, calls) = counting_registry();
        let first =
            record_capture_cached(&cache_dir, &path, &project, runtime.clone(), Some(registry))
                .await
                .expect("first call records a capture");
        assert_eq!(first.len(), 1, "passthrough engine → one capture");
        assert_eq!(calls.load(Ordering::SeqCst), 1, "engine should run once");

        // Second call against unchanged content — cache hit, no engine
        // invocation.
        let (registry2, calls2) = counting_registry();
        let second = record_capture_cached(&cache_dir, &path, &project, runtime, Some(registry2))
            .await
            .expect("second call succeeds");
        assert_eq!(second.len(), 1, "cache hit → one capture");
        assert_eq!(
            calls2.load(Ordering::SeqCst),
            0,
            "engine must NOT run on cache hit"
        );

        // Sanity: both captures carry the same payload.
        assert_eq!(first[0].engine_name, second[0].engine_name);
        assert_eq!(first[0].input_qmd, second[0].input_qmd);
        assert_eq!(first[0].result, second[0].result);
    }

    #[tokio::test]
    async fn cached_wrapper_runs_engine_again_when_content_changes() {
        // Cache key is the input_qmd hash, so a content edit must
        // produce a cache miss. Counter-test for the above.
        let tmp = TempDir::with_prefix("c7-cached-miss-").unwrap();
        let cache_dir = tmp.path().join("captures");
        let path = tmp.path().join("doc.qmd");
        let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());

        // First content.
        std::fs::write(
            &path,
            "---\nengine: test-passthrough\n---\n\n```{test-passthrough}\nFIRST\n```\n",
        )
        .unwrap();
        let project = ProjectContext::discover(&path, runtime.as_ref()).unwrap();
        let (registry, calls) = counting_registry();
        let _ = record_capture_cached(&cache_dir, &path, &project, runtime.clone(), Some(registry))
            .await
            .expect("first run");
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        // Edit on disk.
        std::fs::write(
            &path,
            "---\nengine: test-passthrough\n---\n\n```{test-passthrough}\nSECOND\n```\n",
        )
        .unwrap();
        let project2 = ProjectContext::discover(&path, runtime.as_ref()).unwrap();
        let (registry2, calls2) = counting_registry();
        let capture2 =
            record_capture_cached(&cache_dir, &path, &project2, runtime, Some(registry2))
                .await
                .expect("second run");
        assert_eq!(
            calls2.load(Ordering::SeqCst),
            1,
            "different content must miss the cache and invoke the engine"
        );
        assert!(
            capture2[0].input_qmd.contains("SECOND"),
            "second capture should reflect the edit"
        );
    }

    #[tokio::test]
    async fn cached_wrapper_recovers_to_engine_on_corrupt_cache() {
        // Pre-populate the cache with a corrupt file at the expected
        // key. The wrapper should log + fall through to the engine,
        // not propagate the error.
        let tmp = TempDir::with_prefix("c7-cached-corrupt-").unwrap();
        let cache_dir = tmp.path().join("captures");
        std::fs::create_dir_all(&cache_dir).unwrap();
        let (path, project, runtime) = write_fixture(
            &tmp,
            "---\nengine: test-passthrough\n---\n\n```{test-passthrough}\nFOO\n```\n",
        );

        // Pre-compute the cache key the wrapper will look up, then
        // poison the cache entry at that key.
        let input_qmd = compute_input_qmd(&path, &project, runtime.clone())
            .await
            .unwrap();
        let key = compute_key(&input_qmd);
        std::fs::write(cache_file_path(&cache_dir, &key), b"\x00\x01\x02 not gzip").unwrap();

        let (registry, calls) = counting_registry();
        let capture = record_capture_cached(&cache_dir, &path, &project, runtime, Some(registry))
            .await
            .expect("wrapper recovers");
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "corrupt cache should force a real engine run"
        );
        assert!(capture[0].input_qmd.contains("FOO"));

        // And the corrupt entry should now be replaced by a valid one.
        let re_read = read_cached(&cache_dir, &key)
            .expect("cache file is now valid")
            .expect("entry present");
        assert_eq!(re_read[0].engine_name, "test-passthrough");
    }

    #[tokio::test]
    async fn cached_wrapper_returns_empty_for_prose_only_doc() {
        // No engine in play → record_capture returns an empty vec →
        // record_capture_cached must do the same and NOT write a
        // bogus cache entry.
        let tmp = TempDir::with_prefix("c7-cached-prose-").unwrap();
        let cache_dir = tmp.path().join("captures");
        let (path, project, runtime) = write_fixture(&tmp, "---\ntitle: Prose\n---\n\nHello.\n");

        let result = record_capture_cached(&cache_dir, &path, &project, runtime, None)
            .await
            .expect("wrapper succeeds");
        assert!(result.is_empty(), "prose-only doc yields no captures");

        // The cache_dir may not even exist; if it does, nothing in it.
        if cache_dir.exists() {
            let entries: Vec<_> = std::fs::read_dir(&cache_dir).unwrap().collect();
            assert!(
                entries.is_empty(),
                "no cache entry should be written for empty captures; got {:?}",
                entries
                    .into_iter()
                    .map(|e| e.unwrap().file_name())
                    .collect::<Vec<_>>()
            );
        }
    }
}
