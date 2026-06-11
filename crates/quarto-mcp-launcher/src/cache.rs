//! Per-user bundle cache: extraction, lifetime locks, and GC.
//!
//! Layout: `<cache_root>/<content-hash>/` holds one extracted bundle,
//! plus two metadata files inside each bundle dir:
//!
//! - `.lock` — advisory-lock file. Every running `q2 mcp` instance
//!   holds a SHARED lock on it for its lifetime (the lock fd survives
//!   the exec into node on Unix; on Windows the launcher stays alive
//!   and holds it). GC may delete a dir only after winning an
//!   EXCLUSIVE try-lock, which proves no instance uses it. The kernel
//!   releases locks on process death, so crashes can never wedge GC
//!   (this is why locks, not refcount files).
//! - `.last-used` — rewritten on every launch; its mtime is the
//!   recency signal for GC. Age-gating (rather than keep-only-current)
//!   prevents a dev build and an installed release from evicting each
//!   other's bundles on every alternation.
//!
//! Extraction is crash-safe and race-safe: extract to a `.tmp-*`
//! sibling, atomically rename into place, losers of the rename race
//! discard their temp dir. Deletion is the mirror image: trash-rename
//! the dir first (`.trash-*`), then delete, so a concurrent launcher
//! never sees a half-deleted bundle at the canonical path — it either
//! wins the path race or re-extracts.
//!
//! Design discussion: claude-notes/plans/2026-06-11-q2-mcp-hub-auth.md
//! (risk 5). Quarto 1 users have complained about temp-dir pollution;
//! GC is proactive, bounded, and best-effort (it must never break a
//! launch).

use anyhow::{Context, Result, bail};
use fs2::FileExt;
use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use crate::bundle::BundleFile;

pub const LOCK_FILE: &str = ".lock";
pub const LAST_USED_FILE: &str = ".last-used";

/// Bundles unused for this long are GC candidates.
pub const DEFAULT_MAX_AGE: Duration = Duration::from_secs(14 * 24 * 60 * 60);

/// `.tmp-*` / `.trash-*` leftovers older than this are presumed
/// crashed mid-operation and get cleaned up.
const LEFTOVER_MAX_AGE: Duration = Duration::from_secs(60 * 60);

/// Uniquifier for `.tmp-*` extraction dirs within this process.
static TMP_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// An extracted bundle with its lifetime shared lock held.
pub struct ExtractedBundle {
    pub dir: PathBuf,
    /// Shared-locked for as long as this value lives. `delegate()`
    /// arranges for the lock to survive into the node process.
    pub lock: File,
}

/// The default per-user cache root (`<os cache dir>/quarto/hub-mcp`).
pub fn default_cache_root() -> Result<PathBuf> {
    let base = dirs::cache_dir().context(
        "could not determine the user cache directory (HOME unset?); \
         set QUARTO_MCP_CACHE_DIR to choose one explicitly",
    )?;
    Ok(base.join("quarto").join("hub-mcp"))
}

/// Extract `files` into `<cache_root>/<hash>/` (skipping extraction
/// when that dir already exists) and take the lifetime shared lock.
pub fn extract_and_lock(
    cache_root: &Path,
    files: &[BundleFile],
    hash: &str,
) -> Result<ExtractedBundle> {
    fs::create_dir_all(cache_root)
        .with_context(|| format!("creating cache dir {}", cache_root.display()))?;
    restrict_permissions(cache_root);

    let target = cache_root.join(hash);

    // Bounded retry: each iteration either succeeds or has observed a
    // transient race (GC trashed the dir between our existence check
    // and our lock) that a fresh extraction resolves.
    for _attempt in 0..4 {
        if !target.is_dir() {
            // The temp name must be unique per extraction attempt —
            // pid alone is not enough (concurrent threads in one
            // process share it; caught by the convergence test).
            let unique = TMP_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let tmp = cache_root.join(format!(".tmp-{}-{}", std::process::id(), unique));
            extract_files(files, &tmp)?;
            match fs::rename(&tmp, &target) {
                Ok(()) => {}
                Err(_) if target.is_dir() => {
                    // Lost the race to a concurrent launcher — its copy
                    // is identical (same content hash). Use it.
                    let _ = fs::remove_dir_all(&tmp);
                }
                Err(e) => {
                    let _ = fs::remove_dir_all(&tmp);
                    return Err(e).with_context(|| {
                        format!("moving extracted bundle into place at {}", target.display())
                    });
                }
            }
        }

        // Take the lifetime shared lock, then re-verify the payload:
        // GC may have trash-renamed the dir between our existence
        // check and the open.
        let lock_path = target.join(LOCK_FILE);
        let Ok(lock) = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)
        else {
            continue; // dir vanished — retry extraction
        };
        lock.lock_shared()
            .with_context(|| format!("locking {}", lock_path.display()))?;

        if target.join("index.mjs").is_file() {
            touch_last_used(&target);
            return Ok(ExtractedBundle { dir: target, lock });
        }

        // Payload missing under our shared lock: either GC raced us
        // (dir is gone) or the dir is corrupt (partial state at the
        // canonical path). Self-heal: if no other instance holds it,
        // remove and re-extract.
        drop(lock);
        if target.is_dir() {
            remove_bundle_dir(cache_root, &target);
        }
    }
    bail!(
        "could not extract the MCP bundle into {} after repeated attempts",
        cache_root.display()
    );
}

