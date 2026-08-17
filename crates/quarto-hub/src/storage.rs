//! Storage management for the hub
//!
//! Manages the hub data directory and lockfile.
//!
//! Two modes are supported:
//! - **Project mode**: Storage lives in `<project_root>/.quarto/hub/`. The hub
//!   discovers and syncs files from the project directory.
//! - **Standalone mode**: Storage lives in a user-specified data directory. The
//!   hub acts as a pure sync server with no local project.

use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

use fs2::FileExt;
use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

use crate::error::{Error, Result};

/// Current hub storage format version.
///
/// Increment this when making breaking changes to the storage format.
/// The hub will check this version on startup and can perform migrations.
pub const CURRENT_HUB_VERSION: u32 = 1;

/// Hub configuration stored in `hub.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HubStorageConfig {
    /// Storage format version (for migrations)
    pub version: u32,

    /// When this hub directory was created (ISO 8601)
    pub created_at: String,

    /// Last time the hub was started (ISO 8601)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_started_at: Option<String>,

    /// The bs58-encoded DocumentId for the project index document.
    /// This stores the mapping from file paths to automerge document IDs.
    /// None on first run, populated after the index document is created.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub index_document_id: Option<String>,

    /// URLs of sync servers to peer with (e.g., "wss://sync.automerge.org").
    /// These are persisted so the hub reconnects to the same peers on restart.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub peers: Vec<String>,

    /// Server secret for HMAC actor ID derivation (hex-encoded 32 bytes).
    /// Auto-generated on first run, used to compute per-project actor IDs:
    /// `HMAC-SHA256(server_secret, sub || "\0" || project_id)`.
    /// Absent in old configs; a new secret is generated on first startup.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server_secret: Option<String>,

    /// Session-token signing secret (hex-encoded 32 bytes), used to mint
    /// and verify hub session cookies (HS256). Deliberately distinct from
    /// `server_secret`: leaking this one enables full session forgery,
    /// not just actor-id correlation. Auto-generated on first run.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_secret: Option<String>,

    /// Previous session secret during a **graceful** rotation (hex).
    /// Verification-only; signing always uses `session_secret`. Both
    /// verify during an overlap window of one idle timeout from
    /// `session_secret_rotated_at`, after which this entry is ignored.
    /// **Never set this when rotating in response to a compromise** —
    /// an overlap window keeps accepting attacker-forgeable cookies;
    /// the emergency procedure is a new `session_secret` with no
    /// previous (immediate global invalidation).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_session_secret: Option<String>,

    /// When the graceful rotation happened (epoch seconds). Required
    /// whenever `previous_session_secret` is set — it bounds the
    /// overlap window.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_secret_rotated_at: Option<i64>,
}

impl HubStorageConfig {
    /// Create a new config with current version and timestamp.
    fn new() -> Self {
        Self {
            version: CURRENT_HUB_VERSION,
            created_at: chrono_now(),
            last_started_at: None,
            index_document_id: None,
            peers: Vec::new(),
            server_secret: None,
            session_secret: None,
            previous_session_secret: None,
            session_secret_rotated_at: None,
        }
    }

    /// Load config from file, or create new if it doesn't exist.
    fn load_or_create(hub_dir: &Path) -> Result<Self> {
        let config_path = hub_dir.join("hub.json");

        if config_path.exists() {
            let content = fs::read_to_string(&config_path)?;
            let mut config: HubStorageConfig =
                serde_json::from_str(&content).map_err(|e| Error::ConfigParse(e.to_string()))?;

            // Check version compatibility
            if config.version > CURRENT_HUB_VERSION {
                return Err(Error::ConfigVersionTooNew {
                    found: config.version,
                    supported: CURRENT_HUB_VERSION,
                });
            }

            if config.version < CURRENT_HUB_VERSION {
                // Future: perform migrations here
                warn!(
                    old_version = config.version,
                    new_version = CURRENT_HUB_VERSION,
                    "Hub storage version upgrade needed (not yet implemented)"
                );
            }

            // Update last_started_at
            config.last_started_at = Some(chrono_now());
            config.save(hub_dir)?;

            Ok(config)
        } else {
            let config = HubStorageConfig::new();
            config.save(hub_dir)?;
            Ok(config)
        }
    }

    /// Save config to file.
    ///
    /// On Unix the file is opened with `mode(0o600)` before writing, so it is
    /// never visible with permissive permissions (no TOCTOU window). On
    /// non-Unix platforms the file is written without an explicit mode.
    fn save(&self, hub_dir: &Path) -> Result<()> {
        let config_path = hub_dir.join("hub.json");
        let content =
            serde_json::to_string_pretty(self).map_err(|e| Error::ConfigParse(e.to_string()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            let mut f = std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(&config_path)?;
            f.write_all(content.as_bytes())?;
        }
        #[cfg(not(unix))]
        {
            fs::write(&config_path, content)?;
        }
        Ok(())
    }
}

/// Default data directory for standalone mode.
///
/// Uses the platform-appropriate data directory:
/// - Linux: `$XDG_DATA_HOME/quarto-hub` or `~/.local/share/quarto-hub`
/// - macOS: `~/Library/Application Support/quarto-hub`
/// - Windows: `{FOLDERID_RoamingAppData}/quarto-hub`
pub fn default_standalone_data_dir() -> PathBuf {
    if let Some(data_dir) = dirs::data_dir() {
        data_dir.join("quarto-hub")
    } else {
        dirs::home_dir()
            .expect("Could not determine home directory")
            .join(".local")
            .join("share")
            .join("quarto-hub")
    }
}

/// Decode a 64-char hex string into a 32-byte array, with a source label for
/// error messages.
fn decode_secret_hex(hex: &str, source: &str) -> Result<[u8; 32]> {
    let bytes =
        hex::decode(hex).map_err(|e| Error::ConfigParse(format!("{source}: invalid hex: {e}")))?;
    bytes.as_slice().try_into().map_err(|_| {
        Error::ConfigParse(format!(
            "{source}: expected 32 bytes (64 hex chars), got {}",
            bytes.len()
        ))
    })
}

/// Generate a fresh random 32-byte secret.
fn generate_secret() -> [u8; 32] {
    use rand::RngCore;
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    bytes
}

/// How a [`StorageManager`] treats the two signing secrets it resolves at
/// startup (the server secret for actor-id derivation and the session
/// secret for session-cookie signing).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SecretPolicy {
    /// Resolve from env / `hub.json`, auto-generating **and persisting** on
    /// first run with a loud warning: the secret is now pinned to the data
    /// directory, so multi-instance deployments must keep it in sync. The
    /// right policy for real hub servers.
    Persist,
    /// Resolve from env if set, else generate fresh per process. Never
    /// persisted to `hub.json`, never warned about: the data directory is
    /// per-session, so pinning is meaningless. The right policy for
    /// short-lived embedded hubs (`q2 preview`).
    Ephemeral,
}

