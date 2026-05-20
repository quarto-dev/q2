//! Quarto CLI - Main entry point

use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use quarto_core::attribution::AttributionMode;

mod commands;

#[derive(Parser)]
#[command(name = "quarto")]
#[command(version = quarto_util::cli_version())]
#[command(about = "Quarto CLI", long_about = None)]
struct Cli {
    /// Increase log verbosity. Repeat for more detail:
    /// `-v` adds info, `-vv` adds debug + samod=info,
    /// `-vvv` adds trace + samod=debug + tower_http=debug.
    /// `RUST_LOG` overrides this flag entirely when set.
    #[arg(short = 'v', long = "verbose", global = true, action = clap::ArgAction::Count)]
    verbose: u8,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Render files or projects to various document types
    Render {
        /// Input files, directories, or project root. Zero arguments
        /// means "render the project rooted at the current working
        /// directory." Multiple arguments are required to share a
        /// single project (one project per render).
        inputs: Vec<String>,

        /// Specify output format(s)
        #[arg(short = 't', long)]
        to: Option<String>,

        /// Write output to FILE (use '--output -' for stdout)
        #[arg(short = 'o', long)]
        output: Option<String>,

        /// Write output to DIR (path is input/project relative)
        #[arg(long)]
        output_dir: Option<String>,

        /// Metadata value (KEY:VALUE)
        #[arg(short = 'M', long)]
        metadata: Vec<String>,

        /// Override site-url for website or book output
        #[arg(long)]
        site_url: Option<String>,

        /// Execute code (--no-execute to skip execution)
        #[arg(long)]
        execute: bool,

        /// Execution parameter (KEY:VALUE)
        #[arg(short = 'P', long)]
        execute_param: Vec<String>,

        /// YAML file with execution parameters
        #[arg(long)]
        execute_params: Option<String>,

        /// Working directory for code execution
        #[arg(long)]
        execute_dir: Option<String>,

        /// Keep Jupyter kernel alive (defaults to 300 seconds)
        #[arg(long)]
        execute_daemon: Option<u32>,

        /// Restart keepalive Jupyter kernel before render
        #[arg(long)]
        execute_daemon_restart: bool,

        /// Show debug output when executing computations
        #[arg(long)]
        execute_debug: bool,

        /// Force use of frozen computations for an incremental file render
        #[arg(long)]
        use_freezer: bool,

        /// Cache execution output (--no-cache to prevent cache)
        #[arg(long)]
        cache: bool,

        /// Force refresh of execution cache
        #[arg(long)]
        cache_refresh: bool,

        /// Do not clean project output-dir prior to render
        #[arg(long)]
        no_clean: bool,

        /// Wipe the Pass-1 profile cache (`<project>/.quarto/cache/profiles/`)
        /// and `nav-config-hash` before rendering. Preserves the SCSS
        /// `sass/` cache. (Phase 8.)
        #[arg(long)]
        clean_cache: bool,

        /// Leave intermediate files in place after render
        #[arg(long)]
        debug: bool,

        /// Path to log file
        #[arg(long)]
        log: Option<String>,

        /// Log level (debug, info, warning, error, critical)
        #[arg(long)]
        log_level: Option<String>,

        /// Log format (plain, json-stream)
        #[arg(long)]
        log_format: Option<String>,

        /// Suppress console output
        #[arg(long)]
        quiet: bool,

        /// Replay engine output from a recorded trace file
        /// (`<trace>.json`) instead of running the real engine
        /// (bd-45yw). Useful for reproducing reported bugs and
        /// running CI regression tests without R/Python/Jupyter.
        /// Hard-fails if the document's content does not match the
        /// recorded input.
        ///
        /// Also activated by `QUARTO_REPLAY=<trace>` if the flag is
        /// not set.
        #[arg(long, value_name = "TRACE")]
        replay: Option<String>,

        /// Active project profile(s)
        #[arg(long)]
        profile: Vec<String>,

        /// Additional pandoc command line arguments. Pass after `--`
        /// so they don't collide with `inputs`. Example:
        /// `quarto render index.qmd -- --metadata foo=bar`.
        #[arg(last = true, allow_hyphen_values = true)]
        pandoc_args: Vec<String>,

        /// Annotate output with per-author attribution.
        /// `--attribution=git` shells out to `git blame`; `--attribution=off`
        /// disables attribution even when the YAML opts in.
        #[arg(long, value_enum)]
        attribution: Option<AttributionMode>,
    },

