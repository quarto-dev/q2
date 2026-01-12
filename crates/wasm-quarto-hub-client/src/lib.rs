/*
 * wasm-quarto-hub-client
 * Copyright (c) 2025 Posit, PBC
 *
 * WASM client for quarto-hub web frontend.
 * Provides VFS management and document rendering capabilities.
 */

// For `vsnprintf()` and `fprintf()`, which are variadic.
#![feature(c_variadic)]

// Provide rust implementation of blessed stdlib functions to
// tree-sitter itself and any grammars that have `scanner.c`.
#[cfg(target_arch = "wasm32")]
pub mod c_shim;

use std::path::Path;
use std::sync::{Arc, OnceLock};

use quarto_core::{
    BinaryDependencies, DocumentInfo, Format, HtmlRenderConfig, ProjectConfig, ProjectContext,
    QuartoError, RenderContext, RenderOptions, render_qmd_to_html,
};
use quarto_error_reporting::{DiagnosticKind, DiagnosticMessage};
use quarto_pandoc_types::ConfigValue;
use quarto_source_map::SourceContext;
use quarto_system_runtime::{SystemRuntime, WasmRuntime};
use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;

// Global runtime instance for VFS operations
static RUNTIME: OnceLock<WasmRuntime> = OnceLock::new();

fn get_runtime() -> &'static WasmRuntime {
    RUNTIME.get_or_init(WasmRuntime::new)
}

#[wasm_bindgen(start)]
pub fn init() {
    // Set up panic hook for better error messages in browser console
    console_error_panic_hook::set_once();
}

// ============================================================================
// RESPONSE TYPES
// ============================================================================

#[derive(Serialize, Deserialize)]
struct VfsResponse {
    success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    files: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
}

impl VfsResponse {
    fn ok() -> String {
        serde_json::to_string(&VfsResponse {
            success: true,
            error: None,
            files: None,
            content: None,
        })
        .unwrap()
    }

    fn error(msg: &str) -> String {
        serde_json::to_string(&VfsResponse {
            success: false,
            error: Some(msg.to_string()),
            files: None,
            content: None,
        })
        .unwrap()
    }

    fn with_files(paths: Vec<String>) -> String {
        serde_json::to_string(&VfsResponse {
            success: true,
            error: None,
            files: Some(paths),
            content: None,
        })
        .unwrap()
    }

    fn with_content(text: String) -> String {
        serde_json::to_string(&VfsResponse {
            success: true,
            error: None,
            files: None,
            content: Some(text),
        })
        .unwrap()
    }
}

// ============================================================================
// VFS MANAGEMENT API
// ============================================================================

/// Add a text file to the virtual filesystem.
///
/// # Arguments
/// * `path` - File path (e.g., "index.qmd" or "chapters/intro.qmd")
/// * `content` - File content as UTF-8 string
///
/// # Returns
/// JSON: `{ "success": true }` or `{ "success": false, "error": "..." }`
#[wasm_bindgen]
pub fn vfs_add_file(path: &str, content: &str) -> String {
    get_runtime().add_file(Path::new(path), content.as_bytes().to_vec());
    VfsResponse::ok()
}

/// Add a binary file to the virtual filesystem.
///
/// # Arguments
/// * `path` - File path
/// * `content` - File content as bytes (Uint8Array from JS)
///
/// # Returns
/// JSON: `{ "success": true }` or `{ "success": false, "error": "..." }`
#[wasm_bindgen]
pub fn vfs_add_binary_file(path: &str, content: &[u8]) -> String {
    get_runtime().add_file(Path::new(path), content.to_vec());
    VfsResponse::ok()
}

/// Remove a file from the virtual filesystem.
///
/// # Arguments
/// * `path` - File path to remove
///
/// # Returns
/// JSON: `{ "success": true }` or `{ "success": false, "error": "File not found" }`
#[wasm_bindgen]
pub fn vfs_remove_file(path: &str) -> String {
    if get_runtime().remove_file(Path::new(path)) {
        VfsResponse::ok()
    } else {
        VfsResponse::error("File not found")
    }
}

