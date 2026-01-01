/*
 * lib.rs
 * Copyright (c) 2025 Posit, PBC
 */

// For `vsnprintf()` and `fprintf()`, which are variadic.
// Otherwise rustc yells at us that we need to enable this.
#![feature(c_variadic)]

// Provide rust implementation of blessed stdlib functions to
// tree-sitter itself and any grammars that have `scanner.c`.
// Here is the list blessed for `scanner.c` usage:
// https://github.com/tree-sitter/tree-sitter/blob/master/lib/src/wasm/stdlib-symbols.txt
// But note that we need a few extra for tree-sitter itself.
#[cfg(target_arch = "wasm32")]
pub mod c_shim;

mod utils;

use std::panic;

use pampa::readers;
use pampa::wasm_entry_points;
use pampa::writers;
use wasm_bindgen::prelude::*;

#[wasm_bindgen(start)]
pub fn run() {
    // Set a panic hook on program start that prints panics to the console
    panic::set_hook(Box::new(console_error_panic_hook::hook));
}

fn json_to_pandoc(
    input: &str,
) -> Result<(pampa::pandoc::Pandoc, pampa::pandoc::ASTContext), String> {
    match readers::json::read(&mut input.as_bytes()) {
        Ok(doc) => Ok(doc),
        Err(err) => Err(format!("Unable to read as json: {:?}", err)),
    }
}

fn pandoc_to_json(
    doc: &pampa::pandoc::Pandoc,
    context: &pampa::pandoc::ASTContext,
) -> Result<String, String> {
    let mut buf = Vec::new();
    match writers::json::write(doc, context, &mut buf) {
        Ok(_) => {
            // Nothing to do
        }
        Err(err) => {
            return Err(format!("Unable to write as json: {:?}", err));
        }
    }

    match String::from_utf8(buf) {
        Ok(json) => Ok(json),
        Err(err) => Err(format!("Unable to convert json to string: {:?}", err)),
    }
}

fn pandoc_to_qmd(doc: &pampa::pandoc::Pandoc) -> Result<String, String> {
    let mut buf = Vec::new();
    match writers::qmd::write(doc, &mut buf) {
        Ok(_) => {
            // Nothing to do
        }
        Err(err) => {
            return Err(format!("Unable to write as qmd: {:?}", err));
        }
    }

    match String::from_utf8(buf) {
        Ok(qmd) => Ok(qmd),
        Err(err) => Err(format!("Unable to convert qmd to string: {:?}", err)),
    }
}

#[wasm_bindgen]
pub fn parse_qmd(input: JsValue, include_resolved_locations: JsValue) -> JsValue {
    let input = as_string(&input, "input");
    let include_resolved_locations = as_string(&include_resolved_locations, "input") == "true";
    let json = wasm_entry_points::parse_qmd(input.as_bytes(), include_resolved_locations);
    JsValue::from_str(&json)
}

#[wasm_bindgen]
pub fn write_qmd(input: JsValue) -> JsValue {
    let input = as_string(&input, "input");
    let (result, context) = json_to_pandoc(&input).unwrap();

    let json = pandoc_to_json(&result, &context).unwrap();
    JsValue::from_str(&json)
}

#[wasm_bindgen]
pub fn convert(document: JsValue, input_format: JsValue, output_format: JsValue) -> JsValue {
    let input = as_string(&document, "document");
    let input_format = as_string(&input_format, "input_format");
    let output_format = as_string(&output_format, "output_format");
    let (doc, context) = match input_format.as_str() {
        "qmd" => wasm_entry_points::qmd_to_pandoc(input.as_bytes()).unwrap(),
        "json" => json_to_pandoc(&input).unwrap(),
        _ => panic!("Unsupported input format: {}", input_format),
    };
    let output = match output_format.as_str() {
        "qmd" => pandoc_to_qmd(&doc).unwrap(),
        "json" => pandoc_to_json(&doc, &context).unwrap(),
        _ => panic!("Unsupported output format: {}", output_format),
    };
    JsValue::from_str(&output)
}

fn as_string(value: &JsValue, name: &str) -> String {
    match value.as_string() {
        Some(s) => s,
        None => panic!("Unable to parse `{}` as a `String`.", name),
    }
}

/// Render a QMD document with a template bundle.
///
/// # Arguments
/// * `input` - QMD source text
/// * `bundle_json` - Template bundle as JSON string
/// * `body_format` - "html" or "plaintext"
///
/// # Returns
/// JSON object with `{ "output": "..." }` or `{ "error": "...", "diagnostics": [...] }`
#[wasm_bindgen]
pub fn render_with_template(input: JsValue, bundle_json: JsValue, body_format: JsValue) -> JsValue {
    let input = as_string(&input, "input");
    let bundle_json = as_string(&bundle_json, "bundle_json");
    let body_format = as_string(&body_format, "body_format");

    let result =
        wasm_entry_points::parse_and_render_qmd(input.as_bytes(), &bundle_json, &body_format);
    JsValue::from_str(&result)
}

/// Get a built-in template as a JSON bundle.
///
/// # Arguments
/// * `name` - Template name ("html5" or "plain")
///
/// # Returns
/// Template bundle JSON or `{ "error": "..." }`
#[wasm_bindgen]
pub fn get_builtin_template(name: JsValue) -> JsValue {
    let name = as_string(&name, "name");
    let result = wasm_entry_points::get_builtin_template_json(&name);
    JsValue::from_str(&result)
}
