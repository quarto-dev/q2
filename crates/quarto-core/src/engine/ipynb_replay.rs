/*
 * engine/ipynb_replay.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Plan 7c Phase 3: stored-output replay for `.ipynb` documents.
 */

//! Stored-output replay: render an `.ipynb`'s code cells and their
//! *stored* nbformat outputs through the same presenter the live jupyter
//! engine uses (a record of the stored outputs, presented by
//! `present_document`; EXR-6) — no kernel, no jupyter binary, no
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
use std::sync::Arc;

use quarto_system_runtime::SystemRuntime;

use quarto_source_map::SourceInfo;
use serde_json::Value;

use crate::cell_options::partition_cell_options;
use crate::engine::content_processors::ORIGINAL_FILE_ID;
use crate::engine::context::{ExecuteResult, ExecutionContext};
use crate::engine::error::ExecutionError;
use crate::engine::traits::ExecutionEngine;
use crate::engine::units::{Unit, runs_in_legacy_executor};
use crate::presenter::cell::describe_location;
use crate::presenter::document::present_document;
use crate::presenter::figures::{FigureSink, FsFigureSink, RuntimeFigureSink};
use crate::presenter::inline::InlineTarget;
use crate::record::{
    AuthorTexts, EngineEntryInput, HashDefinition, RecordBuilder, UnitResult, fill_ids,
};

/// Renders `.ipynb` documents' stored outputs (Plan 7c Phase 3, option B).
///
/// Deliberately NOT registered in the [`EngineRegistry`] (the stage
/// constructs it directly): it never claims files and is never resolved
/// by the normal engine-resolution path. It rides the same per-engine
/// loop a live engine rides — mask → serialize → execute → unmask →
/// capture → reparse — so capture/splice of `::: {.cell}` wrappers works
/// unchanged.
#[derive(Default)]
pub struct IpynbReplayEngine {
    /// When set, figures are written through this runtime (the WASM VFS) instead of
    /// `std::fs`. The native stage leaves it unset.
    runtime: Option<Arc<dyn SystemRuntime>>,
}

impl IpynbReplayEngine {
    /// Replay writing figures to the real filesystem.
    pub fn new() -> Self {
        Self::default()
    }

    /// Replay writing figures through `runtime`.
    pub fn with_runtime(runtime: Arc<dyn SystemRuntime>) -> Self {
        Self {
            runtime: Some(runtime),
        }
    }
}

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
        replay_stored_outputs(input, ctx, self.runtime.as_ref())
    }
}

