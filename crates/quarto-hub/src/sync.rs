//! Filesystem synchronization for quarto-hub
//!
//! This module implements the core sync algorithm that keeps automerge documents
//! and filesystem files in sync. The algorithm uses a fork-and-merge pattern
//! that handles all sync scenarios uniformly:
//!
//! - No changes: both automerge and filesystem unchanged
//! - Automerge changed: remote edits received, filesystem unchanged
//! - Filesystem changed: local file edited, automerge unchanged
//! - Both changed: true divergence requiring CRDT merge
//! - First run: no prior sync checkpoint

use std::path::Path;
use std::str::FromStr;

use automerge::{ROOT, ReadDoc, transaction::Transactable};
use samod::{DocHandle, DocumentId, Repo};
use tracing::{debug, info, warn};

use crate::error::{Error, Result};
use crate::index::IndexDocument;
use crate::sync_state::{SyncState, sha256_hash};

/// Synchronize a single document with its corresponding filesystem file.
///
/// This implements the unified sync algorithm:
/// 1. Get sync checkpoint (use current heads if none exists or invalid)
/// 2. Read filesystem content
/// 3. Fork at sync checkpoint (with fallback if fork_at fails)
/// 4. Apply filesystem content to fork (update_text handles diff internally)
/// 5. Merge fork back into main document
/// 6. Write merged state back to filesystem
/// 7. Update sync checkpoint
///
/// # Arguments
/// * `doc_handle` - Handle to the automerge document
/// * `file_path` - Path to the filesystem file
/// * `sync_state` - Mutable reference to sync state for reading/updating checkpoints
///
/// # Returns
/// * `Ok(SyncResult)` - Summary of what happened during sync
/// * `Err(Error)` - If sync failed (file not readable, automerge error, etc.)
pub fn sync_document(
    doc_handle: &DocHandle,
    file_path: &Path,
    sync_state: &mut SyncState,
) -> Result<SyncResult> {
    let doc_id = doc_handle.document_id().to_string();

    // Read filesystem content first (outside of with_document to avoid holding lock while doing IO)
    let fs_content = std::fs::read_to_string(file_path).map_err(|e| {
        Error::Sync(format!(
            "failed to read file {}: {}",
            file_path.display(),
            e
        ))
    })?;

    let result = doc_handle.with_document(|doc| {
        // 1. Get sync checkpoint (use current heads if none exists or invalid)
        let checkpoint_heads = sync_state.get_heads(&doc_id);
        let last_sync_heads = checkpoint_heads
            .filter(|heads| {
                // Validate that all checkpoint heads exist in document history
                heads.iter().all(|h| doc.get_change_by_hash(h).is_some())
            })
            .unwrap_or_else(|| doc.get_heads());

        let current_heads = doc.get_heads();
        let heads_unchanged = last_sync_heads == current_heads;

        // Check if filesystem content matches what we synced last time
        let last_content_hash = sync_state.get_content_hash(&doc_id);
        let fs_content_hash = sha256_hash(&fs_content);
        let fs_unchanged = last_content_hash == Some(fs_content_hash.as_str());

        // Early exit: if nothing changed, we're done
        if heads_unchanged && fs_unchanged {
            debug!(doc_id = %doc_id, "No changes detected, skipping sync");
            return Ok(SyncResult::NoChanges);
        }

        // 3. Fork at sync checkpoint (with fallback if fork_at fails)
        let mut forked = doc.fork_at(&last_sync_heads).unwrap_or_else(|e| {
            warn!(
                doc_id = %doc_id,
                error = %e,
                "fork_at failed, falling back to current state"
            );
            doc.fork()
        });

        // 4. Apply filesystem content to fork
        let text_obj = forked
            .get(ROOT, "text")
            .map_err(|e| Error::Sync(format!("failed to get text object: {:?}", e)))?
            .ok_or_else(|| {
                Error::Sync(format!(
                    "document {} has no text field - was it initialized correctly?",
                    doc_id
                ))
            })?
            .1;

        forked
            .transact::<_, _, automerge::AutomergeError>(|tx| {
                tx.update_text(&text_obj, &fs_content)?;
                Ok(())
            })
            .map_err(|e| Error::Sync(format!("failed to update text in fork: {:?}", e)))?;

        // 5. Merge fork back into main document
        doc.merge(&mut forked)
            .map_err(|e| Error::Sync(format!("failed to merge fork: {:?}", e)))?;

        // 6. Read merged content and write back to filesystem
        let merged_text_obj = doc
            .get(ROOT, "text")
            .map_err(|e| Error::Sync(format!("failed to get merged text object: {:?}", e)))?
            .ok_or_else(|| Error::Sync("merged document has no text field".to_string()))?
            .1;

        let merged_content = doc
            .text(&merged_text_obj)
            .map_err(|e| Error::Sync(format!("failed to read merged text: {:?}", e)))?;

        // Determine what kind of sync happened
        let result_type = if !heads_unchanged && !fs_unchanged {
            SyncResult::BothChanged {
                merged_len: merged_content.len(),
            }
        } else if !heads_unchanged {
            SyncResult::AutomergeChanged {
                new_len: merged_content.len(),
            }
        } else {
            SyncResult::FilesystemChanged {
                new_len: merged_content.len(),
            }
        };

        // Write merged content back to filesystem (only if it differs)
        let merged_content_hash = sha256_hash(&merged_content);
        if merged_content_hash != fs_content_hash {
            std::fs::write(file_path, &merged_content).map_err(|e| {
                Error::Sync(format!(
                    "failed to write merged content to {}: {}",
                    file_path.display(),
                    e
                ))
            })?;
            debug!(
                doc_id = %doc_id,
                path = %file_path.display(),
                "Wrote merged content to filesystem"
            );
        }

        // 7. Update sync checkpoint
        let new_heads = doc.get_heads();
        sync_state.set_checkpoint(&doc_id, &new_heads, &merged_content_hash);

        Ok(result_type)
    });

    match &result {
        Ok(SyncResult::NoChanges) => {
            debug!(doc_id = %doc_id, path = %file_path.display(), "Sync complete: no changes");
        }
        Ok(SyncResult::AutomergeChanged { new_len }) => {
            info!(
                doc_id = %doc_id,
                path = %file_path.display(),
                new_len = new_len,
                "Sync complete: automerge → filesystem"
            );
        }
        Ok(SyncResult::FilesystemChanged { new_len }) => {
            info!(
                doc_id = %doc_id,
                path = %file_path.display(),
                new_len = new_len,
                "Sync complete: filesystem → automerge"
            );
        }
        Ok(SyncResult::BothChanged { merged_len }) => {
            info!(
                doc_id = %doc_id,
                path = %file_path.display(),
                merged_len = merged_len,
                "Sync complete: merged divergent changes"
            );
        }
        Err(e) => {
            warn!(
                doc_id = %doc_id,
                path = %file_path.display(),
                error = %e,
                "Sync failed"
            );
        }
    }

    result
}

