/*
 * engine/knitr/error_parser.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Parse R error messages for better user feedback.
 */

//! Parse R error messages to provide better user feedback.
//!
//! R and knitr produce various error messages that we can parse to:
//! - Detect missing packages and suggest installation commands
//! - Extract line numbers from knitr "Quitting from lines X-Y" messages
//! - Provide actionable suggestions for common errors
//!
//! # Error Patterns
//!
//! ## Missing Package
//! ```text
//! Error in library(knitr) : there is no package called 'knitr'
//! ```
//!
//! ## Knitr Execution Error
//! ```text
//! Quitting from lines 5-8 (document.Rmd): Error in eval(expr, envir, enclos): object 'x' not found
//! ```
//!
//! ## Package Version Error
//! ```text
//! Error: knitr >= 1.44 is required for rendering with Quarto from `.R` files.
//! ```

use std::ops::Range;

use regex::Regex;

/// Parsed information from an R error message.
#[derive(Debug, Clone, PartialEq)]
pub struct RErrorInfo {
    /// The type of error detected.
    pub error_type: RErrorType,

    /// The original error message (possibly cleaned up).
    pub message: String,

    /// Suggested action for the user.
    pub suggestion: Option<String>,

    /// Source line range if available (1-indexed, inclusive).
    /// This is from knitr's "Quitting from lines X-Y" messages.
    pub source_lines: Option<Range<usize>>,
}

/// Types of R errors we can detect.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RErrorType {
    /// A required R package is not installed.
    MissingPackage {
        /// Name of the missing package.
        package: String,
    },

    /// A package version is too old.
    PackageVersionTooOld {
        /// Name of the package.
        package: String,
        /// Required minimum version.
        required_version: String,
    },

    /// knitr execution error with line information.
    KnitrExecutionError {
        /// The source file name (as reported by knitr).
        source_file: Option<String>,
    },

    /// R was not found on the system.
    RNotFound,

    /// Generic R error.
    Generic,
}

/// Parse R stderr output to extract structured error information.
///
/// This function looks for common patterns in R error messages and
/// returns structured information that can be used to provide better
/// error messages to the user.
///
/// # Examples
///
/// ```ignore
/// let stderr = "Error in library(knitr) : there is no package called 'knitr'";
/// let info = parse_r_error(stderr);
/// assert!(matches!(info.error_type, RErrorType::MissingPackage { .. }));
/// ```
pub fn parse_r_error(stderr: &str) -> RErrorInfo {
    // Try each parser in order of specificity

    // 1. Missing package
    if let Some(info) = parse_missing_package(stderr) {
        return info;
    }

    // 2. Package version too old
    if let Some(info) = parse_package_version(stderr) {
        return info;
    }

    // 3. Knitr line number error
    if let Some(info) = parse_knitr_error(stderr) {
        return info;
    }

    // 4. R not found (various forms)
    if let Some(info) = parse_r_not_found(stderr) {
        return info;
    }

    // Default: generic error
    RErrorInfo {
        error_type: RErrorType::Generic,
        message: clean_error_message(stderr),
        suggestion: None,
        source_lines: None,
    }
}

/// Parse "there is no package called 'X'" errors.
fn parse_missing_package(stderr: &str) -> Option<RErrorInfo> {
    // Pattern: "there is no package called 'X'"
    // Can appear in various contexts:
    //   - Error in library(X) : there is no package called 'X'
    //   - Error in loadNamespace(name) : there is no package called 'X'
    let re = Regex::new(r"there is no package called '([^']+)'").unwrap();

    if let Some(caps) = re.captures(stderr) {
        let package = caps.get(1).unwrap().as_str().to_string();
        let suggestion = format!(
            "Install the package with: install.packages(\"{}\")",
            package
        );

        return Some(RErrorInfo {
            error_type: RErrorType::MissingPackage {
                package: package.clone(),
            },
            message: format!("R package '{}' is not installed", package),
            suggestion: Some(suggestion),
            source_lines: None,
        });
    }

    // Alternative pattern: "Package 'X' required but not available"
    // Sometimes produced by requireNamespace()
    let re2 = Regex::new(r"(?i)package '([^']+)' required but").unwrap();
    if let Some(caps) = re2.captures(stderr) {
        let package = caps.get(1).unwrap().as_str().to_string();
        let suggestion = format!(
            "Install the package with: install.packages(\"{}\")",
            package
        );

        return Some(RErrorInfo {
            error_type: RErrorType::MissingPackage {
                package: package.clone(),
            },
            message: format!("R package '{}' is not installed", package),
            suggestion: Some(suggestion),
            source_lines: None,
        });
    }

    None
}

