//! Verify command - Full project verification.
//!
//! Runs all build and test steps to ensure the entire project is healthy:
//! 1. Build all Rust crates
//! 2. Run all Rust tests
//! 3. Build hub-client (including WASM)
//! 4. Run hub-client tests

use anyhow::{Context, Result, bail};
use std::process::Command;

/// Configuration for the verify command.
pub struct VerifyConfig {
    /// Skip Rust build step.
    pub skip_rust_build: bool,
    /// Skip Rust tests.
    pub skip_rust_tests: bool,
    /// Skip hub-client build.
    pub skip_hub_build: bool,
    /// Skip hub-client tests.
    pub skip_hub_tests: bool,
    /// Run hub-client e2e tests (slower, requires browser).
    pub include_e2e: bool,
}

impl Default for VerifyConfig {
    fn default() -> Self {
        Self {
            skip_rust_build: false,
            skip_rust_tests: false,
            skip_hub_build: false,
            skip_hub_tests: false,
            include_e2e: false,
        }
    }
}

/// Run the verify command.
pub fn run(config: &VerifyConfig) -> Result<()> {
    let project_root = find_project_root()?;

    // Step 1: Build Rust workspace
    if !config.skip_rust_build {
        println!("\n━━━ Step 1/4: Building Rust workspace ━━━\n");
        run_command(
            "cargo",
            &["build", "--workspace"],
            &project_root,
            "Rust build failed",
        )?;
        println!("✓ Rust build complete");
    } else {
        println!("\n━━━ Step 1/4: Skipping Rust build ━━━\n");
    }

    // Step 2: Run Rust tests
    if !config.skip_rust_tests {
        println!("\n━━━ Step 2/4: Running Rust tests ━━━\n");
        run_command(
            "cargo",
            &["nextest", "run", "--workspace"],
            &project_root,
            "Rust tests failed",
        )?;
        println!("✓ Rust tests complete");
    } else {
        println!("\n━━━ Step 2/4: Skipping Rust tests ━━━\n");
    }

    // Step 3: Build hub-client (includes WASM)
    let hub_client_dir = project_root.join("hub-client");
    if !config.skip_hub_build {
        println!("\n━━━ Step 3/4: Building hub-client (includes WASM) ━━━\n");
        run_command(
            "npm",
            &["run", "build:all"],
            &hub_client_dir,
            "hub-client build failed",
        )?;
        println!("✓ hub-client build complete");
    } else {
        println!("\n━━━ Step 3/4: Skipping hub-client build ━━━\n");
    }

    // Step 4: Run hub-client tests
    if !config.skip_hub_tests {
        let test_script = if config.include_e2e {
            "test:all"
        } else {
            "test:ci"
        };
        println!(
            "\n━━━ Step 4/4: Running hub-client tests ({}) ━━━\n",
            test_script
        );
        run_command(
            "npm",
            &["run", test_script],
            &hub_client_dir,
            "hub-client tests failed",
        )?;
        println!("✓ hub-client tests complete");
    } else {
        println!("\n━━━ Step 4/4: Skipping hub-client tests ━━━\n");
    }

    println!("\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("✓ All verification steps passed!");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");

    Ok(())
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

/// Run a command and check for success.
fn run_command(program: &str, args: &[&str], dir: &std::path::Path, error_msg: &str) -> Result<()> {
    let status = Command::new(program)
        .args(args)
        .current_dir(dir)
        .status()
        .with_context(|| format!("Failed to run {} {:?}", program, args))?;

    if !status.success() {
        bail!("{}", error_msg);
    }

    Ok(())
}