/// List all files in the virtual filesystem.
///
/// # Returns
/// JSON: `{ "success": true, "files": ["path1", "path2", ...] }`
#[wasm_bindgen]
pub fn vfs_list_files() -> String {
    let files = get_runtime().list_files();
    let paths: Vec<String> = files
        .into_iter()
        .map(|p| p.to_string_lossy().to_string())
        .collect();
    VfsResponse::with_files(paths)
}

/// Clear all files from the virtual filesystem.
///
/// # Returns
/// JSON: `{ "success": true }`
#[wasm_bindgen]
pub fn vfs_clear() -> String {
    get_runtime().clear_files();
    VfsResponse::ok()
}

/// Read a text file from the virtual filesystem.
///
/// # Arguments
/// * `path` - File path to read
///
/// # Returns
/// JSON: `{ "success": true, "content": "..." }` or `{ "success": false, "error": "..." }`
#[wasm_bindgen]
pub fn vfs_read_file(path: &str) -> String {
    let runtime = get_runtime();

    match runtime.file_read(Path::new(path)) {
        Ok(content) => match String::from_utf8(content) {
            Ok(text) => VfsResponse::with_content(text),
            Err(_) => VfsResponse::error("File is not valid UTF-8"),
        },
        Err(e) => VfsResponse::error(&format!("Failed to read file: {}", e)),
    }
}

/// Read a binary file from the virtual filesystem.
///
/// Returns the content as base64-encoded string, suitable for data URLs.
///
/// # Arguments
/// * `path` - File path to read
///
/// # Returns
/// JSON: `{ "success": true, "content": "<base64>" }` or `{ "success": false, "error": "..." }`
#[wasm_bindgen]
pub fn vfs_read_binary_file(path: &str) -> String {
    use base64::Engine;
    let runtime = get_runtime();

    match runtime.file_read(Path::new(path)) {
        Ok(content) => {
            let base64_content = base64::engine::general_purpose::STANDARD.encode(&content);
            VfsResponse::with_content(base64_content)
        }
        Err(e) => VfsResponse::error(&format!("Failed to read file: {}", e)),
    }
}

// ============================================================================
// DIAGNOSTIC TYPES FOR JSON TRANSPORT
// ============================================================================

/// A diagnostic detail item for JSON serialization.
#[derive(Serialize)]
struct JsonDiagnosticDetail {
    kind: String,
    content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    start_line: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    start_column: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    end_line: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    end_column: Option<u32>,
}

/// A diagnostic message for JSON serialization.
///
/// This struct is designed for transport to the TypeScript/Monaco layer.
/// Line and column numbers are 1-based to match Monaco's expectations.
#[derive(Serialize)]
struct JsonDiagnostic {
    kind: String,
    title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    problem: Option<String>,
    hints: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    start_line: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    start_column: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    end_line: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    end_column: Option<u32>,
    details: Vec<JsonDiagnosticDetail>,
}

