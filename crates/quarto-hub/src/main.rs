//! Hub binary - standalone collaborative sync server for Quarto projects
//!
//! By default, runs as a standalone sync server (no local project).
//! Use `--project <path>` to watch a local Quarto project directory.

use std::path::PathBuf;

use clap::{Parser, Subcommand};
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use quarto_hub::{StorageManager, auth, context::HubConfig, default_standalone_data_dir, server};

#[derive(Parser, Debug)]
#[command(name = "hub")]
#[command(about = "Collaborative sync server for Quarto projects")]
struct Args {
    /// Maintainer subcommands (`hub admin …`). When omitted, the hub
    /// runs as a server with the flags below — the pre-subcommand CLI
    /// is unchanged.
    #[command(subcommand)]
    command: Option<Command>,

    /// Increase log verbosity. Repeat for more detail:
    /// `-v` adds info, `-vv` adds debug + samod=info,
    /// `-vvv` adds trace + samod=debug + tower_http=debug.
    /// `RUST_LOG` overrides this flag entirely when set.
    #[arg(short = 'v', long = "verbose", action = clap::ArgAction::Count)]
    verbose: u8,

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

    /// Additional OAuth client IDs accepted as JWT audiences alongside
    /// `--oidc-client-id`. The primary use is sharing a hub between the
    /// SPA (whose Google ID token's `aud = --oidc-client-id`) and
    /// quarto-hub-mcp (whose Google device-flow ID token's `aud` is the
    /// hub-mcp client_id registered separately as "TV and Limited Input
    /// devices"). Comma-separated; exact matches only — no wildcards.
    /// Plan §Phase 2 — `claude-notes/plans/2026-05-05-hub-mcp-device-flow-implementation.md`.
    #[arg(long, env = "QUARTO_HUB_ADDITIONAL_AUDIENCES", value_delimiter = ',')]
    additional_audiences: Vec<String>,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Sync-server maintainer tools (storage hygiene, bd-eiku4ymo).
    /// See claude-notes/instructions/hub-storage-hygiene.md.
    Admin {
        #[command(subcommand)]
        cmd: AdminCommand,
    },
}

#[derive(Subcommand, Debug)]
enum AdminCommand {
    /// Read-only orphan analysis: inventory every stored doc and emit
    /// a manifest of safely-removable engine-capture docs. Safe to run
    /// against a live server.
    Scan {
        /// Hub data directory (contains `automerge/` and `hub.lock`).
        #[arg(long)]
        data_dir: PathBuf,
        /// Only captures older than this many days are candidates.
        /// Negative values (e.g. `--older-than-days=-1`) disable the
        /// age gate — every orphaned stamped capture qualifies.
        #[arg(long, default_value_t = 30, allow_hyphen_values = true)]
        older_than_days: i64,
        /// Also consider captures without a `meta.createdAt` stamp
        /// (recorded before the audit envelope existed).
        #[arg(long)]
        include_unstamped: bool,
        /// Write the manifest JSON here (the input `collect` takes).
        #[arg(long)]
        output: Option<PathBuf>,
        /// Print the manifest JSON to stdout instead of the summary.
        #[arg(long)]
        json: bool,
    },
    /// Quarantine the docs a scan manifest deems removable, after
    /// re-verifying each against current storage. Moves doc chunks to
    /// `<data-dir>/trash/<batch>/`; never deletes. Dry-run unless
    /// --execute. Refuses while a server holds the data dir.
    Collect {
        #[arg(long)]
        data_dir: PathBuf,
        /// Manifest produced by `hub admin scan --output`.
        #[arg(long)]
        manifest: PathBuf,
        /// Actually quarantine (default is a dry-run report).
        #[arg(long)]
        execute: bool,
    },
    /// Move a quarantined batch (or named docs from it) back into the
    /// store, verifying chunk hashes recorded at collection time.
    Restore {
        #[arg(long)]
        data_dir: PathBuf,
        /// Batch directory under `<data-dir>/trash/`.
        #[arg(long)]
        batch: PathBuf,
        /// Restore only these doc ids (default: the whole batch).
        doc_ids: Vec<String>,
    },
    /// Delete trash batches older than the retention window. The only
    /// operation that permanently removes bytes. Dry-run unless
    /// --execute.
    Purge {
        #[arg(long)]
        data_dir: PathBuf,
        /// Negative values disable the retention gate.
        #[arg(long, default_value_t = 30, allow_hyphen_values = true)]
        retention_days: i64,
        /// Actually delete eligible batches (default: list them).
        #[arg(long)]
        execute: bool,
    },
}

