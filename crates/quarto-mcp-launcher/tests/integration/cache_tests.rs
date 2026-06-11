//! Extraction, lifetime-lock, and GC semantics (plan Phase 2,
//! bd-81cfshmw). These encode the safety properties that justify the
//! lock-based design: GC can never delete a bundle in use, crashes
//! release locks automatically, and corrupted/raced cache states heal.

use fs2::FileExt;
use quarto_mcp_launcher::{DEFAULT_MAX_AGE, LAST_USED_FILE, LOCK_FILE, extract_and_lock, gc};
use std::borrow::Cow;
use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

type BundleFile = (PathBuf, Cow<'static, [u8]>);

fn test_bundle() -> Vec<BundleFile> {
    vec![
        (
            PathBuf::from("index.mjs"),
            Cow::Borrowed(b"// fake bundle entry\n".as_slice()),
        ),
        (
            PathBuf::from("build-info.json"),
            Cow::Borrowed(b"{}\n".as_slice()),
        ),
        (
            PathBuf::from("node_modules/@napi-rs/keyring/index.js"),
            Cow::Borrowed(b"// fake addon loader\n".as_slice()),
        ),
    ]
}

const HASH: &str = "cafebabe01234567";

/// Set a file's mtime into the past (the GC recency signal).
fn age_file(path: &Path, age: Duration) {
    let f = OpenOptions::new().write(true).open(path).unwrap();
    f.set_modified(SystemTime::now() - age).unwrap();
}

fn lock_handle(dir: &Path) -> File {
    OpenOptions::new()
        .read(true)
        .write(true)
        .open(dir.join(LOCK_FILE))
        .unwrap()
}

#[test]
fn extraction_creates_payload_and_metadata() {
    let cache = tempfile::tempdir().unwrap();
    let extracted = extract_and_lock(cache.path(), &test_bundle(), HASH).unwrap();

    assert_eq!(extracted.dir, cache.path().join(HASH));
    assert_eq!(
        fs::read(extracted.dir.join("index.mjs")).unwrap(),
        b"// fake bundle entry\n"
    );
    assert!(
        extracted
            .dir
            .join("node_modules/@napi-rs/keyring/index.js")
            .is_file()
    );
    assert!(extracted.dir.join(LAST_USED_FILE).is_file());
    // no extraction temp dirs left behind
    let leftovers: Vec<_> = fs::read_dir(cache.path())
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with(".tmp-"))
        .collect();
    assert!(leftovers.is_empty());
}

#[test]
fn extraction_holds_shared_lock_for_lifetime() {
    let cache = tempfile::tempdir().unwrap();
    let extracted = extract_and_lock(cache.path(), &test_bundle(), HASH).unwrap();

    // While the ExtractedBundle lives, an exclusive try-lock must fail
    // — this is exactly the probe GC uses before deleting.
    let probe = lock_handle(&extracted.dir);
    assert!(probe.try_lock_exclusive().is_err());

    // Dropping the bundle (process exit, in real life) releases it.
    let dir = extracted.dir.clone();
    drop(extracted);
    let probe2 = lock_handle(&dir);
    assert!(probe2.try_lock_exclusive().is_ok());
}

#[test]
fn second_launch_reuses_existing_dir() {
    let cache = tempfile::tempdir().unwrap();
    let first = extract_and_lock(cache.path(), &test_bundle(), HASH).unwrap();
    let marker = first.dir.join("user-marker");
    fs::write(&marker, b"still here").unwrap();
    drop(first);

    let second = extract_and_lock(cache.path(), &test_bundle(), HASH).unwrap();
    // Same dir, not re-extracted from scratch.
    assert!(marker.is_file(), "re-extraction clobbered the dir");
    drop(second);
}

#[test]
fn concurrent_extractions_converge() {
    let cache = tempfile::tempdir().unwrap();
    let root = cache.path().to_path_buf();
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let root = root.clone();
            std::thread::spawn(move || extract_and_lock(&root, &test_bundle(), HASH).unwrap())
        })
        .collect();
    for h in handles {
        let extracted = h.join().unwrap();
        assert!(extracted.dir.join("index.mjs").is_file());
    }
    // Exactly the one hash dir; every .tmp-* cleaned up.
    let names: Vec<String> = fs::read_dir(&root)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, vec![HASH.to_string()], "cache dirs: {names:?}");
}

#[test]
fn corrupt_cache_dir_self_heals() {
    let cache = tempfile::tempdir().unwrap();
    // A canonical-path dir with no payload (e.g. interrupted manual
    // tampering): extraction must replace it rather than fail forever.
    fs::create_dir_all(cache.path().join(HASH)).unwrap();
    let extracted = extract_and_lock(cache.path(), &test_bundle(), HASH).unwrap();
    assert!(extracted.dir.join("index.mjs").is_file());
}