/// Convert a DiagnosticMessage to a JsonDiagnostic.
///
/// Uses the SourceContext to map byte offsets to 1-based line/column numbers.
fn diagnostic_to_json(diag: &DiagnosticMessage, ctx: &SourceContext) -> JsonDiagnostic {
    // Map the main location
    let (start_line, start_column, end_line, end_column) = if let Some(loc) = &diag.location {
        // Map start position (offset 0 relative to this SourceInfo)
        let start = loc.map_offset(0, ctx);
        // Map end position (offset = length of span)
        let end = loc
            .map_offset(loc.length(), ctx)
            .or_else(|| {
                // Fallback: if end mapping fails, try length-1
                if loc.length() > 0 {
                    loc.map_offset(loc.length() - 1, ctx)
                } else {
                    None
                }
            })
            .or_else(|| start.clone());

        match (start, end) {
            (Some(s), Some(e)) => (
                Some((s.location.row + 1) as u32),    // 1-based line
                Some((s.location.column + 1) as u32), // 1-based column
                Some((e.location.row + 1) as u32),
                Some((e.location.column + 1) as u32),
            ),
            (Some(s), None) => (
                Some((s.location.row + 1) as u32),
                Some((s.location.column + 1) as u32),
                None,
                None,
            ),
            _ => (None, None, None, None),
        }
    } else {
        (None, None, None, None)
    };

    // Convert details
    let details: Vec<JsonDiagnosticDetail> = diag
        .details
        .iter()
        .map(|detail| {
            let (d_start_line, d_start_col, d_end_line, d_end_col) =
                if let Some(loc) = &detail.location {
                    let start = loc.map_offset(0, ctx);
                    let end = loc.map_offset(loc.length(), ctx).or_else(|| start.clone());

                    match (start, end) {
                        (Some(s), Some(e)) => (
                            Some((s.location.row + 1) as u32),
                            Some((s.location.column + 1) as u32),
                            Some((e.location.row + 1) as u32),
                            Some((e.location.column + 1) as u32),
                        ),
                        (Some(s), None) => (
                            Some((s.location.row + 1) as u32),
                            Some((s.location.column + 1) as u32),
                            None,
                            None,
                        ),
                        _ => (None, None, None, None),
                    }
                } else {
                    (None, None, None, None)
                };

            let kind_str = match detail.kind {
                quarto_error_reporting::DetailKind::Error => "error",
                quarto_error_reporting::DetailKind::Info => "info",
                quarto_error_reporting::DetailKind::Note => "note",
            };

            JsonDiagnosticDetail {
                kind: kind_str.to_string(),
                content: detail.content.as_str().to_string(),
                start_line: d_start_line,
                start_column: d_start_col,
                end_line: d_end_line,
                end_column: d_end_col,
            }
        })
        .collect();

    // Convert kind
    let kind_str = match diag.kind {
        DiagnosticKind::Error => "error",
        DiagnosticKind::Warning => "warning",
        DiagnosticKind::Info => "info",
        DiagnosticKind::Note => "note",
    };

    // Convert hints
    let hints: Vec<String> = diag.hints.iter().map(|h| h.as_str().to_string()).collect();

    JsonDiagnostic {
        kind: kind_str.to_string(),
        title: diag.title.clone(),
        code: diag.code.clone(),
        problem: diag.problem.as_ref().map(|p| p.as_str().to_string()),
        hints,
        start_line,
        start_column,
        end_line,
        end_column,
        details,
    }
}

/// Convert a slice of DiagnosticMessages to JsonDiagnostics.
fn diagnostics_to_json(diags: &[DiagnosticMessage], ctx: &SourceContext) -> Vec<JsonDiagnostic> {
    diags.iter().map(|d| diagnostic_to_json(d, ctx)).collect()
}

// ============================================================================
// RENDERING API
// ============================================================================

#[derive(Serialize)]
struct RenderResponse {
    success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    html: Option<String>,
    /// Structured diagnostics (errors) with line/column information for Monaco.
    #[serde(skip_serializing_if = "Option::is_none")]
    diagnostics: Option<Vec<JsonDiagnostic>>,
    /// Structured warnings with line/column information for Monaco.
    #[serde(skip_serializing_if = "Option::is_none")]
    warnings: Option<Vec<JsonDiagnostic>>,
}

/// Create a minimal project context for WASM rendering.
fn create_wasm_project_context(path: &Path) -> ProjectContext {
    let dir = path.parent().unwrap_or(Path::new("/")).to_path_buf();
    ProjectContext {
        dir: dir.clone(),
        config: None,
        is_single_file: true,
        files: vec![DocumentInfo::from_path(path)],
        output_dir: dir,
    }
}

