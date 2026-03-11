//! Hub binary - standalone collaborative sync server for Quarto projects
//!
//! By default, runs as a standalone sync server (no local project).
//! Use `--project <path>` to watch a local Quarto project directory.

use std::path::PathBuf;

use clap::Parser;
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use quarto_hub::{StorageManager, auth, context::HubConfig, default_standalone_data_dir, server};

#[derive(Parser, Debug)]
#[command(name = "hub")]
#[command(about = "Collaborative sync server for Quarto projects")]
struct Args {
    /// Watch a local Quarto project directory.
    /// When provided, the hub discovers and syncs files from this directory.
    /// When omitted, the hub runs as a standalone sync server.
    #[arg(short, long)]
    project: Option<PathBuf>,

    /// Data directory for standalone mode (where automerge documents are stored).
    /// Defaults to `.quarto/hub/` inside the project when --project is used.
    /// Required when running without --project unless the default location is acceptable.
    #[arg(long, env = "QUARTO_HUB_DATA_DIR")]
    data_dir: Option<PathBuf>,

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
    /// Default: 30 seconds. Only relevant in project mode.
    #[arg(long, default_value = "30")]
    sync_interval: u64,

    /// Disable filesystem watching.
    /// When disabled, file changes won't be detected until periodic sync runs.
    /// Only relevant in project mode.
    #[arg(long)]
    no_watch: bool,

    /// Debounce duration for filesystem events in milliseconds.
    /// Default: 500ms. Only relevant in project mode.
    #[arg(long, default_value = "500")]
    watch_debounce: u64,

    /// OIDC client ID. Presence enables auth.
    /// Requires --behind-tls-proxy (or --allow-insecure-auth for local dev).
    #[arg(long, env = "OIDC_CLIENT_ID")]
    oidc_client_id: Option<String>,

    /// OIDC issuer URL for JWT validation.
    /// The JWKS URL is discovered automatically from {issuer}/.well-known/openid-configuration.
    #[arg(
        long,
        env = "OIDC_ISSUER",
        default_value = "https://accounts.google.com"
    )]
    oidc_issuer: String,

    /// Comma-separated domains allowed in CSP img-src for profile pictures.
    #[arg(
        long,
        env = "OIDC_IMAGE_DOMAINS",
        value_delimiter = ',',
        default_value = "lh3.googleusercontent.com"
    )]
    oidc_image_domains: Vec<String>,

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
    /// Note: relies on the OIDC provider's `email_verified` claim.
    /// Ensure your provider verifies email ownership before trusting domain-based access.
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

    // Initialize storage based on mode
    let mut storage = if let Some(project) = &args.project {
        // Project mode: watch a local Quarto project
        let project_root = project
            .canonicalize()
            .expect("Failed to canonicalize project root");

        if args.data_dir.is_some() {
            anyhow::bail!(
                "--data-dir and --project are mutually exclusive. \
                 In project mode, data is stored in <project>/.quarto/hub/"
            );
        }

        info!(project_root = %project_root.display(), "Starting hub (project mode)");
        StorageManager::new(&project_root)?
    } else {
        // Standalone mode: pure sync server
        let data_dir = args
            .data_dir
            .clone()
            .unwrap_or_else(default_standalone_data_dir);

        info!(data_dir = %data_dir.display(), "Starting hub (standalone sync mode)");
        StorageManager::new_standalone(&data_dir)?
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
                args.oidc_issuer,
                args.oidc_image_domains,
                args.allowed_emails,
                args.allowed_domains,
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
        auth_config,
        allow_insecure_auth: args.allow_insecure_auth,
    };

    server::run_server(storage, config).await?;

    Ok(())
}
