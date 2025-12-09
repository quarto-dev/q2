//! Hub binary - collaborative editing server for Quarto projects

use std::path::PathBuf;

use clap::Parser;
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use quarto_hub::{context::HubConfig, server, StorageManager};

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

    // Determine project root
    let project_root = args
        .project
        .unwrap_or_else(|| std::env::current_dir().expect("Failed to get current directory"));

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
    let config = HubConfig {
        port: args.port,
        host: args.host,
        peers,
    };

    server::run_server(storage, config).await?;

    Ok(())
}