/// Render a QMD file from the virtual filesystem.
///
/// # Arguments
/// * `path` - Path to the QMD file in VFS (e.g., "index.qmd")
///
/// # Returns
/// JSON: `{ "success": true, "html": "..." }` or `{ "success": false, "error": "...", "diagnostics": [...] }`
#[wasm_bindgen]
pub async fn render_qmd(path: &str) -> String {
    let runtime = get_runtime();
    let path = Path::new(path);

    // Read the file from VFS
    let content = match runtime.file_read(path) {
        Ok(bytes) => bytes,
        Err(e) => {
            return serde_json::to_string(&RenderResponse {
                success: false,
                error: Some(format!("Failed to read file: {}", e)),
                html: None,
                diagnostics: None,
                warnings: None,
            })
            .unwrap();
        }
    };

    // Create minimal project context for WASM
    let project = create_wasm_project_context(path);
    let doc = DocumentInfo::from_path(path);
    let format = Format::html();
    let binaries = BinaryDependencies::new();

    let options = RenderOptions {
        verbose: false,
        execute: false,
        use_freeze: false,
        output_path: None,
    };

    let mut ctx = RenderContext::new(&project, &doc, &format, &binaries).with_options(options);

    // Use the unified async pipeline (same as CLI)
    let config = HtmlRenderConfig::default();
    let source_name = path.to_string_lossy();

    // Create Arc runtime for the async pipeline
    let runtime_arc: Arc<dyn SystemRuntime> = Arc::new(WasmRuntime::new());

    match render_qmd_to_html(&content, &source_name, &mut ctx, &config, runtime_arc).await {
        Ok(output) => {
            // Populate VFS with artifacts so post-processor can resolve them.
            // This includes CSS at /.quarto/project-artifacts/styles.css.
            for (_key, artifact) in ctx.artifacts.iter() {
                if let Some(artifact_path) = &artifact.path {
                    runtime.add_file(artifact_path, artifact.content.clone());
                }
            }

            // Convert warnings to structured JSON with line/column info
            let warnings = diagnostics_to_json(&output.warnings, &output.source_context);
            serde_json::to_string(&RenderResponse {
                success: true,
                error: None,
                html: Some(output.html),
                diagnostics: None,
                warnings: if warnings.is_empty() {
                    None
                } else {
                    Some(warnings)
                },
            })
            .unwrap()
        }
        Err(e) => {
            // Extract structured diagnostics from parse errors
            let (error_msg, diagnostics) = match &e {
                QuartoError::Parse(parse_error) => {
                    let diags =
                        diagnostics_to_json(&parse_error.diagnostics, &parse_error.source_context);
                    (e.to_string(), Some(diags))
                }
                _ => (e.to_string(), None),
            };

            serde_json::to_string(&RenderResponse {
                success: false,
                error: Some(error_msg),
                html: None,
                diagnostics,
                warnings: None,
            })
            .unwrap()
        }
    }
}

/// Render QMD content directly (without reading from VFS).
///
/// # Arguments
/// * `content` - QMD source text
/// * `_template_bundle` - Optional template bundle JSON (currently unused, reserved for future use)
///
/// # Returns
/// JSON: `{ "success": true, "html": "..." }` or `{ "success": false, "error": "...", "diagnostics": [...] }`
#[wasm_bindgen]
pub async fn render_qmd_content(content: &str, _template_bundle: &str) -> String {
    // Create a virtual path for this content
    let path = Path::new("/input.qmd");

    // Create minimal project context for WASM
    let project = create_wasm_project_context(path);
    let doc = DocumentInfo::from_path(path);
    let format = Format::html();
    let binaries = BinaryDependencies::new();

    let options = RenderOptions {
        verbose: false,
        execute: false,
        use_freeze: false,
        output_path: None,
    };

    let mut ctx = RenderContext::new(&project, &doc, &format, &binaries).with_options(options);

    // Use the unified async pipeline (same as CLI)
    // TODO: Support custom templates via template_bundle parameter
    let config = HtmlRenderConfig::default();

    // Create Arc runtime for the async pipeline
    let runtime_arc: Arc<dyn SystemRuntime> = Arc::new(WasmRuntime::new());

    let result = render_qmd_to_html(
        content.as_bytes(),
        "/input.qmd",
        &mut ctx,
        &config,
        runtime_arc,
    )
    .await;

    match result {
        Ok(output) => {
            // Populate VFS with artifacts so post-processor can resolve them.
            // This includes CSS at /.quarto/project-artifacts/styles.css.
            let runtime = get_runtime();
            for (_key, artifact) in ctx.artifacts.iter() {
                if let Some(path) = &artifact.path {
                    runtime.add_file(path, artifact.content.clone());
                }
            }

            // Convert warnings to structured JSON with line/column info
            let warnings = diagnostics_to_json(&output.warnings, &output.source_context);
            serde_json::to_string(&RenderResponse {
                success: true,
                error: None,
                html: Some(output.html),
                diagnostics: None,
                warnings: if warnings.is_empty() {
                    None
                } else {
                    Some(warnings)
                },
            })
            .unwrap()
        }
        Err(e) => {
            // Extract structured diagnostics from parse errors
            let (error_msg, diagnostics) = match &e {
                QuartoError::Parse(parse_error) => {
                    let diags =
                        diagnostics_to_json(&parse_error.diagnostics, &parse_error.source_context);
                    (e.to_string(), Some(diags))
                }
                _ => (e.to_string(), None),
            };

            serde_json::to_string(&RenderResponse {
                success: false,
                error: Some(error_msg),
                html: None,
                diagnostics,
                warnings: None,
            })
            .unwrap()
        }
    }
}