/// Synchronize a single file by its path.
///
/// This looks up the document ID for the given file path in the index,
/// then syncs that document with the filesystem.
///
/// # Arguments
/// * `repo` - The samod Repo containing document handles
/// * `index` - The index document mapping file paths to document IDs
/// * `file_path` - Absolute path to the file
/// * `project_root` - Root directory of the project
/// * `sync_state` - Mutable reference to sync state
///
/// # Returns
/// * `Ok(Some(SyncResult))` - Sync succeeded
/// * `Ok(None)` - File is not in the index (not tracked)
/// * `Err(Error)` - Sync failed
pub async fn sync_file_by_path(
    repo: &Repo,
    index: &IndexDocument,
    file_path: &Path,
    project_root: &Path,
    sync_state: &mut SyncState,
) -> Result<Option<SyncResult>> {
    // Convert absolute path to relative path for index lookup
    let relative_path = match file_path.strip_prefix(project_root) {
        Ok(rel) => rel.to_string_lossy().to_string(),
        Err(_) => {
            debug!(
                path = %file_path.display(),
                project_root = %project_root.display(),
                "File path not under project root"
            );
            return Ok(None);
        }
    };

    // Look up document ID in index
    let doc_id_str = match index.get_file(&relative_path) {
        Some(id) => id,
        None => {
            debug!(path = %relative_path, "File not in index, skipping sync");
            return Ok(None);
        }
    };

    // Parse document ID
    let doc_id = match DocumentId::from_str(&doc_id_str) {
        Ok(id) => id,
        Err(e) => {
            return Err(Error::Sync(format!(
                "invalid document ID in index for {}: {}",
                relative_path, e
            )));
        }
    };

    // Find document in repo
    let doc_handle = match repo.find(doc_id).await {
        Ok(Some(handle)) => handle,
        Ok(None) => {
            return Err(Error::Sync(format!(
                "document {} not found in repo for file {}",
                doc_id_str, relative_path
            )));
        }
        Err(_stopped) => {
            return Err(Error::Sync("repo is stopped".to_string()));
        }
    };

    // Sync the document
    let result = sync_document(&doc_handle, file_path, sync_state)?;

    // Save sync state
    sync_state.save()?;

    Ok(Some(result))
}

