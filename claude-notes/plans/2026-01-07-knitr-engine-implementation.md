# Plan: Knitr Engine Implementation (Phase 4)

**Issue**: k-ydzc
**Date**: 2026-01-07
**Status**: Ready for Implementation
**Blocks**: k-oomv (ExecutionEngine infrastructure)

## Decisions Made

- **Resource files**: Lazy temp directory only (no env override needed)
- **R scripts**: Copy verbatim from quarto-cli - aim for full compatibility
- **Deferred features**: spin files, code-link, dependencies action
- **CI**: Set up R like quarto-cli's GHA workflows
- **Pandoc**: Assume Pandoc on PATH is sufficient (no special QUARTO_BIN_PATH handling)

## Overview

Implement the knitr execution engine for R code cells in Quarto documents. This enables rendering `.qmd` files with `{r}` code blocks by shelling out to R/Rscript.

## Background

### TypeScript Implementation Analysis

The TypeScript knitr engine (`src/execute/rmd.ts`) follows this architecture:

1. **Entry Point**: `knitrEngineDiscovery.launch()` returns an `ExecutionEngineInstance`
2. **R Communication**: `callR()` spawns `Rscript` with `rmd/rmd.R` as the script
3. **Protocol**: JSON parameters via stdin, JSON results via temp file
4. **Actions**: `execute`, `dependencies`, `postprocess`, `run`, `spin`

### R Script Structure

The R scripts in `src/resources/rmd/`:

| File | Purpose |
|------|---------|
| `rmd.R` | Entry point - reads JSON from stdin, dispatches to handlers |
| `execute.R` | Main execution via `rmarkdown::render()` |
| `hooks.R` | Knitr knit_hooks and opts_hooks for Quarto output formatting |
| `patch.R` | Monkeypatches to knitr functions for Quarto compatibility |
| `ojs.R` | OJS integration for Shiny |
| `ojs_static.R` | OJS definitions for static documents |

### Execution Flow

```
Rust                          R (Rscript)
─────                         ───────────
1. Preprocess markdown
   (inline R resolution)
2. Serialize to JSON
3. Create temp results file
4. Spawn Rscript rmd/rmd.R ──→ 5. Read JSON from stdin
   (pass JSON via stdin)       6. Source helper files
                               7. Dispatch to execute()
                               8. Call rmarkdown::render()
                               9. Collect output markdown
                              10. Write JSON to results file
11. Read results file ←───────
12. Parse JSON
13. Post-process output
    (filename fixups)
14. Return ExecuteResult
```

### Key Data Structures

**Request JSON** (stdin to R):
```json
{
  "action": "execute",
  "params": {
    "input": "/path/to/doc.qmd",
    "markdown": "---\ntitle: Doc\n---\n\n# Hello\n\n```{r}\n1+1\n```",
    "format": { "pandoc": {"to": "html"}, "execute": {...} },
    "tempDir": "/tmp/quarto-xxx",
    "libDir": null,
    "dependencies": true,
    "cwd": "/project",
    "params": null,
    "resourceDir": "/usr/share/quarto/rmd",
    "handledLanguages": ["ojs", "mermaid", "dot"]
  },
  "results": "/tmp/r-results-xxx.json",
  "wd": "/project"
}
```

**Note on `handledLanguages`**: These are languages that knitr should pass through unchanged (as fenced code blocks) because Quarto handles them separately. The R code in `execute.R:68-92` registers pass-through knitr engines for these languages.

**Response JSON** (from results file):
```json
{
  "engine": "knitr",
  "markdown": "---\ntitle: Doc\n---\n\n# Hello\n\n::: {.cell}\n```{.r .cell-code}\n1+1\n```\n\n::: {.cell-output .cell-output-stdout}\n```\n[1] 2\n```\n:::\n:::",
  "supporting": ["/path/to/doc_files"],
  "filters": ["rmarkdown/pagebreak.lua"],
  "includes": {"include-in-header": "/tmp/header.html"},
  "engineDependencies": null,
  "preserve": null,
  "postProcess": false
}
```

**Note on `includes`**: R may return `[]` instead of `{}` for empty includes. Handle this defensively during deserialization.

**Note on `filters`**: The path `rmarkdown/pagebreak.lua` is relative to Quarto's resource directory. For MVP, we can ignore this filter or resolve it appropriately.

---

## Design

### 1. R Resource File Management

**Problem**: R scripts must exist on disk for Rscript to execute them.

**Solution**: Lazy-populated temp directory with embedded R scripts.

