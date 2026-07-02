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
//! - `create-worktree`: Create git worktree with CLAUDE.local.md context stub
//! - `braid-snapshot`: Write a backup-only `braid export` to `.braid/snapshot.jsonl`
//! - `pandoc-check`: Check local pandoc against the pampa oracle tests
//! - `test`: Run workspace tests with platform-appropriate crate exclusions
//! - `verify`: Run full project verification (build + tests for Rust and hub-client)
//! - `build-all`: Fresh-clone build orchestration (npm install + hub-client + Rust workspace)
//! - `build-trace-viewer`: Build just the trace-viewer SPA
//! - `build-hub-mcp-bundle`: Build the self-contained hub MCP server bundle
//! - `stage-doc-examples`: Render `examples/manifest.yml` projects into `docs/examples/`

mod braid_snapshot;
mod build_all;
mod build_hub_mcp_bundle;
mod build_q2_preview_spa;
mod build_trace_viewer;
mod create_worktree;
mod dev_setup;
mod lint;
mod pandoc_check;
mod stage_doc_examples;
mod switch_task;
mod test;
mod treesitter_crlf;
mod ts_packages;
mod util;
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

    /// Create a new git worktree with a CLAUDE.local.md context stub.
    ///
    /// Modes (exactly one required):
    ///   <bd-id>      — braid strand (positional)
    ///   --issue N    — GitHub issue triage
    ///   --upgrade    — cargo dependency upgrade (date-based branch)
    #[command(verbatim_doc_comment)]
    CreateWorktree {
        #[command(flatten)]
        args: create_worktree::Args,
    },

    /// Write a backup-only `braid export` snapshot to `.braid/snapshot.jsonl`.
    ///
    /// The braid skein (CRDT) is the source of truth; this committed snapshot
    /// is for grep/diff/recovery only. It is STRICTLY ONE-DIRECTIONAL —
    /// never `braid import` it back, and on a git conflict regenerate rather
    /// than hand-merge. See CLAUDE.md § Snapshot backup policy.
    BraidSnapshot {},

    /// Switch the current worktree to a new sub-task branch (no new worktree).
    ///
    /// For *sequential* sub-task work in an epic: reuses the worktree's
    /// warm node_modules/ + target/ caches instead of paying the
    /// fresh-clone cost. Companion to `create-worktree`, which is the
    /// right answer for *parallel* / *investigation* work.
    ///
    /// With `--from <branch>`, switches to that branch and fast-forward-
    /// pulls before creating the topic branch. Without `--from`,
    /// branches off the current HEAD. Updates CLAUDE.local.md and marks
    /// the braid strand `in_progress`.
    SwitchTask {
        /// Braid strand ID to switch to (e.g. `bd-yxqt`).
        beads_id: String,

        /// Integration / epic branch to switch+pull before branching.
        /// Omit to branch off the current HEAD.
        #[arg(long)]
        from: Option<String>,

        /// Optional slug override (kebab-case).
        #[arg(long)]
        slug: Option<String>,

        /// Don't mark the strand `in_progress` in braid.
        #[arg(long)]
        no_claim: bool,
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
    /// 6. Build ts-packages workspaces + quarto-hub-mcp smoke check
    /// 7. Build hub-client including WASM (npm run build:all)
    /// 8. Run hub-client tests (npm run test:ci)
    ///
    /// Use this before pushing to ensure nothing will fail in CI.
    Verify {
        /// Skip Rust build step.
        #[arg(long)]
        skip_rust_build: bool,

        /// Skip Rust tests.
        #[arg(long)]
        skip_rust_tests: bool,

        /// Skip the ts-packages build + quarto-hub-mcp smoke check.
        #[arg(long)]
        skip_ts_packages_build: bool,

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

        /// Skip the shared preview-renderer + preview-runtime tests.
        #[arg(long)]
        skip_shared_package_tests: bool,

        /// Skip the q2-preview-spa placeholder build.
        #[arg(long)]
        skip_q2_preview_spa_build: bool,

        /// Skip the quarto-sync-client + quarto-hub-mcp package tests.
        #[arg(long)]
        skip_hub_mcp_tests: bool,

        /// Include hub-client e2e tests (slower, requires browser).
        #[arg(long)]
        e2e: bool,

        /// Do not set RUSTFLAGS="-D warnings" (allows warnings during iteration).
        #[arg(long)]
        no_deny_warnings: bool,
    },

    /// Check the local `pandoc` against the pampa oracle tests.
    ///
    /// Print-only calibration tool for bd-i9i5ad2t's version-gated oracle
    /// tests (`test_html_writer`, `test_json_writer`,
    /// `unit_test_corpus_matches_pandoc_markdown`,
    /// `unit_test_corpus_matches_pandoc_commonmark`). Runs those 4 tests
    /// against the local pandoc under a bypass of the calibrated version
    /// gate, then reports pass/fail and — on a green run past the current
    /// ceiling — the exact line in `test.rs` to bump. Never edits the file.
    PandocCheck {},

    /// Stage doc-example projects into the docs site for `.embed-example-iframe`.
    ///
    /// Renders each project listed in `examples/manifest.yml` with `q2` and
    /// copies its static output (`*.html` + `*_files/`) into
    /// `docs/examples/<entry>/`, where the docs `.embed-example-iframe`
    /// placeholders point. The staged output is generated and gitignored —
    /// regenerate with this command; never commit it. See bd-z1smhvuo.
    StageDocExamples {},

    /// Build just the trace-viewer SPA.
    ///
    /// Faster than `build-all` when only the SPA source has changed. The
    /// resulting `trace-viewer/dist/` is picked up on the next `cargo
    /// build -p quarto-trace-server` (via `include_dir!`).
    BuildTraceViewer {},

    /// Build just the q2-preview SPA.
    ///
    /// Faster than `build-all` when only the SPA source has changed. The
    /// resulting `q2-preview-spa/dist/` is picked up on the next `cargo
    /// build -p quarto-preview` (via `include_dir!`).
    BuildQ2PreviewSpa {},

    /// Build the self-contained hub MCP server bundle.
    ///
    /// Produces `ts-packages/quarto-hub-mcp/dist-bundle/` (esbuild) —
    /// the artifact `q2 mcp` embeds (bd-81cfshmw). A plain `cargo
    /// build` does NOT refresh it; run this after changing
    /// quarto-hub-mcp / quarto-sync-client / quarto-automerge-schema.
    BuildHubMcpBundle {},

    /// Fresh-clone build orchestration.
    ///
    /// Runs the full build sequence in dependency order, serving as the source
    /// of truth for what a fresh checkout (or CI) needs to produce a working
    /// build:
    /// 1. npm install at the repo root (npm workspaces)
    /// 2. ts-packages build (dist/ for Node consumers like quarto-hub-mcp)
    /// 3. hub-client build (includes WASM)
    /// 4. trace-viewer build (if present; Phase 4.3+)
    /// 5. q2-preview-spa build (if present; q2-preview Phase A.4)
    /// 6. hub MCP bundle (q2-mcp embed artifact; bd-81cfshmw)
    /// 7. cargo build --workspace
    BuildAll {
        /// Skip `npm install`.
        #[arg(long)]
        skip_npm_install: bool,

        /// Skip the ts-packages build.
        #[arg(long)]
        skip_ts_packages_build: bool,

        /// Skip the hub-client build.
        #[arg(long)]
        skip_hub_build: bool,

        /// Skip the trace-viewer build.
        #[arg(long)]
        skip_trace_viewer_build: bool,

        /// Skip the q2-preview-spa build.
        #[arg(long)]
        skip_q2_preview_spa_build: bool,

        /// Skip the hub MCP bundle build.
        #[arg(long)]
        skip_hub_mcp_bundle: bool,

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
        Command::BraidSnapshot {} => braid_snapshot::run(),
        Command::SwitchTask {
            beads_id,
            from,
            slug,
            no_claim,
        } => switch_task::run(switch_task::Args {
            beads_id,
            from,
            slug,
            no_claim,
        }),
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
            skip_ts_packages_build,
            skip_hub_build,
            skip_hub_tests,
            skip_trace_viewer_build,
            skip_trace_viewer_tests,
            skip_treesitter_tests,
            skip_treesitter_crlf_tests,
            skip_shared_package_tests,
            skip_q2_preview_spa_build,
            skip_hub_mcp_tests,
            e2e,
            no_deny_warnings,
        } => {
            let config = verify::VerifyConfig {
                skip_rust_build,
                skip_rust_tests,
                skip_ts_packages_build,
                skip_hub_build,
                skip_hub_tests,
                skip_trace_viewer_build,
                skip_trace_viewer_tests,
                skip_treesitter_tests,
                skip_treesitter_crlf_tests,
                skip_shared_package_tests,
                skip_q2_preview_spa_build,
                skip_hub_mcp_tests,
                include_e2e: e2e,
                no_deny_warnings,
            };
            verify::run(&config)
        }
        Command::PandocCheck {} => pandoc_check::run(),
        Command::StageDocExamples {} => stage_doc_examples::run(),
        Command::BuildTraceViewer {} => build_trace_viewer::run(),
        Command::BuildQ2PreviewSpa {} => build_q2_preview_spa::run(),
        Command::BuildHubMcpBundle {} => build_hub_mcp_bundle::run(),
        Command::BuildAll {
            skip_npm_install,
            skip_ts_packages_build,
            skip_hub_build,
            skip_trace_viewer_build,
            skip_q2_preview_spa_build,
            skip_hub_mcp_bundle,
            skip_rust_build,
            release,
        } => {
            let config = build_all::BuildAllConfig {
                skip_npm_install,
                skip_ts_packages_build,
                skip_hub_build,
                skip_trace_viewer_build,
                skip_q2_preview_spa_build,
                skip_hub_mcp_bundle,
                skip_rust_build,
                release,
            };
            build_all::run(&config)
        }
    }
}
