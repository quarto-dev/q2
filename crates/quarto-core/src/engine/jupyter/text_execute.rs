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
/// of the form ```` ```{lang} ```` (e.g. `{python}`, `{julia}`).
///
/// Quarto 2 is strict: only a bare `{lang}` is accepted. The Quarto 1
/// variant `{python echo=false}` (fence-attached options) is not
/// supported — per-cell directives live inside the block as
/// `#| key: value` YAML comments. Dropping fence-attached options
/// keeps the tree-sitter grammar for qmd tractable.
fn parse_code_blocks(input: &str) -> Vec<CodeBlock> {
    // Match ```{language} ... ``` blocks (no fence options allowed).
    let pattern = r"(?m)^```\s*\{(\w+)\}\s*\n([\s\S]*?)^```\s*$";
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

        // A fenced div cannot interrupt a paragraph — make sure the
        // `::: {.cell}` opener starts its own block even when the
        // source had no blank line before the cell.
        if output.ends_with('\n') && !output.ends_with("\n\n") {
            output.push('\n');
        }

        // Execute the code
        let exec_result = daemon
            .execute_in_session(&key, &block.code)
            .await
            .ok_or(JupyterError::NotConnected)??;

        // Emit the whole cell — echoed source plus outputs — in the
        // Quarto-canonical `::: {.cell}` shape.
        output.push_str(&render_cell(block, &exec_result));

        last_end = block.end;
    }

    // Append any remaining content after the last block
    output.push_str(&input[last_end..]);

    Ok(ExecuteResult::new(output))
}

/// Fence for `text`, sized so the fence can never collide with the
/// content: max(3, longest leading backtick run in `text` + 1)
/// backticks. Mirrors Q1's `ticksForCode`
/// (`external-sources/quarto-cli/src/core/jupyter/jupyter.ts`); a
/// fixed ``` corrupts the emitted markdown whenever a cell or its
/// output contains a line starting with three backticks.
fn ticks_for_code(text: &str) -> String {
    let longest = text
        .lines()
        .map(|line| line.trim_start().bytes().take_while(|b| *b == b'`').count())
        .max()
        .unwrap_or(0);
    "`".repeat(std::cmp::max(3, longest + 1))
}

/// Render one executed cell — echoed source plus formatted outputs —
/// in the Quarto-canonical cell shape shared with knitr's hooks and
/// Q1's jupyter engine (bd-gthycd33):
///
/// ```markdown
/// ::: {.cell}
///
/// ```{.python .cell-code}
/// 2 + 3
/// ```
///
/// ::: {.cell-output .cell-output-display}
///
/// ```
/// 5
/// ```
///
/// :::
/// :::
/// ```
///
/// The `Div.cell` wrapper is a cross-engine contract: the preview
/// capture splice (`crate::engine::capture_splice`) replaces a live
/// engine cell with exactly one wrapper Div, and the Bootstrap CSS
/// targets `.cell .cell-output-* pre code`. A cell with no outputs
/// still gets the wrapper (knitr wraps output-less chunks too).
fn render_cell(block: &CodeBlock, result: &KernelExecuteResult) -> String {
    format!(
        "::: {{.cell}}\n\n{}\n{}\n:::\n",
        echoed_source_fence(block),
        format_outputs(result)
    )
}

/// Reconstruct the echoed source fence for an executed cell.
///
/// The parser captures code cells with a `{lang}` fence (the curly
/// braces mean "the engine should execute this"). After the engine
/// runs, we emit the source back as an *attribute-form* fence —
/// `{.python .cell-code}` — so the block is no longer scheduled for
/// execution, the highlight stage resolves the language from the
/// first class, and downstream consumers can target `.cell-code`
/// (same classes knitr's hooks emit). Per-cell directives like
/// `#| echo: false` travel inside `block.code` and are handled by
/// whatever stage consumes them — this function only rewrites the
/// fence.
fn echoed_source_fence(block: &CodeBlock) -> String {
    let code = block.code.trim_end_matches('\n');
    let ticks = ticks_for_code(code);
    format!(
        "{ticks}{{.{} .cell-code}}\n{}\n{ticks}",
        block.language, code
    )
}