- R scripts copied verbatim from `quarto-cli/src/resources/rmd/`
- Embedded via `include_bytes!` macro
- Extracted to temp directory on first use
- Auto-cleaned on process exit via `tempfile::TempDir`

```rust
// crates/quarto-core/src/engine/knitr/resources.rs

use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use tempfile::TempDir;

use super::ExecutionError;

/// R script resources embedded in the binary (copied verbatim from quarto-cli)
static R_RESOURCES: &[(&str, &[u8])] = &[
    ("rmd.R", include_bytes!("resources/rmd.R")),
    ("execute.R", include_bytes!("resources/execute.R")),
    ("hooks.R", include_bytes!("resources/hooks.R")),
    ("patch.R", include_bytes!("resources/patch.R")),
    ("ojs.R", include_bytes!("resources/ojs.R")),
    ("ojs_static.R", include_bytes!("resources/ojs_static.R")),
];

/// Global resource directory (lazily initialized)
static RESOURCE_DIR: OnceLock<TempDir> = OnceLock::new();

/// Get the resource directory, creating it if necessary.
pub fn resource_dir() -> Result<&'static Path, ExecutionError> {
    let temp_dir = RESOURCE_DIR.get_or_init(|| {
        let dir = tempfile::Builder::new()
            .prefix("quarto-r-resources-")
            .tempdir()
            .expect("Failed to create temp directory for R resources");

        // Write all R scripts to the temp directory
        let rmd_dir = dir.path().join("rmd");
        std::fs::create_dir_all(&rmd_dir).expect("Failed to create rmd subdirectory");

        for (name, content) in R_RESOURCES {
            let path = rmd_dir.join(name);
            std::fs::write(&path, content).expect("Failed to write R resource");
        }

        dir
    });

    Ok(temp_dir.path())
}
```

### 2. Inline R Expression Resolution

**Problem**: Inline R expressions like `` `r 1+1` `` need preprocessing before being sent to R.

**Solution**: Transform inline expressions to use `.QuartoInlineRender()` wrapper.

