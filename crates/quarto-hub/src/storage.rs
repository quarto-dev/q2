//! Storage management for the hub
//!
//! Manages the `.quarto/hub/` directory structure and lockfile.

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

/// Hub configuration stored in `.quarto/hub/hub.json`.
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
    fn save(&self, hub_dir: &Path) -> Result<()> {
        let config_path = hub_dir.join("hub.json");
        let content = serde_json::to_string_pretty(self)
            .map_err(|e| Error::ConfigParse(e.to_string()))?;
        fs::write(&config_path, content)?;
        Ok(())
    }
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

/// Manages the `.quarto/hub/` directory and holds the lockfile.
///
/// The lockfile is held for the lifetime of this struct, preventing
/// multiple hub instances from running on the same project.
pub struct StorageManager {
    /// Root of the Quarto project
    project_root: PathBuf,

    /// Path to `.quarto/hub/`
    hub_dir: PathBuf,

    /// Open lockfile (lock released on drop)
    #[allow(dead_code)]
    lock_file: File,

    /// Hub storage configuration (version, timestamps)
    config: HubStorageConfig,
}

impl StorageManager {
    /// Create a new StorageManager for the given project root.
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
        let config = HubStorageConfig::load_or_create(&hub_dir)?;

        info!(
            project_root = %project_root.display(),
            hub_dir = %hub_dir.display(),
            version = config.version,
            "Storage manager initialized"
        );

        Ok(Self {
            project_root,
            hub_dir,
            lock_file,
            config,
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

    /// Returns the project root directory.
    pub fn project_root(&self) -> &Path {
        &self.project_root
    }

    /// Returns the hub directory (`.quarto/hub/`).
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
}
