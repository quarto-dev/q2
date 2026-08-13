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
    /// If set, the tool must report exactly this version string (matched as a
    /// substring of its `--version` output).  A wrong version is treated the
    /// same as "not installed" and the pinned version is (re-)installed.
    required_version: Option<&'static str>,
}

const TOOLS: &[Tool] = &[
    Tool {
        package: "cargo-nextest",
        check_cmd: "cargo",
        check_args: &["nextest", "--version"],
        required_version: None,
    },
    Tool {
        package: "tree-sitter-cli",
        check_cmd: "tree-sitter",
        check_args: &["--version"],
        required_version: None,
    },
    // wasm-bindgen-cli must exactly match the wasm-bindgen crate version locked
    // in crates/wasm-quarto-hub-client/Cargo.lock.  A version mismatch produces
    // a hard build error ("schema version … must exactly match").
    Tool {
        package: "wasm-bindgen-cli",
        check_cmd: "wasm-bindgen",
        check_args: &["--version"],
        required_version: Some("0.2.108"),
    },
];

fn is_installed(tool: &Tool) -> bool {
    let output = Command::new(tool.check_cmd).args(tool.check_args).output();

    let Ok(output) = output else { return false };
    if !output.status.success() {
        return false;
    }

    if let Some(required) = tool.required_version {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let combined = format!("{stdout}{stderr}");
        if !combined.contains(required) {
            let found = combined.lines().next().unwrap_or("unknown").trim();
            println!(
                "  {} — wrong version installed ({found}), need {required}",
                tool.package
            );
            return false;
        }
    }

    true
}

