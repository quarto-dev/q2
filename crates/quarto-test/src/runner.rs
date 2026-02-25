/*
 * quarto-test/src/runner.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Test runner for executing embedded document tests.
 */

//! Test runner that orchestrates rendering and verification.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde_yaml::Value;

use crate::assertions::{LogLevel, LogMessage, VerifyContext};
use crate::spec::{TestSpec, parse_test_specs};

/// Result of running tests on a single file.
#[derive(Debug)]
pub enum TestResult {
    /// All tests passed.
    Pass,
    /// One or more tests failed.
    Fail(Vec<FailureDetail>),
    /// Tests were skipped (with reason).
    Skipped(String),
}

/// Details about a test failure.
#[derive(Debug)]
pub struct FailureDetail {
    /// Format that failed.
    pub format: String,
    /// Assertion that failed.
    pub assertion: String,
    /// Error message.
    pub message: String,
}

/// Summary of running tests on multiple files.
#[derive(Debug, Default)]
pub struct TestSummary {
    /// Number of files that passed all tests.
    pub passed: usize,
    /// Number of files that had failures.
    pub failed: usize,
    /// Number of files that were skipped.
    pub skipped: usize,
    /// Details of all failures.
    pub failures: Vec<(PathBuf, Vec<FailureDetail>)>,
}

impl TestSummary {
    /// Check if all tests passed.
    pub fn all_passed(&self) -> bool {
        self.failed == 0
    }
}

/// Run tests for a single QMD file.
///
/// This reads the `_quarto.tests` metadata, renders for each format,
/// and runs the specified assertions.
pub fn run_test_file(path: &Path) -> Result<TestResult> {
    let path = path
        .canonicalize()
        .with_context(|| format!("failed to resolve path: {}", path.display()))?;

    // Read and parse YAML frontmatter
    let content = fs::read_to_string(&path)
        .with_context(|| format!("failed to read file: {}", path.display()))?;

    let metadata = extract_yaml_metadata(&content)?;

    // Parse test specifications
    let (run_config, specs) = parse_test_specs(&metadata, &path)?;

    // Check if tests should be skipped
    if let Some(config) = &run_config {
        if let Some(reason) = config.should_skip() {
            return Ok(TestResult::Skipped(reason));
        }
    }

    // If no test specs, nothing to do
    if specs.is_empty() {
        return Ok(TestResult::Skipped(
            "no test specifications found".to_string(),
        ));
    }

    // Run tests for each format
    let mut failures: Vec<FailureDetail> = Vec::new();

    for spec in specs {
        let format_failures = run_format_tests(&path, &spec)?;
        failures.extend(format_failures);
    }

    if failures.is_empty() {
        Ok(TestResult::Pass)
    } else {
        Ok(TestResult::Fail(failures))
    }
}

/// Run tests for multiple files.
pub fn run_test_files(paths: &[PathBuf]) -> Result<TestSummary> {
    let mut summary = TestSummary::default();

    for path in paths {
        match run_test_file(path) {
            Ok(TestResult::Pass) => {
                summary.passed += 1;
            }
            Ok(TestResult::Fail(failures)) => {
                summary.failed += 1;
                summary.failures.push((path.clone(), failures));
            }
            Ok(TestResult::Skipped(_)) => {
                summary.skipped += 1;
            }
            Err(e) => {
                summary.failed += 1;
                summary.failures.push((
                    path.clone(),
                    vec![FailureDetail {
                        format: "N/A".to_string(),
                        assertion: "setup".to_string(),
                        message: e.to_string(),
                    }],
                ));
            }
        }
    }

    Ok(summary)
}

/// Output from rendering a document.
struct RenderOutput {
    /// Path to the output file (may not exist if render failed).
    output_path: PathBuf,
    /// Error message if render failed.
    error: Option<String>,
    /// Log messages captured during rendering.
    messages: Vec<LogMessage>,
}