```rust
// crates/quarto-core/src/engine/knitr/preprocess.rs

use regex::Regex;

/// Resolve inline R expressions for proper rendering.
///
/// Transforms `` `r expr` `` to `` `r .QuartoInlineRender(expr)` ``
/// This wrapper function (defined in execute.R) handles proper escaping
/// of special markdown characters in the output.
pub fn resolve_inline_r_expressions(markdown: &str) -> String {
    // Match inline R code: `r expression`
    // The backtick-r pattern followed by code until closing backtick
    let re = Regex::new(r"`r\s+([^`]+)`").unwrap();

    re.replace_all(markdown, |caps: &regex::Captures| {
        let expr = &caps[1];
        format!("`r .QuartoInlineRender({})`", expr.trim())
    }).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_inline_r_resolution() {
        let input = "The answer is `r 1+1` and `r x*2`.";
        let output = resolve_inline_r_expressions(input);
        assert_eq!(
            output,
            "The answer is `r .QuartoInlineRender(1+1)` and `r .QuartoInlineRender(x*2)`."
        );
    }

    #[test]
    fn test_no_inline_r() {
        let input = "No R code here, just `code`.";
        let output = resolve_inline_r_expressions(input);
        assert_eq!(output, input);
    }
}
```

### 3. Subprocess Management

```rust
// crates/quarto-core/src/engine/knitr/subprocess.rs

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::de::DeserializeOwned;
use serde::Serialize;

use super::resources::resource_dir;
use super::ExecutionError;

/// Find the Rscript binary.
///
/// Checks in order:
/// 1. QUARTO_R environment variable (path to R installation)
/// 2. PATH via `which`
pub fn find_rscript() -> Option<PathBuf> {
    // Try environment variable first
    if let Ok(path) = std::env::var("QUARTO_R") {
        let rscript = PathBuf::from(&path).join("Rscript");
        if rscript.exists() {
            return Some(rscript);
        }
        // Also try the path directly if it points to Rscript
        let direct = PathBuf::from(&path);
        if direct.exists() && direct.file_name().map(|n| n == "Rscript").unwrap_or(false) {
            return Some(direct);
        }
    }

    // Try which/where
    which::which("Rscript").ok()
}

/// Check if we're within an active renv project.
///
/// Returns true if `.Rprofile` exists and contains `source("renv/activate.R")`
/// that is not commented out.
fn within_active_renv(dir: &Path) -> bool {
    let rprofile = dir.join(".Rprofile");
    if let Ok(content) = std::fs::read_to_string(&rprofile) {
        let activate_line = r#"source("renv/activate.R")"#;
        let commented_line = format!("# {}", activate_line);
        content.contains(activate_line) && !content.contains(&commented_line)
    } else {
        false
    }
}

/// Callback for filtering/transforming R subprocess output.
pub type OutputFilter = fn(&str) -> String;

/// Call R with the given action and parameters.
///
/// # Arguments
///
/// * `action` - The action to perform ("execute", "dependencies", etc.)
/// * `params` - Parameters to pass to R (will be JSON-serialized)
/// * `temp_dir` - Temporary directory for result files
/// * `project_dir` - Project directory (used for working directory determination)
/// * `document_dir` - Document's parent directory (for renv detection)
/// * `quiet` - If true, capture stderr; if false, inherit it
/// * `output_filter` - Optional callback to transform stderr output
pub fn call_r<T: DeserializeOwned>(
    action: &str,
    params: &impl Serialize,
    temp_dir: &Path,
    project_dir: Option<&Path>,
    document_dir: &Path,
    quiet: bool,
    output_filter: Option<OutputFilter>,
) -> Result<T, ExecutionError> {
    let rscript = find_rscript()
        .ok_or_else(|| ExecutionError::runtime_not_found("knitr", "Rscript"))?;

    // Create results file using tempfile
    let results_file = tempfile::Builder::new()
        .prefix("r-results-")
        .suffix(".json")
        .tempfile_in(temp_dir)
        .map_err(|e| ExecutionError::io("creating results file", e))?;
    let results_path = results_file.path().to_path_buf();
    // Keep the file open so it's not deleted until we're done
    let _results_handle = results_file;

    // Determine working directory:
    // - If within active renv, use document_dir
    // - Otherwise, use project_dir if specified, else document_dir
    let cwd = if within_active_renv(document_dir) {
        document_dir.to_path_buf()
    } else {
        project_dir.unwrap_or(document_dir).to_path_buf()
    };

    // Build request JSON
    let request = serde_json::json!({
        "action": action,
        "params": params,
        "results": results_path,
        "wd": cwd,
    });
    let input = serde_json::to_string(&request)
        .map_err(|e| ExecutionError::serialization("request JSON", e))?;

    // Get resource directory
    let resource_dir = resource_dir()?;
    let rmd_script = resource_dir.join("rmd").join("rmd.R");

    // Parse additional Rscript args from environment
    // e.g., QUARTO_KNITR_RSCRIPT_ARGS="--vanilla,--no-init-file"
    let rscript_args: Vec<String> = std::env::var("QUARTO_KNITR_RSCRIPT_ARGS")
        .unwrap_or_default()
        .split(',')
        .filter(|s| !s.trim().is_empty())
        .map(String::from)
        .collect();

    // Spawn Rscript
    let mut child = Command::new(&rscript)
        .args(&rscript_args)
        .arg(&rmd_script)
        .current_dir(&cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(if quiet { Stdio::piped() } else { Stdio::inherit() })
        .spawn()
        .map_err(|e| ExecutionError::io("spawning Rscript", e))?;

    // Write request to stdin
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(input.as_bytes())
            .map_err(|e| ExecutionError::io("writing to Rscript stdin", e))?;
    }

    // Wait for completion
    let output = child
        .wait_with_output()
        .map_err(|e| ExecutionError::io("waiting for Rscript", e))?;

    if !output.status.success() {
        let mut stderr = String::from_utf8_lossy(&output.stderr).into_owned();

        // Apply output filter if provided
        if let Some(filter) = output_filter {
            stderr = filter(&stderr);
        }

        return Err(ExecutionError::execution_failed(stderr));
    }

    // Read results
    let results_json = std::fs::read_to_string(&results_path)
        .map_err(|e| ExecutionError::io("reading results file", e))?;

    // Parse results, handling the array/object quirk for includes
    let result: T = serde_json::from_str(&results_json)
        .map_err(|e| ExecutionError::serialization("results JSON", e))?;

    Ok(result)
}
```

### 4. Format Configuration Types

The R scripts need detailed format configuration. These structs mirror the TypeScript format structure.

```rust
// crates/quarto-core/src/engine/knitr/format.rs

use serde::Serialize;

/// Complete format configuration for knitr execution.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KnitrFormatConfig {
    /// Pandoc options
    pub pandoc: PandocConfig,

    /// Execution options (controls knitr behavior)
    pub execute: ExecuteConfig,

    /// Render options
    pub render: RenderConfig,

    /// Format identifier info
    #[serde(skip_serializing_if = "Option::is_none")]
    pub identifier: Option<IdentifierConfig>,

    /// Additional metadata passed through to R
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

/// Pandoc-specific options.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PandocConfig {
    /// Output format (e.g., "html", "pdf", "latex")
    pub to: String,

    /// Input format
    pub from: String,
}

