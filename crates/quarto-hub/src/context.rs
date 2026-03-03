//! Hub context - shared state for the server
//!
//! Contains the automerge repo and storage manager.

use std::path::Path;
use std::sync::{Arc, OnceLock};

use automerge::{Automerge, ObjType, ROOT, transaction::Transactable};
use axum::http::StatusCode;
use axum_jwt_auth::JwtDecoder;
use samod::Repo;
use samod::storage::TokioFilesystemStorage;
use tokio::sync::Mutex;
use tracing::{debug, info, warn};

use crate::auth::{self, AuthConfig, AuthState, GoogleClaims};
use crate::discovery::ProjectFiles;
use crate::error::Result;
use crate::index::{IndexDocument, load_or_create_index};
use crate::peer::spawn_peer_connection;
use crate::resource::{create_binary_document, detect_mime_type};
use crate::storage::StorageManager;
use crate::sync::{SyncAllResult, SyncResult, sync_all_documents, sync_file_by_path};
use crate::sync_state::SyncState;

/// Configuration for the hub.
#[derive(Debug)]
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

    /// OAuth2 auth configuration. None = auth disabled.
    pub auth_config: Option<AuthConfig>,

    /// Allow auth without TLS (local dev). When true, the `Secure` flag is
    /// omitted from auth cookies so browsers send them over plain HTTP.
    pub allow_insecure_auth: bool,
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
            auth_config: None,
            allow_insecure_auth: false,
        }
    }
}

/// Shared context for the hub server.
///
/// This is wrapped in `Arc` and shared across all request handlers.
/// The struct is Clone-friendly: samod::Repo wraps Arc internally,
/// and StorageManager is wrapped in Arc at the SharedContext level.
///
/// Supports two modes:
/// - **Project mode**: Discovers files, syncs with filesystem, watches for changes.
/// - **Standalone mode**: Pure sync server with no local project files.
pub struct HubContext {
    /// Storage manager (holds lockfile, manages directories)
    storage: StorageManager,

    /// Discovered project files (None in standalone mode)
    project_files: Option<ProjectFiles>,

    /// samod Repo - handles document storage, sync, and concurrency internally.
    /// Clone is cheap: Repo wraps Arc<Mutex<Inner>>.
    repo: Repo,

    /// The project index document (maps file paths to document IDs)
    index: IndexDocument,

    /// Sync state for filesystem synchronization (protected by Mutex for interior mutability).
    /// None in standalone mode (no filesystem to sync with).
    sync_state: Option<Mutex<SyncState>>,

    /// OAuth2 auth configuration (immutable after startup). None = auth disabled.
    auth_config: Option<AuthConfig>,

    /// Auth state: JWT decoder + JWKS refresh handle. Initialized once
    /// at server startup when auth is configured. Using OnceLock because
    /// it's set after construction but before the server accepts requests.
    auth_state: OnceLock<AuthState>,

    /// Whether insecure (HTTP) auth is allowed. When true, `Secure` flag
    /// is omitted from auth cookies.
    allow_insecure_auth: bool,
}

impl HubContext {
    /// Create a new hub context.
    ///
    /// In project mode (when `StorageManager` has a project root):
    /// 1. Discovers project files on the filesystem
    /// 2. Initializes the samod Repo with filesystem storage
    /// 3. Loads or creates the index document
    /// 4. Reconciles discovered files with the index
    /// 5. Performs initial filesystem sync
    ///
    /// In standalone mode (no project root):
    /// 1. Initializes the samod Repo with filesystem storage
    /// 2. Loads or creates the index document
    /// 3. Spawns peer connections
    pub async fn new(mut storage: StorageManager, mut config: HubConfig) -> Result<Self> {
        let project_root = storage.project_root().map(|p| p.to_path_buf());

        // Discover project files (only in project mode)
        let project_files = if let Some(ref project_root) = project_root {
            let files = ProjectFiles::discover(project_root);
            info!(
                qmd_count = files.qmd_files.len(),
                config_count = files.config_files.len(),
                binary_count = files.binary_files.len(),
                "Discovered project files"
            );
            Some(files)
        } else {
            info!("Standalone mode: skipping file discovery");
            None
        };

        // Initialize samod repo with filesystem storage
        let automerge_dir = storage.automerge_dir();
        info!(automerge_dir = %automerge_dir.display(), "Initializing samod repo");

        let samod_storage = TokioFilesystemStorage::new(&automerge_dir);
        let repo = Repo::build_tokio()
            .with_storage(samod_storage)
            .with_announce_policy(|_doc_id, _peer_id| false)
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

        // Reconcile discovered files with the index and perform initial sync
        // (only in project mode)
        let sync_state =
            if let (Some(project_root), Some(project_files)) = (&project_root, &project_files) {
                let reconciled =
                    reconcile_files_with_index(&repo, &index, project_files, project_root).await?;
                if reconciled > 0 {
                    info!(count = reconciled, "Reconciled new files with index");
                }

                // Initialize sync state from hub directory
                let mut sync_state = SyncState::load(storage.hub_dir())?;

                // Perform initial sync on startup
                let sync_result =
                    sync_all_documents(&repo, &index, project_root, &mut sync_state).await;

                info!(
                    synced = sync_result.total_synced(),
                    errors = sync_result.errors.len(),
                    "Initial filesystem sync complete"
                );

                Some(Mutex::new(sync_state))
            } else {
                info!("Standalone mode: skipping file reconciliation and initial sync");
                None
            };

        // Spawn background tasks to connect to configured peers
        for peer_url in &config.peers {
            info!(url = %peer_url, "Starting peer connection");
            spawn_peer_connection(repo.clone(), peer_url.clone());
        }

        let auth_config = config.auth_config.take();
        let allow_insecure_auth = config.allow_insecure_auth;

        Ok(Self {
            storage,
            project_files,
            repo,
            index,
            sync_state,
            auth_config,
            auth_state: OnceLock::new(),
            allow_insecure_auth,
        })
    }

