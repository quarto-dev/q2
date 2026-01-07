/*
 * engine/jupyter/text_execute.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Text-based execution for the ExecutionEngine trait.
 */

//! Text-based code execution for Jupyter.
//!
//! This module implements the text-in/text-out pattern required by [`ExecutionEngine`].
//! It parses QMD input, executes code blocks via the Jupyter daemon, and returns
//! markdown with outputs inserted.

use std::path::PathBuf;

use regex::Regex;

use super::daemon::daemon;
use super::error::JupyterError;
use super::execute::{CellOutput, ExecuteResult as KernelExecuteResult, ExecuteStatus};
use super::output::strip_ansi_codes;
use super::session::SessionKey;
use crate::engine::context::{ExecuteResult, ExecutionContext};
use crate::engine::error::ExecutionError;

type JupyterResult<T> = std::result::Result<T, JupyterError>;

/// A parsed code block from the input markdown.
#[derive(Debug)]
struct CodeBlock {
    /// Start byte offset in the input.
    start: usize,
    /// End byte offset in the input (exclusive).
    end: usize,
    /// The language/engine specifier (e.g., "python", "julia").
    language: String,
    /// The code content.
    code: String,
    /// The full original fence (for preservation).
    original: String,
}

/// Execute code blocks in QMD input and return markdown with outputs.
///
/// This is the main entry point for text-based Jupyter execution.
pub fn execute_qmd(
    input: &str,
    ctx: &ExecutionContext,
) -> std::result::Result<ExecuteResult, ExecutionError> {
    // Parse code blocks from input
    let blocks = parse_code_blocks(input);

    if blocks.is_empty() {
        // No executable code - passthrough
        return Ok(ExecuteResult::new(input));
    }

    // Determine the kernel from the first code block
    let kernel_name = map_language_to_kernel(&blocks[0].language);

    // Execute via async runtime
    let result = execute_blocks_async(input, &blocks, &kernel_name, &ctx.cwd);

    result.map_err(|e| ExecutionError::execution_failed("jupyter", e.to_string()))
}

/// Map a language name to a Jupyter kernel name.
fn map_language_to_kernel(language: &str) -> String {
    match language.to_lowercase().as_str() {
        "python" | "python3" | "py" => "python3".to_string(),
        "julia" | "jl" => "julia".to_string(),
        "r" => "ir".to_string(),
        "ruby" | "rb" => "ruby".to_string(),
        "rust" | "rs" => "rust".to_string(),
        "typescript" | "ts" => "deno".to_string(),
        "javascript" | "js" => "deno".to_string(),
        other => other.to_string(),
    }
}

/// Parse code blocks from markdown input.
///
/// Finds all fenced code blocks with executable language specifiers
/// like ```{python}, ```{julia}, etc.
fn parse_code_blocks(input: &str) -> Vec<CodeBlock> {
    // Match ```{language} ... ``` blocks
    // The pattern captures:
    // - Opening fence with {language} specifier
    // - Code content
    // - Closing fence
    let pattern = r"(?m)^```\s*\{(\w+)(?:[^}]*)?\}\s*\n([\s\S]*?)^```\s*$";
    let re = Regex::new(pattern).expect("Invalid regex pattern");

    let mut blocks = Vec::new();

    for cap in re.captures_iter(input) {
        let full_match = cap.get(0).unwrap();
        let language = cap.get(1).unwrap().as_str().to_string();
        let code = cap.get(2).unwrap().as_str().to_string();

        // Only include executable languages (not plain code blocks)
        if is_executable_language(&language) {
            blocks.push(CodeBlock {
                start: full_match.start(),
                end: full_match.end(),
                language,
                code,
                original: full_match.as_str().to_string(),
            });
        }
    }

    blocks
}

/// Check if a language specifier indicates executable code.
fn is_executable_language(language: &str) -> bool {
    matches!(
        language.to_lowercase().as_str(),
        "python"
            | "python3"
            | "py"
            | "julia"
            | "jl"
            | "r"
            | "ruby"
            | "rb"
            | "rust"
            | "rs"
            | "typescript"
            | "ts"
            | "javascript"
            | "js"
    )
}

/// Execute code blocks asynchronously and build output markdown.
fn execute_blocks_async(
    input: &str,
    blocks: &[CodeBlock],
    kernel_name: &str,
    working_dir: &PathBuf,
) -> JupyterResult<ExecuteResult> {
    // Use tokio runtime to execute async code
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| JupyterError::RuntimeLibError(e.to_string()))?;

    rt.block_on(execute_blocks_inner(
        input,
        blocks,
        kernel_name,
        working_dir,
    ))
}

/// Inner async function that does the actual execution.
async fn execute_blocks_inner(
    input: &str,
    blocks: &[CodeBlock],
    kernel_name: &str,
    working_dir: &PathBuf,
) -> JupyterResult<ExecuteResult> {
    let daemon = daemon();

    // Start or get existing kernel session
    let key: SessionKey = daemon
        .get_or_start_session(kernel_name, working_dir)
        .await?;

    // Build output by processing blocks in order
    let mut output = String::new();
    let mut last_end = 0;

    for block in blocks {
        // Append content before this block
        output.push_str(&input[last_end..block.start]);

        // Keep the original code block
        output.push_str(&block.original);
        output.push('\n');

        // Execute the code
        let exec_result = daemon
            .execute_in_session(&key, &block.code)
            .await
            .ok_or_else(|| JupyterError::NotConnected)??;

        // Format and append outputs
        let output_md = format_outputs(&exec_result);
        if !output_md.is_empty() {
            output.push_str(&output_md);
        }

        last_end = block.end;
    }

    // Append any remaining content after the last block
    output.push_str(&input[last_end..]);

    Ok(ExecuteResult::new(output))
}