/// Ephemeral secret resolution: the env var if set (same highest-priority
/// override as the persistent path), else fresh random bytes. Never touches
/// `hub.json`, never warns — see [`SecretPolicy::Ephemeral`].
fn resolve_ephemeral_secret(env_var: &str) -> Result<[u8; 32]> {
    if let Ok(hex) = std::env::var(env_var) {
        return decode_secret_hex(&hex, env_var);
    }
    let bytes = generate_secret();
    debug!(env_var, "generated ephemeral secret (not persisted)");
    Ok(bytes)
}

/// Resolve the server secret for HMAC actor ID derivation.
///
/// Resolution order (highest priority first):
/// 1. `QUARTO_HUB_SERVER_SECRET` environment variable (64-char lowercase hex). Use for
///    containers, secret managers, and CI. No file I/O is performed.
/// 2. `config.server_secret` field in `hub.json`. Auto-loaded from the existing file.
/// 3. Auto-generate: 32 random bytes are generated, hex-encoded, stored in
///    `config.server_secret`, and persisted via `config.save(hub_dir)`.
///
/// Returns the resolved secret as a 32-byte array.
pub fn resolve_server_secret(config: &mut HubStorageConfig, hub_dir: &Path) -> Result<[u8; 32]> {
    // 1. Environment variable (highest priority — no file I/O, no config mutation)
    if let Ok(hex) = std::env::var("QUARTO_HUB_SERVER_SECRET") {
        return decode_secret_hex(&hex, "QUARTO_HUB_SERVER_SECRET");
    }

    // 2. Existing config value
    if let Some(ref hex) = config.server_secret {
        return decode_secret_hex(hex, "hub.json server_secret");
    }

    // 3. Auto-generate, persist, and return
    let bytes = generate_secret();
    config.server_secret = Some(hex::encode(bytes));
    config.save(hub_dir)?;
    // Loud because it is now pinned to *this* data directory: a second
    // instance with its own generated secret derives a different actor ID
    // for the same user in the same project. The value itself is never
    // logged.
    warn!(
        hub_dir = %hub_dir.display(),
        "generated a new server secret and persisted it to hub.json. \
         Multi-instance deployments must set QUARTO_HUB_SERVER_SECRET to \
         the same value on every instance; otherwise each derives its own \
         actor IDs."
    );
    Ok(bytes)
}

/// Resolve the session-token signing secret.
///
/// Same resolution order as [`resolve_server_secret`], with its own
/// sources (the two secrets must never be shared — different blast
/// radius):
/// 1. `QUARTO_HUB_SESSION_SECRET` environment variable (64-char hex).
///    Also the multi-instance deployment mechanism: hubs sharing this
///    env var accept each other's session cookies.
/// 2. `config.session_secret` field in `hub.json`.
/// 3. Auto-generate 32 random bytes, persist via `config.save(hub_dir)`.
pub fn resolve_session_secret(config: &mut HubStorageConfig, hub_dir: &Path) -> Result<[u8; 32]> {
    // 1. Environment variable (highest priority — no file I/O, no config mutation)
    if let Ok(hex) = std::env::var("QUARTO_HUB_SESSION_SECRET") {
        return decode_secret_hex(&hex, "QUARTO_HUB_SESSION_SECRET");
    }

    // 2. Existing config value
    if let Some(ref hex) = config.session_secret {
        return decode_secret_hex(hex, "hub.json session_secret");
    }

    // 3. Auto-generate, persist, and return
    let bytes = generate_secret();
    config.session_secret = Some(hex::encode(bytes));
    config.save(hub_dir)?;
    // The multi-instance hazard this warns about is genuinely hard to
    // diagnose from symptoms: two hubs with divergent generated secrets
    // reject each other's session cookies and sealed login blobs, so
    // sign-in fails intermittently and heals itself on retry. Its audit
    // signature is a run of `*_kid_mismatch`. The value itself is never
    // logged.
    warn!(
        hub_dir = %hub_dir.display(),
        "generated a new session secret and persisted it to hub.json — it is \
         now pinned to this data directory. Multi-instance deployments must \
         set QUARTO_HUB_SESSION_SECRET to the same value on every instance; \
         otherwise instances reject each other's session cookies."
    );
    Ok(bytes)
}

/// Resolve the **previous** session secret for a graceful-rotation
/// overlap window (verification-only; C5b).
///
/// Sources, strictly paired (an env previous requires an env
/// rotated-at; a `hub.json` previous requires the `hub.json` field):
/// 1. `QUARTO_HUB_SESSION_SECRET_PREVIOUS` +
///    `QUARTO_HUB_SESSION_SECRET_ROTATED_AT` (epoch seconds);
/// 2. `config.previous_session_secret` + `config.session_secret_rotated_at`.
///
/// Returns `None` when no previous secret is configured **or the
/// overlap window has lapsed** (`rotated_at + idle ≤ now` — every
/// active session has re-minted under the current `kid` by then, §2c).
/// A previous secret without its rotated-at timestamp is a hard config
/// error: an unbounded overlap window silently defeats rotation.
///
/// **Emergency rotation (secret compromise)** is the *absence* of this
/// configuration: supply only the new current secret — every
/// outstanding token dies immediately. Never respond to a compromise
/// with a graceful rotation; the overlap window would keep accepting
/// attacker-minted cookies.
pub fn resolve_previous_session_secret(
    config: &HubStorageConfig,
    idle_secs: i64,
    now: i64,
) -> Result<Option<[u8; 32]>> {
    let (hex, rotated_at, source) =
        if let Ok(hex) = std::env::var("QUARTO_HUB_SESSION_SECRET_PREVIOUS") {
            let rotated_at = match std::env::var("QUARTO_HUB_SESSION_SECRET_ROTATED_AT") {
                Ok(raw) => raw.parse::<i64>().map_err(|e| {
                    Error::ConfigParse(format!(
                        "QUARTO_HUB_SESSION_SECRET_ROTATED_AT: invalid epoch seconds '{raw}': {e}"
                    ))
                })?,
                Err(_) => {
                    return Err(Error::ConfigParse(
                        "QUARTO_HUB_SESSION_SECRET_PREVIOUS requires \
                     QUARTO_HUB_SESSION_SECRET_ROTATED_AT (epoch seconds): an unbounded \
                     overlap window would silently defeat the rotation"
                            .to_string(),
                    ));
                }
            };
            (hex, rotated_at, "QUARTO_HUB_SESSION_SECRET_PREVIOUS")
        } else if let Some(ref hex) = config.previous_session_secret {
            let Some(rotated_at) = config.session_secret_rotated_at else {
                return Err(Error::ConfigParse(
                    "hub.json previous_session_secret requires session_secret_rotated_at \
                 (epoch seconds): an unbounded overlap window would silently defeat \
                 the rotation"
                        .to_string(),
                ));
            };
            (hex.clone(), rotated_at, "hub.json previous_session_secret")
        } else {
            return Ok(None);
        };

    if rotated_at + idle_secs <= now {
        tracing::info!(
            rotated_at,
            "previous session secret overlap window has lapsed; ignoring it \
             (remove it from hub.json / the environment)"
        );
        return Ok(None);
    }

    decode_secret_hex(&hex, source).map(Some)
}

