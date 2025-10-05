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

use quarto_markdown_pandoc::readers;
use quarto_markdown_pandoc::wasm_entry_points;
use quarto_markdown_pandoc::writers;
use wasm_bindgen::prelude::*;

#[wasm_bindgen(start)]
pub fn run() {
    // Set a panic hook on program start that prints panics to the console
    panic::set_hook(Box::new(console_error_panic_hook::hook));
}

fn json_to_pandoc(input: &str) -> Result<(quarto_markdown_pandoc::pandoc::Pandoc, quarto_markdown_pandoc::pandoc::ASTContext), String> {
    match readers::json::read(&mut input.as_bytes()) {
        Ok(doc) => Ok(doc),
        Err(err) => Err(format!("Unable to read as json: {:?}", err)),
    }
}

fn pandoc_to_json(doc: &quarto_markdown_pandoc::pandoc::Pandoc, context: &quarto_markdown_pandoc::pandoc::ASTContext) -> Result<String, String> {
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

fn pandoc_to_qmd(doc: &quarto_markdown_pandoc::pandoc::Pandoc) -> Result<String, String> {
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
pub fn parse_qmd(input: JsValue) -> JsValue {
    let input = as_string(&input, "input");
    let json = wasm_entry_points::parse_qmd(input.as_bytes());
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
        "qmd" => wasm_entry_points::qmd_to_pandoc(&input.as_bytes()).unwrap(),
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