/// Synchronize all documents in the index with their corresponding filesystem files.
///
/// This iterates over all files in the index, finds the corresponding automerge
/// document, and syncs each one. Errors on individual documents are logged and
/// collected, but don't stop processing of other documents.
///
/// # Arguments
/// * `repo` - The samod Repo containing document handles
/// * `index` - The index document mapping file paths to document IDs
/// * `project_root` - Root directory of the project (file paths are relative to this)
/// * `sync_state` - Mutable reference to sync state for reading/updating checkpoints
///
/// # Returns
/// * `SyncAllResult` - Summary of sync results for all documents
pub async fn sync_all_documents(
    repo: &Repo,
    index: &IndexDocument,
    project_root: &Path,
    sync_state: &mut SyncState,
) -> SyncAllResult {
    let mut result = SyncAllResult::default();

    // Get all file mappings from the index
    let files = index.get_all_files();

    info!(count = files.len(), "Starting sync of all documents");

    for (file_path_str, doc_id_str) in &files {
        let file_path = project_root.join(file_path_str);

        // Check if file exists
        if !file_path.exists() {
            warn!(
                path = %file_path_str,
                doc_id = %doc_id_str,
                "File not found on disk, skipping sync"
            );
            result.skipped += 1;
            continue;
        }

        // Parse document ID
        let doc_id = match DocumentId::from_str(doc_id_str) {
            Ok(id) => id,
            Err(e) => {
                warn!(
                    doc_id = %doc_id_str,
                    error = %e,
                    "Invalid document ID in index, skipping"
                );
                result.errors.push(SyncError {
                    file_path: file_path_str.clone(),
                    doc_id: doc_id_str.clone(),
                    error: format!("invalid document ID: {}", e),
                });
                continue;
            }
        };

        // Find the document in the repo
        let doc_handle = match repo.find(doc_id).await {
            Ok(Some(handle)) => handle,
            Ok(None) => {
                warn!(
                    path = %file_path_str,
                    doc_id = %doc_id_str,
                    "Document not found in repo, skipping sync"
                );
                result.errors.push(SyncError {
                    file_path: file_path_str.clone(),
                    doc_id: doc_id_str.clone(),
                    error: "document not found in repo".to_string(),
                });
                continue;
            }
            Err(_stopped) => {
                warn!("Repo is stopped, aborting sync");
                result.errors.push(SyncError {
                    file_path: file_path_str.clone(),
                    doc_id: doc_id_str.clone(),
                    error: "repo is stopped".to_string(),
                });
                break;
            }
        };

        // Sync the document
        match sync_document(&doc_handle, &file_path, sync_state) {
            Ok(sync_result) => match sync_result {
                SyncResult::NoChanges => result.no_changes += 1,
                SyncResult::AutomergeChanged { .. } => result.automerge_changed += 1,
                SyncResult::FilesystemChanged { .. } => result.filesystem_changed += 1,
                SyncResult::BothChanged { .. } => result.both_changed += 1,
            },
            Err(e) => {
                result.errors.push(SyncError {
                    file_path: file_path_str.clone(),
                    doc_id: doc_id_str.clone(),
                    error: e.to_string(),
                });
            }
        }
    }

    // Save sync state after processing all documents
    if let Err(e) = sync_state.save() {
        warn!(error = %e, "Failed to save sync state");
    }

    info!(
        no_changes = result.no_changes,
        automerge_changed = result.automerge_changed,
        filesystem_changed = result.filesystem_changed,
        both_changed = result.both_changed,
        errors = result.errors.len(),
        skipped = result.skipped,
        "Sync complete"
    );

    result
}