/// Get current time as ISO 8601 string (without external crate).
fn chrono_now() -> String {
    use std::time::SystemTime;
    let now = SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    // Simple ISO-ish format: just seconds since epoch for now
    // In production, you'd use chrono crate
    format!("{}", now.as_secs())
}

/// Manages the hub data directory and holds the lockfile.
///
/// The lockfile is held for the lifetime of this struct, preventing
/// multiple hub instances from running on the same data directory.
pub struct StorageManager {
    /// Root of the Quarto project (None in standalone mode)
    project_root: Option<PathBuf>,

    /// Path to the hub data directory.
    /// In project mode: `<project_root>/.quarto/hub/`
    /// In standalone mode: the user-specified data directory.
    hub_dir: PathBuf,

    /// Open lockfile (lock released on drop)
    #[allow(dead_code)]
    lock_file: File,

    /// Hub storage configuration (version, timestamps)
    config: HubStorageConfig,

    /// Resolved server secret (32 bytes). Decoded once at startup from the
    /// env var or `hub.json`; never re-derived per request.
    server_secret: [u8; 32],

    /// Resolved session-token signing secret (32 bytes). Decoded once at
    /// startup; distinct from `server_secret` (session-forgery blast radius).
    session_secret: [u8; 32],
}

impl StorageManager {
    /// Create a new StorageManager for the given project root.
    ///
    /// Storage is placed in `<project_root>/.quarto/hub/`. This is the
    /// default mode for `quarto hub`, where the hub watches a local project.
    ///
    /// This will:
    /// 1. Create `.quarto/hub/` if it doesn't exist
    /// 2. Acquire an exclusive lock on `hub.lock`
    /// 3. Write the current PID to the lockfile
    /// 4. Load or create `hub.json` config file
    ///
    /// Returns an error if another hub instance is already running.
    pub fn new(project_root: impl AsRef<Path>) -> Result<Self> {
        let project_root = project_root.as_ref().to_path_buf();

        if !project_root.exists() {
            return Err(Error::ProjectNotFound(project_root));
        }

        let hub_dir = project_root.join(".quarto").join("hub");

        Self::init(Some(project_root), hub_dir, SecretPolicy::Persist)
    }

    /// Create a StorageManager for standalone mode (no local project).
    ///
    /// Storage is placed directly in `data_dir`. This mode is used when
    /// the hub acts as a pure sync server without watching any local files.
    ///
    /// The directory will be created if it doesn't exist.
    pub fn new_standalone(data_dir: impl AsRef<Path>) -> Result<Self> {
        let hub_dir = data_dir.as_ref().to_path_buf();

        Self::init(None, hub_dir, SecretPolicy::Persist)
    }

    /// Create a StorageManager for project mode with an explicit data dir.
    ///
    /// Used by `quarto preview`: the project is watched (so file
    /// changes propagate into automerge as usual), but the samod
    /// storage lives in a caller-controlled `data_dir` — typically a
    /// `tempfile::TempDir` that's deleted on shutdown. This keeps each
    /// `q2 preview` invocation ephemeral instead of leaving a
    /// `.quarto/hub/` directory in the user's project.
    pub fn new_with_data_dir(
        project_root: impl AsRef<Path>,
        data_dir: impl AsRef<Path>,
    ) -> Result<Self> {
        let project_root = project_root.as_ref().to_path_buf();
        if !project_root.exists() {
            return Err(Error::ProjectNotFound(project_root));
        }
        let hub_dir = data_dir.as_ref().to_path_buf();
        Self::init(Some(project_root), hub_dir, SecretPolicy::Persist)
    }

    /// Create a StorageManager for standalone mode (no local project)
    /// with **ephemeral secrets**: the server and session secrets are
    /// resolved per process — from the `QUARTO_HUB_SERVER_SECRET` /
    /// `QUARTO_HUB_SESSION_SECRET` env vars when set, otherwise freshly
    /// generated — and are never persisted to `hub.json`.
    ///
    /// Use this for short-lived embedded hubs (e.g. `q2 preview`) whose
    /// data directory is deleted on exit: a persisted secret would pin
    /// nothing, so the multi-instance warning emitted by
    /// [`new_standalone`](Self::new_standalone) would be noise.
    pub fn new_standalone_ephemeral(data_dir: impl AsRef<Path>) -> Result<Self> {
        let hub_dir = data_dir.as_ref().to_path_buf();

        Self::init(None, hub_dir, SecretPolicy::Ephemeral)
    }

    /// Create a StorageManager for project mode with an explicit data dir
    /// and **ephemeral secrets** — see
    /// [`new_standalone_ephemeral`](Self::new_standalone_ephemeral) for the
    /// secret semantics. Used by `q2 preview`, which watches the project
    /// but keeps samod storage in a per-session `TempDir`.
    pub fn new_with_data_dir_ephemeral(
        project_root: impl AsRef<Path>,
        data_dir: impl AsRef<Path>,
    ) -> Result<Self> {
        let project_root = project_root.as_ref().to_path_buf();
        if !project_root.exists() {
            return Err(Error::ProjectNotFound(project_root));
        }
        let hub_dir = data_dir.as_ref().to_path_buf();
        Self::init(Some(project_root), hub_dir, SecretPolicy::Ephemeral)
    }

