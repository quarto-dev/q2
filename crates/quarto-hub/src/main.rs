//! Hub binary - collaborative editing server for Quarto projects

use std::path::PathBuf;

use clap::Parser;
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use quarto_hub::{StorageManager, auth, context::HubConfig, server};

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

    /// Google OAuth2 client ID. Presence enables auth.
    /// Requires --behind-tls-proxy (or --allow-insecure-auth for local dev).
    #[arg(long, env = "QUARTO_HUB_GOOGLE_CLIENT_ID")]
    google_client_id: Option<String>,

    /// Acknowledge that a TLS-terminating reverse proxy (nginx, Caddy,
    /// cloud LB) sits in front of the hub. Required when auth is enabled.
    #[arg(long)]
    behind_tls_proxy: bool,

    /// Allow auth without TLS (local development only). Tokens will
    /// transit in plaintext — never use this in production.
    #[arg(long)]
    allow_insecure_auth: bool,

    /// Allowed email addresses (comma-separated).
    #[arg(long, env = "QUARTO_HUB_ALLOWED_EMAILS", value_delimiter = ',')]
    allowed_emails: Option<Vec<String>>,

    /// Allowed email domains (comma-separated).
    #[arg(long, env = "QUARTO_HUB_ALLOWED_DOMAINS", value_delimiter = ',')]
    allowed_domains: Option<Vec<String>>,
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
