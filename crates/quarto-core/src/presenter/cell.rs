/*
 * presenter/cell.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Cell presentation: executed cell to Quarto-canonical cell markdown.
 */

//! Render one executed cell — echoed source plus formatted outputs — as markdown.
//!
//! Target-neutral: no kernel, no engine availability. The live Jupyter path, the
//! stored-notebook replay and (later) record presentation all call [`render_cell`].

use quarto_source_map::SourceInfo;

use super::figures::FigureSink;
use super::output::{CellOutput, CellResult, ExecuteStatus, extract_text_content};
use super::visibility::CellVisibility;
use crate::engine::ExecutionContext;

/// Whether an output is a "warning" for visibility purposes: a stderr
/// stream. Mirrors Q1's `isWarningOutput` in
/// `src/core/jupyter/jupyter.ts` — the kernel gives no richer signal,
/// so stderr is the whole definition.
fn is_warning_output(output: &CellOutput) -> bool {
    matches!(output, CellOutput::Stream { name, .. } if name == "stderr")
}

/// Render `info` + `offset` as `path:line:column` (1-based) through
/// the execution context's source map, when it resolves.
pub fn describe_location(
    info: &SourceInfo,
    offset: usize,
    ctx: &ExecutionContext,
) -> Option<String> {
    let mapped = info.map_offset(offset, &ctx.source_context)?;
    let path = ctx
        .source_context
        .get_file(mapped.file_id)
        .map(|f| f.path.clone())?;
    Some(format!(
        "{}:{}:{}",
        path,
        mapped.location.row + 1,
        mapped.location.column + 1
    ))
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
///
/// `visibility` (bd-nn2fou8h) decides which halves survive. When it
/// suppresses *both* the source and every output, the cell emits the
/// empty string rather than an empty wrapper: `<div class="cell">`
/// with nothing in it is still a box in the rendered page, and Q1
/// likewise builds its div opener but writes it only "if there is
/// actually content in the div" (`jupyter.ts`). knitr behaves the
/// same way — an `include: false` chunk leaves no wrapper — so this
/// keeps the two engines agreeing.
pub fn render_cell(
    language: &str,
    code: &str,
    result: &CellResult,
    fig: &mut dyn FigureSink,
    visibility: CellVisibility,
) -> String {
    let source = if visibility.echo && !code.trim().is_empty() {
        echoed_source_fence(language, code)
    } else {
        String::new()
    };
    let outputs = if visibility.output {
        format_outputs(result, fig, visibility.warning)
    } else {
        String::new()
    };
    if source.is_empty() && outputs.trim().is_empty() {
        return String::new();
    }
    format!("::: {{.cell}}\n\n{source}\n{outputs}\n:::\n")
}

/// Reconstruct the echoed source fence for an executed cell.
///
/// The parser captures code cells with a `{lang}` fence (the curly
/// braces mean "the engine should execute this"). After the engine
/// runs, we emit the source back as an *attribute-form* fence —
/// `{.python .cell-code}` — so the block is no longer scheduled for
/// execution, the highlight stage resolves the language from the
/// first class, and downstream consumers can target `.cell-code`
/// (same classes knitr's hooks emit). The caller passes the
/// *partitioned* code — `#|` option lines are consumed by
/// `crate::cell_options` before execution and are not echoed
/// (knitr/Q1 parity).
fn echoed_source_fence(language: &str, code: &str) -> String {
    let code = code.trim_end_matches('\n');
    let ticks = ticks_for_code(code);
    format!("{ticks}{{.{language} .cell-code}}\n{code}\n{ticks}")
}

/// Wrap already-trimmed output text in a `::: {<classes>}` div around
/// a plain fence, with the fence sized to the content.
/// The markdown a rich output presents as: `text/markdown`, or math `text/latex` when
/// there is no `text/markdown` (Q1's `displayDataWithMarkdownMath`; math is text that
/// starts and ends with `$`, or a `\begin{` environment).
fn markdown_output(data: &crate::presenter::output::MimeBundle) -> Option<String> {
    if let Some(markdown) = data.get("text/markdown") {
        return Some(extract_text_content(markdown));
    }
    let latex = extract_text_content(data.get("text/latex")?);
    let is_math = (latex.starts_with('$') && latex.ends_with('$'))
        || (latex.starts_with("\\begin{") && latex.contains("\\end{"));
    is_math.then_some(latex)
}

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
///
/// Rich-output MIME priority follows Q1's `displayDataMimeType` for
/// HTML targets, scoped to the types q2 handles: `text/html` →
/// `text/markdown` (or math `text/latex`) → `image/svg+xml` →
/// `image/png` → `image/jpeg` → `text/plain` **last** (a matplotlib
/// bundle carries an image plus a text fallback; the image must win).
/// Jupyter-widget payloads remain unhandled (follow-up).
fn format_outputs(result: &CellResult, fig: &mut dyn FigureSink, show_warnings: bool) -> String {
    const IMAGE_MIMES: [&str; 3] = ["image/svg+xml", "image/png", "image/jpeg"];

    let mut output = String::new();

    for cell_output in &result.outputs {
        // `warning: false` drops stderr streams — where Python's
        // `warnings` module and most library chatter land. Q1 applies
        // exactly this filter (`isWarningOutput`: output_type
        // "stream" with name "stderr") before emitting anything.
        // Errors are a separate output kind and are never dropped
        // here; the error *policy* decides those.
        if !show_warnings && is_warning_output(cell_output) {
            continue;
        }
        match cell_output {
            CellOutput::Stream { name, text } => {
                output.push_str(&fenced_output_div(
                    &format!(".cell-output .cell-output-{}", name),
                    text.trim_end(),
                ));
            }
            CellOutput::ExecuteResult { data, .. } | CellOutput::DisplayData { data, .. } => {
                // Rich output — pick the best format (Q1 priority).
                let image_entry = IMAGE_MIMES
                    .iter()
                    .find_map(|m| data.get(*m).map(|v| (*m, v)));
                if let Some(html) = data.get("text/html") {
                    let s = extract_text_content(html);
                    let ticks = ticks_for_code(&s);
                    output.push_str(&format!(
                        "\n::: {{.cell-output .cell-output-display}}\n\n{ticks}{{=html}}\n{}\n{ticks}\n\n:::\n",
                        s
                    ));
                } else if let Some(markdown) = markdown_output(data) {
                    // Written as markdown itself (Q1's `mdMarkdownOutput`), with the
                    // extra class `isMarkdown` adds.
                    output.push_str(&format!(
                        "\n::: {{.cell-output .cell-output-display .cell-output-markdown}}\n\n{markdown}\n\n:::\n"
                    ));
                } else if let Some(rel_path) =
                    image_entry.and_then(|(mime, value)| fig.write_image(mime, value))
                {
                    // Figure divs carry ONLY `cell-output-display` — no
                    // generic `cell-output` — matching q2-knitr's
                    // vendored figure hook (hooks.R:627) so the
                    // cross-engine parity contract holds. (Q1's jupyter
                    // adds `.cell-output` here and thus disagrees with
                    // Q1's own knitr; q2 resolves the asymmetry toward
                    // knitr. The only generic `.cell-output` consumer,
                    // the print stylesheet, also matches `img`
                    // directly.)
                    output.push_str(&format!(
                        "\n::: {{.cell-output-display}}\n\n![]({rel_path})\n\n:::\n",
                    ));
                } else if let Some(text) = data.get("text/plain") {
                    let s = extract_text_content(text);
                    output.push_str(&fenced_output_div(
                        ".cell-output .cell-output-display",
                        s.trim_end(),
                    ));
                } else if image_entry.is_some() {
                    // An image payload we failed to write (bad base64,
                    // I/O error) and no text fallback: keep the
                    // pre-bd-5t6wvu7m placeholder rather than dropping
                    // the output silently.
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
    use std::path::Path;

    use crate::presenter::figures::FsFigureSink;

    fn result_with(outputs: Vec<CellOutput>) -> CellResult {
        CellResult {
            status: ExecuteStatus::Ok,
            outputs,
            execution_count: Some(1),
        }
    }

    fn stream(name: &str, text: &str) -> CellOutput {
        CellOutput::Stream {
            name: name.to_string(),
            text: text.to_string(),
        }
    }

    fn render(code: &str, outputs: Vec<CellOutput>, visibility: CellVisibility) -> String {
        let mut fig = FsFigureSink::new(Path::new("/nonexistent/doc.qmd"));
        render_cell("python", code, &result_with(outputs), &mut fig, visibility)
    }

    #[test]
    fn test_render_cell_omits_source_when_echo_false() {
        let md = render(
            "print(1)",
            vec![stream("stdout", "1")],
            CellVisibility {
                echo: false,
                ..CellVisibility::VISIBLE
            },
        );
        assert!(!md.contains(".cell-code"), "no source fence; got:\n{md}");
        assert!(!md.contains("print(1)"), "no source text; got:\n{md}");
        assert!(md.contains("::: {.cell}"), "wrapper kept; got:\n{md}");
        assert!(md.contains("cell-output-stdout"), "output kept; got:\n{md}");
    }

    #[test]
    fn test_render_cell_omits_outputs_when_output_false() {
        let md = render(
            "print(1)",
            vec![stream("stdout", "1")],
            CellVisibility {
                output: false,
                ..CellVisibility::VISIBLE
            },
        );
        assert!(md.contains(".cell-code"), "source kept; got:\n{md}");
        assert!(!md.contains("cell-output"), "no output div; got:\n{md}");
    }

    #[test]
    fn test_render_cell_drops_stderr_when_warning_false() {
        let md = render(
            "warn()",
            vec![
                stream("stderr", "UserWarning: boom"),
                stream("stdout", "ok"),
            ],
            CellVisibility {
                warning: false,
                ..CellVisibility::VISIBLE
            },
        );
        assert!(!md.contains("boom"), "stderr dropped; got:\n{md}");
        assert!(md.contains("ok"), "stdout kept; got:\n{md}");
    }

    /// Nothing visible ⇒ no wrapper at all. An empty `::: {.cell}`
    /// still paints a `<div class="cell">` box in the output.
    #[test]
    fn test_render_cell_emits_nothing_when_fully_hidden() {
        assert_eq!(
            render(
                "print(1)",
                vec![stream("stdout", "1")],
                CellVisibility::HIDDEN
            ),
            ""
        );
        // Same when echo and output are individually off rather than
        // via include.
        assert_eq!(
            render(
                "print(1)",
                vec![stream("stdout", "1")],
                CellVisibility {
                    echo: false,
                    output: false,
                    warning: false,
                }
            ),
            ""
        );
    }

    /// An output-less cell keeps its wrapper as long as the source is
    /// echoed — knitr wraps output-less chunks too, and the preview
    /// capture splice pairs cells to wrappers.
    #[test]
    fn test_render_cell_keeps_wrapper_for_output_less_cell() {
        let md = render("x = 1", vec![], CellVisibility::VISIBLE);
        assert!(md.contains("::: {.cell}"), "wrapper kept; got:\n{md}");
        assert!(md.contains(".cell-code"), "source kept; got:\n{md}");
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

    fn ok_result(outputs: Vec<CellOutput>) -> CellResult {
        CellResult {
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

    /// An `FsFigureSink` rooted in a fresh temp dir, positioned at cell 1.
    /// The temp dir is leaked for the process lifetime — tests that
    /// assert on written files create their own named `TempDir` instead.
    fn test_fig() -> FsFigureSink {
        let dir = tempfile::tempdir().unwrap();
        let mut fig = FsFigureSink::new(&dir.path().join("doc.qmd"));
        fig.begin_cell();
        std::mem::forget(dir);
        fig
    }

    /// Valid base64 of a 1x1 transparent PNG, split across two lines
    /// (the nbformat multiline convention the decoder must handle).
    const PNG_B64_MULTILINE: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk\nYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==";

    #[test]
    fn test_echoed_source_fence_emits_cell_code_class() {
        // `{python}` fence means "execute"; after execution the echoed
        // source comes back as an attribute-form fence with the
        // language class first (the highlight stage resolves the
        // language from the first class) plus `.cell-code`.
        assert_eq!(
            echoed_source_fence("python", "print(\"hi\")\n"),
            "```{.python .cell-code}\nprint(\"hi\")\n```"
        );
    }

    #[test]
    fn test_echoed_source_fence_grows_ticks_for_backtick_content() {
        // Q1's ticksForCode rule: max(3, longest leading backtick
        // run + 1). Code containing a ``` line must get a 4-tick
        // fence or the emitted markdown is corrupt.
        let fence = echoed_source_fence("python", "s = \"\"\n```\n\"\"\n");
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
            format_outputs(&result, &mut test_fig(), true),
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
            format_outputs(&result, &mut test_fig(), true),
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
            format_outputs(&result, &mut test_fig(), true),
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
            format_outputs(&result, &mut test_fig(), true),
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
            format_outputs(&result, &mut test_fig(), true),
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
            format_outputs(&result, &mut test_fig(), true),
            "\n::: {.cell-output .cell-output-stdout}\n\n````\n```\n````\n\n:::\n"
        );
    }

    #[test]
    fn test_render_cell_wraps_source_and_outputs_in_cell_div() {
        let result = ok_result(vec![CellOutput::ExecuteResult {
            execution_count: 1,
            data: text_plain("5"),
            metadata: serde_json::json!({}),
        }]);
        assert_eq!(
            render_cell(
                "python",
                "2 + 3\n",
                &result,
                &mut test_fig(),
                CellVisibility::VISIBLE,
            ),
            "::: {.cell}\n\n```{.python .cell-code}\n2 + 3\n```\n\n::: {.cell-output .cell-output-display}\n\n```\n5\n```\n\n:::\n\n:::\n"
        );
    }

    #[test]
    fn test_display_bundle_with_png_and_text_prefers_image() {
        // bd-5t6wvu7m: a matplotlib display bundle carries BOTH
        // image/png and a text/plain fallback (`<Figure size 640x480
        // with 1 Axes>`). The image must win — Q1's
        // displayDataMimeType puts text/plain last. Before the fix,
        // format_outputs checked text/plain first, so the figure
        // rendered as its text repr.
        let mut data = std::collections::HashMap::new();
        // 1x1 transparent PNG, valid base64.
        data.insert(
            "image/png".to_string(),
            serde_json::json!(
                "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk\nYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg=="
            ),
        );
        data.insert(
            "text/plain".to_string(),
            serde_json::json!("<Figure size 640x480 with 1 Axes>"),
        );
        let result = ok_result(vec![CellOutput::DisplayData {
            data,
            metadata: serde_json::json!({}),
        }]);

        let md = format_outputs(&result, &mut test_fig(), true);
        assert!(
            !md.contains("<Figure size"),
            "image must beat the text/plain fallback; got:\n{md}"
        );
    }

    #[test]
    fn test_png_output_writes_file_and_emits_image_ref() {
        // The full bd-5t6wvu7m behavior: a PNG display output writes
        // `<stem>_files/figure-html/cell-<i>-output-<j>.png` next to
        // the doc and emits `![](...)` inside the display div.
        let tmp = tempfile::tempdir().unwrap();
        let mut fig = FsFigureSink::new(&tmp.path().join("pyfig.qmd"));
        fig.begin_cell();

        let mut data = std::collections::HashMap::new();
        data.insert(
            "image/png".to_string(),
            serde_json::json!(PNG_B64_MULTILINE),
        );
        data.insert(
            "text/plain".to_string(),
            serde_json::json!("<Figure size 640x480 with 1 Axes>"),
        );
        let result = ok_result(vec![CellOutput::DisplayData {
            data,
            metadata: serde_json::json!({}),
        }]);

        let md = format_outputs(&result, &mut fig, true);
        assert_eq!(
            md,
            "\n::: {.cell-output-display}\n\n![](pyfig_files/figure-html/cell-1-output-1.png)\n\n:::\n"
        );
        // Multiline base64 decoded and written to disk.
        let bytes = std::fs::read(
            tmp.path()
                .join("pyfig_files/figure-html/cell-1-output-1.png"),
        )
        .expect("figure file written");
        assert!(
            bytes.starts_with(b"\x89PNG\r\n"),
            "decoded bytes must be a PNG"
        );
        assert!(fig.wrote_any());
        assert_eq!(fig.files_dir(), tmp.path().join("pyfig_files"));
    }

    #[test]
    fn test_svg_markup_written_as_text_with_svg_extension() {
        // Q1's mdImageOutput rule: literal `<svg` payloads are written
        // as text, not base64-decoded.
        let tmp = tempfile::tempdir().unwrap();
        let mut fig = FsFigureSink::new(&tmp.path().join("doc.qmd"));
        fig.begin_cell();

        let mut data = std::collections::HashMap::new();
        data.insert(
            "image/svg+xml".to_string(),
            serde_json::json!("<svg xmlns=\"http://www.w3.org/2000/svg\"/>"),
        );
        let result = ok_result(vec![CellOutput::DisplayData {
            data,
            metadata: serde_json::json!({}),
        }]);

        let md = format_outputs(&result, &mut fig, true);
        assert!(
            md.contains("![](doc_files/figure-html/cell-1-output-1.svg)"),
            "got:\n{md}"
        );
        let text =
            std::fs::read_to_string(tmp.path().join("doc_files/figure-html/cell-1-output-1.svg"))
                .expect("svg written");
        assert!(text.starts_with("<svg"));
    }

    #[test]
    fn test_html_still_beats_image() {
        // Q1's html-target priority puts text/html above images
        // (jupyter widgets and DataFrame tables ship html + png +
        // plain; the html representation wins).
        let mut data = std::collections::HashMap::new();
        data.insert("text/html".to_string(), serde_json::json!("<b>table</b>"));
        data.insert(
            "image/png".to_string(),
            serde_json::json!(PNG_B64_MULTILINE),
        );
        data.insert("text/plain".to_string(), serde_json::json!("table"));
        let result = ok_result(vec![CellOutput::DisplayData {
            data,
            metadata: serde_json::json!({}),
        }]);

        let md = format_outputs(&result, &mut test_fig(), true);
        assert!(md.contains("{=html}"), "html must win; got:\n{md}");
        assert!(!md.contains("![]("), "no image when html wins; got:\n{md}");
    }

    fn display(mimes: &[(&str, serde_json::Value)]) -> CellResult {
        let data = mimes
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect();
        ok_result(vec![CellOutput::DisplayData {
            data,
            metadata: serde_json::json!({}),
        }])
    }

    #[test]
    fn test_text_markdown_is_emitted_as_markdown() {
        // Q1: `text/markdown` is written as markdown itself, in a div that also carries
        // `.cell-output-markdown`, not as the `text/plain` fallback in a code fence.
        let result = display(&[
            ("text/markdown", serde_json::json!(["**bold**"])),
            ("text/plain", serde_json::json!("bold")),
        ]);
        assert_eq!(
            format_outputs(&result, &mut test_fig(), true),
            "\n::: {.cell-output .cell-output-display .cell-output-markdown}\n\n**bold**\n\n:::\n"
        );
    }

    #[test]
    fn test_math_latex_is_emitted_as_markdown() {
        // Q1 (`displayDataWithMarkdownMath`): math LaTeX with no `text/markdown` is
        // presented as markdown so pandoc reads the math.
        for latex in ["$x^2$", "$$\\int f$$", "\\begin{align}a\\end{align}"] {
            let result = display(&[
                ("text/latex", serde_json::json!([latex])),
                ("text/plain", serde_json::json!("fallback")),
            ]);
            let md = format_outputs(&result, &mut test_fig(), true);
            assert_eq!(
                md,
                format!(
                    "\n::: {{.cell-output .cell-output-display .cell-output-markdown}}\n\n{latex}\n\n:::\n"
                ),
                "latex {latex:?}"
            );
        }
    }

    #[test]
    fn test_non_math_latex_falls_back_to_plain_text() {
        // For an HTML target Q1 leaves other `text/latex` out of the priority list.
        let result = display(&[
            ("text/latex", serde_json::json!(["\\textbf{x}"])),
            ("text/plain", serde_json::json!("x")),
        ]);
        let md = format_outputs(&result, &mut test_fig(), true);
        assert!(!md.contains("cell-output-markdown"), "got:\n{md}");
        assert!(md.contains("```\nx\n```"), "got:\n{md}");
    }

    #[test]
    fn test_markdown_beats_latex_and_image_but_not_html() {
        let md_over_latex = display(&[
            ("text/markdown", serde_json::json!(["m"])),
            ("text/latex", serde_json::json!(["$l$"])),
        ]);
        let md = format_outputs(&md_over_latex, &mut test_fig(), true);
        assert!(
            md.contains("\n\nm\n\n") && !md.contains("$l$"),
            "got:\n{md}"
        );

        let md_over_image = display(&[
            ("text/markdown", serde_json::json!(["m"])),
            ("image/png", serde_json::json!(PNG_B64_MULTILINE)),
        ]);
        let md = format_outputs(&md_over_image, &mut test_fig(), true);
        assert!(
            md.contains("cell-output-markdown") && !md.contains("![]("),
            "got:\n{md}"
        );

        let html_over_md = display(&[
            ("text/html", serde_json::json!("<b>h</b>")),
            ("text/markdown", serde_json::json!(["m"])),
        ]);
        let md = format_outputs(&html_over_md, &mut test_fig(), true);
        assert!(
            md.contains("{=html}") && !md.contains("cell-output-markdown"),
            "got:\n{md}"
        );
    }

    #[test]
    fn test_markdown_execute_result_is_also_markdown() {
        let result = ok_result(vec![CellOutput::ExecuteResult {
            data: [("text/markdown".to_string(), serde_json::json!("*it*"))]
                .into_iter()
                .collect(),
            metadata: serde_json::json!({}),
            execution_count: 1,
        }]);
        let md = format_outputs(&result, &mut test_fig(), true);
        assert!(
            md.contains(".cell-output-markdown") && md.contains("*it*"),
            "got:\n{md}"
        );
    }

    #[test]
    fn test_two_outputs_in_one_cell_get_distinct_names() {
        let tmp = tempfile::tempdir().unwrap();
        let mut fig = FsFigureSink::new(&tmp.path().join("doc.qmd"));
        fig.begin_cell();

        let png_output = || {
            let mut data = std::collections::HashMap::new();
            data.insert(
                "image/png".to_string(),
                serde_json::json!(PNG_B64_MULTILINE),
            );
            CellOutput::DisplayData {
                data,
                metadata: serde_json::json!({}),
            }
        };
        let result = ok_result(vec![png_output(), png_output()]);

        let md = format_outputs(&result, &mut fig, true);
        assert!(md.contains("cell-1-output-1.png"), "got:\n{md}");
        assert!(md.contains("cell-1-output-2.png"), "got:\n{md}");
        assert!(
            tmp.path()
                .join("doc_files/figure-html/cell-1-output-2.png")
                .exists()
        );
    }

    #[test]
    fn test_invalid_base64_with_text_fallback_uses_text() {
        // Fail-soft: an unwritable image with a text/plain sibling
        // falls back to the text representation rather than a
        // placeholder or a panic.
        let mut data = std::collections::HashMap::new();
        data.insert(
            "image/png".to_string(),
            serde_json::json!("!!!not-base64!!!"),
        );
        data.insert("text/plain".to_string(), serde_json::json!("fallback"));
        let result = ok_result(vec![CellOutput::DisplayData {
            data,
            metadata: serde_json::json!({}),
        }]);

        let md = format_outputs(&result, &mut test_fig(), true);
        assert!(md.contains("fallback"), "got:\n{md}");
        assert!(!md.contains("[Image output]"), "got:\n{md}");
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
            format_outputs(&result, &mut test_fig(), true),
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
            format_outputs(&result, &mut test_fig(), true),
            "\n::: {.cell-output .cell-output-display}\n\n```\n42\n```\n\n:::\n"
        );
    }

    #[test]
    fn test_render_cell_without_output_still_wraps() {
        // A source-only cell (assignment, `output: false`, …) still
        // gets the `.cell` wrapper — the splice must be able to
        // replace the live cell, and knitr wraps output-less chunks
        // too.
        let result = ok_result(vec![]);
        assert_eq!(
            render_cell(
                "python",
                "x = 1\n",
                &result,
                &mut test_fig(),
                CellVisibility::VISIBLE,
            ),
            "::: {.cell}\n\n```{.python .cell-code}\nx = 1\n```\n\n:::\n"
        );
    }
}
