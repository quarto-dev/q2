/*
 * engine/jupyter/stored.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Plan 7c Phase 3: stored-output replay for `.ipynb` documents.
 */

//! Stored-output replay: render an `.ipynb`'s code cells and their
//! *stored* nbformat outputs through the same rendering machinery the
//! live jupyter engine uses — no kernel, no jupyter binary, no
//! availability probe.
//!
//! A stored notebook's outputs ARE its document content (Q1 parity):
//! Q1 renders stored outputs by default and re-executes only on
//! demand. q2 follows the same split — `EngineExecutionStage` routes
//! every `.ipynb` whose fully merged metadata does not set
//! `execute.enabled: true` here, and only the demand case reaches a
//! real engine.
//!
//! Alignment between the serialized qmd's fences and the notebook's
//! cells never guesses by order: each fence's `code_start` is mapped
//! through the source map into the owning cell's virtual file
//! (`ORIGINAL_FILE_ID + 1 + i`, converter-constructed), so a literal
//! ```{python} fence inside a markdown cell maps into a markdown cell
//! and stays inert.

use std::path::{Path, PathBuf};

use quarto_source_map::SourceInfo;
use serde_json::Value;

use super::execute::{CellOutput, ExecuteResult as KernelExecuteResult, ExecuteStatus, MimeBundle};
use super::text_execute::{
    CellVisibility, CodeBlock, FigureWriter, describe_location, extract_text_content,
    parse_code_blocks, render_cell, resolve_cell_options,
};
use crate::cell_options::partition_cell_options;
use crate::engine::content_processors::ORIGINAL_FILE_ID;
use crate::engine::context::{ExecuteResult, ExecutionContext};
use crate::engine::error::ExecutionError;
use crate::engine::traits::ExecutionEngine;

/// Renders `.ipynb` documents' stored outputs (Plan 7c Phase 3, option B).
///
/// Deliberately NOT registered in the [`EngineRegistry`] (the stage
/// constructs it directly): it never claims files and is never resolved
/// by the normal engine-resolution path. It rides the same per-engine
/// loop a live engine rides — mask → serialize → execute → unmask →
/// capture → reparse — so capture/splice of `::: {.cell}` wrappers works
/// unchanged.
pub struct IpynbReplayEngine;

impl ExecutionEngine for IpynbReplayEngine {
    fn name(&self) -> &str {
        "ipynb"
    }

    fn is_available(&self) -> bool {
        // Replay needs nothing outside this process.
        true
    }

    fn intermediate_files(&self, input_path: &Path) -> Vec<PathBuf> {
        // Figure files land in the same `<stem>_files/` directory a live
        // jupyter run would produce — declared so `q2 render` copies it.
        let stem = input_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("");

        if stem.is_empty() {
            return Vec::new();
        }

        let parent = input_path.parent().unwrap_or(Path::new("."));
        vec![parent.join(format!("{}_files", stem))]
    }

    fn execute(
        &self,
        input: &str,
        ctx: &ExecutionContext,
    ) -> Result<ExecuteResult, ExecutionError> {
        replay_stored_outputs(input, ctx)
    }
}

