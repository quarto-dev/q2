//! Xtask - Project-specific automation tasks for Quarto Rust.
//!
//! This crate provides development automation tasks that can be run via:
//! ```bash
//! cargo xtask <command>
//! ```
//!
//! Available commands:
//! - `dev-setup`: Install required development tools (cargo-nextest, wasm-bindgen-cli)
//! - `lint`: Run custom lint checks on the codebase
//! - `create-worktree`: Create git worktree with beads redirect and CLAUDE.local.md
//! - `test`: Run workspace tests with platform-appropriate crate exclusions
//! - `verify`: Run full project verification (build + tests for Rust and hub-client)
//! - `build-all`: Fresh-clone build orchestration (npm install + hub-client + Rust workspace)
//! - `build-trace-viewer`: Build just the trace-viewer SPA

mod build_all;
mod build_trace_viewer;
mod create_worktree;
mod dev_setup;
mod lint;
mod test;
mod treesitter_crlf;
mod verify;

use anyhow::Result;
use clap::{Parser, Subcommand};

/// Project-specific automation tasks for Quarto Rust.
#[derive(Parser)]
#[command(name = "xtask")]
#[command(about = "Development automation tasks for Quarto Rust")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Install required development tools.
    ///
    /// Checks for cargo-nextest and wasm-bindgen-cli (pinned version), installing any that are missing.
    /// Uses cargo-binstall for faster binary installs when available,
    /// falling back to cargo install --locked otherwise.
    DevSetup {},

    /// Run custom lint checks on the codebase.
    ///
    /// These checks catch issues that standard Rust linters miss,
    /// such as references to external-sources/ in compile-time macros.
    Lint {
        /// Show verbose output including all files checked.
        #[arg(short, long)]
        verbose: bool,

        /// Only show errors, no progress or summary.
        #[arg(short, long)]
        quiet: bool,
    },

    /// Create a new git worktree with beads redirect and CLAUDE.local.md context stub.
    ///
    /// Modes (exactly one required):
    ///   <bd-id>      — beads issue (positional)
    ///   --issue N    — GitHub issue triage
    ///   --upgrade    — cargo dependency upgrade (date-based branch)
    #[command(verbatim_doc_comment)]
    CreateWorktree {
        #[command(flatten)]
        args: create_worktree::Args,
    },

    /// Run workspace tests with platform-appropriate crate exclusions.
    ///
    /// On Windows, automatically excludes crates that depend on v8 (which cannot
    /// compile test binaries on Windows). On other platforms, runs the full suite.
    ///
    /// Extra arguments after `--` are forwarded to cargo nextest.
    Test {
        /// Set RUSTFLAGS="-D warnings" (deny warnings, matching CI).
        #[arg(long)]
        deny_warnings: bool,

        /// Extra arguments to pass to cargo nextest run.
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },

    /// Run full project verification (mirrors CI checks).
    ///
    /// This runs all build and test steps to ensure the entire project is healthy:
    /// 1. Run custom lint checks (cargo xtask lint)
    /// 2. Check Rust formatting (cargo fmt --check)
    /// 3. Build all Rust crates (cargo build --workspace, with -D warnings)
    /// 4. Test tree-sitter grammars (tree-sitter test)
    /// 5. Run all Rust tests (cargo nextest run --workspace, with -D warnings)
    /// 6. Build hub-client including WASM (npm run build:all)
    /// 7. Run hub-client tests (npm run test:ci)
    ///
    /// Use this before pushing to ensure nothing will fail in CI.
    Verify {
        /// Skip Rust build step.
        #[arg(long)]
        skip_rust_build: bool,

        /// Skip Rust tests.
        #[arg(long)]
        skip_rust_tests: bool,

        /// Skip hub-client build.
        #[arg(long)]
        skip_hub_build: bool,

        /// Skip hub-client tests.
        #[arg(long)]
        skip_hub_tests: bool,

        /// Skip trace-viewer build.
        #[arg(long)]
        skip_trace_viewer_build: bool,

        /// Skip trace-viewer tests.
        #[arg(long)]
        skip_trace_viewer_tests: bool,

        /// Skip tree-sitter grammar tests.
        #[arg(long)]
        skip_treesitter_tests: bool,

        /// Skip the CRLF parity run of tree-sitter grammar tests.
        #[arg(long)]
        skip_treesitter_crlf_tests: bool,

        /// Include hub-client e2e tests (slower, requires browser).
        #[arg(long)]
        e2e: bool,

        /// Do not set RUSTFLAGS="-D warnings" (allows warnings during iteration).
        #[arg(long)]
        no_deny_warnings: bool,
    },

    /// Build just the trace-viewer SPA.
    ///
    /// Faster than `build-all` when only the SPA source has changed. The
    /// resulting `trace-viewer/dist/` is picked up on the next `cargo
    /// build -p quarto-trace-server` (via `include_dir!`).
    BuildTraceViewer {},

    /// Fresh-clone build orchestration.
    ///
    /// Runs the full build sequence in dependency order, serving as the source
    /// of truth for what a fresh checkout (or CI) needs to produce a working
    /// build:
    /// 1. npm install at the repo root (npm workspaces)
    /// 2. hub-client build (includes WASM)
    /// 3. trace-viewer build (if present; Phase 4.3+)
    /// 4. cargo build --workspace
    BuildAll {
        /// Skip `npm install`.
        #[arg(long)]
        skip_npm_install: bool,

        /// Skip the hub-client build.
        #[arg(long)]
        skip_hub_build: bool,

        /// Skip the trace-viewer build.
        #[arg(long)]
        skip_trace_viewer_build: bool,

        /// Skip the Rust workspace build.
        #[arg(long)]
        skip_rust_build: bool,

        /// Pass `--release` to `cargo build`.
        #[arg(long)]
        release: bool,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::DevSetup {} => dev_setup::run(),
        Command::Lint { verbose, quiet } => {
            let config = lint::LintConfig { verbose, quiet };
            lint::run(&config)
        }
        Command::CreateWorktree { args } => create_worktree::run(args),
        Command::Test {
            deny_warnings,
            args,
        } => {
            let rustflags = if deny_warnings {
                Some("-D warnings")
            } else {
                None
            };
            test::run(&args, rustflags)
        }
        Command::Verify {
            skip_rust_build,
            skip_rust_tests,
            skip_hub_build,
            skip_hub_tests,
            skip_trace_viewer_build,
            skip_trace_viewer_tests,
            skip_treesitter_tests,
            skip_treesitter_crlf_tests,
            e2e,
            no_deny_warnings,
        } => {
            let config = verify::VerifyConfig {
                skip_rust_build,
                skip_rust_tests,
                skip_hub_build,
                skip_hub_tests,
                skip_trace_viewer_build,
                skip_trace_viewer_tests,
                skip_treesitter_tests,
                skip_treesitter_crlf_tests,
                include_e2e: e2e,
                no_deny_warnings,
            };
            verify::run(&config)
        }
        Command::BuildTraceViewer {} => build_trace_viewer::run(),
        Command::BuildAll {
            skip_npm_install,
            skip_hub_build,
            skip_trace_viewer_build,
            skip_rust_build,
            release,
        } => {
            let config = build_all::BuildAllConfig {
                skip_npm_install,
                skip_hub_build,
                skip_trace_viewer_build,
                skip_rust_build,
                release,
            };
            build_all::run(&config)
        }
    }
}