/// Format kernel outputs as markdown.
fn format_outputs(result: &KernelExecuteResult) -> String {
    let mut output = String::new();

    for cell_output in &result.outputs {
        match cell_output {
            CellOutput::Stream { name, text } => {
                // Stream output as a code block with output class
                output.push_str(&format!(
                    "\n```{{.cell-output-{}}}\n{}\n```\n",
                    name,
                    text.trim_end()
                ));
            }
            CellOutput::ExecuteResult { data, .. } | CellOutput::DisplayData { data, .. } => {
                // Rich output - pick best format
                if let Some(text) = data.get("text/plain") {
                    if let Some(s) = text.as_str() {
                        output.push_str(&format!("\n```{{.cell-output}}\n{}\n```\n", s.trim_end()));
                    }
                } else if let Some(html) = data.get("text/html") {
                    if let Some(s) = html.as_str() {
                        output.push_str(&format!(
                            "\n::: {{.cell-output-display}}\n```{{=html}}\n{}\n```\n:::\n",
                            s
                        ));
                    }
                } else if data.contains_key("image/png") || data.contains_key("image/svg+xml") {
                    // TODO: Save image to file and reference it
                    output.push_str("\n::: {.cell-output-display}\n[Image output]\n:::\n");
                }
            }
            CellOutput::Error {
                ename,
                evalue,
                traceback,
            } => {
                // Error output
                let mut error_text = format!("{}: {}\n", ename, evalue);
                for line in traceback {
                    error_text.push_str(&strip_ansi_codes(line));
                    error_text.push('\n');
                }
                output.push_str(&format!(
                    "\n```{{.cell-output-error}}\n{}\n```\n",
                    error_text.trim_end()
                ));
            }
        }
    }

    // Also include error status if execution failed
    if let ExecuteStatus::Error {
        ename,
        evalue,
        traceback,
    } = &result.status
    {
        // Only add if not already in outputs
        if result.outputs.is_empty() {
            let mut error_text = format!("{}: {}\n", ename, evalue);
            for line in traceback {
                error_text.push_str(&strip_ansi_codes(line));
                error_text.push('\n');
            }
            output.push_str(&format!(
                "\n```{{.cell-output-error}}\n{}\n```\n",
                error_text.trim_end()
            ));
        }
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_code_blocks_single() {
        let input = r#"---
title: Test
---

Some text.

```{python}
print("hello")
```

More text.
"#;

        let blocks = parse_code_blocks(input);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].language, "python");
        assert_eq!(blocks[0].code.trim(), "print(\"hello\")");
    }

    #[test]
    fn test_parse_code_blocks_multiple() {
        let input = r#"
```{python}
x = 1
```

```{python}
print(x)
```
"#;

        let blocks = parse_code_blocks(input);
        assert_eq!(blocks.len(), 2);
    }

    #[test]
    fn test_parse_code_blocks_with_options() {
        let input = r#"
```{python echo=false}
print("hello")
```
"#;

        let blocks = parse_code_blocks(input);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].language, "python");
    }

    #[test]
    fn test_parse_code_blocks_non_executable() {
        let input = r#"
```{python}
print("hello")
```

```json
{"key": "value"}
```

```{.python}
# This is a plain code block, not executable
```
"#;

        let blocks = parse_code_blocks(input);
        // Only the first block should be detected as executable
        assert_eq!(blocks.len(), 1);
    }

    #[test]
    fn test_map_language_to_kernel() {
        assert_eq!(map_language_to_kernel("python"), "python3");
        assert_eq!(map_language_to_kernel("Python"), "python3");
        assert_eq!(map_language_to_kernel("py"), "python3");
        assert_eq!(map_language_to_kernel("julia"), "julia");
        assert_eq!(map_language_to_kernel("r"), "ir");
        assert_eq!(map_language_to_kernel("rust"), "rust");
        assert_eq!(map_language_to_kernel("typescript"), "deno");
        assert_eq!(map_language_to_kernel("ts"), "deno");
        assert_eq!(map_language_to_kernel("javascript"), "deno");
        assert_eq!(map_language_to_kernel("js"), "deno");
        assert_eq!(map_language_to_kernel("unknown"), "unknown");
    }

    #[test]
    fn test_is_executable_language() {
        assert!(is_executable_language("python"));
        assert!(is_executable_language("Python"));
        assert!(is_executable_language("julia"));
        assert!(is_executable_language("r"));
        assert!(is_executable_language("typescript"));
        assert!(is_executable_language("ts"));
        assert!(is_executable_language("javascript"));
        assert!(is_executable_language("js"));
        assert!(!is_executable_language("json"));
        assert!(!is_executable_language("markdown"));
    }

    #[test]
    fn test_format_outputs_stream() {
        let result = KernelExecuteResult {
            status: ExecuteStatus::Ok,
            outputs: vec![CellOutput::Stream {
                name: "stdout".to_string(),
                text: "Hello, World!\n".to_string(),
            }],
            execution_count: Some(1),
        };

        let output = format_outputs(&result);
        assert!(output.contains("cell-output-stdout"));
        assert!(output.contains("Hello, World!"));
    }

    #[test]
    fn test_format_outputs_error() {
        let result = KernelExecuteResult {
            status: ExecuteStatus::Error {
                ename: "NameError".to_string(),
                evalue: "name 'x' is not defined".to_string(),
                traceback: vec!["Traceback...".to_string()],
            },
            outputs: vec![CellOutput::Error {
                ename: "NameError".to_string(),
                evalue: "name 'x' is not defined".to_string(),
                traceback: vec!["Traceback...".to_string()],
            }],
            execution_count: Some(1),
        };

        let output = format_outputs(&result);
        assert!(output.contains("cell-output-error"));
        assert!(output.contains("NameError"));
    }
}