    /// Returns whether this hub is running in project mode (has a local project).
    pub fn has_project(&self) -> bool {
        self.storage.project_root().is_some()
    }

    /// Get reference to storage manager.
    pub fn storage(&self) -> &StorageManager {
        &self.storage
    }

    /// Get discovered project files (None in standalone mode).
    pub fn project_files(&self) -> Option<&ProjectFiles> {
        self.project_files.as_ref()
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
    /// In standalone mode, this is a no-op (returns a default result with no synced documents).
    /// In project mode, syncs all documents to disk.
    pub async fn sync_all(&self) -> SyncAllResult {
        let Some(ref project_root) = self.storage.project_root().map(|p| p.to_path_buf()) else {
            return SyncAllResult::default();
        };
        let Some(ref sync_state_mutex) = self.sync_state else {
            return SyncAllResult::default();
        };
        let mut sync_state = sync_state_mutex.lock().await;
        sync_all_documents(&self.repo, &self.index, project_root, &mut sync_state).await
    }

    /// Sync a single file by its path.
    ///
    /// This is called when the filesystem watcher detects a file change.
    /// In standalone mode, this is a no-op (returns Ok(None)).
    ///
    /// # Arguments
    /// * `file_path` - Absolute path to the changed file
    ///
    /// # Returns
    /// * `Ok(Some(SyncResult))` - Sync succeeded
    /// * `Ok(None)` - File is not tracked (not in index), or standalone mode
    /// * `Err(Error)` - Sync failed
    pub async fn sync_file(&self, file_path: &std::path::Path) -> Result<Option<SyncResult>> {
        let Some(project_root) = self.storage.project_root().map(|p| p.to_path_buf()) else {
            return Ok(None);
        };
        let Some(ref sync_state_mutex) = self.sync_state else {
            return Ok(None);
        };
        let mut sync_state = sync_state_mutex.lock().await;
        sync_file_by_path(
            &self.repo,
            &self.index,
            file_path,
            &project_root,
            &mut sync_state,
        )
        .await
    }

    /// Get the auth configuration, if auth is enabled.
    pub fn auth_config(&self) -> Option<&AuthConfig> {
        self.auth_config.as_ref()
    }

    /// Store the auth state (decoder + refresh task handle).
    /// Called once during server startup in `build_router`.
    pub fn set_auth_state(&self, state: AuthState) -> std::result::Result<(), &'static str> {
        self.auth_state
            .set(state)
            .map_err(|_| "auth_state already initialized")
    }

    /// Whether auth cookies should omit the `Secure` flag (HTTP dev mode).
    pub fn allow_insecure_auth(&self) -> bool {
        self.allow_insecure_auth
    }

    /// Authenticate a request. If auth is disabled, always succeeds.
    /// If auth is enabled, token must be present and valid.
    /// Used by both REST and WebSocket handlers.
    pub async fn authenticate(&self, token: Option<&str>) -> std::result::Result<(), StatusCode> {
        if self.auth_config().is_none() {
            return Ok(()); // Auth disabled — allow all.
        }
        self.authenticate_claims(token).await.map(|_| ())
    }

