//! Hub context - shared state for the server
//!
//! Contains the automerge repo and storage manager.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex as StdMutex, OnceLock};

use automerge::{Automerge, ObjType, ROOT, transaction::Transactable};
use axum::http::StatusCode;
use axum_jwt_auth::JwtDecoder;
use samod::storage::TokioFilesystemStorage;
use samod::{AcceptorHandle, NeverAnnounce, PeerId, Repo};
use tokio::sync::Mutex;
use tracing::{debug, info, warn};

use crate::access_policy::AuditAccessPolicy;
use crate::auth::{self, AuthConfig, AuthState, OidcClaims};
use crate::discovery::ProjectFiles;
use crate::error::Result;
use crate::index::{IndexDocument, load_or_create_index};
use crate::peer::spawn_peer_connection;
use crate::resource::{create_binary_document, detect_mime_type};
use crate::revocation::{RevocationLedger, RevocationStatus};
use crate::session::{SessionKeys, SessionLifetimes, VerifiedSession};
use crate::storage::StorageManager;
use crate::sync::{
    DiskWritePolicy, SyncAllResult, SyncResult, sync_all_documents, sync_file_by_path,
};
use crate::sync_state::SyncState;
use crate::watch::WatchFilter;

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

    /// Which files surface as watch events. See [`WatchFilter`].
    /// Default: `WatchFilter::QmdOnly` (legacy hub behaviour).
    /// `quarto-preview` overrides this to `WatchFilter::PreviewBroad`
    /// so config + asset edits trigger re-render.
    pub watch_filter: WatchFilter,

    /// Single-file mode for `q2 preview` (bd-tnm3k): when `Some`,
    /// discovery skips the project walk and indexes only this one
    /// file (relative to `project_root`), and the watcher subscribes
    /// to just that file instead of the project root.
    ///
    /// The absolute target path used by the watcher is reconstructed
    /// inside `run_server_with` as `project_root.join(single_file)`,
    /// so the caller must supply a non-empty relative path that
    /// resolves to an existing `.qmd` file under `project_root`.
    pub single_file: Option<PathBuf>,

    /// Resources-scoped `.html` files to sync into the VFS as text
    /// (project-root-relative paths). (bd-kjrpya2d)
    ///
    /// `q2 preview` populates this with the `.html` files made visible
    /// by `project.resources:` (resolved upstream in `quarto-preview`,
    /// which has `quarto-core`), so embedded example decks land in the
    /// VFS source and the preview iframe post-processor can inline them.
    /// The bare `discover` walk can't find `.html` on its own; this is
    /// how the resolved set reaches [`ProjectFiles::with_resource_files`].
    /// Empty for the standalone `quarto hub` server.
    pub resource_files: Vec<PathBuf>,

    /// Single-file mode (bd-kpuweafo): binary **asset** siblings the deck
    /// references (project-root-relative paths), e.g. `![](./img.png)`.
    ///
    /// Single-file mode does not walk the directory (the `bd-tnm3k` safety
    /// property), so the bare `single_file` discovery never finds these.
    /// `q2 preview` resolves them upstream in `quarto-preview` (which has the
    /// qmd parser) via `config::resolve_single_file_assets` and injects them
    /// here so they reach [`ProjectFiles::with_binary_files`] and flow through
    /// the same automerge → VFS → blob-URL path project mode uses. Empty for
    /// project mode (the dir walk finds binaries) and the `quarto hub` server.
    pub single_file_assets: Vec<PathBuf>,

    /// Single-file mode (bd-9cyza5vy): included `.qmd` files the deck pulls in
    /// via `{{< include >}}` (transitively), project-root-relative.
    ///
    /// Single-file mode does not walk the directory, so the bare `single_file`
    /// discovery never finds these. `q2 preview` resolves the deck's transitive
    /// include closure upstream in `quarto-preview` (which has the qmd parser)
    /// via `config::resolve_single_file_deps` and injects them here so they reach
    /// [`ProjectFiles::with_text_deps`] — synced into the VFS as text (readable
    /// by the WASM include-expansion stage) but kept out of `qmd_files` (invisible
    /// dependencies). Empty for project mode and the `quarto hub` server.
    pub single_file_text_deps: Vec<PathBuf>,

    /// OAuth2 auth configuration. None = auth disabled.
    pub auth_config: Option<AuthConfig>,

    /// Allow auth without TLS (local dev). When true, the `Secure` flag is
    /// omitted from auth cookies so browsers send them over plain HTTP.
    pub allow_insecure_auth: bool,

    /// Register `GET /` as the samod ws upgrade endpoint.
    ///
    /// Default: `true`. The `quarto hub` CLI keeps it true for
    /// sync.automerge.org compatibility (the convention is that
    /// `/` is the upgrade path). `quarto preview` sets it false so
    /// its embedded SPA can own `/` for `index.html`; samod still
    /// works at `/ws`, which hub-client and the q2-preview SPA both
    /// already use as their canonical connect path.
    pub register_root_ws: bool,

    /// Whether automerge document changes may be written back to files on
    /// disk. See [`DiskWritePolicy`].
    ///
    /// Default: `WriteBack` (the hub's collaborative semantics). `q2 preview`
    /// sets `ReadOnly` unless `--allow-edit` is given, so browser-originated
    /// document changes can never modify the user's files.
    pub disk_write_policy: DiskWritePolicy,
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
            watch_filter: WatchFilter::default(),
            single_file: None,
            resource_files: Vec::new(),
            single_file_assets: Vec::new(),
            single_file_text_deps: Vec::new(),
            auth_config: None,
            allow_insecure_auth: false,
            register_root_ws: true,
            disk_write_policy: DiskWritePolicy::default(),
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

    /// Acceptor handle for inbound WebSocket connections.
    acceptor: AcceptorHandle,

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

    /// Session-token signing/verification keys, derived from the storage
    /// manager's session secret at startup.
    session_keys: SessionKeys,

    /// Sliding-session lifetime configuration (idle + absolute caps),
    /// resolved from the environment at startup.
    session_lifetimes: SessionLifetimes,

    /// Revocation-event store (`revocations.json`): per-`sub`
    /// `not_before` entries from logout-everywhere plus operator bans.
    revocations: RevocationLedger,

    /// See [`HubConfig::register_root_ws`]. Stashed on the context so
    /// `build_router` can consult it after `HubContext::new` consumes
    /// the originating `HubConfig`.
    register_root_ws: bool,

    /// See [`HubConfig::disk_write_policy`]. Applied to every filesystem
    /// sync this context performs (initial, periodic, watcher-triggered).
    disk_write_policy: DiskWritePolicy,

    /// Maps peer IDs to authenticated user emails.
    /// Populated by handle_websocket when auth is enabled; read by the
    /// AuditAccessPolicy for audit logging.
    peer_emails: Arc<StdMutex<HashMap<PeerId, String>>>,

    /// The access policy installed on the repo. Held so the WebSocket handler
    /// can prune a peer's audit-dedup entries on disconnect via `forget_peer`.
    audit_policy: AuditAccessPolicy,
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

        // Discover project files (only in project mode). bd-tnm3k:
        // single-file mode skips the WalkDir entirely — the relative
        // path is already known, and walking the parent directory
        // would index sibling files the user didn't ask for.
        let project_files = if let Some(ref project_root) = project_root {
            let files = match config.single_file.as_ref() {
                // bd-ggvq1j68: single-file mode skips the WalkDir, but a deck
                // that references `brand: _brand.yml` needs that sibling in the
                // VFS. Pick up the conventionally-named brand file next to the
                // target without walking the directory (stays narrow).
                Some(rel) => ProjectFiles::single_file(rel.clone())
                    .with_config_siblings(project_root, rel)
                    // bd-kpuweafo: sync the sibling images the deck references
                    // (resolved upstream by quarto-preview) so they reach the
                    // VFS like project mode, without walking the directory.
                    .with_binary_files(config.single_file_assets.clone())
                    // bd-9cyza5vy: sync the deck's transitive `{{< include >}}`
                    // closure as invisible text deps (also resolved upstream)
                    // so includes expand in the WASM pipeline.
                    .with_text_deps(config.single_file_text_deps.clone()),
                // bd-kjrpya2d: the bare walk can't see resources-scoped
                // `.html` (it falls through every category), so the
                // caller-resolved set is injected here to ride the text
                // sync path into the VFS.
                None => ProjectFiles::discover(project_root)
                    .with_resource_files(config.resource_files.clone()),
            };
            info!(
                qmd_count = files.qmd_files.len(),
                config_count = files.config_files.len(),
                binary_count = files.binary_files.len(),
                extension_count = files.extension_files.len(),
                source_count = files.source_files.len(),
                resource_count = files.resource_files.len(),
                text_dep_count = files.text_dep_files.len(),
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
        let peer_emails = Arc::new(StdMutex::new(HashMap::new()));
        let audit_policy = AuditAccessPolicy::new(peer_emails.clone());

        let builder = Repo::build_tokio()
            .with_storage(samod_storage)
            .with_announce_policy(NeverAnnounce)
            .with_access_policy(audit_policy.clone());

        let repo = builder.load().await;

        // Create an acceptor for inbound WebSocket connections
        let acceptor_url: url::Url = format!("ws://{}:{}", config.host, config.port)
            .parse()
            .expect("valid acceptor URL");
        let acceptor = repo
            .make_acceptor(acceptor_url)
            .map_err(|_| crate::error::Error::Server("repo is stopped".to_string()))?;

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
                let sync_result = sync_all_documents(
                    &repo,
                    &index,
                    project_root,
                    &mut sync_state,
                    config.disk_write_policy,
                )
                .await;

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
        let register_root_ws = config.register_root_ws;
        let disk_write_policy = config.disk_write_policy;

        let session_lifetimes =
            SessionLifetimes::from_env().map_err(crate::error::Error::Server)?;
        // Graceful rotation (C5b): verify under current + previous kid
        // during the overlap window (= one idle timeout); sign always
        // under current. No previous configured (or window lapsed) →
        // single-key ring; that absence is also the emergency-rotation
        // mode (immediate global invalidation).
        let session_keys = match crate::storage::resolve_previous_session_secret(
            storage.config(),
            session_lifetimes.idle_secs,
            crate::session::unix_now(),
        )? {
            Some(previous) => SessionKeys::with_previous(*storage.session_secret(), previous),
            None => SessionKeys::new(*storage.session_secret()),
        };
        let revocations = RevocationLedger::load(
            storage.hub_dir(),
            session_lifetimes.absolute_secs,
            crate::session::unix_now(),
        )?;

        Ok(Self {
            storage,
            project_files,
            repo,
            acceptor,
            index,
            sync_state,
            auth_config,
            auth_state: OnceLock::new(),
            allow_insecure_auth,
            session_keys,
            session_lifetimes,
            revocations,
            register_root_ws,
            disk_write_policy,
            peer_emails,
            audit_policy,
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

    /// Returns the server secret bytes for HMAC actor ID derivation.
    pub fn server_secret_bytes(&self) -> &[u8] {
        self.storage.server_secret()
    }

    /// Get discovered project files (None in standalone mode).
    pub fn project_files(&self) -> Option<&ProjectFiles> {
        self.project_files.as_ref()
    }

    /// Get reference to the samod repo.
    pub fn repo(&self) -> &Repo {
        &self.repo
    }

    /// Get reference to the acceptor handle for inbound connections.
    pub fn acceptor(&self) -> &AcceptorHandle {
        &self.acceptor
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
        sync_all_documents(
            &self.repo,
            &self.index,
            project_root,
            &mut sync_state,
            self.disk_write_policy,
        )
        .await
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
            self.disk_write_policy,
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

    /// Whether [`set_auth_state`](Self::set_auth_state) has already
    /// installed an [`AuthState`]. Used by [`crate::server::build_router_with_state`]
    /// to skip the OIDC-discovery initialization path when a caller
    /// (notably integration tests built against a mock OIDC provider
    /// on `http://`) has supplied the decoder out-of-band.
    pub fn auth_state_initialized(&self) -> bool {
        self.auth_state.get().is_some()
    }

    /// Whether auth cookies should omit the `Secure` flag (HTTP dev mode).
    pub fn allow_insecure_auth(&self) -> bool {
        self.allow_insecure_auth
    }

    /// See [`HubConfig::register_root_ws`].
    pub fn register_root_ws(&self) -> bool {
        self.register_root_ws
    }

    /// Peer→email map for document access audit logging.
    pub fn peer_emails(&self) -> &Arc<StdMutex<HashMap<PeerId, String>>> {
        &self.peer_emails
    }

    /// Audit access policy, exposed so the WebSocket handler can prune a peer's
    /// audit-dedup entries when it disconnects.
    pub fn audit_policy(&self) -> &AuditAccessPolicy {
        &self.audit_policy
    }

    /// Validate a **Google ID token** against JWKS and return its
    /// claims. Returns `Err` when auth is disabled (there are no claims
    /// to return).
    ///
    /// Since the sliding-sessions cutover this is never a request-
    /// credential path by itself: it is reachable only from the Bearer
    /// branch of [`Self::authenticate_credential`] and from the
    /// mint-time validation in `auth_callback`/`auth_session`, whose
    /// input is a fresh Google credential from the request body — never
    /// the cookie (that path is [`Self::authenticate_session`]).
    ///
    /// Mint-time semantics: the revocation ledger is **skipped** here
    /// (see [`RevocationEnforcement::Skip`]) — bans gate mint
    /// explicitly in the handlers, and the `min_auth_time` clamp
    /// handles `not_before` floors so same-second re-login after a
    /// logout-everywhere keeps working.
    pub async fn authenticate_claims(
        &self,
        token: Option<&str>,
    ) -> std::result::Result<OidcClaims, StatusCode> {
        self.authenticate_claims_for_kind(token, "unknown", RevocationEnforcement::Skip)
            .await
    }

    /// Variant of [`authenticate_claims`] that records the
    /// `credential_kind` (`"bearer"` / `"unknown"`) on every audit
    /// event, as required by Phase 2 of the device-flow plan, and lets
    /// the caller pick the revocation-ledger posture (request
    /// credentials enforce; mint-time validation skips).
    pub async fn authenticate_claims_for_kind(
        &self,
        token: Option<&str>,
        credential_kind: &'static str,
        revocation: RevocationEnforcement,
    ) -> std::result::Result<OidcClaims, StatusCode> {
        let auth_config = self.auth_config().ok_or_else(|| {
            tracing::event!(
                target: "quarto_hub::audit",
                tracing::Level::WARN,
                action = "auth_fail",
                outcome = "deny",
                credential_kind = credential_kind,
                detail = "auth_disabled",
            );
            StatusCode::UNAUTHORIZED
        })?;

        let token = token.ok_or_else(|| {
            tracing::event!(
                target: "quarto_hub::audit",
                tracing::Level::INFO,
                action = "auth_fail",
                outcome = "deny",
                credential_kind = credential_kind,
                detail = "missing_credential",
            );
            StatusCode::UNAUTHORIZED
        })?;
        let auth_state = self
            .auth_state
            .get()
            .expect("auth_state is always present when auth is configured");

        // JwtDecoder<T>::decode returns TokenData<T>. The T parameter
        // lives on the trait, so we use a type annotation (not turbofish)
        // to select OidcClaims.
        let token_data: jsonwebtoken::TokenData<OidcClaims> =
            auth_state.decoder.decode(token).await.map_err(|err| {
                // Bearer/JWT failures never reach OidcClaims, so we
                // identify the event by `detail` rather than `sub`.
                let detail = format!("jwt_decode:{err}");
                tracing::event!(
                    target: "quarto_hub::audit",
                    tracing::Level::INFO,
                    action = "auth_fail",
                    outcome = "deny",
                    credential_kind = credential_kind,
                    detail = %detail,
                );
                StatusCode::UNAUTHORIZED
            })?;

        // OIDC §3.1.3.7 azp rule + future-iat rejection — gaps the
        // jsonwebtoken validator does not cover. The audience list
        // matches the one used when building the validator, so any
        // azp ∈ allowed_audiences is by construction one we already
        // accept.
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs() as i64);
        if let Err(status) =
            auth::validate_azp_and_iat(&token_data.claims, auth_state.audiences().iter(), now, 60)
        {
            tracing::event!(
                target: "quarto_hub::audit",
                tracing::Level::INFO,
                action = "auth_fail",
                outcome = "deny",
                credential_kind = credential_kind,
                sub = %token_data.claims.sub,
                detail = "azp_or_iat_rejected",
            );
            return Err(status);
        }

        if let Err(status) = auth::check_allowlists(&token_data.claims, auth_config) {
            // 401 = unverified email; 403 = good creds, not in allowlist.
            // The plan's audit test expects detail=user_not_allowlisted
            // for the 403 case so a downstream log-aggregator can
            // distinguish identity policy from credential policy.
            let detail = if status == StatusCode::FORBIDDEN {
                "user_not_allowlisted"
            } else {
                "email_not_verified"
            };
            tracing::event!(
                target: "quarto_hub::audit",
                tracing::Level::INFO,
                action = "auth_fail",
                outcome = "deny",
                credential_kind = credential_kind,
                sub = %token_data.claims.sub,
                detail = detail,
            );
            return Err(status);
        }

        // Revocation ledger on the request-credential path (bd-jkih1ql7):
        // bans and logout-everywhere floors must bite Bearer credentials
        // too. Anchored at the token's `iat`; a missing `iat` (the type
        // admits it, OIDC requires it) fails closed against any
        // `not_before` entry. Must run before the `auth_ok` emission —
        // an allow-then-deny pair for one request would corrupt the
        // audit log.
        if revocation == RevocationEnforcement::Enforce {
            let anchor = token_data.claims.iat.unwrap_or(0);
            match self.revocations.check(&token_data.claims.sub, anchor).await {
                RevocationStatus::Banned => {
                    tracing::event!(
                        target: "quarto_hub::audit",
                        tracing::Level::INFO,
                        action = "auth_fail",
                        outcome = "deny",
                        credential_kind = credential_kind,
                        sub = %token_data.claims.sub,
                        detail = "user_banned",
                    );
                    return Err(StatusCode::FORBIDDEN);
                }
                RevocationStatus::Revoked => {
                    // Deliberately not `session_revoked` — it isn't a
                    // session; the dead thing is the Bearer token itself.
                    tracing::event!(
                        target: "quarto_hub::audit",
                        tracing::Level::INFO,
                        action = "auth_fail",
                        outcome = "deny",
                        credential_kind = credential_kind,
                        sub = %token_data.claims.sub,
                        detail = "bearer_revoked",
                    );
                    return Err(StatusCode::UNAUTHORIZED);
                }
                RevocationStatus::Ok => {}
            }
        }

        tracing::event!(
            target: "quarto_hub::audit",
            tracing::Level::INFO,
            action = "auth_ok",
            outcome = "allow",
            credential_kind = credential_kind,
            sub = %token_data.claims.sub,
        );
        Ok(token_data.claims)
    }

    /// Session key material for minting/verifying hub session cookies.
    pub fn session_keys(&self) -> &SessionKeys {
        &self.session_keys
    }

    /// Sliding-session lifetime configuration.
    pub fn session_lifetimes(&self) -> SessionLifetimes {
        self.session_lifetimes
    }

    /// The revocation-event store (logout-everywhere + bans).
    pub fn revocations(&self) -> &RevocationLedger {
        &self.revocations
    }

    /// Whether verified session claims still pass the per-request
    /// policy checks (allowlist + revocation/ban) — the silent variant
    /// used by the sliding re-issue layer, which must never extend a
    /// session that would fail authentication but has already had its
    /// authoritative auth decision audit-logged by the handler path.
    pub async fn session_policy_ok(&self, claims: &crate::session::SessionClaims) -> bool {
        let Some(auth_config) = self.auth_config() else {
            return false;
        };
        if auth::check_allowlists_for(&claims.email, claims.email_verified, auth_config).is_err() {
            return false;
        }
        self.revocations.check(&claims.sub, claims.auth_time).await == RevocationStatus::Ok
    }

    /// Verify a hub-minted session token (the Cookie credential path).
    ///
    /// Beyond the cryptographic/lifetime checks in
    /// [`crate::session::verify_session`], this re-runs the allowlist
    /// check on the session claims (`email`, `email_verified` stamped at
    /// mint) — allowlist removal bites on the user's next request, not
    /// at absolute expiry. Failures are audit-logged distinguishably
    /// (`session_kid_mismatch` / `session_expired` /
    /// `session_absolute_cap` / `session_tampered`); token contents are
    /// never logged.
    pub async fn authenticate_session(
        &self,
        token: &str,
    ) -> std::result::Result<VerifiedSession, StatusCode> {
        let auth_config = self.auth_config().ok_or_else(|| {
            tracing::event!(
                target: "quarto_hub::audit",
                tracing::Level::WARN,
                action = "auth_fail",
                outcome = "deny",
                credential_kind = "cookie",
                detail = "auth_disabled",
            );
            StatusCode::UNAUTHORIZED
        })?;

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs() as i64);

        let verified =
            crate::session::verify_session(&self.session_keys, self.session_lifetimes, token, now)
                .map_err(|err| {
                    let detail = format!("session_{}", err.audit_class());
                    tracing::event!(
                        target: "quarto_hub::audit",
                        tracing::Level::INFO,
                        action = "auth_fail",
                        outcome = "deny",
                        credential_kind = "cookie",
                        detail = %detail,
                    );
                    StatusCode::UNAUTHORIZED
                })?;

        // Revocation events (§3): bans dominate; a logout-everywhere
        // event kills every token whose auth_time predates it.
        match self
            .revocations
            .check(&verified.claims.sub, verified.claims.auth_time)
            .await
        {
            RevocationStatus::Banned => {
                tracing::event!(
                    target: "quarto_hub::audit",
                    tracing::Level::INFO,
                    action = "auth_fail",
                    outcome = "deny",
                    credential_kind = "cookie",
                    sub = %verified.claims.sub,
                    detail = "user_banned",
                );
                return Err(StatusCode::FORBIDDEN);
            }
            RevocationStatus::Revoked => {
                tracing::event!(
                    target: "quarto_hub::audit",
                    tracing::Level::INFO,
                    action = "auth_fail",
                    outcome = "deny",
                    credential_kind = "cookie",
                    sub = %verified.claims.sub,
                    detail = "session_revoked",
                );
                return Err(StatusCode::UNAUTHORIZED);
            }
            RevocationStatus::Ok => {}
        }

        // Per-request allowlist re-check on the session claims (§5).
        if let Err(status) = auth::check_allowlists_for(
            &verified.claims.email,
            verified.claims.email_verified,
            auth_config,
        ) {
            let detail = if status == StatusCode::FORBIDDEN {
                "user_not_allowlisted"
            } else {
                "email_not_verified"
            };
            tracing::event!(
                target: "quarto_hub::audit",
                tracing::Level::INFO,
                action = "auth_fail",
                outcome = "deny",
                credential_kind = "cookie",
                sub = %verified.claims.sub,
                detail = detail,
            );
            return Err(status);
        }

        tracing::event!(
            target: "quarto_hub::audit",
            tracing::Level::INFO,
            action = "auth_ok",
            outcome = "allow",
            credential_kind = "cookie",
            sub = %verified.claims.sub,
        );
        Ok(verified)
    }

    /// Central credential dispatch (§5): each credential kind gets its
    /// own pinned verification path — the token itself never selects
    /// how it is verified.
    ///
    /// * `Cookie` → hub-session HS256 verify ([`Self::authenticate_session`]).
    /// * `Bearer` → Google ID token via JWKS, unchanged (the MCP path).
    pub async fn authenticate_credential(
        &self,
        credential: &crate::server::Credential,
    ) -> std::result::Result<AuthenticatedUser, StatusCode> {
        match credential {
            crate::server::Credential::Cookie(token) => self
                .authenticate_session(token)
                .await
                .map(AuthenticatedUser::Session),
            crate::server::Credential::Bearer(token) => self
                .authenticate_claims_for_kind(
                    Some(token),
                    crate::server::CredentialKind::Bearer.label(),
                    RevocationEnforcement::Enforce,
                )
                .await
                .map(AuthenticatedUser::Google),
        }
    }
}

/// Whether [`HubContext::authenticate_claims_for_kind`] enforces the
/// revocation ledger (bans + `not_before` floors) on the validated
/// claims.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RevocationEnforcement {
    /// Request-credential path (the Bearer arm of
    /// [`HubContext::authenticate_credential`]): a banned `sub` is 403,
    /// an `iat` below the user's `not_before` floor is 401.
    Enforce,
    /// Mint-time validation (`auth_callback` / `auth_session`): bans
    /// gate mint explicitly in the handlers, and the `min_auth_time`
    /// clamp handles the floor — a raw `iat` check here would break
    /// same-second re-login after logout-everywhere.
    Skip,
}

/// A successfully validated request credential, tagged with the path
/// that verified it.
#[derive(Debug, Clone)]
pub enum AuthenticatedUser {
    /// Cookie path: hub-minted session token.
    Session(VerifiedSession),
    /// Bearer path: Google ID token verified against JWKS (MCP clients).
    Google(OidcClaims),
}

impl AuthenticatedUser {
    pub fn email(&self) -> &str {
        match self {
            AuthenticatedUser::Session(v) => &v.claims.email,
            AuthenticatedUser::Google(c) => &c.email,
        }
    }

    /// The JWT `sub` claim. (Named `subject` to avoid colliding with
    /// `std::ops::Sub::sub` in method resolution/lints.)
    pub fn subject(&self) -> &str {
        match self {
            AuthenticatedUser::Session(v) => &v.claims.sub,
            AuthenticatedUser::Google(c) => &c.sub,
        }
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

        debug!(path = %path_str, doc_id = %doc_id, content_len = file_content.len(), "Added new text file to index");
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

        debug!(
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

    /// bd-9cyza5vy: in single-file mode, included `.qmd` injected via
    /// `single_file_text_deps` must be synced into the index as invisible text
    /// deps — present in the VFS (so includes expand) but NOT in `qmd_files`.
    #[tokio::test]
    async fn test_single_file_mode_syncs_text_deps_invisibly() {
        let temp = TempDir::new().unwrap();
        std::fs::write(temp.path().join("main.qmd"), "{{< include part.qmd >}}\n").unwrap();
        std::fs::write(temp.path().join("part.qmd"), "## Included\n").unwrap();

        let storage = StorageManager::new(temp.path()).unwrap();
        let config = HubConfig {
            single_file: Some(PathBuf::from("main.qmd")),
            single_file_text_deps: vec![PathBuf::from("part.qmd")],
            ..HubConfig::default()
        };

        let ctx = HubContext::new(storage, config).await.unwrap();

        let pf = ctx.project_files().unwrap();
        // The deck is the only nav qmd; the include is an invisible text dep.
        assert_eq!(pf.qmd_files, vec![PathBuf::from("main.qmd")]);
        assert_eq!(pf.text_dep_files, vec![PathBuf::from("part.qmd")]);

        // Both reach the index (so both are in the VFS for include expansion).
        let files = ctx.index().get_all_files();
        assert!(
            files.iter().any(|(f, _)| f.as_str() == "main.qmd"),
            "deck missing from index: {files:?}"
        );
        assert!(
            files.iter().any(|(f, _)| f.as_str() == "part.qmd"),
            "included text dep missing from index: {files:?}"
        );
    }

    /// Build an automerge text document with the given content.
    fn text_doc(content: &str) -> Automerge {
        let mut doc = Automerge::new();
        doc.transact::<_, _, automerge::AutomergeError>(|tx| {
            let text_obj = tx.put_object(ROOT, "text", ObjType::Text)?;
            tx.update_text(&text_obj, content)?;
            Ok(())
        })
        .unwrap();
        doc
    }

    /// A poisoned index key with `..` must not let an attacker overwrite a file
    /// outside the project root. Routes through the real `sync_all()` path.
    #[tokio::test]
    async fn sync_all_rejects_parent_traversal_write() {
        let temp = TempDir::new().unwrap();
        let project_root = temp.path().join("project");
        std::fs::create_dir(&project_root).unwrap();
        std::fs::write(project_root.join("index.qmd"), "# Hello").unwrap();

        // Victim file *outside* the root (so `.exists()` would pass).
        let victim = temp.path().join("victim.txt");
        std::fs::write(&victim, "original").unwrap();

        let storage = StorageManager::new(&project_root).unwrap();
        let ctx = HubContext::new(storage, HubConfig::default())
            .await
            .unwrap();

        // Poison the index exactly as a malicious wire client would: `add_file`
        // performs no containment by design, so this is a faithful attack model.
        let doc = ctx.repo().create(text_doc("PWNED")).await.unwrap();
        ctx.index()
            .add_file("../victim.txt", &doc.document_id().to_string())
            .unwrap();

        let result = ctx.sync_all().await;

        assert_eq!(result.rejected, 1, "poisoned entry must be rejected");
        assert_eq!(
            std::fs::read_to_string(&victim).unwrap(),
            "original",
            "victim file outside root must be untouched"
        );
        // Legitimate file still syncs.
        assert!(result.total_synced() >= 1);
    }

    /// A symlink *inside* the root pointing outward must not let a poisoned key
    /// with no `..` (passes the lexical check) write through it.
    #[cfg(unix)]
    #[tokio::test]
    async fn sync_all_rejects_symlink_escape_write() {
        let temp = TempDir::new().unwrap();
        let project_root = temp.path().join("project");
        std::fs::create_dir(&project_root).unwrap();
        std::fs::write(project_root.join("index.qmd"), "# Hello").unwrap();

        // Victim dir + file outside the root.
        let outside = temp.path().join("outside");
        std::fs::create_dir(&outside).unwrap();
        let victim = outside.join("victim.txt");
        std::fs::write(&victim, "original").unwrap();

        // `project/assets` is a symlink to the outside dir.
        std::os::unix::fs::symlink(&outside, project_root.join("assets")).unwrap();

        let storage = StorageManager::new(&project_root).unwrap();
        let ctx = HubContext::new(storage, HubConfig::default())
            .await
            .unwrap();

        // Key `assets/victim.txt` has no `..` — lexical check passes; only the
        // canonicalize + starts_with check catches the symlink escape.
        let doc = ctx.repo().create(text_doc("PWNED")).await.unwrap();
        ctx.index()
            .add_file("assets/victim.txt", &doc.document_id().to_string())
            .unwrap();

        let result = ctx.sync_all().await;

        assert_eq!(result.rejected, 1, "symlink-escape entry must be rejected");
        assert_eq!(
            std::fs::read_to_string(&victim).unwrap(),
            "original",
            "victim file reachable via symlink must be untouched"
        );
    }
}
