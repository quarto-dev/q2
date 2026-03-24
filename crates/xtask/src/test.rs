//! Test command - runs `cargo nextest run --workspace`.

use anyhow::{Context, Result, bail};
use std::process::Command;

/// Build nextest arguments.
///
/// Returns the base arguments for `cargo nextest run`, including `--workspace`.
/// Additional arguments (filters, `--no-fail-fast`, etc.) should be appended
/// by the caller.
pub fn nextest_base_args() -> Vec<String> {
    vec![
        "nextest".to_string(),
        "run".to_string(),
        "--workspace".to_string(),
    ]
}

/// Run the test command.
pub fn run(extra_args: &[String], rustflags: Option<&str>) -> Result<()> {
    let project_root = find_project_root()?;

    let mut args = nextest_base_args();

    if !extra_args.is_empty() {
        args.extend(extra_args.iter().cloned());
    }

    println!("Running tests");

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
        assert_eq!(args, vec!["nextest", "run", "--workspace"]);
    }
}
