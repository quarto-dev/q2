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

/// Sentinel panic payload raised by `c_shim::rust_lua_throw`.
///
/// On wasm32 Lua's `LUAI_THROW` macro cannot use `setjmp`/`longjmp`, so
/// it is rewired to raise a Rust panic that `rust_lua_protected_call`
/// catches via `catch_unwind`. This happens on every Lua runtime error —
/// including ones caught by `pcall` — so the panic is expected control
/// flow. The `init()` panic hook filters panics carrying this payload
/// so they do not spam `console.error` with stack traces.
pub struct LuaThrow;

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;
use std::sync::{Arc, OnceLock};

use quarto_core::{
    BinaryDependencies, DocumentInfo, Format, HtmlRenderConfig, ProjectConfig, ProjectContext,
    QuartoError, RenderContext, RenderOptions, ResourceResolverContext, render_qmd_to_html,
    render_qmd_to_preview_ast,
};
use quarto_error_reporting::{
    DiagnosticMessage, JsonDiagnostic, JsonPass1Failure, diagnostic_to_json, with_source_file,
};
use quarto_pandoc_types::ConfigValue;
use quarto_sass::{
    BOOTSTRAP_RESOURCES, RESOURCE_PATH_PREFIX, ThemeConfig, ThemeContext, compile_theme_css,
};
use quarto_source_map::SourceContext;
use quarto_system_runtime::{SystemRuntime, WasmRuntime};
use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;

// Global runtime instance for VFS operations.
// Stored as Arc so it can be shared with the rendering pipeline.
static RUNTIME: OnceLock<Arc<WasmRuntime>> = OnceLock::new();

/// Get a reference to the global VFS runtime for direct method calls.
fn get_runtime() -> &'static WasmRuntime {
    get_runtime_arc()
}

/// Get a clone of the global VFS runtime as `Arc<dyn SystemRuntime>`
/// for passing into the rendering pipeline.
fn get_runtime_arc() -> &'static Arc<WasmRuntime> {
    RUNTIME.get_or_init(|| {
        let runtime = WasmRuntime::new();
        // Populate VFS with embedded Bootstrap SCSS resources
        populate_vfs_with_embedded_resources(&runtime);
        Arc::new(runtime)
    })
}

/// Populate the VFS with embedded resources.
///
/// This makes the following available in the VFS:
/// - Bootstrap 5.3.1 SCSS files under `/__quarto_resources__/bootstrap/scss/`
/// - Built-in extensions under `/__quarto_resources__/extensions/`
fn populate_vfs_with_embedded_resources(runtime: &WasmRuntime) {
    // Bootstrap SCSS resources
    let prefix = format!("{}/bootstrap/scss", RESOURCE_PATH_PREFIX);
    for file_path in BOOTSTRAP_RESOURCES.file_paths() {
        let vfs_path = format!("{}/{}", prefix, file_path);
        if let Some(content) = BOOTSTRAP_RESOURCES.read(Path::new(file_path)) {
            runtime.add_file(Path::new(&vfs_path), content.to_vec());
        }
    }

    // Built-in extensions
    populate_builtin_extensions(runtime);
}

/// Populate the VFS with built-in extensions from the embedded directory.
fn populate_builtin_extensions(runtime: &WasmRuntime) {
    use include_dir::{Dir, include_dir};

    static EXTENSIONS_DIR: Dir = include_dir!("$CARGO_MANIFEST_DIR/../../resources/extensions");

    let prefix = format!("{}/extensions", RESOURCE_PATH_PREFIX);
    populate_dir_recursive(runtime, &EXTENSIONS_DIR, &prefix);
}

/// Recursively add all files from an embedded directory to the VFS.
fn populate_dir_recursive(runtime: &WasmRuntime, dir: &include_dir::Dir<'_>, prefix: &str) {
    for file in dir.files() {
        let vfs_path = format!("{}/{}", prefix, file.path().display());
        runtime.add_file(Path::new(&vfs_path), file.contents().to_vec());
    }
    for subdir in dir.dirs() {
        populate_dir_recursive(runtime, subdir, prefix);
    }
}

#[wasm_bindgen(start)]
pub fn init() {
    // Install console_error_panic_hook as the base, then wrap it to
    // filter out expected Lua control-flow panics (see `LuaThrow` above).
    // Without this wrapper, every pcall-caught Lua error would leave a
    // full panic stack trace in console.error.
    //
    // See claude-notes/plans/2026-04-16-suppress-lua-panic-noise.md
    console_error_panic_hook::set_once();
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if info.payload().downcast_ref::<LuaThrow>().is_some() {
            return;
        }
        default_hook(info);
    }));
}

/// Basic unwind test — no Lua, just catch_unwind.
#[wasm_bindgen]
pub fn test_unwind() -> String {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        panic!("test panic");
    })) {
        Ok(()) => "no panic (unexpected)".to_string(),
        Err(_) => "caught panic successfully".to_string(),
    }
}

/// Test function: create a Lua VM and run a simple script.
/// Returns the result string or an error message.
#[wasm_bindgen]
pub fn test_lua(script: &str) -> String {
    pampa::lua_wasm_test(script)
}

/// Test entry point: validates that mlua's `async` feature works on wasm32.
/// Returns "ok:async_result" on success or "error:<msg>" on failure.
#[wasm_bindgen]
pub async fn test_lua_async() -> String {
    pampa::lua_wasm_async_test().await
}

/// Test entry point for Phase 3 of syntax highlighting — consumed by
/// `hub-client/tests/wasm-highlight.vitest.ts`. Not used in production.
///
/// Calls through to `quarto_highlight::highlight()`, which is the same
/// `Registry::global().highlight()` → `tree_sitter_highlight::Highlighter`
/// path the native CLI uses. The return value is the JSON triple-array
/// encoding that ends up in the `data-hl-spans` attribute, so if this
/// matches the native golden output for the same `(class, source)`
/// input, the native and WASM highlight paths are equivalent.
///
/// Returns:
///   - `Ok(Some(json))` when the class resolves to a built-in grammar
///     and highlighting succeeds;
///   - `Ok(None)` when the class is not registered (matches native
///     fall-through behavior);
///   - `Err(msg)` if the underlying Highlighter errored — propagated
///     back to JS as a thrown exception via wasm-bindgen.
#[wasm_bindgen]
pub fn quarto_highlight_for_test(
    language_class: &str,
    source: &str,
) -> Result<Option<String>, JsValue> {
    quarto_highlight::highlight(language_class, source)
        .map_err(|e| JsValue::from_str(&e.to_string()))
}

// ============================================================================
// USER-GRAMMAR BRIDGE (Phase 4.3)
// ============================================================================

/// JS-interop user-grammar provider. JS constructs one of these via
/// `new JsUserGrammars()`, registers highlight callbacks per language
/// class via `register(class, fn)`, and hands the handle to
/// `render_qmd` (or the test-only `quarto_highlight_with_user_for_test`).
///
/// The registered callback has signature
///   `(class: string, source: string) => string | null | undefined`
/// where the return value is either the JSON triple-array encoding
/// expected in `data-hl-spans` or a nullish value meaning "no spans to
/// emit for this input" (maps to `Ok(None)` on the Rust side).
#[wasm_bindgen]
pub struct JsUserGrammars {
    grammars: std::collections::HashMap<String, js_sys::Function>,
}

#[wasm_bindgen]
impl JsUserGrammars {
    #[wasm_bindgen(constructor)]
    pub fn new() -> JsUserGrammars {
        JsUserGrammars {
            grammars: std::collections::HashMap::new(),
        }
    }

    /// Register a highlight callback for a given language class. If a
    /// callback was already registered for `language_class`, it is
    /// replaced — this matches the "user grammar wins on collision"
    /// semantics of the native loader.
    pub fn register(&mut self, language_class: &str, highlight_fn: js_sys::Function) {
        self.grammars
            .insert(language_class.to_string(), highlight_fn);
    }
}

impl Default for JsUserGrammars {
    fn default() -> Self {
        Self::new()
    }
}

impl quarto_highlight::UserGrammarProvider for JsUserGrammars {
    fn contains(&self, class: &str) -> bool {
        self.grammars.contains_key(class)
    }