/// Code execution options.
///
/// These control knitr's behavior for code chunks.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct ExecuteConfig {
    /// Figure width in inches
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fig_width: Option<f64>,

    /// Figure height in inches
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fig_height: Option<f64>,

    /// Figure aspect ratio
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fig_asp: Option<f64>,

    /// Figure DPI
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fig_dpi: Option<u32>,

    /// Figure format (e.g., "png", "svg", "pdf")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fig_format: Option<String>,

    /// Whether to evaluate code chunks
    #[serde(skip_serializing_if = "Option::is_none")]
    pub eval: Option<bool>,

    /// Whether to display code
    #[serde(skip_serializing_if = "Option::is_none")]
    pub echo: Option<serde_json::Value>, // Can be bool or "fenced"

    /// Whether to display warnings
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warning: Option<bool>,

    /// Whether to halt on errors
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<bool>,

    /// Whether to include chunk output
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include: Option<bool>,

    /// Whether to output results
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<serde_json::Value>, // Can be bool or "asis"

    /// Cache behavior
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache: Option<serde_json::Value>, // Can be bool or "refresh"

    /// How to print data frames
    #[serde(skip_serializing_if = "Option::is_none")]
    pub df_print: Option<String>,

    /// Execution enabled
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,

    /// Debug mode
    #[serde(skip_serializing_if = "Option::is_none")]
    pub debug: Option<bool>,
}

/// Render options.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct RenderConfig {
    /// Keep hidden chunks in output
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keep_hidden: Option<bool>,

    /// Enable code linking via downlit
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code_link: Option<bool>,

    /// Keep TeX source
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keep_tex: Option<bool>,

    /// Default figure position for LaTeX
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fig_pos: Option<String>,

    /// Preserve notebook cells
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notebook_preserve_cells: Option<bool>,

    /// Produce source notebook
    #[serde(skip_serializing_if = "Option::is_none")]
    pub produce_source_notebook: Option<bool>,

    /// Prefer HTML output (for markdown formats)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prefer_html: Option<bool>,
}

/// Format identifier info.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct IdentifierConfig {
    /// Base format name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_format: Option<String>,
}

impl Default for ExecuteConfig {
    fn default() -> Self {
        Self {
            fig_width: Some(7.0),
            fig_height: Some(5.0),
            fig_asp: None,
            fig_dpi: Some(96),
            fig_format: Some("png".into()),
            eval: Some(true),
            echo: Some(serde_json::Value::Bool(true)),
            warning: Some(true),
            error: Some(false),
            include: Some(true),
            output: None,
            cache: None,
            df_print: Some("default".into()),
            enabled: None,
            debug: None,
        }
    }
}

impl Default for RenderConfig {
    fn default() -> Self {
        Self {
            keep_hidden: Some(false),
            code_link: Some(false),
            keep_tex: None,
            fig_pos: None,
            notebook_preserve_cells: None,
            produce_source_notebook: None,
            prefer_html: None,
        }
    }
}
```

### 5. KnitrEngine Implementation

```rust
// crates/quarto-core/src/engine/knitr.rs

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::engine::{ExecuteResult, ExecutionContext, ExecutionEngine, ExecutionError};
use crate::stage::PandocIncludes;

mod format;
mod preprocess;
mod resources;
mod subprocess;

use format::{ExecuteConfig, IdentifierConfig, KnitrFormatConfig, PandocConfig, RenderConfig};
use preprocess::resolve_inline_r_expressions;
use resources::resource_dir;
use subprocess::{call_r, find_rscript};

/// Knitr engine for R code execution.
#[cfg(not(target_arch = "wasm32"))]
pub struct KnitrEngine {
    rscript_path: Option<PathBuf>,
}

#[cfg(not(target_arch = "wasm32"))]
impl KnitrEngine {
    pub fn new() -> Self {
        Self {
            rscript_path: find_rscript(),
        }
    }