/// Best-effort GC: remove bundle dirs (other than `keep_hash`) whose
/// `.last-used` is older than `max_age` and on which an exclusive
/// try-lock succeeds, plus stale `.tmp-*`/`.trash-*` leftovers.
/// Failures skip the entry — GC must never break a launch.
pub fn gc(cache_root: &Path, keep_hash: &str, max_age: Duration) {
    let Ok(entries) = fs::read_dir(cache_root) else {
        return;
    };
    let now = SystemTime::now();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()).map(String::from) else {
            continue;
        };
        if name == keep_hash {
            continue;
        }

        if name.starts_with(".tmp-") || name.starts_with(".trash-") {
            // Crashed mid-extraction / mid-deletion. Age-gate so we
            // never yank a temp dir out from under a live extraction.
            if older_than(&dir_mtime(&path), now, LEFTOVER_MAX_AGE) {
                let _ = fs::remove_dir_all(&path);
            }
            continue;
        }

        // Candidate bundle dir: recency via .last-used (dir mtime as
        // fallback for dirs that predate the marker).
        let recency = file_mtime(&path.join(LAST_USED_FILE)).unwrap_or_else(|| dir_mtime(&path));
        if !older_than(&recency, now, max_age) {
            continue;
        }
        remove_bundle_dir(cache_root, &path);
    }
}

/// Delete a bundle dir if (and only if) an exclusive try-lock proves
/// it unused: trash-rename first so concurrent launchers fail cleanly,
/// then delete. Best-effort.
fn remove_bundle_dir(cache_root: &Path, path: &Path) {
    let Ok(lock) = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path.join(LOCK_FILE))
    else {
        return;
    };
    if lock.try_lock_exclusive().is_err() {
        return; // in use — skip
    }
    let nonce = format!(
        ".trash-{}-{}",
        std::process::id(),
        path.file_name().and_then(|n| n.to_str()).unwrap_or("x")
    );
    let trash = cache_root.join(nonce);
    if fs::rename(path, &trash).is_ok() {
        // The locked inode moved with the rename; safe to release and
        // delete — no launcher can reach the dir at its canonical path
        // anymore, and any holding the old path retries by design.
        drop(lock);
        let _ = fs::remove_dir_all(&trash);
    }
}

fn extract_files(files: &[BundleFile], into: &Path) -> Result<()> {
    for (rel, contents) in files {
        let dest = into.join(rel);
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
        }
        fs::write(&dest, contents).with_context(|| format!("writing {}", dest.display()))?;
    }
    Ok(())
}

/// Rewrite `.last-used`, refreshing its mtime (the GC recency signal).
fn touch_last_used(dir: &Path) {
    let _ = fs::write(dir.join(LAST_USED_FILE), b"");
}

fn file_mtime(path: &Path) -> Option<SystemTime> {
    fs::metadata(path).and_then(|m| m.modified()).ok()
}

fn dir_mtime(path: &Path) -> SystemTime {
    file_mtime(path).unwrap_or(SystemTime::UNIX_EPOCH)
}

fn older_than(t: &SystemTime, now: SystemTime, age: Duration) -> bool {
    now.duration_since(*t).map(|d| d > age).unwrap_or(false)
}

/// 0700 on the cache root: the extracted bundle is executed code; keep
/// other users out. No-op on Windows (per-user profile ACLs apply).
fn restrict_permissions(dir: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(dir, fs::Permissions::from_mode(0o700));
    }
    #[cfg(not(unix))]
    {
        let _ = dir;
    }
}
