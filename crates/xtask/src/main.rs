//! Xtask - Project-specific automation tasks for Quarto Rust.
//!
//! This crate provides development automation tasks that can be run via:
//! ```bash
//! cargo xtask <command>
//! ```
//!
//! Available commands:
//! - `lint`: Run custom lint checks on the codebase
//! - `verify`: Run full project verification (build + tests for Rust and hub-client)

mod lint;
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

    /// Run full project verification.
    ///
    /// This runs all build and test steps to ensure the entire project is healthy:
    /// 1. Build all Rust crates (cargo build --workspace)
    /// 2. Run all Rust tests (cargo nextest run --workspace)
    /// 3. Build hub-client including WASM (npm run build:all)
    /// 4. Run hub-client tests (npm run test:ci)
    ///
    /// Use this before committing to ensure nothing is broken.
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

        /// Include hub-client e2e tests (slower, requires browser).
        #[arg(long)]
        e2e: bool,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Lint { verbose, quiet } => {
            let config = lint::LintConfig { verbose, quiet };
            lint::run(&config)
        }
        Command::Verify {
            skip_rust_build,
            skip_rust_tests,
            skip_hub_build,
            skip_hub_tests,
            e2e,
        } => {
            let config = verify::VerifyConfig {
                skip_rust_build,
                skip_rust_tests,
                skip_hub_build,
                skip_hub_tests,
                include_e2e: e2e,
            };
            verify::run(&config)
        }
    }
}
