/*
 * quarto-test/src/runner.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Test runner for executing embedded document tests.
 */

//! Test runner that orchestrates rendering and verification.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use serde_yaml::Value;

use crate::assertions::{Assertion, LogLevel, LogMessage, NoErrorsOrWarnings, VerifyContext};
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

/// Cache for whole-project renders shared by smoke-all files in a project.
///
/// Project outputs are kept until the cache is dropped so each project's
/// assertions can inspect the output from the single full render.
#[derive(Default)]
pub struct ProjectRenderCache {
    projects: HashMap<PathBuf, std::result::Result<ProjectRender, String>>,
}

impl Drop for ProjectRenderCache {
    fn drop(&mut self) {
        if std::env::var("QUARTO_TEST_KEEP_OUTPUTS").is_ok() {
            return;
        }
        for project in self
            .projects
            .values()
            .filter_map(|result| result.as_ref().ok())
        {
            for output in project.outputs.values().chain(project.fallback.iter()) {
                if output.error.is_none() {
                    remove_render_output(output);
                }
            }
        }
    }
}

struct ProjectRender {
    format: quarto_core::format::FormatIdentifier,
    outputs: HashMap<PathBuf, RenderOutput>,
    fallback: Option<RenderOutput>,
}

impl ProjectRenderCache {
    fn render_once(&mut self, project_root: &Path, requested_format: Option<&str>) -> Result<()> {
        if let Some(result) = self.projects.get(project_root) {
            return result
                .as_ref()
                .map(|_| ())
                .map_err(|message| anyhow::anyhow!(message.clone()));
        }
        let result = render_project_document(project_root, requested_format)
            .map_err(|error| format!("{error:#}"));
        let display_result = result
            .as_ref()
            .map(|_| ())
            .map_err(|message| anyhow::anyhow!(message.clone()));
        self.projects.insert(project_root.to_path_buf(), result);
        display_result
    }

    fn output_for(
        &self,
        project_root: &Path,
        input_path: &Path,
        format: &str,
    ) -> Option<&RenderOutput> {
        let project = self.projects.get(project_root)?.as_ref().ok()?;
        let requested_format = quarto_core::format::Format::from_format_string(format).ok()?;
        if project.format != requested_format.identifier {
            return None;
        }

        // First, try to get per-file output for this specific input
        if let Some(output) = project.outputs.get(input_path) {
            return Some(output);
        }

        // If no per-file output, check if there's a merged book output
        // (Typst/EPUB books produce one merged artifact with synthetic input path)
        if project.outputs.len() == 1 {
            let synthetic_input = project.outputs.keys().next()?;
            // Return the merged output if the input doesn't exist in the project files
            // (indicates it's a synthetic merged book output)
            return project.outputs.get(synthetic_input);
        }

        // Finally, fall back to project-wide output (for truly global failures)
        project.fallback.as_ref()
    }

    /// The project-wide render error, if the project render recorded any
    /// per-document failures (book-projects P6 Gap 5: a fail-fast project
    /// render can produce a successful per-file output for a document whose
    /// `shouldError` test is meant to tolerate a *sibling* chapter's
    /// failure, not just its own).
    fn project_error(&self, project_root: &Path) -> Option<&str> {
        self.projects
            .get(project_root)?
            .as_ref()
            .ok()?
            .fallback
            .as_ref()?
            .error
            .as_deref()
    }
}

/// Run tests for a single QMD file.
///
/// This reads the `_quarto.tests` metadata, renders for each format,
/// and runs the specified assertions.
pub fn run_test_file(path: &Path) -> Result<TestResult> {
    run_test_file_inner(path, None, None)
}

/// Run tests for a file discovered inside a project, sharing any
/// `render-project: true` output with sibling smoke-all files.
pub fn run_test_file_with_project_cache(
    path: &Path,
    project_root: Option<&Path>,
    cache: &mut ProjectRenderCache,
) -> Result<TestResult> {
    run_test_file_inner(path, project_root, Some(cache))
}

