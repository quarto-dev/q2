//! Verify command - Full project verification.
//!
//! Runs all build and test steps to ensure the entire project is healthy,
//! matching the CI environment as closely as possible:
//! 1. Run custom lint checks
//! 2. Check Rust formatting (cargo fmt --check)
//! 3. Build all Rust crates (with -D warnings, matching CI)
//! 4. Test tree-sitter grammars
//! 5. Run all Rust tests (with -D warnings, matching CI)
//! 6. Build ts-packages workspaces + quarto-hub-mcp smoke check
//!    (bd-6rczoll3 — see `ts_packages.rs`)
//! 7. Build hub-client (including WASM)
//! 8. Run hub-client tests
//! 9. Build trace-viewer SPA
//! 10. Run trace-viewer tests
//! 11. Run shared workspace-package tests (@quarto/preview-renderer +
//!     @quarto/preview-runtime, both unit and integration)
//! 12. Run hub MCP package tests (quarto-sync-client + quarto-hub-mcp,
//!     including the q2-mcp bundle smoke test)
//! 13. Build q2-preview-spa placeholder
//! 14. q2-preview-spa Playwright E2E (only when --e2e is set)

use anyhow::{Context, Result, bail};
use std::process::Command;

use crate::lint;
use crate::test;

const TOTAL_STEPS: u32 = 14;

/// Configuration for the verify command.
pub struct VerifyConfig {
    /// Skip Rust build step.
    pub skip_rust_build: bool,
    /// Skip Rust tests.
    pub skip_rust_tests: bool,
    /// Skip the ts-packages build + quarto-hub-mcp smoke check.
    pub skip_ts_packages_build: bool,
    /// Skip hub-client build.
    pub skip_hub_build: bool,
    /// Skip hub-client tests.
    pub skip_hub_tests: bool,
    /// Skip trace-viewer build.
    pub skip_trace_viewer_build: bool,
    /// Skip trace-viewer tests.
    pub skip_trace_viewer_tests: bool,
    /// Skip tree-sitter grammar tests.
    pub skip_treesitter_tests: bool,
    /// Skip the CRLF parity run of tree-sitter grammar tests.
    pub skip_treesitter_crlf_tests: bool,
    /// Skip the shared preview-renderer + preview-runtime tests.
    pub skip_shared_package_tests: bool,
    /// Skip the q2-preview-spa placeholder build.
    pub skip_q2_preview_spa_build: bool,
    /// Skip the quarto-sync-client + quarto-hub-mcp package tests.
    pub skip_hub_mcp_tests: bool,
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
            skip_ts_packages_build: false,
            skip_hub_build: false,
            skip_hub_tests: false,
            skip_trace_viewer_build: false,
            skip_trace_viewer_tests: false,
            skip_treesitter_tests: false,
            skip_treesitter_crlf_tests: false,
            skip_shared_package_tests: false,
            skip_q2_preview_spa_build: false,
            skip_hub_mcp_tests: false,
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
        // On Windows, Cargo cannot re-link target/debug/xtask.exe while the
        // currently-running xtask holds a file lock on it. Linux/macOS
        // tolerate overwriting a running binary.
        #[cfg(windows)]
        let build_args: &[&str] = &["build", "--workspace", "--exclude", "xtask"];
        #[cfg(not(windows))]
        let build_args: &[&str] = &["build", "--workspace"];

        run_command(
            "cargo",
            build_args,
            &project_root,
            rustflags,
            "Rust build failed",
        )?;