/// Wrap already-trimmed output text in a `::: {<classes>}` div around
/// a plain fence, with the fence sized to the content.
fn fenced_output_div(classes: &str, text: &str) -> String {
    let ticks = ticks_for_code(text);
    format!("\n::: {{{classes}}}\n\n{ticks}\n{text}\n{ticks}\n\n:::\n")
}

/// Format kernel outputs as markdown.
///
/// Every output becomes a `::: {.cell-output .cell-output-<type>}`
/// div wrapping a plain fence — the Q1/knitr class scheme
/// (`outputTypeCssClass` in quarto-cli's `jupyter.ts`): streams are
/// `-stdout` / `-stderr`, `execute_result` / `display_data` are
/// `-display`, errors are `-error`.
fn format_outputs(result: &KernelExecuteResult) -> String {
    let mut output = String::new();

    for cell_output in &result.outputs {
        match cell_output {
            CellOutput::Stream { name, text } => {
                output.push_str(&fenced_output_div(
                    &format!(".cell-output .cell-output-{}", name),
                    text.trim_end(),
                ));
            }
            CellOutput::ExecuteResult { data, .. } | CellOutput::DisplayData { data, .. } => {
                // Rich output - pick best format
                if let Some(text) = data.get("text/plain") {
                    let s = extract_text_content(text);
                    output.push_str(&fenced_output_div(
                        ".cell-output .cell-output-display",
                        s.trim_end(),
                    ));
                } else if let Some(html) = data.get("text/html") {
                    let s = extract_text_content(html);
                    let ticks = ticks_for_code(&s);
                    output.push_str(&format!(
                        "\n::: {{.cell-output .cell-output-display}}\n\n{ticks}{{=html}}\n{}\n{ticks}\n\n:::\n",
                        s
                    ));
                } else if data.contains_key("image/png") || data.contains_key("image/svg+xml") {
                    // TODO(bd-5t6wvu7m): save the image to a supporting
                    // file and emit a real figure instead of a placeholder.
                    output.push_str(
                        "\n::: {.cell-output .cell-output-display}\n\n[Image output]\n\n:::\n",
                    );
                }
            }
            CellOutput::Error {
                ename,
                evalue,
                traceback,
            } => {
                output.push_str(&fenced_output_div(
                    ".cell-output .cell-output-error",
                    format_error_text(ename, evalue, traceback).trim_end(),
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
            output.push_str(&fenced_output_div(
                ".cell-output .cell-output-error",
                format_error_text(ename, evalue, traceback).trim_end(),
            ));
        }
    }

    output
}

/// `"<ename>: <evalue>"` plus the ANSI-stripped traceback lines.
fn format_error_text(ename: &str, evalue: &str, traceback: &[String]) -> String {
    let mut error_text = format!("{}: {}\n", ename, evalue);
    for line in traceback {
        error_text.push_str(&strip_ansi_codes(line));
        error_text.push('\n');
    }
    error_text
}

/// Extract text content from a MIME-bundle JSON value. Jupyter can
/// send text as either a single string or an array of line strings
/// (the nbformat multiline convention).
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

/// Strip ANSI escape codes from a string (kernel tracebacks arrive
/// colorized).
fn strip_ansi_codes(s: &str) -> String {
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
    fn test_parse_code_blocks_rejects_fence_options() {
        // Q2 doesn't accept Q1-style fence options like
        // `{python echo=false}`. Per-cell directives go inside the
        // block as `#| echo: false` YAML comments.
        let input = r#"
```{python echo=false}
print("hello")
```
"#;

        let blocks = parse_code_blocks(input);
        assert!(
            blocks.is_empty(),
            "Q1-style fence options must not be recognized as an executable cell"
        );
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

    // ── Emission shape (bd-gthycd33): Quarto-canonical cells ──────
    //
    // The post-engine markdown must match the shape knitr's vendored
    // Q1 hooks and Q1's own jupyter engine emit: every executed cell
    // wrapped in `::: {.cell}`, echoed source as a
    // `{.<lang> .cell-code}` fence, outputs as
    // `::: {.cell-output .cell-output-*}` divs around plain fences.
    // The preview capture splice keys on the `Div.cell` wrapper and
    // the Bootstrap CSS keys on `.cell .cell-output-* pre code`;
    // wrapper-less emission breaks both. Exact-string assertions on
    // purpose — this shape is a cross-engine contract (see the
    // engine_output_parity integration suite).

    fn py_block(code: &str) -> CodeBlock {
        CodeBlock {
            start: 0,
            end: 0,
            language: "python".to_string(),
            code: code.to_string(),
        }
    }

    fn ok_result(outputs: Vec<CellOutput>) -> KernelExecuteResult {
        KernelExecuteResult {
            status: ExecuteStatus::Ok,
            outputs,
            execution_count: Some(1),
        }
    }

    fn text_plain(s: &str) -> std::collections::HashMap<String, serde_json::Value> {
        let mut data = std::collections::HashMap::new();
        data.insert("text/plain".to_string(), serde_json::json!(s));
        data
    }

    #[test]
    fn test_echoed_source_fence_emits_cell_code_class() {
        // `{python}` fence means "execute"; after execution the echoed
        // source comes back as an attribute-form fence with the
        // language class first (the highlight stage resolves the
        // language from the first class) plus `.cell-code`.
        let block = py_block("print(\"hi\")\n");
        assert_eq!(
            echoed_source_fence(&block),
            "```{.python .cell-code}\nprint(\"hi\")\n```"
        );
    }

    #[test]
    fn test_echoed_source_fence_grows_ticks_for_backtick_content() {
        // Q1's ticksForCode rule: max(3, longest leading backtick
        // run + 1). Code containing a ``` line must get a 4-tick
        // fence or the emitted markdown is corrupt.
        let block = py_block("s = \"\"\n```\n\"\"\n");
        let fence = echoed_source_fence(&block);
        assert!(
            fence.starts_with("````{.python .cell-code}\n"),
            "expected a 4-tick fence, got:\n{fence}"
        );
        assert!(fence.ends_with("\n````"), "got:\n{fence}");
    }

    #[test]
    fn test_format_outputs_stream_stdout_is_cell_output_div() {
        let result = ok_result(vec![CellOutput::Stream {
            name: "stdout".to_string(),
            text: "Hello, World!\n".to_string(),
        }]);
        assert_eq!(
            format_outputs(&result),
            "\n::: {.cell-output .cell-output-stdout}\n\n```\nHello, World!\n```\n\n:::\n"
        );
    }

    #[test]
    fn test_format_outputs_stream_stderr_is_cell_output_div() {
        let result = ok_result(vec![CellOutput::Stream {
            name: "stderr".to_string(),
            text: "warning\n".to_string(),
        }]);
        assert_eq!(
            format_outputs(&result),
            "\n::: {.cell-output .cell-output-stderr}\n\n```\nwarning\n```\n\n:::\n"
        );
    }

    #[test]
    fn test_format_outputs_execute_result_is_display_div() {
        // Q1: execute_result / display_data are `.cell-output-display`
        // (not `-stdout` — they aren't streams).
        let result = ok_result(vec![CellOutput::ExecuteResult {
            execution_count: 1,
            data: text_plain("5"),
            metadata: serde_json::json!({}),
        }]);
        assert_eq!(
            format_outputs(&result),
            "\n::: {.cell-output .cell-output-display}\n\n```\n5\n```\n\n:::\n"
        );
    }

    #[test]
    fn test_format_outputs_html_is_display_div_with_raw_html() {
        let mut data = std::collections::HashMap::new();
        data.insert("text/html".to_string(), serde_json::json!("<b>5</b>"));
        let result = ok_result(vec![CellOutput::DisplayData {
            data,
            metadata: serde_json::json!({}),
        }]);
        assert_eq!(
            format_outputs(&result),
            "\n::: {.cell-output .cell-output-display}\n\n```{=html}\n<b>5</b>\n```\n\n:::\n"
        );
    }

    #[test]
    fn test_format_outputs_error_is_error_div() {
        let result = ok_result(vec![CellOutput::Error {
            ename: "NameError".to_string(),
            evalue: "name 'x' is not defined".to_string(),
            traceback: vec!["Traceback...".to_string()],
        }]);
        assert_eq!(
            format_outputs(&result),
            "\n::: {.cell-output .cell-output-error}\n\n```\nNameError: name 'x' is not defined\nTraceback...\n```\n\n:::\n"
        );
    }

    #[test]
    fn test_format_outputs_grows_ticks_for_backtick_output() {
        // An output whose text contains a ``` line must get a wider
        // fence, or the emitted markdown is corrupt.
        let result = ok_result(vec![CellOutput::Stream {
            name: "stdout".to_string(),
            text: "```\n".to_string(),
        }]);
        assert_eq!(
            format_outputs(&result),
            "\n::: {.cell-output .cell-output-stdout}\n\n````\n```\n````\n\n:::\n"
        );
    }

    #[test]
    fn test_render_cell_wraps_source_and_outputs_in_cell_div() {
        let block = py_block("2 + 3\n");
        let result = ok_result(vec![CellOutput::ExecuteResult {
            execution_count: 1,
            data: text_plain("5"),
            metadata: serde_json::json!({}),
        }]);
        assert_eq!(
            render_cell(&block, &result),
            "::: {.cell}\n\n```{.python .cell-code}\n2 + 3\n```\n\n::: {.cell-output .cell-output-display}\n\n```\n5\n```\n\n:::\n\n:::\n"
        );
    }

    #[test]
    fn test_format_outputs_image_placeholder_is_display_div() {
        // Until bd-5t6wvu7m lands, image outputs render a placeholder —
        // but it must already sit in the canonical display div.
        let mut data = std::collections::HashMap::new();
        data.insert("image/png".to_string(), serde_json::json!("base64…"));
        let result = ok_result(vec![CellOutput::DisplayData {
            data,
            metadata: serde_json::json!({}),
        }]);
        assert_eq!(
            format_outputs(&result),
            "\n::: {.cell-output .cell-output-display}\n\n[Image output]\n\n:::\n"
        );
    }

    #[test]
    fn test_strip_ansi_codes() {
        let input = "\x1b[31mRed\x1b[0m Normal";
        assert_eq!(strip_ansi_codes(input), "Red Normal");

        let input = "No escape codes";
        assert_eq!(strip_ansi_codes(input), "No escape codes");
    }

    #[test]
    fn test_extract_text_content_string_and_array() {
        assert_eq!(
            extract_text_content(&serde_json::json!("Hello, World!")),
            "Hello, World!"
        );
        // nbformat multiline convention: array of line strings.
        assert_eq!(
            extract_text_content(&serde_json::json!(["Hello, ", "World!"])),
            "Hello, World!"
        );
        assert_eq!(extract_text_content(&serde_json::json!(42)), "");
    }

    #[test]
    fn test_format_outputs_multiline_array_text_plain() {
        // A kernel sending text/plain as an array of lines must not
        // be silently dropped (the old `as_str()` path did that).
        let mut data = std::collections::HashMap::new();
        data.insert("text/plain".to_string(), serde_json::json!(["4", "2"]));
        let result = ok_result(vec![CellOutput::ExecuteResult {
            execution_count: 1,
            data,
            metadata: serde_json::json!({}),
        }]);
        assert_eq!(
            format_outputs(&result),
            "\n::: {.cell-output .cell-output-display}\n\n```\n42\n```\n\n:::\n"
        );
    }

    #[test]
    fn test_render_cell_without_output_still_wraps() {
        // A source-only cell (assignment, `output: false`, …) still
        // gets the `.cell` wrapper — the splice must be able to
        // replace the live cell, and knitr wraps output-less chunks
        // too.
        let block = py_block("x = 1\n");
        let result = ok_result(vec![]);
        assert_eq!(
            render_cell(&block, &result),
            "::: {.cell}\n\n```{.python .cell-code}\nx = 1\n```\n\n:::\n"
        );
    }
}