/// Dispatch `hub admin …`. Exits the process with a non-zero status
/// on failure so scripts can gate on it.
async fn run_admin(cmd: AdminCommand) -> anyhow::Result<()> {
    use quarto_hub::admin::{collect as collect_mod, scan as scan_mod};
    match cmd {
        AdminCommand::Scan {
            data_dir,
            older_than_days,
            include_unstamped,
            output,
            json,
        } => {
            let canonical = data_dir
                .canonicalize()
                .map_err(|e| anyhow::anyhow!("cannot canonicalize {}: {e}", data_dir.display()))?;
            let automerge_dir = canonical.join("automerge");
            if !automerge_dir.is_dir() {
                anyhow::bail!(
                    "{} has no automerge/ directory — is this a hub data dir?",
                    canonical.display()
                );
            }
            let storage = samod::storage::TokioFilesystemStorage::new(&automerge_dir);
            let doc_ids = scan_mod::list_doc_ids_filesystem(&automerge_dir)?;
            let manifest = scan_mod::scan(
                &storage,
                &doc_ids,
                &canonical.to_string_lossy(),
                &scan_mod::ScanOptions {
                    older_than_days,
                    include_unstamped,
                },
            )
            .await;
            if let Some(path) = &output {
                std::fs::write(path, serde_json::to_vec_pretty(&manifest)?)?;
                eprintln!("Manifest written to {}", path.display());
            }
            if json {
                println!("{}", serde_json::to_string_pretty(&manifest)?);
            } else {
                println!("{}", scan_mod::human_summary(&manifest));
            }
            Ok(())
        }
        AdminCommand::Collect {
            data_dir,
            manifest,
            execute,
        } => {
            let manifest: quarto_hub::admin::manifest::ScanManifest =
                serde_json::from_slice(&std::fs::read(&manifest)?)?;
            let outcome = collect_mod::collect(&data_dir, &manifest, execute)
                .await
                .map_err(|e| anyhow::anyhow!(e))?;
            for (doc_id, reason) in &outcome.skipped {
                println!("SKIP {doc_id}: {reason}");
            }
            for c in &outcome.verified {
                println!(
                    "{} {} ({} bytes)",
                    if execute {
                        "QUARANTINED"
                    } else {
                        "WOULD COLLECT"
                    },
                    c.doc_id,
                    c.size_bytes
                );
            }
            match &outcome.batch_dir {
                Some(dir) => println!(
                    "Batch: {} (restore with `hub admin restore --data-dir {} --batch {}`)",
                    dir.display(),
                    data_dir.display(),
                    dir.display()
                ),
                None if !execute && !outcome.verified.is_empty() => {
                    println!("Dry-run only. Re-run with --execute to quarantine.");
                }
                None => {}
            }
            Ok(())
        }
        AdminCommand::Restore {
            data_dir,
            batch,
            doc_ids,
        } => {
            let results = collect_mod::restore(&data_dir, &batch, &doc_ids)
                .map_err(|e| anyhow::anyhow!(e))?;
            let mut failed = false;
            for (doc_id, result) in &results {
                match result {
                    Ok(()) => println!("RESTORED {doc_id}"),
                    Err(reason) => {
                        failed = true;
                        println!("FAILED {doc_id}: {reason}");
                    }
                }
            }
            if failed {
                anyhow::bail!("some docs were not restored");
            }
            Ok(())
        }
        AdminCommand::Purge {
            data_dir,
            retention_days,
            execute,
        } => {
            let candidates = collect_mod::purge(&data_dir, retention_days, execute)
                .map_err(|e| anyhow::anyhow!(e))?;
            if candidates.is_empty() {
                println!("No trash batches.");
            }
            for c in &candidates {
                println!(
                    "{} {} (created {:?}, age {:?} days)",
                    match (c.eligible, execute) {
                        (true, true) => "PURGED",
                        (true, false) => "WOULD PURGE",
                        (false, _) => "KEPT",
                    },
                    c.batch_dir.display(),
                    c.created_at.as_deref().unwrap_or("<no batch.json>"),
                    c.age_days,
                );
            }
            Ok(())
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    // `hub admin …`: maintainer tools, no server startup.
    if let Some(Command::Admin { cmd }) = args.command {
        tracing_subscriber::registry()
            .with(
                tracing_subscriber::EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| quarto_util::verbose_to_filter(args.verbose).into()),
            )
            .with(tracing_subscriber::fmt::layer())
            .init();
        return run_admin(cmd).await;
    }

    // Initialize tracing. `-v` chooses a default filter directive (see
    // `quarto_util::verbose_to_filter`); `RUST_LOG`, when set, takes
    // precedence and is parsed directly by `try_from_default_env`.
    // This shares its mapping with the `q2` root command so both
    // binaries agree on what each verbosity level means.
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| quarto_util::verbose_to_filter(args.verbose).into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

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
        // Standalone `hub` server: resources-scoped `.html` sync is a
        // preview-only concern (bd-kjrpya2d).
        resource_files: Vec::new(),
        // Single-file asset sync is a preview-only concern (bd-kpuweafo).
        single_file_assets: Vec::new(),
        single_file_text_deps: Vec::new(),
        auth_config,
        allow_insecure_auth: args.allow_insecure_auth,
        register_root_ws: true,
        // The collaborative hub always persists document changes to disk.
        disk_write_policy: quarto_hub::sync::DiskWritePolicy::WriteBack,
        // Long-running server: shutdown acknowledgment stays in the
        // tracing log, nothing user-facing on stdout.
        shutdown_message: None,
    };

    server::run_server(storage, config).await?;

    Ok(())
}