        // On Windows we excluded xtask above; `cargo check -p xtask` still
        // gives xtask the `-D warnings` pass without relinking the running
        // exe (cargo check doesn't produce a binary). This keeps xtask
        // validated even under `cargo xtask verify --skip-rust-tests`.
        #[cfg(windows)]
        run_command(
            "cargo",
            &["check", "-p", "xtask"],
            &project_root,
            rustflags,
            "xtask check failed",
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

        if !config.skip_treesitter_crlf_tests {
            println!("\n  ↳ Re-running with CRLF line endings...");
            crate::treesitter_crlf::run_parity_check(&ts_dir)
                .context("Tree-sitter CRLF parity check failed")?;
            println!("  ✓ CRLF parity check complete");
        } else {
            println!("\n  ↳ Skipping CRLF parity check");
        }
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

    // Step 6: Build ts-packages workspaces + quarto-hub-mcp smoke check
    //
    // The ts-packages `dist/` output is consumed at runtime only by Node
    // consumers — concretely the quarto-hub-mcp server, which resolves
    // workspace imports via the `"import": "./dist/index.js"` export
    // condition. hub-client bundles these packages from source, so no
    // other step builds them (bd-6rczoll3).
    let ts_workspaces = crate::ts_packages::workspace_paths(&project_root);
    if !config.skip_ts_packages_build && !ts_workspaces.is_empty() {
        println!(
            "\n━━━ Step 6/{}: Building ts-packages workspaces ━━━\n",
            TOTAL_STEPS
        );
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

        // Smoke check: `--help` exits 0 only after the entire ESM graph
        // links, so a missing/stale dependency dist (the failure mode that
        // kills the MCP server with ERR_MODULE_NOT_FOUND) fails verify
        // instead of the next MCP session.
        if ts_workspaces.iter().any(|w| w.ends_with("quarto-hub-mcp")) {
            let mcp_entry = project_root.join("ts-packages/quarto-hub-mcp/dist/index.js");
            if !mcp_entry.is_file() {
                bail!(
                    "ts-packages build did not produce {} — quarto-hub-mcp's build script is broken",
                    mcp_entry.display()
                );
            }
            println!(
                "\n  ↳ Smoke-checking quarto-hub-mcp module graph (node dist/index.js --help)..."
            );
            let mcp_entry_str = mcp_entry.to_string_lossy();
            run_command(
                "node",
                &[mcp_entry_str.as_ref(), "--help"],
                &project_root,
                None,
                "quarto-hub-mcp smoke check failed (module graph did not load)",
            )?;
            println!("  ✓ quarto-hub-mcp module graph loads");
        }
        println!("✓ ts-packages build complete");
    } else if ts_workspaces.is_empty() {
        println!(
            "\n━━━ Step 6/{}: ts-packages/ not present, skipping ━━━\n",
            TOTAL_STEPS
        );
    } else {
        println!(
            "\n━━━ Step 6/{}: Skipping ts-packages build ━━━\n",
            TOTAL_STEPS
        );
    }

    // Step 7: Build hub-client (includes WASM)
    let hub_client_dir = project_root.join("hub-client");
    if !config.skip_hub_build {
        println!(
            "\n━━━ Step 7/{}: Building hub-client (includes WASM) ━━━\n",
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
            "\n━━━ Step 7/{}: Skipping hub-client build ━━━\n",
            TOTAL_STEPS
        );
    }

    // Step 8: Run hub-client tests
    if !config.skip_hub_tests {
        let test_script = if config.include_e2e {
            "test:all"
        } else {
            "test:ci"
        };
        println!(
            "\n━━━ Step 8/{}: Running hub-client tests ({}) ━━━\n",
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
            "\n━━━ Step 8/{}: Skipping hub-client tests ━━━\n",
            TOTAL_STEPS
        );
    }

    // Step 9: Build trace-viewer SPA
    let trace_viewer_dir = project_root.join("trace-viewer");
    let have_trace_viewer = trace_viewer_dir.join("package.json").is_file();
    if !config.skip_trace_viewer_build && have_trace_viewer {
        println!(
            "\n━━━ Step 9/{}: Building trace-viewer SPA ━━━\n",
            TOTAL_STEPS
        );
        run_command(
            "npm",
            &["run", "build"],
            &trace_viewer_dir,
            None,
            "trace-viewer build failed",
        )?;
        println!("✓ trace-viewer build complete");
    } else if !have_trace_viewer {
        println!(
            "\n━━━ Step 9/{}: trace-viewer/ not present, skipping ━━━\n",
            TOTAL_STEPS
        );
    } else {
        println!(
            "\n━━━ Step 9/{}: Skipping trace-viewer build ━━━\n",
            TOTAL_STEPS
        );
    }

    // Step 10: Run trace-viewer tests
    if !config.skip_trace_viewer_tests && have_trace_viewer {
        println!(
            "\n━━━ Step 10/{}: Running trace-viewer tests ━━━\n",
            TOTAL_STEPS
        );
        run_command(
            "npm",
            &["test"],
            &trace_viewer_dir,
            None,
            "trace-viewer tests failed",
        )?;
        println!("✓ trace-viewer tests complete");
    } else if !have_trace_viewer {
        println!(
            "\n━━━ Step 10/{}: trace-viewer/ not present, skipping ━━━\n",
            TOTAL_STEPS
        );
    } else {
        println!(
            "\n━━━ Step 10/{}: Skipping trace-viewer tests ━━━\n",
            TOTAL_STEPS
        );
    }

    // Step 11: Run shared workspace-package tests
    //
    // These run the @quarto/preview-renderer + @quarto/preview-runtime
    // unit + integration suites. The packages were carved out of
    // hub-client in bd-hfjj; they hold the bulk of the q2-preview-format
    // code now, and changes here aren't covered by hub-client's
    // npm scripts alone.
    let preview_renderer_dir = project_root.join("ts-packages/preview-renderer");
    let preview_runtime_dir = project_root.join("ts-packages/preview-runtime");
    let have_shared_packages = preview_renderer_dir.join("package.json").is_file()
        && preview_runtime_dir.join("package.json").is_file();
    if !config.skip_shared_package_tests && have_shared_packages {
        println!(
            "\n━━━ Step 11/{}: Running shared preview-* package tests ━━━\n",
            TOTAL_STEPS
        );
        run_command(
            "npm",
            &["test"],
            &preview_renderer_dir,
            None,
            "preview-renderer unit tests failed",
        )?;
        run_command(
            "npm",
            &["run", "test:integration"],
            &preview_renderer_dir,
            None,
            "preview-renderer integration tests failed",
        )?;
        run_command(
            "npm",
            &["test"],
            &preview_runtime_dir,
            None,
            "preview-runtime tests failed",
        )?;
        println!("✓ Shared package tests complete");
    } else if !have_shared_packages {
        println!(
            "\n━━━ Step 11/{}: shared preview-* packages not present, skipping ━━━\n",
            TOTAL_STEPS
        );
    } else {
        println!(
            "\n━━━ Step 11/{}: Skipping shared package tests ━━━\n",
            TOTAL_STEPS
        );
    }

    // Step 12: hub MCP package tests (bd-81cfshmw)
    //
    // quarto-sync-client + quarto-hub-mcp carry the MCP server that
    // `q2 mcp` embeds, including stdio-hygiene regression tests and the
    // bundle smoke test (which runs the esbuild bundler itself). Their
    // vitest suites spawn the tsc build (dist/index.js) and resolve
    // workspace deps through dist entries, so build in dependency
    // order first. (Step 6 typically pre-built these dists — the
    // builds here are incremental no-ops then — but this step stays
    // self-contained so --skip-ts-packages-build can't break it.)
    let schema_dir = project_root.join("ts-packages/quarto-automerge-schema");
    let sync_client_dir = project_root.join("ts-packages/quarto-sync-client");
    let hub_mcp_dir = project_root.join("ts-packages/quarto-hub-mcp");
    let have_hub_mcp = schema_dir.join("package.json").is_file()
        && sync_client_dir.join("package.json").is_file()
        && hub_mcp_dir.join("package.json").is_file();
    if !config.skip_hub_mcp_tests && have_hub_mcp {
        println!(
            "\n━━━ Step 12/{}: Running hub MCP package tests ━━━\n",
            TOTAL_STEPS
        );
        for (dir, what) in [
            (&schema_dir, "quarto-automerge-schema build"),
            (&sync_client_dir, "quarto-sync-client build"),
            (&hub_mcp_dir, "quarto-hub-mcp build"),
        ] {
            run_command(
                "npm",
                &["run", "build"],
                dir,
                None,
                &format!("{} failed", what),
            )?;
        }
        run_command(
            "npm",
            &["test"],
            &sync_client_dir,
            None,
            "quarto-sync-client tests failed",
        )?;
        run_command(
            "npm",
            &["test"],
            &hub_mcp_dir,
            None,
            "quarto-hub-mcp tests failed",
        )?;
        println!("✓ hub MCP package tests complete");
    } else if !have_hub_mcp {
        println!(
            "\n━━━ Step 12/{}: hub MCP packages not present, skipping ━━━\n",
            TOTAL_STEPS
        );
    } else {
        println!(
            "\n━━━ Step 12/{}: Skipping hub MCP package tests ━━━\n",
            TOTAL_STEPS
        );
    }

    // Step 13: Build q2-preview-spa placeholder
    //
    // The SPA is the future host of `quarto preview` (bd-kw93). Today
    // it's a skeleton — building it confirms the cross-package boundary
    // is healthy. Phase A of bd-kw93 will replace the placeholder with
    // the real wiring.
    let q2_preview_spa_dir = project_root.join("q2-preview-spa");
    let have_q2_preview_spa = q2_preview_spa_dir.join("package.json").is_file();
    if !config.skip_q2_preview_spa_build && have_q2_preview_spa {
        println!(
            "\n━━━ Step 13/{}: Building q2-preview-spa placeholder ━━━\n",
            TOTAL_STEPS
        );
        run_command(
            "npm",
            &["run", "build"],
            &q2_preview_spa_dir,
            None,
            "q2-preview-spa build failed",
        )?;
        println!("✓ q2-preview-spa build complete");
    } else if !have_q2_preview_spa {
        println!(
            "\n━━━ Step 13/{}: q2-preview-spa/ not present, skipping ━━━\n",
            TOTAL_STEPS
        );
    } else {
        println!(
            "\n━━━ Step 13/{}: Skipping q2-preview-spa build ━━━\n",
            TOTAL_STEPS
        );
    }

    // Step 14: q2-preview-spa Playwright E2E (gated on --e2e).
    //
    // The Playwright suite spawns the real `q2 preview` binary
    // against a temp fixture; for that to work we need the binary
    // built and Playwright's chromium present. The Rust build above
    // (step 3) already produces the debug binary at target/debug/q2;
    // the chromium browser is the developer's responsibility
    // (`npx playwright install chromium`).
    if config.include_e2e && have_q2_preview_spa {
        println!(
            "\n━━━ Step 14/{}: Running q2-preview-spa Playwright E2E ━━━\n",
            TOTAL_STEPS
        );
        run_command(
            "npm",
            &["run", "test:e2e"],
            &q2_preview_spa_dir,
            None,
            "q2-preview-spa Playwright E2E failed",
        )?;
        println!("✓ q2-preview-spa E2E complete");
    } else {
        let reason = if !have_q2_preview_spa {
            "q2-preview-spa/ not present"
        } else {
            "--e2e not set"
        };
        println!(
            "\n━━━ Step 14/{}: Skipping q2-preview-spa Playwright E2E ({}) ━━━\n",
            TOTAL_STEPS, reason
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
    // Nested cargo/npm: strip the outer `cargo xtask`'s package env vars so a
    // child cargo doesn't spuriously rebuild q2's TLS-stack closure (an
    // inherited CARGO_MANIFEST_DIR flips the `ring` build script dirty). See
    // `util::strip_inherited_cargo_env` (bd-awchm8w7).
    crate::util::strip_inherited_cargo_env(&mut cmd);

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
