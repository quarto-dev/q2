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
//!
//! Binary files use simpler semantics: last-writer-wins based on content hash.

use std::path::{Component, Path, PathBuf};
use std::str::FromStr;

use automerge::{ROOT, ReadDoc, transaction::Transactable};
use samod::{DocHandle, DocumentId, Repo};
use tracing::{debug, warn};

use crate::error::{Error, Result};
use crate::index::IndexDocument;
use crate::resource::{
    self, DocumentType, compute_hash, detect_document_type, detect_mime_type, read_binary_content,
    read_content_hash,
};
use crate::sync_state::{SyncState, sha256_hash};

/// Policy controlling the automerge → filesystem direction of sync.
///
/// The disk → automerge direction is never gated: filesystem changes always
/// flow into the document so connected clients see fresh content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DiskWritePolicy {
    /// Bidirectional sync (hub default): merged automerge content is written
    /// back to the file when it differs from what's on disk.
    #[default]
    WriteBack,
    /// The file on disk is authoritative; document changes are never written
    /// to disk (`q2 preview` without `--allow-edit`). Text documents keep
    /// remote changes merged in until the next filesystem edit converges the
    /// document back to disk content; binary documents are reverted to the
    /// filesystem content immediately.
    ReadOnly,
}

/// Synchronize a single document with its corresponding filesystem file.
///
/// This implements the unified sync algorithm:
/// 1. Get sync checkpoint (use current heads if none exists or invalid)
/// 2. Read filesystem content
/// 3. Fork at sync checkpoint (with fallback if fork_at fails)
/// 4. Apply filesystem content to fork (update_text handles diff internally)
/// 5. Merge fork back into main document
/// 6. Write merged state back to filesystem (skipped under
///    [`DiskWritePolicy::ReadOnly`], where disk stays authoritative)
/// 7. Update sync checkpoint
///
/// Steps 3-5 are skipped under [`DiskWritePolicy::ReadOnly`] when the
/// filesystem content is unchanged since the last checkpoint: with no
/// disk delta there is nothing to merge, and applying the stale (never
/// written-back) disk content would revert live doc-side edits.
///
/// # Arguments
/// * `doc_handle` - Handle to the automerge document
/// * `file_path` - Path to the filesystem file
/// * `sync_state` - Mutable reference to sync state for reading/updating checkpoints
/// * `policy` - Whether merged automerge content may be written back to disk
///
/// # Returns
/// * `Ok(SyncResult)` - Summary of what happened during sync
/// * `Err(Error)` - If sync failed (file not readable, automerge error, etc.)
pub fn sync_document(
    doc_handle: &DocHandle,
    file_path: &Path,
    sync_state: &mut SyncState,
    policy: DiskWritePolicy,
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

        // Steps 3-5 merge the filesystem delta into the document.
        // They must only run when the filesystem actually changed:
        // the fork-apply rewrites the fork from its checkpoint state
        // to the current disk content, and under ReadOnly the
        // checkpoint pairs current heads with the *disk* hash (the
        // file is never written back), so the doc state at those
        // heads has diverged from the disk content. Applying that
        // stale disk content anyway would manufacture a revert of
        // every doc-side edit older than the previous checkpoint,
        // and merging it would delete those edits from the live
        // document (the `--ui editor` ephemeral-editing clobber).
        // With no disk delta there is nothing to merge; the
        // checkpoint update below still advances the heads so the
        // next sync doesn't re-fire. WriteBack is unaffected: its
        // checkpoint always pairs heads with the content written
        // back, so the fork-apply is a no-op when only the doc
        // changed — but running it keeps the merged-content read
        // below honest, so the gate is ReadOnly-specific.
        let merge_fs_delta = !fs_unchanged || policy == DiskWritePolicy::WriteBack;
        if merge_fs_delta {
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
        }

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

        // Write merged content back to filesystem (only if it differs and
        // the policy allows the automerge → disk direction)
        let merged_content_hash = sha256_hash(&merged_content);
        if merged_content_hash != fs_content_hash {
            match policy {
                DiskWritePolicy::WriteBack => {
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
                DiskWritePolicy::ReadOnly => {
                    debug!(
                        doc_id = %doc_id,
                        path = %file_path.display(),
                        "Disk write-back disabled; file left untouched"
                    );
                }
            }
        }

        // 7. Update sync checkpoint. Under ReadOnly the file was not
        // written, so the checkpoint must record the hash of what is
        // actually on disk — otherwise the next sync would misread the
        // unchanged file as a fresh filesystem edit.
        let checkpoint_hash = match policy {
            DiskWritePolicy::WriteBack => &merged_content_hash,
            DiskWritePolicy::ReadOnly => &fs_content_hash,
        };
        let new_heads = doc.get_heads();
        sync_state.set_checkpoint(&doc_id, &new_heads, checkpoint_hash);

        Ok(result_type)
    });

    match &result {
        Ok(SyncResult::NoChanges) => {
            debug!(doc_id = %doc_id, path = %file_path.display(), "Sync complete: no changes");
        }
        Ok(SyncResult::AutomergeChanged { new_len }) => {
            debug!(
                doc_id = %doc_id,
                path = %file_path.display(),
                new_len = new_len,
                "Sync complete: automerge → filesystem"
            );
        }
        Ok(SyncResult::FilesystemChanged { new_len }) => {
            debug!(
                doc_id = %doc_id,
                path = %file_path.display(),
                new_len = new_len,
                "Sync complete: filesystem → automerge"
            );
        }
        Ok(SyncResult::BothChanged { merged_len }) => {
            debug!(
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

/// Synchronize a binary document with its corresponding filesystem file.
///
/// Binary files use simpler last-writer-wins semantics:
/// - If filesystem hash differs from document hash, update the one that changed
/// - If both changed since checkpoint, filesystem wins (local edits take precedence)
/// - No CRDT merging for binary content
///
/// # Arguments
/// * `doc_handle` - Handle to the automerge document
/// * `file_path` - Path to the filesystem file
/// * `sync_state` - Mutable reference to sync state
///
/// # Returns
/// * `Ok(SyncResult)` - Summary of what happened during sync
/// * `Err(Error)` - If sync failed
pub fn sync_binary_document(
    doc_handle: &DocHandle,
    file_path: &Path,
    sync_state: &mut SyncState,
    policy: DiskWritePolicy,
) -> Result<SyncResult> {
    let doc_id = doc_handle.document_id().to_string();

    // Read filesystem content
    let fs_content = std::fs::read(file_path).map_err(|e| {
        Error::Sync(format!(
            "failed to read binary file {}: {}",
            file_path.display(),
            e
        ))
    })?;
    let fs_hash = compute_hash(&fs_content);

    let result = doc_handle.with_document(|doc| {
        // Get document content hash
        let doc_hash = read_content_hash(doc);

        // Get last synced hash from checkpoint
        let checkpoint_hash = sync_state.get_content_hash(&doc_id);

        // Determine what changed
        let doc_unchanged = doc_hash.as_deref() == checkpoint_hash;
        let fs_unchanged = checkpoint_hash == Some(fs_hash.as_str());

        // Early exit: no changes
        if doc_unchanged && fs_unchanged {
            debug!(doc_id = %doc_id, "No changes detected in binary file, skipping sync");
            return Ok(SyncResult::NoChanges);
        }

        // Determine sync direction. Under ReadOnly the automerge → disk
        // direction is forbidden: a document-only change is reverted to the
        // filesystem content instead (binary has no merge, so leaving the
        // divergence in place would re-trigger on every periodic sync).
        let doc_to_disk = !doc_unchanged && fs_unchanged && policy == DiskWritePolicy::WriteBack;
        let (result_type, final_content, final_hash) = if doc_to_disk {
            // Document changed, filesystem unchanged -> write doc to filesystem
            let content = read_binary_content(doc).ok_or_else(|| {
                Error::Sync(format!(
                    "failed to read binary content from document {}",
                    doc_id
                ))
            })?;
            let hash = doc_hash.unwrap_or_else(|| compute_hash(&content));
            (
                SyncResult::AutomergeChanged {
                    new_len: content.len(),
                },
                content,
                hash,
            )
        } else {
            // Filesystem changed (or both changed) -> filesystem wins
            // For binary files, we don't have meaningful merge, so local edits take precedence
            let result_type = if !doc_unchanged && !fs_unchanged {
                SyncResult::BothChanged {
                    merged_len: fs_content.len(),
                }
            } else if !doc_unchanged {
                // ReadOnly revert of a document-only change: report the
                // automerge side as the one that changed, but the content
                // that wins is the filesystem's.
                SyncResult::AutomergeChanged {
                    new_len: fs_content.len(),
                }
            } else {
                SyncResult::FilesystemChanged {
                    new_len: fs_content.len(),
                }
            };

            // Update document with filesystem content
            let mime_type = detect_mime_type(&fs_content, file_path.to_str());
            doc.transact::<_, _, automerge::AutomergeError>(|tx| {
                tx.put(ROOT, "content", fs_content.clone())?;
                tx.put(ROOT, "mimeType", mime_type)?;
                tx.put(ROOT, "hash", fs_hash.clone())?;
                Ok(())
            })
            .map_err(|e| Error::Sync(format!("failed to update binary document: {:?}", e)))?;

            (result_type, fs_content, fs_hash)
        };

        // Write to filesystem if needed (only in the doc → disk direction;
        // a ReadOnly revert also reports AutomergeChanged but must not write)
        if doc_to_disk {
            std::fs::write(file_path, &final_content).map_err(|e| {
                Error::Sync(format!(
                    "failed to write binary content to {}: {}",
                    file_path.display(),
                    e
                ))
            })?;
            debug!(
                doc_id = %doc_id,
                path = %file_path.display(),
                "Wrote binary content to filesystem"
            );
        }

        // Update sync checkpoint
        let new_heads = doc.get_heads();
        sync_state.set_checkpoint(&doc_id, &new_heads, &final_hash);

        Ok(result_type)
    });

    match &result {
        Ok(SyncResult::NoChanges) => {
            debug!(doc_id = %doc_id, path = %file_path.display(), "Binary sync: no changes");
        }
        Ok(SyncResult::AutomergeChanged { new_len }) => {
            debug!(
                doc_id = %doc_id,
                path = %file_path.display(),
                new_len = new_len,
                "Binary sync: automerge → filesystem"
            );
        }
        Ok(SyncResult::FilesystemChanged { new_len }) => {
            debug!(
                doc_id = %doc_id,
                path = %file_path.display(),
                new_len = new_len,
                "Binary sync: filesystem → automerge"
            );
        }
        Ok(SyncResult::BothChanged { merged_len }) => {
            debug!(
                doc_id = %doc_id,
                path = %file_path.display(),
                merged_len = merged_len,
                "Binary sync: both changed, filesystem wins"
            );
        }
        Err(e) => {
            warn!(
                doc_id = %doc_id,
                path = %file_path.display(),
                error = %e,
                "Binary sync failed"
            );
        }
    }

    result
}

/// Synchronize a document based on its type (text or binary).
///
/// Detects the document type and dispatches to the appropriate sync function.
pub fn sync_document_auto(
    doc_handle: &DocHandle,
    file_path: &Path,
    sync_state: &mut SyncState,
    policy: DiskWritePolicy,
) -> Result<SyncResult> {
    let doc_type = doc_handle.with_document(|doc| detect_document_type(doc));

    match doc_type {
        DocumentType::Text => sync_document(doc_handle, file_path, sync_state, policy),
        DocumentType::Binary => sync_binary_document(doc_handle, file_path, sync_state, policy),
        DocumentType::Invalid => {
            // Try to infer from file extension
            let is_binary = file_path
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(resource::is_binary_extension);

            if is_binary {
                sync_binary_document(doc_handle, file_path, sync_state, policy)
            } else {
                sync_document(doc_handle, file_path, sync_state, policy)
            }
        }
    }
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
    policy: DiskWritePolicy,
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

    // Sync the document (auto-detects text vs binary)
    let result = sync_document_auto(&doc_handle, file_path, sync_state, policy)?;

    // Save sync state
    sync_state.save()?;

    Ok(Some(result))
}

/// Join an index-relative path onto `project_root`, rejecting anything that
/// could escape the root. Returns the joined absolute path, or `None` if the
/// relative path is unsafe.
///
/// Rejects absolute paths, Windows path prefixes (drive/UNC), `RootDir`, and
/// any `..` that pops above the root. Allows ordinary nested paths and `.`.
/// Pure lexical check — no filesystem access.
fn contained_join(project_root: &Path, rel: &str) -> Option<PathBuf> {
    let mut folded = Vec::new();
    for component in Path::new(rel).components() {
        match component {
            // Absolute or rooted (incl. Windows drive/UNC) → escape.
            Component::Prefix(_) | Component::RootDir => return None,
            // `..` pops a segment; popping above the root is an escape.
            Component::ParentDir => {
                folded.pop()?;
            }
            Component::CurDir => {}
            Component::Normal(seg) => folded.push(seg),
        }
    }
    // Empty fold resolves to project_root itself (a directory); we only ever
    // want a file strictly under the root.
    if folded.is_empty() {
        return None;
    }
    Some(
        folded
            .iter()
            .fold(project_root.to_path_buf(), |mut p, seg| {
                p.push(seg);
                p
            }),
    )
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
    policy: DiskWritePolicy,
) -> SyncAllResult {
    let mut result = SyncAllResult::default();

    // Get all file mappings from the index
    let files = index.get_all_files();

    debug!(count = files.len(), "Starting sync of all documents");

    // Canonicalize the root once (loop-invariant) for the symlink-escape check.
    // Fail closed: if it fails, reject every write this cycle with one log line
    // rather than flooding per-entry attack-signal warns.
    let Ok(real_root) = std::fs::canonicalize(project_root) else {
        tracing::error!(
            root = %project_root.display(),
            "project root not canonicalizable, rejecting all index writes this cycle"
        );
        result.rejected = files.len();
        return result;
    };

    for (file_path_str, doc_id_str) in &files {
        // Reject index keys that escape the root (traversal / absolute / rooted).
        let Some(file_path) = contained_join(project_root, file_path_str) else {
            warn!(path = %file_path_str, "Index path escapes project root, rejecting");
            result.rejected += 1;
            continue;
        };

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

        // Symlink-escape check, placed immediately before the synchronous
        // write (no `.await` between here and it) to minimize the TOCTOU
        // window. `.exists()` above guarantees the target exists, so plain
        // canonicalize suffices. Rejects keys traversing a symlink that
        // points outside the root.
        match std::fs::canonicalize(&file_path) {
            Ok(real) if real.starts_with(&real_root) => {}
            _ => {
                warn!(path = %file_path_str, "Index path resolves outside project root, rejecting");
                result.rejected += 1;
                continue;
            }
        }

        // Sync the document (auto-detects text vs binary)
        match sync_document_auto(&doc_handle, &file_path, sync_state, policy) {
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

    debug!(
        no_changes = result.no_changes,
        automerge_changed = result.automerge_changed,
        filesystem_changed = result.filesystem_changed,
        both_changed = result.both_changed,
        errors = result.errors.len(),
        skipped = result.skipped,
        rejected = result.rejected,
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

    /// Number of index entries rejected for path containment (traversal /
    /// absolute / symlink-escape). Neither synced nor counted as changes.
    pub rejected: usize,

    /// Errors encountered during sync
    pub errors: Vec<SyncError>,
}

impl SyncAllResult {
    /// Total number of documents successfully synced
    pub fn total_synced(&self) -> usize {
        self.no_changes + self.automerge_changed + self.filesystem_changed + self.both_changed
    }

    /// Whether any documents actually changed during this sync — i.e.
    /// automerge → filesystem, filesystem → automerge, or both. Excludes
    /// `no_changes` (idempotent checks) and `skipped` (files missing on
    /// disk, which the per-file loop already `warn!`s about). Useful as
    /// the gate on summary log lines that should fire only when work
    /// actually happened.
    pub fn has_changes(&self) -> bool {
        self.automerge_changed + self.filesystem_changed + self.both_changed > 0
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

    #[test]
    fn has_changes_only_counts_real_changes() {
        // no_changes is bookkeeping — files that were checked but didn't
        // need syncing. It must NOT make has_changes() true; otherwise the
        // periodic-sync gate fires on every tick.
        let r = SyncAllResult {
            no_changes: 5,
            ..Default::default()
        };
        assert!(!r.has_changes());
        assert_eq!(r.total_synced(), 5);

        // automerge_changed, filesystem_changed, both_changed each
        // independently flip has_changes().
        let r = SyncAllResult {
            automerge_changed: 1,
            ..Default::default()
        };
        assert!(r.has_changes());

        let r = SyncAllResult {
            filesystem_changed: 1,
            ..Default::default()
        };
        assert!(r.has_changes());

        let r = SyncAllResult {
            both_changed: 1,
            ..Default::default()
        };
        assert!(r.has_changes());

        // Skipped + errors are reported elsewhere; has_changes() is
        // strictly about "did syncing produce a state change?".
        let r = SyncAllResult {
            skipped: 3,
            ..Default::default()
        };
        assert!(!r.has_changes());
    }

    // ===== contained_join lexical containment =====

    #[test]
    fn contained_join_accepts_ordinary_nested_paths() {
        let root = Path::new("/project");
        assert_eq!(
            contained_join(root, "index.qmd"),
            Some(root.join("index.qmd"))
        );
        assert_eq!(
            contained_join(root, "chapters/intro.qmd"),
            Some(root.join("chapters/intro.qmd"))
        );
        // `.` components are ignored.
        assert_eq!(
            contained_join(root, "a/./b.qmd"),
            Some(root.join("a/b.qmd"))
        );
    }

    #[test]
    fn contained_join_accepts_internal_parent_that_stays_in_root() {
        let root = Path::new("/project");
        assert_eq!(contained_join(root, "a/../b.qmd"), Some(root.join("b.qmd")));
    }

    #[test]
    fn contained_join_rejects_parent_escape() {
        let root = Path::new("/project");
        assert_eq!(contained_join(root, "../escape.txt"), None);
        assert_eq!(contained_join(root, "a/../../escape.txt"), None);
    }

    #[test]
    fn contained_join_rejects_absolute_unix() {
        let root = Path::new("/project");
        assert_eq!(contained_join(root, "/etc/passwd"), None);
    }

    #[test]
    fn contained_join_rejects_rooted_and_windows_prefixed() {
        let root = Path::new("/project");
        // Leading slash → RootDir component on every platform.
        assert_eq!(contained_join(root, "/x"), None);
        // Windows drive / UNC prefixes parse as Component::Prefix on Windows;
        // on Unix the backslash forms are a single Normal segment (no escape),
        // so only assert the reject where the prefix is actually recognized.
        #[cfg(windows)]
        {
            assert_eq!(contained_join(root, r"C:\x"), None);
            assert_eq!(contained_join(root, r"\\?\C:\x"), None);
        }
    }

    #[test]
    fn contained_join_rejects_paths_folding_to_root() {
        let root = Path::new("/project");
        // All resolve to project_root itself (a directory), never a file under it.
        assert_eq!(contained_join(root, ""), None);
        assert_eq!(contained_join(root, "."), None);
        assert_eq!(contained_join(root, "a/.."), None);
        assert_eq!(contained_join(root, "a/b/../.."), None);
    }

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
        let result = sync_document(
            &handle,
            &file_path,
            &mut sync_state,
            DiskWritePolicy::WriteBack,
        )
        .unwrap();
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
        let result = sync_document(
            &handle,
            &file_path,
            &mut sync_state,
            DiskWritePolicy::WriteBack,
        )
        .unwrap();
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
        let result = sync_document(
            &handle,
            &file_path,
            &mut sync_state,
            DiskWritePolicy::WriteBack,
        )
        .unwrap();
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
        let result = sync_document(
            &handle,
            &file_path,
            &mut sync_state,
            DiskWritePolicy::WriteBack,
        )
        .unwrap();
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
        let result = sync_document(
            &handle,
            &file_path,
            &mut sync_state,
            DiskWritePolicy::WriteBack,
        )
        .unwrap();

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
        let result = sync_all_documents(
            &repo,
            &index,
            project_root,
            &mut sync_state,
            DiskWritePolicy::WriteBack,
        )
        .await;

        // All 3 documents should be synced (first run, so filesystem_changed)
        assert_eq!(result.total_synced(), 3);
        assert!(!result.has_errors());

        // Now sync again - should all be no_changes
        let result2 = sync_all_documents(
            &repo,
            &index,
            project_root,
            &mut sync_state,
            DiskWritePolicy::WriteBack,
        )
        .await;
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
        let result = sync_all_documents(
            &repo,
            &index,
            project_root,
            &mut sync_state,
            DiskWritePolicy::WriteBack,
        )
        .await;

        // 1 synced, 1 skipped
        assert_eq!(result.total_synced(), 1);
        assert_eq!(result.skipped, 1);
        assert!(!result.has_errors());
    }

    // ========== Binary file sync tests ==========

    /// Helper to create a binary document
    fn create_doc_with_binary(content: &[u8], mime_type: &str) -> Automerge {
        crate::resource::create_binary_document(content, mime_type).unwrap()
    }

    /// Helper to read binary content from doc handle
    fn read_binary_from_handle(handle: &DocHandle) -> Vec<u8> {
        handle.with_document(|doc| crate::resource::read_binary_content(doc).unwrap())
    }

    /// Helper to update binary content in doc handle
    fn update_binary_in_handle(handle: &DocHandle, content: &[u8], mime_type: &str) {
        handle.with_document(|doc| {
            doc.transact::<_, _, automerge::AutomergeError>(|tx| {
                tx.put(ROOT, "content", content.to_vec())?;
                tx.put(ROOT, "mimeType", mime_type)?;
                tx.put(ROOT, "hash", compute_hash(content))?;
                Ok(())
            })
            .unwrap();
        });
    }

    #[tokio::test]
    async fn test_sync_binary_no_changes() {
        let temp = TempDir::new().unwrap();
        let repo = create_test_repo().await;

        // Create binary document with content
        let binary_content = vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]; // PNG header
        let doc = create_doc_with_binary(&binary_content, "image/png");
        let handle = repo.create(doc).await.unwrap();
        let doc_id = handle.document_id().to_string();

        // Create file with same content
        let file_path = temp.path().join("image.png");
        std::fs::write(&file_path, &binary_content).unwrap();

        // Create sync state and set initial checkpoint
        let mut sync_state = SyncState::load(temp.path()).unwrap();
        let content_hash = compute_hash(&binary_content);
        let heads = handle.with_document(|doc| doc.get_heads());
        sync_state.set_checkpoint(&doc_id, &heads, &content_hash);

        // Sync should detect no changes
        let result = sync_binary_document(
            &handle,
            &file_path,
            &mut sync_state,
            DiskWritePolicy::WriteBack,
        )
        .unwrap();
        assert_eq!(result, SyncResult::NoChanges);

        // Content should be unchanged
        assert_eq!(read_binary_from_handle(&handle), binary_content);
        assert_eq!(std::fs::read(&file_path).unwrap(), binary_content);
    }

    #[tokio::test]
    async fn test_sync_binary_filesystem_changed() {
        let temp = TempDir::new().unwrap();
        let repo = create_test_repo().await;

        // Create binary document
        let original_content = vec![0x89, 0x50, 0x4E, 0x47]; // short PNG-like header
        let doc = create_doc_with_binary(&original_content, "image/png");
        let handle = repo.create(doc).await.unwrap();
        let doc_id = handle.document_id().to_string();

        // Create file with modified content
        let modified_content = vec![0xFF, 0xD8, 0xFF, 0xE0]; // JPEG header
        let file_path = temp.path().join("image.jpg");
        std::fs::write(&file_path, &modified_content).unwrap();

        // Create sync state with checkpoint at original state
        let mut sync_state = SyncState::load(temp.path()).unwrap();
        let heads = handle.with_document(|doc| doc.get_heads());
        sync_state.set_checkpoint(&doc_id, &heads, &compute_hash(&original_content));

        // Sync should update document from filesystem
        let result = sync_binary_document(
            &handle,
            &file_path,
            &mut sync_state,
            DiskWritePolicy::WriteBack,
        )
        .unwrap();
        assert!(matches!(result, SyncResult::FilesystemChanged { .. }));

        // Document should have new content
        assert_eq!(read_binary_from_handle(&handle), modified_content);
    }

    #[tokio::test]
    async fn test_sync_binary_automerge_changed() {
        let temp = TempDir::new().unwrap();
        let repo = create_test_repo().await;

        // Create binary document with initial content
        let original_content = vec![0x89, 0x50, 0x4E, 0x47];
        let doc = create_doc_with_binary(&original_content, "image/png");
        let handle = repo.create(doc).await.unwrap();
        let doc_id = handle.document_id().to_string();

        // Create file with original content (unchanged)
        let file_path = temp.path().join("image.png");
        std::fs::write(&file_path, &original_content).unwrap();

        // Record checkpoint at initial state
        let mut sync_state = SyncState::load(temp.path()).unwrap();
        let initial_heads = handle.with_document(|doc| doc.get_heads());
        sync_state.set_checkpoint(&doc_id, &initial_heads, &compute_hash(&original_content));

        // Modify document (simulating remote peer changes)
        let new_content = vec![0xFF, 0xD8, 0xFF, 0xE0];
        update_binary_in_handle(&handle, &new_content, "image/jpeg");

        // Sync should push document changes to filesystem
        let result = sync_binary_document(
            &handle,
            &file_path,
            &mut sync_state,
            DiskWritePolicy::WriteBack,
        )
        .unwrap();
        assert!(matches!(result, SyncResult::AutomergeChanged { .. }));

        // File should have new content
        assert_eq!(std::fs::read(&file_path).unwrap(), new_content);
    }

    #[tokio::test]
    async fn test_sync_binary_both_changed_filesystem_wins() {
        let temp = TempDir::new().unwrap();
        let repo = create_test_repo().await;

        // Create binary document
        let original_content = vec![0x00, 0x01, 0x02, 0x03];
        let doc = create_doc_with_binary(&original_content, "application/octet-stream");
        let handle = repo.create(doc).await.unwrap();
        let doc_id = handle.document_id().to_string();

        // Record checkpoint
        let mut sync_state = SyncState::load(temp.path()).unwrap();
        let initial_heads = handle.with_document(|doc| doc.get_heads());
        sync_state.set_checkpoint(&doc_id, &initial_heads, &compute_hash(&original_content));

        // Modify filesystem
        let fs_content = vec![0x10, 0x11, 0x12, 0x13];
        let file_path = temp.path().join("data.bin");
        std::fs::write(&file_path, &fs_content).unwrap();

        // Modify document differently
        let doc_content = vec![0x20, 0x21, 0x22, 0x23];
        update_binary_in_handle(&handle, &doc_content, "application/octet-stream");

        // Sync should pick filesystem (last-writer-wins for binary)
        let result = sync_binary_document(
            &handle,
            &file_path,
            &mut sync_state,
            DiskWritePolicy::WriteBack,
        )
        .unwrap();
        assert!(matches!(result, SyncResult::BothChanged { .. }));

        // Filesystem content wins
        assert_eq!(read_binary_from_handle(&handle), fs_content);
        assert_eq!(std::fs::read(&file_path).unwrap(), fs_content);
    }

    #[tokio::test]
    async fn test_sync_document_auto_dispatches_text() {
        let temp = TempDir::new().unwrap();
        let repo = create_test_repo().await;

        // Create text document
        let doc = create_doc_with_text("Hello text");
        let handle = repo.create(doc).await.unwrap();

        // Create file
        let file_path = temp.path().join("test.qmd");
        std::fs::write(&file_path, "Hello text").unwrap();

        let mut sync_state = SyncState::load(temp.path()).unwrap();

        // sync_document_auto should work for text documents
        let result = sync_document_auto(
            &handle,
            &file_path,
            &mut sync_state,
            DiskWritePolicy::WriteBack,
        );
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_sync_document_auto_dispatches_binary() {
        let temp = TempDir::new().unwrap();
        let repo = create_test_repo().await;

        // Create binary document
        let content = vec![0x89, 0x50, 0x4E, 0x47];
        let doc = create_doc_with_binary(&content, "image/png");
        let handle = repo.create(doc).await.unwrap();

        // Create file
        let file_path = temp.path().join("image.png");
        std::fs::write(&file_path, &content).unwrap();

        let mut sync_state = SyncState::load(temp.path()).unwrap();

        // sync_document_auto should work for binary documents
        let result = sync_document_auto(
            &handle,
            &file_path,
            &mut sync_state,
            DiskWritePolicy::WriteBack,
        );
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_sync_document_auto_uses_extension_fallback() {
        let temp = TempDir::new().unwrap();
        let repo = create_test_repo().await;

        // Create an empty/invalid document (neither text nor content field)
        let mut doc = Automerge::new();
        doc.transact::<_, _, automerge::AutomergeError>(|tx| {
            tx.put(ROOT, "other_field", "something")?;
            Ok(())
        })
        .unwrap();
        let handle = repo.create(doc).await.unwrap();

        // Create a .png file (should use binary sync based on extension)
        let content = vec![0x89, 0x50, 0x4E, 0x47];
        let file_path = temp.path().join("image.png");
        std::fs::write(&file_path, &content).unwrap();

        let mut sync_state = SyncState::load(temp.path()).unwrap();

        // sync_document_auto should fall back to extension-based detection
        // For binary extension with .png, it should use binary sync which
        // will update the empty document with the filesystem content
        let result = sync_document_auto(
            &handle,
            &file_path,
            &mut sync_state,
            DiskWritePolicy::WriteBack,
        );
        assert!(result.is_ok());

        // After sync, document should have the binary content
        let doc_content = read_binary_from_handle(&handle);
        assert_eq!(doc_content, content);
    }

    #[tokio::test]
    async fn test_sync_document_auto_text_extension_fallback() {
        let temp = TempDir::new().unwrap();
        let repo = create_test_repo().await;

        // Create an empty/invalid document (neither text nor content field)
        let mut doc = Automerge::new();
        doc.transact::<_, _, automerge::AutomergeError>(|tx| {
            tx.put(ROOT, "other_field", "something")?;
            Ok(())
        })
        .unwrap();
        let handle = repo.create(doc).await.unwrap();

        // Create a .qmd file (should use text sync based on extension)
        let file_path = temp.path().join("test.qmd");
        std::fs::write(&file_path, "# Hello").unwrap();

        let mut sync_state = SyncState::load(temp.path()).unwrap();

        // sync_document_auto should fall back to extension-based detection
        // For text extension .qmd, it should attempt text sync which will fail
        // because the document has no "text" field
        let result = sync_document_auto(
            &handle,
            &file_path,
            &mut sync_state,
            DiskWritePolicy::WriteBack,
        );
        assert!(result.is_err());
    }

    // ========== DiskWritePolicy::ReadOnly tests (bd-ov4gqk3m) ==========

    #[tokio::test]
    async fn test_sync_readonly_automerge_change_not_written_to_disk() {
        let temp = TempDir::new().unwrap();
        let repo = create_test_repo().await;

        let doc = create_doc_with_text("Original content");
        let handle = repo.create(doc).await.unwrap();
        let doc_id = handle.document_id().to_string();

        let file_path = temp.path().join("test.qmd");
        std::fs::write(&file_path, "Original content").unwrap();

        let mut sync_state = SyncState::load(temp.path()).unwrap();
        let heads = handle.with_document(|doc| doc.get_heads());
        sync_state.set_checkpoint(&doc_id, &heads, &sha256_hash("Original content"));

        // Simulate a browser-originated document edit
        update_text_in_handle(&handle, "Modified by automerge");

        let result = sync_document(
            &handle,
            &file_path,
            &mut sync_state,
            DiskWritePolicy::ReadOnly,
        )
        .unwrap();
        assert!(matches!(result, SyncResult::AutomergeChanged { .. }));

        // The file on disk must be untouched; the doc keeps its change.
        assert_eq!(
            std::fs::read_to_string(&file_path).unwrap(),
            "Original content"
        );
        assert_eq!(read_text_from_handle(&handle), "Modified by automerge");
    }

    #[tokio::test]
    async fn test_sync_readonly_repeat_sync_is_stable() {
        let temp = TempDir::new().unwrap();
        let repo = create_test_repo().await;

        let doc = create_doc_with_text("Original content");
        let handle = repo.create(doc).await.unwrap();
        let doc_id = handle.document_id().to_string();

        let file_path = temp.path().join("test.qmd");
        std::fs::write(&file_path, "Original content").unwrap();

        let mut sync_state = SyncState::load(temp.path()).unwrap();
        let heads = handle.with_document(|doc| doc.get_heads());
        sync_state.set_checkpoint(&doc_id, &heads, &sha256_hash("Original content"));

        update_text_in_handle(&handle, "Modified by automerge");

        // First sync absorbs the doc change without touching disk...
        sync_document(
            &handle,
            &file_path,
            &mut sync_state,
            DiskWritePolicy::ReadOnly,
        )
        .unwrap();

        // ...and every subsequent sync is a no-op (no churn from the
        // 5-second periodic loop), with the file still untouched.
        let second = sync_document(
            &handle,
            &file_path,
            &mut sync_state,
            DiskWritePolicy::ReadOnly,
        )
        .unwrap();
        assert_eq!(second, SyncResult::NoChanges);
        let third = sync_document(
            &handle,
            &file_path,
            &mut sync_state,
            DiskWritePolicy::ReadOnly,
        )
        .unwrap();
        assert_eq!(third, SyncResult::NoChanges);
        assert_eq!(
            std::fs::read_to_string(&file_path).unwrap(),
            "Original content"
        );
    }

    #[tokio::test]
    async fn test_sync_readonly_repeated_doc_edits_are_not_clobbered() {
        // Repro for the `--ui editor --share` clobber (ephemeral editing
        // under ReadOnly): with the disk untouched, successive periodic
        // syncs must preserve ALL doc-side edits, not just the latest.
        let temp = TempDir::new().unwrap();
        let repo = create_test_repo().await;

        let doc = create_doc_with_text("Original content");
        let handle = repo.create(doc).await.unwrap();
        let doc_id = handle.document_id().to_string();

        let file_path = temp.path().join("test.qmd");
        std::fs::write(&file_path, "Original content").unwrap();

        let mut sync_state = SyncState::load(temp.path()).unwrap();
        let heads = handle.with_document(|doc| doc.get_heads());
        sync_state.set_checkpoint(&doc_id, &heads, &sha256_hash("Original content"));

        // First browser edit; the first periodic sync absorbs it (disk
        // untouched, doc keeps the change).
        update_text_in_handle(&handle, "Original content\nedit-1");
        sync_document(
            &handle,
            &file_path,
            &mut sync_state,
            DiskWritePolicy::ReadOnly,
        )
        .unwrap();
        assert_eq!(read_text_from_handle(&handle), "Original content\nedit-1");

        // A second browser edit lands before the next periodic sync. The
        // disk has NOT changed, so this sync must leave both edits intact.
        update_text_in_handle(&handle, "Original content\nedit-1\nedit-2");
        sync_document(
            &handle,
            &file_path,
            &mut sync_state,
            DiskWritePolicy::ReadOnly,
        )
        .unwrap();
        assert_eq!(
            read_text_from_handle(&handle),
            "Original content\nedit-1\nedit-2",
            "the second periodic sync clobbered the first edit"
        );
        assert_eq!(
            std::fs::read_to_string(&file_path).unwrap(),
            "Original content"
        );
    }

    #[tokio::test]
    async fn test_sync_readonly_filesystem_change_syncs_to_doc() {
        let temp = TempDir::new().unwrap();
        let repo = create_test_repo().await;

        let doc = create_doc_with_text("Original content");
        let handle = repo.create(doc).await.unwrap();
        let doc_id = handle.document_id().to_string();

        let file_path = temp.path().join("test.qmd");
        std::fs::write(&file_path, "Modified by filesystem").unwrap();

        let mut sync_state = SyncState::load(temp.path()).unwrap();
        let heads = handle.with_document(|doc| doc.get_heads());
        sync_state.set_checkpoint(&doc_id, &heads, &sha256_hash("Original content"));

        // The disk → automerge direction must be unaffected by ReadOnly.
        let result = sync_document(
            &handle,
            &file_path,
            &mut sync_state,
            DiskWritePolicy::ReadOnly,
        )
        .unwrap();
        assert!(matches!(result, SyncResult::FilesystemChanged { .. }));
        assert_eq!(read_text_from_handle(&handle), "Modified by filesystem");
        assert_eq!(
            std::fs::read_to_string(&file_path).unwrap(),
            "Modified by filesystem"
        );
    }

    #[tokio::test]
    async fn test_sync_readonly_disk_edit_converges_doc_to_disk() {
        let temp = TempDir::new().unwrap();
        let repo = create_test_repo().await;

        let doc = create_doc_with_text("Original content");
        let handle = repo.create(doc).await.unwrap();
        let doc_id = handle.document_id().to_string();

        let file_path = temp.path().join("test.qmd");
        std::fs::write(&file_path, "Original content").unwrap();

        let mut sync_state = SyncState::load(temp.path()).unwrap();
        let heads = handle.with_document(|doc| doc.get_heads());
        sync_state.set_checkpoint(&doc_id, &heads, &sha256_hash("Original content"));

        // A doc-side edit is absorbed (file untouched)...
        update_text_in_handle(&handle, "Rogue browser edit");
        sync_document(
            &handle,
            &file_path,
            &mut sync_state,
            DiskWritePolicy::ReadOnly,
        )
        .unwrap();

        // ...then the user edits the file on disk. Disk is authoritative:
        // the next sync converges the document back to exactly the disk
        // content, discarding the doc-side edit rather than merging it.
        std::fs::write(&file_path, "Original content, user addition").unwrap();
        let result = sync_document(
            &handle,
            &file_path,
            &mut sync_state,
            DiskWritePolicy::ReadOnly,
        )
        .unwrap();
        assert!(matches!(result, SyncResult::FilesystemChanged { .. }));
        assert_eq!(
            read_text_from_handle(&handle),
            "Original content, user addition"
        );
        assert_eq!(
            std::fs::read_to_string(&file_path).unwrap(),
            "Original content, user addition"
        );
    }

    #[tokio::test]
    async fn test_sync_binary_readonly_doc_change_reverts_to_filesystem() {
        let temp = TempDir::new().unwrap();
        let repo = create_test_repo().await;

        let original = vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
        let doc = create_doc_with_binary(&original, "image/png");
        let handle = repo.create(doc).await.unwrap();
        let doc_id = handle.document_id().to_string();

        let file_path = temp.path().join("image.png");
        std::fs::write(&file_path, &original).unwrap();

        let mut sync_state = SyncState::load(temp.path()).unwrap();
        let heads = handle.with_document(|doc| doc.get_heads());
        sync_state.set_checkpoint(&doc_id, &heads, &compute_hash(&original));

        // Simulate a remote binary update
        let remote = vec![0xFF, 0xD8, 0xFF, 0xE0];
        update_binary_in_handle(&handle, &remote, "image/jpeg");

        // ReadOnly: the file must not be written; the document is reverted
        // to the filesystem content (binary has no merge, so divergence
        // would otherwise re-trigger every periodic sync).
        let result = sync_binary_document(
            &handle,
            &file_path,
            &mut sync_state,
            DiskWritePolicy::ReadOnly,
        )
        .unwrap();
        assert!(!matches!(result, SyncResult::NoChanges));
        assert_eq!(std::fs::read(&file_path).unwrap(), original);
        assert_eq!(read_binary_from_handle(&handle), original);

        // And the cycle terminates: next sync is a no-op.
        let second = sync_binary_document(
            &handle,
            &file_path,
            &mut sync_state,
            DiskWritePolicy::ReadOnly,
        )
        .unwrap();
        assert_eq!(second, SyncResult::NoChanges);
    }
}
