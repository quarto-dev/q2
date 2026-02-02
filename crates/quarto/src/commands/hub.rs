//! Hub command - collaborative editing server
//!
//! This command starts the Quarto Hub server, which provides real-time
//! collaborative editing for Quarto projects using Automerge CRDTs.

use std::path::PathBuf;

use anyhow::Result;
use quarto_hub::{StorageManager, context::HubConfig, server};
use tracing::info;

/// Arguments for the hub command.
pub struct HubArgs {
    pub project: Option<PathBuf>,
    pub port: u16,
    pub host: String,
    pub peers: Vec<String>,
    pub sync_interval: u64,
    pub no_watch: bool,
    pub watch_debounce: u64,
}

/// Execute the hub command.
///
/// This starts a collaborative editing server for the given project.
/// The server provides:
/// - HTTP/WebSocket API for document synchronization
/// - Automerge-based CRDT document management
/// - Filesystem watching and sync
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
    // Determine project root (canonicalize to ensure consistent paths for file watching)
    let project_root = args
        .project
        .unwrap_or_else(|| std::env::current_dir().expect("Failed to get current directory"));
    let project_root = project_root
        .canonicalize()
        .expect("Failed to canonicalize project root");

    info!(project_root = %project_root.display(), "Starting hub");

    // Initialize storage (acquires lockfile)
    let mut storage = StorageManager::new(&project_root)?;

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
    };

    server::run_server(storage, config).await?;

    Ok(())
}
