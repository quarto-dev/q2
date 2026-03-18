//! Verify command - Full project verification.
//!
//! Runs all build and test steps to ensure the entire project is healthy,
//! matching the CI environment as closely as possible:
//! 1. Run custom lint checks
//! 2. Check Rust formatting (cargo fmt --check)
//! 3. Build all Rust crates (with -D warnings, matching CI)
//! 4. Test tree-sitter grammars
//! 5. Run all Rust tests (with -D warnings, matching CI)
//! 6. Build hub-client (including WASM)
//! 7. Run hub-client tests

use anyhow::{Context, Result, bail};
use std::process::Command;

use crate::lint;
use crate::test;

const TOTAL_STEPS: u32 = 7;

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
    /// Skip tree-sitter grammar tests.
    pub skip_treesitter_tests: bool,
    /// Run hub-client e2e tests (slower, requires browser).
    pub include_e2e: bool,
    /// Do not set RUSTFLAGS="-D warnings" (allows warnings during iteration).
    pub no_deny_warnings: bool,
}

impl Default for VerifyConfig {
    fn default() -> Self {
        Self {
            skip_rust_build: false,
            skip_rust_tests: false,
            skip_hub_build: false,
            skip_hub_tests: false,
            skip_treesitter_tests: false,
            include_e2e: false,
            no_deny_warnings: false,
        }
    }
}

/// Run the verify command.
pub fn run(config: &VerifyConfig) -> Result<()> {
    let project_root = find_project_root()?;

    let rustflags = if config.no_deny_warnings {
        None
    } else {
        Some("-D warnings")
    };

    // Step 1: Custom lint checks
    {
        println!(
            "\n━━━ Step 1/{}: Running custom lint checks ━━━\n",
            TOTAL_STEPS
        );
        let lint_config = lint::LintConfig {
            verbose: false,
            quiet: false,
        };
        lint::run_check(&lint_config)?;
        println!("✓ Custom lint checks complete");
    }

    // Step 2: Check Rust formatting
    {
        println!(
            "\n━━━ Step 2/{}: Checking Rust formatting ━━━\n",
            TOTAL_STEPS
        );
        run_command(
            "cargo",
            &["fmt", "--check"],
            &project_root,
            None,
            "Rust formatting check failed (run `cargo fmt` to fix)",
        )?;
        println!("✓ Rust formatting check complete");
    }

    // Step 3: Build Rust workspace
    if !config.skip_rust_build {
        println!(
            "\n━━━ Step 3/{}: Building Rust workspace{} ━━━\n",
            TOTAL_STEPS,
            if rustflags.is_some() {
                " (warnings denied)"
            } else {
                ""
            }
        );
        run_command(
            "cargo",
            &["build", "--workspace"],
            &project_root,
            rustflags,
            "Rust build failed",
        )?;
        println!("✓ Rust build complete");
    } else {
        println!("\n━━━ Step 3/{}: Skipping Rust build ━━━\n", TOTAL_STEPS);
    }

    // Step 4: Tree-sitter grammar tests
    if !config.skip_treesitter_tests {
        println!(
            "\n━━━ Step 4/{}: Testing tree-sitter grammars ━━━\n",
            TOTAL_STEPS
        );
        let ts_dir = project_root.join("crates/tree-sitter-qmd/tree-sitter-markdown");
        run_command(
            "tree-sitter",
            &["test"],
            &ts_dir,
            None,
            "Tree-sitter grammar tests failed",
        )?;
        println!("✓ Tree-sitter grammar tests complete");
    } else {
        println!(
            "\n━━━ Step 4/{}: Skipping tree-sitter grammar tests ━━━\n",
            TOTAL_STEPS
        );
    }

    // Step 5: Run Rust tests
    if !config.skip_rust_tests {
        println!(
            "\n━━━ Step 5/{}: Running Rust tests{} ━━━\n",
            TOTAL_STEPS,
            if rustflags.is_some() {
                " (warnings denied)"
            } else {
                ""
            }
        );
        let nextest_args = test::nextest_base_args();
        let nextest_refs: Vec<&str> = nextest_args.iter().map(|s| s.as_str()).collect();
        run_command(
            "cargo",
            &nextest_refs,
            &project_root,
            rustflags,
            "Rust tests failed",
        )?;
        println!("✓ Rust tests complete");
    } else {
        println!("\n━━━ Step 5/{}: Skipping Rust tests ━━━\n", TOTAL_STEPS);
    }

    // Step 6: Build hub-client (includes WASM)
    let hub_client_dir = project_root.join("hub-client");
    if !config.skip_hub_build {
        println!(
            "\n━━━ Step 6/{}: Building hub-client (includes WASM) ━━━\n",
            TOTAL_STEPS
        );
        run_command(
            "npm",
            &["run", "build:all"],
            &hub_client_dir,
            None,
            "hub-client build failed",
        )?;
        println!("✓ hub-client build complete");
    } else {
        println!(
            "\n━━━ Step 6/{}: Skipping hub-client build ━━━\n",
            TOTAL_STEPS
        );
    }

    // Step 7: Run hub-client tests
    if !config.skip_hub_tests {
        let test_script = if config.include_e2e {
            "test:all"
        } else {
            "test:ci"
        };
        println!(
            "\n━━━ Step 7/{}: Running hub-client tests ({}) ━━━\n",
            TOTAL_STEPS, test_script
        );
        run_command(
            "npm",
            &["run", test_script],
            &hub_client_dir,
            None,
            "hub-client tests failed",
        )?;
        println!("✓ hub-client tests complete");
    } else {
        println!(
            "\n━━━ Step 7/{}: Skipping hub-client tests ━━━\n",
            TOTAL_STEPS
        );
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
///
/// If `rustflags` is provided, it is set as the `RUSTFLAGS` environment variable
/// for the command, matching CI behavior.
fn run_command(
    program: &str,
    args: &[&str],
    dir: &std::path::Path,
    rustflags: Option<&str>,
    error_msg: &str,
) -> Result<()> {
    let mut cmd = Command::new(program);
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
