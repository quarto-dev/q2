/*
 * engine/knitr/subprocess.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * R subprocess management for knitr engine.
 */

//! R subprocess management for the knitr engine.
//!
//! This module provides functions for:
//! - Finding the Rscript binary on the system
//! - Detecting active renv projects
//! - Spawning R subprocesses and communicating via JSON
//!
//! # Finding Rscript
//!
//! The [`find_rscript`] function searches for Rscript in this order:
//! 1. `QUARTO_R` environment variable (path to R installation or Rscript binary)
//! 2. System PATH via `which`
//!
//! # R Communication Protocol
//!
//! Communication with R uses a JSON protocol:
//! - Request is written to stdin
//! - Response is written to a temp file (path specified in request)
//!
//! ```json
//! // Request (stdin)
//! {
//!   "action": "execute",
//!   "params": { ... },
//!   "results": "/tmp/results.json",
//!   "wd": "/project"
//! }
//!
//! // Response (in results file)
//! {
//!   "engine": "knitr",
//!   "markdown": "...",
//!   ...
//! }
//! ```

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::Serialize;
use serde::de::DeserializeOwned;
use tempfile::NamedTempFile;

use super::KNITR_RESOURCES;
use super::error_parser::{RErrorType, parse_r_error};
use super::types::KnitrRequest;
use crate::engine::error::ExecutionError;

// ============================================================================
// Rscript Discovery
// ============================================================================

/// Find the Rscript binary on the system.
///
/// Searches in this order:
/// 1. `QUARTO_R` environment variable - can be:
///    - Path to R installation directory (looks for `bin/Rscript`)
///    - Direct path to Rscript binary
/// 2. System PATH via `which`
///
/// # Returns
///
/// `Some(path)` if Rscript is found, `None` otherwise.
///
/// # Examples
///
/// ```ignore
/// if let Some(rscript) = find_rscript() {
///     println!("Found Rscript at: {}", rscript.display());
/// }
/// ```
pub fn find_rscript() -> Option<PathBuf> {
    // First, check QUARTO_R environment variable
    if let Ok(quarto_r) = std::env::var("QUARTO_R") {
        let quarto_r_path = PathBuf::from(&quarto_r);

        // If QUARTO_R points directly to Rscript binary
        if quarto_r_path.is_file() && is_rscript(&quarto_r_path) {
            return Some(quarto_r_path);
        }

        // If QUARTO_R is a directory, look for Rscript inside it
        if quarto_r_path.is_dir() {
            // Try bin/Rscript (standard R installation layout)
            let rscript_in_bin = quarto_r_path.join("bin").join(rscript_name());
            if rscript_in_bin.is_file() {
                return Some(rscript_in_bin);
            }

            // Try Rscript directly in the directory
            let rscript_direct = quarto_r_path.join(rscript_name());
            if rscript_direct.is_file() {
                return Some(rscript_direct);
            }
        }
    }

    // Fall back to PATH lookup
    which::which("Rscript").ok()
}

/// Get the platform-appropriate Rscript binary name.
fn rscript_name() -> &'static str {
    #[cfg(windows)]
    {
        "Rscript.exe"
    }
    #[cfg(not(windows))]
    {
        "Rscript"
    }
}

/// Check if a path looks like an Rscript binary.
fn is_rscript(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .map(|name| name == "Rscript" || name == "Rscript.exe")
        .unwrap_or(false)
}

// ============================================================================
// renv Detection
// ============================================================================

/// Check if a directory is within an active renv project.
///
/// Returns `true` if the directory contains an `.Rprofile` that sources
/// `renv/activate.R` without being commented out.
///
/// This affects working directory selection: when inside an active renv,
/// R should be run from the document's directory rather than the project root.
///
/// # Arguments
///
/// * `dir` - Directory to check (typically the document's parent directory)
///
/// # Examples
///
/// ```ignore
/// let doc_dir = Path::new("/project/analysis");
/// if within_active_renv(doc_dir) {
///     // Run R from doc_dir
/// } else {
///     // Run R from project root
/// }
/// ```
pub fn within_active_renv(dir: &Path) -> bool {
    let rprofile = dir.join(".Rprofile");

    let Ok(content) = std::fs::read_to_string(&rprofile) else {
        return false;
    };

    // Look for the renv activation line
    // It should be: source("renv/activate.R")
    // But NOT: # source("renv/activate.R") (commented out)
    for line in content.lines() {
        let trimmed = line.trim();

        // Skip empty lines and comments
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        // Check for the activation pattern
        // Allow some flexibility in quoting: source("renv/activate.R") or source('renv/activate.R')
        if trimmed.contains("source(") && trimmed.contains("renv/activate.R") {
            return true;
        }
    }

    false
}

