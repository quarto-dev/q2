//! Hub context - shared state for the server
//!
//! Contains the automerge repo and storage manager.

use std::path::Path;
use std::sync::Arc;

use automerge::{Automerge, ObjType, ROOT, transaction::Transactable};
use samod::Repo;
use samod::storage::TokioFilesystemStorage;
use tokio::sync::{Mutex, RwLock};
use tracing::{debug, info, warn};

use crate::discovery::ProjectFiles;
use crate::error::Result;
use crate::index::{IndexDocument, load_or_create_index};
use crate::peer::spawn_peer_connection;
use crate::storage::StorageManager;
use crate::sync::{SyncAllResult, SyncResult, sync_all_documents, sync_file_by_path};
use crate::sync_state::SyncState;

/// Configuration for the hub.
#[derive(Debug, Clone)]
pub struct HubConfig {
    /// Port to listen on
    pub port: u16,

    /// Host to bind to
    pub host: String,

    /// URLs of sync servers to peer with
    pub peers: Vec<String>,

    /// Periodic filesystem sync interval in seconds.
    /// Set to None to disable periodic sync.
    /// Default: 30 seconds.
    pub sync_interval_secs: Option<u64>,

    /// Enable filesystem watching for real-time sync.
    /// When enabled, changes to .qmd files are detected and synced immediately.
    /// Default: true.
    pub watch_enabled: bool,

    /// Debounce duration for filesystem events in milliseconds.
    /// Default: 500ms.
    pub watch_debounce_ms: u64,
}

impl Default for HubConfig {
    fn default() -> Self {
        Self {
            port: 3000,
            host: "127.0.0.1".to_string(),
            peers: Vec::new(),
            sync_interval_secs: Some(30),
            watch_enabled: true,
            watch_debounce_ms: 500,
        }
    }
}

/// Shared context for the hub server.
///
/// This is wrapped in `Arc` and shared across all request handlers.
/// The struct is Clone-friendly: samod::Repo wraps Arc internally,
/// and StorageManager is wrapped in Arc at the SharedContext level.
pub struct HubContext {
    /// Storage manager (holds lockfile, manages directories)
    storage: StorageManager,

    /// Hub configuration
    config: RwLock<HubConfig>,

    /// Discovered project files
    project_files: ProjectFiles,

    /// samod Repo - handles document storage, sync, and concurrency internally.
    /// Clone is cheap: Repo wraps Arc<Mutex<Inner>>.
    repo: Repo,

    /// The project index document (maps file paths to document IDs)
    index: IndexDocument,

    /// Sync state for filesystem synchronization (protected by Mutex for interior mutability)
    sync_state: Mutex<SyncState>,
}

impl HubContext {
    /// Create a new hub context for the given project.
    ///
    /// This:
    /// 1. Initializes the samod Repo with filesystem storage at `.quarto/hub/automerge/`
    /// 2. Loads or creates the index document
    /// 3. Reconciles discovered .qmd files with the index
    pub async fn new(mut storage: StorageManager, config: HubConfig) -> Result<Self> {
        // Discover project files
        let project_files = ProjectFiles::discover(storage.project_root());

        info!(
            qmd_count = project_files.qmd_files.len(),
            config_count = project_files.config_files.len(),
            "Discovered project files"
        );

        // Initialize samod repo with filesystem storage
        let automerge_dir = storage.automerge_dir();
        info!(automerge_dir = %automerge_dir.display(), "Initializing samod repo");

        let samod_storage = TokioFilesystemStorage::new(&automerge_dir);
        let repo = Repo::build_tokio().with_storage(samod_storage).load().await;

        info!("samod repo initialized");

        // Load or create the index document
        let existing_index_id = storage.index_document_id();
        let (index, new_index_id) = load_or_create_index(&repo, existing_index_id).await?;

        // If we created a new index, persist the ID
        if let Some(new_id) = new_index_id {
            storage.set_index_document_id(&new_id)?;
            info!(index_doc_id = %new_id, "Created and persisted new index document");
        }

        // Reconcile discovered files with the index
        let project_root = storage.project_root();
        let reconciled =
            reconcile_files_with_index(&repo, &index, &project_files, project_root).await?;
        if reconciled > 0 {
            info!(count = reconciled, "Reconciled new files with index");
        }

        // Spawn background tasks to connect to configured peers
        for peer_url in &config.peers {
            info!(url = %peer_url, "Starting peer connection");
            spawn_peer_connection(repo.clone(), peer_url.clone());
        }

        // Initialize sync state from hub directory
        let sync_state = SyncState::load(storage.hub_dir())?;

        // Perform initial sync on startup
        let project_root = storage.project_root().to_path_buf();
        let mut sync_state_guard = sync_state;
        let sync_result =
            sync_all_documents(&repo, &index, &project_root, &mut sync_state_guard).await;

        info!(
            synced = sync_result.total_synced(),
            errors = sync_result.errors.len(),
            "Initial filesystem sync complete"
        );

        Ok(Self {
            storage,
            config: RwLock::new(config),
            project_files,
            repo,
            index,
            sync_state: Mutex::new(sync_state_guard),
        })
    }