    /// Start a live preview of a Quarto document or project.
    ///
    /// Watches the project for changes and re-renders the active
    /// page incrementally. The preview keeps its JavaScript state
    /// (open menus, scroll position, math rendering, listings
    /// filters) across edits — only the parts of the page that
    /// actually changed are updated.
    ///
    /// Engine execution (Jupyter, knitr) is controlled by the
    /// `preview.engine` key in `_quarto.yml`. Setting it to `manual`
    /// (the default) makes the server detect when code has changed
    /// and show a "Re-execute" button; `auto` re-executes on every
    /// settled code edit; `off` skips engine execution entirely, so
    /// code cells render as inert source.
    ///
    /// Press Ctrl-C to stop the server.
    Preview {
        /// File or project root to preview.
        ///
        /// If you pass a file path inside a project (one with
        /// `_quarto.yml` somewhere up the tree), the whole project
        /// is loaded and the preview opens to that page. If you
        /// pass a directory, the preview opens to `index.qmd` when
        /// present, otherwise the first `.qmd` it finds. Defaults
        /// to the current directory.
        path: Option<std::path::PathBuf>,

        /// Port to listen on. Defaults to a random free port. Pass
        /// `--port 0` to explicitly request an OS-assigned port.
        #[arg(long)]
        port: Option<u16>,

        /// Network interface to bind to. Defaults to `127.0.0.1`
        /// (loopback-only — the preview is not reachable from
        /// other machines on your network).
        #[arg(long)]
        host: Option<String>,

        /// Don't open a browser tab on startup. The URL is still
        /// printed on stdout for copy-paste.
        #[arg(long)]
        no_browser: bool,

        /// Override the directory the preview uses for ephemeral
        /// per-session state. Default: a fresh tempdir that is
        /// deleted when `q2 preview` exits.
        #[arg(long)]
        data_dir: Option<std::path::PathBuf>,

        /// Override the embedded preview UI with a copy on disk —
        /// useful when you're iterating on the preview UI itself.
        /// Most users never need this.
        #[arg(long)]
        preview_dir: Option<std::path::PathBuf>,

        /// Run as a bare sync server with no local project. The
        /// opposite of the default project-mode boot.
        #[arg(long)]
        no_project: bool,
    },

    /// Serve a Shiny interactive document
    Serve {
        /// Input file to serve
        input: Option<String>,

        /// Port to listen on
        #[arg(long)]
        port: Option<u16>,

        /// Host to bind to
        #[arg(long)]
        host: Option<String>,
    },

    /// Create a Quarto project or extension
    Create {
        /// Type of project or extension to create
        #[arg(value_name = "TYPE")]
        type_: Option<String>,

        /// Additional arguments
        #[arg(trailing_var_arg = true)]
        args: Vec<String>,
    },

    /// Automate document or project setup tasks
    Use {
        /// Type of setup task
        #[arg(value_name = "TYPE")]
        type_: String,

        /// Target for the setup task
        target: Option<String>,
    },

    /// Add an extension to this folder or project
    Add {
        /// Extension to add
        extension: String,
    },

    /// Updates an extension or global dependency
    Update {
        /// Targets to update
        #[arg(trailing_var_arg = true)]
        target: Vec<String>,
    },

    /// Removes an extension
    Remove {
        /// Targets to remove
        #[arg(trailing_var_arg = true)]
        target: Vec<String>,
    },

    /// Convert documents to alternate representations
    Convert {
        /// Input file to convert
        input: String,

        /// Output format
        #[arg(long)]
        output: Option<String>,
    },

    /// Run the version of Pandoc embedded within Quarto
    Pandoc {
        /// Arguments to pass to pandoc
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },

    /// Run the version of Typst embedded within Quarto
    Typst {
        /// Arguments to pass to typst
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },

    /// Run a TypeScript, R, Python, or Lua script
    Run {
        /// Script to run
        script: Option<String>,

        /// Arguments to pass to the script
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },

    /// Lists an extension or global dependency
    List {
        /// Type of item to list
        #[arg(value_name = "TYPE")]
        type_: Option<String>,
    },

    /// Installs a global dependency (TinyTex or Chromium)
    Install {
        /// Targets to install
        #[arg(trailing_var_arg = true)]
        target: Vec<String>,
    },