#[test]
fn gc_removes_old_unlocked_bundles() {
    let cache = tempfile::tempdir().unwrap();
    let old = extract_and_lock(cache.path(), &test_bundle(), "0ldhash000000000").unwrap();
    let old_dir = old.dir.clone();
    drop(old); // lock released
    age_file(&old_dir.join(LAST_USED_FILE), DEFAULT_MAX_AGE * 2);

    gc(cache.path(), "current-hash", DEFAULT_MAX_AGE);
    assert!(!old_dir.exists(), "old unlocked bundle should be GC'd");
    // no trash leftovers
    let trash: Vec<_> = fs::read_dir(cache.path())
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with(".trash-"))
        .collect();
    assert!(trash.is_empty());
}

#[test]
fn gc_spares_recently_used_bundles() {
    let cache = tempfile::tempdir().unwrap();
    let recent = extract_and_lock(cache.path(), &test_bundle(), "recenthash000000").unwrap();
    let dir = recent.dir.clone();
    drop(recent); // unlocked, but young
    gc(cache.path(), "current-hash", DEFAULT_MAX_AGE);
    assert!(dir.exists(), "recently used bundle must survive GC");
}

#[test]
fn gc_spares_locked_bundles_even_when_old() {
    let cache = tempfile::tempdir().unwrap();
    let held = extract_and_lock(cache.path(), &test_bundle(), "l0ckedhash000000").unwrap();
    age_file(&held.dir.join(LAST_USED_FILE), DEFAULT_MAX_AGE * 2);

    gc(cache.path(), "current-hash", DEFAULT_MAX_AGE);
    assert!(
        held.dir.join("index.mjs").is_file(),
        "a bundle with a live shared lock must never be GC'd"
    );
}

#[test]
fn gc_never_touches_the_current_hash() {
    let cache = tempfile::tempdir().unwrap();
    let current = extract_and_lock(cache.path(), &test_bundle(), HASH).unwrap();
    let dir = current.dir.clone();
    drop(current);
    age_file(&dir.join(LAST_USED_FILE), DEFAULT_MAX_AGE * 2);

    gc(cache.path(), HASH, DEFAULT_MAX_AGE);
    assert!(
        dir.exists(),
        "keep_hash dir must survive GC regardless of age"
    );
}

#[test]
fn gc_cleans_stale_tmp_and_trash_leftovers() {
    let cache = tempfile::tempdir().unwrap();
    let stale_tmp = cache.path().join(".tmp-99999-0");
    let stale_trash = cache.path().join(".trash-99999-x");
    fs::create_dir_all(&stale_tmp).unwrap();
    fs::create_dir_all(&stale_trash).unwrap();
    fs::write(stale_tmp.join("partial"), b"x").unwrap();
    // Make them look hours old (dir mtime).
    let two_hours = Duration::from_secs(2 * 60 * 60);
    File::open(&stale_tmp)
        .and_then(|f| f.set_modified(SystemTime::now() - two_hours))
        .ok();
    File::open(&stale_trash)
        .and_then(|f| f.set_modified(SystemTime::now() - two_hours))
        .ok();

    gc(cache.path(), HASH, DEFAULT_MAX_AGE);

    // On platforms where dir-handle mtime setting works (unix), the
    // leftovers must be gone; elsewhere they must at least not break GC.
    #[cfg(unix)]
    {
        assert!(!stale_tmp.exists());
        assert!(!stale_trash.exists());
    }
}

#[test]
fn fresh_tmp_dirs_survive_gc() {
    let cache = tempfile::tempdir().unwrap();
    // A just-created .tmp dir simulates an extraction in flight.
    let live_tmp = cache.path().join(".tmp-12345-0");
    fs::create_dir_all(&live_tmp).unwrap();
    gc(cache.path(), HASH, DEFAULT_MAX_AGE);
    assert!(
        live_tmp.exists(),
        "in-flight extraction dir must not be GC'd"
    );
}

#[test]
fn crashed_instance_releases_lock_making_bundle_collectable() {
    // Simulate "crash" by dropping the lock without any cleanup, then
    // aging the dir: GC must collect it. (The kernel releases flocks on
    // process death; in-process drop models that without forking.)
    let cache = tempfile::tempdir().unwrap();
    let crashed = extract_and_lock(cache.path(), &test_bundle(), "crashedhash00000").unwrap();
    let dir = crashed.dir.clone();
    drop(crashed.lock); // "process died"
    age_file(&dir.join(LAST_USED_FILE), DEFAULT_MAX_AGE * 2);

    gc(cache.path(), "current-hash", DEFAULT_MAX_AGE);
    assert!(!dir.exists(), "crash-released bundle must be collectable");
}

#[test]
fn launch_after_gc_re_extracts() {
    let cache = tempfile::tempdir().unwrap();
    let first = extract_and_lock(cache.path(), &test_bundle(), HASH).unwrap();
    let dir = first.dir.clone();
    drop(first);
    age_file(&dir.join(LAST_USED_FILE), DEFAULT_MAX_AGE * 2);
    gc(cache.path(), "other-hash", DEFAULT_MAX_AGE);
    assert!(!dir.exists());

    // Next launch simply re-extracts.
    let again = extract_and_lock(cache.path(), &test_bundle(), HASH).unwrap();
    assert!(again.dir.join("index.mjs").is_file());
}
