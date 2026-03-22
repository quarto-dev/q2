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
    use rand::RngCore;
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    config.server_secret = Some(hex::encode(bytes));
    config.save(hub_dir)?;
    Ok(bytes)
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

        Self::init(Some(project_root), hub_dir)
    }

    /// Create a StorageManager for standalone mode (no local project).
    ///
    /// Storage is placed directly in `data_dir`. This mode is used when
    /// the hub acts as a pure sync server without watching any local files.
    ///
    /// The directory will be created if it doesn't exist.
    pub fn new_standalone(data_dir: impl AsRef<Path>) -> Result<Self> {
        let hub_dir = data_dir.as_ref().to_path_buf();

        Self::init(None, hub_dir)
    }

    /// Shared initialization logic for both project and standalone modes.
    fn init(project_root: Option<PathBuf>, hub_dir: PathBuf) -> Result<Self> {
        fs::create_dir_all(&hub_dir).map_err(Error::CreateHubDir)?;

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

        // Resolve and cache the server secret for HMAC actor ID derivation.
        let server_secret = resolve_server_secret(&mut config, &hub_dir)?;

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
}