// ============================================================================
// RENDER OPTIONS API
// ============================================================================

/// Options for rendering QMD content.
///
/// These are parsed from JSON and used to configure the render pipeline.
#[derive(Deserialize, Default)]
struct WasmRenderOptions {
    /// Enable source location tracking in HTML output.
    ///
    /// When true, injects `format.html.source-location: full` into the config,
    /// which adds `data-loc` attributes to HTML elements for scroll sync.
    #[serde(default)]
    source_location: bool,
}

/// Render QMD content with options.
///
/// # Arguments
/// * `content` - QMD source text
/// * `template_bundle` - Template bundle JSON (currently unused, reserved for future use)
/// * `options_json` - Options JSON: `{"source_location": true}`
///
/// # Returns
/// JSON: `{ "success": true, "html": "..." }` or `{ "success": false, "error": "...", "diagnostics": [...] }`
#[wasm_bindgen]
pub async fn render_qmd_content_with_options(
    content: &str,
    _template_bundle: &str,
    options_json: &str,
) -> String {
    // Parse options, defaulting to empty if invalid
    let wasm_options: WasmRenderOptions = serde_json::from_str(options_json).unwrap_or_default();

    // Create a virtual path for this content
    let path = Path::new("/input.qmd");

    // Create project context, optionally with format config for source location tracking
    let project = if wasm_options.source_location {
        let format_config = ConfigValue::from_path(&["format", "html", "source-location"], "full");
        let project_config = ProjectConfig::with_format_config(format_config);
        let dir = path.parent().unwrap_or(Path::new("/")).to_path_buf();
        ProjectContext {
            dir: dir.clone(),
            config: Some(project_config),
            is_single_file: true,
            files: vec![DocumentInfo::from_path(path)],
            output_dir: dir,
        }
    } else {
        create_wasm_project_context(path)
    };

    let doc = DocumentInfo::from_path(path);
    let format = Format::html();
    let binaries = BinaryDependencies::new();

    let options = RenderOptions {
        verbose: false,
        execute: false,
        use_freeze: false,
        output_path: None,
    };

    let mut ctx = RenderContext::new(&project, &doc, &format, &binaries).with_options(options);

    // Use the unified async pipeline (same as CLI)
    let config = HtmlRenderConfig::default();

    // Create Arc runtime for the async pipeline
    let runtime_arc: Arc<dyn SystemRuntime> = Arc::new(WasmRuntime::new());

    let result = render_qmd_to_html(
        content.as_bytes(),
        "/input.qmd",
        &mut ctx,
        &config,
        runtime_arc,
    )
    .await;

    match result {
        Ok(output) => {
            // Populate VFS with artifacts so post-processor can resolve them.
            let runtime = get_runtime();
            for (_key, artifact) in ctx.artifacts.iter() {
                if let Some(path) = &artifact.path {
                    runtime.add_file(path, artifact.content.clone());
                }
            }

            // Convert warnings to structured JSON with line/column info
            let warnings = diagnostics_to_json(&output.warnings, &output.source_context);
            serde_json::to_string(&RenderResponse {
                success: true,
                error: None,
                html: Some(output.html),
                diagnostics: None,
                warnings: if warnings.is_empty() {
                    None
                } else {
                    Some(warnings)
                },
            })
            .unwrap()
        }
        Err(e) => {
            // Extract structured diagnostics from parse errors
            let (error_msg, diagnostics) = match &e {
                QuartoError::Parse(parse_error) => {
                    let diags =
                        diagnostics_to_json(&parse_error.diagnostics, &parse_error.source_context);
                    (e.to_string(), Some(diags))
                }
                _ => (e.to_string(), None),
            };

            serde_json::to_string(&RenderResponse {
                success: false,
                error: Some(error_msg),
                html: None,
                diagnostics,
                warnings: None,
            })
            .unwrap()
        }
    }
}

