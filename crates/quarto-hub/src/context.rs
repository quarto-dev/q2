//! Hub context - shared state for the server
//!
//! Contains the automerge repo and storage manager.

use std::sync::Arc;

use automerge::Automerge;
use samod::storage::TokioFilesystemStorage;
use samod::Repo;
use tokio::sync::RwLock;
use tracing::{debug, info};

use crate::discovery::ProjectFiles;
use crate::error::Result;
use crate::index::{load_or_create_index, IndexDocument};
use crate::peer::spawn_peer_connection;
use crate::storage::StorageManager;

/// Configuration for the hub.
#[derive(Debug, Clone)]
pub struct HubConfig {
    /// Port to listen on
    pub port: u16,

    /// Host to bind to
    pub host: String,

    /// URLs of sync servers to peer with
    pub peers: Vec<String>,
}

impl Default for HubConfig {
    fn default() -> Self {
        Self {
            port: 3000,
            host: "127.0.0.1".to_string(),
            peers: Vec::new(),
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
            "Discovered project files"
        );

        // Initialize samod repo with filesystem storage
        let automerge_dir = storage.automerge_dir();
        info!(automerge_dir = %automerge_dir.display(), "Initializing samod repo");

        let samod_storage = TokioFilesystemStorage::new(&automerge_dir);
        let repo = Repo::build_tokio()
            .with_storage(samod_storage)
            .load()
            .await;

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
        let reconciled = reconcile_files_with_index(&repo, &index, &project_files).await?;
        if reconciled > 0 {
            info!(count = reconciled, "Reconciled new files with index");
        }

        // Spawn background tasks to connect to configured peers
        for peer_url in &config.peers {
            info!(url = %peer_url, "Starting peer connection");
            spawn_peer_connection(repo.clone(), peer_url.clone());
        }

        Ok(Self {
            storage,
            config: RwLock::new(config),
            project_files,
            repo,
            index,
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
}

/// Type alias for the shared context used in axum handlers.
pub type SharedContext = Arc<HubContext>;

/// Reconcile discovered files with the index document.
///
/// For each file in `project_files` that is not already in the index:
/// - Create a new automerge document for the file
/// - Add the mapping to the index
///
/// Returns the number of new files added.
async fn reconcile_files_with_index(
    repo: &Repo,
    index: &IndexDocument,
    project_files: &ProjectFiles,
) -> Result<usize> {
    let mut added = 0;

    for file_path in &project_files.qmd_files {
        let path_str = file_path.to_string_lossy();

        // Skip if already in index
        if index.has_file(&path_str) {
            debug!(path = %path_str, "File already in index");
            continue;
        }

        // Create a new document for this file
        // For now, we create an empty document - the actual file content
        // will be loaded/synced in a future phase (filesystem serialization)
        let doc = Automerge::new();
        let doc_handle = repo
            .create(doc)
            .await
            .map_err(|_| crate::error::Error::IndexDocument("repo is stopped".to_string()))?;

        let doc_id = doc_handle.document_id().to_string();

        // Add to index
        index.add_file(&path_str, &doc_id)?;

        info!(path = %path_str, doc_id = %doc_id, "Added new file to index");
        added += 1;
    }

    Ok(added)
}