    fn highlight(
        &mut self,
        class: &str,
        source: &str,
    ) -> Result<Option<String>, quarto_highlight::HighlightError> {
        let Some(func) = self.grammars.get(class) else {
            return Ok(None);
        };
        let this = JsValue::NULL;
        let result = func
            .call2(&this, &JsValue::from_str(class), &JsValue::from_str(source))
            .map_err(|e| {
                // JS exceptions are not convertible to Display directly;
                // extract a reasonable message via JSON.stringify fallback.
                let msg = js_sys::JSON::stringify(&e)
                    .map(|s| s.as_string().unwrap_or_else(|| "<unstringifiable>".into()))
                    .unwrap_or_else(|_| "<unknown JS error>".into());
                quarto_highlight::HighlightError::Provider(format!(
                    "JS user-grammar callback for `{}` threw: {}",
                    class, msg
                ))
            })?;

        if result.is_null() || result.is_undefined() {
            return Ok(None);
        }
        match result.as_string() {
            Some(json) => Ok(Some(json)),
            None => Err(quarto_highlight::HighlightError::Provider(format!(
                "JS user-grammar callback for `{}` returned non-string non-null: {:?}",
                class, result
            ))),
        }
    }
}

/// Test entry point for Phase 4.3 — consumed by
/// `hub-client/src/services/userGrammarBridge.wasm.test.ts`.
///
/// Calls the bridge's `UserGrammarProvider::highlight` directly. Unlike
/// the full `render_qmd` path, this does no AST walking — it's the
/// narrowest verification that the JS callback is invoked correctly
/// and its return value flows back through wasm-bindgen as expected.
#[wasm_bindgen]
pub fn quarto_highlight_with_user_for_test(
    language_class: &str,
    source: &str,
    user: &mut JsUserGrammars,
) -> Result<Option<String>, JsValue> {
    use quarto_highlight::UserGrammarProvider;
    if user.contains(language_class) {
        user.highlight(language_class, source)
            .map_err(|e| JsValue::from_str(&e.to_string()))
    } else {
        Ok(None)
    }
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

/// Clear user files from the virtual filesystem.
///
/// This clears project files while preserving embedded resources
/// (Bootstrap SCSS files under `/__quarto_resources__/`).
///
/// # ⚠️  Session-teardown only
///
/// **Do not call between renders.** Phase 9 (hub-client project
/// rendering) makes the VFS load-bearing across renders:
/// `WebsiteProjectType::post_render` flushes Project-scoped
/// artifacts (theme CSS, shared JS) to
/// `/.quarto/project-artifacts/...`, and the iframe post-processor
/// reads those entries back from VFS by absolute path. Clearing
/// mid-session loses these artifacts and breaks the next preview
/// (broken `<link rel="stylesheet">` to theme CSS, missing
/// quarto-nav JS).
///
/// Safe call sites: session disconnect, project switch, end-to-end
/// test teardown. See
/// `claude-notes/plans/2026-04-27-websites-phase-9.md` §Decision 7.
///
/// # Returns
/// JSON: `{ "success": true }`
#[wasm_bindgen]
pub fn vfs_clear() -> String {
    get_runtime().clear_user_files(RESOURCE_PATH_PREFIX);
    VfsResponse::ok()
}

/// Set runtime metadata for the configuration merge pipeline.
///
/// Runtime metadata is merged as the highest-precedence layer, above project,
/// directory, and document metadata. This allows the host environment to inject
/// settings like `format.html.source-location: full` for scroll sync.
///
/// # Arguments
/// * `yaml` - YAML string with metadata to inject, or empty string to clear
///
/// # Returns
/// JSON: `{ "success": true }` or `{ "success": false, "error": "..." }`
///
/// # Example
///
/// ```javascript
/// vfs_set_runtime_metadata("format:\n  html:\n    source-location: full\n");
/// ```
#[wasm_bindgen]
pub fn vfs_set_runtime_metadata(yaml: &str) -> String {
    if yaml.is_empty() {
        get_runtime().set_runtime_metadata(None);
        return VfsResponse::ok();
    }

    match serde_yaml::from_str::<serde_json::Value>(yaml) {
        Ok(value) => {
            if value.is_object() {
                get_runtime().set_runtime_metadata(Some(value));
                VfsResponse::ok()
            } else {
                VfsResponse::error("Runtime metadata must be a YAML mapping")
            }
        }
        Err(e) => VfsResponse::error(&format!("Failed to parse YAML: {}", e)),
    }
}

/// Get the current runtime metadata.
///
/// # Returns
/// JSON: `{ "success": true, "content": "..." }` with YAML string,
/// or `{ "success": true, "content": null }` if no metadata is set
#[wasm_bindgen]
pub fn vfs_get_runtime_metadata() -> String {
    match get_runtime().get_runtime_metadata() {
        Some(value) => match serde_yaml::to_string(&value) {
            Ok(yaml) => VfsResponse::with_content(yaml),
            Err(e) => VfsResponse::error(&format!("Failed to serialize metadata: {}", e)),
        },
        None => serde_json::to_string(&serde_json::json!({
            "success": true,
            "content": null
        }))
        .unwrap(),
    }
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
//
// The JSON wire shape (`JsonDiagnostic`, `JsonDiagnosticDetail`,
// `JsonPass1Failure`) plus the per-diagnostic `diagnostic_to_json`
// helper and the `with_source_file` tagger now live in
// `quarto_error_reporting::json`, shared with `quarto-preview`'s
// server-side diagnostics endpoint (bd-b9kzg). Local helpers that
// build on those primitives stay below.

/// Convert a list of [`FileFailure`]s into wire-shape
/// [`JsonPass1Failure`]s, attaching structured diagnostics when
/// the failure came from a [`ParseError`](quarto_core::error::ParseError).
/// Used to surface non-active-page Pass-1 failures to the
/// hub-client overlay (bd-rqba).
fn pass1_failures_to_json(
    failures: &[quarto_core::project::orchestrator::FileFailure],
) -> Vec<JsonPass1Failure> {
    failures
        .iter()
        .map(|failure| {
            let source_file = failure.input.to_string_lossy().into_owned();
            let diagnostics = match &failure.source_context {
                Some(ctx) => diagnostics_to_json(&failure.diagnostics, ctx)
                    .into_iter()
                    .map(|d| with_source_file(d, source_file.clone()))
                    .collect(),
                None => Vec::new(),
            };
            JsonPass1Failure::new(source_file, failure.error.clone(), diagnostics)
        })
        .collect()
}

/// Convert a slice of DiagnosticMessages to JsonDiagnostics.
fn diagnostics_to_json(diags: &[DiagnosticMessage], ctx: &SourceContext) -> Vec<JsonDiagnostic> {
    diags.iter().map(|d| diagnostic_to_json(d, ctx)).collect()
}

// ============================================================================
// RENDERING API
// ============================================================================

/// `skip_serializing_if` predicate for the default-false `is_slides` flag.
fn is_false(b: &bool) -> bool {
    !*b
}

#[derive(Serialize, Default)]
struct RenderResponse {
    success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    /// Rendered HTML for the active page. Populated by the
    /// HTML-pipeline branch (single-doc and project-active),
    /// `None` for q2-preview / error responses.
    #[serde(skip_serializing_if = "Option::is_none")]
    html: Option<String>,
    /// Serialized Pandoc AST JSON for the q2-preview format.
    /// Populated by the q2-preview pipeline branch (added in a
    /// later commit), `None` for HTML / error responses. The JS
    /// layer dispatches on which of `html` / `ast_json` is
    /// present.
    #[serde(skip_serializing_if = "Option::is_none")]
    ast_json: Option<String>,
    /// True when `ast_json` is a `format: revealjs` deck (preview
    /// pseudo-format `q2-slides`). The SPA renders the section AST with a
    /// reveal shell (`.reveal/.slides` + reveal.js) instead of the plain
    /// q2-preview html mirror. Absent (false) for ordinary q2-preview.
    #[serde(skip_serializing_if = "is_false")]
    is_slides: bool,
    /// Structured diagnostics (errors) with line/column information for Monaco.
    #[serde(skip_serializing_if = "Option::is_none")]
    diagnostics: Option<Vec<JsonDiagnostic>>,
    /// Structured warnings with line/column information for Monaco.
    #[serde(skip_serializing_if = "Option::is_none")]
    warnings: Option<Vec<JsonDiagnostic>>,
    /// Pass-1 failures for project files other than the active
    /// page (bd-rqba). Carries the structured parse diagnostic so
    /// the overlay can show "about.qmd had a parse error" with
    /// line/column rather than the misleading
    /// "Sidebar references missing document information for 'about.qmd'" alone.
    /// Decision D1: dedicated field — engine policy-free, consumer
    /// chooses strict vs lenient.
    #[serde(skip_serializing_if = "Option::is_none")]
    pass1_failures: Option<Vec<JsonPass1Failure>>,
    /// Compiled theme CSS fingerprint (Plan 2A item 11). Pulled from
    /// the `css:theme:<fingerprint>` artifact key produced by
    /// `CompileThemeCssStage`. The hub-client iframe wrapper uses
    /// this to dedupe theme CSS posts: if the fingerprint hasn't
    /// changed, the parent skips the blob-URL re-mint cycle.
    /// `None` for renders that produced no theme artifact (q2-debug,
    /// errors, single-doc renders without a `theme:` YAML key).
    #[serde(skip_serializing_if = "Option::is_none")]
    theme_fingerprint: Option<String>,
}

/// Create a minimal project context for WASM rendering.
fn create_wasm_project_context(path: &Path) -> ProjectContext {
    let dir = path.parent().unwrap_or(Path::new("/")).to_path_buf();
    ProjectContext {
        dir: dir.clone(),
        config: ProjectConfig::default(),
        is_single_file: true,
        files: vec![DocumentInfo::from_path(path)],
        output_dir: dir,
    }
}

/// Map a format string for `q2 preview`'s default-format substitution.
///
/// `quarto preview` treats `html` — which is also the default when no
/// `format:` key is present in the YAML — as a request for the
/// `q2-preview` pipeline (AST-iframe rendering), so a bare markdown
/// file with no frontmatter previews live. Explicit non-html formats
/// (`q2-slides`, `q2-debug`, `acm-html`, …) pass through unchanged.
///
/// The substitution lives here rather than in the CLI shim so the
/// dispatch in `render_*_to_response` sees a stable format string and
/// doesn't need a parallel preview-mode branch.
fn map_format_for_preview(format_str: &str) -> &str {
    match format_str {
        "html" => "q2-preview",
        // `format: revealjs` previews as `q2-slides`: same AST/preview
        // pipeline_kind (so the response carries `ast_json`), but the reveal
        // slide tree is built and the SPA renders it with a reveal shell.
        "revealjs" => "q2-slides",
        other => other,
    }
}

/// Detect the format string from QMD content's YAML frontmatter.
/// Returns the format name (e.g., "q2-slides", "q2-debug", "html", "acm-html")
/// or "html" as default when no format key is present.
fn detect_format_from_content(content: &str) -> String {
    let trimmed = content.trim_start();
    if !trimmed.starts_with("---") {
        return "html".to_string();
    }
    let after_first = &trimmed[3..];
    let end = after_first.find("\n---");
    let yaml_str = match end {
        Some(pos) => &after_first[..pos],
        None => return "html".to_string(),
    };
    let docs = match yaml_rust2::YamlLoader::load_from_str(yaml_str) {
        Ok(docs) => docs,
        Err(_) => return "html".to_string(),
    };
    let doc = match docs.first() {
        Some(doc) => doc,
        None => return "html".to_string(),
    };
    match doc["format"] {
        yaml_rust2::Yaml::String(ref s) => s.clone(),
        yaml_rust2::Yaml::Hash(ref map) => {
            // yaml-rust2 preserves insertion order (uses LinkedHashMap)
            map.keys()
                .next()
                .and_then(|k| k.as_str())
                .unwrap_or("html")
                .to_string()
        }
        _ => "html".to_string(),
    }
}

/// Parse QMD content to Pandoc AST JSON using the unified pipeline.
///
/// This function uses the same pipeline infrastructure as `render_qmd_to_html`,
/// ensuring feature parity. It runs through:
/// 1. ParseDocumentStage - Parse QMD to Pandoc AST
/// 2. EngineExecutionStage - Execute code cells (passes through in WASM)
/// 3. AstTransformsStage - Apply Quarto transforms (callouts, metadata, etc.)
///
/// # Arguments
/// * `content` - QMD source text
///
/// # Returns
/// JSON string containing:
/// - `success`: true/false
/// - `ast`: Serialized Pandoc AST (on success)
/// - `error`: Error message (on failure)
/// - `diagnostics`: Structured error diagnostics with line/column info
/// - `warnings`: Structured warning diagnostics with line/column info
///
/// Equivalent to
/// [`parse_qmd_to_ast_with_attribution(content, None)`](parse_qmd_to_ast_with_attribution).
/// Kept as a separate entry point for callers that have no attribution
/// to ship and want the simpler signature.
#[wasm_bindgen]
pub async fn parse_qmd_to_ast(content: &str) -> String {
    parse_qmd_to_ast_with_attribution(content, None).await
}

/// Parse QMD content to Pandoc AST JSON, optionally with attribution.
///
/// When `attribution_json` is `Some(s)`, the JSON string is wrapped in
/// a [`PreBuiltAttributionProvider`] and installed on the
/// `RenderContext`; `AttributionGenerateTransform` and
/// `AttributionRenderTransform` are then invoked **directly** on the
/// AST after the existing 3-stage parse (NOT via the full
/// `AstTransformsStage`, which would also fire every other transform
/// on the q2-debug surface). When `None`, the result is byte-identical
/// to [`parse_qmd_to_ast`] for every fixture — same code path, no
/// provider installed, no transforms fire.
///
/// # Byte-identicality invariant
///
/// `parse_qmd_to_ast(content)` is byte-identical to
/// `parse_qmd_to_ast_with_attribution(content, None)` for every
/// fixture. A regression on the `None` branch would break *all*
/// renders, not just attribution ones.
///
/// [`PreBuiltAttributionProvider`]:
///     quarto_core::attribution::PreBuiltAttributionProvider
#[wasm_bindgen]
pub async fn parse_qmd_to_ast_with_attribution(
    content: &str,
    attribution_json: Option<String>,
) -> String {
    // Create a virtual path for this content
    let path = Path::new("/input.qmd");

    // Create project context
    let project = create_wasm_project_context(path);
    let doc = DocumentInfo::from_path(path);
    let binaries = BinaryDependencies::new();

    let format_str = detect_format_from_content(content);
    let format = match Format::from_format_string(&format_str) {
        Ok(f) => f,
        Err(e) => {
            return serde_json::to_string(&AstResponse {
                success: false,
                ast: None,
                qmd: None,
                error: Some(e),
                diagnostics: None,
                warnings: None,
            })
            .unwrap_or_default();
        }
    };

    let options = RenderOptions {
        verbose: false,
        execute: false,
        use_freeze: false,
        output_path: None,
    };

    let mut ctx = RenderContext::new(&project, &doc, &format, &binaries).with_options(options);

    // Install the prebuilt provider before the pipeline runs. JSON
    // parse + interning is lazy inside `build()`, so this is cheap and
    // infallible at construction time; any payload error surfaces
    // through `AttributionGenerateTransform`'s diagnostics path.
    if let Some(json) = attribution_json {
        ctx.attribution_provider = Some(Arc::new(
            quarto_core::attribution::PreBuiltAttributionProvider::new(json),
        ));
    }

    // Share the global VFS runtime with the pipeline
    let runtime_arc: Arc<dyn SystemRuntime> =
        Arc::clone(get_runtime_arc()) as Arc<dyn SystemRuntime>;

    let result = quarto_core::pipeline::parse_qmd_to_ast(
        content.as_bytes(),
        "/input.qmd",
        &mut ctx,
        runtime_arc,
    )
    .await;

    let mut output = match result {
        Ok(out) => out,
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

            return serde_json::to_string(&AstResponse {
                success: false,
                error: Some(error_msg),
                ast: None,
                qmd: None,
                diagnostics,
                warnings: None,
            })
            .unwrap();
        }
    };

    // Direct-invocation flow: when the provider is installed, run
    // both attribution transforms on the parsed AST *outside* the
    // `AstTransformsStage` so the rest of the q2-debug pipeline
    // (callouts/sectionize/etc.) doesn't suddenly fire on this
    // surface. The transforms read/write only `RenderContext`.
    if ctx.attribution_provider.is_some() {
        use quarto_core::transform::AstTransform;
        use quarto_core::transforms::{AttributionGenerateTransform, AttributionRenderTransform};
        if let Err(e) = AttributionGenerateTransform::new()
            .transform(&mut output.ast, &mut ctx)
            .await
        {
            return serde_json::to_string(&AstResponse {
                success: false,
                error: Some(format!("attribution generate failed: {e}")),
                ast: None,
                qmd: None,
                diagnostics: None,
                warnings: None,
            })
            .unwrap();
        }
        if let Err(e) = AttributionRenderTransform::new()
            .transform(&mut output.ast, &mut ctx)
            .await
        {
            return serde_json::to_string(&AstResponse {
                success: false,
                error: Some(format!("attribution render failed: {e}")),
                ast: None,
                qmd: None,
                diagnostics: None,
                warnings: None,
            })
            .unwrap();
        }
    }

    // Create an ASTContext from the SourceContext returned by the pipeline
    let ast_context = pampa::pandoc::ASTContext {
        filenames: vec!["/input.qmd".to_string()],
        example_list_counter: std::cell::Cell::new(1),
        source_context: output.source_context.clone(),
        parent_source_info: None,
    };

    // Serialize the AST to JSON using pampa's writer. When attribution
    // ran, forward `ctx.format_options.json` into JsonConfig so the
    // streaming writer emits `astContext.attribution` and
    // `astContext.attributionActors`. Off-path (provider absent), both
    // fields stay `None` and the JSON output is byte-identical to the
    // unflagged path (Phase 0 test #10).
    let (attribution_by_node, attribution_actors) =
        quarto_core::attribution::json_attribution_fields(&ctx.format_options.json);
    let mut buf = Vec::new();
    let json_config = pampa::writers::json::JsonConfig {
        include_inline_locations: true,
        attribution_by_node,
        attribution_actors,
    };

    let ast_json = match pampa::writers::json::write_with_config(
        &output.ast,
        &ast_context,
        &mut buf,
        &json_config,
    ) {
        Ok(_) => match String::from_utf8(buf) {
            Ok(json) => json,
            Err(e) => {
                return serde_json::to_string(&AstResponse {
                    success: false,
                    error: Some(format!("Failed to convert AST JSON to string: {}", e)),
                    ast: None,
                    qmd: None,
                    diagnostics: None,
                    warnings: None,
                })
                .unwrap();
            }
        },
        Err(e) => {
            return serde_json::to_string(&AstResponse {
                success: false,
                error: Some(format!("Failed to serialize AST: {:?}", e)),
                ast: None,
                qmd: None,
                diagnostics: None,
                warnings: None,
            })
            .unwrap();
        }
    };

    // Convert warnings to structured JSON with line/column info.
    // Attribution diagnostics collected on `ctx.diagnostics` are
    // surfaced through the same channel.
    let mut warnings = diagnostics_to_json(&output.warnings, &output.source_context);
    if !ctx.diagnostics.is_empty() {
        warnings.extend(diagnostics_to_json(
            &ctx.diagnostics,
            &output.source_context,
        ));
    }
    serde_json::to_string(&AstResponse {
        success: true,
        error: None,
        ast: Some(ast_json),
        qmd: None,
        diagnostics: None,
        warnings: if warnings.is_empty() {
            None
        } else {
            Some(warnings)
        },
    })
    .unwrap()
}

/// Render a QMD file from the virtual filesystem.
///
/// # Arguments
/// * `path` - Path to the QMD file in VFS (e.g., "index.qmd")
/// * `user_grammars` - Optional user-grammar provider. If present, the
///   render pipeline consults it for any code block whose language class
///   the provider recognizes, before falling back to built-in grammars.
///   The value is consumed by this call; JS callers typically construct
///   a fresh `JsUserGrammars` per render (re-registering from an in-JS
///   cache of loaded grammars is cheap).
///
/// # Returns
/// JSON: `{ "success": true, "html": "..." }` or `{ "success": false, "error": "...", "diagnostics": [...] }`
#[wasm_bindgen]
pub async fn render_qmd(path: &str, user_grammars: Option<JsUserGrammars>) -> String {
    let runtime = get_runtime();
    let path = Path::new(path);

    // Read the file from VFS
    let content = match runtime.file_read(path) {
        Ok(bytes) => bytes,
        Err(e) => return error_response(format!("Failed to read file: {}", e)),
    };

    // Discover project context from VFS (finds _quarto.yml in parent directories)
    let project = match ProjectContext::discover(path, runtime) {
        Ok(p) => p,
        Err(e) => return error_response(format!("Failed to discover project context: {}", e)),
    };

    render_single_doc_to_response(
        path,
        &content,
        &project,
        user_grammars,
        false,
        Vec::new(),
        None,
    )
    .await
}

/// Render QMD content directly (without reading from VFS).
///
/// # Arguments
/// * `content` - QMD source text
/// * `_template_bundle` - Optional template bundle JSON (currently unused, reserved for future use)
/// * `user_grammars` - Optional user-grammar provider; same semantics as
///   for [`render_qmd`]. Consumed by the call.
///
/// # Returns
/// JSON: `{ "success": true, "html": "..." }` or `{ "success": false, "error": "...", "diagnostics": [...] }`
#[wasm_bindgen]
pub async fn render_qmd_content(
    content: &str,
    _template_bundle: &str,
    user_grammars: Option<JsUserGrammars>,
) -> String {
    // Synthetic path for path-less render. Source diagnostics
    // surface as `/input.qmd` to the JS layer.
    let path = Path::new("/input.qmd");
    let project = create_wasm_project_context(path);
    render_single_doc_to_response(
        path,
        content.as_bytes(),
        &project,
        user_grammars,
        false,
        Vec::new(),
        None,
    )
    .await
}

/// Render a single page **in the context of its surrounding project**.
///
/// Phase 9 entry point used by the hub-client live preview.
/// Equivalent to
/// [`render_page_in_project_with_attribution(path, user_grammars, None)`](render_page_in_project_with_attribution).
/// Kept as a separate entry point for callers that have no
/// attribution payload to ship and want the simpler signature.
#[wasm_bindgen]
pub async fn render_page_in_project(path: &str, user_grammars: Option<JsUserGrammars>) -> String {
    render_page_in_project_with_attribution(path, user_grammars, None).await
}

/// Render a single page **in the context of its surrounding project**,
/// optionally with attribution data.
///
/// Phase 9 entry point used by the hub-client live preview. The
/// flow:
///
/// 1. Read the source from VFS.
/// 2. Discover the project context (walks parent dirs for
///    `_quarto.yml`).
/// 3. **Single-file** (no `_quarto.yml` ancestor) → fall through
///    to the existing single-doc render path. Output is byte-
///    identical to `render_qmd` when `attribution_json` is `None`.
/// 4. **Multi-file project** → drive `ProjectPipeline` with the
///    WASM `RenderToHtmlRenderer` (HTML) or
///    `RenderToPreviewAstRenderer` (q2-preview) and
///    `RenderMode::ActivePage(…)`. Pass-1 runs over every project
///    file (cache-backed via the Phase-8 `cache_get`/`cache_set`
///    infra), `pre_render` runs, Pass-2 renders just the active
///    page, and `post_render` flushes Project-scoped artifacts to
///    VFS via the same resolver Pass-2 used.
///
/// When `attribution_json` is `Some(s)`, the JSON string is wrapped
/// in a [`PreBuiltAttributionProvider`] and installed on the active
/// page's `RenderContext`; the attribution-generate and
/// attribution-render transforms (registered in the q2-preview
/// transform pipeline) fire from inside the pipeline, populating
/// `astContext.attribution` and `astContext.attributionActors` on
/// the resulting AST JSON. When `None`, the result is byte-identical
/// to [`render_page_in_project`] for every fixture — same code path,
/// no provider installed, no transforms surface attribution data.
///
/// # Byte-identicality invariant
///
/// `render_page_in_project(path, user_grammars)` is byte-identical
/// to `render_page_in_project_with_attribution(path, user_grammars,
/// None)` for every fixture. A regression on the `None` branch
/// would break *all* q2-preview renders, not just attributed ones.
///
/// In both branches the response shape is the same `RenderResponse`
/// JSON `render_qmd` returns today — the JS layer doesn't need a
/// new type.
///
/// # Arguments
/// * `path` - Path to the active QMD file in VFS.
/// * `user_grammars` - Optional user-grammar provider; same
///   semantics as for [`render_qmd`].
/// * `attribution_json` - Optional serialized transport JSON. Same
///   shape as the payload accepted by
///   [`parse_qmd_to_ast_with_attribution`].
///
/// [`PreBuiltAttributionProvider`]:
///     quarto_core::attribution::PreBuiltAttributionProvider
#[wasm_bindgen]
pub async fn render_page_in_project_with_attribution(
    path: &str,
    user_grammars: Option<JsUserGrammars>,
    attribution_json: Option<String>,
) -> String {
    let runtime = get_runtime();
    let path_buf = std::path::PathBuf::from(path);
    let path = path_buf.as_path();

    // Read the file from VFS up front. Both branches need it.
    let content = match runtime.file_read(path) {
        Ok(bytes) => bytes,
        Err(e) => {
            return error_response(format!("Failed to read file: {}", e));
        }
    };

    // Discover from the active path first to find any
    // `_quarto.yml` ancestor and learn whether this is a
    // single-file or multi-file project.
    let project = match ProjectContext::discover(path, runtime) {
        Ok(p) => p,
        Err(e) => {
            return error_response(format!("Failed to discover project context: {}", e));
        }
    };

    // Single-file: no `_quarto.yml` was found in any ancestor.
    // Behavior is byte-identical to `render_qmd` — same single-doc
    // pipeline, same VFS-root resolver, same VFS artifact dump.
    if project.is_single_file {
        return render_single_doc_to_response(
            path,
            &content,
            &project,
            user_grammars,
            false,
            Vec::new(),
            attribution_json,
        )
        .await;
    }

    // Multi-doc project. The discover-from-file form returns a
    // project whose `files` is just `[active]`, which would
    // starve Pass-1 of every sibling's profile and break the
    // sidebar's title resolution / cross-doc link rewriter.
    // Re-discover from the project root so we get the full
    // sibling list.
    let project = match ProjectContext::discover(&project.dir, runtime) {
        Ok(p) => p,
        Err(e) => {
            return error_response(format!("Failed to enumerate project files: {}", e));
        }
    };
    render_project_active_page_to_response(
        &path_buf,
        &content,
        project,
        user_grammars,
        false,
        Vec::new(),
        attribution_json,
    )
    .await
}

/// Like [`render_page_in_project`], but applies the `q2 preview`
/// format-default substitution: a document with no explicit
/// `format:` key (which `detect_format_from_content` reports as
/// `html`) renders through the q2-preview pipeline and returns
/// `ast_json`. Explicit non-html formats are honoured as-is.
///
/// The `q2 preview` CLI calls this entry point; hub-client keeps
/// using [`render_page_in_project`] so its existing format dispatch
/// is unchanged.
///
/// Phase C.4 (bd-kw93.3): if `capture_gz_json` is provided — gzipped
/// JSON bytes of a [`EngineCapture`], the same wire format Phase C.1
/// writes to the capture binary doc — the WASM constructs an
/// [`EngineRegistry::with_replay`] from it. `EngineExecutionStage` then
/// routes calls to [`quarto_core::engine::ReplayEngine`] for the
/// captured engine name; everything else (markdown, future engines)
/// stays on the default registry. The SPA reads the capture binary
/// doc out of the IndexDocument's capture sidecar (V2 schema, C.3)
/// and threads it through this argument.
#[wasm_bindgen]
pub async fn render_page_for_preview(
    path: &str,
    user_grammars: Option<JsUserGrammars>,
    capture_gz_json: Option<Vec<u8>>,
) -> String {
    let runtime = get_runtime();
    let path_buf = std::path::PathBuf::from(path);
    let path = path_buf.as_path();

    let content = match runtime.file_read(path) {
        Ok(bytes) => bytes,
        Err(e) => {
            return error_response(format!("Failed to read file: {}", e));
        }
    };

    // bd-lucp: deserialize the gzipped JSON capture into an
    // `EngineCapture`. Both render branches (single-doc + project)
    // thread this into the q2-preview pipeline's
    // `CaptureSpliceStage` rather than constructing a
    // `ReplayEngine`-bearing registry; `ReplayEngine` remains the
    // bd-45yw regression-testing tool and is not on this path.
    let captures = match parse_capture_from(capture_gz_json) {
        Ok(caps) => caps,
        Err(e) => {
            return error_response(format!("Failed to parse capture: {}", e));
        }
    };

    let project = match ProjectContext::discover(path, runtime) {
        Ok(p) => p,
        Err(e) => {
            return error_response(format!("Failed to discover project context: {}", e));
        }
    };

    if project.is_single_file {
        return render_single_doc_to_response(
            path,
            &content,
            &project,
            user_grammars,
            true,
            captures,
            None,
        )
        .await;
    }

    let project = match ProjectContext::discover(&project.dir, runtime) {
        Ok(p) => p,
        Err(e) => {
            return error_response(format!("Failed to enumerate project files: {}", e));
        }
    };
    render_project_active_page_to_response(
        &path_buf,
        &content,
        project,
        user_grammars,
        true,
        captures,
        None,
    )
    .await
}

/// Ungzip + JSON-deserialize a capture payload into the ordered
/// sequence of [`EngineCapture`](quarto_trace::EngineCapture)s (one per
/// engine that ran server-side; bd-5yff4). `None`/empty input → empty
/// vec, for symmetry with WASM/JS callers that may pass an empty
/// `Uint8Array` to mean "no capture."
///
/// The payload is a gzipped JSON **array** of captures. For robustness
/// against a stale pre-bd-5yff4 doc (a single capture object), a failed
/// array parse falls back to a single-object parse wrapped in a vec.
///
/// bd-lucp: the preview path consumes these via `CaptureSpliceStage`,
/// not `ReplayEngine` — see
/// `claude-notes/plans/2026-05-18-q2-preview-project-replay-engine.md`.
fn parse_capture_from(
    capture_gz_json: Option<Vec<u8>>,
) -> Result<Vec<quarto_trace::EngineCapture>, String> {
    use std::io::Read;

    let Some(bytes) = capture_gz_json else {
        return Ok(Vec::new());
    };
    if bytes.is_empty() {
        return Ok(Vec::new());
    }

    let mut decoder = flate2::read::GzDecoder::new(&bytes[..]);
    let mut json = Vec::new();
    decoder
        .read_to_end(&mut json)
        .map_err(|e| format!("ungzip failed: {}", e))?;

    match serde_json::from_slice::<Vec<quarto_trace::EngineCapture>>(&json) {
        Ok(captures) => Ok(captures),
        Err(array_err) => {
            // Lenient fallback: a stale single-object capture doc.
            match serde_json::from_slice::<quarto_trace::EngineCapture>(&json) {
                Ok(single) => Ok(vec![single]),
                Err(_) => Err(format!("JSON parse failed: {}", array_err)),
            }
        }
    }
}

/// Single-doc render path — used by `render_qmd` directly and by
/// `render_page_in_project` when no `_quarto.yml` ancestor exists.
///
/// `prefer_preview_format` is `true` only when the call originates
/// from `render_page_for_preview`; in that mode `html` (including the
/// no-frontmatter default) maps to `q2-preview`.
/// `attribution_json`, when `Some`, is a serialized
/// [`quarto_core::attribution::types::TransportAttributionData`]
/// shipped by the hub-client. Installed on `ctx.attribution_provider`
/// so [`quarto_core::transforms::AttributionGenerateTransform`] +
/// [`quarto_core::transforms::AttributionRenderTransform`] fire from
/// inside the pipeline (they're registered in
/// [`quarto_core::pipeline::build_q2_preview_transform_pipeline`]).
/// `None` is byte-identical to today's output for every existing caller.
///
/// Returns the [`RenderResponse`] JSON string the JS layer expects.
async fn render_single_doc_to_response(
    path: &Path,
    content: &[u8],
    project: &ProjectContext,
    user_grammars: Option<JsUserGrammars>,
    prefer_preview_format: bool,
    // bd-lucp / bd-5yff4: recorded engine capture sequence for the
    // preview-time AST-splice path (one per engine, in order). Forwarded
    // to `render_qmd_to_preview_ast` (which folds them through
    // `CaptureSpliceStage`) on the preview branch. The HTML branch
    // ignores it (HTML renders don't consume captures in the WASM today —
    // `q2 render` natively runs engines instead).
    captures: Vec<quarto_trace::EngineCapture>,
    attribution_json: Option<String>,
) -> String {
    let doc = DocumentInfo::from_path(path);
    let binaries = BinaryDependencies::new();

    let content_str = std::str::from_utf8(content).unwrap_or("");
    let detected = detect_format_from_content(content_str);
    let format_str = if prefer_preview_format {
        map_format_for_preview(&detected).to_string()
    } else {
        detected
    };
    let format = match Format::from_format_string(&format_str) {
        Ok(f) => f,
        Err(e) => return error_response(e),
    };

    let options = RenderOptions {
        verbose: false,
        execute: false,
        use_freeze: false,
        output_path: None,
    };

    let mut ctx = RenderContext::new(project, &doc, &format, &binaries).with_options(options);
    if let Some(provider) = user_grammars {
        ctx.user_grammar_provider = Some(std::rc::Rc::new(std::cell::RefCell::new(provider)));
    }
    if let Some(json) = attribution_json {
        ctx.attribution_provider = Some(Arc::new(
            quarto_core::attribution::PreBuiltAttributionProvider::new(json),
        ));
    }

    // Phase 5 VFS-root resolver — every artifact resolves under
    // `/.quarto/project-artifacts/...` so the post-processor can
    // read them from VFS at the matching path.
    let resolver = ResourceResolverContext::vfs_root("/.quarto/project-artifacts");
    ctx.resource_resolver = Some(resolver.clone());
    let source_name = path.to_string_lossy();

    let runtime_arc: Arc<dyn SystemRuntime> =
        Arc::clone(get_runtime_arc()) as Arc<dyn SystemRuntime>;

    // Dispatch on `format.pipeline_kind`: q2-preview runs the
    // q2-preview entry point and returns AST JSON; everything else
    // runs the HTML pipeline. Both branches share the same prelude
    // (RenderContext + resolver) and tail (VFS artifact flush +
    // RenderResponse construction with `skip_serializing_if = None`
    // on the unused payload field).
    let (html, ast_json, diagnostics, source_context) = match format.pipeline_kind {
        Some("preview") => {
            match render_qmd_to_preview_ast(
                content,
                &source_name,
                &mut ctx,
                runtime_arc,
                None,
                captures,
            )
            .await
            {
                Ok(out) => (
                    None,
                    Some(out.ast_json),
                    out.diagnostics,
                    out.source_context,
                ),
                Err(e) => return render_error_response(e),
            }
        }
        _ => {
            let config = HtmlRenderConfig::with_resolver(resolver.clone());
            match render_qmd_to_html(content, &source_name, &mut ctx, &config, runtime_arc).await {
                Ok(out) => (Some(out.html), None, out.diagnostics, out.source_context),
                Err(e) => return render_error_response(e),
            }
        }
    };

    // Populate VFS with artifacts — Phase 5 routes both
    // page- and project-scope artifacts under the same
    // synthetic root in vfs_root mode. Format-agnostic: both the
    // HTML pipeline's `RenderHtmlBodyStage` and the q2-preview
    // pipeline's `ResourceCollectorTransform` accumulate artifacts
    // on `ctx.artifacts` using the same resolver, so the iframe
    // (HTML or AST-iframe) finds the bytes at the matching VFS
    // path either way.
    //
    // The shared flush carries the bd-3gtn empty-content skip
    // (manifest entries must never be written) and the bd-q3bxnq2e
    // change-detection (byte-identical re-writes are skipped, so
    // unchanged theme CSS / fonts / JS are not re-cloned per
    // keystroke). See `quarto_core::artifact_flush`.
    let runtime = get_runtime();
    runtime.with_vfs_mut(|vfs| {
        quarto_core::flush_artifacts_to_vfs(&ctx.artifacts, &resolver, vfs);
    });

    let warnings = diagnostics_to_json(&diagnostics, &source_context);
    let theme_fingerprint = extract_theme_fingerprint(&ctx.artifacts);
    serde_json::to_string(&RenderResponse {
        success: true,
        error: None,
        html,
        ast_json,
        is_slides: format.target_format == "q2-slides",
        diagnostics: None,
        warnings: if warnings.is_empty() {
            None
        } else {
            Some(warnings)
        },
        pass1_failures: None,
        theme_fingerprint,
    })
    .unwrap()
}

/// Pull the compiled theme fingerprint out of the artifact store. The
/// `CompileThemeCssStage` keys its artifact `css:theme:<fingerprint>`;
/// we recover the fingerprint from the suffix without re-hashing CSS
/// bytes. Returns `None` if no theme artifact was produced (q2-debug,
/// errors, single-doc renders without a `theme:` YAML key).
fn extract_theme_fingerprint(store: &quarto_core::ArtifactStore) -> Option<String> {
    store
        .get_by_prefix("css:theme:")
        .first()
        .and_then(|(key, _)| key.strip_prefix("css:theme:"))
        .map(|s| s.to_string())
}

/// Project-scoped render path — drives the orchestrator with
/// `RenderToHtmlRenderer` to produce just the active page in the
/// context of its surrounding project (sidebar, navbar, cross-doc
/// links, deduplicated theme CSS).
///
/// `attribution_json` is plumbed through to
/// `RenderToPreviewAstRenderer::with_attribution` on the q2-preview
/// branch (the active-page ctx is constructed inside the renderer's
/// `render()`, so installing the provider from here is impossible —
/// the builder is the attachment point). Ignored on the HTML
/// branch (q2-preview is the only consumer of attribution wire data
/// for v1; HTML CLI uses `data-attr-*` inline markup via a separate
/// transform).
async fn render_project_active_page_to_response(
    active_path: &Path,
    _content: &[u8],
    mut project: ProjectContext,
    user_grammars: Option<JsUserGrammars>,
    prefer_preview_format: bool,
    // bd-lucp / bd-5yff4: recorded engine capture sequence attached to
    // the q2-preview pass-2 renderer (one per engine, in order).
    // `RenderToPreviewAstRenderer::with_captures` threads them into every
    // per-doc `render_qmd_to_preview_ast` call; [`CaptureSpliceStage`]
    // inside the q2-preview pipeline folds the splices. The non-preview
    // branch ignores it.
    captures: Vec<quarto_trace::EngineCapture>,
    attribution_json: Option<String>,
) -> String {
    use quarto_core::project::orchestrator::{ProjectPipeline, RenderMode, project_type_for};
    use quarto_core::project::pass2_renderer::{
        Pass2Payload, RenderToHtmlRenderer, RenderToPreviewAstRenderer,
    };

    // Detect format from the *active* file's content. (Pass 1 in
    // the orchestrator re-reads each file's bytes — for the active
    // file, those bytes are the freshly-edited buffer in VFS.)
    let active_bytes = match get_runtime().file_read(active_path) {
        Ok(b) => b,
        Err(e) => return error_response(format!("Failed to read active file: {}", e)),
    };
    let content_str = std::str::from_utf8(&active_bytes).unwrap_or("");
    let detected = detect_format_from_content(content_str);
    let format_str = if prefer_preview_format {
        map_format_for_preview(&detected).to_string()
    } else {
        detected
    };
    let format = match Format::from_format_string(&format_str) {
        Ok(f) => f,
        Err(e) => return error_response(e),
    };

    // Wrap once so both pipeline branches can clone the same `Rc`
    // into their renderer (bd-izfv). Plan 1's dispatch-on-`pipeline_kind`
    // refactor moved the `RenderToHtmlRenderer::new(...)` call into the
    // `_ =>` arm; the bd-izfv fix lives here as the per-renderer
    // attachment.
    let user_grammars_rc: Option<Rc<RefCell<JsUserGrammars>>> =
        user_grammars.map(|g| Rc::new(RefCell::new(g)));

    let project_type = project_type_for(&project);

    // Canonicalize the active path so it matches the form
    // `project.files` was filled with (Pass-2's `RenderMode::ActivePage`
    // filter compares absolute-path equality against `DocumentInfo.input`).
    let active_canonical = get_runtime()
        .canonicalize(active_path)
        .unwrap_or_else(|_| active_path.to_path_buf());

    let runtime_arc: Arc<dyn SystemRuntime> =
        Arc::clone(get_runtime_arc()) as Arc<dyn SystemRuntime>;

    // Dispatch on `format.pipeline_kind`. `pipeline_kind` is
    // `Option<&'static str>` (Copy), so reading it doesn't move
    // `format`, leaving it free to consume in the chosen branch.
    // Both renderers have `type Output = WasmPassTwoOutput;`
    // (commit 2's enum-payload payoff), so the resulting
    // `ProjectRenderSummary<WasmPassTwoOutput>` unifies across
    // arms and the downstream summary-handling code is shared.
    let kind = format.pipeline_kind;
    // Captured before `format` is moved into the pipeline below; signals the
    // SPA to render the AST with a reveal shell (Phase 1P).
    let is_slides = format.target_format == "q2-slides";
    let summary = match kind {
        Some("preview") => {
            let mut renderer = RenderToPreviewAstRenderer::new("/.quarto/project-artifacts");
            // bd-lucp / bd-5yff4: when captures are present, attach the
            // sequence to the renderer so `CaptureSpliceStage` (inserted
            // by `build_q2_preview_pipeline_stages`) can fold the recorded
            // engine output blocks into each per-doc AST.
            if !captures.is_empty() {
                renderer = renderer.with_captures(captures);
            }
            if let Some(json) = attribution_json {
                renderer = renderer.with_attribution(json);
            }
            let mut pipeline = ProjectPipeline::with_renderer(
                &mut project,
                project_type,
                format,
                format_str,
                runtime_arc,
                renderer,
            )
            .with_mode(RenderMode::ActivePage(active_canonical.clone()));
            match pipeline.run().await {
                Ok(s) => s,
                Err(e) => return render_error_response(e),
            }
        }
        _ => {
            let mut renderer = RenderToHtmlRenderer::new("/.quarto/project-artifacts");
            if let Some(ref provider) = user_grammars_rc {
                renderer = renderer.with_user_grammars(provider.clone());
            }
            let mut pipeline = ProjectPipeline::with_renderer(
                &mut project,
                project_type,
                format,
                format_str,
                runtime_arc,
                renderer,
            )
            .with_mode(RenderMode::ActivePage(active_canonical.clone()));
            match pipeline.run().await {
                Ok(s) => s,
                Err(e) => return render_error_response(e),
            }
        }
    };

    // Locate the active page's output. With `RenderMode::ActivePage`
    // the orchestrator emits exactly one entry — the active page.
    let active_output = match summary.outputs.into_iter().next() {
        Some(o) => o,
        None => {
            // No output. Three reasons in priority order:
            //   1. The active page itself failed Pass-1 (parse
            //      error / metadata error). The orchestrator
            //      drops the page from the index and Pass-2
            //      never sees it. Surface the structured
            //      diagnostics so the overlay can show the
            //      parse error instead of a generic message
            //      (bd-mwtf).
            //   2. Pass-2 failed (rare for the active-page
            //      mode but possible for renderer errors).
            //   3. Genuinely empty — fall through to the
            //      catch-all message.
            if let Some(failure) = summary
                .pass1_failures
                .iter()
                .find(|f| f.input == active_canonical)
            {
                return pass_failure_response("Pass 1", failure);
            }
            if let Some(failure) = summary.pass2_failures.into_iter().next() {
                return error_response(format!(
                    "Pass 2 failed for {}: {}",
                    failure.input.display(),
                    failure.error
                ));
            }
            return error_response("Project render produced no output for the active page");
        }
    };

    // Populate VFS with the active page's Page-scoped artifacts.
    // (Project-scoped artifacts were already flushed to VFS by
    // `WebsiteProjectType::post_render` → `flush_site_libs` via
    // the WASM renderer's vfs_root resolver.)
    //
    // The shared flush carries the bd-3gtn empty-content skip and the
    // bd-q3bxnq2e byte-identical-skip; see
    // `quarto_core::artifact_flush` and the matching call in
    // `render_single_doc_to_response`.
    let runtime = get_runtime();
    let resolver = ResourceResolverContext::vfs_root("/.quarto/project-artifacts");
    runtime.with_vfs_mut(|vfs| {
        quarto_core::flush_artifacts_to_vfs(&active_output.page_artifacts, &resolver, vfs);
    });

    // Per-page diagnostics + project-level diagnostics flow into a
    // single `warnings` array (Phase 9 §Decision 12). The JS layer
    // converts these to Monaco markers.
    let mut all_diags = active_output.diagnostics.clone();
    all_diags.extend(summary.project_diagnostics);
    let warnings = diagnostics_to_json(&all_diags, &active_output.source_context);

    // Plan 2A item 11: theme fingerprint is captured at the
    // pass-2 renderer level (before drain_project_scoped) and
    // stashed on `WasmPassTwoOutput`. Surviving both website-merge
    // and default-project-flush paths.
    let theme_fingerprint_from_output = active_output.theme_fingerprint.clone();

    // Pass-1 failures for non-active-page files (bd-rqba). The
    // active page's own Pass-1 failure shortcuts above via
    // `pass_failure_response`, so anything reaching this branch
    // belongs to a sibling. Surface them as a dedicated
    // `pass1_failures` field — D1 in the plan: engine stays
    // policy-free, the hub-client / preview consumer renders them
    // as warnings while the CLI consumer (bd-creo) treats them
    // as a non-zero exit.
    let pass1_failures = pass1_failures_to_json(&summary.pass1_failures);

    // Dispatch on the renderer's payload shape: HTML render fills
    // `html`, q2-preview render (added in a later commit) fills
    // `ast_json`. The shared orchestrator code above doesn't care
    // which.
    let (html, ast_json) = match active_output.payload {
        Pass2Payload::Html(s) => (Some(s), None),
        Pass2Payload::AstJson(s) => (None, Some(s)),
    };

    serde_json::to_string(&RenderResponse {
        success: true,
        error: None,
        html,
        ast_json,
        is_slides,
        diagnostics: None,
        warnings: if warnings.is_empty() {
            None
        } else {
            Some(warnings)
        },
        pass1_failures: if pass1_failures.is_empty() {
            None
        } else {
            Some(pass1_failures)
        },
        theme_fingerprint: theme_fingerprint_from_output,
    })
    .unwrap()
}

/// Build a `success: false` response with no diagnostics.
fn error_response(msg: impl Into<String>) -> String {
    serde_json::to_string(&RenderResponse {
        success: false,
        error: Some(msg.into()),
        html: None,
        ast_json: None,
        is_slides: false,
        diagnostics: None,
        warnings: None,
        pass1_failures: None,
        theme_fingerprint: None,
    })
    .unwrap()
}

/// Build a `success: false` response, attaching parser diagnostics
/// when available.
fn render_error_response(e: QuartoError) -> String {
    let (error_msg, diagnostics) = match &e {
        QuartoError::Parse(parse_error) => {
            let diags = diagnostics_to_json(&parse_error.diagnostics, &parse_error.source_context);
            (e.to_string(), Some(diags))
        }
        _ => (e.to_string(), None),
    };
    serde_json::to_string(&RenderResponse {
        success: false,
        error: Some(error_msg),
        html: None,
        ast_json: None,
        is_slides: false,
        diagnostics,
        warnings: None,
        pass1_failures: None,
        theme_fingerprint: None,
    })
    .unwrap()
}

/// Build a `success: false` response from a [`FileFailure`] when a
/// project-render pass dropped the active page (Pass-1 parse error
/// or Pass-2 render error). The structured diagnostics, when
/// present, become Monaco markers + the in-app preview overlay
/// snippet (bd-mwtf).
///
/// `phase` is a short label like "Pass 1" / "Pass 2" prepended to
/// the error string.
fn pass_failure_response(
    phase: &str,
    failure: &quarto_core::project::orchestrator::FileFailure,
) -> String {
    let diagnostics = match &failure.source_context {
        Some(ctx) if !failure.diagnostics.is_empty() => {
            Some(diagnostics_to_json(&failure.diagnostics, ctx))
        }
        _ => None,
    };
    serde_json::to_string(&RenderResponse {
        success: false,
        error: Some(format!(
            "{} failed for {}: {}",
            phase,
            failure.input.display(),
            failure.error
        )),
        html: None,
        ast_json: None,
        is_slides: false,
        diagnostics,
        warnings: None,
        pass1_failures: None,
        theme_fingerprint: None,
    })
    .unwrap()
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
// QMD PARSING AND AST CONVERSION API
// =============================================================================

/// Response type for parse/write operations.
#[derive(Serialize)]
struct AstResponse {
    success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    ast: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    qmd: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    /// Structured diagnostics (errors) with line/column information for Monaco.
    #[serde(skip_serializing_if = "Option::is_none")]
    diagnostics: Option<Vec<JsonDiagnostic>>,
    /// Structured warnings with line/column information for Monaco.
    #[serde(skip_serializing_if = "Option::is_none")]
    warnings: Option<Vec<JsonDiagnostic>>,
}

/// Parse QMD content and return the Pandoc JSON AST.
///
/// This is the WASM equivalent of `pampa -f qmd -t json`.
///
/// # Arguments
/// * `content` - QMD source text
///
/// # Returns
/// JSON: `{ "success": true, "ast": "<json-ast-string>" }`
/// or `{ "success": false, "error": "...", "diagnostics": [...] }`
///
/// The `ast` field contains the JSON-serialized Pandoc AST with source info,
/// matching the `RustQmdJson` TypeScript type from `@quarto/annotated-qmd`.
#[wasm_bindgen]
pub fn parse_qmd_content(content: &str) -> String {
    use pampa::wasm_entry_points::qmd_to_pandoc;
    use pampa::writers::json::{JsonConfig, write_with_config};

    match qmd_to_pandoc(content.as_bytes()) {
        Ok((pandoc, context)) => {
            let mut buf = Vec::new();
            let config = JsonConfig {
                include_inline_locations: false,
                ..JsonConfig::default()
            };
            match write_with_config(&pandoc, &context, &mut buf, &config) {
                Ok(_) => {
                    let ast_json = String::from_utf8(buf).unwrap_or_default();
                    serde_json::to_string(&AstResponse {
                        success: true,
                        ast: Some(ast_json),
                        qmd: None,
                        error: None,
                        diagnostics: None,
                        warnings: None,
                    })
                    .unwrap()
                }
                Err(diags) => {
                    let diagnostics = diagnostics_to_json(&diags, &context.source_context);
                    serde_json::to_string(&AstResponse {
                        success: false,
                        ast: None,
                        qmd: None,
                        error: Some("Failed to serialize AST to JSON".to_string()),
                        diagnostics: Some(diagnostics),
                        warnings: None,
                    })
                    .unwrap()
                }
            }
        }
        Err(error_strings) => {
            // qmd_to_pandoc returns Vec<String> for backward compat
            let error_msg = error_strings.join("\n");
            serde_json::to_string(&AstResponse {
                success: false,
                ast: None,
                qmd: None,
                error: Some(error_msg),
                diagnostics: None,
                warnings: None,
            })
            .unwrap()
        }
    }
}

/// Convert a Pandoc JSON AST back to QMD source text.
///
/// This is the WASM equivalent of `pampa -f json -t qmd`.
///
/// # Arguments
/// * `ast_json` - JSON-serialized Pandoc AST (as produced by `parse_qmd_content`)
///
/// # Returns
/// JSON: `{ "success": true, "qmd": "<qmd-text>" }`
/// or `{ "success": false, "error": "..." }`
#[wasm_bindgen]
pub fn ast_to_qmd(ast_json: &str) -> String {
    use pampa::readers::json::read as json_read;
    use pampa::writers::qmd::write as qmd_write;

    let mut cursor = std::io::Cursor::new(ast_json.as_bytes());
    match json_read(&mut cursor) {
        Ok((pandoc, _context)) => {
            let mut buf = Vec::new();
            match qmd_write(&pandoc, &mut buf) {
                Ok(_) => {
                    let qmd_text = String::from_utf8(buf).unwrap_or_default();
                    serde_json::to_string(&AstResponse {
                        success: true,
                        ast: None,
                        qmd: Some(qmd_text),
                        error: None,
                        diagnostics: None,
                        warnings: None,
                    })
                    .unwrap()
                }
                Err(diags) => {
                    let error_msg = diags
                        .iter()
                        .map(|d| d.to_text(None))
                        .collect::<Vec<_>>()
                        .join("\n");
                    serde_json::to_string(&AstResponse {
                        success: false,
                        ast: None,
                        qmd: None,
                        error: Some(format!("Failed to write QMD: {}", error_msg)),
                        diagnostics: None,
                        warnings: None,
                    })
                    .unwrap()
                }
            }
        }
        Err(e) => serde_json::to_string(&AstResponse {
            success: false,
            ast: None,
            qmd: None,
            error: Some(format!("Failed to parse JSON AST: {}", e)),
            diagnostics: None,
            warnings: None,
        })
        .unwrap(),
    }
}

/// Incrementally write a modified AST back to QMD, preserving unchanged
/// portions of the original source text verbatim.
///
/// Deserializes the caller-supplied **baseline** AST (the AST whose
/// source spans line up byte-for-byte with `original_qmd`) and computes
/// a reconciliation plan against the new AST. Plan 7 removed the
/// internal re-parse: previously the bridge re-parsed `original_qmd`
/// to recover spans, which lost any provenance the host had already
/// attached to the baseline (e.g. `preimage_in` after a prior
/// incremental edit). Now the caller is responsible for the
/// baseline-tier contract.
///
/// # Arguments
/// * `original_qmd` - The original QMD source text
/// * `baseline_ast_json` - JSON-serialized Pandoc AST whose source
///   spans correspond to `original_qmd`. **Must be the same tier as
///   `new_ast_json`** (e.g. both `parse`-tier or both
///   `parse+sugar`-tier). Mixing tiers will mis-anchor reconciliation
///   and corrupt the write.
/// * `new_ast_json` - JSON-serialized Pandoc AST representing the
///   modified document, in the same tier as `baseline_ast_json`.
///
/// # Returns
/// JSON: `{ "success": true, "qmd": "<result-qmd-text>" }`
/// or `{ "success": false, "error": "...", "diagnostics": [...] }`
#[wasm_bindgen]
pub fn incremental_write_qmd(original_qmd: &str, new_ast_json: &str) -> String {
    use pampa::readers::json::read as json_read;
    use pampa::wasm_entry_points::qmd_to_pandoc;
    use pampa::writers::incremental::incremental_write;
    use quarto_ast_reconcile::compute_reconciliation;

    // Step 1: Parse original QMD to get AST with accurate source spans
    let (original_ast, _original_context) = match qmd_to_pandoc(original_qmd.as_bytes()) {
        Ok(result) => result,
        Err(error_strings) => {
            let error_msg = error_strings.join("\n");
            return serde_json::to_string(&AstResponse {
                success: false,
                ast: None,
                qmd: None,
                error: Some(format!("Failed to parse original QMD: {}", error_msg)),
                diagnostics: None,
                warnings: None,
            })
            .unwrap();
        }
    };

    // Step 2: Deserialize new AST from JSON
    let mut cursor = std::io::Cursor::new(new_ast_json.as_bytes());
    let (new_ast, _new_context) = match json_read(&mut cursor) {
        Ok(result) => result,
        Err(e) => {
            return serde_json::to_string(&AstResponse {
                success: false,
                ast: None,
                qmd: None,
                error: Some(format!("Failed to parse new AST JSON: {}", e)),
                diagnostics: None,
                warnings: None,
            })
            .unwrap();
        }
    };

    // Step 3: Compute reconciliation plan
    let plan = compute_reconciliation(&original_ast, &new_ast);

    // Step 4: Incremental write
    match incremental_write(original_qmd, &original_ast, &new_ast, &plan) {
        Ok(result_qmd) => serde_json::to_string(&AstResponse {
            success: true,
            ast: None,
            qmd: Some(result_qmd),
            error: None,
            diagnostics: None,
            warnings: None,
        })
        .unwrap(),
        Err(diags) => {
            let error_msg = diags
                .iter()
                .map(|d| d.to_text(None))
                .collect::<Vec<_>>()
                .join("\n");
            serde_json::to_string(&AstResponse {
                success: false,
                ast: None,
                qmd: None,
                error: Some(format!("Incremental write failed: {}", error_msg)),
                diagnostics: None,
                warnings: None,
            })
            .unwrap()
        }
    }
}

// ============================================================================
// TEMPLATE PROCESSING
// ============================================================================

/// Response type for template preparation.
#[derive(Serialize)]
struct PrepareTemplateResponse {
    success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    template_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stripped_content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

impl PrepareTemplateResponse {
    fn success(template_name: Option<String>, stripped_content: String) -> String {
        serde_json::to_string(&PrepareTemplateResponse {
            success: true,
            template_name,
            stripped_content: Some(stripped_content),
            error: None,
        })
        .unwrap()
    }

    fn error(msg: &str) -> String {
        serde_json::to_string(&PrepareTemplateResponse {
            success: false,
            template_name: None,
            stripped_content: None,
            error: Some(msg.to_string()),
        })
        .unwrap()
    }
}

/// Process a template file: extract template-name and produce stripped content.
///
/// This function parses a QMD template file, extracts the `template-name` metadata
/// field (if present), removes it from the document metadata, and re-serializes
/// the document to QMD format.
///
/// # Arguments
/// * `content` - The QMD source text of the template
///
/// # Returns
/// JSON response:
/// - Success: `{ "success": true, "template_name": "..." | null, "stripped_content": "..." }`
/// - Error: `{ "success": false, "error": "..." }`
///
/// The `template_name` field is `null` if no `template-name` metadata was found.
/// The `stripped_content` contains the template with `template-name` removed from
/// the YAML frontmatter.
#[wasm_bindgen]
pub fn prepare_template(content: &str) -> String {
    use pampa::wasm_entry_points::qmd_to_pandoc;
    use pampa::writers::qmd::write as qmd_write;

    // Step 1: Parse QMD to Pandoc AST
    let (mut pandoc, _context) = match qmd_to_pandoc(content.as_bytes()) {
        Ok(result) => result,
        Err(error_strings) => {
            return PrepareTemplateResponse::error(&format!(
                "Failed to parse template: {}",
                error_strings.join("; ")
            ));
        }
    };

    // Step 2: Extract and remove template-name from metadata
    let template_name = extract_and_remove_template_name(&mut pandoc.meta);

    // Step 3: Re-serialize to QMD
    let mut buf = Vec::new();
    match qmd_write(&pandoc, &mut buf) {
        Ok(_) => {}
        Err(diags) => {
            let error_msg = diags
                .iter()
                .map(|d| d.to_text(None))
                .collect::<Vec<_>>()
                .join("\n");
            return PrepareTemplateResponse::error(&format!(
                "Failed to write template: {}",
                error_msg
            ));
        }
    }

    let stripped_content = match String::from_utf8(buf) {
        Ok(s) => s,
        Err(e) => {
            return PrepareTemplateResponse::error(&format!("Invalid UTF-8 in output: {}", e));
        }
    };

    PrepareTemplateResponse::success(template_name, stripped_content)
}

/// Extract the `template-name` field from metadata and remove it.
///
/// If the metadata is a Map and contains a `template-name` key, this function:
/// 1. Extracts the value as plain text
/// 2. Removes the entry from the map
/// 3. Returns the extracted value
///
/// Returns `None` if metadata is not a Map or doesn't contain `template-name`.
fn extract_and_remove_template_name(meta: &mut ConfigValue) -> Option<String> {
    use quarto_pandoc_types::config_value::ConfigValueKind;

    if let ConfigValueKind::Map(entries) = &mut meta.value {
        // Find and extract the template-name value
        let mut template_name = None;

        // Find the index of template-name entry
        let idx = entries.iter().position(|e| e.key == "template-name");

        if let Some(idx) = idx {
            // Extract the value before removing
            template_name = entries[idx].value.as_plain_text();
            // Remove the entry
            entries.remove(idx);
        }

        template_name
    } else {
        None
    }
}