    /// Shared initialization logic for both project and standalone modes.
    fn init(
        project_root: Option<PathBuf>,
        hub_dir: PathBuf,
        secret_policy: SecretPolicy,
    ) -> Result<Self> {
        fs::create_dir_all(&hub_dir).map_err(Error::CreateHubDir)?;

        // The hub dir holds signing secrets (`hub.json`). In project mode it
        // sits inside the user's project tree, so a catch-all .gitignore is
        // written on every startup that finds it missing — a committed
        // session secret means full session forgery, not just actor-id
        // correlation.
        let gitignore_path = hub_dir.join(".gitignore");
        if !gitignore_path.exists() {
            fs::write(&gitignore_path, "*\n")?;
        }

        let lock_path = hub_dir.join("hub.lock");
        debug!(?lock_path, "Acquiring lockfile");

        let mut lock_file = File::create(&lock_path).map_err(Error::LockfileAcquire)?;

        // Try to acquire exclusive lock (non-blocking)
        lock_file.try_lock_exclusive().map_err(|e| {
            if e.kind() == std::io::ErrorKind::WouldBlock {
                Error::HubAlreadyRunning
            } else {
                Error::LockfileAcquire(e)
            }
        })?;

        // Write PID to lockfile for debugging
        writeln!(lock_file, "{}", std::process::id())?;

        // Load or create hub config
        let mut config = HubStorageConfig::load_or_create(&hub_dir)?;

        // Resolve and cache the server secret (HMAC actor ID derivation)
        // and the session-token signing secret. Ephemeral hubs keep both
        // in memory only: nothing is persisted to hub.json, and the
        // multi-instance pinning warning does not apply.
        let (server_secret, session_secret) = match secret_policy {
            SecretPolicy::Persist => (
                resolve_server_secret(&mut config, &hub_dir)?,
                resolve_session_secret(&mut config, &hub_dir)?,
            ),
            SecretPolicy::Ephemeral => (
                resolve_ephemeral_secret("QUARTO_HUB_SERVER_SECRET")?,
                resolve_ephemeral_secret("QUARTO_HUB_SESSION_SECRET")?,
            ),
        };

        if let Some(ref project_root) = project_root {
            info!(
                project_root = %project_root.display(),
                hub_dir = %hub_dir.display(),
                version = config.version,
                "Storage manager initialized (project mode)"
            );
        } else {
            info!(
                hub_dir = %hub_dir.display(),
                version = config.version,
                "Storage manager initialized (standalone mode)"
            );
        }

        Ok(Self {
            project_root,
            hub_dir,
            lock_file,
            config,
            server_secret,
            session_secret,
        })
    }

    /// Returns the storage format version.
    pub fn version(&self) -> u32 {
        self.config.version
    }

    /// Returns the storage config.
    pub fn config(&self) -> &HubStorageConfig {
        &self.config
    }

    /// Returns the project root directory, if running in project mode.
    ///
    /// Returns `None` in standalone mode (no local project).
    pub fn project_root(&self) -> Option<&Path> {
        self.project_root.as_deref()
    }

    /// Returns the hub data directory.
    ///
    /// In project mode: `<project_root>/.quarto/hub/`
    /// In standalone mode: the user-specified data directory.
    pub fn hub_dir(&self) -> &Path {
        &self.hub_dir
    }

    /// Returns the path where samod stores automerge documents.
    /// This directory is managed entirely by samod's TokioFilesystemStorage.
    pub fn automerge_dir(&self) -> PathBuf {
        self.hub_dir.join("automerge")
    }

    /// Returns the index document ID if one has been set.
    pub fn index_document_id(&self) -> Option<&str> {
        self.config.index_document_id.as_deref()
    }

    /// Update and persist the index document ID.
    /// Called after creating the index document for the first time.
    pub fn set_index_document_id(&mut self, doc_id: &str) -> Result<()> {
        self.config.index_document_id = Some(doc_id.to_string());
        self.config.save(&self.hub_dir)
    }

    /// Returns the resolved server secret (32 bytes).
    ///
    /// The secret is decoded once at startup and stored opaquely.
    /// Use with [`crate::auth::sub_to_actor_id_for_project`] to compute
    /// per-project actor IDs.
    pub fn server_secret(&self) -> &[u8] {
        &self.server_secret
    }

    /// Returns the resolved session-token signing secret (32 bytes).
    ///
    /// Decoded once at startup. Feed to [`crate::session::SessionKeys`]
    /// to mint/verify hub session cookies; never reuse for actor IDs.
    pub fn session_secret(&self) -> &[u8; 32] {
        &self.session_secret
    }

    /// Returns the configured peer URLs.
    pub fn peers(&self) -> &[String] {
        &self.config.peers
    }

    /// Update and persist the peer URLs.
    /// Called when CLI provides peer URLs.
    pub fn set_peers(&mut self, peers: Vec<String>) -> Result<()> {
        self.config.peers = peers;
        self.config.save(&self.hub_dir)
    }
}

