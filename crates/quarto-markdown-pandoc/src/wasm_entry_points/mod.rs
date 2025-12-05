/*
 * mod.rs
 * Copyright (c) 2025 Posit, PBC
 */

use crate::readers;
use crate::utils::output::VerboseOutput;
use crate::writers::json::JsonConfig;
use std::io;

fn pandoc_to_json(
    doc: &crate::pandoc::Pandoc,
    context: &crate::pandoc::ast_context::ASTContext,
    include_resolved_locations: bool,
) -> Result<String, String> {
    let mut buf = Vec::new();
    let config = JsonConfig {
        include_inline_locations: include_resolved_locations,
    };
    match crate::writers::json::write_with_config(doc, context, &mut buf, &config) {
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

pub fn qmd_to_pandoc(
    input: &[u8],
) -> Result<
    (
        crate::pandoc::Pandoc,
        crate::pandoc::ast_context::ASTContext,
    ),
    Vec<String>,
> {
    let mut output = VerboseOutput::Sink(io::sink());
    match readers::qmd::read(input, false, "<input>", &mut output, true, None) {
        Ok((pandoc, context, _warnings)) => {
            // TODO: Decide how to handle warnings in WASM context
            Ok((pandoc, context))
        }
        Err(diagnostics) => {
            // Convert diagnostics to strings for backward compatibility
            Err(diagnostics.iter().map(|d| d.to_text(None)).collect())
        }
    }
}

pub fn parse_qmd(input: &[u8], include_resolved_locations: bool) -> String {
    let (pandoc, context) = qmd_to_pandoc(input).unwrap();
    pandoc_to_json(&pandoc, &context, include_resolved_locations).unwrap()
}

/// Render a parsed document using a template bundle.
///
/// This function is designed for WASM usage where filesystem access is not available.
/// The template is provided as a JSON bundle containing the main template and any partials.
///
/// # Arguments
///
/// * `pandoc` - The parsed Pandoc document
/// * `context` - The AST context from parsing
/// * `bundle_json` - JSON string containing the template bundle
/// * `body_format` - "html" or "plaintext"
///
/// # Returns
///
/// A JSON object with either:
/// * `{ "output": "..." }` on success
/// * `{ "error": "...", "diagnostics": [...] }` on failure
pub fn render_with_template_bundle(
    pandoc: &crate::pandoc::Pandoc,
    context: &crate::pandoc::ast_context::ASTContext,
    bundle_json: &str,
    body_format: &str,
) -> String {
    use crate::template::{render_with_bundle, BodyFormat, TemplateBundle};

    // Parse the bundle
    let bundle = match TemplateBundle::from_json(bundle_json) {
        Ok(b) => b,
        Err(e) => {
            return serde_json::json!({
                "error": format!("Failed to parse template bundle: {}", e),
                "diagnostics": []
            })
            .to_string();
        }
    };

    // Determine body format
    let format = match body_format {
        "html" => BodyFormat::Html,
        "plaintext" | "plain" => BodyFormat::Plaintext,
        _ => {
            return serde_json::json!({
                "error": format!("Unknown body format: '{}'. Use 'html' or 'plaintext'", body_format),
                "diagnostics": []
            })
            .to_string();
        }
    };

    // Render
    match render_with_bundle(pandoc, context, &bundle, format) {
        Ok((output, diagnostics)) => {
            let diag_json: Vec<serde_json::Value> = diagnostics
                .iter()
                .map(|d| serde_json::json!({"message": d.to_text(None)}))
                .collect();
            serde_json::json!({
                "output": output,
                "diagnostics": diag_json
            })
            .to_string()
        }
        Err(e) => {
            serde_json::json!({
                "error": format!("Template render error: {}", e),
                "diagnostics": []
            })
            .to_string()
        }
    }
}

/// Parse QMD input and render with a template bundle in one step.
///
/// This is a convenience function combining `qmd_to_pandoc` and `render_with_template_bundle`.
///
/// # Arguments
///
/// * `input` - QMD source text as bytes
/// * `bundle_json` - JSON string containing the template bundle
/// * `body_format` - "html" or "plaintext"
///
/// # Returns
///
/// A JSON object with either:
/// * `{ "output": "..." }` on success
/// * `{ "error": "...", "diagnostics": [...] }` on failure
pub fn parse_and_render_qmd(input: &[u8], bundle_json: &str, body_format: &str) -> String {
    match qmd_to_pandoc(input) {
        Ok((pandoc, context)) => {
            render_with_template_bundle(&pandoc, &context, bundle_json, body_format)
        }
        Err(errors) => {
            serde_json::json!({
                "error": "Failed to parse QMD input",
                "diagnostics": errors.iter().map(|e| serde_json::json!({"message": e})).collect::<Vec<_>>()
            })
            .to_string()
        }
    }
}

/// Get a built-in template as a JSON bundle.
///
/// Available templates: "html5", "plain"
///
/// # Returns
///
/// A JSON object with either:
/// * The template bundle JSON on success
/// * `{ "error": "..." }` on failure
pub fn get_builtin_template_json(name: &str) -> String {
    use crate::template::builtin::{get_builtin_template, BUILTIN_TEMPLATE_NAMES};

    match get_builtin_template(name) {
        Some(bundle) => match bundle.to_json() {
            Ok(json) => json,
            Err(e) => {
                serde_json::json!({
                    "error": format!("Failed to serialize template: {}", e)
                })
                .to_string()
            }
        },
        None => {
            serde_json::json!({
                "error": format!("Unknown built-in template: '{}'. Available: {}", name, BUILTIN_TEMPLATE_NAMES.join(", "))
            })
            .to_string()
        }
    }
}