fn has_binstall() -> bool {
    Command::new("cargo")
        .args(["binstall", "--version"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

fn install(tool: &Tool, use_binstall: bool) -> Result<()> {
    let package = tool.package;
    // Build the versioned package spec, e.g. "wasm-bindgen-cli@0.2.108"
    let versioned = tool
        .required_version
        .map_or_else(|| package.to_string(), |v| format!("{package}@{v}"));

    if use_binstall {
        println!("  Installing {versioned} via cargo binstall...");
        let status = Command::new("cargo")
            .args(["binstall", "--no-confirm", &versioned])
            .status()
            .with_context(|| format!("Failed to run cargo binstall {versioned}"))?;

        if status.success() {
            return Ok(());
        }
        println!("  binstall failed, falling back to cargo install...");
    }

    let mut args = vec!["install", "--locked"];
    if let Some(v) = tool.required_version {
        args.extend(["--version", v]);
    }
    args.push(package);

    println!("  Installing {versioned} via cargo install...");
    let status = Command::new("cargo")
        .args(&args)
        .status()
        .with_context(|| format!("Failed to run cargo install {versioned}"))?;

    if !status.success() {
        bail!("Failed to install {versioned}");
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
            install(tool, use_binstall)?;
            installed += 1;
        }
    }

    println!();
    if installed == 0 {
        println!("All {already} tools already installed. Nothing to do.");
    } else {
        println!("Installed {installed} tool(s), {already} already present.");
    }

    check_cmake();
    check_pandoc();
    check_wasm_opt();

    Ok(())
}

/// Check for cmake (required — `cargo build --workspace` pulls in
/// wasmtime-c-api-impl transitively via quarto-highlight, and its build.rs
/// shells out to cmake).
fn check_cmake() {
    let Ok(output) = Command::new("cmake").arg("--version").output() else {
        warn_cmake_missing();
        return;
    };
    if !output.status.success() {
        warn_cmake_missing();
        return;
    }

    let first_line = String::from_utf8_lossy(&output.stdout)
        .lines()
        .next()
        .unwrap_or("cmake")
        .trim()
        .to_string();
    println!("\n  {first_line} — detected");
}

fn warn_cmake_missing() {
    println!(
        "\n  Warning: cmake not found. `cargo build --workspace` will fail.\n  \
         cmake is required transitively by quarto-highlight (wasmtime-c-api-impl).\n  \
         Install:"
    );
    for line in cmake_install_hints() {
        println!("    {line}");
    }
}

/// Platform-specific cmake install commands. Order matters: preferred first.
fn cmake_install_hints() -> &'static [&'static str] {
    #[cfg(windows)]
    {
        &["scoop install cmake", "winget install Kitware.CMake"]
    }
    #[cfg(target_os = "macos")]
    {
        &["brew install cmake"]
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        &[
            "apt install cmake    # Debian/Ubuntu",
            "dnf install cmake    # Fedora/RHEL",
        ]
    }
    #[cfg(not(any(windows, unix)))]
    {
        &["See https://cmake.org/download/"]
    }
}

/// Check for wasm-opt (required — `npm run build:wasm` runs `wasm-opt -Oz`
/// on the hub-client WASM after wasm-bindgen; live-share payload plan
/// Phase 1). Not cargo-installed: the `wasm-opt` crate builds all of
/// binaryen from C++ source, which is a heavy install when every platform
/// already packages the binary.
fn check_wasm_opt() {
    let Ok(output) = Command::new("wasm-opt").arg("--version").output() else {
        warn_wasm_opt_missing();
        return;
    };
    if !output.status.success() {
        warn_wasm_opt_missing();
        return;
    }

    let first_line = String::from_utf8_lossy(&output.stdout)
        .lines()
        .next()
        .unwrap_or("wasm-opt")
        .trim()
        .to_string();
    println!("\n  {first_line} — detected");
}

fn warn_wasm_opt_missing() {
    println!(
        "\n  Warning: wasm-opt not found. `npm run build:wasm` (hub-client) will fail.\n  \
         wasm-opt ships with binaryen. Install:"
    );
    for line in wasm_opt_install_hints() {
        println!("    {line}");
    }
}

/// Platform-specific binaryen install commands. Order matters: preferred first.
fn wasm_opt_install_hints() -> &'static [&'static str] {
    #[cfg(windows)]
    {
        &["npm install -g binaryen", "scoop install binaryen"]
    }
    #[cfg(target_os = "macos")]
    {
        &["brew install binaryen", "npm install -g binaryen"]
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        &[
            "apt install binaryen          # Debian/Ubuntu",
            "dnf install binaryen          # Fedora/RHEL",
            "npm install -g binaryen       # any OS with node",
        ]
    }
    #[cfg(not(any(windows, unix)))]
    {
        &["npm install -g binaryen"]
    }
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

/// Parse the first line of `pandoc --version` output into `(major, minor)`.
/// Expects format like "pandoc 3.6.1" or "pandoc 3.6". Malformed or empty
/// input parses as `(0, 0)`.
///
/// Duplicated (byte-for-byte identical behavior, pinned by
/// [`PANDOC_VERSION_PARSE_CASES`]) in
/// `crates/pampa/tests/integration/test.rs` — xtask is bin-only, so pampa's
/// test binary can't import this directly without pulling xtask's deps into
/// the test build. See bd-i9i5ad2t.
pub(crate) fn parse_pandoc_version(version_output: &str) -> (u32, u32) {
    let first_line = version_output.lines().next().unwrap_or("");
    let version_part = first_line.strip_prefix("pandoc ").unwrap_or(first_line);
    let mut parts = version_part.split('.');
    let major: u32 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let minor: u32 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    (major, minor)
}

/// Check if a parsed pandoc `--version` output is at least `major.minor`.
fn pandoc_version_at_least(version_output: &str, min_major: u32, min_minor: u32) -> bool {
    parse_pandoc_version(version_output) >= (min_major, min_minor)
}

/// Shared parse test vectors, duplicated identically in
/// `crates/pampa/tests/integration/test.rs`. Keep both copies in lockstep.
#[cfg(test)]
const PANDOC_VERSION_PARSE_CASES: &[(&str, (u32, u32))] = &[
    ("pandoc 3.6\n...", (3, 6)),
    ("pandoc 3.6.1\n...", (3, 6)),
    ("pandoc 3.9\n...", (3, 9)),
    ("pandoc 3.10\n...", (3, 10)),
    ("pandoc 4.0\n...", (4, 0)),
    ("pandoc 3.1.3\n...", (3, 1)),
    ("pandoc 2.19\n...", (2, 19)),
    ("garbage\n...", (0, 0)),
    ("", (0, 0)),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_pandoc_version_cases() {
        for (input, expected) in PANDOC_VERSION_PARSE_CASES {
            assert_eq!(parse_pandoc_version(input), *expected, "parsing {input:?}");
        }
    }

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
