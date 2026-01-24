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
use quarto_sass::{
    BOOTSTRAP_RESOURCES, RESOURCE_PATH_PREFIX, ThemeConfig, ThemeContext, compile_theme_css,
};
use quarto_source_map::SourceContext;
use quarto_system_runtime::{SystemRuntime, WasmRuntime};
use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;

// Global runtime instance for VFS operations
static RUNTIME: OnceLock<WasmRuntime> = OnceLock::new();

fn get_runtime() -> &'static WasmRuntime {
    RUNTIME.get_or_init(|| {
        let runtime = WasmRuntime::new();
        // Populate VFS with embedded Bootstrap SCSS resources
        populate_vfs_with_embedded_resources(&runtime);
        runtime
    })
}

/// Populate the VFS with embedded Bootstrap SCSS resources.
///
/// This makes Bootstrap 5.3.1 SCSS files available in the VFS under
/// `/__quarto_resources__/bootstrap/scss/` for SASS compilation.
fn populate_vfs_with_embedded_resources(runtime: &WasmRuntime) {
    let prefix = format!("{}/bootstrap/scss", RESOURCE_PATH_PREFIX);

    for file_path in BOOTSTRAP_RESOURCES.file_paths() {
        let vfs_path = format!("{}/{}", prefix, file_path);
        if let Some(content) = BOOTSTRAP_RESOURCES.read(Path::new(file_path)) {
            runtime.add_file(Path::new(&vfs_path), content.to_vec());
        }
    }
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

// ============================================================================
// PROJECT CREATION API
// ============================================================================
//
// These functions provide the WASM entry points for creating new Quarto projects.
// They use the quarto-project-create crate which renders EJS templates via the
// JS bridge.

use quarto_project_create::{
    CreateFromChoiceOptions, ScaffoldedFile, create_project_from_choice, implemented_choices,
};

/// A project choice for JSON serialization.
#[derive(Serialize)]
struct JsonProjectChoice {
    /// Unique identifier (e.g., "website", "blog")
    id: String,
    /// Display name (e.g., "Website", "Blog")
    name: String,
    /// Short description
    description: String,
}

/// Response for get_project_choices().
#[derive(Serialize)]
struct ProjectChoicesResponse {
    success: bool,
    choices: Vec<JsonProjectChoice>,
}

/// A project file for JSON serialization.
#[derive(Serialize)]
struct JsonProjectFile {
    /// Relative path within the project
    path: String,
    /// Content type: "text" or "binary"
    content_type: String,
    /// File content (string for text, base64 for binary)
    content: String,
    /// MIME type (only for binary files)
    #[serde(skip_serializing_if = "Option::is_none")]
    mime_type: Option<String>,
}

/// Response for create_project().
#[derive(Serialize)]
struct CreateProjectResponse {
    success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    files: Option<Vec<JsonProjectFile>>,
}

impl CreateProjectResponse {
    fn error(msg: &str) -> String {
        serde_json::to_string(&CreateProjectResponse {
            success: false,
            error: Some(msg.to_string()),
            files: None,
        })
        .unwrap()
    }

    fn ok(files: Vec<JsonProjectFile>) -> String {
        serde_json::to_string(&CreateProjectResponse {
            success: true,
            error: None,
            files: Some(files),
        })
        .unwrap()
    }
}

/// Get available project choices for the Create Project UI.
///
/// Returns a list of project types that can be created. Each choice has
/// an id, display name, and description suitable for showing in a dropdown
/// or selection list.
///
/// # Returns
/// JSON: `{ "success": true, "choices": [{ "id": "website", "name": "Website", "description": "..." }, ...] }`
///
/// # Example
/// ```javascript
/// const response = JSON.parse(get_project_choices());
/// // Show choices in a dropdown
/// response.choices.forEach(choice => {
///     dropdown.addOption(choice.id, choice.name);
/// });
/// ```
#[wasm_bindgen]
pub fn get_project_choices() -> String {
    let choices: Vec<JsonProjectChoice> = implemented_choices()
        .into_iter()
        .map(|c| JsonProjectChoice {
            id: c.id,
            name: c.name,
            description: c.description,
        })
        .collect();

    serde_json::to_string(&ProjectChoicesResponse {
        success: true,
        choices,
    })
    .unwrap()
}

/// Create a new Quarto project.
///
/// Creates a project scaffold based on the selected choice and title.
/// Returns a list of files with their paths and contents.
///
/// For text files, content is returned as a UTF-8 string.
/// For binary files, content is returned as a base64-encoded string.
///
/// # Arguments
/// * `choice_id` - The project choice ID (from get_project_choices)
/// * `title` - The project title (used in _quarto.yml and document titles)
///
/// # Returns
/// JSON: `{ "success": true, "files": [...] }` or `{ "success": false, "error": "..." }`
///
/// # Example
/// ```javascript
/// const response = JSON.parse(await create_project("website", "My Website"));
/// if (response.success) {
///     for (const file of response.files) {
///         if (file.content_type === "text") {
///             await createTextDocument(file.path, file.content);
///         } else {
///             const bytes = base64ToUint8Array(file.content);
///             await createBinaryDocument(file.path, bytes, file.mime_type);
///         }
///     }
/// }
/// ```
#[wasm_bindgen]
pub async fn create_project(choice_id: &str, title: &str) -> String {
    use base64::Engine;

    let runtime = get_runtime();

    // Check if JS is available (required for EJS template rendering)
    if !runtime.js_available() {
        return CreateProjectResponse::error(
            "JavaScript execution is not available for template rendering",
        );
    }

    // Create project options
    let options = CreateFromChoiceOptions::new(choice_id, title);

    // Create the project
    match create_project_from_choice(runtime, options).await {
        Ok(files) => {
            let json_files: Vec<JsonProjectFile> = files
                .into_iter()
                .map(|f| match f {
                    ScaffoldedFile::Text { path, content } => JsonProjectFile {
                        path: path.to_string_lossy().to_string(),
                        content_type: "text".to_string(),
                        content,
                        mime_type: None,
                    },
                    ScaffoldedFile::Binary {
                        path,
                        content,
                        mime_type,
                    } => JsonProjectFile {
                        path: path.to_string_lossy().to_string(),
                        content_type: "binary".to_string(),
                        content: base64::engine::general_purpose::STANDARD.encode(&content),
                        mime_type: Some(mime_type),
                    },
                })
                .collect();

            CreateProjectResponse::ok(json_files)
        }
        Err(e) => CreateProjectResponse::error(&e.to_string()),
    }
}

// ============================================================================
// LSP INTELLIGENCE API
// ============================================================================
//
// These functions provide the WASM entry points for language intelligence
// features (document symbols, diagnostics, folding ranges).
//
// They use quarto-lsp-core which is transport-agnostic and compiles to both
// native and WASM targets.

use quarto_lsp_core::{Document, DocumentAnalysisJson, analyze_document};

/// Response for LSP analyze_document().
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct LspAnalyzeResponse {
    success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    symbols: Option<Vec<quarto_lsp_core::Symbol>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    folding_ranges: Option<Vec<quarto_lsp_core::FoldingRange>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    diagnostics: Option<Vec<quarto_lsp_core::Diagnostic>>,
}

impl LspAnalyzeResponse {
    fn ok(analysis: DocumentAnalysisJson) -> String {
        serde_json::to_string(&LspAnalyzeResponse {
            success: true,
            error: None,
            symbols: Some(analysis.symbols),
            folding_ranges: Some(analysis.folding_ranges),
            diagnostics: Some(analysis.diagnostics),
        })
        .unwrap()
    }

    fn error(msg: &str) -> String {
        serde_json::to_string(&LspAnalyzeResponse {
            success: false,
            error: Some(msg.to_string()),
            symbols: None,
            folding_ranges: None,
            diagnostics: None,
        })
        .unwrap()
    }
}

/// Response for LSP get_symbols().
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct LspSymbolsResponse {
    success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    symbols: Option<Vec<quarto_lsp_core::Symbol>>,
}

impl LspSymbolsResponse {
    fn ok(symbols: Vec<quarto_lsp_core::Symbol>) -> String {
        serde_json::to_string(&LspSymbolsResponse {
            success: true,
            error: None,
            symbols: Some(symbols),
        })
        .unwrap()
    }

    fn error(msg: &str) -> String {
        serde_json::to_string(&LspSymbolsResponse {
            success: false,
            error: Some(msg.to_string()),
            symbols: None,
        })
        .unwrap()
    }
}

/// Response for LSP get_folding_ranges().
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct LspFoldingRangesResponse {
    success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    folding_ranges: Option<Vec<quarto_lsp_core::FoldingRange>>,
}

impl LspFoldingRangesResponse {
    fn ok(ranges: Vec<quarto_lsp_core::FoldingRange>) -> String {
        serde_json::to_string(&LspFoldingRangesResponse {
            success: true,
            error: None,
            folding_ranges: Some(ranges),
        })
        .unwrap()
    }

    fn error(msg: &str) -> String {
        serde_json::to_string(&LspFoldingRangesResponse {
            success: false,
            error: Some(msg.to_string()),
            folding_ranges: None,
        })
        .unwrap()
    }
}

/// Response for LSP get_diagnostics().
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct LspDiagnosticsResponse {
    success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    diagnostics: Option<Vec<quarto_lsp_core::Diagnostic>>,
}

impl LspDiagnosticsResponse {
    fn ok(diagnostics: Vec<quarto_lsp_core::Diagnostic>) -> String {
        serde_json::to_string(&LspDiagnosticsResponse {
            success: true,
            error: None,
            diagnostics: Some(diagnostics),
        })
        .unwrap()
    }

    fn error(msg: &str) -> String {
        serde_json::to_string(&LspDiagnosticsResponse {
            success: false,
            error: Some(msg.to_string()),
            diagnostics: None,
        })
        .unwrap()
    }
}

/// Analyze a document in the VFS, returning all intelligence data.
///
/// This is the primary entry point for hub-client intelligence.
/// Performs a single parse and extracts symbols, folding ranges, and diagnostics.
///
/// # Arguments
/// * `path` - Path to the file in VFS (e.g., "index.qmd")
///
/// # Returns
/// JSON: `{ "success": true, "symbols": [...], "foldingRanges": [...], "diagnostics": [...] }`
/// or `{ "success": false, "error": "..." }`
///
/// # Example
/// ```javascript
/// const result = JSON.parse(lsp_analyze_document("index.qmd"));
/// if (result.success) {
///     console.log("Symbols:", result.symbols);
///     console.log("Folding ranges:", result.foldingRanges);
///     console.log("Diagnostics:", result.diagnostics);
/// }
/// ```
#[wasm_bindgen]
pub fn lsp_analyze_document(path: &str) -> String {
    let runtime = get_runtime();
    let file_path = Path::new(path);

    // Read the file from VFS
    let content = match runtime.file_read(file_path) {
        Ok(bytes) => match String::from_utf8(bytes) {
            Ok(text) => text,
            Err(_) => return LspAnalyzeResponse::error("File is not valid UTF-8"),
        },
        Err(e) => return LspAnalyzeResponse::error(&format!("Failed to read file: {}", e)),
    };

    // Create document and analyze
    let doc = Document::new(path, &content);
    let analysis = analyze_document(&doc);

    // Convert to JSON-serializable format
    let json_analysis: DocumentAnalysisJson = analysis.into();
    LspAnalyzeResponse::ok(json_analysis)
}

/// Get document symbols for a file in the VFS.
///
/// Convenience wrapper around lsp_analyze_document() for callers
/// who only need symbols.
///
/// # Arguments
/// * `path` - Path to the file in VFS (e.g., "index.qmd")
///
/// # Returns
/// JSON: `{ "success": true, "symbols": [...] }` or `{ "success": false, "error": "..." }`
///
/// # Example
/// ```javascript
/// const result = JSON.parse(lsp_get_symbols("index.qmd"));
/// if (result.success) {
///     for (const symbol of result.symbols) {
///         console.log(symbol.name, symbol.kind);
///     }
/// }
/// ```
#[wasm_bindgen]
pub fn lsp_get_symbols(path: &str) -> String {
    let runtime = get_runtime();
    let file_path = Path::new(path);

    // Read the file from VFS
    let content = match runtime.file_read(file_path) {
        Ok(bytes) => match String::from_utf8(bytes) {
            Ok(text) => text,
            Err(_) => return LspSymbolsResponse::error("File is not valid UTF-8"),
        },
        Err(e) => return LspSymbolsResponse::error(&format!("Failed to read file: {}", e)),
    };

    // Create document and analyze
    let doc = Document::new(path, &content);
    let analysis = analyze_document(&doc);

    LspSymbolsResponse::ok(analysis.symbols)
}

/// Get folding ranges for a file in the VFS.
///
/// Folding ranges include:
/// - YAML frontmatter (`---` to `---`)
/// - Code cells (` ```{lang}` to ` ``` `)
/// - Sections (header to next same-level-or-higher header)
///
/// # Arguments
/// * `path` - Path to the file in VFS (e.g., "index.qmd")
///
/// # Returns
/// JSON: `{ "success": true, "foldingRanges": [...] }` or `{ "success": false, "error": "..." }`
///
/// # Example
/// ```javascript
/// const result = JSON.parse(lsp_get_folding_ranges("index.qmd"));
/// if (result.success) {
///     for (const range of result.foldingRanges) {
///         console.log(`Fold: line ${range.startLine} to ${range.endLine}`);
///     }
/// }
/// ```
#[wasm_bindgen]
pub fn lsp_get_folding_ranges(path: &str) -> String {
    let runtime = get_runtime();
    let file_path = Path::new(path);

    // Read the file from VFS
    let content = match runtime.file_read(file_path) {
        Ok(bytes) => match String::from_utf8(bytes) {
            Ok(text) => text,
            Err(_) => return LspFoldingRangesResponse::error("File is not valid UTF-8"),
        },
        Err(e) => return LspFoldingRangesResponse::error(&format!("Failed to read file: {}", e)),
    };

    // Create document and analyze
    let doc = Document::new(path, &content);
    let analysis = analyze_document(&doc);

    LspFoldingRangesResponse::ok(analysis.folding_ranges)
}

/// Get diagnostics for a file in the VFS.
///
/// Returns rich diagnostics matching quarto-error-reporting::DiagnosticMessage
/// structure, including title, problem, hints, and details.
///
/// # Arguments
/// * `path` - Path to the file in VFS (e.g., "index.qmd")
///
/// # Returns
/// JSON: `{ "success": true, "diagnostics": [...] }` or `{ "success": false, "error": "..." }`
///
/// # Example
/// ```javascript
/// const result = JSON.parse(lsp_get_diagnostics("index.qmd"));
/// if (result.success) {
///     for (const diag of result.diagnostics) {
///         console.log(`${diag.severity}: ${diag.title}`);
///         if (diag.problem) console.log(`  Problem: ${diag.problem.content}`);
///         for (const hint of diag.hints) {
///             console.log(`  Hint: ${hint.content}`);
///         }
///     }
/// }
/// ```
#[wasm_bindgen]
pub fn lsp_get_diagnostics(path: &str) -> String {
    let runtime = get_runtime();
    let file_path = Path::new(path);

    // Read the file from VFS
    let content = match runtime.file_read(file_path) {
        Ok(bytes) => match String::from_utf8(bytes) {
            Ok(text) => text,
            Err(_) => return LspDiagnosticsResponse::error("File is not valid UTF-8"),
        },
        Err(e) => return LspDiagnosticsResponse::error(&format!("Failed to read file: {}", e)),
    };

    // Create document and analyze
    let doc = Document::new(path, &content);
    let analysis = analyze_document(&doc);

    LspDiagnosticsResponse::ok(analysis.diagnostics)
}

// ============================================================================
// SASS COMPILATION API
// ============================================================================
//
// These functions provide direct access to SASS compilation for use with
// JavaScript-side caching. The hub-client can call these functions and
// implement LRU caching in IndexedDB around the compilation results.

/// Response for SASS compilation.
#[derive(Serialize)]
struct SassCompileResponse {
    success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    css: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

impl SassCompileResponse {
    fn ok(css: String) -> String {
        serde_json::to_string(&SassCompileResponse {
            success: true,
            css: Some(css),
            error: None,
        })
        .unwrap()
    }

    fn error(msg: &str) -> String {
        serde_json::to_string(&SassCompileResponse {
            success: false,
            css: None,
            error: Some(msg.to_string()),
        })
        .unwrap()
    }
}

/// Check if SASS compilation is available.
///
/// # Returns
/// `true` if SASS compilation is available, `false` otherwise.
#[wasm_bindgen]
pub fn sass_available() -> bool {
    get_runtime().sass_available()
}

/// Get the name of the SASS compiler being used.
///
/// # Returns
/// The compiler name (e.g., "dart-sass") or null if not available.
#[wasm_bindgen]
pub fn sass_compiler_name() -> Option<String> {
    get_runtime().sass_compiler_name().map(|s| s.to_string())
}

/// Compile SCSS to CSS.
///
/// This function compiles SCSS source code to CSS using dart-sass (via the JS bridge).
/// The result can be cached by the JavaScript caller to avoid recompilation.
///
/// # Arguments
/// * `scss` - The SCSS source code to compile
/// * `minified` - Whether to produce minified output
/// * `load_paths_json` - JSON array of additional load paths for @use/@import resolution
///
/// # Returns
/// JSON: `{ "success": true, "css": "..." }` or `{ "success": false, "error": "..." }`
///
/// # Example
/// ```javascript
/// const result = JSON.parse(await compile_scss(
///     "$primary: blue; .btn { color: $primary; }",
///     false,
///     '["/__quarto_resources__/bootstrap/scss"]'
/// ));
/// if (result.success) {
///     console.log("Compiled CSS:", result.css);
/// } else {
///     console.error("Compilation failed:", result.error);
/// }
/// ```
#[wasm_bindgen]
pub async fn compile_scss(scss: &str, minified: bool, load_paths_json: &str) -> String {
    use std::path::PathBuf;

    let runtime = get_runtime();

    // Check if SASS is available
    if !runtime.sass_available() {
        return SassCompileResponse::error("SASS compilation is not available");
    }

    // Parse load paths from JSON
    let load_paths: Vec<PathBuf> = match serde_json::from_str::<Vec<String>>(load_paths_json) {
        Ok(paths) => paths.into_iter().map(PathBuf::from).collect(),
        Err(e) => {
            return SassCompileResponse::error(&format!("Invalid load_paths JSON: {}", e));
        }
    };

    // Compile SCSS
    match runtime.compile_sass(scss, &load_paths, minified).await {
        Ok(css) => SassCompileResponse::ok(css),
        Err(e) => SassCompileResponse::error(&format!("{}", e)),
    }
}

/// Compile SCSS with default Bootstrap load paths.
///
/// Convenience function that automatically includes the embedded Bootstrap SCSS
/// in the load paths. Use this when compiling SCSS that depends on Bootstrap.
///
/// # Arguments
/// * `scss` - The SCSS source code to compile
/// * `minified` - Whether to produce minified output
///
/// # Returns
/// JSON: `{ "success": true, "css": "..." }` or `{ "success": false, "error": "..." }`
#[wasm_bindgen]
pub async fn compile_scss_with_bootstrap(scss: &str, minified: bool) -> String {
    // Default load path includes embedded Bootstrap SCSS
    let load_paths = format!("[\"{}/bootstrap/scss\"]", RESOURCE_PATH_PREFIX);
    compile_scss(scss, minified, &load_paths).await
}

/// Compile CSS for a document's theme configuration.
///
/// Extracts the theme from the document's YAML frontmatter and compiles
/// the appropriate CSS. Supports:
/// - Single theme: `theme: cosmo`
/// - Multiple themes: `theme: [cosmo, custom.scss]`
/// - No theme: uses default Bootstrap
///
/// # Arguments
/// * `content` - The QMD document content (must include YAML frontmatter)
///
/// # Returns
/// JSON: `{ "success": true, "css": "..." }` or `{ "success": false, "error": "..." }`
///
/// # Example
/// ```javascript
/// const qmd = `---
/// title: My Document
/// format:
///   html:
///     theme: cosmo
/// ---
///
/// # Hello World
/// `;
/// const result = JSON.parse(await compile_document_css(qmd));
/// if (result.success) {
///     document.querySelector('style').textContent = result.css;
/// }
/// ```
#[wasm_bindgen]
pub async fn compile_document_css(content: &str) -> String {
    let runtime = get_runtime();

    // Check if SASS is available
    if !runtime.sass_available() {
        return SassCompileResponse::error("SASS compilation is not available");
    }

    // Extract YAML frontmatter and parse it
    let config = match extract_frontmatter_config(content) {
        Ok(config) => config,
        Err(e) => return SassCompileResponse::error(&e),
    };

    // Extract theme configuration
    let theme_config = match ThemeConfig::from_config_value(&config) {
        Ok(config) => config,
        Err(e) => return SassCompileResponse::error(&format!("Invalid theme config: {}", e)),
    };

    // Create theme context (using root as document dir since VFS is flat)
    let context = ThemeContext::new(std::path::PathBuf::from("/"), runtime);

    // Compile CSS
    match compile_theme_css(&theme_config, &context).await {
        Ok(css) => SassCompileResponse::ok(css),
        Err(e) => SassCompileResponse::error(&format!("SASS compilation failed: {}", e)),
    }
}

/// Compile CSS for a specific theme name.
///
/// Convenience function for compiling a single built-in Bootswatch theme.
/// Use this when you know the exact theme name (e.g., from a UI selection).
///
/// # Arguments
/// * `theme_name` - The Bootswatch theme name (e.g., "cosmo", "darkly", "flatly")
/// * `minified` - Whether to produce minified output
///
/// # Returns
/// JSON: `{ "success": true, "css": "..." }` or `{ "success": false, "error": "..." }`
///
/// # Example
/// ```javascript
/// const result = JSON.parse(await compile_theme_css_by_name("cosmo", true));
/// if (result.success) {
///     applyThemeCSS(result.css);
/// }
/// ```
#[wasm_bindgen]
pub async fn compile_theme_css_by_name(theme_name: &str, minified: bool) -> String {
    use quarto_sass::themes::ThemeSpec;

    let runtime = get_runtime();

    // Check if SASS is available
    if !runtime.sass_available() {
        return SassCompileResponse::error("SASS compilation is not available");
    }

    // Parse theme spec
    let theme_spec = match ThemeSpec::parse(theme_name) {
        Ok(spec) => spec,
        Err(e) => return SassCompileResponse::error(&format!("Invalid theme: {}", e)),
    };

    // Create theme config
    let theme_config = ThemeConfig::new(vec![theme_spec], minified);

    // Create theme context
    let context = ThemeContext::new(std::path::PathBuf::from("/"), runtime);

    // Compile CSS
    match compile_theme_css(&theme_config, &context).await {
        Ok(css) => SassCompileResponse::ok(css),
        Err(e) => SassCompileResponse::error(&format!("SASS compilation failed: {}", e)),
    }
}

/// Compile default Bootstrap CSS (no theme customization).
///
/// Use this when you need basic Bootstrap styling without any Bootswatch theme.
/// This is what gets used when no theme is specified in the document.
///
/// # Arguments
/// * `minified` - Whether to produce minified output
///
/// # Returns
/// JSON: `{ "success": true, "css": "..." }` or `{ "success": false, "error": "..." }`
///
/// # Example
/// ```javascript
/// const result = JSON.parse(await compile_default_bootstrap_css(true));
/// if (result.success) {
///     applyThemeCSS(result.css);
/// }
/// ```
#[wasm_bindgen]
pub async fn compile_default_bootstrap_css(minified: bool) -> String {
    let runtime = get_runtime();

    // Check if SASS is available
    if !runtime.sass_available() {
        return SassCompileResponse::error("SASS compilation is not available");
    }

    // Create default theme config (no themes, just Bootstrap)
    let theme_config = ThemeConfig::default_bootstrap();
    let theme_config = ThemeConfig::new(theme_config.themes, minified);

    // Create theme context
    let context = ThemeContext::new(std::path::PathBuf::from("/"), runtime);

    // Compile CSS
    match compile_theme_css(&theme_config, &context).await {
        Ok(css) => SassCompileResponse::ok(css),
        Err(e) => SassCompileResponse::error(&format!("SASS compilation failed: {}", e)),
    }
}

// =============================================================================
// Helper Functions for Theme Compilation
// =============================================================================

/// Extract YAML frontmatter from QMD content and parse to ConfigValue.
///
/// Looks for content between `---` markers at the start of the document.
fn extract_frontmatter_config(content: &str) -> Result<ConfigValue, String> {
    use quarto_pandoc_types::{ConfigValueKind, MergeOp};
    use quarto_source_map::SourceInfo;

    // Find YAML frontmatter boundaries
    let trimmed = content.trim_start();
    if !trimmed.starts_with("---") {
        // No frontmatter - return empty config
        return Ok(ConfigValue {
            value: ConfigValueKind::Map(vec![]),
            source_info: SourceInfo::default(),
            merge_op: MergeOp::default(),
        });
    }

    // Find the closing `---`
    let after_first = &trimmed[3..];
    let end_pos = after_first
        .find("\n---")
        .ok_or_else(|| "Unclosed YAML frontmatter".to_string())?;

    // Extract YAML content
    let yaml_str = &after_first[..end_pos].trim();

    // Parse YAML to serde_json::Value first
    let yaml_value: serde_json::Value = serde_yaml::from_str(yaml_str)
        .map_err(|e| format!("Failed to parse YAML frontmatter: {}", e))?;

    // Convert to ConfigValue
    Ok(json_to_config_value(&yaml_value))
}

/// Convert a serde_json::Value to ConfigValue.
///
/// This is a simplified conversion that preserves the structure needed
/// for theme configuration extraction.
fn json_to_config_value(value: &serde_json::Value) -> ConfigValue {
    use quarto_pandoc_types::{ConfigMapEntry, ConfigValueKind, MergeOp};
    use quarto_source_map::SourceInfo;
    use yaml_rust2::Yaml;

    let kind = match value {
        serde_json::Value::Null => ConfigValueKind::Scalar(Yaml::Null),
        serde_json::Value::Bool(b) => ConfigValueKind::Scalar(Yaml::Boolean(*b)),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                ConfigValueKind::Scalar(Yaml::Integer(i))
            } else if let Some(f) = n.as_f64() {
                ConfigValueKind::Scalar(Yaml::Real(f.to_string()))
            } else {
                ConfigValueKind::Scalar(Yaml::String(n.to_string()))
            }
        }
        serde_json::Value::String(s) => ConfigValueKind::Scalar(Yaml::String(s.clone())),
        serde_json::Value::Array(arr) => {
            let items: Vec<ConfigValue> = arr.iter().map(json_to_config_value).collect();
            ConfigValueKind::Array(items)
        }
        serde_json::Value::Object(obj) => {
            let entries: Vec<ConfigMapEntry> = obj
                .iter()
                .map(|(k, v)| ConfigMapEntry {
                    key: k.clone(),
                    key_source: SourceInfo::default(),
                    value: json_to_config_value(v),
                })
                .collect();
            ConfigValueKind::Map(entries)
        }
    };

    ConfigValue {
        value: kind,
        source_info: SourceInfo::default(),
        merge_op: MergeOp::default(),
    }
}