/// Parse package version requirement errors.
fn parse_package_version(stderr: &str) -> Option<RErrorInfo> {
    // Pattern from rmd.R: "knitr >= 1.44 is required"
    let re = Regex::new(r"(\w+)\s*>=\s*([\d.]+)\s+is required").unwrap();

    if let Some(caps) = re.captures(stderr) {
        let package = caps.get(1).unwrap().as_str().to_string();
        let version = caps.get(2).unwrap().as_str().to_string();

        let suggestion = format!("Update the package with: install.packages(\"{}\")", package);

        return Some(RErrorInfo {
            error_type: RErrorType::PackageVersionTooOld {
                package: package.clone(),
                required_version: version.clone(),
            },
            message: format!(
                "R package '{}' version {} or higher is required",
                package, version
            ),
            suggestion: Some(suggestion),
            source_lines: None,
        });
    }

    None
}

/// Parse knitr "Quitting from lines X-Y" errors.
fn parse_knitr_error(stderr: &str) -> Option<RErrorInfo> {
    // Pattern: "Quitting from lines X-Y (file.Rmd)..."
    // or "Quitting from lines X-Y ..."
    let re = Regex::new(r"Quitting from lines\s+(\d+)-(\d+)\s*(?:\(([^)]+)\))?[:\s]*(.+?)(?:\n|$)")
        .unwrap();

    if let Some(caps) = re.captures(stderr) {
        let start_line: usize = caps.get(1).unwrap().as_str().parse().ok()?;
        let end_line: usize = caps.get(2).unwrap().as_str().parse().ok()?;
        let source_file = caps.get(3).map(|m| m.as_str().to_string());
        let error_detail = caps.get(4).map(|m| m.as_str().trim().to_string());

        // Clean up the error message
        let message = if let Some(ref detail) = error_detail {
            clean_error_message(detail)
        } else {
            clean_error_message(stderr)
        };

        return Some(RErrorInfo {
            error_type: RErrorType::KnitrExecutionError { source_file },
            message,
            suggestion: None,
            source_lines: Some(start_line..end_line + 1), // Convert to exclusive range
        });
    }

    None
}

/// Parse "R not found" style errors.
fn parse_r_not_found(stderr: &str) -> Option<RErrorInfo> {
    let patterns = [
        r"(?i)Rscript.*not found",
        r"(?i)command not found.*Rscript",
        r"(?i)cannot find.*Rscript",
        r"(?i)R is not installed",
    ];

    for pattern in patterns {
        let re = Regex::new(pattern).unwrap();
        if re.is_match(stderr) {
            return Some(RErrorInfo {
                error_type: RErrorType::RNotFound,
                message: "R is not installed or not in PATH".to_string(),
                suggestion: Some("Install R from https://www.r-project.org/".to_string()),
                source_lines: None,
            });
        }
    }

    None
}

/// Clean up an R error message for display.
///
/// This removes common noise and formats the message nicely.
fn clean_error_message(message: &str) -> String {
    let mut cleaned = message.to_string();

    // Remove ANSI color codes
    let ansi_re = Regex::new(r"\x1b\[[0-9;]*m").unwrap();
    cleaned = ansi_re.replace_all(&cleaned, "").to_string();

    // Trim first so the regex can match at the start
    cleaned = cleaned.trim().to_string();

    // Remove "Error: " or "Error in X : " prefix (we'll add our own context)
    let error_prefix_re = Regex::new(r"^Error(?:\s+in\s+[^:]+)?\s*:\s*").unwrap();
    cleaned = error_prefix_re.replace(&cleaned, "").to_string();

    // Trim again after removing prefix
    cleaned = cleaned.trim().to_string();

    // Collapse multiple newlines
    let multi_newline_re = Regex::new(r"\n{3,}").unwrap();
    cleaned = multi_newline_re.replace_all(&cleaned, "\n\n").to_string();

    cleaned
}