/// Render `input` (the serialized qmd) with each fence that maps into a
/// *code* cell replaced by the canonical `::: {.cell}` shape carrying the
/// cell's stored nbformat outputs. Fences mapping into markdown cells —
/// literal ```{lang} examples in prose — pass through verbatim, as do
/// unmapped fences.
fn replay_stored_outputs(
    input: &str,
    ctx: &ExecutionContext,
) -> Result<ExecuteResult, ExecutionError> {
    let blocks = parse_code_blocks(input);
    if blocks.is_empty() {
        // No code-cell fences (markdown-only notebook): passthrough.
        return Ok(ExecuteResult::new(input));
    }

    // The notebook bytes were registered at ORIGINAL_FILE_ID by
    // ParseDocumentStage; the converter's Concat pieces point at that
    // FileId, so finding them here is an invariant, not a convention.
    // A miss means a wiring bug upstream — fail loudly rather than
    // render a notebook silently stripped of its outputs.
    let notebook_bytes = ctx
        .source_context
        .get_file(ORIGINAL_FILE_ID)
        .and_then(|f| f.content.as_deref())
        .ok_or_else(|| {
            ExecutionError::execution_failed(
                "ipynb",
                "internal error: the original notebook is not registered in the source context \
                 at the ORIGINAL_FILE_ID slot, so stored outputs cannot be replayed",
            )
        })?;
    let notebook: Value = serde_json::from_str(notebook_bytes).map_err(|e| {
        ExecutionError::execution_failed("ipynb", format!("stored notebook is not valid JSON: {e}"))
    })?;
    let cells = notebook
        .get("cells")
        .and_then(|c| c.as_array())
        .ok_or_else(|| {
            ExecutionError::execution_failed("ipynb", "stored notebook has no `cells` array")
        })?;

    // Document-level defaults for cell options: the same merged
    // `execute:` scope a live engine execution receives.
    let doc_scope = ctx.execute_scope.clone();

    let mut output = String::new();
    let mut last_end = 0;
    let mut fig = FigureWriter::new(&ctx.source_path);

    for block in blocks {
        let Some(cell_index) = owning_code_cell_index(&block, ctx, cells) else {
            // A literal fence in a markdown cell, a redacted empty cell,
            // or an offset that doesn't map — all stay verbatim.
            continue;
        };

        // Append content before this block, keeping the `::: {.cell}`
        // opener in its own block (same splice discipline as the live path).
        output.push_str(&input[last_end..block.start]);
        if output.ends_with('\n') && !output.ends_with("\n\n") {
            output.push('\n');
        }

        // Partition `#|` options + code with the same source-tracked
        // machinery the live path uses (option spans resolve into the
        // owning cell's virtual file).
        let body_source = SourceInfo::substring(
            ctx.source_info.clone(),
            block.code_start,
            block.code_start + block.code.len(),
        );
        let cell = partition_cell_options(&block.language, &block.code, body_source.clone())
            .map_err(|e| {
                let at = e
                    .location()
                    .and_then(|loc| describe_location(loc, 0, ctx))
                    .or_else(|| describe_location(&body_source, 0, ctx))
                    .map(|l| format!(" at {l}"))
                    .unwrap_or_default();
                ExecutionError::execution_failed(
                    "ipynb",
                    format!("cell options are not valid YAML{at}: {e}"),
                )
            })?;
        let resolved = resolve_cell_options(doc_scope.as_ref(), cell.options);
        let visibility = CellVisibility::resolve(resolved.as_ref());

        // Stored error outputs are content — rendered as `-error` divs
        // subject to output visibility. The live path's `error: false`
        // abort policy is execution semantics and does not apply to
        // output that already happened.
        let exec_result = stored_result_for_cell(&cells[cell_index]);

        // `begin_cell` for every replayed cell, visible or not, so
        // figure names stay keyed to document position (live-path parity).
        fig.begin_cell();
        output.push_str(&render_cell(
            &block.language,
            &cell.code,
            &exec_result,
            &mut fig,
            visibility,
        ));

        last_end = block.end;
    }

    output.push_str(&input[last_end..]);

    let mut result = ExecuteResult::new(output);
    if fig.wrote_any {
        result = result.with_supporting_files(vec![fig.files_dir()]);
    }
    Ok(result)
}

/// Which notebook cell owns this fence's code content, if it is a *code*
/// cell. The fence's content start is mapped through the source map into
/// the converter's virtual files: cell *i* lives at
/// `ORIGINAL_FILE_ID + 1 + i` (converter-constructed; see the converter's
/// Concat pieces and ParseDocumentStage's registration).
fn owning_code_cell_index(
    block: &CodeBlock,
    ctx: &ExecutionContext,
    cells: &[Value],
) -> Option<usize> {
    let mapped = ctx
        .source_info
        .map_offset(block.code_start, &ctx.source_context)?;
    let index = mapped.file_id.0.checked_sub(ORIGINAL_FILE_ID.0 + 1)?;
    let cell = cells.get(index)?;
    (cell.get("cell_type").and_then(|t| t.as_str()) == Some("code")).then_some(index)
}

