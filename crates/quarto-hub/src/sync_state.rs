//! Sync state management for filesystem synchronization
//!
//! Tracks per-document sync checkpoints (heads and content hash) in a local-only
//! file at `.quarto/hub/sync-state.json`. This state is per-machine and not synced
//! via automerge because "when did THIS hub instance last sync to THIS filesystem"
//! is inherently local information.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use automerge::ChangeHash;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tracing::{debug, warn};

use crate::error::{Error, Result};

/// Sync checkpoint for a single document.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentSyncCheckpoint {
    /// Automerge heads at the time of last sync.
    /// These are hex-encoded change hashes.
    pub last_sync_heads: Vec<String>,

    /// SHA-256 hash of the file content at last sync.
    /// Format: "sha256:<hex-digest>"
    pub last_sync_content_hash: String,
}

/// Persistent sync state for all documents.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SyncStateData {
    /// Map from document ID (bs58-encoded) to sync checkpoint.
    pub documents: HashMap<String, DocumentSyncCheckpoint>,
}

/// Manager for sync state, handling persistence with atomic writes.
pub struct SyncState {
    /// Path to the hub directory (`.quarto/hub/`)
    hub_dir: PathBuf,

    /// In-memory sync state data
    data: SyncStateData,
}

impl SyncState {
    /// Load sync state from disk, or create empty state if file doesn't exist.
    pub fn load(hub_dir: &Path) -> Result<Self> {
        let state_path = hub_dir.join("sync-state.json");

        let data = if state_path.exists() {
            let content = std::fs::read_to_string(&state_path)?;
            match serde_json::from_str(&content) {
                Ok(data) => {
                    debug!("Loaded sync state from {}", state_path.display());
                    data
                }
                Err(e) => {
                    warn!("Failed to parse sync-state.json: {}. Starting fresh.", e);
                    SyncStateData::default()
                }
            }
        } else {
            debug!("No sync-state.json found, starting with empty state");
            SyncStateData::default()
        };

        Ok(Self {
            hub_dir: hub_dir.to_path_buf(),
            data,
        })
    }

    /// Save sync state to disk using atomic write (write to temp, then rename).
    pub fn save(&self) -> Result<()> {
        let target = self.hub_dir.join("sync-state.json");
        let temp = self.hub_dir.join("sync-state.json.tmp");

        // Serialize to pretty JSON
        let content = serde_json::to_string_pretty(&self.data)
            .map_err(|e| Error::SyncState(format!("failed to serialize sync state: {}", e)))?;

        // Write to temp file
        std::fs::write(&temp, &content)?;

        // Atomic rename
        std::fs::rename(&temp, &target)?;

        debug!("Saved sync state to {}", target.display());
        Ok(())
    }

    /// Get the sync checkpoint for a document, if one exists.
    pub fn get_checkpoint(&self, doc_id: &str) -> Option<&DocumentSyncCheckpoint> {
        self.data.documents.get(doc_id)
    }

    /// Get the last sync heads for a document, parsed as ChangeHashes.
    /// Returns None if no checkpoint exists or if heads can't be parsed.
    pub fn get_heads(&self, doc_id: &str) -> Option<Vec<ChangeHash>> {
        let checkpoint = self.data.documents.get(doc_id)?;
        parse_heads(&checkpoint.last_sync_heads)
    }

    /// Get the last sync content hash for a document.
    pub fn get_content_hash(&self, doc_id: &str) -> Option<&str> {
        self.data
            .documents
            .get(doc_id)
            .map(|c| c.last_sync_content_hash.as_str())
    }

    /// Update the sync checkpoint for a document.
    pub fn set_checkpoint(&mut self, doc_id: &str, heads: &[ChangeHash], content_hash: &str) {
        let checkpoint = DocumentSyncCheckpoint {
            last_sync_heads: heads.iter().map(|h| format!("{}", h)).collect(),
            last_sync_content_hash: content_hash.to_string(),
        };
        self.data.documents.insert(doc_id.to_string(), checkpoint);
    }

    /// Remove the sync checkpoint for a document (e.g., when file is deleted).
    pub fn remove_checkpoint(&mut self, doc_id: &str) {
        self.data.documents.remove(doc_id);
    }

    /// Check if a document has a sync checkpoint.
    pub fn has_checkpoint(&self, doc_id: &str) -> bool {
        self.data.documents.contains_key(doc_id)
    }

    /// Get a reference to all document checkpoints.
    pub fn all_checkpoints(&self) -> &HashMap<String, DocumentSyncCheckpoint> {
        &self.data.documents
    }
}

/// Compute SHA-256 hash of content, returning "sha256:<hex-digest>" format.
pub fn sha256_hash(content: &str) -> String {
    let digest = Sha256::digest(content.as_bytes());
    format!("sha256:{:x}", digest)
}

