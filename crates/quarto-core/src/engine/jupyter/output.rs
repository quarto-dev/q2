/*
 * engine/jupyter/output.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Convert Jupyter outputs to Pandoc AST blocks.
 */

//! Convert Jupyter kernel outputs to Pandoc AST blocks.
//!
//! This module handles converting the various output types from Jupyter
//! (stream, display_data, execute_result, error) into Pandoc AST
//! elements that can be inserted into the document.

use hashlink::LinkedHashMap;

use quarto_pandoc_types::{
    AttrSourceInfo, Block, CodeBlock as CodeBlockStruct, Div, Inline, Paragraph, RawBlock, Str,
};
use quarto_source_map::SourceInfo;

use super::execute::{CellOutput, MimeBundle};

/// Create an Attr tuple with the given id and classes.
fn make_attr(id: impl Into<String>, classes: Vec<String>) -> quarto_pandoc_types::Attr {
    (id.into(), classes, LinkedHashMap::new())
}

/// Options for output conversion.
pub struct OutputOptions {
    /// Preferred image format for figures.
    pub image_format: ImageFormat,
    /// Whether to include raw HTML output.
    pub include_html: bool,
    /// Whether to include raw LaTeX output.
    pub include_latex: bool,
    /// Directory for writing figure files.
    pub figure_dir: Option<std::path::PathBuf>,
}

impl Default for OutputOptions {
    fn default() -> Self {
        Self {
            image_format: ImageFormat::Png,
            include_html: true,
            include_latex: true,
            figure_dir: None,
        }
    }
}

/// Preferred image format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageFormat {
    Png,
    Svg,
    Pdf,
}

/// Convert cell outputs to AST blocks.
///
/// Returns a vector of blocks representing the outputs.
pub fn outputs_to_blocks(outputs: &[CellOutput], _options: &OutputOptions) -> Vec<Block> {
    let mut blocks = Vec::new();

    for output in outputs {
        match output {
            CellOutput::Stream { name, text } => {
                // Stream output → CodeBlock with output class
                let class = format!("cell-output-{}", name);
                let block = Block::CodeBlock(CodeBlockStruct {
                    attr: make_attr("", vec![class]),
                    text: text.clone(),
                    source_info: SourceInfo::default(),
                    attr_source: AttrSourceInfo::empty(),
                });
                blocks.push(block);
            }
            CellOutput::DisplayData { data, .. } | CellOutput::ExecuteResult { data, .. } => {
                // Rich output → best representation
                if let Some(block) = mime_bundle_to_block(data) {
                    blocks.push(block);
                }
            }
            CellOutput::Error {
                ename,
                evalue,
                traceback,
            } => {
                // Error output → CodeBlock with error styling
                let error_text = format_error(ename, evalue, traceback);
                let block = Block::CodeBlock(CodeBlockStruct {
                    attr: make_attr("", vec!["cell-output-error".to_string()]),
                    text: error_text,
                    source_info: SourceInfo::default(),
                    attr_source: AttrSourceInfo::empty(),
                });
                blocks.push(block);
            }
        }
    }

    blocks
}

/// Convert a MIME bundle to the best AST representation.
fn mime_bundle_to_block(data: &MimeBundle) -> Option<Block> {
    // Priority order for output
    let priorities = [
        "text/html",
        "image/svg+xml",
        "image/png",
        "image/jpeg",
        "text/markdown",
        "text/latex",
        "text/plain",
    ];

    for mime_type in priorities {
        if let Some(content) = data.get(mime_type) {
            return convert_mime_content(mime_type, content);
        }
    }

    None
}

/// Convert a single MIME type content to a block.
fn convert_mime_content(mime_type: &str, content: &serde_json::Value) -> Option<Block> {
    match mime_type {
        "text/plain" => {
            let text = content.as_str().unwrap_or("");
            Some(Block::CodeBlock(CodeBlockStruct {
                attr: make_attr("", vec!["cell-output".to_string()]),
                text: text.to_string(),
                source_info: SourceInfo::default(),
                attr_source: AttrSourceInfo::empty(),
            }))
        }
        "text/html" => {
            let html = extract_text_content(content);
            Some(Block::RawBlock(RawBlock {
                format: "html".to_string(),
                text: html,
                source_info: SourceInfo::default(),
            }))
        }
        "text/markdown" => {
            // For now, wrap markdown in a Div
            // Full implementation would parse the markdown
            let md = extract_text_content(content);
            Some(Block::Div(Div {
                attr: make_attr("", vec!["cell-output-markdown".to_string()]),
                content: vec![Block::Paragraph(Paragraph {
                    content: vec![Inline::Str(Str {
                        text: md,
                        source_info: SourceInfo::default(),
                    })],
                    source_info: SourceInfo::default(),
                })],
                source_info: SourceInfo::default(),
                attr_source: AttrSourceInfo::empty(),
            }))
        }
        "text/latex" => {
            let latex = extract_text_content(content);
            Some(Block::RawBlock(RawBlock {
                format: "latex".to_string(),
                text: latex,
                source_info: SourceInfo::default(),
            }))
        }
        "image/png" | "image/jpeg" | "image/svg+xml" => {
            // For images, we'd normally save to a file and reference it
            // For now, create a placeholder
            let ext = match mime_type {
                "image/png" => "png",
                "image/jpeg" => "jpg",
                "image/svg+xml" => "svg",
                _ => "bin",
            };

            // TODO: Save image data to file and reference it
            // For now, create a placeholder paragraph
            let placeholder = format!("[Image output: {}]", ext);
            Some(Block::Div(Div {
                attr: make_attr("", vec!["cell-output-display".to_string()]),
                content: vec![Block::Paragraph(Paragraph {
                    content: vec![Inline::Str(Str {
                        text: placeholder,
                        source_info: SourceInfo::default(),
                    })],
                    source_info: SourceInfo::default(),
                })],
                source_info: SourceInfo::default(),
                attr_source: AttrSourceInfo::empty(),
            }))
        }
        _ => None,
    }
}