    /// Get reference to storage manager.
    pub fn storage(&self) -> &StorageManager {
        &self.storage
    }

    /// Get the current configuration.
    pub async fn config(&self) -> HubConfig {
        self.config.read().await.clone()
    }

    /// Get discovered project files.
    pub fn project_files(&self) -> &ProjectFiles {
        &self.project_files
    }

    /// Get reference to the samod repo.
    pub fn repo(&self) -> &Repo {
        &self.repo
    }

    /// Get reference to the index document.
    pub fn index(&self) -> &IndexDocument {
        &self.index
    }

    /// Perform a full sync of all documents with the filesystem.
    ///
    /// This is called on shutdown to ensure all changes are persisted.
    pub async fn sync_all(&self) -> SyncAllResult {
        let project_root = self.storage.project_root().to_path_buf();
        let mut sync_state = self.sync_state.lock().await;
        sync_all_documents(&self.repo, &self.index, &project_root, &mut sync_state).await
    }

    /// Sync a single file by its path.
    ///
    /// This is called when the filesystem watcher detects a file change.
    ///
    /// # Arguments
    /// * `file_path` - Absolute path to the changed file
    ///
    /// # Returns
    /// * `Ok(Some(SyncResult))` - Sync succeeded
    /// * `Ok(None)` - File is not tracked (not in index)
    /// * `Err(Error)` - Sync failed
    pub async fn sync_file(&self, file_path: &std::path::Path) -> Result<Option<SyncResult>> {
        let project_root = self.storage.project_root().to_path_buf();
        let mut sync_state = self.sync_state.lock().await;
        sync_file_by_path(
            &self.repo,
            &self.index,
            file_path,
            &project_root,
            &mut sync_state,
        )
        .await
    }
}

/// Type alias for the shared context used in axum handlers.
pub type SharedContext = Arc<HubContext>;

/// Reconcile discovered files with the index document.
///
/// For each file in `project_files` that is not already in the index:
/// - Read the file content from disk
/// - Create a new automerge document with a Text object containing the content
/// - Add the mapping to the index
///
/// Returns the number of new files added.
async fn reconcile_files_with_index(
    repo: &Repo,
    index: &IndexDocument,
    project_files: &ProjectFiles,
    project_root: &Path,
) -> Result<usize> {
    let mut added = 0;

    for file_path in project_files.all_files() {
        let path_str = file_path.to_string_lossy();

        // Skip if already in index
        if index.has_file(&path_str) {
            debug!(path = %path_str, "File already in index");
            continue;
        }

        // Read file content from disk
        let full_path = project_root.join(file_path);
        let file_content = match std::fs::read_to_string(&full_path) {
            Ok(content) => content,
            Err(e) => {
                warn!(path = %path_str, error = %e, "Failed to read file, skipping");
                continue;
            }
        };

        // Create a new automerge document with Text object initialized from file content
        let mut doc = Automerge::new();
        if let Err(e) = doc.transact::<_, _, automerge::AutomergeError>(|tx| {
            // Create a Text object at ROOT.text
            let text_obj = tx.put_object(ROOT, "text", ObjType::Text)?;
            // Initialize with file content using update_text (which handles diffing internally)
            tx.update_text(&text_obj, &file_content)?;
            Ok(())
        }) {
            warn!(path = %path_str, error = ?e, "Failed to initialize document, skipping");
            continue;
        }

        let doc_handle = repo
            .create(doc)
            .await
            .map_err(|_| crate::error::Error::IndexDocument("repo is stopped".to_string()))?;

        let doc_id = doc_handle.document_id().to_string();

        // Add to index
        index.add_file(&path_str, &doc_id)?;

        info!(path = %path_str, doc_id = %doc_id, content_len = file_content.len(), "Added new file to index");
        added += 1;
    }

    Ok(added)
}
