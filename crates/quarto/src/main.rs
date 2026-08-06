//! Quarto CLI - Main entry point

use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use quarto_core::attribution::AttributionMode;

mod commands;

#[derive(Parser)]
// "q2" is the actual binary name (and what usage/help should show);
// "(quarto 2)" in the version string disambiguates from TS Quarto, so
// `q2 --version` prints "q2 (quarto 2) 0.1.0" (bd-qyjsncfx). The LAST
// token must stay the bare version — release.yml's verify step parses it.
#[command(name = "q2")]
#[command(version = quarto_util::cli_version_display())]
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

        /// Wipe caches before rendering. These include the Pass-1 profile cache
        /// and `nav-config-hash` caches. Preserves the SCSS `sass/` cache.
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
        /// Fails if the document's content does not match the
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

        /// Emit diagnostics as one JSON object per line on stderr
        /// instead of human-readable text.
        #[arg(long = "json-errors")]
        json_errors: bool,

        /// Stop rendering as soon as the first error occurs.
        /// When executed with many threads, many errors might still be reported.
        #[arg(long = "fail-fast")]
        fail_fast: bool,

        /// Treat warnings as errors: warning diagnostics are reported
        /// with error severity and any of them makes the command exit
        /// non-zero. Useful in CI. Does not stop the render early.
        #[arg(long)]
        strict: bool,

        /// Skip the project's `pre-render` and `post-render` scripts.
        #[arg(long = "no-render-scripts")]
        no_render_scripts: bool,
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

        /// Run as a bare sync server with no local project.
        #[arg(long)]
        no_project: bool,

        /// Allow edits made in the preview UI (clicking a paragraph or
        /// heading and typing) to be written back to the source files
        /// on disk. Without this flag the preview is read-only: the
        /// edit surface is disabled and the server refuses to persist
        /// document changes to your files.
        #[arg(long)]
        allow_edit: bool,

        /// Share this preview session over an end-to-end encrypted
        /// peer-to-peer tunnel (via iroh). Prints a join string;
        /// anyone who has it can VIEW the project and RE-RUN its code
        /// on this machine (and EDIT the files if --allow-edit is also
        /// set), so treat the string like a password.
        #[arg(long)]
        share: bool,

        /// Join a shared preview session using the `q2preview…` string
        /// printed by `q2 preview --share` on the host machine.
        ///
        /// Runs a local proxy for the host's session — no local project
        /// is read and nothing is written to disk on this machine, so
        /// the host-mode flags (a path, --share, --no-project,
        /// --allow-edit, --data-dir, --preview-dir) don't combine with
        /// it. --port/--host pick where the local proxy listens;
        /// --no-browser still applies.
        #[arg(
            long,
            value_name = "TICKET",
            conflicts_with_all = ["path", "share", "no_project", "allow_edit", "data_dir", "preview_dir"]
        )]
        join: Option<String>,
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
        /// Type of artifact to create (e.g. "project")
        #[arg(value_name = "TYPE")]
        type_: Option<String>,

        /// Additional arguments (for "project": <type> <directory> [title])
        args: Vec<String>,

        /// Read a JSON create directive from stdin and emit a JSON
        /// result on stdout (diagnostics go to stderr as JSON lines)
        #[arg(long)]
        json: bool,

        /// List available artifact types and choices
        #[arg(long)]
        list: bool,

        /// Report the file plan without writing anything
        #[arg(long)]
        dry_run: bool,

        /// Never prompt interactively, even on a terminal; missing
        /// arguments are errors
        #[arg(long)]
        no_prompt: bool,
    },

    /// Automate document or project setup tasks
    Use {
        #[command(subcommand)]
        command: UseCommand,
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

        /// Active project profile(s) (comma-separated or repeated).
        #[arg(long)]
        profile: Vec<String>,

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

    /// Print a document's merged configuration as JSON.
    ///
    /// Resolves the effective configuration that applies to a document after
    /// Quarto 2's full metadata-merge semantics (`_quarto.yml`, directory
    /// `_metadata.yml` layers, frontmatter, and `format.<fmt>.*` flattening),
    /// so external tools don't have to reimplement them.
    ///
    /// With no path, prints the entire merged metadata. A dot-separated path
    /// (e.g. `format.html.toc` or `authors.0.name`) selects a value; a numeric
    /// segment indexes into an array.
    #[command(name = "get-config")]
    GetConfig {
        /// Document to inspect (.qmd/.md).
        file: PathBuf,

        /// Dot-separated key path. Omit for the entire merged metadata.
        path: Option<String>,

        /// Target format whose `format.<fmt>.*` overrides are flattened in.
        #[arg(long, default_value = "html")]
        to: String,

        /// Prose representation: `value` (markdown string) or `pandoc` (AST).
        #[arg(long, value_enum, default_value_t = commands::get_config::OutputMode::Value)]
        output: commands::get_config::OutputMode,

        /// Exit non-zero if the path does not exist (instead of printing null).
        #[arg(long)]
        strict: bool,

        /// Emit compact single-line JSON instead of pretty-printed.
        #[arg(long)]
        compact: bool,

        /// Active project profile(s) (comma-separated or repeated).
        #[arg(long)]
        profile: Vec<String>,
    },

    /// Inspect pipeline execution traces under `.quarto/trace/`.
    ///
    /// Output is always JSON. Pipe to `jq`/`fx` for pretty filtering, or
    /// use `quarto trace view` (future) for an interactive SPA.
    Trace {
        #[command(subcommand)]
        command: TraceCommand,
    },

    /// Run the Quarto Hub MCP server (for AI agents; needs Node.js).
    ///
    /// Delegates to the embedded TypeScript MCP server: all arguments
    /// pass through verbatim (`q2 mcp --help` shows launcher options
    /// followed by the server's own usage, e.g. `--server <url>`,
    /// `--read-only`). Launcher-specific controls: `q2 mcp
    /// --print-config` prints a ready-to-paste `.mcp.json` entry, `q2
    /// mcp --launcher-info` prints embed/cache/node diagnostics, and the
    /// `QUARTO_NODE` env var picks the Node.js binary when discovery
    /// fails (GUI MCP hosts don't see your shell PATH).
    #[command(disable_help_flag = true)]
    Mcp {
        /// Arguments passed through to the MCP server verbatim.
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },

    /// Connect to a hub session as a code-execution provider.
    ///
    /// Authenticates with the hub (opening a browser the first time) and
    /// joins the project's collaborative session to run the project's code on
    /// THIS machine.
    ///
    /// By default this is a ONE-SHOT run: it connects, executes the single
    /// document named by --file once (after you review and accept it at an
    /// interactive prompt), pushes the results to every collaborator, and
    /// exits. Pass --watch to instead stay online and serve execution requests
    /// as collaborators click "Run" (each still gated by the prompt).
    ///
    /// Every execution requires your affirmative consent at a terminal prompt
    /// that shows the resolved document to be evaluated. Use
    /// --dangerously-accept-requests only if you fully trust the session and
    /// want unattended execution.
    ProvideHub {
        /// A quarto-hub share URL or a bare project index-document id.
        project: String,

        /// The project-relative document to execute once. REQUIRED in the
        /// default one-shot mode; ignored under --watch (the path comes from
        /// the collaborator's request there).
        #[arg(long = "file")]
        file: Option<String>,

        /// Hub websocket URL (defaults to $QUARTO_HUB_SERVER, else the
        /// canonical hub).
        #[arg(long, env = "QUARTO_HUB_SERVER")]
        server: Option<String>,

        /// Stay online and serve execution requests from collaborators (the
        /// editor's "Run" button) until Ctrl-C, instead of the default
        /// one-shot run. Each request is still gated by the consent prompt.
        #[arg(long = "watch")]
        watch: bool,

        /// Skip the interactive consent prompt and auto-accept every execution.
        /// DANGEROUS: a hijacked or spoofed session could run arbitrary code on
        /// this machine unattended. Only use in a fully-trusted session.
        #[arg(long = "dangerously-accept-requests")]
        dangerously_accept_requests: bool,

        /// Dev/testing: use this bearer token instead of the interactive OAuth
        /// bridge. Only for a local, no-auth hub (`q2 hub`), which ignores it.
        #[arg(long, env = "QUARTO_HUB_TOKEN")]
        token: Option<String>,
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
enum UseCommand {
    /// Add a brand (`_brand.yml`) to this project and declare it in
    /// `_quarto.yml`
    ///
    /// With no TARGET, writes a starter `_brand.yml` you can edit.
    /// With a TARGET, copies a brand from a local path or a remote
    /// source (e.g. `<gh-org>/<gh-repo>`).
    ///
    /// Unlike Quarto 1, Quarto 2 does not auto-discover `_brand.yml`,
    /// so this command also writes the `brand:` key that makes the
    /// brand take effect.
    Brand {
        /// Where to get the brand from: a local path, `<org>/<repo>`,
        /// or an archive URL. Omit to scaffold a starter brand.
        target: Option<String>,

        /// Report what would happen without writing anything
        #[arg(long)]
        dry_run: bool,

        /// Proceed even though this project already has a brand file
        /// or a `brand:` declaration. Does **not** waive the
        /// remote-source trust prompt — see --trust.
        #[arg(long)]
        force: bool,

        /// Skip the trust prompt for a remote source. Does **not**
        /// override local-state checks — see --force.
        #[arg(long)]
        trust: bool,

        /// Never prompt interactively, even on a terminal
        #[arg(long)]
        no_prompt: bool,

        /// Emit one JSON result object on stdout; diagnostics go to
        /// stderr as JSON lines
        #[arg(long)]
        json: bool,
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

#[cfg(test)]
mod cli_parse_tests {
    //! clap parse harness (live-share plan, Phase 2). These are the first
    //! parse-level tests for the `q2` CLI; the Phase 3 `--join` conflict
    //! matrix extends this module.

    use clap::Parser;

    use super::{Cli, Commands};

    /// Parse argv (without the implicit binary name) into `Cli`.
    fn try_parse(args: &[&str]) -> Result<Cli, clap::Error> {
        Cli::try_parse_from(std::iter::once("q2").chain(args.iter().copied()))
    }

    /// Unwrap a parsed `Preview` command or panic with the actual variant.
    fn parse_preview(args: &[&str]) -> Commands {
        let cli = try_parse(args).expect("args should parse");
        match cli.command {
            cmd @ Commands::Preview { .. } => cmd,
            _ => panic!("expected a Preview command from {args:?}"),
        }
    }

    #[test]
    fn preview_share_flag_parses() {
        let Commands::Preview { share, .. } = parse_preview(&["preview", "--share"]) else {
            unreachable!()
        };
        assert!(share, "--share must set PreviewArgs::share");
    }

    #[test]
    fn preview_share_defaults_off() {
        let Commands::Preview { share, .. } = parse_preview(&["preview"]) else {
            unreachable!()
        };
        assert!(!share, "share must default to false");
    }

    #[test]
    fn preview_share_composes_with_allow_edit() {
        // Composition from the plan's CLI surface: `--share --allow-edit`
        // (viewer with inline-edit write-back for guests).
        let Commands::Preview {
            share, allow_edit, ..
        } = parse_preview(&["preview", "--share", "--allow-edit"])
        else {
            unreachable!()
        };
        assert!(share && allow_edit, "--share --allow-edit must both parse");
    }

    #[test]
    fn preview_share_conflicts_with_join() {
        // (match instead of expect_err: `Cli` deliberately has no Debug impl)
        let err = match try_parse(&["preview", "--share", "--join", "x"]) {
            Ok(_) => panic!("--share and --join are host vs. guest; must conflict"),
            Err(e) => e,
        };
        assert_eq!(err.kind(), clap::error::ErrorKind::ArgumentConflict);
    }

    // ── Phase 3 (bd-6y0p1bne): `--join` conflict matrix ──────────────
    // The guest path has no local project, hub, or disk surface, so
    // every host-mode-only flag must be a hard parse error, not a
    // silent no-op. (`--ui` joins this matrix in Phase 4, when the
    // flag itself lands.)

    /// Assert argv is rejected specifically as an argument conflict
    /// (not, say, an unknown-arg error).
    fn assert_join_conflict(args: &[&str]) {
        let err = match try_parse(args) {
            Ok(_) => panic!("{args:?} mixes guest mode with a host-only flag; must conflict"),
            Err(e) => e,
        };
        assert_eq!(
            err.kind(),
            clap::error::ErrorKind::ArgumentConflict,
            "args: {args:?}"
        );
    }

    #[test]
    fn preview_join_parses_and_captures_ticket() {
        let Commands::Preview { join, .. } = parse_preview(&["preview", "--join", "q2previewabc"])
        else {
            unreachable!()
        };
        assert_eq!(join.as_deref(), Some("q2previewabc"));
    }

    #[test]
    fn preview_join_conflicts_with_positional_path() {
        assert_join_conflict(&["preview", "some/project", "--join", "x"]);
    }

    #[test]
    fn preview_join_conflicts_with_share() {
        assert_join_conflict(&["preview", "--join", "x", "--share"]);
    }

    #[test]
    fn preview_join_conflicts_with_no_project() {
        assert_join_conflict(&["preview", "--join", "x", "--no-project"]);
    }

    #[test]
    fn preview_join_conflicts_with_allow_edit() {
        assert_join_conflict(&["preview", "--join", "x", "--allow-edit"]);
    }

    #[test]
    fn preview_join_conflicts_with_data_dir() {
        assert_join_conflict(&["preview", "--join", "x", "--data-dir", "d"]);
    }

    #[test]
    fn preview_join_conflicts_with_preview_dir() {
        assert_join_conflict(&["preview", "--join", "x", "--preview-dir", "d"]);
    }

    #[test]
    fn preview_join_composes_with_guest_flags() {
        // `--port` picks the local proxy port, `--host` its bind
        // interface, `--no-browser` suppresses the auto-open — all
        // meaningful for a guest and must keep parsing.
        let Commands::Preview {
            join,
            port,
            host,
            no_browser,
            ..
        } = parse_preview(&[
            "preview",
            "--join",
            "q2previewabc",
            "--port",
            "9280",
            "--host",
            "127.0.0.1",
            "--no-browser",
        ])
        else {
            unreachable!()
        };
        assert_eq!(join.as_deref(), Some("q2previewabc"));
        assert_eq!(port, Some(9280));
        assert_eq!(host.as_deref(), Some("127.0.0.1"));
        assert!(no_browser);
    }
}

fn main() -> Result<()> {
    // Install Quarto's `Q-*` error catalog into the catalog-agnostic
    // `quarto-error-reporting` host, so diagnostics can resolve docs URLs and
    // catalog metadata. Must run before any diagnostic is rendered; idempotent.
    quarto_error_catalog::install();

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
        // Logs go to stderr like every other q2 diagnostic — stdout
        // stays reserved for command output (`get-config` JSON, etc.).
        .with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr))
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
            json_errors,
            fail_fast,
            strict,
            no_render_scripts,
            profile,
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
            json_errors,
            fail_fast,
            strict,
            no_render_scripts,
            profile,
        }),
        Commands::Preview {
            path,
            port,
            host,
            no_browser,
            data_dir,
            preview_dir,
            no_project,
            allow_edit,
            share,
            join,
        } => {
            if let Some(ticket) = join {
                // Guest mode (live-share plan Phase 3): clap has already
                // rejected every host-mode flag via conflicts_with_all.
                commands::preview::execute_join(commands::preview::JoinArgs {
                    ticket,
                    port,
                    host,
                    no_browser,
                })
            } else {
                commands::preview::execute(commands::preview::PreviewArgs {
                    path,
                    port,
                    host,
                    no_browser,
                    data_dir,
                    preview_dir,
                    no_project,
                    allow_edit,
                    share,
                })
            }
        }
        Commands::Serve { .. } => commands::serve::execute(),
        Commands::Create {
            type_,
            args,
            json,
            list,
            dry_run,
            no_prompt,
        } => commands::create::execute(type_, args, json, list, dry_run, no_prompt),
        Commands::Use { command } => match command {
            UseCommand::Brand {
                target,
                dry_run,
                force,
                trust,
                no_prompt,
                json,
            } => commands::use_cmd::execute_brand(commands::use_cmd::BrandArgs {
                target,
                dry_run,
                force,
                trust,
                no_prompt,
                json,
            }),
        },
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
            profile,
        } => commands::publish::execute(commands::publish::PublishArgs {
            provider,
            path,
            no_render,
            profile,
            no_prompt,
            no_browser,
            no_wait,
            dry_run,
            json,
        }),
        Commands::Check { .. } => commands::check::execute(),
        Commands::Call { function, args } => commands::call::execute(function, args),
        Commands::Lsp => commands::lsp::execute(),
        Commands::GetConfig {
            file,
            path,
            to,
            output,
            strict,
            compact,
            profile,
        } => commands::get_config::execute(commands::get_config::GetConfigArgs {
            file,
            path,
            to,
            output,
            strict,
            compact,
            profile,
        }),
        Commands::Mcp { args } => commands::mcp::run(&args),

        Commands::ProvideHub {
            project,
            file,
            server,
            watch,
            dangerously_accept_requests,
            token,
        } => commands::provide_hub::execute(commands::provide_hub::ProvideHubArgs {
            project,
            file,
            server,
            watch,
            dangerously_accept_requests,
            token,
        }),

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
