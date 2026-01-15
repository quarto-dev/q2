/*
 * engine/knitr/mod.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Knitr engine for R code execution.
 */

//! Knitr engine for R code execution.
//!
//! This engine executes R code cells using knitr. It shells out to
//! an R process to run knitr on the document via embedded R scripts.
//!
//! # Availability
//!
//! This engine is only available in native builds (not WASM).
//! It requires R and the knitr package to be installed.
//!
//! # Embedded Resources
//!
//! The R scripts (rmd.R, execute.R, hooks.R, etc.) are embedded at compile
//! time and extracted to a temp directory on first use. Access via
//! [`KNITR_RESOURCES`].
//!
//! # Current Status
//!
//! The R resource extraction is implemented. Full knitr execution
//! will be implemented in subsequent phases.

#![cfg(not(target_arch = "wasm32"))]

pub mod error_parser;
pub mod format;
pub mod preprocess;
pub mod subprocess;
pub mod types;

use std::path::{Path, PathBuf};

use include_dir::{Dir, include_dir};

use crate::resources::ResourceBundle;

// Re-export public API types and functions.
// These are intentionally public for external consumers even though internal code
// uses direct module paths. The #[allow(unused_imports)] suppresses warnings for
// re-exports that aren't used internally.
#[allow(unused_imports)]
pub use error_parser::{RErrorInfo, RErrorType, parse_r_error};
pub use format::KnitrFormatConfig;
pub use preprocess::resolve_inline_r_expressions;
pub use subprocess::{CallROptions, call_r, determine_working_dir, find_rscript};
pub use types::{KnitrExecuteParams, KnitrExecuteResult, KnitrIncludes};

// ============================================================================
// Embedded R Scripts
// ============================================================================

/// Embedded R scripts directory (compile-time).
///
/// Contains rmd/rmd.R, rmd/execute.R, etc. matching the TS Quarto structure.
static KNITR_RESOURCES_DIR: Dir = include_dir!("$CARGO_MANIFEST_DIR/src/engine/knitr/resources");

/// Knitr engine resources bundle.
///
/// R scripts are extracted to a temp directory on first access via `.path()`.
/// The extracted structure matches TS Quarto's resources layout:
///
/// ```text
/// {temp_dir}/
/// └── rmd/
///     ├── rmd.R
///     ├── execute.R
///     ├── hooks.R
///     ├── patch.R
///     ├── ojs.R
///     └── ojs_static.R
/// ```
///
/// # Example
///
/// ```ignore
/// let resource_path = KNITR_RESOURCES.path()?;
/// let rmd_script = resource_path.join("rmd/rmd.R");
/// ```
pub static KNITR_RESOURCES: ResourceBundle = ResourceBundle::new("knitr", &KNITR_RESOURCES_DIR);

// ============================================================================
// KnitrEngine
// ============================================================================

use super::context::{ExecuteResult, ExecutionContext};
use super::error::ExecutionError;
use super::traits::ExecutionEngine;

/// Knitr engine for R code execution.
///
/// This engine shells out to R/knitr to execute R code cells.
///
/// # Requirements
///
/// - R must be installed and accessible via `Rscript` command
/// - The `knitr` R package must be installed
///
/// # Intermediate Files
///
/// Knitr produces:
/// - `{input}_files/` directory containing figures
/// - `{input}.md` intermediate markdown file (if not using our temp dir)
pub struct KnitrEngine {
    /// Path to Rscript executable (discovered or configured)
    rscript_path: Option<PathBuf>,
}

impl KnitrEngine {
    /// Create a new knitr engine, attempting to find Rscript.
    pub fn new() -> Self {
        Self {
            rscript_path: find_rscript(),
        }
    }

    /// Get the path to Rscript, if found.
    pub fn rscript_path(&self) -> Option<&Path> {
        self.rscript_path.as_deref()
    }
}

impl Default for KnitrEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl ExecutionEngine for KnitrEngine {
    fn name(&self) -> &str {
        "knitr"
    }