/// Convert one notebook code cell's stored nbformat outputs into the
/// kernel result shape [`render_cell`] consumes. Handled `output_type`s:
/// `stream`, `execute_result`, `display_data`, `error`; legacy
/// pre-nbformat-4 types (`pyout`/`pyerr`) are skipped — they do not
/// appear in any notebook produced this century. The status is always
/// `Ok`: a stored `error` output is content to show, not a status to
/// abort on.
fn stored_result_for_cell(cell: &Value) -> KernelExecuteResult {
    let execution_count = cell
        .get("execution_count")
        .and_then(|v| v.as_u64())
        .and_then(|v| u32::try_from(v).ok());
    let mut outputs = Vec::new();
    if let Some(list) = cell.get("outputs").and_then(|v| v.as_array()) {
        for out in list {
            match out.get("output_type").and_then(|t| t.as_str()) {
                Some("stream") => outputs.push(CellOutput::Stream {
                    name: out
                        .get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("stdout")
                        .to_string(),
                    text: out
                        .get("text")
                        .map(extract_text_content)
                        .unwrap_or_default(),
                }),
                Some("execute_result") => outputs.push(CellOutput::ExecuteResult {
                    execution_count: out
                        .get("execution_count")
                        .and_then(|v| v.as_u64())
                        .and_then(|v| u32::try_from(v).ok())
                        .unwrap_or(0),
                    data: mime_bundle(out.get("data")),
                    metadata: out.get("metadata").cloned().unwrap_or(Value::Null),
                }),
                Some("display_data") => outputs.push(CellOutput::DisplayData {
                    data: mime_bundle(out.get("data")),
                    metadata: out.get("metadata").cloned().unwrap_or(Value::Null),
                }),
                Some("error") => outputs.push(CellOutput::Error {
                    ename: out
                        .get("ename")
                        .and_then(|v| v.as_str())
                        .unwrap_or("Error")
                        .to_string(),
                    evalue: out
                        .get("evalue")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string(),
                    traceback: out
                        .get("traceback")
                        .and_then(|v| v.as_array())
                        .map(|a| {
                            a.iter()
                                .filter_map(|s| s.as_str())
                                .map(str::to_string)
                                .collect()
                        })
                        .unwrap_or_default(),
                }),
                other => tracing::debug!(
                    output_type = other.unwrap_or("<missing>"),
                    "ipynb replay: skipping unsupported stored output"
                ),
            }
        }
    }
    KernelExecuteResult {
        status: ExecuteStatus::Ok,
        outputs,
        execution_count,
    }
}