/// Run tests for a single format specification.
fn run_format_tests(input_path: &Path, spec: &TestSpec) -> Result<Vec<FailureDetail>> {
    let mut failures = Vec::new();

    // Try to render the document
    let render_output = render_document(input_path, &spec.format);

    // Create verification context
    let context = VerifyContext {
        output_path: render_output.output_path.clone(),
        input_path: input_path.to_path_buf(),
        format: spec.format.clone(),
        render_error: render_output.error.clone(),
        messages: render_output.messages,
    };

    // If render failed and we don't expect errors, that's a failure
    if render_output.error.is_some() && !spec.expects_error {
        failures.push(FailureDetail {
            format: spec.format.clone(),
            assertion: "render".to_string(),
            message: context.render_error.clone().unwrap(),
        });
        return Ok(failures);
    }

    // Run each assertion
    for assertion in &spec.assertions {
        if let Err(e) = assertion.verify(&context) {
            failures.push(FailureDetail {
                format: spec.format.clone(),
                assertion: assertion.name().to_string(),
                message: e.to_string(),
            });
        }
    }

    // Clean up output file (unless keeping outputs)
    if std::env::var("QUARTO_TEST_KEEP_OUTPUTS").is_err() {
        let _ = fs::remove_file(&render_output.output_path);
        // Also try to remove supporting files directory
        let support_dir = render_output
            .output_path
            .with_extension("")
            .to_string_lossy()
            .to_string()
            + "_files";
        let _ = fs::remove_dir_all(&support_dir);
    }

    Ok(failures)
}

/// Render a document to the specified format.
///
/// Uses `quarto_core::render_to_file` which runs the full staged pipeline:
/// - ParseDocumentStage: QMD → Pandoc AST
/// - EngineExecutionStage: Execute code cells (skipped when execute=false)
/// - AstTransformsStage: Project metadata merging + all transforms
/// - RenderHtmlBodyStage: AST → HTML body
/// - ApplyTemplateStage: Apply HTML template
///
/// This also writes resources (CSS) to the `_files` directory.
///
/// Returns a RenderOutput with the output path, any error, and captured messages.
fn render_document(input_path: &Path, format: &str) -> RenderOutput {
    use std::sync::Arc;

    use quarto_core::render_to_file::{RenderToFileOptions, render_to_file};
    use quarto_system_runtime::NativeRuntime;

    let mut messages = Vec::new();

    // Determine expected output path (for error reporting if render fails early)
    let output_dir = input_path.parent().unwrap_or(Path::new("."));
    let stem = input_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("output");

    let extension = match format {
        "html" => "html",
        "pdf" => "pdf",
        "docx" => "docx",
        "latex" | "tex" => "tex",
        "typst" => "typ",
        _ => "html",
    };

    let fallback_output_path = output_dir.join(format!("{}.{}", stem, extension));

    // Create runtime
    let runtime = Arc::new(NativeRuntime::new());

    // Render using the shared render_to_file function
    let options = RenderToFileOptions {
        quiet: true, // Don't log to console during tests
        ..Default::default()
    };

    match render_to_file(input_path, format, &options, runtime) {
        Ok(result) => {
            // Capture diagnostics as log messages
            for diag in &result.render_output.diagnostics {
                let level = match diag.kind {
                    quarto_error_reporting::DiagnosticKind::Error => LogLevel::Error,
                    quarto_error_reporting::DiagnosticKind::Warning => LogLevel::Warn,
                    quarto_error_reporting::DiagnosticKind::Info => LogLevel::Info,
                    quarto_error_reporting::DiagnosticKind::Note => LogLevel::Debug,
                };
                messages.push(LogMessage {
                    level,
                    message: diag.title.clone(),
                });
            }

            RenderOutput {
                output_path: result.output_path,
                error: None,
                messages,
            }
        }
        Err(e) => {
            let error_msg = e.to_string();
            messages.push(LogMessage {
                level: LogLevel::Error,
                message: error_msg.clone(),
            });
            RenderOutput {
                output_path: fallback_output_path,
                error: Some(error_msg),
                messages,
            }
        }
    }
}

/// Extract YAML metadata from QMD content.
fn extract_yaml_metadata(content: &str) -> Result<Value> {
    // Look for YAML frontmatter delimited by ---
    let content = content.trim_start();

    if !content.starts_with("---") {
        return Ok(Value::Mapping(Default::default()));
    }

    // Find the closing ---
    let rest = &content[3..];
    let end = rest
        .find("\n---")
        .or_else(|| rest.find("\r\n---"))
        .context("unterminated YAML frontmatter")?;

    let yaml_str = &rest[..end];

    serde_yaml::from_str(yaml_str).context("failed to parse YAML frontmatter")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_yaml_metadata() {
        let content = r#"---
title: Test
format: html
_quarto:
  tests:
    html:
      ensureFileRegexMatches:
        - ["pattern"]
---

Content here.
"#;

        let metadata = extract_yaml_metadata(content).unwrap();
        assert_eq!(metadata["title"].as_str(), Some("Test"));
        assert!(metadata["_quarto"]["tests"]["html"].is_mapping());
    }

    #[test]
    fn test_extract_yaml_metadata_no_frontmatter() {
        let content = "Just some content without frontmatter.";
        let metadata = extract_yaml_metadata(content).unwrap();
        assert!(metadata.as_mapping().unwrap().is_empty());
    }
}
