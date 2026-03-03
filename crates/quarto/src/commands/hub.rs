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
    pub google_client_id: Option<String>,
    pub behind_tls_proxy: bool,
    pub allow_insecure_auth: bool,
    pub allowed_emails: Option<Vec<String>>,
    pub allowed_domains: Option<Vec<String>>,
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
    // Initialize storage based on mode
    let mut storage = if args.no_project {
        // Standalone mode: pure sync server
        if args.project.is_some() {
            anyhow::bail!("--no-project and --project are mutually exclusive");
        }
        let data_dir = args
            .data_dir
            .clone()
            .unwrap_or_else(default_standalone_data_dir);
        info!(data_dir = %data_dir.display(), "Starting hub (standalone sync mode)");
        StorageManager::new_standalone(&data_dir)?
    } else {
        // Project mode (default for `quarto hub`): watch a local project
        if args.data_dir.is_some() {
            anyhow::bail!(
                "--data-dir requires --no-project. \
                 In project mode, data is stored in <project>/.quarto/hub/"
            );
        }
        let project_root = args
            .project
            .unwrap_or_else(|| std::env::current_dir().expect("Failed to get current directory"));
        let project_root = project_root
            .canonicalize()
            .expect("Failed to canonicalize project root");

        info!(project_root = %project_root.display(), "Starting hub (project mode)");
        StorageManager::new(&project_root)?
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
        args.google_client_id.as_deref(),
        args.behind_tls_proxy,
        args.allow_insecure_auth,
    )
    .map_err(|e| anyhow::anyhow!(e))?;

    // Build auth config if Google client ID is provided
    let auth_config = args.google_client_id.map(|client_id| auth::AuthConfig {
        client_id,
        allowed_emails: args.allowed_emails,
        allowed_domains: args.allowed_domains,
    });

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
        auth_config,
        allow_insecure_auth: args.allow_insecure_auth,
    };

    server::run_server(storage, config).await?;

    Ok(())
}