/// Extract text content from a JSON value.
///
/// Jupyter can send text as either a string or an array of strings.
fn extract_text_content(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Array(arr) => arr
            .iter()
            .filter_map(|v| v.as_str())
            .collect::<Vec<_>>()
            .join(""),
        _ => String::new(),
    }
}

/// Format error output for display.
fn format_error(ename: &str, evalue: &str, traceback: &[String]) -> String {
    let mut output = String::new();

    // Add error name and value
    output.push_str(&format!("{}: {}\n", ename, evalue));

    // Add traceback if present
    if !traceback.is_empty() {
        output.push('\n');
        for line in traceback {
            // Strip ANSI escape codes
            let clean_line = strip_ansi_codes(line);
            output.push_str(&clean_line);
            output.push('\n');
        }
    }

    output
}

/// Strip ANSI escape codes from a string.
pub fn strip_ansi_codes(s: &str) -> String {
    // Simple pattern to remove ANSI escape sequences
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '\x1b' {
            // Start of escape sequence
            if chars.peek() == Some(&'[') {
                chars.next(); // consume '['
                // Skip until we hit a letter (end of sequence)
                while let Some(&next) = chars.peek() {
                    chars.next();
                    if next.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
        } else {
            result.push(c);
        }
    }

    result
}

/// Determine the MIME type priority for a target format.
pub fn mime_priority_for_format(format: &str) -> &'static [&'static str] {
    match format {
        "latex" | "pdf" | "beamer" => &[
            "text/latex",
            "application/pdf",
            "image/pdf",
            "image/png",
            "image/jpeg",
            "text/plain",
        ],
        "html" | "html5" | "revealjs" => &[
            "text/html",
            "image/svg+xml",
            "image/png",
            "image/jpeg",
            "text/markdown",
            "text/plain",
        ],
        _ => &["text/plain", "image/png", "image/jpeg", "text/html"],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_text_content_string() {
        let value = serde_json::json!("Hello, World!");
        assert_eq!(extract_text_content(&value), "Hello, World!");
    }

    #[test]
    fn test_extract_text_content_array() {
        let value = serde_json::json!(["Hello, ", "World!"]);
        assert_eq!(extract_text_content(&value), "Hello, World!");
    }

    #[test]
    fn test_extract_text_content_other() {
        let value = serde_json::json!(42);
        assert_eq!(extract_text_content(&value), "");
    }

    #[test]
    fn test_strip_ansi_codes() {
        let input = "\x1b[31mRed\x1b[0m Normal";
        assert_eq!(strip_ansi_codes(input), "Red Normal");

        let input = "No escape codes";
        assert_eq!(strip_ansi_codes(input), "No escape codes");
    }

    #[test]
    fn test_format_error() {
        let output = format_error(
            "NameError",
            "name 'x' is not defined",
            &["  File \"<stdin>\", line 1".to_string()],
        );

        assert!(output.contains("NameError"));
        assert!(output.contains("name 'x' is not defined"));
        assert!(output.contains("File \"<stdin>\""));
    }

    #[test]
    fn test_outputs_to_blocks_stream() {
        let outputs = vec![CellOutput::Stream {
            name: "stdout".to_string(),
            text: "Hello\n".to_string(),
        }];

        let blocks = outputs_to_blocks(&outputs, &OutputOptions::default());
        assert_eq!(blocks.len(), 1);
        assert!(matches!(blocks[0], Block::CodeBlock(_)));
    }

    #[test]
    fn test_mime_priority_for_format() {
        let html_priority = mime_priority_for_format("html");
        assert_eq!(html_priority[0], "text/html");

        let latex_priority = mime_priority_for_format("latex");
        assert_eq!(latex_priority[0], "text/latex");
    }
}