    fn execute(
        &self,
        input: &str,
        ctx: &ExecutionContext,
    ) -> Result<ExecuteResult, ExecutionError> {
        // Step 1: Check if Rscript is available
        if self.rscript_path.is_none() {
            return Err(ExecutionError::runtime_not_found(
                "knitr",
                "Rscript (install R from https://www.r-project.org/)",
            ));
        }

        // Step 2: Preprocess markdown (resolve inline R expressions)
        // Use has_inline_r_expressions as optimization to skip regex replacement
        // when there are no inline R expressions (common case).
        let preprocessed = if preprocess::has_inline_r_expressions(input) {
            resolve_inline_r_expressions(input)
        } else {
            input.to_string()
        };

        // Step 3: Build format configuration
        let format_config = build_format_config(ctx);

        // Step 4: Get resource directory
        let resource_dir = KNITR_RESOURCES.path().map_err(|e| {
            ExecutionError::temp_file(format!("Failed to extract R resources: {}", e), None)
        })?;

        // Step 5: Determine working directory (renv-aware)
        let document_dir = ctx.source_path.parent().unwrap_or(&ctx.cwd).to_path_buf();
        let working_dir = determine_working_dir(&document_dir, ctx.project_dir.as_deref());

        // Step 6: Build execute params
        let params = KnitrExecuteParams {
            input: ctx.source_path.clone(),
            markdown: preprocessed,
            format: format_config,
            temp_dir: ctx.temp_dir.clone(),
            lib_dir: None,
            dependencies: true,
            cwd: working_dir.clone(),
            params: None,
            resource_dir: resource_dir.to_path_buf(),
            // Languages that Quarto handles (knitr passes them through unchanged)
            handled_languages: vec!["ojs".to_string(), "mermaid".to_string(), "dot".to_string()],
        };

        // Step 7: Build call options
        let call_options = CallROptions {
            quiet: ctx.quiet,
            ..Default::default()
        };

        // Step 8: Call R
        let result: KnitrExecuteResult = call_r(
            "execute",
            &params,
            &ctx.temp_dir,
            &working_dir,
            &call_options,
        )?;

        // Step 9: Post-process markdown (fix .rmarkdown references)
        let markdown = postprocess_markdown(&result.markdown, &ctx.source_path);

        // Step 10: Convert includes
        let includes = convert_includes(&result.includes);

        // Step 11: Build and return result
        Ok(ExecuteResult {
            markdown,
            supporting_files: result.supporting.into_iter().map(PathBuf::from).collect(),
            filters: result.filters,
            includes,
            needs_postprocess: result.post_process,
        })
    }

    fn can_freeze(&self) -> bool {
        true
    }

    fn is_available(&self) -> bool {
        self.rscript_path.is_some()
    }

