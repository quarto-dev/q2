//! Revocation-event store for hub sessions (§3 of the sliding-sessions
//! plan, `bd-3dq0x6ut`).
//!
//! Sessions stay **stateless** (tokens are self-contained; there is no
//! session table). This store records only **revocation events**:
//!
//! * a per-`sub` `not_before` map — a session token is dead when its
//!   immutable `auth_time` predates the user's `not_before` entry
//!   (written by self-service `POST /auth/logout-everywhere`);
//! * operator **ban** entries — a ban is `not_before = ∞`: it rejects
//!   every session *and* gates minting, the only per-user deny that
//!   works on hubs running without an allowlist.
//!
//! Persistence is a dedicated `revocations.json` in the hub dir —
//! deliberately **not** `hub.json`, which holds the signing secrets and
//! must not gain a user-triggerable write path. Writes go through an
//! in-memory map behind a tokio mutex and an atomic temp-file + rename;
//! single-writer is guaranteed by construction (`hub.lock` holds an
//! exclusive lock for the process lifetime). Operators must only edit
//! the file while the hub is stopped — a live hand-edit can be
//! overwritten by the hub's own atomic persist.
//!
//! GC: logout entries self-expire once `not_before + absolute_max <
//! now` (every session they could kill has hit the absolute cap
//! anyway); **ban entries are never GC'd**.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use crate::error::{Error, Result};

/// On-disk filename, adjacent to `hub.json` in the hub dir.
const REVOCATIONS_FILE: &str = "revocations.json";

/// On-disk / in-memory revocation data.
///
/// `BTreeMap`/`BTreeSet` keep serialization deterministic (sorted keys)
/// and the file pleasant to hand-edit during the documented stopped-hub
/// ban procedure.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct RevocationData {
    /// Format version for future migrations.
    #[serde(default = "default_version")]
    version: u32,
    /// `sub` → epoch seconds: sessions with `auth_time < not_before`
    /// are rejected.
    #[serde(default)]
    not_before: BTreeMap<String, i64>,
    /// Banned `sub`s: every session rejected, minting refused.
    #[serde(default)]
    banned: BTreeSet<String>,
}

fn default_version() -> u32 {
    1
}

/// Outcome of a revocation check for one session token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RevocationStatus {
    /// No event affects this session.
    Ok,
    /// The user's whole token family was revoked after this token's
    /// `auth_time` (logout-everywhere).
    Revoked,
    /// The `sub` is banned.
    Banned,
}

/// The hub's revocation-event store. One instance per hub process,
/// owned by `HubContext`.
pub struct RevocationLedger {
    path: PathBuf,
    /// Absolute session lifetime — the GC horizon for logout entries.
    absolute_secs: i64,
    inner: Mutex<RevocationData>,
}

impl RevocationLedger {
    /// Load `revocations.json` from the hub dir (empty ledger when the
    /// file does not exist), GC'ing expired logout entries. Expired
    /// entries removed at load are only persisted back on the next
    /// write — a pure read never touches the file.
    pub fn load(hub_dir: &Path, absolute_secs: i64, now: i64) -> Result<Self> {
        let path = hub_dir.join(REVOCATIONS_FILE);
        let mut data = if path.exists() {
            let content = std::fs::read_to_string(&path)?;
            serde_json::from_str(&content)
                .map_err(|e| Error::ConfigParse(format!("{REVOCATIONS_FILE}: {e}")))?
        } else {
            RevocationData::default()
        };
        gc(&mut data, absolute_secs, now);
        Ok(Self {
            path,
            absolute_secs,
            inner: Mutex::new(data),
        })
    }

    /// Record a logout-everywhere event: every session of `sub` whose
    /// `auth_time` is at or before `now` is dead. `not_before` is
    /// stored as `now + 1` — clocks are second-granular, so a token
    /// minted in the same second as the revocation must die too (a
    /// strict `<` against bare `now` let same-second logins survive;
    /// caught by the C7 e2e run). Post-revocation mints stay possible
    /// because the mint path bumps `auth_time` to
    /// [`Self::min_auth_time`]. Persists atomically; the in-memory
    /// entry stays effective even if the persist fails (the error is
    /// returned so the caller can surface it).
    pub async fn revoke_all_for(&self, sub: &str, now: i64) -> Result<()> {
        let not_before = now + 1;
        let mut data = self.inner.lock().await;
        // Never move not_before backwards (an older concurrent caller
        // must not shorten an existing revocation).
        let entry = data.not_before.entry(sub.to_string()).or_insert(not_before);
        *entry = (*entry).max(not_before);
        gc(&mut data, self.absolute_secs, now);
        persist(&self.path, &data)
    }