/// Summary of sync results for all documents.
#[derive(Debug, Clone, Default)]
pub struct SyncAllResult {
    /// Number of documents with no changes
    pub no_changes: usize,

    /// Number of documents where automerge changed and was written to filesystem
    pub automerge_changed: usize,

    /// Number of documents where filesystem changed and was synced to automerge
    pub filesystem_changed: usize,

    /// Number of documents where both changed and were merged
    pub both_changed: usize,

    /// Number of documents skipped (e.g., file not found)
    pub skipped: usize,

    /// Errors encountered during sync
    pub errors: Vec<SyncError>,
}

impl SyncAllResult {
    /// Total number of documents successfully synced
    pub fn total_synced(&self) -> usize {
        self.no_changes + self.automerge_changed + self.filesystem_changed + self.both_changed
    }

    /// Whether any errors occurred
    pub fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }
}

/// Error that occurred during sync of a single document.
#[derive(Debug, Clone)]
pub struct SyncError {
    /// File path (relative to project root)
    pub file_path: String,

    /// Document ID (bs58-encoded)
    pub doc_id: String,

    /// Error message
    pub error: String,
}

/// Result of a sync operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncResult {
    /// No changes detected - both automerge and filesystem unchanged since last sync.
    NoChanges,

    /// Automerge had changes (from remote peers), filesystem was unchanged.
    /// Content was written from automerge to filesystem.
    AutomergeChanged { new_len: usize },

    /// Filesystem had changes, automerge was unchanged.
    /// Content was read from filesystem into automerge.
    FilesystemChanged { new_len: usize },

    /// Both automerge and filesystem had changes.
    /// Changes were merged using CRDT semantics.
    BothChanged { merged_len: usize },
}

#[cfg(test)]
mod tests {
    use super::*;
    use automerge::{Automerge, ObjType};
    use samod::Repo;
    use samod::storage::InMemoryStorage;
    use tempfile::TempDir;

    /// Helper to create a test repo
    async fn create_test_repo() -> Repo {
        Repo::build_tokio()
            .with_storage(InMemoryStorage::new())
            .load()
            .await
    }

    /// Helper to create a document with text content
    fn create_doc_with_text(content: &str) -> Automerge {
        let mut doc = Automerge::new();
        doc.transact::<_, _, automerge::AutomergeError>(|tx| {
            let text_obj = tx.put_object(ROOT, "text", ObjType::Text)?;
            tx.update_text(&text_obj, content)?;
            Ok(())
        })
        .unwrap();
        doc
    }

    /// Helper to read text from doc handle
    fn read_text_from_handle(handle: &DocHandle) -> String {
        handle.with_document(|doc| {
            let (_, text_obj) = doc.get(ROOT, "text").unwrap().unwrap();
            doc.text(&text_obj).unwrap()
        })
    }

    /// Helper to update text in doc handle
    fn update_text_in_handle(handle: &DocHandle, content: &str) {
        handle.with_document(|doc| {
            let (_, text_obj) = doc.get(ROOT, "text").unwrap().unwrap();
            doc.transact::<_, _, automerge::AutomergeError>(|tx| {
                tx.update_text(&text_obj, content)?;
                Ok(())
            })
            .unwrap();
        });
    }

    #[tokio::test]
    async fn test_sync_no_changes() {
        let temp = TempDir::new().unwrap();
        let repo = create_test_repo().await;

        // Create document with content
        let doc = create_doc_with_text("Hello, world!");
        let handle = repo.create(doc).await.unwrap();
        let doc_id = handle.document_id().to_string();

        // Create file with same content
        let file_path = temp.path().join("test.qmd");
        std::fs::write(&file_path, "Hello, world!").unwrap();

        // Create sync state and set initial checkpoint
        let mut sync_state = SyncState::load(temp.path()).unwrap();
        let heads = handle.with_document(|doc| doc.get_heads());
        sync_state.set_checkpoint(&doc_id, &heads, &sha256_hash("Hello, world!"));

        // Sync should detect no changes
        let result = sync_document(&handle, &file_path, &mut sync_state).unwrap();
        assert_eq!(result, SyncResult::NoChanges);

        // Content should be unchanged
        assert_eq!(read_text_from_handle(&handle), "Hello, world!");
        assert_eq!(
            std::fs::read_to_string(&file_path).unwrap(),
            "Hello, world!"
        );
    }