    pub fn is_available(&self) -> bool {
        self.rscript_path.is_some()
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Default for KnitrEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl ExecutionEngine for KnitrEngine {
    fn name(&self) -> &str {
        "knitr"
    }

    fn is_available(&self) -> bool {
        self.rscript_path.is_some()
    }

    fn execute(
        &self,
        input: &str,
        context: &ExecutionContext,
    ) -> Result<ExecuteResult, ExecutionError> {
        if self.rscript_path.is_none() {
            return Err(ExecutionError::runtime_not_found("knitr", "Rscript"));
        }

        // Step 1: Preprocess markdown (resolve inline R expressions)
        let preprocessed = resolve_inline_r_expressions(input);

        // Step 2: Build format configuration
        let format_config = build_format_config(context);

        // Step 3: Build execute parameters
        let params = KnitrExecuteParams {
            input: context.source_path.clone(),
            markdown: preprocessed,
            format: format_config,
            temp_dir: context.temp_dir.clone(),
            lib_dir: None,
            dependencies: true,
            cwd: context.cwd.clone(),
            params: None,
            resource_dir: resource_dir()?.to_path_buf(),
            handled_languages: vec!["ojs".into(), "mermaid".into(), "dot".into()],
        };

        // Step 4: Determine document directory for renv detection
        let document_dir = context
            .source_path
            .parent()
            .unwrap_or(&context.cwd);

        // Step 5: Create output filter for post-processing stderr
        let input_basename = context
            .source_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("document.qmd")
            .to_string();
        let input_stem = context
            .source_path
            .file_stem()
            .and_then(|n| n.to_str())
            .unwrap_or("document")
            .to_string();

        // Step 6: Call R
        let result: KnitrExecuteResult = call_r(
            "execute",
            &params,
            &context.temp_dir,
            context.project_dir.as_deref(),
            document_dir,
            context.quiet,
            None, // Output filter - can add filename fixup later
        )?;

        // Step 7: Post-process the markdown output
        let mut markdown = result.markdown;

        // Fix .rmarkdown references back to original filename
        let rmarkdown_name = format!("{}.rmarkdown", input_stem);
        markdown = markdown.replace(&rmarkdown_name, &input_basename);

        // Step 8: Convert includes
        let includes = convert_includes(result.includes);

        // Step 9: Return result
        Ok(ExecuteResult {
            markdown,
            supporting_files: result
                .supporting
                .into_iter()
                .map(PathBuf::from)
                .collect(),
            filters: result.filters,
            includes,
            needs_postprocess: result.post_process,
        })
    }

    fn can_freeze(&self) -> bool {
        true
    }

    fn intermediate_files(&self, input_path: &Path) -> Vec<PathBuf> {
        // knitr produces {input}_files/ directory
        let stem = input_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("");

        if stem.is_empty() {
            return Vec::new();
        }

        let parent = input_path.parent().unwrap_or(Path::new("."));
        vec![parent.join(format!("{}_files", stem))]
    }
}

/// Build format configuration from execution context.
fn build_format_config(context: &ExecutionContext) -> KnitrFormatConfig {
    // Extract execute options from engine config if present
    let execute_config = if let Some(ref config) = context.engine_config {
        // TODO: Parse execute options from ConfigValue
        ExecuteConfig::default()
    } else {
        ExecuteConfig::default()
    };

    KnitrFormatConfig {
        pandoc: PandocConfig {
            to: context.format.clone(),
            from: "markdown".into(),
        },
        execute: execute_config,
        render: RenderConfig::default(),
        identifier: None,
        metadata: None,
    }
}

/// Convert knitr includes to PandocIncludes.
fn convert_includes(includes: Option<KnitrIncludes>) -> PandocIncludes {
    let Some(inc) = includes else {
        return PandocIncludes::default();
    };

    let mut result = PandocIncludes::default();

    // Read include file contents
    if let Some(path) = inc.include_in_header {
        if let Ok(content) = std::fs::read_to_string(&path) {
            result.in_header.push(content);
        }
    }
    if let Some(path) = inc.include_before_body {
        if let Ok(content) = std::fs::read_to_string(&path) {
            result.before_body.push(content);
        }
    }
    if let Some(path) = inc.include_after_body {
        if let Ok(content) = std::fs::read_to_string(&path) {
            result.after_body.push(content);
        }
    }

    result
}

/// Parameters for the execute action.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct KnitrExecuteParams {
    input: PathBuf,
    markdown: String,
    format: KnitrFormatConfig,
    temp_dir: PathBuf,
    #[serde(skip_serializing_if = "Option::is_none")]
    lib_dir: Option<PathBuf>,
    dependencies: bool,
    cwd: PathBuf,
    #[serde(skip_serializing_if = "Option::is_none")]
    params: Option<serde_json::Value>,
    resource_dir: PathBuf,
    handled_languages: Vec<String>,
}

/// Result from the execute action.
///
/// Note: The `includes` field may be `[]` instead of `{}` when empty.
/// The custom deserializer handles this case.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct KnitrExecuteResult {
    #[allow(dead_code)]
    engine: String,
    markdown: String,
    #[serde(default)]
    supporting: Vec<String>,
    #[serde(default)]
    filters: Vec<String>,
    #[serde(default, deserialize_with = "deserialize_includes")]
    includes: Option<KnitrIncludes>,
    #[allow(dead_code)]
    engine_dependencies: Option<serde_json::Value>,
    #[allow(dead_code)]
    preserve: Option<serde_json::Value>,
    #[serde(default)]
    post_process: bool,
}

