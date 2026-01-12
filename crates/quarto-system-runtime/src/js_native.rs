/*
 * js_native.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * JavaScript execution for native targets using deno_core.
 *
 * This module provides a V8-based JavaScript runtime for executing
 * JS code in native (non-WASM) environments. It's used for:
 * - Simple template rendering (interstitial test)
 * - EJS template rendering (project scaffolding)
 *
 * IMPORTANT: This module is internal implementation detail.
 * deno_core/V8 types MUST NOT leak into the public API.
 *
 * NOTE: Currently using minimal setup without deno_web due to
 * yanked dependency issues. EJS may have limited functionality.
 *
 * ## Performance Considerations
 *
 * Currently, a fresh JsEngine (V8 JsRuntime) is created for each JS operation.
 * This is because V8's JsRuntime is not Send+Sync, so it cannot be stored in
 * NativeRuntime which must implement SystemRuntime's Send+Sync bounds.
 *
 * This approach is adequate for project scaffolding (1-10 templates) but may
 * become problematic for large-scale operations (100-10,000 templates, e.g.,
 * listing pages across a large Quarto website).
 *
 * **For optimization strategies and migration paths, see:**
 * `claude-notes/plans/js-execution-performance.md`
 *
 * The recommended first optimization step (when needed) is thread-local storage,
 * which requires ~20 lines of change with no API impact.
 */

// This module is only compiled for non-WASM targets
#![cfg(not(target_arch = "wasm32"))]

use deno_core::v8;
use deno_core::{JsRuntime, RuntimeOptions};

use crate::traits::{RuntimeError, RuntimeResult};

/// The bundled simple template JavaScript
const SIMPLE_TEMPLATE_BUNDLE: &str = include_str!("../js/dist/simple-template-bundle.js");

/// The bundled EJS JavaScript (browserified)
const EJS_BUNDLE: &str = include_str!("../js/dist/ejs-bundle.js");

/// JavaScript engine wrapper for native runtime.
///
/// This struct manages a deno_core JsRuntime instance and provides
/// methods for executing JavaScript code safely.
///
/// Note: Creating a JsRuntime is expensive. Consider reusing instances
/// when rendering multiple templates.
pub struct JsEngine {
    runtime: JsRuntime,
    simple_template_loaded: bool,
    ejs_loaded: bool,
}

impl JsEngine {
    /// Create a new JavaScript engine.
    ///
    /// This initializes a minimal V8 runtime. For full web API support
    /// (TextEncoder, etc.), we would need deno_web which currently has
    /// dependency issues.
    pub fn new() -> RuntimeResult<Self> {
        // Using minimal setup due to deno_web dependency issues
        Self::new_minimal()
    }

    /// Create a minimal JavaScript engine without web extensions.
    ///
    /// This is lighter weight and suitable for simple template rendering
    /// that doesn't need TextEncoder, etc.
    pub fn new_minimal() -> RuntimeResult<Self> {
        let runtime = JsRuntime::new(RuntimeOptions::default());

        Ok(Self {
            runtime,
            simple_template_loaded: false,
            ejs_loaded: false,
        })
    }

    /// Render a simple template with ${key} placeholders.
    ///
    /// This loads the simple template bundle on first use.
    pub fn render_simple_template(
        &mut self,
        template: &str,
        data: &serde_json::Value,
    ) -> RuntimeResult<String> {
        // Load the simple template bundle if not already loaded
        if !self.simple_template_loaded {
            self.runtime
                .execute_script(
                    "<simple-template-bundle>",
                    SIMPLE_TEMPLATE_BUNDLE.to_string(),
                )
                .map_err(|e| {
                    RuntimeError::NotSupported(format!(
                        "Failed to load simple template bundle: {}",
                        e
                    ))
                })?;
            self.simple_template_loaded = true;
        }

        // Escape template and data for embedding in JavaScript
        let escaped_template = serde_json::to_string(template)
            .map_err(|e| RuntimeError::NotSupported(e.to_string()))?;
        let escaped_data =
            serde_json::to_string(data).map_err(|e| RuntimeError::NotSupported(e.to_string()))?;

        // Execute the render script
        let render_script = format!(
            r#"
            (function() {{
                try {{
                    const template = {escaped_template};
                    const data = {escaped_data};
                    const result = renderSimpleTemplate(template, data);
                    return {{ success: true, output: result }};
                }} catch (e) {{
                    return {{ success: false, error: e.toString() }};
                }}
            }})()
            "#,
        );

        self.eval_and_extract_result(&render_script)
    }

