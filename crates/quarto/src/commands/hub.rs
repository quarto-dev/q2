//! Hub command - collaborative editing server
//!
//! This command starts the Quarto Hub server, which provides real-time
//! collaborative editing for Quarto projects using Automerge CRDTs.
//!
//! By default, `quarto hub` watches the current directory (or `--project` path).
//! Use `--no-project` to run as a standalone sync server without a local project.

use std::path::PathBuf;

use anyhow::Result;
use quarto_hub::{StorageManager, auth, context::HubConfig, default_standalone_data_dir, server};
use tracing::info;

/// Arguments for the hub command.
pub struct HubArgs {
    pub project: Option<PathBuf>,
    pub no_project: bool,
    pub data_dir: Option<PathBuf>,
    pub port: u16,
    pub host: String,
    pub peers: Vec<String>,
    pub sync_interval: u64,
    pub no_watch: bool,
    pub watch_debounce: u64,
    pub oidc_client_id: Option<String>,
    pub oidc_issuer: String,
    pub oidc_image_domains: Vec<String>,
    pub behind_tls_proxy: bool,
    pub allow_insecure_auth: bool,
    pub allowed_emails: Option<Vec<String>>,
    pub allowed_domains: Option<Vec<String>>,
    /// Additional OAuth client IDs accepted as JWT audiences alongside
    /// `oidc_client_id` (Plan §Phase 2 — hub-mcp device flow).
    pub additional_audiences: Vec<String>,
    /// Unix domain socket transport (bd-kosgkxjt): `None` = TCP+WS;
    /// `Some(None)` = UDS at the convention path; `Some(Some(p))` = UDS
    /// at `p`. Exclusive — no TCP listener in UDS mode.
    pub socket_path: Option<Option<PathBuf>>,
    pub socket_group: Option<String>,
    pub socket_allow_uid: Vec<u32>,
}

/// Execute the hub command.
///
/// This starts a collaborative editing server for the given project.
/// The server provides:
/// - HTTP/WebSocket API for document synchronization
/// - Automerge-based CRDT document management
/// - Filesystem watching and sync (in project mode)
/// - Peering with remote sync servers
pub fn execute(args: HubArgs) -> Result<()> {
    // Build async runtime and run the server
    // We create a full tokio runtime (not pollster::block_on) because
    // the hub server needs multi-threaded async for websockets, file
    // watching, and peer connections.
    let runtime = tokio::runtime::Runtime::new()?;
    runtime.block_on(run_hub(args))
}