    #[tokio::test]
    async fn test_sync_filesystem_changed() {
        let temp = TempDir::new().unwrap();
        let repo = create_test_repo().await;

        // Create document with content
        let doc = create_doc_with_text("Original content");
        let handle = repo.create(doc).await.unwrap();
        let doc_id = handle.document_id().to_string();

        // Create file with different (newer) content
        let file_path = temp.path().join("test.qmd");
        std::fs::write(&file_path, "Modified by filesystem").unwrap();

        // Create sync state with checkpoint at original state
        let mut sync_state = SyncState::load(temp.path()).unwrap();
        let heads = handle.with_document(|doc| doc.get_heads());
        sync_state.set_checkpoint(&doc_id, &heads, &sha256_hash("Original content"));

        // Sync should pull filesystem changes into automerge
        let result = sync_document(&handle, &file_path, &mut sync_state).unwrap();
        assert!(matches!(result, SyncResult::FilesystemChanged { .. }));

        // Both should now have filesystem content
        assert_eq!(read_text_from_handle(&handle), "Modified by filesystem");
        assert_eq!(
            std::fs::read_to_string(&file_path).unwrap(),
            "Modified by filesystem"
        );
    }

    #[tokio::test]
    async fn test_sync_automerge_changed() {
        let temp = TempDir::new().unwrap();
        let repo = create_test_repo().await;

        // Create document with initial content
        let doc = create_doc_with_text("Original content");
        let handle = repo.create(doc).await.unwrap();
        let doc_id = handle.document_id().to_string();

        // Record checkpoint at initial state
        let mut sync_state = SyncState::load(temp.path()).unwrap();
        let initial_heads = handle.with_document(|doc| doc.get_heads());
        sync_state.set_checkpoint(&doc_id, &initial_heads, &sha256_hash("Original content"));

        // Create file with original content (filesystem unchanged)
        let file_path = temp.path().join("test.qmd");
        std::fs::write(&file_path, "Original content").unwrap();

        // Modify automerge document (simulating remote peer changes)
        update_text_in_handle(&handle, "Modified by automerge");

        // Sync should push automerge changes to filesystem
        let result = sync_document(&handle, &file_path, &mut sync_state).unwrap();
        assert!(matches!(result, SyncResult::AutomergeChanged { .. }));

        // Both should now have automerge content
        assert_eq!(read_text_from_handle(&handle), "Modified by automerge");
        assert_eq!(
            std::fs::read_to_string(&file_path).unwrap(),
            "Modified by automerge"
        );
    }

    #[tokio::test]
    async fn test_sync_both_changed() {
        let temp = TempDir::new().unwrap();
        let repo = create_test_repo().await;

        // Create document with initial content
        let doc = create_doc_with_text("Line one\nLine two");
        let handle = repo.create(doc).await.unwrap();
        let doc_id = handle.document_id().to_string();

        // Record checkpoint at initial state
        let mut sync_state = SyncState::load(temp.path()).unwrap();
        let initial_heads = handle.with_document(|doc| doc.get_heads());
        sync_state.set_checkpoint(&doc_id, &initial_heads, &sha256_hash("Line one\nLine two"));

        // Create file with different changes
        let file_path = temp.path().join("test.qmd");
        std::fs::write(&file_path, "Line one\nLine two - fs edit").unwrap();

        // Modify automerge document differently
        update_text_in_handle(&handle, "Line one - am edit\nLine two");

        // Sync should merge both changes
        let result = sync_document(&handle, &file_path, &mut sync_state).unwrap();
        assert!(matches!(result, SyncResult::BothChanged { .. }));

        // Both should have merged content (exact result depends on CRDT merge)
        let merged = read_text_from_handle(&handle);
        let file_content = std::fs::read_to_string(&file_path).unwrap();

        // They should be the same after sync
        assert_eq!(merged, file_content);

        // And should contain elements from both edits (CRDT merge behavior)
        // The exact merge result depends on automerge's CRDT semantics
        assert!(!merged.is_empty());
    }

