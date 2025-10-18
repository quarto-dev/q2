//! Quarto CLI - Main entry point

use anyhow::Result;
use clap::{Parser, Subcommand};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod commands;

#[derive(Parser)]
#[command(name = "quarto")]
#[command(version = quarto_util::cli_version())]
#[command(about = "Quarto CLI", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Render files or projects to various document types
    Render {
        /// Input file or project
        input: Option<String>,

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

        /// Active project profile(s)
        #[arg(long)]
        profile: Vec<String>,

        /// Additional pandoc command line arguments
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        pandoc_args: Vec<String>,
    },

    /// Render and preview a document or website project
    Preview {
        /// File or project to preview
        file: Option<String>,

        /// Suggested port to listen on (defaults to random value between 3000 and 8000)
        #[arg(long)]
        port: Option<u16>,

        /// Hostname to bind to (defaults to 127.0.0.1)
        #[arg(long)]
        host: Option<String>,

        /// Render to the specified format(s) before previewing
        #[arg(long, default_value = "none")]
        render: String,

        /// Don't run a local preview web server (just monitor and re-render input files)
        #[arg(long)]
        no_serve: bool,

        /// Don't navigate the browser automatically when outputs are updated
        #[arg(long)]
        no_navigate: bool,

        /// Don't open a browser to preview the site
        #[arg(long)]
        no_browser: bool,

        /// Do not re-render input files when they change
        #[arg(long)]
        no_watch_inputs: bool,

        /// Time (in seconds) after which to exit if there are no active clients
        #[arg(long)]
        timeout: Option<u32>,

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

        /// Active project profile(s)
        #[arg(long)]
        profile: Vec<String>,

        /// Additional arguments to forward to quarto render
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        render_args: Vec<String>,
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
        /// Provider to publish to
        provider: Option<String>,

        /// Path to publish
        path: Option<String>,
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
}

fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "quarto=info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Render { .. } => commands::render::execute(),
        Commands::Preview { .. } => commands::preview::execute(),
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
        Commands::Publish { .. } => commands::publish::execute(),
        Commands::Check { .. } => commands::check::execute(),
        Commands::Call { .. } => commands::call::execute(),
    }
}