    /// Authenticate a request and return the decoded claims.
    /// Unlike `authenticate()`, this returns `Err` when auth is disabled
    /// (because there are no claims to return). Used by `/auth/me`.
    pub async fn authenticate_claims(
        &self,
        token: Option<&str>,
    ) -> std::result::Result<GoogleClaims, StatusCode> {
        let auth_config = self.auth_config().ok_or(StatusCode::UNAUTHORIZED)?;

        let token = token.ok_or(StatusCode::UNAUTHORIZED)?;
        let auth_state = self
            .auth_state
            .get()
            .expect("auth_state is always present when auth is configured");

        // JwtDecoder<T>::decode returns TokenData<T>. The T parameter
        // lives on the trait, so we use a type annotation (not turbofish)
        // to select GoogleClaims.
        let token_data: jsonwebtoken::TokenData<GoogleClaims> =
            auth_state.decoder.decode(token).await.map_err(|err| {
                tracing::warn!(%err, "Auth failed");
                StatusCode::UNAUTHORIZED
            })?;

        auth::check_allowlists(&token_data.claims, auth_config)?;
        tracing::debug!(email = %token_data.claims.email, "Authenticated");
        Ok(token_data.claims)
    }
}

/// Type alias for the shared context used in axum handlers.
pub type SharedContext = Arc<HubContext>;

/// Reconcile discovered files with the index document.
///
/// For each file in `project_files` that is not already in the index:
/// - Read the file content from disk
/// - Create a new automerge document (Text for text files, Binary for binary files)
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

    // Reconcile text files (config + qmd)
    for file_path in project_files.text_files() {
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
                warn!(path = %path_str, error = %e, "Failed to read text file, skipping");
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
            warn!(path = %path_str, error = ?e, "Failed to initialize text document, skipping");
            continue;
        }

        let doc_handle = repo
            .create(doc)
            .await
            .map_err(|_| crate::error::Error::IndexDocument("repo is stopped".to_string()))?;

        let doc_id = doc_handle.document_id().to_string();

        // Add to index
        index.add_file(&path_str, &doc_id)?;

        info!(path = %path_str, doc_id = %doc_id, content_len = file_content.len(), "Added new text file to index");
        added += 1;
    }

    // Reconcile binary files
    for file_path in &project_files.binary_files {
        let path_str = file_path.to_string_lossy();

        // Skip if already in index
        if index.has_file(&path_str) {
            debug!(path = %path_str, "Binary file already in index");
            continue;
        }

        // Read file content from disk as bytes
        let full_path = project_root.join(file_path);
        let file_content = match std::fs::read(&full_path) {
            Ok(content) => content,
            Err(e) => {
                warn!(path = %path_str, error = %e, "Failed to read binary file, skipping");
                continue;
            }
        };

        // Detect MIME type from content and filename
        let mime_type = detect_mime_type(&file_content, full_path.to_str());

        // Create binary document
        let doc = match create_binary_document(&file_content, &mime_type) {
            Ok(doc) => doc,
            Err(e) => {
                warn!(path = %path_str, error = ?e, "Failed to create binary document, skipping");
                continue;
            }
        };

        let doc_handle = repo
            .create(doc)
            .await
            .map_err(|_| crate::error::Error::IndexDocument("repo is stopped".to_string()))?;

        let doc_id = doc_handle.document_id().to_string();

        // Add to index
        index.add_file(&path_str, &doc_id)?;

        info!(
            path = %path_str,
            doc_id = %doc_id,
            content_len = file_content.len(),
            mime_type = %mime_type,
            "Added new binary file to index"
        );
        added += 1;
    }

    Ok(added)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_hub_context_standalone_mode() {
        let temp = TempDir::new().unwrap();
        let data_dir = temp.path().join("hub-data");

        let storage = StorageManager::new_standalone(&data_dir).unwrap();
        let config = HubConfig::default();

        let ctx = HubContext::new(storage, config).await.unwrap();

        // Should be in standalone mode
        assert!(!ctx.has_project());
        assert!(ctx.project_files().is_none());

        // Repo and index should still be initialized
        let files = ctx.index().get_all_files();
        assert!(files.is_empty()); // No files discovered in standalone mode

        // sync_all should be a no-op
        let result = ctx.sync_all().await;
        assert_eq!(result.total_synced(), 0);
    }

    #[tokio::test]
    async fn test_hub_context_project_mode() {
        let temp = TempDir::new().unwrap();

        // Create a qmd file
        std::fs::write(temp.path().join("index.qmd"), "# Hello").unwrap();

        let storage = StorageManager::new(temp.path()).unwrap();
        let config = HubConfig::default();

        let ctx = HubContext::new(storage, config).await.unwrap();

        // Should be in project mode
        assert!(ctx.has_project());
        assert!(ctx.project_files().is_some());

        let pf = ctx.project_files().unwrap();
        assert_eq!(pf.qmd_files.len(), 1);

        // File should be in the index
        let files = ctx.index().get_all_files();
        assert_eq!(files.len(), 1);
    }
}