/// Format an R error for display to the user.
///
/// This creates a nicely formatted error message with the error details
/// and any suggestions.
///
/// # Note
///
/// Public API for displaying R errors. Not currently called from production code
/// but useful for CLI tools and error display. Has unit tests in this module.
#[allow(dead_code)]
pub fn format_r_error(info: &RErrorInfo) -> String {
    let mut parts = vec![info.message.clone()];

    // Add line information if available
    if let Some(ref lines) = info.source_lines {
        parts.push(format!(
            "  at lines {}-{}",
            lines.start,
            lines.end.saturating_sub(1)
        ));
    }

    // Add suggestion if available
    if let Some(ref suggestion) = info.suggestion {
        parts.push(format!("\nSuggestion: {}", suggestion));
    }

    parts.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    // === Missing Package Tests ===

    #[test]
    fn test_parse_missing_package_library() {
        let stderr = "Error in library(knitr) : there is no package called 'knitr'";
        let info = parse_r_error(stderr);

        assert!(matches!(
            info.error_type,
            RErrorType::MissingPackage { ref package } if package == "knitr"
        ));
        assert!(info.suggestion.unwrap().contains("install.packages"));
    }

    #[test]
    fn test_parse_missing_package_loadnamespace() {
        let stderr = "Error in loadNamespace(name) : there is no package called 'rmarkdown'";
        let info = parse_r_error(stderr);

        assert!(matches!(
            info.error_type,
            RErrorType::MissingPackage { ref package } if package == "rmarkdown"
        ));
    }

    #[test]
    fn test_parse_missing_package_require() {
        let stderr = "package 'ggplot2' required but not available";
        let info = parse_r_error(stderr);

        assert!(matches!(
            info.error_type,
            RErrorType::MissingPackage { ref package } if package == "ggplot2"
        ));
    }

    #[test]
    fn test_parse_missing_package_multiline() {
        let stderr = r#"
Loading required package: knitr
Error in library(knitr) : there is no package called 'knitr'
Execution halted
"#;
        let info = parse_r_error(stderr);

        assert!(matches!(
            info.error_type,
            RErrorType::MissingPackage { ref package } if package == "knitr"
        ));
    }

    // === Package Version Tests ===

    #[test]
    fn test_parse_package_version_knitr() {
        let stderr = "Error: knitr >= 1.44 is required for rendering with Quarto from `.R` files.";
        let info = parse_r_error(stderr);

        assert!(matches!(
            info.error_type,
            RErrorType::PackageVersionTooOld {
                ref package,
                ref required_version
            } if package == "knitr" && required_version == "1.44"
        ));
        assert!(info.suggestion.unwrap().contains("install.packages"));
    }

    #[test]
    fn test_parse_package_version_rmarkdown() {
        let stderr = "rmarkdown >= 2.9.4 is required";
        let info = parse_r_error(stderr);

        assert!(matches!(
            info.error_type,
            RErrorType::PackageVersionTooOld {
                ref package,
                ref required_version
            } if package == "rmarkdown" && required_version == "2.9.4"
        ));
    }

    // === Knitr Error Tests ===

    #[test]
    fn test_parse_knitr_error_with_file() {
        let stderr =
            "Quitting from lines 5-8 (document.Rmd): Error in eval(expr): object 'x' not found";
        let info = parse_r_error(stderr);

        assert!(matches!(
            info.error_type,
            RErrorType::KnitrExecutionError { ref source_file }
            if source_file.as_deref() == Some("document.Rmd")
        ));
        assert_eq!(info.source_lines, Some(5..9)); // Exclusive range
        assert!(info.message.contains("object 'x' not found"));
    }

    #[test]
    fn test_parse_knitr_error_without_file() {
        let stderr = "Quitting from lines 10-15 : undefined columns selected";
        let info = parse_r_error(stderr);

        assert!(matches!(
            info.error_type,
            RErrorType::KnitrExecutionError { source_file: None }
        ));
        assert_eq!(info.source_lines, Some(10..16));
    }

    #[test]
    fn test_parse_knitr_error_multiline() {
        let stderr = r#"
processing file: test.qmd
Quitting from lines 3-6 (test.rmarkdown): Error in x + y: non-numeric argument
Execution halted
"#;
        let info = parse_r_error(stderr);

        assert!(matches!(
            info.error_type,
            RErrorType::KnitrExecutionError { .. }
        ));
        assert_eq!(info.source_lines, Some(3..7));
    }

    // === R Not Found Tests ===

    #[test]
    fn test_parse_r_not_found() {
        let stderr = "Rscript: command not found";
        let info = parse_r_error(stderr);

        assert!(matches!(info.error_type, RErrorType::RNotFound));
        assert!(info.suggestion.unwrap().contains("r-project.org"));
    }

    #[test]
    fn test_parse_r_not_found_variant() {
        let stderr = "Cannot find Rscript executable";
        let info = parse_r_error(stderr);

        assert!(matches!(info.error_type, RErrorType::RNotFound));
    }

    // === Generic Error Tests ===

    #[test]
    fn test_parse_generic_error() {
        let stderr = "Error in foo(): something went wrong";
        let info = parse_r_error(stderr);

        assert!(matches!(info.error_type, RErrorType::Generic));
        assert!(info.message.contains("something went wrong"));
        // The "Error in foo():" prefix should be cleaned
        assert!(!info.message.starts_with("Error"));
    }

    #[test]
    fn test_parse_generic_error_multiline() {
        let stderr = r#"Error in someFunction():
  This is a detailed error message
  with multiple lines"#;
        let info = parse_r_error(stderr);

        assert!(matches!(info.error_type, RErrorType::Generic));
    }

    // === Error Cleaning Tests ===

    #[test]
    fn test_clean_error_message_removes_prefix() {
        let msg = "Error: something went wrong";
        let cleaned = clean_error_message(msg);
        assert_eq!(cleaned, "something went wrong");
    }

    #[test]
    fn test_clean_error_message_removes_error_in() {
        let msg = "Error in library(x) : there is no package";
        let cleaned = clean_error_message(msg);
        assert_eq!(cleaned, "there is no package");
    }

    #[test]
    fn test_clean_error_message_removes_ansi() {
        let msg = "\x1b[31mError:\x1b[0m something";
        let cleaned = clean_error_message(msg);
        assert_eq!(cleaned, "something");
    }

    #[test]
    fn test_clean_error_message_trims() {
        let msg = "  \n  Error: test  \n  ";
        let cleaned = clean_error_message(msg);
        assert_eq!(cleaned, "test");
    }

    // === Format Tests ===

    #[test]
    fn test_format_r_error_basic() {
        let info = RErrorInfo {
            error_type: RErrorType::Generic,
            message: "something went wrong".to_string(),
            suggestion: None,
            source_lines: None,
        };
        let formatted = format_r_error(&info);
        assert_eq!(formatted, "something went wrong");
    }

    #[test]
    fn test_format_r_error_with_lines() {
        let info = RErrorInfo {
            error_type: RErrorType::KnitrExecutionError { source_file: None },
            message: "object 'x' not found".to_string(),
            suggestion: None,
            source_lines: Some(5..9),
        };
        let formatted = format_r_error(&info);
        assert!(formatted.contains("object 'x' not found"));
        assert!(formatted.contains("lines 5-8"));
    }

    #[test]
    fn test_format_r_error_with_suggestion() {
        let info = RErrorInfo {
            error_type: RErrorType::MissingPackage {
                package: "knitr".to_string(),
            },
            message: "R package 'knitr' is not installed".to_string(),
            suggestion: Some("Install with install.packages(\"knitr\")".to_string()),
            source_lines: None,
        };
        let formatted = format_r_error(&info);
        assert!(formatted.contains("not installed"));
        assert!(formatted.contains("Suggestion:"));
        assert!(formatted.contains("install.packages"));
    }
}