impl Drop for StorageManager {
    fn drop(&mut self) {
        // Lock is automatically released when file is closed.
        // Optionally remove the lock file (best effort).
        let lock_path = self.hub_dir.join("hub.lock");
        if let Err(e) = fs::remove_file(&lock_path) {
            debug!(?lock_path, error = %e, "Failed to remove lockfile (may be expected)");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_storage_manager_creates_hub_dir() {
        let temp = TempDir::new().unwrap();
        let manager = StorageManager::new(temp.path()).unwrap();

        assert!(manager.hub_dir().exists());
        assert!(manager.hub_dir().join("hub.lock").exists());
    }

    #[test]
    fn test_storage_manager_creates_config_file() {
        let temp = TempDir::new().unwrap();
        let manager = StorageManager::new(temp.path()).unwrap();

        // Config file should exist
        let config_path = manager.hub_dir().join("hub.json");
        assert!(config_path.exists());

        // Version should be current
        assert_eq!(manager.version(), CURRENT_HUB_VERSION);

        // Read and verify the file content
        let content = fs::read_to_string(&config_path).unwrap();
        let config: HubStorageConfig = serde_json::from_str(&content).unwrap();
        assert_eq!(config.version, CURRENT_HUB_VERSION);
    }

    #[test]
    fn test_storage_manager_rejects_future_version() {
        let temp = TempDir::new().unwrap();
        let hub_dir = temp.path().join(".quarto").join("hub");
        fs::create_dir_all(&hub_dir).unwrap();

        // Write a config with a future version
        let future_config = r#"{"version": 999, "created_at": "123456"}"#;
        fs::write(hub_dir.join("hub.json"), future_config).unwrap();

        let result = StorageManager::new(temp.path());
        assert!(matches!(
            result,
            Err(Error::ConfigVersionTooNew {
                found: 999,
                supported: CURRENT_HUB_VERSION
            })
        ));
    }

    #[test]
    fn test_storage_manager_prevents_double_lock() {
        let temp = TempDir::new().unwrap();
        let _manager1 = StorageManager::new(temp.path()).unwrap();

        // Second attempt should fail
        let result = StorageManager::new(temp.path());
        assert!(matches!(result, Err(Error::HubAlreadyRunning)));
    }

    #[test]
    fn test_storage_manager_nonexistent_project() {
        let result = StorageManager::new("/nonexistent/path/that/does/not/exist");
        assert!(matches!(result, Err(Error::ProjectNotFound(_))));
    }

    #[test]
    fn test_storage_manager_project_mode_has_project_root() {
        let temp = TempDir::new().unwrap();
        let manager = StorageManager::new(temp.path()).unwrap();

        assert!(manager.project_root().is_some());
        assert_eq!(manager.project_root().unwrap(), temp.path());
    }

    #[test]
    fn test_storage_manager_standalone_creates_data_dir() {
        let temp = TempDir::new().unwrap();
        let data_dir = temp.path().join("hub-data");

        let manager = StorageManager::new_standalone(&data_dir).unwrap();

        assert!(manager.hub_dir().exists());
        assert!(manager.hub_dir().join("hub.lock").exists());
        assert!(manager.hub_dir().join("hub.json").exists());
        assert_eq!(manager.hub_dir(), data_dir);
    }

    #[test]
    fn test_storage_manager_standalone_has_no_project_root() {
        let temp = TempDir::new().unwrap();
        let data_dir = temp.path().join("hub-data");

        let manager = StorageManager::new_standalone(&data_dir).unwrap();

        assert!(manager.project_root().is_none());
    }

    #[test]
    fn test_storage_manager_standalone_prevents_double_lock() {
        let temp = TempDir::new().unwrap();
        let data_dir = temp.path().join("hub-data");

        let _manager1 = StorageManager::new_standalone(&data_dir).unwrap();

        let result = StorageManager::new_standalone(&data_dir);
        assert!(matches!(result, Err(Error::HubAlreadyRunning)));
    }

    // ── resolve_server_secret ─────────────────────────────────────

    /// Mutex to serialize env var tests (env vars are process-global).
    static ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn resolve_secret_env_var_used_directly() {
        let _guard = ENV_MUTEX.lock().unwrap();

        let temp = TempDir::new().unwrap();
        let hub_dir = temp.path().join("hub");
        fs::create_dir_all(&hub_dir).unwrap();

        let expected = [42u8; 32];
        let hex = hex::encode(expected);

        // SAFETY: test-only env mutation, serialized by ENV_MUTEX.
        unsafe { std::env::set_var("QUARTO_HUB_SERVER_SECRET", &hex) };
        let mut config = HubStorageConfig::new();
        let result = resolve_server_secret(&mut config, &hub_dir);
        unsafe { std::env::remove_var("QUARTO_HUB_SERVER_SECRET") };

        assert_eq!(result.unwrap(), expected);
        // Config must not have been mutated (no file I/O path)
        assert!(config.server_secret.is_none());
        // No hub.json written
        assert!(!hub_dir.join("hub.json").exists());
    }

    #[test]
    fn resolve_secret_env_var_invalid_hex_returns_error() {
        let _guard = ENV_MUTEX.lock().unwrap();

        let temp = TempDir::new().unwrap();
        let hub_dir = temp.path().join("hub");
        fs::create_dir_all(&hub_dir).unwrap();

        unsafe { std::env::set_var("QUARTO_HUB_SERVER_SECRET", "not-hex") };
        let mut config = HubStorageConfig::new();
        let result = resolve_server_secret(&mut config, &hub_dir);
        unsafe { std::env::remove_var("QUARTO_HUB_SERVER_SECRET") };

        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("QUARTO_HUB_SERVER_SECRET"), "got: {msg}");
    }

    #[test]
    fn resolve_secret_generates_and_saves_when_config_empty() {
        let _guard = ENV_MUTEX.lock().unwrap();

        let temp = TempDir::new().unwrap();
        let hub_dir = temp.path().join("hub");
        fs::create_dir_all(&hub_dir).unwrap();

        unsafe { std::env::remove_var("QUARTO_HUB_SERVER_SECRET") };

        let mut config = HubStorageConfig::new();
        let secret = resolve_server_secret(&mut config, &hub_dir).unwrap();

        // Secret should be 32 non-zero bytes (statistically almost always true)
        assert_eq!(secret.len(), 32);
        // Config should now have the secret stored as hex
        assert!(config.server_secret.is_some());
        let stored_hex = config.server_secret.as_ref().unwrap();
        assert_eq!(stored_hex.len(), 64);
        // Should round-trip correctly
        let decoded = hex::decode(stored_hex).unwrap();
        assert_eq!(decoded.as_slice(), &secret);
        // hub.json should have been written
        assert!(hub_dir.join("hub.json").exists());
    }

    #[test]
    fn resolve_secret_returns_same_secret_across_calls() {
        let _guard = ENV_MUTEX.lock().unwrap();

        let temp = TempDir::new().unwrap();
        let hub_dir = temp.path().join("hub");
        fs::create_dir_all(&hub_dir).unwrap();

        unsafe { std::env::remove_var("QUARTO_HUB_SERVER_SECRET") };

        let mut config = HubStorageConfig::new();
        let secret1 = resolve_server_secret(&mut config, &hub_dir).unwrap();
        let secret2 = resolve_server_secret(&mut config, &hub_dir).unwrap();

        assert_eq!(secret1, secret2);
    }

    // ── resolve_session_secret (C1, bd-sekcpmv1) ─────────────────

    #[test]
    fn resolve_session_secret_env_var_used_directly() {
        let _guard = ENV_MUTEX.lock().unwrap();

        let temp = TempDir::new().unwrap();
        let hub_dir = temp.path().join("hub");
        fs::create_dir_all(&hub_dir).unwrap();

        let expected = [7u8; 32];
        let hex = hex::encode(expected);

        // SAFETY: test-only env mutation, serialized by ENV_MUTEX.
        unsafe { std::env::set_var("QUARTO_HUB_SESSION_SECRET", &hex) };
        let mut config = HubStorageConfig::new();
        let result = resolve_session_secret(&mut config, &hub_dir);
        unsafe { std::env::remove_var("QUARTO_HUB_SESSION_SECRET") };

        assert_eq!(result.unwrap(), expected);
        // Config must not have been mutated (no file I/O path)
        assert!(config.session_secret.is_none());
        assert!(!hub_dir.join("hub.json").exists());
    }

    #[test]
    fn resolve_session_secret_reads_existing_config_value() {
        let _guard = ENV_MUTEX.lock().unwrap();
        unsafe { std::env::remove_var("QUARTO_HUB_SESSION_SECRET") };

        let temp = TempDir::new().unwrap();
        let hub_dir = temp.path().join("hub");
        fs::create_dir_all(&hub_dir).unwrap();

        let expected = [9u8; 32];
        let mut config = HubStorageConfig::new();
        config.session_secret = Some(hex::encode(expected));

        let result = resolve_session_secret(&mut config, &hub_dir).unwrap();
        assert_eq!(result, expected);
    }

    #[test]
    fn resolve_session_secret_generates_and_saves_when_config_empty() {
        let _guard = ENV_MUTEX.lock().unwrap();
        unsafe { std::env::remove_var("QUARTO_HUB_SESSION_SECRET") };

        let temp = TempDir::new().unwrap();
        let hub_dir = temp.path().join("hub");
        fs::create_dir_all(&hub_dir).unwrap();

        let mut config = HubStorageConfig::new();
        let secret = resolve_session_secret(&mut config, &hub_dir).unwrap();

        let stored_hex = config.session_secret.as_ref().expect("secret persisted");
        assert_eq!(stored_hex.len(), 64);
        assert_eq!(hex::decode(stored_hex).unwrap().as_slice(), &secret);
        assert!(hub_dir.join("hub.json").exists());

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(hub_dir.join("hub.json"))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600, "hub.json must be owner-only");
        }
    }

    #[test]
    fn session_secret_is_distinct_from_server_secret() {
        let _guard = ENV_MUTEX.lock().unwrap();
        unsafe { std::env::remove_var("QUARTO_HUB_SERVER_SECRET") };
        unsafe { std::env::remove_var("QUARTO_HUB_SESSION_SECRET") };

        let temp = TempDir::new().unwrap();
        let hub_dir = temp.path().join("hub");
        fs::create_dir_all(&hub_dir).unwrap();

        let mut config = HubStorageConfig::new();
        let server = resolve_server_secret(&mut config, &hub_dir).unwrap();
        let session = resolve_session_secret(&mut config, &hub_dir).unwrap();
        assert_ne!(
            server, session,
            "session secret must never equal the actor-id server secret"
        );
    }

    #[test]
    fn session_secret_survives_restart() {
        let _guard = ENV_MUTEX.lock().unwrap();
        unsafe { std::env::remove_var("QUARTO_HUB_SESSION_SECRET") };

        let temp = TempDir::new().unwrap();

        let first = {
            let manager = StorageManager::new(temp.path()).unwrap();
            *manager.session_secret()
        }; // manager dropped → lock released

        let manager = StorageManager::new(temp.path()).unwrap();
        assert_eq!(
            *manager.session_secret(),
            first,
            "session secret must be stable across hub restarts"
        );
    }

    // ── resolve_previous_session_secret (C5b, bd-6kll0jr6) ───────

    #[test]
    fn previous_secret_none_when_unconfigured() {
        let _guard = ENV_MUTEX.lock().unwrap();
        unsafe { std::env::remove_var("QUARTO_HUB_SESSION_SECRET_PREVIOUS") };
        unsafe { std::env::remove_var("QUARTO_HUB_SESSION_SECRET_ROTATED_AT") };

        let config = HubStorageConfig::new();
        assert_eq!(
            resolve_previous_session_secret(&config, 3600, 1_000_000).unwrap(),
            None
        );
    }

    #[test]
    fn previous_secret_from_config_inside_window() {
        let _guard = ENV_MUTEX.lock().unwrap();
        unsafe { std::env::remove_var("QUARTO_HUB_SESSION_SECRET_PREVIOUS") };

        let expected = [3u8; 32];
        let mut config = HubStorageConfig::new();
        config.previous_session_secret = Some(hex::encode(expected));
        config.session_secret_rotated_at = Some(1_000_000);

        // Inside the window (idle = 3600): still verifies.
        let got = resolve_previous_session_secret(&config, 3600, 1_000_000 + 3599).unwrap();
        assert_eq!(got, Some(expected));
    }

    #[test]
    fn previous_secret_dropped_after_window_lapses() {
        let _guard = ENV_MUTEX.lock().unwrap();
        unsafe { std::env::remove_var("QUARTO_HUB_SESSION_SECRET_PREVIOUS") };

        let mut config = HubStorageConfig::new();
        config.previous_session_secret = Some(hex::encode([3u8; 32]));
        config.session_secret_rotated_at = Some(1_000_000);

        // rotated_at + idle <= now → auto-dropped.
        let got = resolve_previous_session_secret(&config, 3600, 1_000_000 + 3600).unwrap();
        assert_eq!(got, None);
    }

    #[test]
    fn previous_secret_without_rotated_at_is_config_error() {
        let _guard = ENV_MUTEX.lock().unwrap();
        unsafe { std::env::remove_var("QUARTO_HUB_SESSION_SECRET_PREVIOUS") };

        let mut config = HubStorageConfig::new();
        config.previous_session_secret = Some(hex::encode([3u8; 32]));
        // No session_secret_rotated_at: an unbounded overlap window
        // would silently defeat the rotation.
        let result = resolve_previous_session_secret(&config, 3600, 1_000_000);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("session_secret_rotated_at")
        );
    }

    #[test]
    fn previous_secret_from_env_pair() {
        let _guard = ENV_MUTEX.lock().unwrap();

        let expected = [4u8; 32];
        unsafe { std::env::set_var("QUARTO_HUB_SESSION_SECRET_PREVIOUS", hex::encode(expected)) };
        unsafe { std::env::set_var("QUARTO_HUB_SESSION_SECRET_ROTATED_AT", "1000000") };
        let config = HubStorageConfig::new();
        let inside = resolve_previous_session_secret(&config, 3600, 1_000_000 + 10);
        let lapsed = resolve_previous_session_secret(&config, 3600, 1_000_000 + 3600);
        unsafe { std::env::remove_var("QUARTO_HUB_SESSION_SECRET_PREVIOUS") };
        unsafe { std::env::remove_var("QUARTO_HUB_SESSION_SECRET_ROTATED_AT") };

        assert_eq!(inside.unwrap(), Some(expected));
        assert_eq!(lapsed.unwrap(), None);
    }

    #[test]
    fn previous_secret_env_without_rotated_at_is_config_error() {
        let _guard = ENV_MUTEX.lock().unwrap();

        unsafe { std::env::set_var("QUARTO_HUB_SESSION_SECRET_PREVIOUS", hex::encode([4u8; 32])) };
        unsafe { std::env::remove_var("QUARTO_HUB_SESSION_SECRET_ROTATED_AT") };
        let config = HubStorageConfig::new();
        let result = resolve_previous_session_secret(&config, 3600, 1_000_000);
        unsafe { std::env::remove_var("QUARTO_HUB_SESSION_SECRET_PREVIOUS") };

        assert!(result.is_err());
    }

    // ── hub-dir .gitignore hygiene (C1, bd-sekcpmv1) ─────────────

    #[test]
    fn init_writes_catch_all_gitignore() {
        let temp = TempDir::new().unwrap();
        let manager = StorageManager::new(temp.path()).unwrap();

        let gitignore = manager.hub_dir().join(".gitignore");
        assert!(gitignore.exists(), ".gitignore created on fresh init");
        assert_eq!(fs::read_to_string(&gitignore).unwrap(), "*\n");
    }

    #[test]
    fn init_adds_gitignore_to_existing_hub_dir_lacking_it() {
        let temp = TempDir::new().unwrap();
        let hub_dir = temp.path().join(".quarto").join("hub");
        // Pre-existing hub dir from an older hub version: config file
        // present, no .gitignore.
        fs::create_dir_all(&hub_dir).unwrap();
        fs::write(
            hub_dir.join("hub.json"),
            r#"{"version": 1, "created_at": "123456"}"#,
        )
        .unwrap();

        let manager = StorageManager::new(temp.path()).unwrap();
        let gitignore = manager.hub_dir().join(".gitignore");
        assert!(gitignore.exists(), ".gitignore added to existing hub dir");
        assert_eq!(fs::read_to_string(&gitignore).unwrap(), "*\n");
    }

    #[test]
    fn init_preserves_user_modified_gitignore() {
        let temp = TempDir::new().unwrap();
        let hub_dir = temp.path().join(".quarto").join("hub");
        fs::create_dir_all(&hub_dir).unwrap();
        // An operator who deliberately narrowed the ignore keeps their
        // version — we only create, never overwrite.
        fs::write(hub_dir.join(".gitignore"), "hub.json\n").unwrap();

        let _manager = StorageManager::new(temp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(hub_dir.join(".gitignore")).unwrap(),
            "hub.json\n"
        );
    }

    #[test]
    fn resolve_secret_old_config_without_field_generates_new() {
        let _guard = ENV_MUTEX.lock().unwrap();

        let temp = TempDir::new().unwrap();
        let hub_dir = temp.path().join("hub");
        fs::create_dir_all(&hub_dir).unwrap();

        unsafe { std::env::remove_var("QUARTO_HUB_SERVER_SECRET") };

        // Write a config that lacks the server_secret field (old format)
        let old_config = r#"{"version": 1, "created_at": "123456"}"#;
        fs::write(hub_dir.join("hub.json"), old_config).unwrap();

        // Deserialize it — server_secret should be None
        let mut config: HubStorageConfig = serde_json::from_str(old_config).unwrap();
        assert!(config.server_secret.is_none());

        let secret = resolve_server_secret(&mut config, &hub_dir).unwrap();

        // Should have generated a new secret
        assert_eq!(secret.len(), 32);
        assert!(config.server_secret.is_some());
    }

    // ── auto-generation is loud (E3, bd-sx7k3vid) ─────────────────

    /// Run `f` under a scoped subscriber that captures formatted output,
    /// so we can assert on what an operator would actually see.
    fn capture_logs(f: impl FnOnce()) -> String {
        use std::sync::{Arc, Mutex};

        #[derive(Clone)]
        struct BufWriter(Arc<Mutex<Vec<u8>>>);
        impl Write for BufWriter {
            fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                self.0.lock().unwrap().extend_from_slice(buf);
                Ok(buf.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for BufWriter {
            type Writer = BufWriter;
            fn make_writer(&'a self) -> Self::Writer {
                self.clone()
            }
        }

        let buf = Arc::new(Mutex::new(Vec::new()));
        let subscriber = tracing_subscriber::fmt()
            .with_writer(BufWriter(buf.clone()))
            .with_ansi(false)
            .with_max_level(tracing::Level::TRACE)
            .finish();
        tracing::subscriber::with_default(subscriber, f);
        String::from_utf8(buf.lock().unwrap().clone()).unwrap()
    }

    fn fresh_hub_dir() -> (TempDir, PathBuf) {
        let temp = TempDir::new().unwrap();
        let hub_dir = temp.path().join("hub");
        fs::create_dir_all(&hub_dir).unwrap();
        (temp, hub_dir)
    }

    /// Two hubs that each auto-generate their own session secret reject
    /// each other's cookies and sealed login blobs — an intermittent,
    /// self-healing sign-in failure whose audit signature
    /// (`login_state_kid_mismatch`) gives no hint about the cause. The
    /// warning is the only thing that surfaces it at the moment the
    /// secret gets pinned to one data directory.
    #[test]
    fn resolve_session_secret_warns_when_it_generates_one() {
        let _guard = ENV_MUTEX.lock().unwrap();
        unsafe { std::env::remove_var("QUARTO_HUB_SESSION_SECRET") };

        let (_temp, hub_dir) = fresh_hub_dir();
        let mut config = HubStorageConfig::new();
        let logs = capture_logs(|| {
            resolve_session_secret(&mut config, &hub_dir).unwrap();
        });

        assert!(logs.contains("WARN"), "must be a warning: {logs}");
        assert!(
            logs.contains("QUARTO_HUB_SESSION_SECRET"),
            "must name the way out: {logs}"
        );
        assert!(
            logs.contains(&hub_dir.display().to_string()),
            "must name the directory the secret is now pinned to: {logs}"
        );

        // Token contents are never logged, and neither is this.
        let generated = config.session_secret.as_deref().unwrap();
        assert!(
            !logs.contains(generated),
            "the secret value must never reach the log"
        );
    }

    #[test]
    fn resolve_session_secret_is_silent_when_a_secret_is_configured() {
        let _guard = ENV_MUTEX.lock().unwrap();
        let (_temp, hub_dir) = fresh_hub_dir();

        // Source 1: the env var — the multi-instance mechanism itself.
        unsafe { std::env::set_var("QUARTO_HUB_SESSION_SECRET", hex::encode([3u8; 32])) };
        let mut config = HubStorageConfig::new();
        let env_logs = capture_logs(|| {
            resolve_session_secret(&mut config, &hub_dir).unwrap();
        });
        unsafe { std::env::remove_var("QUARTO_HUB_SESSION_SECRET") };

        // Source 2: an existing hub.json value — already pinned, nothing
        // new to warn about.
        let mut config = HubStorageConfig::new();
        config.session_secret = Some(hex::encode([5u8; 32]));
        let config_logs = capture_logs(|| {
            resolve_session_secret(&mut config, &hub_dir).unwrap();
        });

        assert!(!env_logs.contains("WARN"), "env branch: {env_logs}");
        assert!(
            !config_logs.contains("WARN"),
            "config branch: {config_logs}"
        );
    }

    #[test]
    fn resolve_server_secret_warns_when_it_generates_one() {
        let _guard = ENV_MUTEX.lock().unwrap();
        unsafe { std::env::remove_var("QUARTO_HUB_SERVER_SECRET") };

        let (_temp, hub_dir) = fresh_hub_dir();
        let mut config = HubStorageConfig::new();
        let logs = capture_logs(|| {
            resolve_server_secret(&mut config, &hub_dir).unwrap();
        });

        assert!(logs.contains("WARN"), "must be a warning: {logs}");
        assert!(
            logs.contains("QUARTO_HUB_SERVER_SECRET"),
            "must name the way out: {logs}"
        );

        let generated = config.server_secret.as_deref().unwrap();
        assert!(
            !logs.contains(generated),
            "the secret value must never reach the log"
        );
    }

    #[test]
    fn resolve_server_secret_is_silent_when_a_secret_is_configured() {
        let _guard = ENV_MUTEX.lock().unwrap();
        let (_temp, hub_dir) = fresh_hub_dir();

        unsafe { std::env::set_var("QUARTO_HUB_SERVER_SECRET", hex::encode([4u8; 32])) };
        let mut config = HubStorageConfig::new();
        let env_logs = capture_logs(|| {
            resolve_server_secret(&mut config, &hub_dir).unwrap();
        });
        unsafe { std::env::remove_var("QUARTO_HUB_SERVER_SECRET") };

        let mut config = HubStorageConfig::new();
        config.server_secret = Some(hex::encode([6u8; 32]));
        let config_logs = capture_logs(|| {
            resolve_server_secret(&mut config, &hub_dir).unwrap();
        });

        assert!(!env_logs.contains("WARN"), "env branch: {env_logs}");
        assert!(
            !config_logs.contains("WARN"),
            "config branch: {config_logs}"
        );
    }

    // ── ephemeral secret policy (bd-tp1l6a0w) ─────────────────────
    //
    // Short-lived embedded hubs (`q2 preview`) resolve secrets per
    // process: never persisted to `hub.json`, never warned about. The
    // multi-instance warning only makes sense for secrets pinned to a
    // data directory.

    #[test]
    fn ephemeral_secrets_are_not_persisted_and_do_not_warn() {
        let _guard = ENV_MUTEX.lock().unwrap();
        // SAFETY: test-only env mutation, serialized by ENV_MUTEX.
        unsafe { std::env::remove_var("QUARTO_HUB_SERVER_SECRET") };
        unsafe { std::env::remove_var("QUARTO_HUB_SESSION_SECRET") };

        let (_temp, hub_dir) = fresh_hub_dir();
        let logs = capture_logs(|| {
            let manager = StorageManager::new_standalone_ephemeral(&hub_dir).unwrap();
            assert_eq!(manager.server_secret().len(), 32);
            assert_eq!(manager.session_secret().len(), 32);
        });

        assert!(
            !logs.contains("WARN"),
            "ephemeral mode must not warn: {logs}"
        );

        // hub.json exists (load_or_create writes it) but carries no secrets.
        let content = fs::read_to_string(hub_dir.join("hub.json")).unwrap();
        let on_disk: HubStorageConfig = serde_json::from_str(&content).unwrap();
        assert!(on_disk.server_secret.is_none());
        assert!(on_disk.session_secret.is_none());
    }

    #[test]
    fn ephemeral_secrets_are_distinct() {
        let _guard = ENV_MUTEX.lock().unwrap();
        // SAFETY: test-only env mutation, serialized by ENV_MUTEX.
        unsafe { std::env::remove_var("QUARTO_HUB_SERVER_SECRET") };
        unsafe { std::env::remove_var("QUARTO_HUB_SESSION_SECRET") };

        let (_temp, hub_dir) = fresh_hub_dir();
        let manager = StorageManager::new_standalone_ephemeral(&hub_dir).unwrap();
        assert_ne!(
            manager.server_secret(),
            manager.session_secret().as_slice(),
            "session secret must never equal the actor-id server secret"
        );
    }

    #[test]
    fn ephemeral_respects_env_override() {
        let _guard = ENV_MUTEX.lock().unwrap();

        let expected = [17u8; 32];
        let hex = hex::encode(expected);
        // SAFETY: test-only env mutation, serialized by ENV_MUTEX.
        unsafe { std::env::set_var("QUARTO_HUB_SERVER_SECRET", &hex) };

        let (_temp, hub_dir) = fresh_hub_dir();
        let logs = capture_logs(|| {
            let manager = StorageManager::new_standalone_ephemeral(&hub_dir).unwrap();
            assert_eq!(manager.server_secret(), expected.as_slice());
        });
        unsafe { std::env::remove_var("QUARTO_HUB_SERVER_SECRET") };

        assert!(!logs.contains("WARN"), "env override must not warn: {logs}");
        let content = fs::read_to_string(hub_dir.join("hub.json")).unwrap();
        let on_disk: HubStorageConfig = serde_json::from_str(&content).unwrap();
        assert!(
            on_disk.server_secret.is_none(),
            "an env-provided secret must not be persisted"
        );
    }

    #[test]
    fn ephemeral_generates_fresh_secrets_each_boot() {
        let _guard = ENV_MUTEX.lock().unwrap();
        // SAFETY: test-only env mutation, serialized by ENV_MUTEX.
        unsafe { std::env::remove_var("QUARTO_HUB_SERVER_SECRET") };
        unsafe { std::env::remove_var("QUARTO_HUB_SESSION_SECRET") };

        let (_temp, hub_dir) = fresh_hub_dir();

        let (first_session, first_server) = {
            let manager = StorageManager::new_standalone_ephemeral(&hub_dir).unwrap();
            (*manager.session_secret(), manager.server_secret().to_vec())
        }; // manager dropped → lock released

        let manager = StorageManager::new_standalone_ephemeral(&hub_dir).unwrap();
        assert_ne!(first_session, *manager.session_secret());
        assert_ne!(first_server.as_slice(), manager.server_secret());

        let content = fs::read_to_string(hub_dir.join("hub.json")).unwrap();
        let on_disk: HubStorageConfig = serde_json::from_str(&content).unwrap();
        assert!(on_disk.server_secret.is_none());
        assert!(on_disk.session_secret.is_none());
    }
}