/// A nbformat mime bundle (`{"text/html": ...}`) as the kernel's
/// [`MimeBundle`].
fn mime_bundle(value: Option<&Value>) -> MimeBundle {
    value
        .and_then(|v| v.as_object())
        .map(|obj| obj.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use quarto_source_map::SourceContext;

    use crate::engine::content_processors::ipynb::convert_notebook;

    const STDOUT_NB: &str = r##"{"cells":[{"cell_type":"markdown","metadata":{},"source":["# Stored notebook\n\nProse before the cell.\n"]},{"cell_type":"code","execution_count":1,"metadata":{},"outputs":[{"output_type":"stream","name":"stdout","text":["hello from the notebook\n"]}],"source":["print('hello from the notebook')\n"]}],"metadata":{"kernelspec":{"name":"python3","language":"python"}},"nbformat":4,"nbformat_minor":5}"##;

    const HTML_OUTPUT_NB: &str = r##"{"cells":[{"cell_type":"code","execution_count":2,"metadata":{},"outputs":[{"output_type":"display_data","data":{"text/html":["<b>rich stored output</b>"]},"metadata":{}}],"source":["from IPython.display import HTML\n"]}],"metadata":{"kernelspec":{"name":"python3","language":"python"}},"nbformat":4,"nbformat_minor":5}"##;

    const ERROR_OUTPUT_NB: &str = r##"{"cells":[{"cell_type":"code","execution_count":3,"metadata":{},"outputs":[{"output_type":"error","ename":"ZeroDivisionError","evalue":"division by zero","traceback":["traceback frame 1"]}],"source":["1 / 0\n"]}],"metadata":{"kernelspec":{"name":"python3","language":"python"}},"nbformat":4,"nbformat_minor":5}"##;

    const LITERAL_FENCE_NB: &str = r##"{"cells":[{"cell_type":"markdown","metadata":{},"source":["A markdown cell showing a fence:\n\n```{python}\nprint('not a real cell')\n```\n\nDone.\n"]},{"cell_type":"code","execution_count":4,"metadata":{},"outputs":[{"output_type":"stream","name":"stdout","text":["real cell ran\n"]}],"source":["print('real cell ran')\n"]}],"metadata":{"kernelspec":{"name":"python3","language":"python"}},"nbformat":4,"nbformat_minor":5}"##;

    const PNG_OUTPUT_NB: &str = r##"{"cells":[{"cell_type":"code","execution_count":5,"metadata":{},"outputs":[{"output_type":"display_data","data":{"image/png":"iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==","text/plain":["<Figure>"]},"metadata":{}}],"source":["import matplotlib.pyplot as plt\n"]}],"metadata":{"kernelspec":{"name":"python3","language":"python"}},"nbformat":4,"nbformat_minor":5}"##;

    const MARKDOWN_ONLY_NB: &str = r##"{"cells":[{"cell_type":"markdown","metadata":{},"source":["# No code cells at all\n"]}],"metadata":{"kernelspec":{"name":"python3"}},"nbformat":4,"nbformat_minor":5}"##;

    const ECHO_FALSE_NB: &str = r##"{"cells":[{"cell_type":"code","execution_count":7,"metadata":{},"outputs":[{"output_type":"stream","name":"stdout","text":["only the output\n"]}],"source":["#| echo: false\nprint('only the output')\n"]}],"metadata":{"kernelspec":{"name":"python3","language":"python"}},"nbformat":4,"nbformat_minor":5}"##;

    /// A context mirroring production registration exactly: converted
    /// buffer at FileId(0), original notebook at ORIGINAL_FILE_ID, cell
    /// virtual files contiguous after — what ParseDocumentStage registers
    /// (see its Plan 7c decision 6 block). Returns the context and the
    /// converted markdown (what the engine's `input` is in production).
    fn ctx_for(notebook: &str, dir: &Path) -> (ExecutionContext, String) {
        let nb_path = dir.join("notebook.ipynb");
        let converted = convert_notebook(&nb_path, notebook).expect("notebook must convert");
        let mut sc = SourceContext::new();
        sc.add_file(
            "<notebook.ipynb (converted by ipynb)>".to_string(),
            Some(converted.markdown.clone()),
        );
        sc.add_file_with_id(
            ORIGINAL_FILE_ID,
            nb_path.display().to_string(),
            Some(notebook.to_string()),
        );
        for cell_file in &converted.files {
            sc.add_file(cell_file.label.clone(), Some(cell_file.text.clone()));
        }
        let ctx = ExecutionContext::new(dir.to_path_buf(), dir.to_path_buf(), nb_path, "html")
            .with_source_info(converted.source_info, Arc::new(sc));
        (ctx, converted.markdown)
    }

    #[test]
    fn stdout_cell_replays_in_canonical_shape() {
        let dir = tempfile::tempdir().unwrap();
        let (ctx, markdown) = ctx_for(STDOUT_NB, dir.path());

        let fence = "```{python}\nprint('hello from the notebook')\n```\n";
        assert!(
            markdown.contains(fence),
            "converter must emit the code cell as a {{python}} fence, got:\n{markdown}"
        );
        let out = replay_stored_outputs(&markdown, &ctx).unwrap();
        let expected = markdown.replacen(
            fence,
            "::: {.cell}\n\n```{.python .cell-code}\nprint('hello from the notebook')\n```\n\n::: {.cell-output .cell-output-stdout}\n\n```\nhello from the notebook\n```\n\n:::\n\n:::\n",
            1,
        );
        assert_eq!(out.markdown, expected, "full replayed markdown mismatch");
    }

    #[test]
    fn markdown_cell_literal_fence_stays_inert() {
        let dir = tempfile::tempdir().unwrap();
        let (ctx, markdown) = ctx_for(LITERAL_FENCE_NB, dir.path());

        let out = replay_stored_outputs(&markdown, &ctx).unwrap();
        assert!(
            out.markdown.contains("not a real cell"),
            "markdown-cell fence content must survive verbatim, got:\n{}",
            out.markdown
        );
        assert!(
            out.markdown.contains("real cell ran"),
            "the real code cell's stored output must replay, got:\n{}",
            out.markdown
        );
        assert_eq!(
            out.markdown.matches("cell-code").count(),
            1,
            "exactly one echoed code cell — the markdown cell's literal fence must not become one:\n{}",
            out.markdown
        );
    }

    #[test]
    fn error_output_renders_error_div() {
        let dir = tempfile::tempdir().unwrap();
        let (ctx, markdown) = ctx_for(ERROR_OUTPUT_NB, dir.path());

        let out = replay_stored_outputs(&markdown, &ctx).unwrap();
        assert!(
            out.markdown.contains("ZeroDivisionError"),
            "got:\n{}",
            out.markdown
        );
        assert!(
            out.markdown.contains("division by zero"),
            "got:\n{}",
            out.markdown
        );
        assert!(
            out.markdown.contains("cell-output-error"),
            "stored error must render as an error div, got:\n{}",
            out.markdown
        );
    }

    #[test]
    fn html_output_renders_raw_html() {
        let dir = tempfile::tempdir().unwrap();
        let (ctx, markdown) = ctx_for(HTML_OUTPUT_NB, dir.path());

        let out = replay_stored_outputs(&markdown, &ctx).unwrap();
        assert!(
            out.markdown.contains("<b>rich stored output</b>"),
            "got:\n{}",
            out.markdown
        );
        assert!(
            out.markdown.contains("cell-output-display"),
            "got:\n{}",
            out.markdown
        );
        assert!(
            out.markdown.contains("{=html}"),
            "html must be emitted as a raw block, got:\n{}",
            out.markdown
        );
    }

    #[test]
    fn png_output_writes_figure_and_reports_supporting_files() {
        let dir = tempfile::tempdir().unwrap();
        let (ctx, markdown) = ctx_for(PNG_OUTPUT_NB, dir.path());

        let out = replay_stored_outputs(&markdown, &ctx).unwrap();
        assert!(
            out.markdown
                .contains("notebook_files/figure-html/cell-1-output-1.png"),
            "got:\n{}",
            out.markdown
        );
        let fig = dir
            .path()
            .join("notebook_files/figure-html/cell-1-output-1.png");
        assert!(
            fig.is_file(),
            "figure file must be written at {}",
            fig.display()
        );
        assert_eq!(
            out.supporting_files,
            vec![dir.path().join("notebook_files")],
            "the `<stem>_files` dir must be reported as a supporting file"
        );
    }

    #[test]
    fn cell_echo_false_suppresses_source_only() {
        let dir = tempfile::tempdir().unwrap();
        let (ctx, markdown) = ctx_for(ECHO_FALSE_NB, dir.path());

        let out = replay_stored_outputs(&markdown, &ctx).unwrap();
        assert!(
            !out.markdown.contains("cell-code"),
            "echo: false must suppress the echoed source, got:\n{}",
            out.markdown
        );
        assert!(
            out.markdown.contains("only the output"),
            "output must still render, got:\n{}",
            out.markdown
        );
    }

    #[test]
    fn markdown_only_notebook_is_passthrough() {
        let dir = tempfile::tempdir().unwrap();
        let (ctx, markdown) = ctx_for(MARKDOWN_ONLY_NB, dir.path());

        let out = replay_stored_outputs(&markdown, &ctx).unwrap();
        assert_eq!(out.markdown, markdown);
    }

    #[test]
    fn missing_original_registration_is_a_loud_error() {
        let dir = tempfile::tempdir().unwrap();
        // Build the input but leave the ORIGINAL_FILE_ID slot without
        // content (a disk-backed registration whose bytes are not in the
        // context) — breaking the ParseDocumentStage invariant. Replay
        // must refuse rather than render a notebook stripped of outputs.
        let nb_path = dir.path().join("notebook.ipynb");
        let converted = convert_notebook(&nb_path, STDOUT_NB).unwrap();
        let mut sc = SourceContext::new();
        sc.add_file(
            "<notebook.ipynb (converted by ipynb)>".to_string(),
            Some(converted.markdown.clone()),
        );
        sc.add_file_with_id(ORIGINAL_FILE_ID, nb_path.display().to_string(), None);
        for cell_file in &converted.files {
            sc.add_file(cell_file.label.clone(), Some(cell_file.text.clone()));
        }
        let ctx = ExecutionContext::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            nb_path,
            "html",
        )
        .with_source_info(converted.source_info, Arc::new(sc));

        let err = replay_stored_outputs(&converted.markdown, &ctx).unwrap_err();
        let message = err.to_string();
        assert!(
            message.contains("ORIGINAL_FILE_ID"),
            "the error must name the broken invariant, got: {message}"
        );
    }
}