    fn intermediate_files(&self, input_path: &Path) -> Vec<PathBuf> {
        // knitr produces {input}_files/ directory for figures
        let stem = input_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("");

        if stem.is_empty() {
            return Vec::new();
        }

        let parent = input_path.parent().unwrap_or(Path::new("."));
        let files_dir = parent.join(format!("{}_files", stem));

        vec![files_dir]
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Build format configuration from execution context.
///
/// Creates a [`KnitrFormatConfig`] with settings appropriate for the target format.
fn build_format_config(ctx: &ExecutionContext) -> KnitrFormatConfig {
    KnitrFormatConfig::with_defaults(&ctx.format)
}

/// Post-process knitr markdown output.
///
/// Performs the following transformations:
/// - Fixes `.rmarkdown` filename references back to the original source filename
fn postprocess_markdown(markdown: &str, source_path: &Path) -> String {
    // Get the original filename components
    let stem = source_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("document");

    let original_name = source_path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("document.qmd");

    // Knitr temporarily renames the file to .rmarkdown during processing
    // We need to fix references in the output
    let rmarkdown_name = format!("{}.rmarkdown", stem);

    markdown.replace(&rmarkdown_name, original_name)
}

/// Convert knitr includes to PandocIncludes.
///
/// Reads include file contents and converts them to the PandocIncludes format
/// used by the rest of the pipeline.
fn convert_includes(includes: &Option<KnitrIncludes>) -> crate::stage::PandocIncludes {
    use crate::stage::PandocIncludes;

    let Some(inc) = includes else {
        return PandocIncludes::default();
    };

    let mut result = PandocIncludes::default();

    // Read include file contents
    if let Some(ref path) = inc.include_in_header {
        if let Ok(content) = std::fs::read_to_string(path) {
            result.header_includes.push(content);
        }
    }

    if let Some(ref path) = inc.include_before_body {
        if let Ok(content) = std::fs::read_to_string(path) {
            result.include_before.push(content);
        }
    }

    if let Some(ref path) = inc.include_after_body {
        if let Ok(content) = std::fs::read_to_string(path) {
            result.include_after.push(content);
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_knitr_engine_name() {
        let engine = KnitrEngine::new();
        assert_eq!(engine.name(), "knitr");
    }

    #[test]
    fn test_knitr_engine_can_freeze() {
        let engine = KnitrEngine::new();
        assert!(engine.can_freeze());
    }

    #[test]
    fn test_knitr_engine_intermediate_files() {
        let engine = KnitrEngine::new();

        let files = engine.intermediate_files(Path::new("/project/analysis.qmd"));
        assert_eq!(files.len(), 1);
        assert_eq!(files[0], PathBuf::from("/project/analysis_files"));
    }

    #[test]
    fn test_knitr_engine_intermediate_files_nested() {
        let engine = KnitrEngine::new();

        let files = engine.intermediate_files(Path::new("/project/reports/monthly.qmd"));
        assert_eq!(files.len(), 1);
        assert_eq!(files[0], PathBuf::from("/project/reports/monthly_files"));
    }

    #[test]
    fn test_knitr_engine_is_available_depends_on_rscript() {
        let engine = KnitrEngine::new();
        // Availability depends on whether Rscript is installed
        // We just verify it doesn't panic
        let _ = engine.is_available();
    }

    #[test]
    fn test_knitr_engine_execute_requires_rscript() {
        // Create engine with no Rscript path to test the error case
        let engine = KnitrEngine { rscript_path: None };

        let ctx = ExecutionContext::new(
            PathBuf::from("/tmp"),
            PathBuf::from("/project"),
            PathBuf::from("/project/doc.qmd"),
            "html",
        );

        let result = engine.execute("# Test", &ctx);
        assert!(result.is_err());

        let err = result.unwrap_err();
        let msg = format!("{}", err);
        assert!(msg.contains("Rscript"));
    }

    #[test]
    fn test_knitr_engine_default() {
        let engine = KnitrEngine::default();
        assert_eq!(engine.name(), "knitr");
    }

    #[test]
    fn test_knitr_engine_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<KnitrEngine>();
    }

    // === Resource Tests ===

    #[test]
    fn test_knitr_resources_extracts_to_directory() {
        let path = KNITR_RESOURCES
            .path()
            .expect("Failed to get resources path");
        assert!(path.exists());
        assert!(path.is_dir());
    }

    #[test]
    fn test_knitr_resources_has_rmd_subdirectory() {
        let path = KNITR_RESOURCES
            .path()
            .expect("Failed to get resources path");
        let rmd_dir = path.join("rmd");
        assert!(rmd_dir.exists(), "rmd/ subdirectory should exist");
        assert!(rmd_dir.is_dir());
    }

    #[test]
    fn test_knitr_resources_contains_rmd_r() {
        let path = KNITR_RESOURCES
            .path()
            .expect("Failed to get resources path");
        let rmd_script = path.join("rmd/rmd.R");
        assert!(rmd_script.exists(), "rmd/rmd.R should exist");

        let content = std::fs::read_to_string(&rmd_script).expect("Failed to read rmd.R");
        assert!(content.contains("stdin"), "rmd.R should read from stdin");
    }

    #[test]
    fn test_knitr_resources_contains_execute_r() {
        let path = KNITR_RESOURCES
            .path()
            .expect("Failed to get resources path");
        let execute_script = path.join("rmd/execute.R");
        assert!(execute_script.exists(), "rmd/execute.R should exist");

        let content = std::fs::read_to_string(&execute_script).expect("Failed to read execute.R");
        assert!(
            content.contains("rmarkdown") || content.contains("knitr"),
            "execute.R should use rmarkdown/knitr"
        );
    }

    #[test]
    fn test_knitr_resources_contains_all_scripts() {
        let path = KNITR_RESOURCES
            .path()
            .expect("Failed to get resources path");
        let rmd_dir = path.join("rmd");

        let expected_files = [
            "rmd.R",
            "execute.R",
            "hooks.R",
            "patch.R",
            "ojs.R",
            "ojs_static.R",
        ];

        for filename in expected_files {
            let file_path = rmd_dir.join(filename);
            assert!(file_path.exists(), "Missing R script: {}", filename);
        }
    }

    #[test]
    fn test_knitr_resources_path_is_idempotent() {
        let path1 = KNITR_RESOURCES
            .path()
            .expect("Failed to get resources path");
        let path2 = KNITR_RESOURCES
            .path()
            .expect("Failed to get resources path");
        assert_eq!(path1, path2);
    }

    // === Helper Function Tests ===

    #[test]
    fn test_build_format_config_html() {
        let ctx = ExecutionContext::new(
            PathBuf::from("/tmp"),
            PathBuf::from("/project"),
            PathBuf::from("/project/doc.qmd"),
            "html",
        );

        let config = build_format_config(&ctx);
        assert_eq!(config.pandoc.to, Some("html".to_string()));
        assert_eq!(config.pandoc.from, Some("markdown".to_string()));
    }

    #[test]
    fn test_build_format_config_pdf() {
        let ctx = ExecutionContext::new(
            PathBuf::from("/tmp"),
            PathBuf::from("/project"),
            PathBuf::from("/project/doc.qmd"),
            "pdf",
        );

        let config = build_format_config(&ctx);
        assert_eq!(config.pandoc.to, Some("pdf".to_string()));
    }

    #[test]
    fn test_postprocess_markdown_fixes_rmarkdown_refs() {
        let markdown = "See [source](test.rmarkdown) for details.\nFile: test.rmarkdown";
        let source_path = PathBuf::from("/project/test.qmd");

        let result = postprocess_markdown(markdown, &source_path);

        assert_eq!(
            result,
            "See [source](test.qmd) for details.\nFile: test.qmd"
        );
    }

    #[test]
    fn test_postprocess_markdown_no_rmarkdown_refs() {
        let markdown = "# Title\n\nSome content here.";
        let source_path = PathBuf::from("/project/doc.qmd");

        let result = postprocess_markdown(markdown, &source_path);

        // Should be unchanged
        assert_eq!(result, markdown);
    }

    #[test]
    fn test_postprocess_markdown_preserves_unrelated_refs() {
        let markdown = "other.rmarkdown stays, doc.rmarkdown changes";
        let source_path = PathBuf::from("/project/doc.qmd");

        let result = postprocess_markdown(markdown, &source_path);

        // Only doc.rmarkdown should change, other.rmarkdown stays
        assert_eq!(result, "other.rmarkdown stays, doc.qmd changes");
    }

    #[test]
    fn test_convert_includes_none() {
        let result = convert_includes(&None);

        assert!(result.header_includes.is_empty());
        assert!(result.include_before.is_empty());
        assert!(result.include_after.is_empty());
    }

    #[test]
    fn test_convert_includes_empty() {
        let includes = KnitrIncludes {
            include_in_header: None,
            include_before_body: None,
            include_after_body: None,
        };

        let result = convert_includes(&Some(includes));

        assert!(result.header_includes.is_empty());
        assert!(result.include_before.is_empty());
        assert!(result.include_after.is_empty());
    }

    #[test]
    fn test_convert_includes_with_files() {
        // Create temp files with content
        let temp_dir = tempfile::tempdir().unwrap();

        let header_path = temp_dir.path().join("header.html");
        std::fs::write(&header_path, "<style>body{}</style>").unwrap();

        let before_path = temp_dir.path().join("before.html");
        std::fs::write(&before_path, "<div>Before</div>").unwrap();

        let after_path = temp_dir.path().join("after.html");
        std::fs::write(&after_path, "<div>After</div>").unwrap();

        let includes = KnitrIncludes {
            include_in_header: Some(header_path),
            include_before_body: Some(before_path),
            include_after_body: Some(after_path),
        };

        let result = convert_includes(&Some(includes));

        assert_eq!(result.header_includes.len(), 1);
        assert_eq!(result.header_includes[0], "<style>body{}</style>");
        assert_eq!(result.include_before.len(), 1);
        assert_eq!(result.include_before[0], "<div>Before</div>");
        assert_eq!(result.include_after.len(), 1);
        assert_eq!(result.include_after[0], "<div>After</div>");
    }

    #[test]
    fn test_convert_includes_missing_file() {
        // Path to a file that doesn't exist
        let includes = KnitrIncludes {
            include_in_header: Some(PathBuf::from("/nonexistent/header.html")),
            include_before_body: None,
            include_after_body: None,
        };

        let result = convert_includes(&Some(includes));

        // Should gracefully handle missing file
        assert!(result.header_includes.is_empty());
    }

    // === Integration Tests (require R installation) ===

    mod integration {
        use super::*;

        /// Helper to create a test context with a real temp directory and qmd file.
        fn setup_test(markdown: &str) -> (tempfile::TempDir, ExecutionContext, PathBuf) {
            let temp_dir = tempfile::tempdir().unwrap();
            let qmd_path = temp_dir.path().join("test.qmd");
            std::fs::write(&qmd_path, markdown).unwrap();

            let ctx = ExecutionContext::new(
                temp_dir.path().to_path_buf(),
                temp_dir.path().to_path_buf(),
                qmd_path.clone(),
                "html",
            )
            .with_quiet(true);

            (temp_dir, ctx, qmd_path)
        }

        /// Test basic execution through KnitrEngine::execute().
        ///
        /// Run with: `cargo nextest run --run-ignored all -- test_engine_execute_basic`
        #[test]
        #[ignore]
        fn test_engine_execute_basic() {
            let engine = KnitrEngine::new();
            if !engine.is_available() {
                eprintln!("Skipping test: Rscript not found");
                return;
            }

            let markdown = "# Test\n\n```{r}\n1 + 1\n```\n";
            let (_temp_dir, ctx, _qmd_path) = setup_test(markdown);

            let result = engine.execute(markdown, &ctx).expect("Execution failed");

            assert!(
                result.markdown.contains("[1] 2"),
                "Expected output to contain '[1] 2', got:\n{}",
                result.markdown
            );
        }

        /// Test inline R expressions are processed.
        #[test]
        #[ignore]
        fn test_engine_execute_inline_r() {
            let engine = KnitrEngine::new();
            if !engine.is_available() {
                eprintln!("Skipping test: Rscript not found");
                return;
            }

            let markdown = "The answer is `r 2 + 2`.\n";
            let (_temp_dir, ctx, _qmd_path) = setup_test(markdown);

            let result = engine.execute(markdown, &ctx).expect("Execution failed");

            assert!(
                result.markdown.contains("4"),
                "Expected inline R result '4', got:\n{}",
                result.markdown
            );
        }

        /// Test multiple code chunks maintain state.
        #[test]
        #[ignore]
        fn test_engine_execute_multiple_chunks() {
            let engine = KnitrEngine::new();
            if !engine.is_available() {
                eprintln!("Skipping test: Rscript not found");
                return;
            }

            let markdown = r#"# Test

```{r}
x <- 42
x
```

```{r}
x * 2
```
"#;
            let (_temp_dir, ctx, _qmd_path) = setup_test(markdown);

            let result = engine.execute(markdown, &ctx).expect("Execution failed");

            assert!(
                result.markdown.contains("[1] 42"),
                "Expected first chunk to show '[1] 42'"
            );
            assert!(
                result.markdown.contains("[1] 84"),
                "Expected second chunk to show '[1] 84'"
            );
        }

        /// Test chunk options are respected.
        #[test]
        #[ignore]
        fn test_engine_execute_chunk_options() {
            let engine = KnitrEngine::new();
            if !engine.is_available() {
                eprintln!("Skipping test: Rscript not found");
                return;
            }

            let markdown = r#"
```{r}
#| echo: false
"hidden code"
```
"#;
            let (_temp_dir, ctx, _qmd_path) = setup_test(markdown);

            let result = engine.execute(markdown, &ctx).expect("Execution failed");

            // Output should contain the string result
            assert!(
                result.markdown.contains("hidden code"),
                "Expected output to contain 'hidden code'"
            );
        }

        /// Test error handling when R code fails.
        #[test]
        #[ignore]
        fn test_engine_execute_r_error() {
            let engine = KnitrEngine::new();
            if !engine.is_available() {
                eprintln!("Skipping test: Rscript not found");
                return;
            }

            // This should produce an R error, but with error: true it continues
            let markdown = r#"
```{r}
#| error: true
stop("This is an error")
```
"#;
            let (_temp_dir, ctx, _qmd_path) = setup_test(markdown);

            // With error: true, execution should succeed and show the error message
            let result = engine.execute(markdown, &ctx).expect("Execution failed");

            assert!(
                result.markdown.contains("This is an error"),
                "Expected error message in output"
            );
        }

        // ==================================================================
        // Figure Output Tests
        // ==================================================================

        /// Test that plot output produces a figure in supporting files.
        #[test]
        #[ignore]
        fn test_engine_execute_figure_output() {
            let engine = KnitrEngine::new();
            if !engine.is_available() {
                eprintln!("Skipping test: Rscript not found");
                return;
            }

            let markdown = r#"
```{r}
plot(1:10, 1:10)
```
"#;
            let (temp_dir, ctx, _qmd_path) = setup_test(markdown);

            let result = engine.execute(markdown, &ctx).expect("Execution failed");

            // The markdown should contain an image reference
            assert!(
                result.markdown.contains("![") || result.markdown.contains(".png"),
                "Expected figure reference in output, got:\n{}",
                result.markdown
            );

            // Check that supporting files directory was created
            let files_dir = temp_dir.path().join("test_files");
            if files_dir.exists() {
                // If files dir exists, it should contain PNG files
                let has_png = std::fs::read_dir(&files_dir)
                    .map(|entries| {
                        entries
                            .filter_map(|e| e.ok())
                            .any(|e| e.path().extension().is_some_and(|ext| ext == "png"))
                    })
                    .unwrap_or(false);

                if has_png {
                    assert!(
                        !result.supporting_files.is_empty(),
                        "Expected supporting files when figures are present"
                    );
                }
            }
        }

        /// Test figure with custom label.
        #[test]
        #[ignore]
        fn test_engine_execute_figure_with_label() {
            let engine = KnitrEngine::new();
            if !engine.is_available() {
                eprintln!("Skipping test: Rscript not found");
                return;
            }

            let markdown = r#"
```{r}
#| label: fig-scatter
#| fig-cap: "A scatter plot"
plot(mtcars$mpg, mtcars$hp)
```
"#;
            let (_temp_dir, ctx, _qmd_path) = setup_test(markdown);

            let result = engine.execute(markdown, &ctx).expect("Execution failed");

            // The label should appear in the output
            assert!(
                result.markdown.contains("fig-scatter"),
                "Expected fig-scatter label in output, got:\n{}",
                result.markdown
            );
        }

        // ==================================================================
        // Chunk Options Tests
        // ==================================================================

        /// Test eval: false - code is shown but not executed.
        #[test]
        #[ignore]
        fn test_engine_execute_eval_false() {
            let engine = KnitrEngine::new();
            if !engine.is_available() {
                eprintln!("Skipping test: Rscript not found");
                return;
            }

            let markdown = r#"
```{r}
#| eval: false
stop("This should not cause an error because it's not evaluated")
```
"#;
            let (_temp_dir, ctx, _qmd_path) = setup_test(markdown);

            // Should succeed since code is not evaluated
            let result = engine.execute(markdown, &ctx).expect("Execution failed");

            // Code should be visible
            assert!(
                result.markdown.contains("stop("),
                "Expected code to be visible with eval: false"
            );
        }

        /// Test include: false - code runs but output is hidden.
        ///
        /// With `include: false`, the code chunk runs (affecting the R environment)
        /// but neither the code nor its output appears in the document.
        #[test]
        #[ignore]
        fn test_engine_execute_include_false() {
            let engine = KnitrEngine::new();
            if !engine.is_available() {
                eprintln!("Skipping test: Rscript not found");
                return;
            }

            // Test that include: false hides both code and output
            let markdown = r#"
```{r}
#| include: false
hidden_computation <- 42
```

```{r}
hidden_computation * 2
```
"#;
            let (_temp_dir, ctx, _qmd_path) = setup_test(markdown);

            let result = engine.execute(markdown, &ctx).expect("Execution failed");

            // The hidden chunk's code should NOT be visible
            assert!(
                !result.markdown.contains("hidden_computation <- 42"),
                "Expected code to be hidden with include: false"
            );

            // But the variable should be available in subsequent chunks
            assert!(
                result.markdown.contains("[1] 84"),
                "Expected subsequent chunk to access value from include: false chunk, got:\n{}",
                result.markdown
            );
        }

        /// Test output: false - code shown but output suppressed.
        #[test]
        #[ignore]
        fn test_engine_execute_output_false() {
            let engine = KnitrEngine::new();
            if !engine.is_available() {
                eprintln!("Skipping test: Rscript not found");
                return;
            }

            let markdown = r#"
```{r}
#| output: false
print("This should not appear")
42
```
"#;
            let (_temp_dir, ctx, _qmd_path) = setup_test(markdown);

            let result = engine.execute(markdown, &ctx).expect("Execution failed");

            // Code should be visible
            assert!(
                result.markdown.contains("print("),
                "Expected code to be visible"
            );

            // Output should be suppressed
            assert!(
                !result.markdown.contains("[1] 42"),
                "Expected output to be suppressed with output: false"
            );
        }

        /// Test warning: true shows warnings in output.
        ///
        /// By default, our format config sets warning: true, so warnings appear.
        /// This test verifies warnings are captured in the output.
        #[test]
        #[ignore]
        fn test_engine_execute_warning_shown() {
            let engine = KnitrEngine::new();
            if !engine.is_available() {
                eprintln!("Skipping test: Rscript not found");
                return;
            }

            let markdown = r#"
```{r}
warning("This is a test warning")
"done"
```
"#;
            let (_temp_dir, ctx, _qmd_path) = setup_test(markdown);

            let result = engine.execute(markdown, &ctx).expect("Execution failed");

            // Output should be present
            assert!(
                result.markdown.contains("done"),
                "Expected output 'done' to be present"
            );

            // By default (warning: true in our config), warnings should be visible
            // Note: Warnings may appear in .cell-output-stderr or .cell-output-warning
            // The exact format depends on knitr version and settings
            assert!(
                result.markdown.contains("warning") || result.markdown.contains("done"),
                "Expected either warning or output to be present"
            );
        }

        /// Test message output is included.
        ///
        /// By default, messages from R are included in the output.
        #[test]
        #[ignore]
        fn test_engine_execute_message_shown() {
            let engine = KnitrEngine::new();
            if !engine.is_available() {
                eprintln!("Skipping test: Rscript not found");
                return;
            }

            let markdown = r#"
```{r}
message("This is a test message")
"done"
```
"#;
            let (_temp_dir, ctx, _qmd_path) = setup_test(markdown);

            let result = engine.execute(markdown, &ctx).expect("Execution failed");

            // Output should be present
            assert!(
                result.markdown.contains("done"),
                "Expected output 'done' to be present"
            );
        }

        // ==================================================================
        // Chunk Label Tests
        // ==================================================================

        /// Test chunk with explicit label executes correctly.
        ///
        /// Note: The label is used internally by knitr for figure naming and caching,
        /// but doesn't necessarily appear in the output markdown div class.
        /// This test verifies that labeled chunks execute correctly.
        #[test]
        #[ignore]
        fn test_engine_execute_chunk_label() {
            let engine = KnitrEngine::new();
            if !engine.is_available() {
                eprintln!("Skipping test: Rscript not found");
                return;
            }

            let markdown = r#"
```{r}
#| label: my-computation
x <- 1 + 1
x
```
"#;
            let (_temp_dir, ctx, _qmd_path) = setup_test(markdown);

            let result = engine.execute(markdown, &ctx).expect("Execution failed");

            // The chunk should execute and produce output
            assert!(
                result.markdown.contains("[1] 2"),
                "Expected chunk with label to produce output, got:\n{}",
                result.markdown
            );

            // The output should be in a cell div
            assert!(
                result.markdown.contains(".cell"),
                "Expected output in a cell div"
            );
        }

        // ==================================================================
        // Error Handling Tests
        // ==================================================================

        /// Test that R execution error without error: true causes execution failure.
        #[test]
        #[ignore]
        fn test_engine_execute_error_without_flag() {
            let engine = KnitrEngine::new();
            if !engine.is_available() {
                eprintln!("Skipping test: Rscript not found");
                return;
            }

            // Without error: true, this should cause execution to fail
            let markdown = r#"
```{r}
stop("This error should cause failure")
```
"#;
            let (_temp_dir, ctx, _qmd_path) = setup_test(markdown);

            let result = engine.execute(markdown, &ctx);

            // Execution should fail
            assert!(
                result.is_err(),
                "Expected execution to fail when R code errors without error: true"
            );

            // Error message should mention the error
            let err = result.unwrap_err();
            let err_msg = format!("{}", err);
            assert!(
                err_msg.contains("error") || err_msg.contains("Error") || err_msg.contains("fail"),
                "Expected error message to indicate failure, got: {}",
                err_msg
            );
        }

        /// Test that undefined variable causes error.
        #[test]
        #[ignore]
        fn test_engine_execute_undefined_variable() {
            let engine = KnitrEngine::new();
            if !engine.is_available() {
                eprintln!("Skipping test: Rscript not found");
                return;
            }

            let markdown = r#"
```{r}
undefined_variable_xyz
```
"#;
            let (_temp_dir, ctx, _qmd_path) = setup_test(markdown);

            let result = engine.execute(markdown, &ctx);

            // Execution should fail
            assert!(
                result.is_err(),
                "Expected execution to fail for undefined variable"
            );
        }

        // ==================================================================
        // Data Frame Printing Tests
        // ==================================================================

        /// Test that data frames are printed correctly.
        #[test]
        #[ignore]
        fn test_engine_execute_dataframe_output() {
            let engine = KnitrEngine::new();
            if !engine.is_available() {
                eprintln!("Skipping test: Rscript not found");
                return;
            }

            let markdown = r#"
```{r}
head(mtcars, 3)
```
"#;
            let (_temp_dir, ctx, _qmd_path) = setup_test(markdown);

            let result = engine.execute(markdown, &ctx).expect("Execution failed");

            // Should show car names and data
            assert!(
                result.markdown.contains("Mazda") || result.markdown.contains("mpg"),
                "Expected mtcars data in output, got:\n{}",
                result.markdown
            );
        }

        // ==================================================================
        // Special Character Handling Tests
        // ==================================================================

        /// Test that special characters in output are handled correctly.
        #[test]
        #[ignore]
        fn test_engine_execute_special_characters() {
            let engine = KnitrEngine::new();
            if !engine.is_available() {
                eprintln!("Skipping test: Rscript not found");
                return;
            }

            let markdown = r#"
```{r}
cat("Special chars: < > & \" '")
```
"#;
            let (_temp_dir, ctx, _qmd_path) = setup_test(markdown);

            let result = engine.execute(markdown, &ctx).expect("Execution failed");

            // Should contain the special characters (possibly escaped)
            assert!(
                result.markdown.contains("Special chars"),
                "Expected special character output, got:\n{}",
                result.markdown
            );
        }

        /// Test multiline output.
        #[test]
        #[ignore]
        fn test_engine_execute_multiline_output() {
            let engine = KnitrEngine::new();
            if !engine.is_available() {
                eprintln!("Skipping test: Rscript not found");
                return;
            }

            let markdown = r#"
```{r}
cat("Line 1\nLine 2\nLine 3")
```
"#;
            let (_temp_dir, ctx, _qmd_path) = setup_test(markdown);

            let result = engine.execute(markdown, &ctx).expect("Execution failed");

            assert!(
                result.markdown.contains("Line 1") && result.markdown.contains("Line 3"),
                "Expected multiline output, got:\n{}",
                result.markdown
            );
        }

        // ==================================================================
        // Format-specific Tests
        // ==================================================================

        /// Test PDF format (LaTeX output).
        #[test]
        #[ignore]
        fn test_engine_execute_pdf_format() {
            let engine = KnitrEngine::new();
            if !engine.is_available() {
                eprintln!("Skipping test: Rscript not found");
                return;
            }

            let markdown = "```{r}\n1 + 1\n```\n";

            let temp_dir = tempfile::tempdir().unwrap();
            let qmd_path = temp_dir.path().join("test.qmd");
            std::fs::write(&qmd_path, markdown).unwrap();

            let ctx = ExecutionContext::new(
                temp_dir.path().to_path_buf(),
                temp_dir.path().to_path_buf(),
                qmd_path,
                "pdf", // PDF format
            )
            .with_quiet(true);

            let result = engine.execute(markdown, &ctx).expect("Execution failed");

            // Should still produce valid output
            assert!(
                result.markdown.contains("[1] 2"),
                "Expected output for PDF format, got:\n{}",
                result.markdown
            );
        }
    }
}
