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
use std::sync::OnceLock;

use quarto_core::{
    render_qmd_to_html, BinaryDependencies, DocumentInfo, Format, HtmlRenderConfig, ProjectContext,
    RenderContext, RenderOptions,
};
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

/// Read a file from the virtual filesystem.
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
    #[serde(skip_serializing_if = "Option::is_none")]
    diagnostics: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    warnings: Option<Vec<String>>,
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
pub fn render_qmd(path: &str) -> String {
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

    // Use the unified pipeline (same as CLI)
    let config = HtmlRenderConfig::default();
    let source_name = path.to_string_lossy();

    match render_qmd_to_html(&content, &source_name, &mut ctx, &config) {
        Ok(output) => {
            // Populate VFS with artifacts so post-processor can resolve them.
            // This includes CSS at /.quarto/project-artifacts/styles.css.
            for (_key, artifact) in ctx.artifacts.iter() {
                if let Some(artifact_path) = &artifact.path {
                    runtime.add_file(artifact_path, artifact.content.clone());
                }
            }

            let warnings: Vec<String> = output.warnings.iter().map(|w| w.message.clone()).collect();
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
        Err(e) => serde_json::to_string(&RenderResponse {
            success: false,
            error: Some(e.to_string()),
            html: None,
            diagnostics: None,
            warnings: None,
        })
        .unwrap(),
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
pub fn render_qmd_content(content: &str, _template_bundle: &str) -> String {
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

    // Use the unified pipeline (same as CLI)
    // TODO: Support custom templates via template_bundle parameter
    let config = HtmlRenderConfig::default();

    match render_qmd_to_html(content.as_bytes(), "/input.qmd", &mut ctx, &config) {
        Ok(output) => {
            // Populate VFS with artifacts so post-processor can resolve them.
            // This includes CSS at /.quarto/project-artifacts/styles.css.
            let runtime = get_runtime();
            for (_key, artifact) in ctx.artifacts.iter() {
                if let Some(path) = &artifact.path {
                    runtime.add_file(path, artifact.content.clone());
                }
            }

            let warnings: Vec<String> = output.warnings.iter().map(|w| w.message.clone()).collect();
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
        Err(e) => serde_json::to_string(&RenderResponse {
            success: false,
            error: Some(e.to_string()),
            html: None,
            diagnostics: None,
            warnings: None,
        })
        .unwrap(),
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
