//! Dev setup command - Install required development tools.
//!
//! Checks for required tools and installs any that are missing.
//! Uses `cargo-binstall` for faster installs when available,
//! falling back to `cargo install --locked` otherwise.

use anyhow::{Context, Result, bail};
use std::process::Command;

/// A tool required for development.
struct Tool {
    /// Cargo package name (used for install).
    package: &'static str,
    /// Command and args to check if the tool is installed.
    check_cmd: &'static str,
    check_args: &'static [&'static str],
}

const TOOLS: &[Tool] = &[
    Tool {
        package: "cargo-nextest",
        check_cmd: "cargo",
        check_args: &["nextest", "--version"],
    },
    Tool {
        package: "wasm-pack",
        check_cmd: "wasm-pack",
        check_args: &["--version"],
    },
];

fn is_installed(tool: &Tool) -> bool {
    Command::new(tool.check_cmd)
        .args(tool.check_args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

fn has_binstall() -> bool {
    Command::new("cargo")
        .args(["binstall", "--version"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

fn install(package: &str, use_binstall: bool) -> Result<()> {
    let (program, args): (&str, Vec<&str>) = if use_binstall {
        ("cargo", vec!["binstall", "--no-confirm", package])
    } else {
        ("cargo", vec!["install", "--locked", package])
    };

    let method = if use_binstall { "binstall" } else { "install" };
    println!("  Installing {package} via cargo {method}...");

    let status = Command::new(program)
        .args(&args)
        .status()
        .with_context(|| format!("Failed to run cargo {method} {package}"))?;

    if !status.success() {
        bail!("Failed to install {package}");
    }

    Ok(())
}

pub fn run() -> Result<()> {
    println!("Checking development tools...\n");

    let use_binstall = has_binstall();
    if use_binstall {
        println!("  cargo-binstall detected — using binary installs\n");
    }

    let mut installed = 0u32;
    let mut already = 0u32;

    for tool in TOOLS {
        if is_installed(tool) {
            println!("  {} — already installed", tool.package);
            already += 1;
        } else {
            install(tool.package, use_binstall)?;
            installed += 1;
        }
    }

    println!();
    if installed == 0 {
        println!("All {already} tools already installed. Nothing to do.");
    } else {
        println!("Installed {installed} tool(s), {already} already present.");
    }

    Ok(())
}
