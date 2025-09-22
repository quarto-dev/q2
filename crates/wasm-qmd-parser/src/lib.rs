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

use std::{io, panic};

use quarto_markdown_pandoc::readers;
use quarto_markdown_pandoc::utils::output::VerboseOutput;
use quarto_markdown_pandoc::utils::tree_sitter_log_observer::TreeSitterLogObserver;
use quarto_markdown_pandoc::writers;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
    fn alert(s: &str);
}

#[wasm_bindgen]
pub fn greet() {
    alert("Hello, wasm-qmd-parser!");
}

#[wasm_bindgen(start)]
pub fn run() {
    // Set a panic hook on program start that prints panics to the console
    panic::set_hook(Box::new(console_error_panic_hook::hook));
}

fn pandoc_to_json(doc: &quarto_markdown_pandoc::pandoc::Pandoc) -> Result<String, String> {
    let mut buf = Vec::new();
    match writers::json::write(doc, &mut buf) {
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

#[wasm_bindgen]
pub fn parse_qmd(input: JsValue) -> JsValue {
    let mut output = VerboseOutput::Sink(io::sink());
    let input = match input.as_string() {
        Some(input) => input,
        None => panic!("Unable to parse `input` as a `String`."),
    };
    let result = match readers::qmd::read(
        input.as_bytes(),
        false,
        "<input>",
        &mut output,
        None::<fn(&[u8], &TreeSitterLogObserver, &str) -> Vec<String>>,
    ) {
        Ok(result) => result,
        Err(err) => panic!("Unable to read as a qmd:\n{}", err.join("\n")),
    };

    let json = pandoc_to_json(&result).unwrap();
    JsValue::from_str(&json)
}

#[wasm_bindgen]
pub fn write_qmd(input: JsValue) -> JsValue {
    let input = match input.as_string() {
        Some(input) => input,
        None => panic!("Unable to parse `input` as a `String`."),
    };
    let result = match readers::json::read(&mut input.as_bytes()) {
        Ok(result) => result,
        Err(err) => panic!("Unable to read as json:\n{}", err),
    };

    let json = pandoc_to_json(&result).unwrap();
    JsValue::from_str(&json)
}