    /// Removes an extension
    Uninstall {
        /// Tool to uninstall
        tool: Option<String>,
    },

    /// Display the status of Quarto installed dependencies
    Tools,

    /// Publish a document or project to a provider
    Publish {
        /// Provider to publish to (e.g. `gh-pages`)
        provider: Option<String>,

        /// Path to publish (defaults to current directory)
        path: Option<String>,

        /// Do not render before publishing
        #[arg(long = "no-render", action = clap::ArgAction::SetTrue)]
        no_render: bool,

        /// Do not prompt for input (errors if input is required)
        #[arg(long = "no-prompt", action = clap::ArgAction::SetTrue)]
        no_prompt: bool,

        /// Do not open the browser to the published URL
        #[arg(long = "no-browser", action = clap::ArgAction::SetTrue)]
        no_browser: bool,

        /// Do not wait for the deployment to be live (incompatible
        /// with --browser; pass --no-browser too)
        #[arg(long = "no-wait", action = clap::ArgAction::SetTrue)]
        no_wait: bool,

        /// Run prepare + render but do not push or upload anything
        #[arg(long = "dry-run", action = clap::ArgAction::SetTrue)]
        dry_run: bool,

        /// Emit machine-readable output (implies --no-prompt;
        /// final PublishOutcome on stdout, NDJSON events on stderr)
        #[arg(long, action = clap::ArgAction::SetTrue)]
        json: bool,
    },

    /// Verify correct functioning of Quarto installation
    Check {
        /// Target to check
        target: Option<String>,
    },

    /// Access functions of Quarto subsystems such as its rendering engines
    Call {
        /// Function to call
        function: Option<String>,

        /// Arguments for the function
        #[arg(trailing_var_arg = true)]
        args: Vec<String>,
    },

    /// Start the Quarto Language Server Protocol server
    Lsp,

    /// Inspect pipeline execution traces under `.quarto/trace/`.
    ///
    /// Output is always JSON. Pipe to `jq`/`fx` for pretty filtering, or
    /// use `quarto trace view` (future) for an interactive SPA.
    Trace {
        #[command(subcommand)]
        command: TraceCommand,
    },

    /// Start collaborative hub server for real-time editing.
    /// By default, watches the current directory (or --project path).
    /// Use --no-project to run as a standalone sync server.
    Hub {
        /// Project root directory (defaults to current directory).
        /// Mutually exclusive with --no-project.
        #[arg(short, long)]
        project: Option<PathBuf>,

        /// Run as a standalone sync server without watching a local project.
        /// Mutually exclusive with --project.
        #[arg(long)]
        no_project: bool,

        /// Data directory for standalone mode (where automerge documents are stored).
        /// Only used with --no-project.
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
        #[arg(long, default_value = "30")]
        sync_interval: u64,

        /// Disable filesystem watching.
        /// When disabled, file changes won't be detected until periodic sync runs.
        #[arg(long)]
        no_watch: bool,

        /// Debounce duration for filesystem events in milliseconds.
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
        /// `--oidc-client-id`. Used to share a hub between the SPA and
        /// quarto-hub-mcp (whose Google device-flow tokens carry a
        /// different `aud`). Exact matches only; no wildcards.
        /// Plan §Phase 2 — claude-notes/plans/2026-05-05-hub-mcp-device-flow-implementation.md.
        #[arg(long, env = "QUARTO_HUB_ADDITIONAL_AUDIENCES", value_delimiter = ',')]
        additional_audiences: Vec<String>,
    },
}

#[derive(Subcommand)]
enum TraceCommand {
    /// List available traces under the `.quarto/trace/` directory.
    List {
        /// Override the trace directory (defaults to `./.quarto/trace/`).
        #[arg(long)]
        trace_dir: Option<PathBuf>,
    },

    /// Print a trace or a single stage entry as JSON.
    Show {
        /// Override the trace directory (defaults to `./.quarto/trace/`).
        #[arg(long)]
        trace_dir: Option<PathBuf>,

        /// Document stem to show. If omitted and exactly one trace exists,
        /// that trace is used.
        #[arg(long)]
        doc: Option<String>,

        /// Print only the entry for this stage name.
        #[arg(long)]
        stage: Option<String>,
    },