async fn run_hub(args: HubArgs) -> Result<()> {
    // Mode roots, needed both for storage and for resolving a bare
    // --socket-path to the convention location.
    let (project_root, standalone_data_dir) = if args.no_project {
        // Standalone mode: pure sync server
        if args.project.is_some() {
            anyhow::bail!("--no-project and --project are mutually exclusive");
        }
        (
            None,
            Some(
                args.data_dir
                    .clone()
                    .unwrap_or_else(default_standalone_data_dir),
            ),
        )
    } else {
        // Project mode (default for `quarto hub`): watch a local project
        if args.data_dir.is_some() {
            anyhow::bail!(
                "--data-dir requires --no-project. \
                 In project mode, data is stored in <project>/.quarto/hub/"
            );
        }
        let root = args
            .project
            .clone()
            .unwrap_or_else(|| std::env::current_dir().expect("Failed to get current directory"));
        (
            Some(
                quarto_system_runtime::canonicalize(&root)
                    .expect("Failed to canonicalize project root"),
            ),
            None,
        )
    };

    // UDS mode (bd-kosgkxjt): resolve and validate before storage
    // touches the disk, so a rejected combination fails fast.
    let socket_path = match &args.socket_path {
        None => None,
        Some(explicit) => {
            quarto_hub::unix_socket::validate_socket_platform().map_err(|e| anyhow::anyhow!(e))?;
            quarto_hub::unix_socket::validate_socket_args(true, args.oidc_client_id.as_deref())
                .map_err(|e| anyhow::anyhow!(e))?;
            let convention_dir = match (&project_root, &standalone_data_dir) {
                (Some(root), None) => root.join(".quarto").join("hub"),
                (None, Some(data_dir)) => data_dir.clone(),
                _ => unreachable!("mode roots are exclusive"),
            };
            let requested = explicit
                .clone()
                .unwrap_or_else(|| convention_dir.join("hub.sock"));
            let effective =
                quarto_hub::unix_socket::apply_sun_path_guard(&requested, &convention_dir)?;
            if effective != requested {
                info!(
                    requested = %requested.display(),
                    effective = %effective.display(),
                    "Socket path exceeds sun_path; using hashed fallback"
                );
            }
            Some(effective)
        }
    };

    // Initialize storage based on mode
    let mut storage = if let Some(project_root) = &project_root {
        info!(project_root = %project_root.display(), "Starting hub (project mode)");
        StorageManager::new(project_root)?
    } else {
        let data_dir = standalone_data_dir.as_ref().unwrap();
        info!(data_dir = %data_dir.display(), "Starting hub (standalone sync mode)");
        StorageManager::new_standalone(data_dir)?
    };

    // Determine peers: CLI peers override stored peers
    let peers = if !args.peers.is_empty() {
        // CLI peers provided - use them and persist
        storage.set_peers(args.peers.clone())?;
        info!(peers = ?args.peers, "Using peers from CLI (persisted to hub.json)");
        args.peers
    } else {
        // Use stored peers
        let stored_peers = storage.peers().to_vec();
        if !stored_peers.is_empty() {
            info!(peers = ?stored_peers, "Using peers from hub.json");
        }
        stored_peers
    };

    // Validate TLS configuration when auth is enabled
    auth::validate_tls_config(
        args.oidc_client_id.as_deref(),
        args.behind_tls_proxy,
        args.allow_insecure_auth,
    )
    .map_err(|e| anyhow::anyhow!(e))?;

    // Build auth config if OIDC client ID is provided
    let auth_config = args
        .oidc_client_id
        .map(|client_id| {
            auth::AuthConfig::new(
                client_id,
                args.additional_audiences,
                args.oidc_issuer,
                args.oidc_image_domains,
                args.allowed_emails,
                args.allowed_domains,
                args.allow_insecure_auth,
            )
        })
        .transpose()
        .map_err(|e| anyhow::anyhow!(e))?;

    // Configure and run server
    let sync_interval_secs = if args.sync_interval == 0 {
        None
    } else {
        Some(args.sync_interval)
    };

    let config = HubConfig {
        port: args.port,
        host: args.host,
        peers,
        sync_interval_secs,
        watch_enabled: !args.no_watch,
        watch_debounce_ms: args.watch_debounce,
        watch_filter: Default::default(),
        single_file: None,
        // Standalone `quarto hub` server: resources-scoped `.html` sync
        // is a preview-only concern (bd-kjrpya2d).
        resource_files: Vec::new(),
        // Single-file asset sync is a preview-only concern (bd-kpuweafo).
        single_file_assets: Vec::new(),
        single_file_text_deps: Vec::new(),
        auth_config,
        allow_insecure_auth: args.allow_insecure_auth,
        // `quarto hub` keeps the sync.automerge.org-style `/` ws path
        // for upstream compatibility. `quarto preview` will set this
        // false so its SPA can own `/`.
        register_root_ws: true,
        // The collaborative hub always persists document changes to disk.
        disk_write_policy: quarto_hub::sync::DiskWritePolicy::WriteBack,
        // Long-running server: shutdown acknowledgment stays in the
        // tracing log, nothing user-facing on stdout.
        shutdown_message: None,
        socket_path,
        socket_group: args.socket_group.clone(),
        socket_allow_uid: args.socket_allow_uid.clone(),
    };

    server::run_server(storage, config).await?;

    Ok(())
}