/// Get a built-in template as a JSON bundle.
///
/// # Arguments
/// * `name` - Template name ("html5" or "plain")
///
/// # Returns
/// Template bundle JSON or `{ "error": "..." }`
#[wasm_bindgen]
pub fn get_builtin_template(name: &str) -> String {
    pampa::wasm_entry_points::get_builtin_template_json(name)
}

// ============================================================================
// JAVASCRIPT EXECUTION TEST API
// ============================================================================
//
// These functions provide test entry points for validating the JS bridge.
// They exercise the WasmRuntime -> JS -> WasmRuntime data flow.
//
// This is the WASM side of the "Interstitial JS runtime validation test"
// (task k-ktjc). These functions can be called from JavaScript to verify
// the template rendering works correctly.

#[derive(Serialize)]
struct JsTestResponse {
    success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

impl JsTestResponse {
    fn ok(result: String) -> String {
        serde_json::to_string(&JsTestResponse {
            success: true,
            result: Some(result),
            error: None,
        })
        .unwrap()
    }

    fn error(msg: &str) -> String {
        serde_json::to_string(&JsTestResponse {
            success: false,
            result: None,
            error: Some(msg.to_string()),
        })
        .unwrap()
    }
}

/// Test simple template rendering via the JS bridge.
///
/// This is an interstitial test to validate the WASM -> JS -> WASM data flow
/// works correctly before implementing full EJS support.
///
/// # Arguments
/// * `template` - Template string with ${key} placeholders
/// * `data_json` - JSON string with key-value pairs
///
/// # Returns
/// JSON: `{ "success": true, "result": "..." }` or `{ "success": false, "error": "..." }`
///
/// # Example
/// ```javascript
/// const result = await test_js_simple_template("Hello, ${name}!", '{"name": "World"}');
/// // result: { "success": true, "result": "Hello, World!" }
/// ```
#[wasm_bindgen]
pub async fn test_js_simple_template(template: &str, data_json: &str) -> String {
    let runtime = get_runtime();

    // Check if JS is available
    if !runtime.js_available() {
        return JsTestResponse::error("JavaScript execution is not available");
    }

    // Parse the JSON data
    let data: serde_json::Value = match serde_json::from_str(data_json) {
        Ok(v) => v,
        Err(e) => return JsTestResponse::error(&format!("Invalid JSON: {}", e)),
    };

    // Call the JS template rendering
    match runtime.js_render_simple_template(template, &data).await {
        Ok(result) => JsTestResponse::ok(result),
        Err(e) => JsTestResponse::error(&format!("Template rendering failed: {}", e)),
    }
}

/// Test EJS template rendering via the JS bridge.
///
/// This tests the full EJS rendering capability through the JS bridge.
///
/// # Arguments
/// * `template` - EJS template string
/// * `data_json` - JSON string with template data
///
/// # Returns
/// JSON: `{ "success": true, "result": "..." }` or `{ "success": false, "error": "..." }`
///
/// # Example
/// ```javascript
/// const result = await test_js_ejs("<%= name %>", '{"name": "World"}');
/// // result: { "success": true, "result": "World" }
/// ```
#[wasm_bindgen]
pub async fn test_js_ejs(template: &str, data_json: &str) -> String {
    let runtime = get_runtime();

    // Check if JS is available
    if !runtime.js_available() {
        return JsTestResponse::error("JavaScript execution is not available");
    }

    // Parse the JSON data
    let data: serde_json::Value = match serde_json::from_str(data_json) {
        Ok(v) => v,
        Err(e) => return JsTestResponse::error(&format!("Invalid JSON: {}", e)),
    };

    // Call the EJS rendering
    match runtime.render_ejs(template, &data).await {
        Ok(result) => JsTestResponse::ok(result),
        Err(e) => JsTestResponse::error(&format!("EJS rendering failed: {}", e)),
    }
}

/// Check if JavaScript execution is available in the WASM runtime.
///
/// # Returns
/// `true` if JS is available, `false` otherwise
#[wasm_bindgen]
pub fn test_js_available() -> bool {
    get_runtime().js_available()
}