    /// The earliest `auth_time` at which a *new* session for `sub` is
    /// valid, if a revocation event exists. The mint path stamps
    /// `auth_time = max(now, min_auth_time)` so an immediate
    /// (same-second) re-login is provably post-revocation.
    pub async fn min_auth_time(&self, sub: &str) -> Option<i64> {
        self.inner.lock().await.not_before.get(sub).copied()
    }

    /// Check one session token's (`sub`, `auth_time`) against the
    /// recorded events. `Banned` dominates `Revoked`.
    pub async fn check(&self, sub: &str, auth_time: i64) -> RevocationStatus {
        let data = self.inner.lock().await;
        if data.banned.contains(sub) {
            return RevocationStatus::Banned;
        }
        match data.not_before.get(sub) {
            Some(&not_before) if auth_time < not_before => RevocationStatus::Revoked,
            _ => RevocationStatus::Ok,
        }
    }

    /// Whether `sub` is banned — the mint gate (`auth_callback` /
    /// `auth_refresh` refuse to mint for a banned user).
    pub async fn is_banned(&self, sub: &str) -> bool {
        self.inner.lock().await.banned.contains(sub)
    }
}

/// Drop logout entries whose every killable session has already hit the
/// absolute cap. Bans are never dropped.
fn gc(data: &mut RevocationData, absolute_secs: i64, now: i64) {
    data.not_before
        .retain(|_, not_before| *not_before + absolute_secs >= now);
}

