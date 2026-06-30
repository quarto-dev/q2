//! Build-all command - fresh-clone build orchestration.
//!
//! Runs the full fresh-build sequence in dependency order, serving as the
//! source of truth for what CI (and a developer on a fresh clone) needs to do
//! to produce a working build:
//!
//! 1. `npm install` at the repo root (npm workspaces)
//! 2. Build the ts-packages workspaces (`dist/` for Node consumers like
//!    the quarto-hub-mcp server; bd-6rczoll3 — see `ts_packages.rs`)
//! 3. Build hub-client (includes WASM via `npm run build:all`)
//! 4. Build trace-viewer (if present; Phase 4.3+)
//! 5. Build q2-preview-spa (if present; q2-preview Phase A.4 / bd-501n)
//! 6. Build the hub MCP bundle (q2-mcp embed artifact; bd-81cfshmw)
//! 7. Build the engine-host-deno bundle (quarto-core embed artifact; Plan 1b)
//! 8. Build the Rust workspace (`cargo build --workspace`)
//!
//! Both SPAs (trace-viewer + q2-preview-spa) must build *before* the
//! Rust workspace because `quarto-trace-server` and `quarto-preview`
//! embed their respective `dist/` directories via `include_dir!`.

use anyhow::{Context, Result, bail};
use std::path::Path;

/// Configuration for the build-all command.
#[derive(Default)]
pub struct BuildAllConfig {
    /// Skip `npm install`. Useful when running in a loop where dependencies
    /// haven't changed.
    pub skip_npm_install: bool,
    /// Skip the ts-packages build step. No-op when `ts-packages/` is absent.
    pub skip_ts_packages_build: bool,
    /// Skip the hub-client build step.
    pub skip_hub_build: bool,
    /// Skip the trace-viewer build step. No-op until Phase 4.3 lands.
    pub skip_trace_viewer_build: bool,
    /// Skip the hub MCP bundle build step (q2-mcp embed artifact).
    pub skip_hub_mcp_bundle: bool,
    /// Skip the engine-host-deno bundle build step (quarto-core embed artifact).
    pub skip_engine_host_bundle: bool,
    /// Skip the q2-preview-spa build step. No-op when the SPA dir is
    /// absent (e.g. branches before bd-hfjj Phase 6).
    pub skip_q2_preview_spa_build: bool,
    /// Skip the final `cargo build --workspace` step.
    pub skip_rust_build: bool,
    /// Pass `--release` to `cargo build`.
    pub release: bool,
}