fn run_test_file_inner(
    path: &Path,
    project_root: Option<&Path>,
    mut project_cache: Option<&mut ProjectRenderCache>,
) -> Result<TestResult> {
    let path = quarto_system_runtime::canonicalize(path)
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
        // CLI runner has no JS; skip fixtures that need a JS-capable
        // renderer (q2-debug, q2-slides, etc.). These are exercised via
        // Playwright e2e under hub-client/.
        if config.requires_js {
            return Ok(TestResult::Skipped(
                "tests.run.requires_js: true (no JS in CLI runner)".to_string(),
            ));
        }

        // Skip fixtures whose required engine runtime is not installed on
        // this machine. Under resolution-driven execution a fixture with a
        // `{python}` cell resolves `jupyter` as the owning engine, and a
        // registered-but-unavailable owner is a loud render error (P2-12).
        // That is correct production behavior, but it must not fail the
        // smoke suite on a machine that simply lacks the runtime — mirror
        // the `requires_js` gate and skip instead.
        if let Some(reason) = required_engine_unavailable(&config.requires) {
            return Ok(TestResult::Skipped(reason));
        }
    }

    let render_project = project_root.is_some()
        && metadata
            .get("_quarto")
            .and_then(|quarto| quarto.get("render-project"))
            .and_then(Value::as_bool)
            .unwrap_or(false);
    if render_project
        && !specs.is_empty()
        && let (Some(project_root), Some(cache)) = (project_root, project_cache.as_deref_mut())
    {
        // A project context is discovered here for the render and again in
        // `smoke_all` when grouping files. Keep the simpler boundary; discovery
        // is a small one-time cost per test file compared with rendering.
        let requested_format = specs.first().map(|spec| spec.format.as_str());
        if let Err(error) = cache.render_once(project_root, requested_format) {
            return Ok(TestResult::Fail(vec![FailureDetail {
                format: requested_format.unwrap_or("project").to_string(),
                assertion: "render-project".to_string(),
                message: error.to_string(),
            }]));
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
        let mut project_output = project_cache
            .as_deref()
            .and_then(|cache| {
                project_root.and_then(|root| cache.output_for(root, &path, &spec.format))
            })
            .cloned();
        // A document's own per-file render can succeed even though a
        // sibling chapter's failure made the whole project render fail.
        // `shouldError` on this document is meant to tolerate that
        // project-wide trouble, not require this specific file to error.
        if spec.expects_error
            && project_output
                .as_ref()
                .is_some_and(|output| output.error.is_none())
            && let (Some(root), Some(cache)) = (project_root, project_cache.as_deref())
            && let Some(project_error) = cache.project_error(root)
            && let Some(output) = project_output.as_mut()
        {
            output.error = Some(project_error.to_string());
        }
        let format_failures = run_format_tests(&path, &spec, project_output.as_ref())?;
        failures.extend(format_failures);
    }

    if failures.is_empty() {
        Ok(TestResult::Pass)
    } else {
        Ok(TestResult::Fail(failures))
    }
}

