//! Test command - Platform-aware test runner.
//!
//! Runs `cargo nextest run --workspace`, automatically excluding crates that
//! cannot compile tests on the current platform.
//!
//! On Windows, the v8 crate does not produce rlib, causing test compilation
//! to fail for 12 crates that transitively depend on it via
//! `quarto-system-runtime`. This command auto-adds `--exclude` flags for
//! those crates so contributors don't need to remember them.
//!
//! On macOS/Linux, this is equivalent to `cargo nextest run --workspace`.

use anyhow::{Context, Result, bail};
use std::process::Command;

/// Crates excluded from test compilation on Windows.
///
/// The v8 crate (via deno_core) does not produce rlib on Windows. Test binaries
/// require rlib from every dependency for static linking, so all crates that
/// transitively depend on v8 via `quarto-system-runtime` fail to compile tests.
///
/// See `claude-notes/instructions/windows-dev.md` for the full dependency cascade.
const WINDOWS_EXCLUDED_CRATES: &[&str] = &[
    "quarto-system-runtime",
    "pampa",
    "quarto-core",
    "quarto-sass",
    "quarto-test",
    "quarto",
    "quarto-project-create",
    "qmd-syntax-helper",
    "comrak-to-pandoc",
    "quarto-lsp",
    "quarto-lsp-core",
    "reconcile-viewer",
];

/// Build nextest arguments with platform-appropriate excludes.
///
/// Returns the base arguments for `cargo nextest run`, including `--workspace`
/// and any platform-specific `--exclude` flags. Additional arguments (filters,
/// `--no-fail-fast`, etc.) should be appended by the caller.
pub fn nextest_base_args() -> Vec<String> {
    let mut args = vec![
        "nextest".to_string(),
        "run".to_string(),
        "--workspace".to_string(),
    ];

    if cfg!(target_os = "windows") {
        for krate in WINDOWS_EXCLUDED_CRATES {
            args.push("--exclude".to_string());
            args.push(krate.to_string());
        }
    }

    args
}

/// Run the test command.
pub fn run(extra_args: &[String], rustflags: Option<&str>) -> Result<()> {
    let project_root = find_project_root()?;

    let mut args = nextest_base_args();

    if !extra_args.is_empty() {
        args.extend(extra_args.iter().cloned());
    }

    if cfg!(target_os = "windows") {
        let excluded = WINDOWS_EXCLUDED_CRATES.len();
        println!(
            "Running tests (Windows: excluding {} v8-dependent crates)",
            excluded
        );
    } else {
        println!("Running tests");
    }

    let args_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();

    let mut cmd = Command::new("cargo");
    cmd.args(&args_refs).current_dir(&project_root);

    if let Some(flags) = rustflags {
        cmd.env("RUSTFLAGS", flags);
    }

    let status = cmd
        .status()
        .with_context(|| format!("Failed to run cargo {:?}", args_refs))?;

    if !status.success() {
        bail!("Tests failed");
    }

    println!("\n✓ Tests complete");
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nextest_base_args_includes_workspace() {
        let args = nextest_base_args();
        assert_eq!(&args[..3], &["nextest", "run", "--workspace"]);
    }

    #[test]
    #[cfg(target_os = "windows")]
    fn nextest_base_args_excludes_v8_crates_on_windows() {
        let args = nextest_base_args();
        // 3 base args + 2 per excluded crate (--exclude + name)
        assert_eq!(args.len(), 3 + WINDOWS_EXCLUDED_CRATES.len() * 2);
        assert!(args.contains(&"--exclude".to_string()));
        assert!(args.contains(&"quarto-system-runtime".to_string()));
    }

    #[test]
    #[cfg(not(target_os = "windows"))]
    fn nextest_base_args_no_excludes_on_non_windows() {
        let args = nextest_base_args();
        assert_eq!(args.len(), 3);
    }
}