// ============================================================================
// R Subprocess Communication
// ============================================================================

/// Options for calling R.
#[derive(Debug, Clone, Default)]
pub struct CallROptions {
    /// Whether to suppress stderr output (capture instead of inherit).
    pub quiet: bool,

    /// Additional arguments to pass to Rscript.
    /// Can be set via `QUARTO_KNITR_RSCRIPT_ARGS` environment variable.
    pub extra_args: Vec<String>,

    /// Optional callback to filter/transform stderr output on error.
    /// Receives the stderr content and returns the filtered version.
    pub stderr_filter: Option<fn(&str) -> String>,
}

impl CallROptions {
    /// Create options with quiet mode enabled.
    pub fn quiet() -> Self {
        Self {
            quiet: true,
            ..Default::default()
        }
    }

    /// Parse additional Rscript args from environment variable.
    ///
    /// The `QUARTO_KNITR_RSCRIPT_ARGS` variable should contain
    /// comma-separated arguments, e.g., `--vanilla,--no-init-file`.
    fn parse_env_args() -> Vec<String> {
        std::env::var("QUARTO_KNITR_RSCRIPT_ARGS")
            .unwrap_or_default()
            .split(',')
            .filter(|s| !s.trim().is_empty())
            .map(|s| s.trim().to_string())
            .collect()
    }
}