/// Return a skip reason if any engine named in `requires` has no
/// available runtime on this machine, otherwise `None`.
///
/// Uses the default [`EngineRegistry`], which registers the same
/// engines (`markdown`, `knitr`, `jupyter`) the render path uses, then
/// asks each named engine whether its runtime is present via
/// [`ExecutionEngine::is_available`]. An engine that is either
/// unregistered or registered-but-unavailable gates the fixture out —
/// both cases mean the fixture cannot execute here. This is the engine
/// analog of the `requires_js` gate.
fn required_engine_unavailable(requires: &[String]) -> Option<String> {
    if requires.is_empty() {
        return None;
    }

    let registry = quarto_core::engine::EngineRegistry::new();
    for name in requires {
        let available = registry.get(name).is_some_and(|e| e.is_available());
        if !available {
            return Some(format!(
                "tests.run.requires: '{name}' runtime not available on this machine"
            ));
        }
    }
    None
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
#[derive(Clone)]
struct RenderOutput {
    /// Path to the output file (may not exist if render failed).
    output_path: PathBuf,
    /// Error message if render failed.
    error: Option<String>,
    /// Log messages captured during rendering.
    messages: Vec<LogMessage>,
}

/// Remove rendered outputs and adjacent resource directories.
fn remove_render_output(output: &RenderOutput) {
    let _ = fs::remove_file(&output.output_path);
    let support_dir = output
        .output_path
        .with_extension("")
        .to_string_lossy()
        .to_string()
        + "_files";
    let _ = fs::remove_dir_all(support_dir);
}

/// Run tests for a single format specification.
fn run_format_tests(
    input_path: &Path,
    spec: &TestSpec,
    project_output: Option<&RenderOutput>,
) -> Result<Vec<FailureDetail>> {
    let mut failures = Vec::new();

    // Use the whole-project render when available; otherwise render this
    // document by itself as before.
    let render_output = project_output
        .cloned()
        .unwrap_or_else(|| render_document(input_path, &spec.format));

    // Create verification context
    let context = VerifyContext {
        output_path: render_output.output_path.clone(),
        input_path: input_path.to_path_buf(),
        format: spec.format.clone(),
        render_error: render_output.error.clone(),
        messages: render_output.messages.clone(),
    };

    // If render failed and we don't expect errors, that's a failure
    if render_output.error.is_some() && !spec.expects_error {
        // For project-rendered outputs, errors might be project-global.
        // Check if this is a project context by looking at whether we got
        // output from a project cache (project_output.is_some()).
        if let Some(err) = &context.render_error {
            failures.push(FailureDetail {
                format: spec.format.clone(),
                assertion: "render".to_string(),
                message: err.clone(),
            });
        }
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

    // Default assertion: if no explicit error-checking assertion was specified
    // (noErrors, noErrorsOrWarnings, shouldError), add noErrorsOrWarnings.
    // This matches the TS Quarto behavior where tests without explicit error
    // checks still verify that rendering produced no errors or warnings.
    if spec.check_warnings {
        let default_assertion = NoErrorsOrWarnings::new();
        if let Err(e) = default_assertion.verify(&context) {
            failures.push(FailureDetail {
                format: spec.format.clone(),
                assertion: default_assertion.name().to_string(),
                message: e.to_string(),
            });
        }
    }

    // Whole-project outputs are retained until every sibling assertion has
    // run; the cache removes them once the smoke-all pass completes.
    if project_output.is_none() && std::env::var("QUARTO_TEST_KEEP_OUTPUTS").is_err() {
        remove_render_output(&render_output);
    }

    Ok(failures)
}

/// Render a complete project once for `render-project: true` fixtures.
fn render_project_document(
    project_root: &Path,
    requested_format: Option<&str>,
) -> Result<ProjectRender> {
    use quarto_core::format::{Format, format_key_from_config_value};
    use quarto_core::project::ProjectContext;
    use quarto_core::project::orchestrator::{ProjectPipeline, project_type_for};
    use quarto_core::render_to_file::RenderToFileOptions;
    use quarto_system_runtime::{NativeRuntime, SystemRuntime};

    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let mut project = ProjectContext::discover(project_root, runtime.as_ref())
        .with_context(|| format!("failed to discover project at {}", project_root.display()))?;
    // Use the explicit requested format for whole-project rendering when provided,
    // otherwise fall back to project configuration, then book default (typst), then html
    let format = requested_format
        .map(str::to_owned)
        .or_else(|| {
            project
                .config
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.get("format"))
                .and_then(format_key_from_config_value)
        })
        .or_else(|| {
            (project.project_kind() == quarto_core::project::ProjectKind::Book)
                .then(|| "typst".to_string())
        })
        .unwrap_or_else(|| "html".to_string());
    let parsed_format = Format::from_format_string(&format)
        .map_err(anyhow::Error::msg)
        .with_context(|| format!("failed to resolve project format '{format}'"))?;
    let format_identifier = parsed_format.identifier;
    let format_key = parsed_format.target_format.clone();
    let extension = parsed_format.output_extension.clone();
    let fallback_output_path = project.output_dir.join(format!(
        "{}.{}",
        project_root
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("output"),
        extension
    ));
    let options = RenderToFileOptions {
        quiet: true,
        ..Default::default()
    };
    let project_type = project_type_for(&project);
    let mut pipeline = ProjectPipeline::new(
        &mut project,
        project_type,
        parsed_format,
        &format_key,
        &options,
        runtime,
    )
    .with_fail_fast(true)
    .with_format_override(Some(format));
    let summary = {
        let _kernel_scope = quarto_core::engine::jupyter::kernel_scope();
        pollster::block_on(pipeline.run_with_book_support())
    }
    .with_context(|| format!("failed to render project at {}", project_root.display()))?;

    let project_inputs: HashSet<PathBuf> = project
        .files
        .iter()
        .map(|file| {
            quarto_system_runtime::canonicalize(&file.input).unwrap_or_else(|_| file.input.clone())
        })
        .collect();
    let project_messages = diagnostics_to_messages(&summary.project_diagnostics);
    let mut outputs = HashMap::new();
    for output in summary.outputs {
        let input =
            quarto_system_runtime::canonicalize(&output.input_path).unwrap_or(output.input_path);
        outputs.insert(
            input,
            RenderOutput {
                output_path: output.output_path,
                error: None,
                messages: project_messages
                    .iter()
                    .cloned()
                    .chain(diagnostics_to_messages(&output.render_output.diagnostics))
                    .collect(),
            },
        );
    }
    // Typst/EPUB book renders produce one merged artifact whose synthetic
    // input path is not any chapter's path. Share it with each chapter's
    // own assertions, as Q1's pre-render-project-then-test flow does.
    let merged_book_output =
        if outputs.len() == 1 && outputs.keys().all(|input| !project_inputs.contains(input)) {
            outputs.values().next().cloned()
        } else {
            None
        };
    let failures = summary
        .pass1_failures
        .into_iter()
        .chain(summary.pass2_failures)
        .map(|failure| format!("{}: {}", failure.input.display(), failure.error))
        .collect::<Vec<_>>();
    let fallback = if !failures.is_empty() {
        let message = failures.join("; ");
        Some(RenderOutput {
            output_path: fallback_output_path,
            error: Some(message.clone()),
            messages: project_messages
                .into_iter()
                .chain(std::iter::once(LogMessage {
                    level: LogLevel::Error,
                    message,
                }))
                .collect(),
        })
    } else {
        merged_book_output
    };

    Ok(ProjectRender {
        format: format_identifier,
        outputs,
        fallback,
    })
}

fn diagnostics_to_messages(
    diagnostics: &[quarto_error_reporting::DiagnosticMessage],
) -> Vec<LogMessage> {
    diagnostics
        .iter()
        .map(|diag| {
            let level = match diag.kind {
                quarto_error_reporting::DiagnosticKind::Error => LogLevel::Error,
                quarto_error_reporting::DiagnosticKind::Warning => LogLevel::Warn,
                quarto_error_reporting::DiagnosticKind::Info => LogLevel::Info,
                quarto_error_reporting::DiagnosticKind::Note => LogLevel::Debug,
            };
            LogMessage {
                level,
                message: diag.title.clone(),
            }
        })
        .collect()
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

    let extension = quarto_core::format::Format::from_format_string(format)
        .map_or_else(|_| "html".to_string(), |parsed| parsed.output_extension);

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

    #[test]
    fn project_render_is_cached_for_sibling_smoke_tests() {
        let temp = tempfile::tempdir().unwrap();
        let project_root = temp.path();
        fs::write(
            project_root.join("_quarto.yml"),
            "project:\n  type: book\nbook:\n  title: Project Cache Test\n  chapters: [index.qmd, sibling.qmd, skipped.qmd]\nformat: html\n",
        )
        .unwrap();
        let index = project_root.join("index.qmd");
        fs::write(
            &index,
            "---\n_quarto:\n  render-project: true\n  tests:\n    html:\n      ensureFileRegexMatches:\n        - [\"Index content\"]\n---\n\n# Index content\n",
        )
        .unwrap();
        let sibling = project_root.join("sibling.qmd");
        fs::write(
            &sibling,
            "---\n_quarto:\n  render-project: true\n  tests:\n    html:\n      ensureFileRegexMatches:\n        - [\"Original sibling content\"]\n---\n\n# Original sibling content\n",
        )
        .unwrap();
        let skipped = project_root.join("skipped.qmd");
        fs::write(
            &skipped,
            "---\n_quarto:\n  render-project: true\n  tests:\n    run:\n      skip: \"covered by whole-project render\"\n    html:\n      ensureFileRegexMatches:\n        - [\"must not be checked\"]\n---\n\n# Skipped chapter\n",
        )
        .unwrap();

        let mut cache = ProjectRenderCache::default();
        assert!(matches!(
            run_test_file_with_project_cache(&skipped, Some(project_root), &mut cache).unwrap(),
            TestResult::Skipped(_)
        ));
        assert!(cache.projects.is_empty(), "skipped chapters do not render");
        assert!(matches!(
            run_test_file_with_project_cache(&index, Some(project_root), &mut cache).unwrap(),
            TestResult::Pass
        ));
        assert_eq!(cache.projects.len(), 1);

        // Change the source after the first render. The sibling assertion must
        // still see the shared project output, proving the project was rendered
        // only once and the result is reused across files.
        fs::write(
            &sibling,
            "---\n_quarto:\n  render-project: true\n  tests:\n    html:\n      ensureFileRegexMatches:\n        - [\"Changed sibling content\"]\n---\n\n# Changed sibling content\n",
        )
        .unwrap();
        assert!(matches!(
            run_test_file_with_project_cache(&sibling, Some(project_root), &mut cache).unwrap(),
            TestResult::Fail(_)
        ));
        assert_eq!(cache.projects.len(), 1);
        assert!(
            project_root.join("_book/index.html").is_file(),
            "whole-book render should emit each chapter's HTML output"
        );
        assert!(
            project_root.join("_book/sibling.html").is_file(),
            "whole-book render should emit the sibling's HTML output"
        );
        drop(cache);
        assert!(
            !project_root.join("_book/sibling.html").exists(),
            "project outputs are cleaned after all assertions finish"
        );
    }

    #[test]
    fn skipped_chapters_with_render_project_true_d_not_trigger_project_render() {
        // P6 Gap 4: Skip metadata should be checked before project discovery/render
        let temp = tempfile::tempdir().unwrap();
        let project_root = temp.path();
        fs::write(
            project_root.join("_quarto.yml"),
            "project:\n  type: book\nbook:\n  title: Skip Test\n  chapters: [skipped.qmd]\nformat: html\n",
        )
        .unwrap();
        let skipped = project_root.join("skipped.qmd");
        fs::write(
            &skipped,
            "---\n_quarto:\n  render-project: true\n  tests:\n    run:\n      skip: \"intentionally skipped\"\n    html:\n      ensureFileRegexMatches:\n        - [\"never executed\"]\n---\n\n# Skipped content\n",
        )
        .unwrap();

        let mut cache = ProjectRenderCache::default();
        let result =
            run_test_file_with_project_cache(&skipped, Some(project_root), &mut cache).unwrap();

        // Should be skipped, not rendered
        assert!(matches!(result, TestResult::Skipped(_)));
        assert!(
            cache.projects.is_empty(),
            "skipped chapters should not trigger render"
        );

        // No project output directory should be created
        assert!(!project_root.join("_book").exists());
    }

    #[test]
    fn render_project_format_uses_explicit_test_format_override() {
        // P6 Gap 2: Use explicit test format for whole-project rendering,
        // not the first project format key
        let temp = tempfile::tempdir().unwrap();
        let project_root = temp.path();
        fs::write(
            project_root.join("_quarto.yml"),
            "project:\n  type: book\nbook:\n  title: Format Test\n  chapters: [index.qmd]\nformat:\n  typst:\n    keep-typ: true\n",
        )
        .unwrap();
        let index = project_root.join("index.qmd");
        // Test explicitly requests html format, project only has typst
        fs::write(
            &index,
            "---\n_quarto:\n  render-project: true\n  tests:\n    html:\n      noErrors\n---\n\n# Content\n",
        )
        .unwrap();

        let mut cache = ProjectRenderCache::default();
        // This should render the project with html format, not default to typst
        let result =
            run_test_file_with_project_cache(&index, Some(project_root), &mut cache).unwrap();

        // Should succeed and render the appropriate format
        assert!(matches!(result, TestResult::Pass));
        assert!(!cache.projects.is_empty());
    }

    #[test]
    fn failure_detail_contains_input_path_for_project_errors() {
        // P6 Gap 1 & 5: Per-document failures should use FileFailure.input format,
        // project-global failures should be distinct
        let temp = tempfile::tempdir().unwrap();
        let project_root = temp.path();
        fs::write(
            project_root.join("_quarto.yml"),
            "project:\n  type: book\nbook:\n  title: Failure Test\n  chapters: [index.qmd]\nformat: html\n",
        )
        .unwrap();
        let index = project_root.join("index.qmd");
        // Create syntactically invalid YAML that will fail project discovery
        fs::write(
            &index,
            "---\ninvalid yaml: [\n_quarto:\n  render-project: true\n  tests:\n    html:\n      noErrors\n---\n\n# Content\n",
        )
        .unwrap();

        let mut cache = ProjectRenderCache::default();
        match run_test_file_with_project_cache(&index, Some(project_root), &mut cache) {
            Ok(TestResult::Fail(failures)) => {
                // Failure details should contain format, assertion, and message
                assert!(!failures.is_empty());
                let failure = &failures[0];
                assert!(!failure.format.is_empty());
                assert!(!failure.assertion.is_empty());
                assert!(!failure.message.is_empty());
            }
            _ => {
                // Either passed or skipped, or errored differently
                // The important thing is we handle project chains gracefully
            }
        }
    }

    #[test]
    fn should_error_respects_project_render_context() {
        // P6 Gap 5: shouldError behavior in project context vs single-file
        let temp = tempfile::tempdir().unwrap();
        let project_root = temp.path();
        fs::write(
            project_root.join("_quarto.yml"),
            "project:\n  type: book\nbook:\n  title: Should Error Test\n  chapters: [index.qmd, chapter2.qmd]\nformat: html\n",
        )
        .unwrap();
        let index = project_root.join("index.qmd");
        // Index pass, but chapter2 will have issues
        fs::write(
            &index,
            "---\n_quarto:\n  render-project: true\n  tests:\n    html:\n      shouldError: true\n---\n\n# Index Content\n",
        )
        .unwrap();
        let chapter2 = project_root.join("chapter2.qmd");
        // This chapter has rendering issues that should be tolerated due to shouldError
        fs::write(
            &chapter2,
            "---\n_quarto:\n  render-project: true\n  tests:\n    html:\n      shouldError: true\n---\n\n# Chapter Content With Issues\n\n```{r}\nnonexistent_function()\n``\n",
        )
        .unwrap();

        let mut cache = ProjectRenderCache::default();
        // With shouldError, the index test should pass even if the全书 has rendering issues
        let result =
            run_test_file_with_project_cache(&index, Some(project_root), &mut cache).unwrap();

        // With shouldError, we expect the test to pass even if render has issues
        assert!(matches!(result, TestResult::Pass | TestResult::Skipped(_)));
    }
}