/// Render `input` (the serialized qmd) with each fence that maps into a
/// *code* cell replaced by the canonical `::: {.cell}` shape carrying the
/// cell's stored nbformat outputs. Fences mapping into markdown cells —
/// literal ```{lang} examples in prose — pass through verbatim, as do
/// unmapped fences.
///
/// EXR-6 Phase 3: the stored outputs become a record (one cell per replayed
/// fence, ids from the cell source) and `present_document` presents it, the
/// same driver the live Jupyter engine presents its record with.
fn replay_stored_outputs(
    input: &str,
    ctx: &ExecutionContext,
    runtime: Option<&Arc<dyn SystemRuntime>>,
) -> Result<ExecuteResult, ExecutionError> {
    let mut units = crate::engine::units::units_for_engine(&ctx.units, input);
    if !units
        .iter()
        .any(|u| matches!(u, Unit::Cell(c) if runs_in_legacy_executor(c)))
    {
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

    // The record: ids from each unit's own text (content-hash ids, a local scan;
    // nothing here needs the stage's author texts), one cell per fence that maps
    // into a notebook code cell.
    let texts = AuthorTexts::from_scanned(&units);
    fill_ids(&mut units, &texts).map_err(record_error)?;
    let definition = HashDefinition::conservative(IPYNB);
    let mut builder =
        RecordBuilder::new(IPYNB, &definition, &units, &texts).map_err(record_error)?;

    for (index, unit) in units.iter().enumerate() {
        let Unit::Cell(block) = unit else {
            continue;
        };
        if !runs_in_legacy_executor(block) {
            continue;
        }
        let Some(cell_index) = owning_code_cell_index(block.code_start, ctx, cells) else {
            // A literal fence in a markdown cell, a redacted empty cell,
            // or an offset that doesn't map — all stay verbatim.
            continue;
        };

        // Check the `#|` options with the same source-tracked machinery
        // the live path uses, so a bad option line is reported at the
        // owning cell's position (option spans resolve into its virtual
        // file). The presenter partitions again when it renders.
        let body_source = SourceInfo::substring(
            ctx.source_info.clone(),
            block.code_start,
            block.code_start + block.code.len(),
        );
        partition_cell_options(&block.lang, &block.code, body_source.clone()).map_err(|e| {
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

        // Stored error outputs are content — rendered as `-error` divs
        // subject to output visibility. The live path's `error: false`
        // abort policy is execution semantics and does not apply to
        // output that already happened.
        let stored = &cells[cell_index];
        builder
            .push(
                index,
                UnitResult {
                    outputs: stored
                        .get("outputs")
                        .and_then(|v| v.as_array())
                        .cloned()
                        .unwrap_or_default(),
                    execution_count: stored.get("execution_count").and_then(|v| v.as_u64()),
                    kernel: None,
                },
            )
            .map_err(record_error)?;
    }
    let record_cells = builder
        .finish(EngineEntryInput::default())
        .map_err(record_error)?
        .cells;

    let mut fig: Box<dyn FigureSink> = match runtime {
        Some(runtime) => Box::new(RuntimeFigureSink::new(runtime.clone(), &ctx.source_path)),
        None => Box::new(FsFigureSink::new(&ctx.source_path)),
    };
    // Document-level defaults for cell options: the same merged
    // `execute:` scope a live engine execution receives.
    let presented = present_document(
        &record_cells,
        input,
        &units,
        ctx.execute_scope.as_ref(),
        InlineTarget::from_format(&ctx.format),
        &ctx.format,
        None,
        fig.as_mut(),
    )
    .map_err(|e| ExecutionError::execution_failed(IPYNB, e.to_string()))?;

    let mut result = ExecuteResult::new(presented.markdown);
    result.warnings.extend(
        presented
            .notes
            .iter()
            .map(|note| crate::engine::diagnostics::note_diagnostic(note, &units, ctx)),
    );
    if !presented.files_written.is_empty() {
        result = result.with_supporting_files(presented.files_written);
    }
    Ok(result)
}

/// The engine name the replay's record carries.
const IPYNB: &str = "ipynb";

fn record_error(e: impl std::fmt::Display) -> ExecutionError {
    ExecutionError::execution_failed(IPYNB, format!("stored outputs record: {e}"))
}

/// Which notebook cell owns this fence's code content, if it is a *code*
/// cell. The fence's content start is mapped through the source map into
/// the converter's virtual files: cell *i* lives at
/// `ORIGINAL_FILE_ID + 1 + i` (converter-constructed; see the converter's
/// Concat pieces and ParseDocumentStage's registration).
fn owning_code_cell_index(
    code_start: usize,
    ctx: &ExecutionContext,
    cells: &[Value],
) -> Option<usize> {
    let mapped = ctx
        .source_info
        .map_offset(code_start, &ctx.source_context)?;
    let index = mapped.file_id.0.checked_sub(ORIGINAL_FILE_ID.0 + 1)?;
    let cell = cells.get(index)?;
    (cell.get("cell_type").and_then(|t| t.as_str()) == Some("code")).then_some(index)
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

    const BAD_PNG_NB: &str = r##"{"cells":[{"cell_type":"code","execution_count":5,"metadata":{},"outputs":[{"output_type":"display_data","data":{"image/png":"!!!not base64!!!"},"metadata":{}}],"source":["plot()\n"]}],"metadata":{"kernelspec":{"name":"python3","language":"python"}},"nbformat":4,"nbformat_minor":5}"##;

    /// The presenter's image-failure note reaches this consumer as a `Q-18-11` warning
    /// (the sink's failure channel is not dropped here), and the placeholder is literal.
    #[test]
    fn an_unusable_stored_image_warns_q_18_11() {
        let dir = tempfile::tempdir().unwrap();
        let (ctx, markdown) = ctx_for(BAD_PNG_NB, dir.path());
        let out = replay_stored_outputs(&markdown, &ctx, None).unwrap();
        let codes: Vec<_> = out
            .warnings
            .iter()
            .filter_map(|w| w.code.as_deref())
            .collect();
        assert_eq!(codes, ["Q-18-11"], "{:?}", out.warnings);
        assert!(
            out.markdown.contains("\\[Image output\\]"),
            "{}",
            out.markdown
        );
        assert!(out.supporting_files.is_empty());
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
        let out = replay_stored_outputs(&markdown, &ctx, None).unwrap();
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

        let out = replay_stored_outputs(&markdown, &ctx, None).unwrap();
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

        let out = replay_stored_outputs(&markdown, &ctx, None).unwrap();
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

        let out = replay_stored_outputs(&markdown, &ctx, None).unwrap();
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

        let out = replay_stored_outputs(&markdown, &ctx, None).unwrap();
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

        let out = replay_stored_outputs(&markdown, &ctx, None).unwrap();
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

        let out = replay_stored_outputs(&markdown, &ctx, None).unwrap();
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

        let err = replay_stored_outputs(&converted.markdown, &ctx, None).unwrap_err();
        let message = err.to_string();
        assert!(
            message.contains("ORIGINAL_FILE_ID"),
            "the error must name the broken invariant, got: {message}"
        );
    }

    #[test]
    fn png_output_with_a_runtime_writes_through_it_not_the_filesystem() {
        use crate::presenter::recording_runtime::{Call, RecordingRuntime};

        let dir = tempfile::tempdir().unwrap();
        let (ctx, markdown) = ctx_for(PNG_OUTPUT_NB, dir.path());
        let runtime = Arc::new(RecordingRuntime::default());

        let out = IpynbReplayEngine::with_runtime(runtime.clone())
            .execute(&markdown, &ctx)
            .unwrap();

        assert!(
            out.markdown
                .contains("![](notebook_files/figure-html/cell-1-output-1.png)"),
            "the figure must be referenced, not degraded to a placeholder; got:\n{}",
            out.markdown
        );
        assert_eq!(
            out.supporting_files,
            vec![dir.path().join("notebook_files")]
        );
        let calls = runtime.calls.lock().unwrap();
        let wrote_png = calls.iter().any(|c| {
            matches!(c, Call::FileWrite(path, _)
                if path == &dir.path().join("notebook_files/figure-html/cell-1-output-1.png"))
        });
        assert!(
            wrote_png,
            "figure written through the runtime; calls: {calls:?}"
        );
        assert!(
            !dir.path().join("notebook_files").exists(),
            "nothing may be written with std::fs"
        );
    }
}