/// Parse hex-encoded heads strings back to ChangeHashes.
/// Returns None if any head fails to parse.
fn parse_heads(heads: &[String]) -> Option<Vec<ChangeHash>> {
    let mut result = Vec::with_capacity(heads.len());
    for head_str in heads {
        // ChangeHash Display format is hex, so we parse hex
        let bytes = hex::decode(head_str).ok()?;
        if bytes.len() != 32 {
            return None;
        }
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&bytes);
        result.push(ChangeHash(arr));
    }
    Some(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_sha256_hash() {
        let hash = sha256_hash("Hello, world!");
        assert!(hash.starts_with("sha256:"));
        // Known SHA-256 hash of "Hello, world!"
        assert_eq!(
            hash,
            "sha256:315f5bdb76d078c43b8ac0064e4a0164612b1fce77c869345bfc94c75894edd3"
        );
    }

    #[test]
    fn test_sync_state_empty() {
        let temp = TempDir::new().unwrap();
        let state = SyncState::load(temp.path()).unwrap();

        assert!(state.all_checkpoints().is_empty());
        assert!(state.get_checkpoint("doc-123").is_none());
        assert!(state.get_heads("doc-123").is_none());
    }

    #[test]
    fn test_sync_state_set_and_get() {
        let temp = TempDir::new().unwrap();
        let mut state = SyncState::load(temp.path()).unwrap();

        // Create some fake heads
        let heads = vec![ChangeHash([1u8; 32]), ChangeHash([2u8; 32])];
        let content_hash = sha256_hash("Test content");

        state.set_checkpoint("doc-123", &heads, &content_hash);

        // Verify checkpoint was set
        assert!(state.has_checkpoint("doc-123"));

        let checkpoint = state.get_checkpoint("doc-123").unwrap();
        assert_eq!(checkpoint.last_sync_heads.len(), 2);
        assert_eq!(checkpoint.last_sync_content_hash, content_hash);

        // Verify heads can be parsed back
        let parsed_heads = state.get_heads("doc-123").unwrap();
        assert_eq!(parsed_heads.len(), 2);
        assert_eq!(parsed_heads[0], heads[0]);
        assert_eq!(parsed_heads[1], heads[1]);
    }

    #[test]
    fn test_sync_state_persistence() {
        let temp = TempDir::new().unwrap();

        // Create and populate state
        {
            let mut state = SyncState::load(temp.path()).unwrap();
            let heads = vec![ChangeHash([42u8; 32])];
            state.set_checkpoint("doc-abc", &heads, "sha256:test");
            state.save().unwrap();
        }

        // Load state again and verify
        {
            let state = SyncState::load(temp.path()).unwrap();
            assert!(state.has_checkpoint("doc-abc"));
            let checkpoint = state.get_checkpoint("doc-abc").unwrap();
            assert_eq!(checkpoint.last_sync_content_hash, "sha256:test");
        }
    }

    #[test]
    fn test_sync_state_remove() {
        let temp = TempDir::new().unwrap();
        let mut state = SyncState::load(temp.path()).unwrap();

        let heads = vec![ChangeHash([1u8; 32])];
        state.set_checkpoint("doc-123", &heads, "sha256:test");
        assert!(state.has_checkpoint("doc-123"));

        state.remove_checkpoint("doc-123");
        assert!(!state.has_checkpoint("doc-123"));
    }

    #[test]
    fn test_sync_state_atomic_write() {
        let temp = TempDir::new().unwrap();
        let mut state = SyncState::load(temp.path()).unwrap();

        let heads = vec![ChangeHash([1u8; 32])];
        state.set_checkpoint("doc-123", &heads, "sha256:test");
        state.save().unwrap();

        // Verify temp file doesn't exist (was renamed)
        assert!(!temp.path().join("sync-state.json.tmp").exists());
        // Verify final file exists
        assert!(temp.path().join("sync-state.json").exists());
    }

    #[test]
    fn test_parse_heads_roundtrip() {
        let original = vec![ChangeHash([0xab; 32]), ChangeHash([0xcd; 32])];

        // Convert to strings (like in serialization)
        let strings: Vec<String> = original.iter().map(|h| format!("{}", h)).collect();

        // Parse back
        let parsed = parse_heads(&strings).unwrap();

        assert_eq!(original, parsed);
    }

    #[test]
    fn test_corrupted_sync_state_recovery() {
        let temp = TempDir::new().unwrap();

        // Write corrupted JSON
        std::fs::write(temp.path().join("sync-state.json"), "not valid json").unwrap();

        // Should recover gracefully with empty state
        let state = SyncState::load(temp.path()).unwrap();
        assert!(state.all_checkpoints().is_empty());
    }
}
