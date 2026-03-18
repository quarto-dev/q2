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
    if use_binstall {
        println!("  Installing {package} via cargo binstall...");
        let status = Command::new("cargo")
            .args(["binstall", "--no-confirm", package])
            .status()
            .with_context(|| format!("Failed to run cargo binstall {package}"))?;

        if status.success() {
            return Ok(());
        }
        println!("  binstall failed, falling back to cargo install...");
    }

    println!("  Installing {package} via cargo install...");
    let status = Command::new("cargo")
        .args(["install", "--locked", package])
        .status()
        .with_context(|| format!("Failed to run cargo install {package}"))?;

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

    check_pandoc();

    Ok(())
}

/// Check for Pandoc 3.6+ (optional — needed only for pampa comparison tests).
fn check_pandoc() {
    let output = Command::new("pandoc").arg("--version").output();

    let Ok(output) = output else {
        println!(
            "\n  Warning: pandoc not found. Four pampa comparison tests will fail.\n  \
             Install from https://pandoc.org/installing.html"
        );
        return;
    };

    let version_str = String::from_utf8_lossy(&output.stdout);
    let is_good = pandoc_version_at_least(&version_str, 3, 6);

    if is_good {
        println!("\n  pandoc — suitable version detected");
    } else {
        // Extract first line for display (e.g. "pandoc 3.1.3")
        let first_line = version_str.lines().next().unwrap_or("unknown version");
        println!(
            "\n  Warning: {first_line} detected — pampa comparison tests require 3.6+.\n  \
             Four tests will fail. Update from https://pandoc.org/installing.html"
        );
    }
}

/// Parse the first line of `pandoc --version` output and check if the version
/// is at least `major.minor`. Expects format like "pandoc 3.6.1" or "pandoc 3.6".
fn pandoc_version_at_least(version_output: &str, min_major: u32, min_minor: u32) -> bool {
    let first_line = version_output.lines().next().unwrap_or("");
    // Extract version string after "pandoc "
    let version_part = first_line.strip_prefix("pandoc ").unwrap_or(first_line);
    let mut parts = version_part.split('.');
    let major: u32 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let minor: u32 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    (major, minor) >= (min_major, min_minor)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pandoc_version_at_least() {
        assert!(pandoc_version_at_least("pandoc 3.6\n...", 3, 6));
        assert!(pandoc_version_at_least("pandoc 3.6.1\n...", 3, 6));
        assert!(pandoc_version_at_least("pandoc 3.9\n...", 3, 6));
        assert!(pandoc_version_at_least("pandoc 4.0\n...", 3, 6));
        assert!(!pandoc_version_at_least("pandoc 3.1.3\n...", 3, 6));
        assert!(!pandoc_version_at_least("pandoc 3.5.9\n...", 3, 6));
        assert!(!pandoc_version_at_least("pandoc 2.19\n...", 3, 6));
        assert!(!pandoc_version_at_least("", 3, 6));
    }
}
