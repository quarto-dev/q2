/**
 * WASM-JS Bridge for Template Rendering
 *
 * This module provides JavaScript template rendering functions that are called
 * from the Rust WASM code via wasm-bindgen. The functions are imported by
 * quarto-system-runtime/src/wasm.rs using:
 *
 *   #[wasm_bindgen(raw_module = "/src/wasm-js-bridge/template.js")]
 *
 * All functions receive data as JSON strings to avoid complex type marshalling
 * between Rust and JavaScript.
 */

import ejs from "ejs";

/**
 * Check if the template bridge is available.
 *
 * This allows the Rust code to gracefully handle cases where the JS bridge
 * is not properly loaded.
 *
 * @returns {boolean} Always returns true when this module is loaded
 */
export function jsTemplateAvailable() {
  return true;
}

/**
 * Render a simple template with ${key} placeholders.
 *
 * This is a lightweight template system for simple variable substitution.
 * It does NOT use JavaScript template literals (no eval).
 *
 * @param {string} template - Template string with ${key} placeholders
 * @param {string} dataJson - JSON-encoded object with key-value pairs
 * @returns {Promise<string>} Rendered template string
 *
 * @example
 * jsRenderSimpleTemplate("Hello, ${name}!", '{"name": "World"}')
 * // Returns: Promise that resolves to "Hello, World!"
 */
export function jsRenderSimpleTemplate(template, dataJson) {
  return new Promise((resolve, reject) => {
    try {
      const data = JSON.parse(dataJson);
      // Replace ${key} patterns with values from data
      // Missing keys are replaced with empty string
      const result = template.replace(/\$\{(\w+)\}/g, (_, key) => {
        return key in data ? String(data[key]) : "";
      });
      resolve(result);
    } catch (e) {
      reject(e instanceof Error ? e : new Error(String(e)));
    }
  });
}

/**
 * Render an EJS template with the given data.
 *
 * EJS is a simple templating language that lets you generate HTML/text
 * with plain JavaScript. See https://ejs.co/ for syntax reference.
 *
 * @param {string} template - EJS template string
 * @param {string} dataJson - JSON-encoded object with template data
 * @returns {Promise<string>} Rendered template string
 *
 * @example
 * jsRenderEjs("<%= name %>", '{"name": "World"}')
 * // Returns: Promise that resolves to "World"
 *
 * @example
 * jsRenderEjs("<% if (show) { %>Visible<% } %>", '{"show": true}')
 * // Returns: Promise that resolves to "Visible"
 */
export function jsRenderEjs(template, dataJson) {
  return new Promise((resolve, reject) => {
    try {
      const data = JSON.parse(dataJson);
      const options = {
        // Enable debug info for better error messages
        compileDebug: true,
        // Don't strip whitespace (preserve formatting)
        rmWhitespace: false,
      };
      const result = ejs.render(template, data, options);
      resolve(result);
    } catch (e) {
      reject(e instanceof Error ? e : new Error(String(e)));
    }
  });
}
