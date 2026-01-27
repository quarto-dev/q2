//! Custom lint checks for Quarto Rust.
//!
//! This module provides lint checks that catch issues standard Rust linters miss.
//! Each lint rule is implemented as a separate submodule.

mod external_sources;

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use walkdir::WalkDir;

/// Configuration for lint runs.
pub struct LintConfig {
    /// Show verbose output including all files checked.
    pub verbose: bool,
    /// Only show errors, no progress or summary.
    pub quiet: bool,
}

/// A lint violation found in the codebase.
#[derive(Debug)]
pub struct Violation {
    /// Path to the file containing the violation.
    pub file: PathBuf,
    /// Line number (1-indexed) where the violation occurs.
    pub line: usize,
    /// Column number (1-indexed) where the violation starts.
    pub column: usize,
    /// Name of the lint rule that was violated.
    pub rule: &'static str,
    /// Human-readable description of the violation.
    pub message: String,
    /// Optional suggestion for how to fix the violation.
    pub suggestion: Option<String>,
}

impl std::fmt::Display for Violation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}:{}:{}: [{}] {}",
            self.file.display(),
            self.line,
            self.column,
            self.rule,
            self.message
        )?;
        if let Some(suggestion) = &self.suggestion {
            write!(f, "\n  suggestion: {}", suggestion)?;
        }
        Ok(())
    }
}

/// Run all lint checks on the codebase.
pub fn run(config: &LintConfig) -> Result<()> {
    let workspace_root = find_workspace_root()?;
    let crates_dir = workspace_root.join("crates");

    if !config.quiet {
        eprintln!("Running lint checks on {}", crates_dir.display());
    }

    // Collect all Rust files
    let rust_files = find_rust_files(&crates_dir)?;

    if config.verbose {
        eprintln!("Found {} Rust files to check", rust_files.len());
    }

    // Run all lint checks
    let mut all_violations = Vec::new();

    for file in &rust_files {
        if config.verbose {
            eprintln!("Checking {}", file.display());
        }

        let violations = check_file(file)?;
        all_violations.extend(violations);
    }

    // Report results
    if all_violations.is_empty() {
        if !config.quiet {
            eprintln!("\nAll checks passed! ({} files checked)", rust_files.len());
        }
        Ok(())
    } else {
        eprintln!();
        for violation in &all_violations {
            eprintln!("{}\n", violation);
        }

        if !config.quiet {
            eprintln!(
                "Found {} violation(s) in {} file(s) ({} files checked)",
                all_violations.len(),
                all_violations
                    .iter()
                    .map(|v| &v.file)
                    .collect::<std::collections::HashSet<_>>()
                    .len(),
                rust_files.len()
            );
        }

        // Exit with error code 1 to indicate violations were found
        std::process::exit(1);
    }
}

/// Find the workspace root directory by looking for the root Cargo.toml.
fn find_workspace_root() -> Result<PathBuf> {
    // Start from the current directory and walk up
    let current_dir = std::env::current_dir().context("Failed to get current working directory")?;

    for ancestor in current_dir.ancestors() {
        let cargo_toml = ancestor.join("Cargo.toml");
        if cargo_toml.exists() {
            // Check if this is a workspace root by looking for [workspace] section
            let content =
                std::fs::read_to_string(&cargo_toml).context("Failed to read Cargo.toml")?;
            if content.contains("[workspace]") {
                return Ok(ancestor.to_path_buf());
            }
        }
    }

    anyhow::bail!(
        "Could not find workspace root (Cargo.toml with [workspace] section) \
         starting from {}",
        current_dir.display()
    )
}

/// Find all Rust source files in the given directory.
fn find_rust_files(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();

    for entry in WalkDir::new(dir)
        .into_iter()
        .filter_entry(|e| !is_hidden(e) && !is_target_dir(e))
    {
        let entry = entry.context("Failed to read directory entry")?;
        let path = entry.path();

        if path.is_file() && path.extension().is_some_and(|ext| ext == "rs") {
            files.push(path.to_path_buf());
        }
    }

    // Sort for deterministic output
    files.sort();

    Ok(files)
}

/// Check if a directory entry is hidden (starts with .).
fn is_hidden(entry: &walkdir::DirEntry) -> bool {
    entry
        .file_name()
        .to_str()
        .is_some_and(|s| s.starts_with('.'))
}

/// Check if a directory entry is a target directory (Cargo build output).
fn is_target_dir(entry: &walkdir::DirEntry) -> bool {
    entry.file_name().to_str().is_some_and(|s| s == "target")
}

/// Run all lint checks on a single file.
fn check_file(path: &Path) -> Result<Vec<Violation>> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read file: {}", path.display()))?;

    let mut violations = Vec::new();

    // Run each lint rule
    violations.extend(external_sources::check(path, &content)?);

    Ok(violations)
}