/// Includes from knitr execution.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
struct KnitrIncludes {
    include_in_header: Option<PathBuf>,
    include_before_body: Option<PathBuf>,
    include_after_body: Option<PathBuf>,
}

/// Custom deserializer that handles both `{}` and `[]` for includes.
fn deserialize_includes<'de, D>(deserializer: D) -> Result<Option<KnitrIncludes>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::Error;

    let value: serde_json::Value = serde::Deserialize::deserialize(deserializer)?;

    match value {
        serde_json::Value::Null => Ok(None),
        serde_json::Value::Array(arr) if arr.is_empty() => Ok(None),
        serde_json::Value::Object(_) => {
            let includes: KnitrIncludes = serde_json::from_value(value)
                .map_err(D::Error::custom)?;
            Ok(Some(includes))
        }
        _ => Err(D::Error::custom("expected object, array, or null for includes")),
    }
}
```

---

## Implementation Phases

### Phase 4.1: Resource Management ✅ COMPLETE
1. Create `crates/quarto-core/src/engine/knitr/` module structure
2. Copy R scripts from quarto-cli to `crates/quarto-core/src/engine/knitr/resources/`
3. Implement lazy resource extraction with `include_dir!` crate
4. Add unit tests for resource directory creation

### Phase 4.2: Data Structures ✅ COMPLETE
1. Implement `KnitrFormatConfig` and related structs (`format.rs`)
2. Implement `KnitrExecuteParams` and `KnitrExecuteResult` (`types.rs`)
3. Implement custom deserializer for `includes` array/object handling
4. Add unit tests for serialization/deserialization

### Phase 4.3: Subprocess Communication ✅ COMPLETE
1. Implement `find_rscript()` with QUARTO_R support (`subprocess.rs`)
2. Implement `within_active_renv()` for working directory logic
3. Implement `call_r()` generic function
4. Test basic R communication with simple action (integration tests)

### Phase 4.4: Preprocessing and Execute Action ✅ COMPLETE
1. Implement `resolve_inline_r_expressions()` (`preprocess.rs`)
2. Implement `build_format_config()`
3. Implement `KnitrEngine::execute()` with full pipeline
4. Implement `.rmarkdown` filename fixup in output (`postprocess_markdown()`)
5. Test with simple R code blocks (8 integration tests passing)

### Phase 4.5: Error Handling ✅ COMPLETE
1. Parse R error messages from stderr
2. Provide helpful error messages for:
   - Missing R/Rscript
   - Missing knitr/rmarkdown packages
   - R execution errors
3. Map "Quitting from lines X-Y" messages back to source (basic version)

**Implemented:**
- Created `error_parser.rs` module with:
  - `RErrorType` enum: `MissingPackage`, `PackageVersionTooOld`, `KnitrExecutionError`, `RNotFound`, `Generic`
  - `RErrorInfo` struct with error type, message, suggestion, and source line range
  - `parse_r_error()` function that parses stderr and returns structured error info
  - Pattern matching for common R error messages
- Added new `ExecutionError` variants:
  - `MissingPackage` - for missing R packages with install suggestions
  - `PackageVersionTooOld` - for outdated packages with update suggestions
  - `ExecutionFailedAtLines` - for execution errors with source line information
- Updated `call_r()` to use error parser and convert to appropriate `ExecutionError`
- Added 27 unit tests for error parsing and conversion

### Phase 4.6: Integration Testing ✅ COMPLETE
1. Add integration tests with actual R execution
2. Test figure output handling
3. Test cache behavior
4. Test various chunk options (echo, eval, warning, etc.)
5. Test error cases

**Implemented:**
- Added 19 integration tests in `crates/quarto-core/src/engine/knitr/mod.rs`
- Tests require R installation and are marked with `#[ignore]`
- Run with: `cargo nextest run --package quarto-core --run-ignored all -- 'knitr::tests::integration'`

**Test Coverage:**
- Basic execution: `1 + 1` produces `[1] 2`
- Inline R expressions: `` `r 2+2` `` produces `4`
- Multiple chunks: variables persist across chunks
- Figure output: plots produce images in markdown
- Figure with label: `#| label: fig-*` chunks execute correctly
- Chunk options:
  - `echo: false` - hides source code
  - `eval: false` - shows code but doesn't execute
  - `include: false` - hides both code and output but runs
  - `output: false` - suppresses output