    /// Launch the trace viewer SPA on a local HTTP server.
    View {
        /// Override the trace directory (defaults to `./.quarto/trace/`).
        #[arg(long)]
        trace_dir: Option<PathBuf>,

        /// Document stem to open on startup.
        #[arg(long)]
        doc: Option<String>,

        /// Port to bind the local server to. `0` (default) picks an OS-assigned port.
        #[arg(long)]
        port: Option<u16>,

        /// Host to bind to. Defaults to `127.0.0.1`.
        #[arg(long)]
        host: Option<String>,

        /// Don't attempt to open the default browser on startup.
        #[arg(long)]
        no_browser: bool,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    // Initialize logging. The `-v` flag chooses a default filter
    // directive (see `quarto_util::verbose_to_filter`); `RUST_LOG`,
    // when set, takes precedence and is parsed directly by
    // `try_from_default_env`. This matches the long-standing
    // convention in the workspace.
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| quarto_util::verbose_to_filter(cli.verbose).into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    match cli.command {
        Commands::Render {
            inputs,
            to,
            output,
            output_dir,
            clean_cache,
            quiet,
            replay,
            debug,
            attribution,
            ..
        } => commands::render::execute(commands::render::RenderArgs {
            inputs,
            to,
            output,
            output_dir,
            clean_cache,
            quiet,
            replay,
            debug,
            attribution,
        }),
        Commands::Preview {
            path,
            port,
            host,
            no_browser,
            data_dir,
            preview_dir,
            no_project,
        } => commands::preview::execute(commands::preview::PreviewArgs {
            path,
            port,
            host,
            no_browser,
            data_dir,
            preview_dir,
            no_project,
        }),
        Commands::Serve { .. } => commands::serve::execute(),
        Commands::Create { .. } => commands::create::execute(),
        Commands::Use { .. } => commands::use_cmd::execute(),
        Commands::Add { .. } => commands::add::execute(),
        Commands::Update { .. } => commands::update::execute(),
        Commands::Remove { .. } => commands::remove::execute(),
        Commands::Convert { .. } => commands::convert::execute(),
        Commands::Pandoc { .. } => commands::pandoc::execute(),
        Commands::Typst { .. } => commands::typst::execute(),
        Commands::Run { .. } => commands::run::execute(),
        Commands::List { .. } => commands::list::execute(),
        Commands::Install { .. } => commands::install::execute(),
        Commands::Uninstall { .. } => commands::uninstall::execute(),
        Commands::Tools => commands::tools::execute(),
        Commands::Publish {
            provider,
            path,
            no_render,
            no_prompt,
            no_browser,
            no_wait,
            dry_run,
            json,
        } => commands::publish::execute(commands::publish::PublishArgs {
            provider,
            path,
            no_render,
            no_prompt,
            no_browser,
            no_wait,
            dry_run,
            json,
        }),
        Commands::Check { .. } => commands::check::execute(),
        Commands::Call { function, args } => commands::call::execute(function, args),
        Commands::Lsp => commands::lsp::execute(),
        Commands::Hub {
            project,
            no_project,
            data_dir,
            port,
            host,
            peers,
            sync_interval,
            no_watch,
            watch_debounce,
            oidc_client_id,
            oidc_issuer,
            oidc_image_domains,
            behind_tls_proxy,
            allow_insecure_auth,
            allowed_emails,
            allowed_domains,
            additional_audiences,
        } => commands::hub::execute(commands::hub::HubArgs {
            project,
            no_project,
            data_dir,
            port,
            host,
            peers,
            sync_interval,
            no_watch,
            watch_debounce,
            oidc_client_id,
            oidc_issuer,
            oidc_image_domains,
            behind_tls_proxy,
            allow_insecure_auth,
            allowed_emails,
            allowed_domains,
            additional_audiences,
        }),
        Commands::Trace { command } => match command {
            TraceCommand::List { trace_dir } => {
                commands::trace::execute_list(commands::trace::TraceListArgs { trace_dir })
            }
            TraceCommand::Show {
                trace_dir,
                doc,
                stage,
            } => commands::trace::execute_show(commands::trace::TraceShowArgs {
                trace_dir,
                doc,
                stage,
            }),
            TraceCommand::View {
                trace_dir,
                doc,
                port,
                host,
                no_browser,
            } => commands::trace::execute_view(commands::trace::TraceViewArgs {
                trace_dir,
                doc,
                port,
                host,
                no_browser,
            }),
        },
    }
}