    /// Render an EJS template with the given data.
    ///
    /// This loads the EJS bundle on first use.
    pub fn render_ejs(
        &mut self,
        template: &str,
        data: &serde_json::Value,
    ) -> RuntimeResult<String> {
        // Load the EJS bundle if not already loaded
        if !self.ejs_loaded {
            self.runtime
                .execute_script("<ejs-bundle>", EJS_BUNDLE.to_string())
                .map_err(|e| {
                    RuntimeError::NotSupported(format!("Failed to load EJS bundle: {}", e))
                })?;
            self.ejs_loaded = true;
        }

        // Escape template and data for embedding in JavaScript
        let escaped_template = serde_json::to_string(template)
            .map_err(|e| RuntimeError::NotSupported(e.to_string()))?;
        let escaped_data =
            serde_json::to_string(data).map_err(|e| RuntimeError::NotSupported(e.to_string()))?;

        // Execute the render script using ejs.render()
        let render_script = format!(
            r#"
            (function() {{
                try {{
                    const template = {escaped_template};
                    const data = {escaped_data};
                    const options = {{
                        compileDebug: true,
                        rmWhitespace: false
                    }};
                    const result = ejs.render(template, data, options);
                    return {{ success: true, output: result }};
                }} catch (e) {{
                    return {{ success: false, error: e.toString() }};
                }}
            }})()
            "#,
        );

        self.eval_and_extract_result(&render_script)
    }

    /// Execute JavaScript and extract the result from the standardized
    /// { success: bool, output?: string, error?: string } format.
    fn eval_and_extract_result(&mut self, script: &str) -> RuntimeResult<String> {
        let global = self
            .runtime
            .execute_script("<eval>", script.to_string())
            .map_err(|e| RuntimeError::NotSupported(format!("Script execution failed: {}", e)))?;

        // Get a scope to work with the V8 value
        deno_core::scope!(scope, self.runtime);
        let local = v8::Local::new(scope, global);

        // Deserialize the V8 value to JSON
        let result: serde_json::Value = serde_v8::from_v8(scope, local).map_err(|e| {
            RuntimeError::NotSupported(format!("Failed to deserialize result: {}", e))
        })?;

        // Extract the result
        if result["success"].as_bool() == Some(true) {
            result["output"]
                .as_str()
                .map(|s| s.to_string())
                .ok_or_else(|| RuntimeError::NotSupported("Missing output in result".to_string()))
        } else {
            let error = result["error"].as_str().unwrap_or("Unknown error");
            Err(RuntimeError::NotSupported(format!(
                "Template rendering failed: {}",
                error
            )))
        }
    }
}

impl Default for JsEngine {
    fn default() -> Self {
        Self::new().expect("Failed to create default JsEngine")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_simple_template_basic() {
        let mut engine = JsEngine::new_minimal().unwrap();
        let result = engine
            .render_simple_template("Hello, ${name}!", &json!({"name": "World"}))
            .unwrap();
        assert_eq!(result, "Hello, World!");
    }

    #[test]
    fn test_simple_template_multiple_vars() {
        let mut engine = JsEngine::new_minimal().unwrap();
        let result = engine
            .render_simple_template(
                "${greeting}, ${name}! You have ${count} messages.",
                &json!({"greeting": "Hi", "name": "Alice", "count": 5}),
            )
            .unwrap();
        assert_eq!(result, "Hi, Alice! You have 5 messages.");
    }

    #[test]
    fn test_simple_template_missing_var() {
        let mut engine = JsEngine::new_minimal().unwrap();
        let result = engine
            .render_simple_template("Hello, ${name}! Your id is ${id}.", &json!({"name": "Bob"}))
            .unwrap();
        // Missing variables are replaced with empty string
        assert_eq!(result, "Hello, Bob! Your id is .");
    }

    #[test]
    fn test_simple_template_no_vars() {
        let mut engine = JsEngine::new_minimal().unwrap();
        let result = engine
            .render_simple_template("No variables here", &json!({}))
            .unwrap();
        assert_eq!(result, "No variables here");
    }

    #[test]
    fn test_ejs_basic() {
        let mut engine = JsEngine::new().unwrap();
        let result = engine
            .render_ejs("Hello, <%= name %>!", &json!({"name": "World"}))
            .unwrap();
        assert_eq!(result, "Hello, World!");
    }

    #[test]
    fn test_ejs_with_logic() {
        let mut engine = JsEngine::new().unwrap();
        let template = r#"<% if (show) { %>Visible<% } else { %>Hidden<% } %>"#;
        let result = engine.render_ejs(template, &json!({"show": true})).unwrap();
        assert_eq!(result, "Visible");
    }

    #[test]
    fn test_ejs_loop() {
        let mut engine = JsEngine::new().unwrap();
        let template = r#"<% items.forEach(function(item) { %><%= item %> <% }); %>"#;
        let result = engine
            .render_ejs(template, &json!({"items": ["a", "b", "c"]}))
            .unwrap();
        assert_eq!(result, "a b c ");
    }
}