- Error handling:
  - `error: true` - shows error messages in output
  - Execution failure returns appropriate error
  - Undefined variables cause execution failure
- Data frame output: `head(mtcars)` works correctly
- Special characters and multiline output handled
- PDF format works correctly

---

## Decisions and Deferred Work

### Decided

| Question | Decision |
|----------|----------|
| R resource files | Lazy temp directory only (no env override) |
| R scripts | Copy verbatim from quarto-cli for compatibility |
| renv detection | Check `.Rprofile` for `source("renv/activate.R")` |
| Pandoc discovery | Assume Pandoc on PATH is sufficient |
| CI testing | Set up R using quarto-cli's GHA workflow patterns |
| Results file | Use `tempfile` crate, not manual UUID generation |

### Deferred to Future Work

| Feature | Reason |
|---------|--------|
| `.R` spin files | Not critical for MVP |
| `code-link: true` (downlit) | Requires additional R packages |
| `dependencies()` action | Used for html_dependencies, not critical for MVP |
| Full error line number mapping | Complex; basic handling sufficient for MVP |
| Filter path resolution | `rmarkdown/pagebreak.lua` - can skip for MVP |

---

## Testing Strategy

### Unit Tests
- Resource extraction works correctly
- JSON serialization/deserialization is correct (including array/object quirk)
- Rscript path detection works (with QUARTO_R)
- renv detection works correctly
- Inline R expression resolution works
- Format config building works

### Integration Tests (require R)

```rust
#[cfg(test)]
mod integration_tests {
    use super::*;

    fn make_test_context(source_path: &str) -> ExecutionContext {
        ExecutionContext::new(
            std::env::temp_dir(),
            std::env::current_dir().unwrap(),
            PathBuf::from(source_path),
            "html",
        )
    }

    #[test]
    #[ignore] // Requires R installation
    fn test_knitr_basic_execution() {
        let engine = KnitrEngine::new();
        if !engine.is_available() {
            eprintln!("Skipping: R not available");
            return;
        }

        let input = "# Hello\n\n```{r}\n1 + 1\n```\n";
        let ctx = make_test_context("/tmp/test.qmd");
        let result = engine.execute(input, &ctx).unwrap();

        assert!(result.markdown.contains("[1] 2"));
        assert!(result.markdown.contains(".cell"));
    }

    #[test]
    #[ignore] // Requires R installation
    fn test_knitr_multiple_chunks() {
        let engine = KnitrEngine::new();
        if !engine.is_available() {
            return;
        }

        let input = r#"
# Test

```{r}
x <- 10
x
```

```{r}
x * 2
```
"#;
        let ctx = make_test_context("/tmp/test.qmd");
        let result = engine.execute(input, &ctx).unwrap();

        assert!(result.markdown.contains("[1] 10"));
        assert!(result.markdown.contains("[1] 20"));
    }

    #[test]
    #[ignore] // Requires R installation
    fn test_knitr_echo_false() {
        let engine = KnitrEngine::new();
        if !engine.is_available() {
            return;
        }

        let input = r#"
```{r}
#| echo: false
1 + 1
```
"#;
        let ctx = make_test_context("/tmp/test.qmd");
        let result = engine.execute(input, &ctx).unwrap();

        // Output should be present
        assert!(result.markdown.contains("[1] 2"));
        // But source code should be hidden
        assert!(!result.markdown.contains("1 + 1") || result.markdown.contains("hidden"));
    }

    #[test]
    #[ignore] // Requires R installation
    fn test_knitr_eval_false() {
        let engine = KnitrEngine::new();
        if !engine.is_available() {
            return;
        }

        let input = r#"
```{r}
#| eval: false
stop("This should not run")
```
"#;
        let ctx = make_test_context("/tmp/test.qmd");
        let result = engine.execute(input, &ctx).unwrap();

        // Code should be shown but not executed
        assert!(result.markdown.contains("stop"));
        assert!(!result.markdown.contains("Error"));
    }

    #[test]
    #[ignore] // Requires R installation
    fn test_knitr_chunk_label() {
        let engine = KnitrEngine::new();
        if !engine.is_available() {
            return;
        }

        let input = r#"
```{r}
#| label: fig-example
#| fig-cap: "A plot"
plot(1:10)
```
"#;
        let ctx = make_test_context("/tmp/test.qmd");
        let result = engine.execute(input, &ctx).unwrap();

        // Should have the label in output
        assert!(result.markdown.contains("fig-example"));
    }

    #[test]
    #[ignore] // Requires R installation
    fn test_knitr_figure_output() {
        let engine = KnitrEngine::new();
        if !engine.is_available() {
            return;
        }

        let input = r#"
```{r}
plot(1:10)
```
"#;
        let ctx = make_test_context("/tmp/test.qmd");
        let result = engine.execute(input, &ctx).unwrap();

        // Should produce a figure
        assert!(result.markdown.contains("!["));
    }

    #[test]
    #[ignore] // Requires R installation
    fn test_knitr_inline_r() {
        let engine = KnitrEngine::new();
        if !engine.is_available() {
            return;
        }

        let input = "The answer is `r 2+2`.\n";
        let ctx = make_test_context("/tmp/test.qmd");
        let result = engine.execute(input, &ctx).unwrap();

        assert!(result.markdown.contains("4"));
    }
}
```