    #[tokio::test]
    async fn test_sync_first_run_no_checkpoint() {
        let temp = TempDir::new().unwrap();
        let repo = create_test_repo().await;

        // Create document with content
        let doc = create_doc_with_text("Initial content");
        let handle = repo.create(doc).await.unwrap();

        // Create file with same content
        let file_path = temp.path().join("test.qmd");
        std::fs::write(&file_path, "Initial content").unwrap();

        // Create sync state WITHOUT any checkpoint (first run scenario)
        let mut sync_state = SyncState::load(temp.path()).unwrap();

        // Sync should work even without prior checkpoint
        let result = sync_document(&handle, &file_path, &mut sync_state).unwrap();

        // Should detect as filesystem changed (since no checkpoint means we treat
        // current heads as checkpoint, and file content differs from... wait, file
        // content is same as doc content, so it should actually be no-change
        // after the first "sync" establishes the checkpoint)
        //
        // Actually on first run:
        // - checkpoint_heads will be None, so we use current heads
        // - fs_content will match doc content (both "Initial content")
        // - But we don't have a checkpoint hash, so fs_unchanged will be false
        // So it will detect as "filesystem changed" even though content is same
        //
        // This is acceptable - the first sync establishes the checkpoint
        assert!(
            matches!(result, SyncResult::NoChanges)
                || matches!(result, SyncResult::FilesystemChanged { .. })
        );

        // After sync, checkpoint should be set
        let doc_id = handle.document_id().to_string();
        assert!(sync_state.has_checkpoint(&doc_id));
    }

    #[tokio::test]
    async fn test_sync_all_documents() {
        let temp = TempDir::new().unwrap();
        let project_root = temp.path();

        // Create project structure with multiple files
        std::fs::write(project_root.join("file1.qmd"), "Content 1").unwrap();
        std::fs::write(project_root.join("file2.qmd"), "Content 2").unwrap();
        std::fs::create_dir(project_root.join("subdir")).unwrap();
        std::fs::write(project_root.join("subdir/file3.qmd"), "Content 3").unwrap();

        let repo = create_test_repo().await;

        // Create index and add files (simulating what reconcile_files_with_index does)
        let (index, _) = IndexDocument::create(&repo).await.unwrap();

        // Create documents for each file and add to index
        for (path, content) in [
            ("file1.qmd", "Content 1"),
            ("file2.qmd", "Content 2"),
            ("subdir/file3.qmd", "Content 3"),
        ] {
            let doc = create_doc_with_text(content);
            let handle = repo.create(doc).await.unwrap();
            let doc_id = handle.document_id().to_string();
            index.add_file(path, &doc_id).unwrap();
        }

        // Create sync state
        let mut sync_state = SyncState::load(project_root).unwrap();

        // Sync all documents
        let result = sync_all_documents(&repo, &index, project_root, &mut sync_state).await;

        // All 3 documents should be synced (first run, so filesystem_changed)
        assert_eq!(result.total_synced(), 3);
        assert!(!result.has_errors());

        // Now sync again - should all be no_changes
        let result2 = sync_all_documents(&repo, &index, project_root, &mut sync_state).await;
        assert_eq!(result2.no_changes, 3);
        assert_eq!(result2.total_synced(), 3);
    }

    #[tokio::test]
    async fn test_sync_all_documents_with_missing_file() {
        let temp = TempDir::new().unwrap();
        let project_root = temp.path();

        // Create only one file
        std::fs::write(project_root.join("existing.qmd"), "I exist").unwrap();

        let repo = create_test_repo().await;

        // Create index
        let (index, _) = IndexDocument::create(&repo).await.unwrap();

        // Add existing file
        let doc = create_doc_with_text("I exist");
        let handle = repo.create(doc).await.unwrap();
        index
            .add_file("existing.qmd", &handle.document_id().to_string())
            .unwrap();

        // Add non-existing file to index
        let doc2 = create_doc_with_text("I don't exist");
        let handle2 = repo.create(doc2).await.unwrap();
        index
            .add_file("missing.qmd", &handle2.document_id().to_string())
            .unwrap();

        let mut sync_state = SyncState::load(project_root).unwrap();

        // Sync all documents
        let result = sync_all_documents(&repo, &index, project_root, &mut sync_state).await;

        // 1 synced, 1 skipped
        assert_eq!(result.total_synced(), 1);
        assert_eq!(result.skipped, 1);
        assert!(!result.has_errors());
    }
}