/// Run the build-all command.
pub fn run(config: &BuildAllConfig) -> Result<()> {
    let project_root = find_project_root()?;
    let ts_workspaces = crate::ts_packages::workspace_paths(&project_root);

    let steps: Vec<(&str, bool)> = vec![
        ("npm install (root workspaces)", !config.skip_npm_install),
        (
            "ts-packages build",
            !config.skip_ts_packages_build && !ts_workspaces.is_empty(),
        ),
        ("hub-client build (WASM + TS)", !config.skip_hub_build),
        (
            "trace-viewer build",
            !config.skip_trace_viewer_build && trace_viewer_exists(&project_root),
        ),
        (
            "q2-preview-spa build",
            !config.skip_q2_preview_spa_build && q2_preview_spa_exists(&project_root),
        ),
        (
            "hub MCP bundle build",
            !config.skip_hub_mcp_bundle && hub_mcp_exists(&project_root),
        ),
        (
            "engine-host-deno bundle build",
            !config.skip_engine_host_bundle && engine_host_exists(&project_root),
        ),
        ("Rust workspace build", !config.skip_rust_build),
    ];

    let enabled_count = steps.iter().filter(|(_, enabled)| *enabled).count();
    let total = enabled_count as u32;
    let mut step_idx: u32 = 0;

    // Step: npm install
    if !config.skip_npm_install {
        step_idx += 1;
        banner(step_idx, total, "Installing npm workspace dependencies");
        run_command(
            "npm",
            &["install"],
            &project_root,
            None,
            "npm install failed",
        )?;
        println!("✓ npm install complete");
    }

    // Step: ts-packages build (bd-6rczoll3)
    //
    // Emits the `dist/` output that Node consumers (the quarto-hub-mcp
    // server) resolve at runtime; nothing else in this sequence builds it
    // because hub-client bundles ts-packages from source.
    if !config.skip_ts_packages_build && !ts_workspaces.is_empty() {
        step_idx += 1;
        banner(step_idx, total, "Building ts-packages workspaces");
        let mut npm_args: Vec<&str> = vec!["run", "build", "--if-present"];
        for workspace in &ts_workspaces {
            npm_args.push("-w");
            npm_args.push(workspace);
        }
        run_command(
            "npm",
            &npm_args,
            &project_root,
            None,
            "ts-packages build failed",
        )?;
        println!("✓ ts-packages build complete");
    }

    // Step: hub-client build (WASM + TS)
    if !config.skip_hub_build {
        step_idx += 1;
        banner(step_idx, total, "Building hub-client (WASM + TS)");
        let hub_client_dir = project_root.join("hub-client");
        run_command(
            "npm",
            &["run", "build:all"],
            &hub_client_dir,
            None,
            "hub-client build failed",
        )?;
        println!("✓ hub-client build complete");
    }

    // Step: trace-viewer build (Phase 4.3+)
    if !config.skip_trace_viewer_build && trace_viewer_exists(&project_root) {
        step_idx += 1;
        banner(step_idx, total, "Building trace-viewer");
        let trace_viewer_dir = project_root.join("trace-viewer");
        run_command(
            "npm",
            &["run", "build"],
            &trace_viewer_dir,
            None,
            "trace-viewer build failed",
        )?;
        println!("✓ trace-viewer build complete");
    }

    // Step: q2-preview-spa build (bd-501n / Phase A.4)
    if !config.skip_q2_preview_spa_build && q2_preview_spa_exists(&project_root) {
        step_idx += 1;
        banner(step_idx, total, "Building q2-preview-spa");
        let spa_dir = project_root.join("q2-preview-spa");
        run_command(
            "npm",
            &["run", "build"],
            &spa_dir,
            None,
            "q2-preview-spa build failed",
        )?;
        println!("✓ q2-preview-spa build complete");
    }

    // Step: hub MCP bundle (bd-81cfshmw) — the artifact `q2 mcp`
    // embeds; must exist before the Rust build that `include_dir!`s it.
    if !config.skip_hub_mcp_bundle && hub_mcp_exists(&project_root) {
        step_idx += 1;
        banner(step_idx, total, "Building hub MCP bundle");
        let pkg_dir = project_root.join("ts-packages/quarto-hub-mcp");
        run_command(
            "npm",
            &["run", "bundle"],
            &pkg_dir,
            None,
            "hub MCP bundle build failed",
        )?;
        println!("✓ hub MCP bundle complete");
    }

    // Step: engine-host-deno bundle (Plan 1b) — the artifact `quarto-core`
    // embeds via `include_str!`; must exist before the Rust build.
    if !config.skip_engine_host_bundle && engine_host_exists(&project_root) {
        step_idx += 1;
        banner(step_idx, total, "Building engine-host-deno bundle");
        let pkg_dir = project_root.join("ts-packages/quarto-engine-host-deno");
        run_command(
            "npm",
            &["run", "bundle"],
            &pkg_dir,
            None,
            "engine-host-deno bundle build failed",
        )?;
        println!("✓ engine-host-deno bundle complete");
    }

    // Step: Rust workspace build
    if !config.skip_rust_build {
        step_idx += 1;
        let label = if config.release {
            "Building Rust workspace (--release)"
        } else {
            "Building Rust workspace"
        };
        banner(step_idx, total, label);
        let mut args: Vec<&str> = vec!["build", "--workspace"];
        if config.release {
            args.push("--release");
        }
        run_command("cargo", &args, &project_root, None, "Rust build failed")?;
        println!("✓ Rust build complete");
    }

    println!("\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("✓ Fresh build complete!");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");

    Ok(())
}

fn banner(step: u32, total: u32, label: &str) {
    println!("\n━━━ Step {}/{}: {} ━━━\n", step, total, label);
}

fn trace_viewer_exists(project_root: &Path) -> bool {
    project_root
        .join("trace-viewer")
        .join("package.json")
        .is_file()
}

fn q2_preview_spa_exists(project_root: &Path) -> bool {
    project_root
        .join("q2-preview-spa")
        .join("package.json")
        .is_file()
}

fn hub_mcp_exists(project_root: &Path) -> bool {
    project_root
        .join("ts-packages/quarto-hub-mcp")
        .join("package.json")
        .is_file()
}

fn engine_host_exists(project_root: &Path) -> bool {
    project_root
        .join("ts-packages/quarto-engine-host-deno")
        .join("package.json")
        .is_file()
}

/// Find the project root directory (where Cargo.toml with [workspace] lives).
fn find_project_root() -> Result<std::path::PathBuf> {
    let mut dir = std::env::current_dir().context("Failed to get current directory")?;

    loop {
        let cargo_toml = dir.join("Cargo.toml");
        if cargo_toml.exists() {
            let content =
                std::fs::read_to_string(&cargo_toml).context("Failed to read Cargo.toml")?;
            if content.contains("[workspace]") {
                return Ok(dir);
            }
        }

        if !dir.pop() {
            bail!("Could not find workspace root (Cargo.toml with [workspace])");
        }
    }
}

fn run_command(
    program: &str,
    args: &[&str],
    dir: &std::path::Path,
    rustflags: Option<&str>,
    error_msg: &str,
) -> Result<()> {
    // `nested_command` strips the outer `cargo xtask`'s package env vars so a
    // child cargo doesn't spuriously rebuild q2's TLS-stack closure (an
    // inherited CARGO_MANIFEST_DIR flips the `ring` build script dirty;
    // bd-awchm8w7), and on Windows runs `.cmd` shims like npm through `cmd /C`
    // so they resolve at all.
    let mut cmd = crate::util::nested_command(program);
    cmd.args(args).current_dir(dir);

    if let Some(flags) = rustflags {
        cmd.env("RUSTFLAGS", flags);
    }

    let status = cmd
        .status()
        .with_context(|| format!("Failed to run {} {:?}", program, args))?;

    if !status.success() {
        bail!("{}", error_msg);
    }

    Ok(())
}