### CI Setup (based on quarto-cli)

Reference: `quarto-cli/.github/workflows/test-smokes.yml`

```yaml
- name: Set up R
  uses: r-lib/actions/setup-r@v2
  with:
    r-version: "4.3.1"
    use-public-rspm: true

- name: Install R packages
  run: |
    install.packages('renv')
    install.packages('rmarkdown', repos = c('https://rstudio.r-universe.dev', getOption('repos')))
    install.packages('knitr', repos = c('https://yihui.r-universe.dev', getOption('repos')))
    install.packages('jsonlite')
  shell: Rscript {0}

- name: Run knitr integration tests
  run: cargo nextest run --features knitr-integration-tests -- --ignored
```

### CI Considerations
- Integration tests marked with `#[ignore]`
- Separate test target or feature flag for R tests
- Run R tests on Linux CI (simplest setup)
- Tests skip gracefully when R unavailable

---

## Dependencies

### Rust Crates

Add to `crates/quarto-core/Cargo.toml`:

```toml
[dependencies]
which = "6.0"      # Find Rscript binary
tempfile = "3.10"  # Temporary directory management
regex = "1.10"     # For inline R expression resolution

# Already present:
# serde, serde_json, async-trait, etc.
```

Note: We use `tempfile` instead of `uuid` for generating unique result filenames.

### R Packages (runtime)

Users must have installed:
- `knitr` >= 1.44 (for yaml chunk options)
- `rmarkdown` >= 2.9.4
- `jsonlite` - JSON parsing
- `xfun` - Utility functions (usually installed with knitr)

---

## Success Criteria

### Phase 4 Complete When:

- [x] R resource files are embedded and extracted correctly
- [x] Rscript subprocess communication works (with renv detection)
- [x] Basic code chunk execution: `1 + 1` produces `[1] 2`
- [x] Multiple chunks in sequence work correctly
- [x] Chunk options work: `echo: false`, `eval: false`
- [x] Chunk labels work: `#| label: fig-example`
- [x] Figure outputs are collected in supporting files
- [x] Inline R expressions work: `` `r 2+2` `` produces `4`
- [x] Errors from R are reported with useful messages
- [x] Integration tests pass (when R is available)

**Phase 4 COMPLETE** ✅

---

## Module Structure

```
crates/quarto-core/src/engine/
├── mod.rs                 # Re-exports
├── knitr/
│   ├── mod.rs            # KnitrEngine implementation
│   ├── format.rs         # Format configuration types
│   ├── preprocess.rs     # Inline R resolution
│   ├── subprocess.rs     # R subprocess management
│   ├── resources.rs      # Embedded R scripts
│   └── resources/        # R script files (copied from quarto-cli)
│       ├── rmd.R
│       ├── execute.R
│       ├── hooks.R
│       ├── patch.R
│       ├── ojs.R
│       └── ojs_static.R
├── jupyter.rs            # (existing placeholder)
├── markdown.rs           # (existing)
├── detection.rs          # (existing)
├── registry.rs           # (existing)
├── traits.rs             # (existing)
├── context.rs            # (existing)
├── error.rs              # (existing)
└── reconcile.rs          # (existing)
```

---

## Related Documents

- [Execution Engine Infrastructure](2026-01-06-execution-engine-infrastructure.md) - Phase 1-3
- [Source Location Reconciliation](2025-12-15-engine-output-source-location-reconciliation.md) - AST reconciliation
- [TypeScript rmd.ts](external-sources/quarto-cli/src/execute/rmd.ts) - Reference implementation
- [TypeScript execute.R](external-sources/quarto-cli/src/resources/rmd/execute.R) - R execution script