/// Atomic persist: write to `<file>.tmp` (0o600 on unix), then rename
/// over the live file.
fn persist(path: &Path, data: &RevocationData) -> Result<()> {
    let content =
        serde_json::to_string_pretty(data).map_err(|e| Error::ConfigParse(e.to_string()))?;
    let tmp = path.with_extension("json.tmp");
    {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut f = options.open(&tmp)?;
        f.write_all(content.as_bytes())?;
    }
    std::fs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    const ABSOLUTE: i64 = 30 * 24 * 3600;
    const NOW: i64 = 2_000_000_000;

    fn ledger(dir: &Path) -> RevocationLedger {
        RevocationLedger::load(dir, ABSOLUTE, NOW).unwrap()
    }

    #[tokio::test]
    async fn missing_file_loads_empty_and_creates_nothing() {
        let temp = TempDir::new().unwrap();
        let l = ledger(temp.path());
        assert_eq!(l.check("anyone", NOW - 1000).await, RevocationStatus::Ok);
        assert!(!l.is_banned("anyone").await);
        assert!(
            !temp.path().join(REVOCATIONS_FILE).exists(),
            "pure reads never create the file"
        );
    }

    #[tokio::test]
    async fn revoke_kills_same_second_and_older_auth_times() {
        let temp = TempDir::new().unwrap();
        let l = ledger(temp.path());
        l.revoke_all_for("alice", NOW).await.unwrap();

        // Any token minted before OR IN the revocation second is dead —
        // second-granularity clocks mean "same second" includes tokens
        // minted just before the user clicked revoke (the e2e-caught
        // gap: same-second logins survived a strict `<` against `now`).
        assert_eq!(l.check("alice", NOW).await, RevocationStatus::Revoked);
        assert_eq!(l.check("alice", NOW - 1).await, RevocationStatus::Revoked);
        assert_eq!(
            l.check("alice", NOW - 25 * 24 * 3600).await,
            RevocationStatus::Revoked
        );
        // A post-revocation mint (auth_time bumped to not_before) works…
        assert_eq!(l.check("alice", NOW + 1).await, RevocationStatus::Ok);
        assert_eq!(l.check("alice", NOW + 10).await, RevocationStatus::Ok);
        // …and other users are untouched.
        assert_eq!(l.check("bob", NOW - 1).await, RevocationStatus::Ok);
    }

    #[tokio::test]
    async fn min_auth_time_reports_the_relogin_floor() {
        let temp = TempDir::new().unwrap();
        let l = ledger(temp.path());
        assert_eq!(l.min_auth_time("alice").await, None);
        l.revoke_all_for("alice", NOW).await.unwrap();
        // The mint path stamps auth_time = max(now, floor) so an
        // immediate (same-second) re-login is provably post-revocation.
        assert_eq!(l.min_auth_time("alice").await, Some(NOW + 1));
    }

    #[tokio::test]
    async fn revoke_never_moves_not_before_backwards() {
        let temp = TempDir::new().unwrap();
        let l = ledger(temp.path());
        l.revoke_all_for("alice", NOW).await.unwrap();
        l.revoke_all_for("alice", NOW - 5000).await.unwrap();
        // The later (stronger) event still governs.
        assert_eq!(
            l.check("alice", NOW - 1).await,
            RevocationStatus::Revoked,
            "an older concurrent revoke must not shorten the window"
        );
    }

    #[tokio::test]
    async fn revocations_survive_restart() {
        let temp = TempDir::new().unwrap();
        {
            let l = ledger(temp.path());
            l.revoke_all_for("alice", NOW).await.unwrap();
        }
        let l = ledger(temp.path());
        assert_eq!(l.check("alice", NOW - 1).await, RevocationStatus::Revoked);
    }

    #[tokio::test]
    async fn banned_sub_rejected_regardless_of_auth_time() {
        let temp = TempDir::new().unwrap();
        std::fs::write(
            temp.path().join(REVOCATIONS_FILE),
            r#"{"version":1,"not_before":{},"banned":["mallory"]}"#,
        )
        .unwrap();
        let l = ledger(temp.path());
        assert_eq!(
            l.check("mallory", NOW + 9999).await,
            RevocationStatus::Banned,
            "bans dominate any auth_time"
        );
        assert!(l.is_banned("mallory").await);
        assert!(!l.is_banned("alice").await);
    }

    #[tokio::test]
    async fn gc_drops_expired_logout_entries_but_never_bans() {
        let temp = TempDir::new().unwrap();
        // One stale logout entry (older than the absolute cap), one
        // live one, one ban.
        let stale = NOW - ABSOLUTE - 10;
        std::fs::write(
            temp.path().join(REVOCATIONS_FILE),
            format!(
                r#"{{"version":1,"not_before":{{"stale-user":{stale},"live-user":{}}},"banned":["mallory"]}}"#,
                NOW - 10
            ),
        )
        .unwrap();
        let l = ledger(temp.path());

        // GC'd at load: the stale entry no longer bites (its window is
        // empty anyway); the live one and the ban still do.
        assert_eq!(l.check("stale-user", stale - 1).await, RevocationStatus::Ok);
        assert_eq!(
            l.check("live-user", NOW - 100).await,
            RevocationStatus::Revoked
        );
        assert_eq!(l.check("mallory", NOW).await, RevocationStatus::Banned);

        // A write persists the GC'd view; the ban is retained on disk.
        l.revoke_all_for("carol", NOW).await.unwrap();
        let on_disk = std::fs::read_to_string(temp.path().join(REVOCATIONS_FILE)).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&on_disk).unwrap();
        assert!(parsed["not_before"].get("stale-user").is_none());
        assert!(parsed["not_before"].get("live-user").is_some());
        assert!(parsed["not_before"].get("carol").is_some());
        assert_eq!(parsed["banned"], serde_json::json!(["mallory"]));
    }

    #[tokio::test]
    async fn persist_is_atomic_and_leaves_no_tmp() {
        let temp = TempDir::new().unwrap();
        let l = ledger(temp.path());
        l.revoke_all_for("alice", NOW).await.unwrap();

        assert!(temp.path().join(REVOCATIONS_FILE).exists());
        let has_tmp = std::fs::read_dir(temp.path())
            .unwrap()
            .any(|e| e.unwrap().file_name().to_string_lossy().ends_with(".tmp"));
        assert!(!has_tmp);

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(temp.path().join(REVOCATIONS_FILE))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600, "revocations.json must be owner-only");
        }
    }

    #[tokio::test]
    async fn malformed_file_is_a_startup_error() {
        // Fail loudly rather than silently starting with an empty
        // ledger (which would un-ban users).
        let temp = TempDir::new().unwrap();
        std::fs::write(temp.path().join(REVOCATIONS_FILE), "not json").unwrap();
        assert!(RevocationLedger::load(temp.path(), ABSOLUTE, NOW).is_err());
    }
}