/// Call R with the given action and parameters.
///
/// This function:
/// 1. Creates a temporary file for results
/// 2. Serializes the request to JSON
/// 3. Spawns Rscript with the rmd.R script
/// 4. Writes the request JSON to stdin
/// 5. Waits for completion
/// 6. Reads and parses the results
///
/// # Type Parameters
///
/// * `P` - The parameters type (must be `Serialize`)
/// * `R` - The result type (must be `DeserializeOwned`)
///
/// # Arguments
///
/// * `action` - The action to perform ("execute", "dependencies", etc.)
/// * `params` - Action-specific parameters
/// * `temp_dir` - Directory for temporary files
/// * `working_dir` - Working directory for the R process
/// * `options` - Additional options (quiet mode, extra args, etc.)
///
/// # Errors
///
/// Returns an error if:
/// - Rscript is not found
/// - Failed to create temp files
/// - R subprocess fails
/// - Result JSON cannot be parsed
///
/// # Examples
///
/// ```ignore
/// let params = KnitrExecuteParams { ... };
/// let result: KnitrExecuteResult = call_r(
///     "execute",
///     &params,
///     temp_dir,
///     working_dir,
///     &CallROptions::quiet(),
/// )?;
/// ```
pub fn call_r<P, R>(
    action: &str,
    params: &P,
    temp_dir: &Path,
    working_dir: &Path,
    options: &CallROptions,
) -> Result<R, ExecutionError>
where
    P: Serialize,
    R: DeserializeOwned,
{
    // Find Rscript
    let rscript = find_rscript().ok_or_else(|| {
        ExecutionError::runtime_not_found(
            "knitr",
            "Rscript (install R from https://www.r-project.org/)",
        )
    })?;

    // Get resource directory
    let resource_dir = KNITR_RESOURCES.path().map_err(|e| {
        ExecutionError::temp_file(format!("Failed to extract R resources: {}", e), None)
    })?;

    let rmd_script = resource_dir.join("rmd").join("rmd.R");
    if !rmd_script.exists() {
        return Err(ExecutionError::temp_file(
            format!("R script not found: {}", rmd_script.display()),
            Some(rmd_script),
        ));
    }

    // Create temp file for results
    let results_file = NamedTempFile::new_in(temp_dir).map_err(|e| {
        ExecutionError::temp_file(format!("Failed to create results file: {}", e), None)
    })?;
    let results_path = results_file.path().to_path_buf();

    // Build request
    let request = KnitrRequest::new(
        action,
        params,
        results_path.clone(),
        working_dir.to_path_buf(),
    );

    let request_json = serde_json::to_string(&request)
        .map_err(|e| ExecutionError::other(format!("Failed to serialize request: {}", e)))?;

    // Collect extra args (from env var and options)
    let mut args: Vec<String> = CallROptions::parse_env_args();
    args.extend(options.extra_args.iter().cloned());

    // Build command
    let mut cmd = Command::new(&rscript);
    cmd.args(&args)
        .arg(&rmd_script)
        .current_dir(working_dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped());

    if options.quiet {
        cmd.stderr(Stdio::piped());
    } else {
        cmd.stderr(Stdio::inherit());
    }

    // Spawn process
    let mut child = cmd.spawn().map_err(|e| {
        ExecutionError::other(format!(
            "Failed to spawn Rscript ({}): {}",
            rscript.display(),
            e
        ))
    })?;

    // Write request to stdin
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(request_json.as_bytes()).map_err(|e| {
            ExecutionError::other(format!("Failed to write to Rscript stdin: {}", e))
        })?;
    }

    // Wait for completion
    let output = child
        .wait_with_output()
        .map_err(|e| ExecutionError::other(format!("Failed to wait for Rscript: {}", e)))?;

    // Check exit status
    if !output.status.success() {
        let mut stderr = String::from_utf8_lossy(&output.stderr).into_owned();

        // Apply filter if provided
        if let Some(filter) = options.stderr_filter {
            stderr = filter(&stderr);
        }

        // Parse the R error for better error messages
        let error_info = parse_r_error(&stderr);

        return Err(convert_r_error_to_execution_error(&error_info, &stderr));
    }

    // Read results file
    let results_json = std::fs::read_to_string(&results_path).map_err(|e| {
        ExecutionError::temp_file(
            format!(
                "Failed to read results file ({}): {}",
                results_path.display(),
                e
            ),
            Some(results_path.clone()),
        )
    })?;

    // Parse results
    let result: R = serde_json::from_str(&results_json).map_err(|e| {
        ExecutionError::other(format!(
            "Failed to parse R results: {}\nJSON: {}",
            e,
            truncate_for_error(&results_json, 500)
        ))
    })?;

    Ok(result)
}

/// Truncate a string for error messages.
fn truncate_for_error(s: &str, max_len: usize) -> &str {
    if s.len() <= max_len { s } else { &s[..max_len] }
}

/// Convert parsed R error info to an ExecutionError.
///
/// This function takes the parsed error information and the original stderr
/// content and returns the most appropriate ExecutionError variant.
fn convert_r_error_to_execution_error(
    error_info: &super::error_parser::RErrorInfo,
    stderr: &str,
) -> ExecutionError {
    match &error_info.error_type {
        RErrorType::MissingPackage { package } => {
            ExecutionError::missing_package("knitr", package.clone(), error_info.suggestion.clone())
        }

        RErrorType::PackageVersionTooOld {
            package,
            required_version,
        } => ExecutionError::package_version_too_old(
            "knitr",
            package.clone(),
            required_version.clone(),
            error_info.suggestion.clone(),
        ),

        RErrorType::KnitrExecutionError { .. } => {
            if let Some(ref lines) = error_info.source_lines {
                ExecutionError::execution_failed_at_lines(
                    "knitr",
                    error_info.message.clone(),
                    lines.start,
                    lines.end.saturating_sub(1), // Convert exclusive to inclusive
                )
            } else {
                ExecutionError::execution_failed("knitr", error_info.message.clone())
            }
        }

        RErrorType::RNotFound => ExecutionError::runtime_not_found(
            "knitr",
            "Rscript (install R from https://www.r-project.org/)",
        ),

        RErrorType::Generic => {
            // For generic errors, provide the cleaned message or fall back to full stderr
            let message = if error_info.message.is_empty() {
                if stderr.is_empty() {
                    "R process failed".to_string()
                } else {
                    format!("R process failed:\n{}", stderr.trim())
                }
            } else {
                error_info.message.clone()
            };

            ExecutionError::execution_failed("knitr", message)
        }
    }
}

