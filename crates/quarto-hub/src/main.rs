//! Hub binary - collaborative editing server for Quarto projects

use std::path::PathBuf;

use clap::Parser;
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use quarto_hub::{StorageManager, context::HubConfig, server};

#[derive(Parser, Debug)]
#[command(name = "hub")]
#[command(about = "Collaborative editing server for Quarto projects")]
struct Args {
    /// Project root directory (defaults to current directory)
    #[arg(short, long)]
    project: Option<PathBuf>,

    /// Port to listen on
    #[arg(short = 'P', long, default_value = "3000")]
    port: u16,

    /// Host to bind to
    #[arg(short = 'H', long, default_value = "127.0.0.1")]
    host: String,

    /// Sync server URL to peer with (can be specified multiple times).
    /// Example: --peer wss://sync.automerge.org
    /// Peers are persisted to hub.json and used on subsequent runs.
    #[arg(long = "peer", value_name = "URL")]
    peers: Vec<String>,

    /// Periodic filesystem sync interval in seconds.
    /// Set to 0 to disable periodic sync.
    /// Default: 30 seconds.
    #[arg(long, default_value = "30")]
    sync_interval: u64,

    /// Disable filesystem watching.
    /// When disabled, file changes won't be detected until periodic sync runs.
    #[arg(long)]
    no_watch: bool,

    /// Debounce duration for filesystem events in milliseconds.
    /// Default: 500ms.
    #[arg(long, default_value = "500")]
    watch_debounce: u64,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "quarto_hub=info,tower_http=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let args = Args::parse();

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