/// Determine the working directory for R execution.
///
/// The working directory is determined as follows:
/// - If the document is in an active renv project, use the document's directory
/// - Otherwise, use the project directory (if provided) or the document's directory
///
/// # Arguments
///
/// * `document_dir` - The document's parent directory
/// * `project_dir` - The project root directory (if known)
///
/// # Returns
///
/// The directory to use as the working directory for R.
pub fn determine_working_dir(document_dir: &Path, project_dir: Option<&Path>) -> PathBuf {
    if within_active_renv(document_dir) {
        document_dir.to_path_buf()
    } else {
        project_dir
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| document_dir.to_path_buf())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // === find_rscript tests ===

    #[test]
    fn test_rscript_name_unix() {
        #[cfg(not(windows))]
        assert_eq!(rscript_name(), "Rscript");
    }

    #[test]
    fn test_rscript_name_windows() {
        #[cfg(windows)]
        assert_eq!(rscript_name(), "Rscript.exe");
    }

    #[test]
    fn test_is_rscript() {
        assert!(is_rscript(Path::new("/usr/bin/Rscript")));
        assert!(is_rscript(Path::new("Rscript")));
        assert!(is_rscript(Path::new("Rscript.exe")));
        assert!(!is_rscript(Path::new("/usr/bin/R")));
        assert!(!is_rscript(Path::new("/usr/bin/python")));
        assert!(!is_rscript(Path::new("R")));
    }

    // === within_active_renv tests ===

    #[test]
    fn test_within_active_renv_no_rprofile() {
        let temp_dir = tempfile::tempdir().unwrap();
        assert!(!within_active_renv(temp_dir.path()));
    }

    #[test]
    fn test_within_active_renv_empty_rprofile() {
        let temp_dir = tempfile::tempdir().unwrap();
        let rprofile = temp_dir.path().join(".Rprofile");
        std::fs::write(&rprofile, "").unwrap();

        assert!(!within_active_renv(temp_dir.path()));
    }

    #[test]
    fn test_within_active_renv_active() {
        let temp_dir = tempfile::tempdir().unwrap();
        let rprofile = temp_dir.path().join(".Rprofile");
        std::fs::write(&rprofile, r#"source("renv/activate.R")"#).unwrap();

        assert!(within_active_renv(temp_dir.path()));
    }

    #[test]
    fn test_within_active_renv_active_single_quotes() {
        let temp_dir = tempfile::tempdir().unwrap();
        let rprofile = temp_dir.path().join(".Rprofile");
        std::fs::write(&rprofile, "source('renv/activate.R')").unwrap();

        assert!(within_active_renv(temp_dir.path()));
    }

    #[test]
    fn test_within_active_renv_commented_out() {
        let temp_dir = tempfile::tempdir().unwrap();
        let rprofile = temp_dir.path().join(".Rprofile");
        std::fs::write(&rprofile, r#"# source("renv/activate.R")"#).unwrap();

        assert!(!within_active_renv(temp_dir.path()));
    }

    #[test]
    fn test_within_active_renv_with_other_content() {
        let temp_dir = tempfile::tempdir().unwrap();
        let rprofile = temp_dir.path().join(".Rprofile");
        std::fs::write(
            &rprofile,
            r#"
# My R profile
options(repos = c(CRAN = "https://cloud.r-project.org"))
source("renv/activate.R")
# Load common packages
"#,
        )
        .unwrap();

        assert!(within_active_renv(temp_dir.path()));
    }

    #[test]
    fn test_within_active_renv_deactivated() {
        let temp_dir = tempfile::tempdir().unwrap();
        let rprofile = temp_dir.path().join(".Rprofile");
        // Simulates renv::deactivate() which comments out the line
        std::fs::write(
            &rprofile,
            r#"
# renv was deactivated
#source("renv/activate.R")
"#,
        )
        .unwrap();

        assert!(!within_active_renv(temp_dir.path()));
    }

    // === determine_working_dir tests ===

    #[test]
    fn test_determine_working_dir_no_renv_no_project() {
        let doc_dir = PathBuf::from("/project/docs");
        let result = determine_working_dir(&doc_dir, None);
        assert_eq!(result, doc_dir);
    }

    #[test]
    fn test_determine_working_dir_no_renv_with_project() {
        let doc_dir = PathBuf::from("/project/docs");
        let project_dir = PathBuf::from("/project");
        let result = determine_working_dir(&doc_dir, Some(&project_dir));
        assert_eq!(result, project_dir);
    }

    #[test]
    fn test_determine_working_dir_with_renv() {
        let temp_dir = tempfile::tempdir().unwrap();
        let rprofile = temp_dir.path().join(".Rprofile");
        std::fs::write(&rprofile, r#"source("renv/activate.R")"#).unwrap();

        let project_dir = PathBuf::from("/project");
        let result = determine_working_dir(temp_dir.path(), Some(&project_dir));

        // Should use doc_dir because renv is active
        assert_eq!(result, temp_dir.path());
    }

    // === CallROptions tests ===

    #[test]
    fn test_call_r_options_default() {
        let opts = CallROptions::default();
        assert!(!opts.quiet);
        assert!(opts.extra_args.is_empty());
        assert!(opts.stderr_filter.is_none());
    }

    #[test]
    fn test_call_r_options_quiet() {
        let opts = CallROptions::quiet();
        assert!(opts.quiet);
    }

    #[test]
    fn test_parse_env_args_empty() {
        // Ensure env var is unset for this test
        // SAFETY: Test runs in a single thread context
        unsafe { std::env::remove_var("QUARTO_KNITR_RSCRIPT_ARGS") };
        let args = CallROptions::parse_env_args();
        assert!(args.is_empty());
    }

    #[test]
    fn test_parse_env_args_with_values() {
        // SAFETY: Test runs in a single thread context
        unsafe { std::env::set_var("QUARTO_KNITR_RSCRIPT_ARGS", "--vanilla,--no-init-file") };
        let args = CallROptions::parse_env_args();
        assert_eq!(args, vec!["--vanilla", "--no-init-file"]);

        // Clean up
        // SAFETY: Test runs in a single thread context
        unsafe { std::env::remove_var("QUARTO_KNITR_RSCRIPT_ARGS") };
    }

    #[test]
    fn test_parse_env_args_with_whitespace() {
        // SAFETY: Test runs in a single thread context
        unsafe { std::env::set_var("QUARTO_KNITR_RSCRIPT_ARGS", " --vanilla , --no-init-file ") };
        let args = CallROptions::parse_env_args();
        assert_eq!(args, vec!["--vanilla", "--no-init-file"]);

        // Clean up
        // SAFETY: Test runs in a single thread context
        unsafe { std::env::remove_var("QUARTO_KNITR_RSCRIPT_ARGS") };
    }

    // === Truncation helper test ===

    #[test]
    fn test_truncate_for_error() {
        assert_eq!(truncate_for_error("short", 100), "short");
        assert_eq!(truncate_for_error("short", 5), "short");
        assert_eq!(truncate_for_error("longer string", 5), "longe");
    }

    // === Error conversion tests ===

    #[test]
    fn test_convert_r_error_missing_package() {
        use super::super::error_parser::RErrorInfo;

        let error_info = RErrorInfo {
            error_type: RErrorType::MissingPackage {
                package: "knitr".to_string(),
            },
            message: "R package 'knitr' is not installed".to_string(),
            suggestion: Some("Install with install.packages(\"knitr\")".to_string()),
            source_lines: None,
        };

        let err = convert_r_error_to_execution_error(&error_info, "");

        assert!(matches!(err, ExecutionError::MissingPackage { .. }));
        let msg = format!("{}", err);
        assert!(msg.contains("knitr"));
    }

    #[test]
    fn test_convert_r_error_package_version() {
        use super::super::error_parser::RErrorInfo;

        let error_info = RErrorInfo {
            error_type: RErrorType::PackageVersionTooOld {
                package: "knitr".to_string(),
                required_version: "1.44".to_string(),
            },
            message: "R package 'knitr' version 1.44 or higher is required".to_string(),
            suggestion: Some("Update with install.packages(\"knitr\")".to_string()),
            source_lines: None,
        };

        let err = convert_r_error_to_execution_error(&error_info, "");

        assert!(matches!(err, ExecutionError::PackageVersionTooOld { .. }));
        let msg = format!("{}", err);
        assert!(msg.contains("knitr"));
        assert!(msg.contains("1.44"));
    }

    #[test]
    fn test_convert_r_error_knitr_execution_with_lines() {
        use super::super::error_parser::RErrorInfo;

        let error_info = RErrorInfo {
            error_type: RErrorType::KnitrExecutionError {
                source_file: Some("test.Rmd".to_string()),
            },
            message: "object 'x' not found".to_string(),
            suggestion: None,
            source_lines: Some(5..9), // Lines 5-8 (exclusive range)
        };

        let err = convert_r_error_to_execution_error(&error_info, "");

        assert!(matches!(err, ExecutionError::ExecutionFailedAtLines { .. }));
        let msg = format!("{}", err);
        assert!(msg.contains("5"));
        assert!(msg.contains("8")); // End line (inclusive)
        assert!(msg.contains("object 'x' not found"));
    }

    #[test]
    fn test_convert_r_error_knitr_execution_without_lines() {
        use super::super::error_parser::RErrorInfo;

        let error_info = RErrorInfo {
            error_type: RErrorType::KnitrExecutionError { source_file: None },
            message: "something went wrong".to_string(),
            suggestion: None,
            source_lines: None,
        };

        let err = convert_r_error_to_execution_error(&error_info, "");

        assert!(matches!(err, ExecutionError::ExecutionFailed { .. }));
        let msg = format!("{}", err);
        assert!(msg.contains("something went wrong"));
    }

    #[test]
    fn test_convert_r_error_r_not_found() {
        use super::super::error_parser::RErrorInfo;

        let error_info = RErrorInfo {
            error_type: RErrorType::RNotFound,
            message: "R is not installed".to_string(),
            suggestion: Some("Install R from https://www.r-project.org/".to_string()),
            source_lines: None,
        };

        let err = convert_r_error_to_execution_error(&error_info, "");

        assert!(matches!(err, ExecutionError::RuntimeNotFound { .. }));
    }

    #[test]
    fn test_convert_r_error_generic() {
        use super::super::error_parser::RErrorInfo;

        let error_info = RErrorInfo {
            error_type: RErrorType::Generic,
            message: "Something bad happened".to_string(),
            suggestion: None,
            source_lines: None,
        };

        let err = convert_r_error_to_execution_error(&error_info, "");

        assert!(matches!(err, ExecutionError::ExecutionFailed { .. }));
        let msg = format!("{}", err);
        assert!(msg.contains("Something bad happened"));
    }

    #[test]
    fn test_convert_r_error_generic_empty_message_uses_stderr() {
        use super::super::error_parser::RErrorInfo;

        let error_info = RErrorInfo {
            error_type: RErrorType::Generic,
            message: "".to_string(),
            suggestion: None,
            source_lines: None,
        };

        let err = convert_r_error_to_execution_error(&error_info, "Raw stderr output");

        let msg = format!("{}", err);
        assert!(msg.contains("Raw stderr output"));
    }

    // === Integration tests (require R installation) ===

    mod integration {
        use super::*;
        use crate::engine::knitr::format::KnitrFormatConfig;
        use crate::engine::knitr::types::{KnitrExecuteParams, KnitrExecuteResult};

        /// Test basic R communication with a simple code chunk.
        ///
        /// This test requires R and the knitr/rmarkdown packages to be installed.
        /// Run with: `cargo nextest run --ignored -- test_call_r_basic_execution`
        #[test]
        #[ignore]
        fn test_call_r_basic_execution() {
            // Skip if Rscript is not available
            let Some(_rscript) = find_rscript() else {
                eprintln!("Skipping test: Rscript not found");
                return;
            };

            let temp_dir = tempfile::tempdir().unwrap();
            let temp_path = temp_dir.path();

            // Create a minimal qmd file
            let qmd_path = temp_path.join("test.qmd");
            std::fs::write(&qmd_path, "# Test\n\n```{r}\n1 + 1\n```\n").unwrap();

            // Get resource directory
            let resource_dir = super::super::KNITR_RESOURCES
                .path()
                .expect("Failed to get resources path");

            // Build execute params
            let params = KnitrExecuteParams {
                input: qmd_path.clone(),
                markdown: "# Test\n\n```{r}\n1 + 1\n```\n".to_string(),
                format: KnitrFormatConfig::with_defaults("html"),
                temp_dir: temp_path.to_path_buf(),
                lib_dir: None,
                dependencies: true,
                cwd: temp_path.to_path_buf(),
                params: None,
                resource_dir: resource_dir.to_path_buf(),
                handled_languages: vec!["ojs".to_string(), "mermaid".to_string()],
            };

            // Call R
            let result: KnitrExecuteResult = call_r(
                "execute",
                &params,
                temp_path,
                temp_path,
                &CallROptions::quiet(),
            )
            .expect("R execution failed");

            // Verify result
            assert_eq!(result.engine, "knitr");
            assert!(
                result.markdown.contains("[1] 2"),
                "Expected output to contain '[1] 2', got:\n{}",
                result.markdown
            );
        }

        /// Test R execution with multiple chunks.
        #[test]
        #[ignore]
        fn test_call_r_multiple_chunks() {
            let Some(_rscript) = find_rscript() else {
                eprintln!("Skipping test: Rscript not found");
                return;
            };

            let temp_dir = tempfile::tempdir().unwrap();
            let temp_path = temp_dir.path();

            let qmd_path = temp_path.join("test.qmd");
            let markdown = r#"# Test

```{r}
x <- 10
x
```

```{r}
x * 2
```
"#;
            std::fs::write(&qmd_path, markdown).unwrap();

            let resource_dir = super::super::KNITR_RESOURCES
                .path()
                .expect("Failed to get resources path");

            let params = KnitrExecuteParams {
                input: qmd_path.clone(),
                markdown: markdown.to_string(),
                format: KnitrFormatConfig::with_defaults("html"),
                temp_dir: temp_path.to_path_buf(),
                lib_dir: None,
                dependencies: true,
                cwd: temp_path.to_path_buf(),
                params: None,
                resource_dir: resource_dir.to_path_buf(),
                handled_languages: vec![],
            };

            let result: KnitrExecuteResult = call_r(
                "execute",
                &params,
                temp_path,
                temp_path,
                &CallROptions::quiet(),
            )
            .expect("R execution failed");

            assert!(
                result.markdown.contains("[1] 10"),
                "Expected output to contain '[1] 10'"
            );
            assert!(
                result.markdown.contains("[1] 20"),
                "Expected output to contain '[1] 20'"
            );
        }

        /// Test R execution with echo: false.
        #[test]
        #[ignore]
        fn test_call_r_echo_false() {
            let Some(_rscript) = find_rscript() else {
                eprintln!("Skipping test: Rscript not found");
                return;
            };

            let temp_dir = tempfile::tempdir().unwrap();
            let temp_path = temp_dir.path();

            let qmd_path = temp_path.join("test.qmd");
            let markdown = r#"# Test

```{r}
#| echo: false
1 + 1
```
"#;
            std::fs::write(&qmd_path, markdown).unwrap();

            let resource_dir = super::super::KNITR_RESOURCES
                .path()
                .expect("Failed to get resources path");

            let params = KnitrExecuteParams {
                input: qmd_path.clone(),
                markdown: markdown.to_string(),
                format: KnitrFormatConfig::with_defaults("html"),
                temp_dir: temp_path.to_path_buf(),
                lib_dir: None,
                dependencies: true,
                cwd: temp_path.to_path_buf(),
                params: None,
                resource_dir: resource_dir.to_path_buf(),
                handled_languages: vec![],
            };

            let result: KnitrExecuteResult = call_r(
                "execute",
                &params,
                temp_path,
                temp_path,
                &CallROptions::quiet(),
            )
            .expect("R execution failed");

            // Output should be present
            assert!(
                result.markdown.contains("[1] 2"),
                "Expected output to contain '[1] 2'"
            );
        }
    }
}
