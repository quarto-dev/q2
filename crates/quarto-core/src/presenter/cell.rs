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

use super::ansi::output_text;
use super::engine_attrs::{ClassPlacement, EngineAttrs, build_head};
use super::figures::FigureSink;
use super::options::{CellOptions, FenceKind, OutputKind};
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
    render_cell_with(
        language,
        code,
        result,
        &CellOptions::default(),
        fig,
        visibility,
    )
}

/// [`render_cell`] with the cell-option layer (KNR-2 Phase 2C): `options` adds classes,
/// attributes and cell-div attributes. `CellOptions::default()` is exactly [`render_cell`].
pub fn render_cell_with(
    language: &str,
    code: &str,
    result: &CellResult,
    options: &CellOptions,
    fig: &mut dyn FigureSink,
    visibility: CellVisibility,
) -> String {
    render_cell_whole(language, code, result, options, &[], fig, visibility)
}

/// The plots of a cell result that become figures (included graphics carry no figure id).
fn count_plots(result: &CellResult) -> usize {
    result
        .outputs
        .iter()
        .filter(|o| is_plot_output(o) && !is_included_graphic(o))
        .count()
}

/// [`render_cell_with`] knowing where each expression of `code` starts (`starts`: line
/// indices, empty when unknown), which knitr's `prompt` needs.
fn render_cell_whole(
    language: &str,
    code: &str,
    result: &CellResult,
    options: &CellOptions,
    starts: &[usize],
    fig: &mut dyn FigureSink,
    visibility: CellVisibility,
) -> String {
    let options = &options.clone().with_plot_count(count_plots(result));
    let source = if visibility.echo && !code.trim().is_empty() {
        echoed_source_fence_with(language, code, options, starts)
    } else {
        String::new()
    };
    let outputs = if visibility.output {
        format_outputs_with(result, fig, visibility.warning, options, &mut 0, 0)
    } else {
        String::new()
    };
    if source.is_empty() && outputs.trim().is_empty() {
        return String::new();
    }
    let body = format!("{source}\n{outputs}");
    let body = if options.collapse() {
        collapse_fences(&body)
    } else {
        body
    };
    wrap_cell(options, &body)
}

/// [`render_cell`] with the source and the outputs alternating (KNR-2, epic decision 21):
/// `anchors[i]` is the number of `code` lines that precede `result.outputs[i]`. Outputs with
/// equal anchors form one group, the source lines between two anchors form one fence, and
/// source after the last output gets its own fence. Without usable anchors (absent, wrong
/// length, decreasing, past the end, or every output after all the source), or when echo or
/// output is hidden, this is exactly [`render_cell`], so cells that never carried anchors
/// keep their bytes.
pub fn render_cell_interleaved(
    language: &str,
    code: &str,
    result: &CellResult,
    anchors: Option<&[usize]>,
    starts: &[usize],
    options: &CellOptions,
    fig: &mut dyn FigureSink,
    visibility: CellVisibility,
) -> String {
    let selected;
    let (code, anchors, starts) = match options.echo_selection().filter(|_| !starts.is_empty()) {
        Some(selection) => {
            selected = select_expressions(code, starts, anchors, selection);
            (
                selected.0.as_str(),
                selected.2.as_deref(),
                selected.1.as_slice(),
            )
        }
        None => (code, anchors, starts),
    };
    let filtered;
    let (result, anchors, options) = match select_by_kind(result, anchors, options) {
        Some(f) => {
            filtered = f;
            (&filtered.0, filtered.1.as_deref(), &filtered.2)
        }
        None => (result, anchors, options),
    };
    let counted = options.clone().with_plot_count(count_plots(result));
    let options = &counted;
    let total = code.lines().count();
    let usable = anchors.filter(|a| {
        a.len() == result.outputs.len()
            && a.windows(2).all(|w| w[0] <= w[1])
            && a.iter().all(|n| *n <= total)
            && a.iter().any(|n| *n < total)
    });
    let Some(anchors) = usable.filter(|_| visibility.echo && visibility.output) else {
        return render_cell_whole(language, code, result, options, starts, fig, visibility);
    };
    let body = interleave_group(
        language,
        &OutputGroup {
            code,
            outputs: &result.outputs,
            anchors,
            starts,
        },
        options,
        fig,
        visibility.warning,
    );
    if body.trim().is_empty() {
        return String::new();
    }
    let body = if options.collapse() {
        collapse_fences(&body)
    } else {
        body
    };
    wrap_cell(options, &body)
}

/// knitr's numeric `echo` (`filter_evaluate`): keep the source expressions `selection` names
/// (1-based; negative numbers name the ones to leave out, zero and out-of-range numbers
/// select nothing) and drop the rest, with every line of a dropped expression. Returns the
/// code, the expression starts, and the anchors, all remapped to the surviving lines.
fn select_expressions(
    code: &str,
    starts: &[usize],
    anchors: Option<&[usize]>,
    selection: &[i64],
) -> (String, Vec<usize>, Option<Vec<usize>>) {
    let keep = |k: i64| -> bool {
        if selection.iter().any(|&s| s < 0) {
            !selection.contains(&-k)
        } else {
            selection.contains(&k)
        }
    };
    let lines: Vec<&str> = code.lines().collect();
    // `kept_before[i]`: surviving lines among the first `i` lines.
    let mut kept_before = vec![0usize];
    let mut out: Vec<&str> = Vec::new();
    let mut new_starts = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let expr = starts.iter().rposition(|&s| s <= i); // 0-based; None before the first
        let shown = expr.is_none_or(|e| keep(e as i64 + 1));
        if shown {
            if expr.is_some_and(|e| starts[e] == i) {
                new_starts.push(out.len());
            }
            out.push(line);
        }
        kept_before.push(out.len());
    }
    let mut text = out.join("\n");
    if !text.is_empty() && code.ends_with('\n') {
        text.push('\n');
    }
    let anchors = anchors.map(|a| {
        a.iter()
            .map(|&x| kept_before[x.min(lines.len())])
            .collect::<Vec<_>>()
    });
    (text, new_starts, anchors)
}

/// knitr's numeric `warning` and `message` (`filter_evaluate`): keep the warnings (messages)
/// the numbers name, counted among the cell's warnings (messages) only. Returns the filtered
/// result, anchors and options (whose kinds follow the surviving outputs), or `None` when no
/// numeric selection applies.
fn select_by_kind(
    result: &CellResult,
    anchors: Option<&[usize]>,
    options: &CellOptions,
) -> Option<(CellResult, Option<Vec<usize>>, CellOptions)> {
    let (warn, msg) = (options.warning_selection(), options.message_selection());
    let kinds = options.output_kinds();
    if (warn.is_none() && msg.is_none()) || kinds.len() != result.outputs.len() {
        return None;
    }
    let keep = |sel: &[i64], k: i64| {
        if sel.iter().any(|&s| s < 0) {
            !sel.contains(&-k)
        } else {
            sel.contains(&k)
        }
    };
    let (mut warnings, mut messages) = (0i64, 0i64);
    let kept: Vec<bool> = kinds
        .iter()
        .map(|kind| match kind {
            Some(OutputKind::Warning) => {
                warnings += 1;
                warn.is_none_or(|s| keep(s, warnings))
            }
            Some(OutputKind::Message) => {
                messages += 1;
                msg.is_none_or(|s| keep(s, messages))
            }
            None => true,
        })
        .collect();
    let pick = |i: usize| kept[i];
    let outputs = result
        .outputs
        .iter()
        .enumerate()
        .filter(|(i, _)| pick(*i))
        .map(|(_, o)| o.clone())
        .collect();
    let anchors = anchors.map(|a| {
        a.iter()
            .enumerate()
            .filter(|(i, _)| pick(*i))
            .map(|(_, n)| *n)
            .collect()
    });
    let kinds = kinds
        .iter()
        .enumerate()
        .filter(|(i, _)| pick(*i))
        .map(|(_, k)| *k)
        .collect();
    Some((
        CellResult {
            status: result.status.clone(),
            outputs,
            execution_count: result.execution_count,
        },
        anchors,
        options.clone().with_output_kinds(kinds),
    ))
}

/// An `include_graphics` image (the mapper marks it `metadata.quarto_knitr.include_graphics`):
/// its own path (`metadata.quarto_file_path`, epic decision 25), and the width knitr computed,
/// when it did.
fn included_graphic(metadata: &serde_json::Value) -> Option<(String, Option<f64>)> {
    let k = metadata.get("quarto_knitr")?;
    if k.get("include_graphics")?.as_bool() != Some(true) {
        return None;
    }
    Some((
        metadata.get("quarto_file_path")?.as_str()?.to_string(),
        k.get("width").and_then(|w| w.as_f64()),
    ))
}

/// A figure\'s markdown in its output div, or bare for a linked image under `results = "asis"`
/// (hooks.R) and for any figure under Jupyter `output: asis` (`jupyter.ts`).
fn push_figure(
    output: &mut String,
    options: &CellOptions,
    index: usize,
    md: &str,
    cell_output: &CellOutput,
) {
    if options.bare_linked_image(index) || options.output_asis() {
        output.push_str(&format!("\n{md}\n"));
    } else {
        let head = output_div_head(None, ".cell-output-display", cell_output);
        output.push_str(&format!("\n::: {{{head}}}\n\n{md}\n\n:::\n"));
    }
}

/// The engine\'s `quarto_attrs` for an output, when it is a kind that keeps metadata
/// (display_data, execute_result). Entries the grammar refuses were already reported by the
/// cell step; here they are skipped silently.
fn output_engine_attrs(output: &CellOutput) -> Option<EngineAttrs> {
    match output {
        CellOutput::DisplayData { metadata, .. } | CellOutput::ExecuteResult { metadata, .. } => {
            EngineAttrs::from_holder(metadata).map(|(a, _)| a)
        }
        CellOutput::Stream { .. } | CellOutput::Error { .. } => None,
    }
}

/// The head of an output div: the presenter\'s id (if any) and space-separated `.class`
/// text, merged with the output\'s engine attributes (engine classes first).
fn output_div_head(id: Option<&str>, classes: &str, output: &CellOutput) -> String {
    let classes: Vec<String> = classes
        .split_whitespace()
        .map(|c| c.trim_start_matches('.').to_string())
        .collect();
    build_head(
        id,
        &classes,
        &[],
        output_engine_attrs(output).as_ref(),
        ClassPlacement::Before,
    )
}

/// The `width` and `height` a stored output carries for its image (`metadata[mime]`, the
/// nbformat place for them), as the text of the number.
fn metadata_size(metadata: &serde_json::Value, mime: &str) -> (Option<String>, Option<String>) {
    let read = |key: &str| {
        metadata
            .get(mime)
            .and_then(|m| m.get(key))
            .filter(|v| v.is_number())
            .map(|v| v.to_string())
    };
    (read("width"), read("height"))
}

fn widget_output(o: &CellOutput) -> bool {
    matches!(o, CellOutput::DisplayData { metadata, .. }
        if metadata["quarto_knitr"]["widget"] == true)
}

/// Whether `o` is an `include_graphics` image (see [`included_graphic`]), which knitr writes
/// without its plot hook.
pub(crate) fn is_included_graphic(o: &CellOutput) -> bool {
    matches!(o, CellOutput::DisplayData { metadata, .. } if included_graphic(metadata).is_some())
}

pub(crate) fn is_plot_output(o: &CellOutput) -> bool {
    matches!(o, CellOutput::DisplayData { data, .. }
        if ["image/svg+xml", "image/png", "image/jpeg"].iter().any(|m| data.contains_key(*m)))
}

/// The `.cell` div around `body`, or `body` alone when `results = "asis"` needs no div.
fn wrap_cell(options: &CellOptions, body: &str) -> String {
    if options.needs_cell_div() {
        format!(
            "::: {{{}}}\n\n{body}\n{}:::\n",
            options.cell_div_head(),
            options.cell_caption()
        )
    } else {
        format!("\n{body}")
    }
}

/// The markdown a `knit_asis` output (marked `quarto_markdown_passthrough`) is written as.
/// Raw HTML that is a whole element (a table, not the `kable-table` div) gets the five-tick
/// `{=html}` fence of Q1\'s `wrap_asis_output` (patch.R), longer if the HTML holds that many
/// backticks; other text is document markdown.
fn knit_asis_text(
    data: &crate::presenter::output::MimeBundle,
    output: &CellOutput,
) -> Option<String> {
    if !markdown_passthrough(output) {
        return None;
    }
    if let Some(md) = data.get("text/markdown") {
        return Some(extract_text_content(md));
    }
    let html = extract_text_content(data.get("text/html")?);
    let trimmed = html.trim_end();
    let word = |s: &str| {
        s.chars()
            .next()
            .is_some_and(|c| c.is_alphanumeric() || c == '_')
    };
    let opens = html.strip_prefix('<').is_some_and(|r| {
        word(r)
            && r.trim_start_matches(|c: char| c.is_alphanumeric() || c == '_')
                .starts_with([' ', '>'])
    });
    let closes =
        trimmed.ends_with('>') && trimmed.rfind("</").is_some_and(|i| word(&trimmed[i + 2..]));
    if opens && closes && !html.starts_with("<div class=\"kable-table\">") {
        // A table gets Q1\'s five-tick `wrap_asis_output` fence; an htmlwidget keeps the
        // three ticks knitr wrote (longer only when its HTML needs it).
        let widget = matches!(output, CellOutput::DisplayData { metadata, .. }
            if metadata["quarto_knitr"]["widget"] == true);
        let ticks = "`".repeat(std::cmp::max(
            if widget { 3 } else { 5 },
            ticks_for_code(&html).len(),
        ));
        return Some(format!("{ticks}{{=html}}\n{html}\n{ticks}"));
    }
    Some(html)
}

/// Printed text in knitr\'s sense: what `results` hides or writes as is.
fn is_printed_text(output: &CellOutput) -> bool {
    // `knit_asis` text is a character vector to knitr, so `results` treats it as printed
    // text; an htmlwidget (marked by the mapper) is not.
    if markdown_passthrough(output) {
        return !matches!(output, CellOutput::DisplayData { metadata, .. }
            if metadata["quarto_knitr"]["widget"] == true);
    }
    match output {
        CellOutput::Stream { name, .. } => name != "stderr",
        CellOutput::ExecuteResult { data, .. } | CellOutput::DisplayData { data, .. } => {
            data.keys().all(|m| m == "text/plain")
        }
        CellOutput::Error { .. } => false,
    }
}

/// knitr\'s collapse merge (`sub("\n```\n+```(.*?)\n", "\n", x)`): a closing fence followed
/// only by blank lines and another code fence (not a raw `{=...}` block) is dropped with that
/// fence\'s opening line, so the two bodies share one fence.
fn collapse_fences(md: &str) -> String {
    let lines: Vec<&str> = md.lines().collect();
    let mut out: Vec<&str> = Vec::new();
    let mut open: Option<usize> = None; // backtick count of the open fence
    let mut i = 0;
    while i < lines.len() {
        let l = lines[i];
        let ticks = l.chars().take_while(|c| *c == '`').count();
        match open {
            None if ticks >= 3 => {
                open = Some(ticks);
                out.push(l);
            }
            Some(n) if ticks >= n && l[ticks..].trim().is_empty() => {
                let mut j = i + 1;
                while j < lines.len() && lines[j].trim().is_empty() {
                    j += 1;
                }
                let next_ticks = lines
                    .get(j)
                    .map_or(0, |n| n.chars().take_while(|c| *c == '`').count());
                if next_ticks >= 3 && !lines[j][next_ticks..].starts_with("{=") {
                    // Stay in the fence: swallow the closer, the blanks and the opener.
                    open = Some(next_ticks);
                    i = j + 1;
                    continue;
                }
                open = None;
                out.push(l);
            }
            _ => out.push(l),
        }
        i += 1;
    }
    let mut s = out.join("\n");
    if md.ends_with('\n') {
        s.push('\n');
    }
    s
}

/// One run of source with the outputs interleaved into it. A cell is one implicit group
/// covering all its outputs; the interleaving works on a group so that a later producer of
/// sub-units (knitr `child=`, `knit_child()`) can hand it a slice of a cell's outputs with
/// anchors relative to that sub-unit's own source. `anchors` is parallel to `outputs` and
/// must already be validated (see [`render_cell_interleaved`]).
pub(crate) struct OutputGroup<'a> {
    pub code: &'a str,
    pub outputs: &'a [CellOutput],
    pub anchors: &'a [usize],
    /// Line indices (into `code`) where each source expression starts; empty when unknown.
    pub starts: &'a [usize],
}

/// The body of an interleaved group: source fences and output blocks, no `.cell` wrapper.
pub(crate) fn interleave_group(
    language: &str,
    group: &OutputGroup<'_>,
    options: &CellOptions,
    fig: &mut dyn FigureSink,
    warning: bool,
) -> String {
    let (code, anchors) = (group.code, group.anchors);
    let mut figures = 0usize; // figures written so far, across the runs
    let lines: Vec<&str> = code.lines().collect();
    let mut body = String::new();
    let mut i = 0usize;
    let mut pending_from = 0usize; // first source line not yet written to a fence
    while i < anchors.len() {
        let at = anchors[i];
        let mut j = i;
        while j < anchors.len() && anchors[j] == at {
            j += 1;
        }
        let i0 = i; // index of the run\'s first output in the cell
        let run = CellResult {
            status: ExecuteStatus::Ok,
            outputs: group.outputs[i..j].to_vec(),
            execution_count: None,
        };
        let shown = format_outputs_with(&run, fig, warning, options, &mut figures, i0);
        i = j;
        // A hidden plot is still a piece in knitr, so it splits the source like a shown one.
        let hidden_plot = options.fig_show_hide() && run.outputs.iter().any(is_plot_output);
        if shown.trim().is_empty() && !hidden_plot {
            // Nothing to show here: the source around it stays in one fence.
            continue;
        }
        let src = lines[pending_from..at].join("\n");
        if !src.trim().is_empty() {
            let rel = starts_within(group.starts, pending_from, at);
            body.push_str(&echoed_source_fence_with(language, &src, options, &rel));
            body.push('\n');
        }
        pending_from = at;
        if !shown.trim().is_empty() {
            body.push_str(&shown);
            body.push('\n');
        }
    }
    let done = pending_from; // source lines already emitted
    let rest = lines[done..].join("\n");
    if !rest.trim().is_empty() {
        let rel = starts_within(group.starts, done, lines.len());
        body.push_str(&echoed_source_fence_with(language, &rest, options, &rel));
        body.push('\n');
    }
    body
}

/// Which outputs knitr holds to the end of the chunk: `fig.show = "hold"` holds plots,
/// `results = "hold"` holds printed text.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HoldMode {
    pub plots: bool,
    pub text: bool,
}

/// Apply knitr's holds at present time (epic decision 21). The record keeps capture order;
/// this moves held outputs to the end, anchored at `total` source lines. Held printed text
/// (stdout streams) merges into one stream and comes before held plots. Returns the
/// reordered outputs with their anchors (kept in step).
pub fn apply_holds(
    outputs: Vec<CellOutput>,
    anchors: &[usize],
    total: usize,
    mode: HoldMode,
) -> (Vec<CellOutput>, Vec<usize>) {
    let is_plot = |o: &CellOutput| {
        matches!(o, CellOutput::DisplayData { data, .. }
            if ["image/svg+xml", "image/png", "image/jpeg"].iter().any(|m| data.contains_key(*m)))
    };
    let mut kept = Vec::new();
    let mut kept_anchors = Vec::new();
    let mut held_text = String::new();
    let mut held_plots = Vec::new();
    for (o, a) in outputs.into_iter().zip(anchors.iter().copied()) {
        match o {
            CellOutput::Stream { ref name, ref text } if mode.text && name == "stdout" => {
                held_text.push_str(text);
            }
            o if mode.plots && is_plot(&o) => held_plots.push(o),
            o => {
                kept.push(o);
                kept_anchors.push(a);
            }
        }
    }
    if !held_text.is_empty() {
        kept.push(CellOutput::Stream {
            name: "stdout".to_string(),
            text: held_text,
        });
        kept_anchors.push(total);
    }
    for p in held_plots {
        kept.push(p);
        kept_anchors.push(total);
    }
    (kept, kept_anchors)
}

/// `fig.beforecode = TRUE` (knitr `fig_before_code`, block.R): a plot that comes after the
/// first code moves to just before the source it followed. Source between two outputs is one
/// run (knitr merges adjacent source), so the plot's new anchor is where its run began: the
/// anchor of the output before it, or 0. Outputs keep their order; only anchors change. A plot
/// anchored at 0 (before any code) stays. Run before [`apply_holds`], as knitr does.
pub fn before_code_anchors(outputs: &[CellOutput], anchors: &[usize]) -> Vec<usize> {
    let mut moved = anchors.to_vec();
    for (i, o) in outputs.iter().enumerate() {
        if is_plot_output(o) && anchors.get(i).is_some_and(|a| *a > 0) {
            moved[i] = if i == 0 { 0 } else { anchors[i - 1] };
        }
    }
    moved
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
#[cfg(test)]
fn echoed_source_fence(language: &str, code: &str) -> String {
    echoed_source_fence_with(language, code, &CellOptions::default(), &[])
}

/// The expression starts in `[from, to)`, relative to `from`.
fn starts_within(starts: &[usize], from: usize, to: usize) -> Vec<usize> {
    starts
        .iter()
        .filter(|s| (from..to).contains(*s))
        .map(|s| s - from)
        .collect()
}

fn echoed_source_fence_with(
    language: &str,
    code: &str,
    options: &CellOptions,
    starts: &[usize],
) -> String {
    if let Some((params, yaml)) = options.fenced_echo_chunk() {
        // `echo: fenced` (hooks.R): the chunk as written, inside a fence one tick longer.
        let body = code.trim_matches('\n');
        let all = if yaml.is_empty() {
            body.to_string()
        } else {
            format!("{yaml}\n{body}")
        };
        let inner = ticks_for_code(&all);
        let outer = format!("{inner}`");
        return format!("{outer}{{.cell-code}}\n{inner}{{{{{params}}}}}\n{all}\n{inner}\n{outer}");
    }
    let code = options.prompt_source(code.trim_end_matches('\n'), starts);
    let code = code.as_str();
    let ticks = ticks_for_code(code);
    let id = options.source_id_prefix();
    let classes = options.source_classes();
    let overflow = options.source_overflow();
    let attrs = options.source_attrs();
    format!("{ticks}{{{id}.{language}{classes} .cell-code{overflow}{attrs}}}\n{code}\n{ticks}")
}

/// Wrap already-trimmed output text in a `::: {<classes>}` div around
/// a plain fence, with the fence sized to the content.
/// Whether an output carries `metadata.quarto_markdown_passthrough == true`: its
/// `text/markdown` is document markdown to splice as is (epic decision 22, engine-neutral).
/// Jupyter outputs never carry it, so what it gates leaves their bytes alone.
fn markdown_passthrough(output: &CellOutput) -> bool {
    matches!(
        output,
        CellOutput::DisplayData { metadata, .. }
            if metadata.get("quarto_markdown_passthrough").and_then(|v| v.as_bool())
                == Some(true)
    )
}

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

fn fenced_output_div(
    classes: &str,
    text: &str,
    fence_attrs: Option<String>,
    options: &CellOptions,
    cell_output: Option<&CellOutput>,
) -> String {
    let ticks = ticks_for_code(text);
    let open = fence_attrs.map_or_else(|| ticks.clone(), |a| format!("{ticks}{{{a}}}"));
    if options.collapse() || options.results_asis() || options.output_asis() {
        // knitr\'s hook returns the bare fence under `collapse`; `collapse_fences` merges it.
        // Jupyter `output: asis` writes every output without its div (`jupyter.ts`).
        return format!("\n{open}\n{text}\n{ticks}\n");
    }
    let head = match cell_output {
        Some(o) => output_div_head(None, classes, o),
        None => classes.to_string(),
    };
    format!("\n::: {{{head}}}\n\n{open}\n{text}\n{ticks}\n\n:::\n")
}

/// `body` in a `::: {<classes>}` output div, or bare under Jupyter `output: asis`, which
/// writes no output div (`jupyter.ts`).
fn output_div(
    classes: &str,
    body: &str,
    options: &CellOptions,
    cell_output: &CellOutput,
) -> String {
    if options.output_asis() {
        format!("\n{body}\n")
    } else {
        let head = output_div_head(None, classes, cell_output);
        format!("\n::: {{{head}}}\n\n{body}\n\n:::\n")
    }
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
#[cfg(test)]
fn format_outputs(result: &CellResult, fig: &mut dyn FigureSink, show_warnings: bool) -> String {
    format_outputs_with(
        result,
        fig,
        show_warnings,
        &CellOptions::default(),
        &mut 0,
        0,
    )
}

fn format_outputs_with(
    result: &CellResult,
    fig: &mut dyn FigureSink,
    show_warnings: bool,
    options: &CellOptions,
    figures: &mut usize,
    first: usize,
) -> String {
    const IMAGE_MIMES: [&str; 3] = ["image/svg+xml", "image/png", "image/jpeg"];

    let mut output = String::new();

    for (n, cell_output) in result.outputs.iter().enumerate() {
        // `warning: false` drops stderr streams — where Python's
        // `warnings` module and most library chatter land. Q1 applies
        // exactly this filter (`isWarningOutput`: output_type
        // "stream" with name "stderr") before emitting anything.
        // Errors are a separate output kind and are never dropped
        // here; the error *policy* decides those.
        //
        // When the record says what kind each output is (Julia: `output_kinds`), only the
        // outputs of kind `Warning` go, so `@info` and a raw `println(stderr, ...)` stay
        // (OPR-4d). With no kind for this output (Jupyter, or a record without the fact),
        // every stderr stream goes, as above.
        if !show_warnings {
            let is_warning = match options.output_kinds().get(first + n) {
                Some(kind) => *kind == Some(OutputKind::Warning),
                None => is_warning_output(cell_output),
            };
            if is_warning {
                continue;
            }
        }
        // knitr `fig.show = "hide"`: the plot stays a piece (it still splits the source) but
        // writes nothing.
        if options.fig_show_hide() && is_plot_output(cell_output) {
            continue;
        }
        // knitr drops every character piece under `results = "hide"`, a widget included.
        if options.results_hide() && widget_output(cell_output) {
            continue;
        }
        // Jupyter `output: asis`: printed stdout is document markdown, written as is
        // (`jupyter.ts`: `asis && stream.name === "stdout"`). Other outputs keep their own
        // markup and only lose the output div.
        if let CellOutput::Stream { name, text } = cell_output
            && options.output_asis()
            && name == "stdout"
        {
            output.push_str(&format!("\n{}\n", text.trim_end()));
            continue;
        }
        if is_printed_text(cell_output) {
            if options.results_hide() {
                continue;
            }
            if options.results_asis() && !markdown_passthrough(cell_output) {
                let text = match cell_output {
                    CellOutput::Stream { text, .. } => text.clone(),
                    CellOutput::ExecuteResult { data, .. }
                    | CellOutput::DisplayData { data, .. } => data
                        .get("text/plain")
                        .map(extract_text_content)
                        .unwrap_or_default(),
                    CellOutput::Error { .. } => String::new(),
                };
                let text = output_text(&text, options.keeps_engine_text());
                output.push_str(&format!("\n{}\n", text.trim_end()));
                continue;
            }
        }
        match cell_output {
            CellOutput::Stream { name, text } => {
                output.push_str(&fenced_output_div(
                    &format!(".cell-output .cell-output-{}", name),
                    &options.comment_out(
                        options.trim_output(&output_text(text, options.keeps_engine_text())),
                    ),
                    options.fence_attrs(if name != "stderr" {
                        FenceKind::Output
                    } else {
                        match options.output_kind(first + n) {
                            Some(OutputKind::Message) => FenceKind::Message,
                            Some(OutputKind::Warning) => FenceKind::Warning,
                            None => FenceKind::Stderr,
                        }
                    }),
                    options,
                    Some(cell_output),
                ));
            }
            CellOutput::ExecuteResult { data, .. } | CellOutput::DisplayData { data, .. } => {
                // Rich output — pick the best format (Q1 priority).
                let image_entry = IMAGE_MIMES
                    .iter()
                    .find_map(|m| data.get(*m).map(|v| (*m, v)));
                if let Some(asis) = knit_asis_text(data, cell_output) {
                    // Opt-in (epic decision 22): the text is written as is, in a display
                    // div (bare under `results = "asis"`, like Q1\'s `wrap_asis_output`).
                    let s = asis.trim_matches('\n');
                    if options.results_asis() || options.output_asis() {
                        output.push_str(&format!("\n{s}\n"));
                    } else {
                        // An htmlwidget is wrapped by Q1\'s `add_html_caption`: the figure
                        // label is the div\'s id and the caption follows the content.
                        let (id, caption) = if widget_output(cell_output) {
                            (options.figure_id(), options.figure_cap(0))
                        } else {
                            (String::new(), String::new())
                        };
                        let caption = if caption.is_empty() {
                            String::new()
                        } else {
                            format!("{caption}\n")
                        };
                        let head = output_div_head(
                            Some(id.trim().trim_start_matches('#')).filter(|i| !i.is_empty()),
                            ".cell-output-display",
                            cell_output,
                        );
                        output.push_str(&format!("\n::: {{{head}}}\n\n{s}\n{caption}\n:::\n"));
                    }
                } else if let Some(html) = data.get("text/html") {
                    let s = extract_text_content(html);
                    let ticks = ticks_for_code(&s);
                    output.push_str(&output_div(
                        ".cell-output .cell-output-display",
                        &format!("{ticks}{{=html}}\n{s}\n{ticks}"),
                        options,
                        cell_output,
                    ));
                } else if let Some(markdown) = markdown_output(data) {
                    // Written as markdown itself (Q1's `mdMarkdownOutput`), with the
                    // extra class `isMarkdown` adds.
                    output.push_str(&output_div(
                        ".cell-output .cell-output-display .cell-output-markdown",
                        &markdown,
                        options,
                        cell_output,
                    ));
                } else if let Some((path, width)) = included_graphic(match cell_output {
                    CellOutput::DisplayData { metadata, .. } => metadata,
                    _ => &serde_json::Value::Null,
                }) {
                    // knitr `include_graphics`: the user\'s own file, referenced where it
                    // is, with the width knitr computed.
                    let alt = options.figure_cap(*figures);
                    let attrs =
                        options.image_attrs_with(*figures, width.map(|w| w.to_string()), None);
                    let md = options.link_image(*figures, format!("![{alt}]({path}){attrs}"));
                    push_figure(&mut output, options, *figures, &md, cell_output);
                    *figures += 1;
                } else if let Some(rel_path) = image_entry.and_then(|(mime, value)| {
                    let preferred = match cell_output {
                        CellOutput::DisplayData { metadata, .. }
                        | CellOutput::ExecuteResult { metadata, .. } => {
                            // `metadata` is nbformat JSON (`serde_json::Value`), not a `ConfigValue`.
                            // lint:allow(metadata-as-str) — serde_json `as_str`
                            metadata.get("quarto_file_path").and_then(|p| p.as_str())
                        }
                        _ => None,
                    };
                    fig.write_image_at(mime, value, preferred)
                }) {
                    // Figure divs carry ONLY `cell-output-display` — no
                    // generic `cell-output` — matching q2-knitr's
                    // vendored figure hook (hooks.R:627) so the
                    // cross-engine parity contract holds. (Q1's jupyter
                    // adds `.cell-output` here and thus disagrees with
                    // Q1's own knitr; q2 resolves the asymmetry toward
                    // knitr. The only generic `.cell-output` consumer,
                    // the print stylesheet, also matches `img`
                    // directly.)
                    let alt = options.figure_cap(*figures);
                    // A stored output may carry the image\'s size in its metadata (nbformat).
                    let (width, height) = match (cell_output, image_entry) {
                        (
                            CellOutput::DisplayData { metadata, .. }
                            | CellOutput::ExecuteResult { metadata, .. },
                            Some((mime, _)),
                        ) => metadata_size(metadata, mime),
                        _ => (None, None),
                    };
                    let attrs = options.plot_image_attrs(*figures, width, height);
                    let md = options.link_image(*figures, format!("![{alt}]({rel_path}){attrs}"));
                    push_figure(&mut output, options, *figures, &md, cell_output);
                    *figures += 1;
                } else if let Some(text) = data.get("text/plain") {
                    let s = extract_text_content(text);
                    let s = output_text(&s, options.keeps_engine_text());
                    output.push_str(&fenced_output_div(
                        ".cell-output .cell-output-display",
                        &options.comment_out(options.trim_output(&s)),
                        options.fence_attrs(FenceKind::Output),
                        options,
                        Some(cell_output),
                    ));
                } else if image_entry.is_some() {
                    // An image payload we failed to write (bad base64,
                    // I/O error) and no text fallback: keep the
                    // pre-bd-5t6wvu7m placeholder rather than dropping
                    // the output silently. The brackets are escaped: a bare
                    // `[Image output]` is a span without attributes, which loses
                    // its brackets and raises Q-2-49.
                    output.push_str(&output_div(
                        ".cell-output .cell-output-display",
                        "\\[Image output\\]",
                        options,
                        cell_output,
                    ));
                }
            }
            CellOutput::Error {
                ename,
                evalue,
                traceback,
            } => {
                output.push_str(&fenced_output_div(
                    ".cell-output .cell-output-error",
                    &options.comment_out(format_error_text(ename, evalue, traceback).trim_end()),
                    options.fence_attrs(FenceKind::Error),
                    options,
                    Some(cell_output),
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
                &options.comment_out(format_error_text(ename, evalue, traceback).trim_end()),
                options.fence_attrs(FenceKind::Error),
                options,
                None,
            ));
        }
    }

    output
}

/// `"<ename>: <evalue>"` plus the traceback lines, every part through `output_text`.
///
/// Errors are always stripped (`keeps_engine_text` is false): kernel tracebacks arrive
/// colorized, and this path never kept escapes, knitr cells included.
fn format_error_text(ename: &str, evalue: &str, traceback: &[String]) -> String {
    // An empty `ename` (Ark, knitr) has no `name:` header; the message stands alone, and an
    // empty message leaves the traceback.
    let ename = output_text(ename, false);
    let evalue = output_text(evalue, false);
    let mut error_text = match (ename.is_empty(), evalue.is_empty()) {
        (true, true) => String::new(),
        (true, false) => format!("{evalue}\n"),
        (false, _) => format!("{ename}: {evalue}\n"),
    };
    for line in traceback {
        error_text.push_str(&output_text(line, false));
        error_text.push('\n');
    }
    error_text
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

    fn knitr_markdown(meta: serde_json::Value) -> CellResult {
        ok_result(vec![CellOutput::DisplayData {
            data: [("text/markdown".to_string(), serde_json::json!("| a |\n"))]
                .into_iter()
                .collect(),
            metadata: meta,
        }])
    }

    #[test]
    fn passthrough_markdown_is_a_display_div_not_cell_output_markdown() {
        let result = knitr_markdown(serde_json::json!({"quarto_markdown_passthrough": true}));
        let md = format_outputs(&result, &mut test_fig(), true);
        assert_eq!(
            md, "\n::: {.cell-output-display}\n\n| a |\n\n:::\n",
            "got:\n{md}"
        );
    }

    #[test]
    fn only_the_flat_true_marker_opts_in() {
        for meta in [
            serde_json::json!({}),
            serde_json::json!({"quarto_markdown_passthrough": false}),
            serde_json::json!({"quarto_knitr": {"asis": true}}),
        ] {
            let md = format_outputs(&knitr_markdown(meta.clone()), &mut test_fig(), true);
            assert!(md.contains(".cell-output-markdown"), "{meta}: got:\n{md}");
        }
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
        assert!(!md.contains("Image output"), "got:\n{md}");
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
            "\n::: {.cell-output .cell-output-display}\n\n\\[Image output\\]\n\n:::\n"
        );
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

    // === OPR-7f: engine attributes (`quarto_attrs`) ===

    fn png_data() -> std::collections::HashMap<String, serde_json::Value> {
        let mut data = std::collections::HashMap::new();
        data.insert(
            "image/png".to_string(),
            serde_json::json!(PNG_B64_MULTILINE),
        );
        data
    }

    fn engine_attrs(v: serde_json::Value) -> super::super::engine_attrs::EngineAttrs {
        super::super::engine_attrs::EngineAttrs::parse(&v).0
    }

    #[test]
    fn engine_attrs_on_the_cell_wrapper_follow_the_grammar_order() {
        let options = CellOptions::default().with_engine_attrs(engine_attrs(serde_json::json!({
            "id": "cell-0",
            "classes": ["extra"],
            "attributes": {"execution_count": "1"},
        })));
        let result = ok_result(vec![CellOutput::Stream {
            name: "stdout".into(),
            text: "x\n".into(),
        }]);
        let md = render_cell_with(
            "python",
            "print(1)\n",
            &result,
            &options,
            &mut test_fig(),
            CellVisibility::VISIBLE,
        );
        assert!(
            md.starts_with("::: {#cell-0 .cell .extra execution_count='1'}\n"),
            "{md}"
        );
    }

    #[test]
    fn a_presenter_wrapper_id_wins_over_the_engine_id() {
        let mut options = CellOptions::default();
        options = options.with_engine_attrs(engine_attrs(serde_json::json!({"id": "cell-0"})));
        let md = wrap_cell(&options, "body");
        assert!(md.starts_with("::: {#cell-0 .cell}"), "{md}");
    }

    #[test]
    fn the_cell_wrapper_that_is_not_written_drops_the_cell_attrs() {
        // `results = "asis"` with nothing else to carry writes no `.cell` div.
        let options = CellOptions::from_knitr(&serde_json::json!({"results": "asis"}))
            .with_engine_attrs(engine_attrs(serde_json::json!({"id": "cell-0"})));
        assert!(!options.needs_cell_div());
        let md = wrap_cell(&options, "body");
        assert!(!md.contains("cell-0"), "{md}");
    }

    #[test]
    fn output_attrs_on_a_figure_div_put_the_engine_class_first() {
        let result = ok_result(vec![CellOutput::DisplayData {
            data: png_data(),
            metadata: serde_json::json!({
                "quarto_attrs": {"classes": ["cell-output"]},
            }),
        }]);
        let md = format_outputs(&result, &mut test_fig(), true);
        assert!(
            md.starts_with("\n::: {.cell-output .cell-output-display}\n\n!["),
            "{md}"
        );
    }

    #[test]
    fn output_attrs_on_an_execute_result_carry_the_count() {
        let mut data = text_plain("2");
        data.insert("text/html".into(), serde_json::json!("<b>2</b>"));
        let result = ok_result(vec![CellOutput::ExecuteResult {
            data: text_plain("2"),
            metadata: serde_json::json!({
                "quarto_attrs": {"attributes": {"execution_count": "3"}},
            }),
            execution_count: 3,
        }]);
        let md = format_outputs(&result, &mut test_fig(), true);
        assert!(
            md.starts_with("\n::: {.cell-output .cell-output-display execution_count='3'}\n"),
            "{md}"
        );
    }

    #[test]
    fn output_attrs_reach_the_html_and_markdown_divs() {
        for (mime, value, expected) in [
            (
                "text/html",
                "<b>x</b>",
                "::: {#h .cell-output .cell-output-display k='v'}",
            ),
            (
                "text/markdown",
                "**x**",
                "::: {#h .cell-output .cell-output-display .cell-output-markdown k='v'}",
            ),
        ] {
            let result = display(&[(mime, serde_json::json!(value))]);
            let CellOutput::DisplayData { data, .. } = &result.outputs[0] else {
                unreachable!()
            };
            let result = ok_result(vec![CellOutput::DisplayData {
                data: data.clone(),
                metadata: serde_json::json!({
                    "quarto_attrs": {"id": "h", "attributes": {"k": "v"}},
                }),
            }]);
            let md = format_outputs(&result, &mut test_fig(), true);
            assert!(md.contains(expected), "{mime}: {md}");
        }
    }

    #[test]
    fn a_bare_fence_drops_the_output_attrs_by_design() {
        let options = CellOptions::from_knitr(&serde_json::json!({"collapse": true}));
        let result = ok_result(vec![CellOutput::ExecuteResult {
            data: text_plain("2"),
            metadata: serde_json::json!({"quarto_attrs": {"id": "dropped"}}),
            execution_count: 1,
        }]);
        let md = format_outputs_with(&result, &mut test_fig(), true, &options, &mut 0, 0);
        assert!(!md.contains("dropped") && !md.contains(":::"), "{md}");
    }

    #[test]
    fn malformed_output_attrs_are_skipped_and_the_rest_renders() {
        let result = ok_result(vec![CellOutput::ExecuteResult {
            data: text_plain("2"),
            metadata: serde_json::json!({"quarto_attrs": {
                "id": "caf\u{e9}",
                "classes": ["1bad", "ok"],
                "attributes": {"bad": "a\nb", "good": "v"},
            }}),
            execution_count: 1,
        }]);
        let md = format_outputs(&result, &mut test_fig(), true);
        assert!(
            md.starts_with("\n::: {.ok .cell-output .cell-output-display good='v'}\n"),
            "{md}"
        );
    }

    #[test]
    fn output_without_attrs_is_byte_identical() {
        let result = ok_result(vec![CellOutput::ExecuteResult {
            data: text_plain("2"),
            metadata: serde_json::json!({}),
            execution_count: 1,
        }]);
        assert_eq!(
            format_outputs(&result, &mut test_fig(), true),
            "\n::: {.cell-output .cell-output-display}\n\n```\n2\n```\n\n:::\n"
        );
    }
}

#[cfg(test)]
mod interleave_tests {
    //! KNR-2 Phase 2B: source and outputs alternate by anchors (epic decision 21).
    use super::*;
    use std::path::Path;

    use crate::presenter::figures::FsFigureSink;

    fn stream(text: &str) -> CellOutput {
        CellOutput::Stream {
            name: "stdout".to_string(),
            text: text.to_string(),
        }
    }

    fn png() -> CellOutput {
        let mut data = crate::presenter::output::MimeBundle::new();
        data.insert("image/png".to_string(), serde_json::json!("AAAA"));
        CellOutput::DisplayData {
            data,
            metadata: serde_json::json!({}),
        }
    }

    fn result(outputs: Vec<CellOutput>) -> CellResult {
        CellResult {
            status: ExecuteStatus::Ok,
            outputs,
            execution_count: None,
        }
    }

    fn fig() -> FsFigureSink {
        FsFigureSink::new(Path::new("/nonexistent/doc.qmd"))
    }

    fn interleaved(code: &str, outputs: Vec<CellOutput>, anchors: Option<&[usize]>) -> String {
        render_cell_interleaved(
            "r",
            code,
            &result(outputs),
            anchors,
            &[],
            &CellOptions::default(),
            &mut fig(),
            CellVisibility::VISIBLE,
        )
    }

    fn plain(code: &str, outputs: Vec<CellOutput>) -> String {
        render_cell(
            "r",
            code,
            &result(outputs),
            &mut fig(),
            CellVisibility::VISIBLE,
        )
    }

    fn pos(md: &str, needle: &str) -> usize {
        md.find(needle)
            .unwrap_or_else(|| panic!("{needle:?} missing from:\n{md}"))
    }

    #[test]
    fn alternates_source_and_output() {
        let md = interleaved("a\nb\n", vec![stream("1"), stream("2")], Some(&[1, 2]));
        let (a, o1, b, o2) = (
            pos(&md, "\na\n"),
            pos(&md, "\n1\n"),
            pos(&md, "\nb\n"),
            pos(&md, "\n2\n"),
        );
        assert!(a < o1 && o1 < b && b < o2, "order a,1,b,2; got:\n{md}");
        assert_eq!(md.matches(".cell-code").count(), 2, "two fences:\n{md}");
        assert_eq!(md.matches("::: {.cell}").count(), 1, "one wrapper:\n{md}");
    }

    #[test]
    fn equal_anchors_form_one_group() {
        let md = interleaved("a\nb\n", vec![stream("1"), stream("2")], Some(&[1, 1]));
        let (a, o1, o2, b) = (
            pos(&md, "\na\n"),
            pos(&md, "\n1\n"),
            pos(&md, "\n2\n"),
            pos(&md, "\nb\n"),
        );
        assert!(a < o1 && o1 < o2 && o2 < b, "order a,1,2,b; got:\n{md}");
    }

    #[test]
    fn source_after_last_output_gets_its_own_fence() {
        let md = interleaved("a\nb\nc\n", vec![stream("1")], Some(&[1]));
        assert_eq!(md.matches(".cell-code").count(), 2, "fences a | b,c:\n{md}");
        assert!(
            pos(&md, "\n1\n") < pos(&md, "\nb\nc\n"),
            "tail after output:\n{md}"
        );
    }

    #[test]
    fn no_anchors_is_render_cell() {
        let out = vec![stream("1"), stream("2")];
        assert_eq!(
            interleaved("a\nb\n", out.clone(), None),
            plain("a\nb\n", out)
        );
    }

    #[test]
    fn all_outputs_after_all_source_is_render_cell() {
        let out = vec![stream("1"), stream("2")];
        assert_eq!(
            interleaved("a\nb\n", out.clone(), Some(&[2, 2])),
            plain("a\nb\n", out)
        );
    }

    #[test]
    fn unusable_anchors_fall_back_to_render_cell() {
        let out = vec![stream("1"), stream("2")];
        for bad in [&[1usize][..], &[2, 1][..], &[1, 9][..]] {
            assert_eq!(
                interleaved("a\nb\n", out.clone(), Some(bad)),
                plain("a\nb\n", out.clone()),
                "anchors {bad:?}"
            );
        }
    }

    #[test]
    fn hidden_echo_or_output_does_not_interleave() {
        let out = vec![stream("1"), stream("2")];
        for vis in [
            CellVisibility {
                echo: false,
                ..CellVisibility::VISIBLE
            },
            CellVisibility {
                output: false,
                ..CellVisibility::VISIBLE
            },
        ] {
            let r = result(out.clone());
            assert_eq!(
                render_cell_interleaved(
                    "r",
                    "a\nb\n",
                    &r,
                    Some(&[1, 2]),
                    &[],
                    &CellOptions::default(),
                    &mut fig(),
                    vis,
                ),
                render_cell("r", "a\nb\n", &r, &mut fig(), vis)
            );
        }
    }

    // --- present-time holds (epic decision 21): the stored anchors stay in capture order.

    #[test]
    fn fig_show_hold_moves_plots_to_the_end() {
        let (outs, anchors) = apply_holds(
            vec![png(), stream("1"), png()],
            &[1, 1, 2],
            2,
            HoldMode {
                plots: true,
                text: false,
            },
        );
        assert!(matches!(outs[0], CellOutput::Stream { .. }));
        assert!(matches!(outs[1], CellOutput::DisplayData { .. }));
        assert!(matches!(outs[2], CellOutput::DisplayData { .. }));
        assert_eq!(anchors, vec![1, 2, 2]);
    }

    #[test]
    fn results_hold_merges_printed_text_at_the_end() {
        let (outs, anchors) = apply_holds(
            vec![stream("1\n"), png(), stream("2\n")],
            &[1, 1, 2],
            2,
            HoldMode {
                plots: false,
                text: true,
            },
        );
        assert_eq!(outs.len(), 2);
        assert!(matches!(outs[0], CellOutput::DisplayData { .. }));
        match &outs[1] {
            CellOutput::Stream { text, .. } => assert_eq!(text, "1\n2\n"),
            other => panic!("expected merged stream, got {other:?}"),
        }
        assert_eq!(anchors, vec![1, 2]);
    }

    #[test]
    fn before_code_moves_each_plot_to_the_start_of_its_source_run() {
        // [src 0-1][p1][src 1-2][p2] -> [p1][src 0-1][p2][src 1-2]
        let outs = vec![png(), png()];
        assert_eq!(before_code_anchors(&outs, &[1, 2]), vec![0, 1]);
    }

    #[test]
    fn before_code_leaves_a_plot_before_any_code_and_other_outputs() {
        let outs = vec![png(), stream("1"), png()];
        // The first plot is at 0; the text stays; the last plot goes to the text's anchor.
        assert_eq!(before_code_anchors(&outs, &[0, 2, 3]), vec![0, 2, 2]);
    }

    #[test]
    fn no_hold_changes_nothing() {
        let (outs, anchors) =
            apply_holds(vec![stream("1"), png()], &[1, 2], 2, HoldMode::default());
        assert_eq!((outs.len(), anchors), (2, vec![1, 2]));
    }
}

#[cfg(test)]
mod option_tests {
    use super::*;
    use std::path::Path;

    use crate::presenter::figures::FsFigureSink;

    fn result(outputs: Vec<CellOutput>) -> CellResult {
        CellResult {
            status: ExecuteStatus::Ok,
            outputs,
            execution_count: Some(1),
        }
    }

    fn stream(text: &str) -> CellOutput {
        CellOutput::Stream {
            name: "stdout".into(),
            text: text.into(),
        }
    }

    fn err(text: &str) -> CellOutput {
        CellOutput::Stream {
            name: "stderr".into(),
            text: text.into(),
        }
    }

    // === KNR-2 Phase 2C: the cell-option layer ===

    fn opts(json: serde_json::Value) -> CellOptions {
        CellOptions::from_knitr(&json)
    }

    fn render_opts(o: &CellOptions, outputs: Vec<CellOutput>) -> String {
        let mut fig = FsFigureSink::new(Path::new("/nonexistent/doc.qmd"));
        render_cell_with(
            "r",
            "x\n",
            &result(outputs),
            o,
            &mut fig,
            CellVisibility::VISIBLE,
        )
    }

    #[test]
    fn a_plot_with_a_producer_path_is_written_there() {
        // Epic decision 25: a knitr plot keeps knitr's own file name; the presenter writes
        // the bytes at `quarto_file_path` instead of a generated `cell-<n>-output-<m>` name.
        let dir = tempfile::tempdir().unwrap();
        let mut fig = FsFigureSink::new(&dir.path().join("doc.qmd"));
        fig.begin_cell();
        let mut data = crate::presenter::output::MimeBundle::new();
        data.insert("image/png".to_string(), serde_json::json!(
            "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg=="
        ));
        let md = render_cell_with(
            "r",
            "x\n",
            &result(vec![CellOutput::DisplayData {
                data,
                metadata: serde_json::json!({"quarto_file_path":
                    "doc_files/figure-html/unnamed-chunk-1-1.png"}),
            }]),
            &CellOptions::default(),
            &mut fig,
            CellVisibility::VISIBLE,
        );
        assert!(
            md.contains("![](doc_files/figure-html/unnamed-chunk-1-1.png)"),
            "{md}"
        );
        assert!(
            dir.path()
                .join("doc_files/figure-html/unnamed-chunk-1-1.png")
                .is_file()
        );
    }

    #[test]
    fn include_graphics_keeps_the_path_and_writes_the_width() {
        let mut data = crate::presenter::output::MimeBundle::new();
        data.insert("image/png".to_string(), serde_json::json!("AAAA"));
        let md = render_opts(
            &CellOptions::default(),
            vec![CellOutput::DisplayData {
                data,
                metadata: serde_json::json!({"quarto_file_path": "made.png", "quarto_knitr": {
                    "include_graphics": true, "width": 200.0}}),
            }],
        );
        assert!(
            md.contains("::: {.cell-output-display}\n\n![](made.png){width=200}\n\n:::"),
            "{md}"
        );
    }

    #[test]
    fn include_graphics_from_the_mappers_output_keeps_the_path() {
        // Built by the mapper, not by hand: a hand-built key kept the old reader passing
        // after `28474d2ec` moved the writer to `quarto_file_path`.
        let chunk: crate::engine::knitr::types::KnitrCapturedChunk =
            serde_json::from_value(serde_json::json!({"ordinal": 1, "label": "t", "pieces": [
                {"class": "image_paths", "paths": ["made.png"], "widths": [200.0]}]}))
            .unwrap();
        let (_, result) =
            crate::engine::knitr::pieces_map::pieces_to_cell(&chunk, &|_| Some(vec![1, 2, 3]))
                .unwrap();
        let md = render_opts(&CellOptions::default(), result.outputs);
        assert!(
            md.contains("::: {.cell-output-display}\n\n![](made.png){width=200}\n\n:::"),
            "{md}"
        );
    }

    #[test]
    fn knitr_output_keeps_trailing_spaces_and_jupyter_does_not() {
        let outs = vec![stream("a   \n")];
        let knitr = render_opts(&opts(serde_json::json!({})), outs.clone());
        assert!(knitr.contains("```\na   \n```"), "{knitr}");
        let jupyter = render_opts(&CellOptions::default(), outs);
        assert!(jupyter.contains("```\na\n```"), "{jupyter}");
    }

    #[test]
    fn a_table_label_becomes_the_cell_id() {
        let md = render_opts(&opts(serde_json::json!({"label": "tbl-demo"})), vec![]);
        assert!(md.starts_with("::: {#tbl-demo .cell}\n"), "{md}");
        // only tbl- labels are let through (hooks.R `is_table_label`)
        let md = render_opts(&opts(serde_json::json!({"label": "setup"})), vec![]);
        assert!(md.starts_with("::: {.cell}\n"), "{md}");
    }

    #[test]
    fn options_outside_the_known_list_become_cell_attributes() {
        let known: Vec<String> = ["echo", "label"].iter().map(|s| s.to_string()).collect();
        let o = CellOptions::from_knitr_known(
            &serde_json::json!({
                "echo": true, "label": "c1", "foo.unknown": "1", "tbl-cap": "A demo",
                "list": [1, 2], "it's": "a\'b", ".hidden": "x", "nothing": null
            }),
            Some(&known),
        );
        let md = render_opts(&o, vec![]);
        assert!(
            md.starts_with(
                "::: {.cell foo.unknown='1' it's='a\\'b' list='[1,2]' tbl-cap='A demo'}\n"
            ),
            "{md}"
        );
        // (attributes come in key order: a record\'s JSON object does not keep R\'s order)
        // an absent list filters nothing
        let none = CellOptions::from_knitr_known(&serde_json::json!({"foo.unknown": "1"}), None);
        assert!(render_opts(&none, vec![]).starts_with("::: {.cell}\n"));
    }

    #[test]
    fn a_widget_raw_fence_is_three_ticks() {
        let mut data = crate::presenter::output::MimeBundle::new();
        data.insert("text/html".to_string(), serde_json::json!("<div>w</div>"));
        let out = CellOutput::DisplayData {
            data,
            metadata: serde_json::json!({
                "quarto_markdown_passthrough": true, "quarto_knitr": {"widget": true}}),
        };
        let md = render_opts(&opts(serde_json::json!({})), vec![out]);
        assert!(md.contains("\n```{=html}\n<div>w</div>\n```\n"), "{md}");
    }

    #[test]
    fn a_widget_takes_its_figure_label_and_caption() {
        let mut data = crate::presenter::output::MimeBundle::new();
        data.insert("text/html".to_string(), serde_json::json!("<div>w</div>"));
        let out = CellOutput::DisplayData {
            data,
            metadata: serde_json::json!({
                "quarto_markdown_passthrough": true, "quarto_knitr": {"widget": true}}),
        };
        let md = render_opts(
            &opts(serde_json::json!({"label": "fig-w", "fig.cap": "A caption"})),
            vec![out],
        );
        assert!(
            md.contains("\n::: {#fig-w .cell-output-display}\n\n```{=html}\n<div>w</div>\n```\nA caption\n\n:::\n"),
            "{md}"
        );
    }

    #[test]
    fn results_hide_hides_a_widget() {
        let mut data = crate::presenter::output::MimeBundle::new();
        data.insert("text/html".to_string(), serde_json::json!("<div>w</div>"));
        let out = CellOutput::DisplayData {
            data,
            metadata: serde_json::json!({
                "quarto_markdown_passthrough": true, "quarto_knitr": {"widget": true}}),
        };
        let md = render_opts(&opts(serde_json::json!({"results": "hide"})), vec![out]);
        assert!(!md.contains("<div>w</div>"), "{md}");
    }

    #[test]
    fn hidden_plots_still_split_the_source() {
        let o = opts(serde_json::json!({"fig.show": "hide"}));
        let md = render_cell_interleaved(
            "r",
            "plot(1)\nplot(2)\n",
            &result(vec![plot_output(), plot_output()]),
            Some(&[1, 2]),
            &[],
            &o,
            &mut FsFigureSink::new(Path::new("/nonexistent/doc.qmd")),
            CellVisibility::VISIBLE,
        );
        assert_eq!(md.matches("```{.r .cell-code}").count(), 2, "{md}");
        assert!(!md.contains("!["), "{md}");
    }

    fn plot_output() -> CellOutput {
        let mut data = crate::presenter::output::MimeBundle::new();
        data.insert("image/png".to_string(), serde_json::json!("AAAA"));
        CellOutput::DisplayData {
            data,
            metadata: serde_json::json!({}),
        }
    }

    #[test]
    fn fig_show_hide_drops_plots_and_keeps_text() {
        let md = render_opts(
            &opts(serde_json::json!({"fig.show": "hide"})),
            vec![stream("1\n"), plot_output()],
        );
        assert!(!md.contains("!["), "{md}");
        assert!(!md.contains("Image output"), "{md}");
        assert!(md.contains('1'), "{md}");
    }

    #[test]
    fn default_options_render_byte_identically() {
        let outs = vec![stream("1\n"), err("w\n")];
        let mut fig = FsFigureSink::new(Path::new("/nonexistent/doc.qmd"));
        let plain = render_cell(
            "r",
            "x\n",
            &result(outs.clone()),
            &mut fig,
            CellVisibility::VISIBLE,
        );
        assert_eq!(render_opts(&CellOptions::default(), outs), plain);
    }

    #[test]
    fn source_options_reach_the_source_fence() {
        let md = render_opts(
            &opts(serde_json::json!({
                "class.source": "k", "attr.source": "a=1", "code-fold": true, "lst-label": "lst-a"
            })),
            vec![],
        );
        assert!(
            md.contains("```{#lst-a .r .k .cell-code a=1 code-fold=\"true\"}\nx\n```"),
            "{md}"
        );
    }

    #[test]
    fn cell_options_reach_the_cell_div() {
        let md = render_opts(
            &opts(serde_json::json!({"column": "margin", "keep-hidden": true})),
            vec![],
        );
        assert!(
            md.starts_with("::: {.cell .column-margin keep-hidden='true'}\n"),
            "{md}"
        );
    }

    #[test]
    fn output_fences_take_their_attributes() {
        let md = render_opts(
            &opts(serde_json::json!({
                "class.output": "o", "class.error": "e", "class.warning": "w"
            })),
            vec![
                stream("1\n"),
                err("w\n"),
                CellOutput::Error {
                    ename: "E".into(),
                    evalue: "v".into(),
                    traceback: vec![],
                },
            ],
        );
        assert!(md.contains("```{.o}\n1\n```"), "{md}");
        assert!(md.contains("```{.w}\nw\n```"), "{md}");
        assert!(md.contains("```{.e}\nE: v\n```"), "{md}");
    }

    #[test]
    fn interleaved_cells_use_the_options_for_every_fence() {
        let mut fig = FsFigureSink::new(Path::new("/nonexistent/doc.qmd"));
        let o = opts(serde_json::json!({"class.source": "k", "class.output": "o"}));
        let md = render_cell_interleaved(
            "r",
            "a\nb\n",
            &result(vec![stream("1\n")]),
            Some(&[1]),
            &[],
            &o,
            &mut fig,
            CellVisibility::VISIBLE,
        );
        assert_eq!(md.matches(".r .k .cell-code").count(), 2, "{md}");
        assert!(md.contains("```{.o}\n1\n```"), "{md}");
    }
    // === KNR-2 Phase 2C slice 3: figure captions and alt text ===

    const PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==";

    fn plot() -> CellOutput {
        let mut data = std::collections::HashMap::new();
        data.insert("image/png".to_string(), serde_json::json!(PNG));
        CellOutput::DisplayData {
            data,
            metadata: Default::default(),
        }
    }

    /// The `![alt](path){attrs}` lines of a rendered cell, with the path replaced by `P`.
    fn images(o: &CellOptions, outputs: Vec<CellOutput>) -> Vec<String> {
        let dir = tempfile::tempdir().unwrap();
        let mut fig = FsFigureSink::new(&dir.path().join("doc.qmd"));
        fig.begin_cell();
        let md = render_cell_with(
            "r",
            "x\n",
            &result(outputs),
            o,
            &mut fig,
            CellVisibility::VISIBLE,
        );
        md.lines()
            .filter(|l| l.starts_with("!["))
            .map(|l| {
                let (head, tail) = l.split_once("](").unwrap();
                let (_, rest) = tail.split_once(')').unwrap();
                format!("{head}](P){rest}")
            })
            .collect()
    }

    #[test]
    fn fig_cap_is_the_image_alt_text() {
        let o = opts(serde_json::json!({"fig.cap": "A *marked up* caption"}));
        assert_eq!(images(&o, vec![plot()]), ["![A *marked up* caption](P)"]);
    }

    // === OPR-7j: group caption and panel ids without a knitr figure count ===

    #[test]
    fn a_labeled_subcap_group_without_a_figure_count_keeps_caption_and_panel_ids() {
        // Julia/Jupyter shape: label, fig-cap, fig-subcap and two plots, no `figure_count`.
        let o = opts(
            serde_json::json!({"label": "fig-two", "fig.cap": "A group", "fig.subcap": ["First", "Second"]}),
        );
        let md = render_figs(&o, vec![plot(), plot()]);
        assert!(md.contains("::: {#fig-two .cell}"), "{md}");
        assert!(
            md.contains("![First](") && md.contains("){#fig-two-1}"),
            "{md}"
        );
        assert!(
            md.contains("![Second](") && md.contains("){#fig-two-2}"),
            "{md}"
        );
        assert!(md.trim_end().ends_with("\nA group\n:::"), "{md}");
    }

    #[test]
    fn a_one_plot_subcap_group_is_not_a_group() {
        // Negative control: one plot, one subcap. knitr and plain Julia render one figure.
        let o = opts(
            serde_json::json!({"label": "fig-two", "fig.cap": "A group", "fig.subcap": ["Only"]}),
        );
        let md = render_figs(&o, vec![plot()]);
        assert!(!md.contains("{#fig-two-1}"), "{md}");
        assert!(!md.contains("::: {#fig-two .cell}"), "{md}");
    }

    #[test]
    fn a_labeled_figure_without_subcap_is_unchanged() {
        // Negative control: no `fig-subcap`, so no group even with two plots.
        let o = opts(serde_json::json!({"label": "fig-two", "fig.cap": "Cap"}));
        let md = render_figs(&o, vec![plot(), plot()]);
        assert!(!md.contains("{#fig-two-1}"), "{md}");
        assert!(!md.contains("::: {#fig-two .cell}"), "{md}");
    }

    #[test]
    fn a_labeled_single_figure_without_a_figure_count_is_unchanged() {
        // The case a generic figure count would have broken (D14): one labeled plot.
        let o = opts(serde_json::json!({"label": "fig-one", "fig.cap": "One"}));
        let md = render_figs(&o, vec![plot()]);
        assert!(!md.contains("#fig-one"), "{md}");
        assert!(!md.contains("::: {#fig-one"), "{md}");
    }

    #[test]
    fn subcaptions_recycle_over_more_plots_and_keep_panel_ids() {
        let o =
            opts(serde_json::json!({"label": "fig-g", "fig.cap": "G", "fig.subcap": ["C1", "C2"]}));
        let md = render_figs(&o, vec![plot(), plot(), plot()]);
        for n in 1..=3 {
            assert!(md.contains(&format!("{{#fig-g-{n}}}")), "{md}");
        }
        assert!(md.contains("![C1](") && md.contains("![C2]("), "{md}");
    }

    #[test]
    fn a_plot_and_an_included_graphic_is_not_a_group() {
        // Included graphics carry no figure id and do not count toward the two plots a group
        // needs.
        let graphic = {
            let mut data = std::collections::HashMap::new();
            data.insert("image/png".to_string(), serde_json::json!(PNG));
            CellOutput::DisplayData {
                data,
                metadata: serde_json::json!({
                    "quarto_knitr": {"include_graphics": true},
                    "quarto_file_path": "x.png"
                }),
            }
        };
        let o =
            opts(serde_json::json!({"label": "fig-g", "fig.cap": "G", "fig.subcap": ["A", "B"]}));
        let md = render_figs(&o, vec![plot(), graphic]);
        assert!(!md.contains("::: {#fig-g .cell}"), "{md}");
        assert!(!md.contains("{#fig-g-1}"), "{md}");
    }

    #[test]
    fn an_engine_written_panel_id_loses_to_the_generated_one() {
        // OPR-7j, item 4: the presenter cannot see an id the engine put in an output's own
        // markdown (a hand-written `{#fig-l-1}` duplicates the generated id; author error,
        // not reproducible at this level because the cell step does not render that mime).
        // What it can see is an engine `quarto_attrs` id on a plot: the presenter's id wins
        // (`build_head`), so the id appears once. Pinned so a change is a visible decision.
        let mut data = std::collections::HashMap::new();
        data.insert("image/png".to_string(), serde_json::json!(PNG));
        let user = CellOutput::DisplayData {
            data,
            metadata: serde_json::json!({"quarto_attrs": {"id": "fig-l-1"}}),
        };
        let o =
            opts(serde_json::json!({"label": "fig-l", "fig.cap": "L", "fig.subcap": ["A", "B"]}));
        let md = render_figs(&o, vec![user, plot()]);
        assert_eq!(md.matches("fig-l-1}").count(), 1, "{md}");
    }

    fn group_opts(extra: serde_json::Value) -> CellOptions {
        let mut j = serde_json::json!({"label": "fig-g", "fig.cap": "G", "fig.subcap": ["A", "B"]});
        for (k, v) in extra.as_object().unwrap() {
            j[k] = v.clone();
        }
        opts(j)
    }

    fn render_group_vis(o: &CellOptions, vis: CellVisibility) -> String {
        let dir = tempfile::tempdir().unwrap();
        let mut fig = FsFigureSink::new(&dir.path().join("doc.qmd"));
        fig.begin_cell();
        render_cell_with("r", "x\n", &result(vec![plot(), plot()]), o, &mut fig, vis)
    }

    fn assert_group(md: &str) {
        assert!(md.contains("::: {#fig-g .cell"), "{md}");
        assert!(
            md.contains("{#fig-g-1}") && md.contains("{#fig-g-2}"),
            "{md}"
        );
        assert!(md.trim_end().ends_with("\nG\n:::"), "{md}");
    }

    #[test]
    fn layout_options_keep_the_group_and_panel_ids() {
        // Design rule 7: the panel references resolve under `layout-ncol` and `layout-nrow`.
        for (k, v) in [("layout-ncol", 2), ("layout-nrow", 2)] {
            let md = render_figs(
                &group_opts(serde_json::json!({ k: v })),
                vec![plot(), plot()],
            );
            assert_group(&md);
            assert!(md.contains(&format!("{k}=\"2\"")), "{md}");
        }
    }

    #[test]
    fn fig_subcap_without_a_label_has_no_group_and_no_ids() {
        let o = opts(serde_json::json!({"fig.cap": "G", "fig.subcap": ["A", "B"]}));
        let md = render_figs(&o, vec![plot(), plot()]);
        assert!(!md.contains("{#"), "{md}");
        assert!(md.contains("![A](") && md.contains("![B]("), "{md}");
        assert!(!md.contains("\nG\n"), "{md}");
    }

    #[test]
    fn echo_false_still_renders_the_group() {
        // The plots are output; hiding the source changes nothing about the group.
        let vis = CellVisibility {
            echo: false,
            ..CellVisibility::VISIBLE
        };
        let md = render_group_vis(&group_opts(serde_json::json!({})), vis);
        assert_group(&md);
        assert!(!md.contains("cell-code"), "{md}");
    }

    #[test]
    fn results_asis_does_not_change_a_plot_group() {
        let md = render_figs(
            &group_opts(serde_json::json!({"results": "asis"})),
            vec![plot(), plot()],
        );
        assert_group(&md);
    }

    #[test]
    fn a_hidden_group_emits_nothing_and_no_stray_caption() {
        // `include: false`: nothing renders, so the references stay unresolved as before.
        let md = render_group_vis(&group_opts(serde_json::json!({})), CellVisibility::HIDDEN);
        assert!(!md.contains("fig-g"), "{md}");
        assert!(!md.contains("\nG\n"), "{md}");
    }

    #[test]
    fn output_false_gives_no_panels() {
        // `output: false` with the source shown: the panels are gone; pin whether a caption
        // is left behind.
        let vis = CellVisibility {
            output: false,
            ..CellVisibility::VISIBLE
        };
        let md = render_group_vis(&group_opts(serde_json::json!({})), vis);
        assert!(!md.contains("{#fig-g-1}"), "{md}");
        assert!(!md.contains("![A]("), "{md}");
        // The group caption stays with the cell, with no panels (as plain Julia does).
        assert!(
            md.contains("::: {#fig-g .cell}") && md.contains("\nG\n:::"),
            "{md}"
        );
    }

    #[test]
    fn fig_subcap_true_keeps_ids_with_empty_subcaptions() {
        let md = render_figs(
            &group_opts(serde_json::json!({"fig.subcap": true})),
            vec![plot(), plot()],
        );
        assert_group(&md);
        assert!(md.contains("![](") && !md.contains("![A]("), "{md}");
    }

    #[test]
    fn a_non_fig_label_keeps_the_recorded_routes_behavior() {
        // Known difference from plain Julia (`foo-1`, `foo-2`); the flip changes it, not 7j.
        let md = render_figs(
            &group_opts(serde_json::json!({"label": "foo"})),
            vec![plot(), plot()],
        );
        assert!(!md.contains("{#foo"), "{md}");
        assert!(!md.contains("\nG\n"), "{md}");
    }

    #[test]
    fn a_list_fig_cap_uses_its_first_entry_as_the_group_caption() {
        let md = render_figs(
            &group_opts(serde_json::json!({"fig.cap": ["L1", "L2"]})),
            vec![plot(), plot()],
        );
        assert!(md.contains("::: {#fig-g .cell"), "{md}");
        assert!(md.trim_end().ends_with("\nL1\n:::"), "{md}");
    }

    #[test]
    fn a_plot_and_a_widget_is_one_plot_so_not_a_group() {
        // The widget is not a plot (`is_plot_output`), so the count is 1: no group.
        let mut data = std::collections::HashMap::new();
        data.insert(
            "application/vnd.jupyter.widget-view+json".to_string(),
            serde_json::json!({"model_id": "abc"}),
        );
        data.insert("text/plain".to_string(), serde_json::json!("Widget"));
        let widget = CellOutput::DisplayData {
            data,
            metadata: Default::default(),
        };
        let md = render_figs(&group_opts(serde_json::json!({})), vec![plot(), widget]);
        assert!(!md.contains("{#fig-g-1}"), "{md}");
        assert!(!md.contains("::: {#fig-g .cell"), "{md}");
        // What renders instead: the one plot, then the widget's text fallback.
        assert!(md.contains("![A]("), "{md}");
        assert!(md.contains("Widget"), "{md}");
    }

    #[test]
    fn each_figure_takes_its_own_subcaption() {
        let o = opts(serde_json::json!({"fig.subcap": ["first", "second"]}));
        assert_eq!(
            images(&o, vec![plot(), plot()]),
            ["![first](P)", "![second](P)"]
        );
    }

    #[test]
    fn subcaptions_win_over_the_figure_caption_and_recycle() {
        let o = opts(serde_json::json!({"fig.cap": "all", "fig.subcap": ["a", "b"]}));
        assert_eq!(
            images(&o, vec![plot(), plot(), plot()]),
            ["![a](P)", "![b](P)", "![a](P)"]
        );
        let o = opts(serde_json::json!({"fig.cap": "all"}));
        assert_eq!(images(&o, vec![plot(), plot()]), ["![all](P)", "![all](P)"]);
    }

    #[test]
    fn fig_subcap_true_means_empty_subcaptions() {
        let o = opts(serde_json::json!({"fig.cap": "all", "fig.subcap": true}));
        assert_eq!(images(&o, vec![plot()]), ["![](P)"]);
    }

    #[test]
    fn fig_alt_is_an_image_attribute_with_escaped_quotes() {
        let o = opts(serde_json::json!({"fig.alt": "Alt text"}));
        assert_eq!(images(&o, vec![plot()]), ["![](P){fig-alt='Alt text'}"]);
        let o = opts(serde_json::json!({"fig.alt": "it's"}));
        assert_eq!(images(&o, vec![plot()]), ["![](P){fig-alt='it\\'s'}"]);
    }

    #[test]
    fn captions_count_figures_across_interleaved_runs() {
        let dir = tempfile::tempdir().unwrap();
        let mut fig = FsFigureSink::new(&dir.path().join("doc.qmd"));
        fig.begin_cell();
        let o = opts(serde_json::json!({"fig.subcap": ["first", "second"]}));
        let md = render_cell_interleaved(
            "r",
            "a\nb\n",
            &result(vec![plot(), plot()]),
            Some(&[1, 2]),
            &[],
            &o,
            &mut fig,
            CellVisibility::VISIBLE,
        );
        assert!(md.contains("![first]("), "{md}");
        assert!(md.contains("![second]("), "{md}");
    }

    // === KNR-3 slice 7, Phase 7b: image attributes ===

    fn one_image(json: serde_json::Value) -> String {
        images(&opts(json), vec![plot()]).remove(0)
    }

    #[test]
    fn each_image_option_alone_writes_its_attribute() {
        for (json, want) in [
            (
                serde_json::json!({"fig-align": "center"}),
                "![](P){fig-align='center'}",
            ),
            (
                serde_json::json!({"fig-env": "figure*"}),
                "![](P){fig-env='figure*'}",
            ),
            (serde_json::json!({"fig-pos": "h"}), "![](P){fig-pos='h'}"),
            (
                serde_json::json!({"fig-scap": "it's short"}),
                "![](P){fig-scap='it\\'s short'}",
            ),
            (serde_json::json!({"out-width": "50%"}), "![](P){width=50%}"),
            (serde_json::json!({"out-width": 0.5}), "![](P){width=0.5}"),
            (
                serde_json::json!({"out-height": "2in"}),
                "![](P){height=2in}",
            ),
            (
                serde_json::json!({"out-extra": "style=\"border: 1px\""}),
                "![](P){style=\"border: 1px\"}",
            ),
            // knitr\'s dotted spellings are aliases.
            (
                serde_json::json!({"fig.align": "left", "out.width": "30%"}),
                "![](P){fig-align='left' width=30%}",
            ),
        ] {
            assert_eq!(one_image(json.clone()), want, "{json}");
        }
    }

    #[test]
    fn all_image_options_together_follow_knitrs_order() {
        let got = one_image(serde_json::json!({
            "out-extra": "style=\"border: 1px\"", "out-height": "50%", "out-width": "50%",
            "fig-scap": "short", "fig-alt": "a", "fig-pos": "h", "fig-env": "figure*",
            "fig-align": "center",
        }));
        assert_eq!(
            got,
            "![](P){fig-align='center' fig-env='figure*' fig-pos='h' fig-alt='a' \
             fig-scap='short' width=50% height=50% style=\"border: 1px\"}"
        );
    }

    #[test]
    fn the_knitr_defaults_write_nothing() {
        assert_eq!(
            one_image(
                serde_json::json!({"fig.align": "default", "fig.env": "figure", "fig.pos": ""})
            ),
            "![](P)"
        );
    }

    #[test]
    fn a_list_valued_image_option_recycles_by_figure_index_for_every_figure() {
        // D3: figure i gets value[i % len]; a scalar applies to every figure.
        let o = opts(serde_json::json!({"fig-align": ["left", "center"], "out-width": "40%"}));
        assert_eq!(
            images(&o, vec![plot(), plot(), plot()]),
            [
                "![](P){fig-align='left' width=40%}",
                "![](P){fig-align='center' width=40%}",
                "![](P){fig-align='left' width=40%}",
            ]
        );
    }

    #[test]
    fn every_figure_of_a_captioned_cell_carries_the_image_attributes() {
        let o = opts(serde_json::json!({
            "fig.cap": "c", "fig.subcap": ["one", "two"], "fig-align": "right"
        }));
        assert_eq!(
            images(&o, vec![plot(), plot()]),
            [
                "![one](P){fig-align='right'}",
                "![two](P){fig-align='right'}"
            ]
        );
    }

    #[test]
    fn fig_align_also_writes_layout_align_on_the_cell() {
        let div = |json: serde_json::Value| opts(json).cell_div_attrs();
        assert_eq!(
            div(serde_json::json!({"fig-align": "center"})),
            " layout-align=\"center\""
        );
        // The first element of a list, never its compact JSON.
        assert_eq!(
            div(serde_json::json!({"fig-align": ["right", "left"]})),
            " layout-align=\"right\""
        );
        // An explicit `layout-align` loses to it (hooks.R).
        assert_eq!(
            div(serde_json::json!({"fig-align": "right", "layout-align": "left"})),
            " layout-align=\"right\""
        );
        assert_eq!(
            div(serde_json::json!({"layout-align": "left"})),
            " layout-align=\"left\""
        );
        // The default writes neither.
        assert_eq!(div(serde_json::json!({"fig.align": "default"})), "");
    }

    #[test]
    fn a_cell_option_width_beats_the_include_graphics_width() {
        let mut data = crate::presenter::output::MimeBundle::new();
        data.insert("image/png".to_string(), serde_json::json!("AAAA"));
        let graphic = || CellOutput::DisplayData {
            data: data.clone(),
            metadata: serde_json::json!({"quarto_file_path": "made.png", "quarto_knitr": {
                "include_graphics": true, "width": 200.0}}),
        };
        let md = render_opts(
            &opts(serde_json::json!({"out-width": "50%"})),
            vec![graphic()],
        );
        assert!(md.contains("![](made.png){width=50%}"), "{md}");
        // D7: the width knitr computed shows only while `out.width` is unset at capture time,
        // so a record made without one keeps showing it until the cell re-runs (slice 8 makes
        // deleting the option from the header stale).
        let md = render_opts(&opts(serde_json::json!({})), vec![graphic()]);
        assert!(md.contains("![](made.png){width=200}"), "{md}");
    }

    #[test]
    fn a_stored_image_takes_its_size_from_the_output_metadata_and_a_cell_option_wins() {
        let image = || {
            let mut data = crate::presenter::output::MimeBundle::new();
            data.insert("image/png".to_string(), serde_json::json!(PNG));
            CellOutput::DisplayData {
                data,
                metadata: serde_json::json!({"image/png": {"width": 300, "height": 200}}),
            }
        };
        assert_eq!(
            images(&CellOptions::default(), vec![image()]),
            ["![](P){width=300 height=200}"]
        );
        assert_eq!(
            images(
                &opts(serde_json::json!({"out-width": "50%"})),
                vec![image()]
            ),
            ["![](P){width=50% height=200}"]
        );
    }

    /// OPR-7h: QuartoNotebookRunner puts the PNG size in the metadata of the last value, which
    /// arrives as an `execute_result`, not a `display_data`. The plain route reads it from both.
    #[test]
    fn an_execute_result_image_takes_its_size_from_the_output_metadata_too() {
        let mut data = crate::presenter::output::MimeBundle::new();
        data.insert("image/png".to_string(), serde_json::json!(PNG));
        let image = CellOutput::ExecuteResult {
            execution_count: 1,
            data,
            metadata: serde_json::json!({"image/png": {"width": 140, "height": 90}}),
        };
        assert_eq!(
            images(&CellOptions::default(), vec![image]),
            ["![](P){width=140 height=90}"]
        );
    }

    // === KNR-3 slice 7, Phase 7c: the figure link ===

    /// Like `render_opts`, with figures written under a temp dir so a plot is an image.
    fn render_figs(o: &CellOptions, outputs: Vec<CellOutput>) -> String {
        let dir = tempfile::tempdir().unwrap();
        let mut fig = FsFigureSink::new(&dir.path().join("doc.qmd"));
        fig.begin_cell();
        render_cell_with(
            "r",
            "x\n",
            &result(outputs),
            o,
            &mut fig,
            CellVisibility::VISIBLE,
        )
    }

    #[test]
    fn a_plot_with_fig_link_is_wrapped_in_a_link() {
        let o = opts(serde_json::json!({"fig-link": "http://example.com/x", "fig-align": "left"}));
        let md = render_figs(&o, vec![plot()]);
        let line = md.lines().find(|l| l.contains("![")).unwrap();
        assert!(
            line.starts_with("[![](")
                && line.contains("){fig-align='left'}](http://example.com/x)"),
            "{md}"
        );
        // knitr\'s dotted spelling is an alias; the link is verbatim.
        let o = opts(serde_json::json!({"fig.link": "a b.html#c"}));
        let md = render_figs(&o, vec![plot()]);
        assert!(md.contains("](a b.html#c)"), "{md}");
    }

    #[test]
    fn an_include_graphics_image_with_fig_link_is_wrapped_in_a_link() {
        let mut data = crate::presenter::output::MimeBundle::new();
        data.insert("image/png".to_string(), serde_json::json!("AAAA"));
        let o = opts(serde_json::json!({"fig-link": "http://x"}));
        let md = render_opts(
            &o,
            vec![CellOutput::DisplayData {
                data,
                metadata: serde_json::json!({"quarto_file_path": "made.png", "quarto_knitr": {
                    "include_graphics": true, "width": 200.0}}),
            }],
        );
        assert!(md.contains("[![](made.png){width=200}](http://x)"), "{md}");
    }

    #[test]
    fn fig_link_recycles_by_figure_and_results_asis_returns_the_bare_link() {
        let o = opts(serde_json::json!({"fig-link": ["http://a", "http://b"]}));
        let md = render_figs(&o, vec![plot(), plot()]);
        let tails: Vec<&str> = md
            .lines()
            .filter(|l| l.contains("!["))
            .map(|l| &l[l.rfind("](").unwrap()..])
            .collect();
        assert_eq!(tails, ["](http://a)", "](http://b)"], "{md}");
        // hooks.R returns the bare markdown (no output div) under `results = "asis"`.
        let o = opts(serde_json::json!({"fig-link": "http://x", "results": "asis"}));
        let md = render_figs(&o, vec![plot()]);
        assert!(
            !md.contains("cell-output-display") && md.contains("](http://x)"),
            "{md}"
        );
    }

    // === KNR-3 slice 7, Phase 7d: cell and source classes ===

    #[test]
    fn the_cell_classes_follow_hooks_rs_order() {
        let o = opts(serde_json::json!({
            "tbl-cap-location": "margin", "fig-cap-location": "bottom", "cap-location": "top",
            "tbl-column": "screen", "fig-column": "page", "column": "margin", "panel": "tabset",
            "classes": ["a", ".b"],
        }));
        assert_eq!(
            o.cell_div_attrs(),
            " .a .b .panel-tabset .column-margin .fig-column-page .tbl-column-screen \
             .caption-top .fig-cap-location-bottom .tbl-cap-location-margin"
        );
        // A string is one class, never split; a leading dot is kept, not doubled.
        assert_eq!(
            opts(serde_json::json!({"classes": "plain"})).cell_div_attrs(),
            " .plain"
        );
        assert_eq!(
            opts(serde_json::json!({"classes": ".plain"})).cell_div_attrs(),
            " .plain"
        );
        // knitr\'s dotted spelling of fig-column and fig-cap-location is an alias.
        assert_eq!(
            opts(serde_json::json!({"fig.column": "page", "fig.cap-location": "top"}))
                .cell_div_attrs(),
            " .fig-column-page .fig-cap-location-top"
        );
    }

    #[test]
    fn cap_location_writes_its_own_value_not_fig_cap_locations() {
        // D5: Quarto 1 writes `.caption-margin` here (quarto-cli#15281); the presenter does not.
        let o = opts(serde_json::json!({"cap-location": "top", "fig-cap-location": "margin"}));
        let div = o.cell_div_attrs();
        assert!(
            div.contains(" .caption-top") && !div.contains(".caption-margin"),
            "{div}"
        );
        assert!(
            !opts(serde_json::json!({"fig-cap-location": "margin"}))
                .cell_div_attrs()
                .contains("caption-")
        );
    }

    #[test]
    fn a_class_option_writes_a_class_only_for_a_scalar_string() {
        for value in [
            serde_json::json!(true),
            serde_json::json!(3),
            serde_json::json!(["top", "bottom"]),
        ] {
            for key in [
                "cap-location",
                "fig-cap-location",
                "tbl-cap-location",
                "fig-column",
                "tbl-column",
            ] {
                let div = opts(serde_json::json!({ key: value.clone() })).cell_div_attrs();
                assert_eq!(div, "", "{key}: {value}");
            }
        }
    }

    #[test]
    fn a_class_or_a_derived_layout_align_keeps_the_div_under_results_asis() {
        for json in [
            serde_json::json!({"results": "asis", "classes": "plain"}),
            serde_json::json!({"results": "asis", "cap-location": "top"}),
            serde_json::json!({"results": "asis", "fig-align": "center"}),
        ] {
            let md = render_opts(&opts(json.clone()), vec![stream("t")]);
            assert!(md.starts_with("::: {.cell"), "{json}: {md}");
        }
    }

    #[test]
    fn code_overflow_adds_a_class_to_the_source_fence_after_cell_code() {
        let fence = |json: serde_json::Value| echoed_source_fence_with("r", "x", &opts(json), &[]);
        assert!(
            fence(serde_json::json!({"code-overflow": "wrap"}))
                .starts_with("```{.r .cell-code .code-overflow-wrap}")
        );
        assert!(
            fence(serde_json::json!({"code-overflow": "scroll"}))
                .starts_with("```{.r .cell-code .code-overflow-scroll}")
        );
        assert!(
            fence(serde_json::json!({"code-overflow": "other"})).starts_with("```{.r .cell-code}")
        );
        assert!(
            fence(serde_json::json!({"code-overflow": "wrap", "class.source": "mine", "attr.source": "data-x=\"1\""}))
                .starts_with("```{.r .mine .cell-code .code-overflow-wrap data-x=\"1\"}"),
        );
    }

    #[test]
    fn the_canonical_fig_alt_and_fig_cap_reach_the_image() {
        // `fig-alt` is read under its canonical name (KNR-3 slice 7, D10), as `fig.alt` is.
        let o = CellOptions::from_knitr(&serde_json::json!({"fig-alt": "a"}));
        assert_eq!(images(&o, vec![plot()]), ["![](P){fig-alt='a'}"]);
        let dotted = CellOptions::from_knitr(&serde_json::json!({"fig.alt": "a"}));
        assert_eq!(images(&dotted, vec![plot()]), ["![](P){fig-alt='a'}"]);
        // `fig-cap` is the caption of the figure, read under its canonical name (Phase 7g).
        let o = CellOptions::from_knitr(&serde_json::json!({"fig-cap": "c"}));
        assert_eq!(images(&o, vec![plot()]), ["![c](P)"]);
    }

    // === KNR-2 Phase 2D: knitr text hooks (prompt, comment) ===

    /// Render `code` (anchors `None` unless given) with `starts` and `o`, no echo hidden.
    fn text_cell(
        o: &CellOptions,
        code: &str,
        starts: &[usize],
        anchors: Option<&[usize]>,
        outputs: Vec<CellOutput>,
    ) -> String {
        let mut fig = FsFigureSink::new(Path::new("/nonexistent/doc.qmd"));
        render_cell_interleaved(
            "r",
            code,
            &result(outputs),
            anchors,
            starts,
            o,
            &mut fig,
            CellVisibility::VISIBLE,
        )
    }

    /// The lines between the first fence pair of `md` that holds code (`.cell-code`).
    fn code_lines(md: &str) -> Vec<&str> {
        let mut lines = md.lines();
        lines.find(|l| l.contains(".cell-code"));
        lines.take_while(|l| !l.starts_with("```")).collect()
    }

    #[test]
    fn prompt_marks_each_expression_start() {
        let o = opts(serde_json::json!({"prompt": true}));
        let md = text_cell(&o, "x <- 1:3\nf(\n  1)\ny\n", &[0, 1, 3], None, vec![]);
        assert_eq!(
            code_lines(&md),
            ["> x <- 1:3", "> f(", "+   1)", "> y"],
            "{md}"
        );
    }

    #[test]
    fn prompt_without_starts_treats_the_code_as_one_expression() {
        let o = opts(serde_json::json!({"prompt": true}));
        let md = text_cell(&o, "a\nb\n", &[], None, vec![]);
        assert_eq!(code_lines(&md), ["> a", "+ b"], "{md}");
    }

    #[test]
    fn prompt_follows_the_interleaved_fences() {
        let o = opts(serde_json::json!({"prompt": true}));
        let md = text_cell(&o, "a\nb\n", &[0, 1], Some(&[1]), vec![stream("1\n")]);
        assert!(md.contains("\n> a\n```"), "{md}");
        assert!(md.contains("\n> b\n```"), "{md}");
        assert!(!md.contains("+ b"), "{md}");
    }

    #[test]
    fn prompt_false_or_unset_changes_nothing() {
        let plain = text_cell(&CellOptions::default(), "a\nb\n", &[0, 1], None, vec![]);
        let off = opts(serde_json::json!({"prompt": false}));
        assert_eq!(text_cell(&off, "a\nb\n", &[0, 1], None, vec![]), plain);
        assert!(!plain.contains("> a"), "{plain}");
    }

    #[test]
    fn comment_prefixes_every_output_line() {
        let o = opts(serde_json::json!({"comment": "#>"}));
        let md = render_opts(&o, vec![stream("[1] 1\n\nz\n"), err("w\n")]);
        assert!(md.contains("```\n#> [1] 1\n#> \n#> z\n```"), "{md}");
        assert!(md.contains("```\n#> w\n```"), "{md}");
    }

    #[test]
    fn comment_prefixes_error_text_and_plain_results() {
        let o = opts(serde_json::json!({"comment": "##"}));
        let mut data = std::collections::HashMap::new();
        data.insert("text/plain".to_string(), serde_json::json!("[1] 2"));
        let shown = CellOutput::ExecuteResult {
            data,
            metadata: serde_json::json!({}),
            execution_count: 1,
        };
        let failed = CellOutput::Error {
            ename: String::new(),
            evalue: String::new(),
            traceback: vec!["Error: boom".into()],
        };
        let md = render_opts(&o, vec![shown, failed]);
        assert!(md.contains("```\n## [1] 2\n```"), "{md}");
        assert!(md.contains("## Error: boom"), "{md}");
    }

    #[test]
    fn empty_comment_adds_no_prefix() {
        let outs = vec![stream("1\n")];
        let plain = render_opts(&CellOptions::default(), outs.clone());
        let o = opts(serde_json::json!({"comment": ""}));
        assert_eq!(render_opts(&o, outs), plain);
    }

    // ── collapse (KNR-2 2D): outputs merge into the source fence ──

    #[test]
    fn collapse_merges_source_and_text_outputs_into_one_fence() {
        let o = opts(serde_json::json!({"collapse": true}));
        let md = text_cell(
            &o,
            "x\nmessage(1)\n",
            &[0, 1],
            Some(&[1, 2]),
            vec![stream("[1] 1\n"), err("1\n")],
        );
        assert!(
            md.contains("```{.r .cell-code}\nx\n## [1] 1\nmessage(1)\n## 1\n```\n"),
            "{md}"
        );
        assert!(!md.contains("cell-output"), "no output divs; {md}");
    }

    #[test]
    fn collapse_defaults_comment_to_hashes_only_when_unset() {
        let o = opts(serde_json::json!({"collapse": true}));
        assert_eq!(o.comment_out("a"), "## a");
        let o = opts(serde_json::json!({"collapse": true, "comment": "#>"}));
        assert_eq!(o.comment_out("a"), "#> a");
        let o = opts(serde_json::json!({"collapse": false}));
        assert_eq!(o.comment_out("a"), "a");
    }

    #[test]
    fn collapse_false_keeps_separate_fences() {
        let o = opts(serde_json::json!({"collapse": false}));
        let md = text_cell(&o, "x\n", &[0], Some(&[1]), vec![stream("1\n")]);
        assert!(md.contains("cell-output-stdout"), "{md}");
    }

    // ── results = hide | asis (KNR-2 2D) ──

    fn plain_result(text: &str) -> CellOutput {
        let mut data = std::collections::HashMap::new();
        data.insert("text/plain".to_string(), serde_json::json!(text));
        CellOutput::ExecuteResult {
            data,
            metadata: serde_json::json!({}),
            execution_count: 1,
        }
    }

    #[test]
    fn results_hide_drops_printed_text_but_keeps_messages() {
        let o = opts(serde_json::json!({"results": "hide"}));
        let md = text_cell(
            &o,
            "x\nmessage(1)\n",
            &[0, 1],
            Some(&[1, 2]),
            vec![stream("[1] 1\n"), plain_result("[1] 2"), err("1\n")],
        );
        assert!(!md.contains("[1] 1") && !md.contains("[1] 2"), "{md}");
        assert!(md.contains("cell-output-stderr"), "{md}");
    }

    #[test]
    fn results_asis_writes_text_bare_and_drops_the_wrapper() {
        let o = opts(serde_json::json!({"results": "asis"}));
        let md = text_cell(
            &o,
            "x\ncat(1)\n",
            &[0, 1],
            Some(&[1, 2]),
            vec![stream("[1] 1\n"), stream("*em*\n")],
        );
        assert_eq!(
            md,
            "\n```{.r .cell-code}\nx\n```\n\n[1] 1\n\n```{.r .cell-code}\ncat(1)\n```\n\n*em*\n\n"
        );
    }

    #[test]
    fn results_asis_keeps_the_wrapper_when_the_cell_has_attributes() {
        let o = opts(serde_json::json!({"results": "asis", "panel": "tabset"}));
        let md = text_cell(&o, "x\n", &[0], Some(&[1]), vec![stream("*em*\n")]);
        assert!(md.starts_with("::: {.cell .panel-tabset}"), "{md}");
        assert!(md.contains("\n*em*\n"), "{md}");
        assert!(!md.contains("cell-output-stdout"), "{md}");
    }

    #[test]
    fn source_runs_with_no_output_between_them_share_one_fence() {
        let o = opts(serde_json::json!({"results": "hide"}));
        let md = text_cell(
            &o,
            "x\ny\nz\n",
            &[0, 1, 2],
            Some(&[1, 2, 3]),
            vec![stream("1\n"), stream("2\n"), stream("3\n")],
        );
        assert_eq!(md.matches(".cell-code").count(), 1, "{md}");
        assert!(md.contains("x\ny\nz\n```"), "{md}");
    }

    fn asis_output(mime: &str, text: &str) -> CellOutput {
        let mut data = std::collections::HashMap::new();
        data.insert(mime.to_string(), serde_json::json!(text));
        CellOutput::DisplayData {
            data,
            metadata: serde_json::json!({"quarto_markdown_passthrough": true}),
        }
    }

    #[test]
    fn knit_asis_html_is_a_display_div_with_a_five_tick_raw_fence() {
        let o = opts(serde_json::json!({}));
        let html = "<div class='raw'>x</div>";
        let md = text_cell(
            &o,
            "f()\n",
            &[0],
            Some(&[1]),
            vec![asis_output("text/html", html)],
        );
        assert!(
            md.contains(&format!(
                "::: {{.cell-output-display}}\n\n`````{{=html}}\n{html}\n`````\n\n:::"
            )),
            "{md}"
        );
    }

    #[test]
    fn knit_asis_html_under_results_asis_is_the_bare_fence() {
        let o = opts(serde_json::json!({"results": "asis"}));
        let html = "<div class='raw'>x</div>";
        let md = text_cell(
            &o,
            "f()\n",
            &[0],
            Some(&[1]),
            vec![asis_output("text/html", html)],
        );
        assert!(
            md.contains(&format!("\n`````{{=html}}\n{html}\n`````\n")),
            "{md}"
        );
        assert!(!md.contains("cell-output-display"), "{md}");
    }

    #[test]
    fn knit_asis_markdown_under_results_asis_is_bare() {
        let o = opts(serde_json::json!({"results": "asis"}));
        let md = text_cell(
            &o,
            "f()\n",
            &[0],
            Some(&[1]),
            vec![asis_output("text/markdown", "**b**")],
        );
        assert!(md.contains("\n**b**\n"), "{md}");
        assert!(!md.contains("cell-output-display"), "{md}");
    }

    #[test]
    fn results_hide_drops_knit_asis_text_and_widgets() {
        let o = opts(serde_json::json!({"results": "hide"}));
        let widget = {
            let mut data = std::collections::HashMap::new();
            data.insert(
                "text/html".to_string(),
                serde_json::json!("<div id=\"w\"></div>"),
            );
            CellOutput::DisplayData {
                data,
                metadata: serde_json::json!({
                    "quarto_markdown_passthrough": true, "quarto_knitr": {"widget": true}}),
            }
        };
        let md = text_cell(
            &o,
            "f()\ng()\n",
            &[0, 1],
            Some(&[1, 2]),
            vec![asis_output("text/markdown", "dropped"), widget],
        );
        assert!(!md.contains("dropped"), "{md}");
        // knitr drops every character piece under `hide`; Q1 hides the widget too
        // (corpus s2/09_widget_filters)
        assert!(!md.contains("id=\"w\""), "{md}");
    }

    // ── fenced.echo (KNR-2 2D): the chunk as the user wrote it ──

    #[test]
    fn fenced_echo_shows_the_original_chunk_in_a_longer_fence() {
        let o = opts(serde_json::json!({"fenced.echo": true}))
            .with_original_chunk("r c1", "#| fenced.echo: true\n#| echo: fenced");
        let md = text_cell(&o, "x <- 1\nx\n", &[0, 1], None, vec![]);
        assert!(
            md.contains(
                "````{.cell-code}\n```{{r c1}}\n#| fenced.echo: true\nx <- 1\nx\n```\n````"
            ),
            "{md}"
        );
    }

    #[test]
    fn fenced_echo_without_option_lines_trims_the_code() {
        let o = opts(serde_json::json!({"fenced.echo": true})).with_original_chunk("r", "");
        let md = text_cell(&o, "\n\nx\n", &[2], None, vec![]);
        assert!(
            md.contains("````{.cell-code}\n```{{r}}\nx\n```\n````"),
            "{md}"
        );
    }

    #[test]
    fn fenced_echo_lengthens_the_fences_around_backticks_in_the_code() {
        let o = opts(serde_json::json!({"fenced.echo": true})).with_original_chunk("md", "");
        let md = text_cell(&o, "cat(\n```\n)\n", &[0], None, vec![]);
        assert!(md.contains("`````{.cell-code}\n````{{md}}"), "{md}");
    }

    #[test]
    fn without_the_original_chunk_fenced_echo_is_an_ordinary_fence() {
        let o = opts(serde_json::json!({"fenced.echo": true}));
        let md = text_cell(&o, "x\n", &[0], None, vec![]);
        assert!(md.contains("```{.r .cell-code}"), "{md}");
    }

    fn pos(md: &str, needle: &str) -> usize {
        md.find(needle)
            .unwrap_or_else(|| panic!("{needle:?} missing from:\n{md}"))
    }

    fn fig() -> FsFigureSink {
        FsFigureSink::new(Path::new("/nonexistent/doc.qmd"))
    }

    /// Numeric `echo` (knitr `filter_evaluate`): the selected source expressions stay, the
    /// outputs stay, and an output keeps its place relative to the surviving source.
    fn numeric_echo(
        echo: serde_json::Value,
        outputs: Vec<CellOutput>,
        anchors: &[usize],
    ) -> String {
        render_cell_interleaved(
            "r",
            "a <- 1\nb <- c(1,\n  2)\nb\n",
            &result(outputs),
            Some(anchors),
            &[0, 1, 3],
            &CellOptions::from_knitr(&serde_json::json!({"echo": echo})),
            &mut fig(),
            CellVisibility::VISIBLE,
        )
    }

    #[test]
    fn numeric_echo_keeps_only_the_selected_expressions() {
        let md = numeric_echo(serde_json::json!([1, 3]), vec![stream("out")], &[4]);
        assert!(md.contains("a <- 1\nb\n```"), "{md}");
        assert!(!md.contains("b <- c(1,"), "{md}");
        assert!(md.contains("out"), "{md}");
    }

    #[test]
    fn numeric_echo_drops_every_line_of_a_multiline_expression() {
        let md = numeric_echo(serde_json::json!(1), vec![stream("out")], &[3]);
        assert!(md.contains("a <- 1\n```"), "{md}");
        assert!(!md.contains("2)"), "{md}");
    }

    #[test]
    fn negative_numeric_echo_excludes_the_named_expressions() {
        let md = numeric_echo(serde_json::json!(-2), vec![stream("out")], &[4]);
        assert!(md.contains("a <- 1\nb\n```"), "{md}");
        assert!(!md.contains("b <- c(1,"), "{md}");
    }

    #[test]
    fn numeric_echo_keeps_outputs_after_their_surviving_source() {
        // The output printed after expression 2 (anchor 3 lines) lands after the source kept
        // before it, and the unselected expression leaves no gap.
        let md = numeric_echo(
            serde_json::json!([1, 3]),
            vec![stream("mid"), stream("end")],
            &[1, 4],
        );
        let (a, mid, b, end) = (
            pos(&md, "a <- 1"),
            pos(&md, "mid"),
            pos(&md, "\nb\n"),
            pos(&md, "end"),
        );
        assert!(a < mid && mid < b && b < end, "{md}");
        assert!(!md.contains("b <- c(1,"), "{md}");
    }

    #[test]
    fn numeric_echo_selecting_nothing_leaves_only_outputs() {
        let md = numeric_echo(serde_json::json!(9), vec![stream("out")], &[3]);
        assert!(!md.contains("cell-code"), "{md}");
        assert!(md.contains("out"), "{md}");
    }

    #[test]
    fn error_without_a_name_has_no_header() {
        // Ark and knitr give no `ename`: the text is the message and the traceback alone.
        assert_eq!(format_error_text("", "boom", &[]), "boom\n");
        assert_eq!(
            format_error_text("", "", &["Error: boom\n".to_string()]),
            "Error: boom\n\n"
        );
        assert_eq!(
            format_error_text("", "boom", &["  at f()".to_string()]),
            "boom\n  at f()\n"
        );
    }

    #[test]
    fn error_text_strips_ansi_from_the_value_too() {
        assert_eq!(
            format_error_text("E", "\u{1b}[31mboom\u{1b}[0m", &[]),
            "E: boom\n"
        );
    }

    #[test]
    fn error_with_a_name_keeps_the_header() {
        assert_eq!(
            format_error_text("ValueError", "bad", &[]),
            "ValueError: bad\n"
        );
    }

    fn stderr(text: &str) -> CellOutput {
        CellOutput::Stream {
            name: "stderr".to_string(),
            text: text.to_string(),
        }
    }

    fn kinded() -> CellOptions {
        use crate::presenter::options::OutputKind::{Message, Warning};
        CellOptions::from_knitr(&serde_json::json!({
            "class.message": "m",
            "class.warning": "w",
        }))
        .with_output_kinds(vec![Some(Message), Some(Warning), None])
    }

    #[test]
    fn class_message_and_class_warning_go_to_their_own_outputs() {
        let md = render_cell_with(
            "r",
            "x",
            &result(vec![
                stderr("a message"),
                stderr("Warning: w"),
                stderr("plain"),
            ]),
            &kinded(),
            &mut fig(),
            CellVisibility::VISIBLE,
        );
        assert!(md.contains("```{.m}\na message"), "{md}");
        assert!(md.contains("```{.w}\nWarning: w"), "{md}");
        // No kind: stderr of unknown kind takes both, as for Jupyter.
        assert!(md.contains("```{.m .w}\nplain"), "{md}");
    }

    #[test]
    fn kinds_follow_the_outputs_across_interleaved_groups() {
        // Outputs split into groups by their anchors: the second group\'s output is the cell\'s
        // second output, so it is the warning.
        let md = render_cell_interleaved(
            "r",
            "a\nb\n",
            &result(vec![stderr("a message"), stderr("Warning: w")]),
            Some(&[1, 2]),
            &[],
            &kinded(),
            &mut fig(),
            CellVisibility::VISIBLE,
        );
        assert!(md.contains("```{.m}\na message"), "{md}");
        assert!(md.contains("```{.w}\nWarning: w"), "{md}");
    }

    #[test]
    fn numeric_warning_keeps_only_the_selected_warnings_and_all_messages() {
        use crate::presenter::options::OutputKind::{Message, Warning};
        let options = CellOptions::from_knitr(&serde_json::json!({"warning": 1}))
            .with_output_kinds(vec![Some(Warning), Some(Message), Some(Warning)]);
        let md = render_cell_interleaved(
            "r",
            "a\nb\nc\n",
            &result(vec![
                stderr("Warning: one"),
                stderr("msg"),
                stderr("Warning: two"),
            ]),
            Some(&[1, 2, 3]),
            &[],
            &options,
            &mut fig(),
            CellVisibility::VISIBLE,
        );
        assert!(md.contains("Warning: one") && md.contains("msg"), "{md}");
        assert!(!md.contains("Warning: two"), "{md}");
        // The source stays in order around what remains.
        assert!(pos(&md, "\nc\n") > pos(&md, "msg"), "{md}");
    }

    #[test]
    fn negative_numeric_message_drops_the_named_messages() {
        use crate::presenter::options::OutputKind::Message;
        let options = CellOptions::from_knitr(&serde_json::json!({"message": -1}))
            .with_output_kinds(vec![Some(Message), Some(Message)]);
        let md = render_cell_interleaved(
            "r",
            "a\nb\n",
            &result(vec![stderr("first"), stderr("second")]),
            Some(&[1, 2]),
            &[],
            &options,
            &mut fig(),
            CellVisibility::VISIBLE,
        );
        assert!(!md.contains("first") && md.contains("second"), "{md}");
    }

    #[test]
    fn logical_warning_and_message_select_nothing_in_the_presenter() {
        use crate::presenter::options::OutputKind::Warning;
        let options = CellOptions::from_knitr(&serde_json::json!({"warning": false}))
            .with_output_kinds(vec![Some(Warning)]);
        assert_eq!(options.warning_selection(), None);
    }
    // ---- OPR-4d: `warning: false` with output kinds hides only the warnings ------------------

    fn warnings_off() -> CellVisibility {
        CellVisibility {
            warning: false,
            ..CellVisibility::VISIBLE
        }
    }

    /// With the runner\'s kinds (Julia), `warning: false` drops the `@warn` and keeps the
    /// `@info` and the raw stderr write, whose kind is `null`.
    #[test]
    fn warning_false_with_kinds_hides_only_the_warning_outputs() {
        let md = render_cell_with(
            "r",
            "x",
            &result(vec![
                stderr("a message"),
                stderr("Warning: w"),
                stderr("plain"),
            ]),
            &kinded(),
            &mut fig(),
            warnings_off(),
        );
        assert!(md.contains("a message") && md.contains("plain"), "{md}");
        assert!(!md.contains("Warning: w"), "{md}");
    }

    /// Without kinds (Jupyter) every stderr stream is dropped, as before.
    #[test]
    fn warning_false_without_kinds_hides_all_stderr() {
        let opts = CellOptions::from_knitr(&serde_json::json!({}));
        let md = render_cell_with(
            "r",
            "x",
            &result(vec![stderr("a message"), stderr("Warning: w")]),
            &opts,
            &mut fig(),
            warnings_off(),
        );
        assert!(
            !md.contains("a message") && !md.contains("Warning: w"),
            "{md}"
        );
    }
}

/// Jupyter `output: asis` (GH #1156): the shapes Quarto 1's `jupyter.ts` writes, checked
/// against its renders in `claude-notes/issue-reports/1156/*.q1.md`.
#[cfg(test)]
mod jupyter_asis_tests {
    use super::*;
    use std::path::Path;

    use crate::cell_options::options_to_config;
    use crate::presenter::figures::FsFigureSink;
    use crate::presenter::output::MimeBundle;

    fn jupyter(yaml: &str) -> CellOptions {
        let parsed = quarto_yaml::parse(yaml).expect("test YAML parses");
        CellOptions::from_config(Some(&options_to_config(parsed).0))
    }

    fn stream(name: &str, text: &str) -> CellOutput {
        CellOutput::Stream {
            name: name.into(),
            text: text.into(),
        }
    }

    fn bundle(mime: &str, text: &str) -> MimeBundle {
        let mut data = MimeBundle::new();
        data.insert(mime.to_string(), serde_json::json!(text));
        data
    }

    fn display(mime: &str, text: &str) -> CellOutput {
        CellOutput::DisplayData {
            data: bundle(mime, text),
            metadata: serde_json::json!({}),
        }
    }

    fn present(options: &CellOptions, echo: bool, outputs: Vec<CellOutput>) -> String {
        present_in(Path::new("/nonexistent/doc.qmd"), options, echo, outputs)
    }

    fn present_in(
        doc: &Path,
        options: &CellOptions,
        echo: bool,
        outputs: Vec<CellOutput>,
    ) -> String {
        let mut fig = FsFigureSink::new(doc);
        fig.begin_cell();
        render_cell_with(
            "python",
            "print(x)\n",
            &CellResult {
                status: ExecuteStatus::Ok,
                outputs,
                execution_count: Some(1),
            },
            options,
            &mut fig,
            CellVisibility {
                echo,
                ..CellVisibility::VISIBLE
            },
        )
    }

    const ASIS: &str = "output: asis\n";

    #[test]
    fn stdout_is_written_as_markdown_without_any_div() {
        let md = present(
            &jupyter(ASIS),
            false,
            vec![stream(
                "stdout",
                "## Generated heading\n\nSome **bold** text.\n\n::: {.callout-note}\nA note.\n:::\n",
            )],
        );
        assert_eq!(
            md,
            "\n\n\n## Generated heading\n\nSome **bold** text.\n\n::: {.callout-note}\nA note.\n:::\n"
        );
    }

    #[test]
    fn a_plain_execute_result_stays_a_fenced_block_without_its_div() {
        // Q1 (`exp-variants.q1.md`, A): `'a trailing value'` is a code block, not markdown.
        let data = bundle("text/plain", "'a trailing value'");
        let md = present(
            &jupyter(ASIS),
            false,
            vec![
                stream("stdout", "Printed **markdown**.\n"),
                CellOutput::ExecuteResult {
                    data,
                    metadata: serde_json::json!({}),
                    execution_count: 1,
                },
            ],
        );
        assert!(md.contains("\nPrinted **markdown**.\n"), "{md}");
        assert!(md.contains("\n```\n'a trailing value'\n```\n"), "{md}");
        assert!(!md.contains(":::"), "no cell or output div: {md}");
    }

    #[test]
    fn stderr_and_errors_are_bare_fences() {
        let md = present(
            &jupyter(ASIS),
            false,
            vec![
                stream("stderr", "to stderr\n"),
                CellOutput::Error {
                    ename: "ValueError".into(),
                    evalue: "boom".into(),
                    traceback: vec![],
                },
            ],
        );
        assert!(md.contains("\n```\nto stderr\n```\n"), "{md}");
        assert!(md.contains("\n```\nValueError: boom\n```\n"), "{md}");
        assert!(!md.contains(":::"), "{md}");
    }

    #[test]
    fn markdown_and_html_displays_lose_their_divs() {
        let md = present(
            &jupyter(ASIS),
            false,
            vec![
                display("text/markdown", "Displayed *markdown*."),
                display("text/html", "<b>hi</b>"),
            ],
        );
        assert!(md.contains("\nDisplayed *markdown*.\n"), "{md}");
        assert!(md.contains("\n```{=html}\n<b>hi</b>\n```\n"), "{md}");
        assert!(!md.contains(":::"), "{md}");
    }

    #[test]
    fn an_image_is_bare_figure_markdown() {
        let dir = tempfile::tempdir().unwrap();
        let md = present_in(
            &dir.path().join("doc.qmd"),
            &jupyter(ASIS),
            false,
            vec![display(
                "image/png",
                "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==",
            )],
        );
        assert!(md.contains("\n![]("), "{md}");
        assert!(!md.contains(":::"), "{md}");
    }

    #[test]
    fn an_echoed_source_still_precedes_the_bare_output() {
        // Q1 (`exp-variants.q1.md`, C): the code fence, then the printed markdown, no div.
        let md = present(&jupyter(ASIS), true, vec![stream("stdout", "**bold**\n")]);
        assert_eq!(md, "\n```{.python .cell-code}\nprint(x)\n```\n\n**bold**\n");
    }

    #[test]
    fn classes_keep_the_cell_div() {
        // Q1 (`exp-variants.q1.md`, D): `needCell`.
        let md = present(
            &jupyter("output: asis\nclasses: keepme\n"),
            false,
            vec![stream("stdout", "Inside a **div**.\n")],
        );
        assert!(md.starts_with("::: {.cell .keepme}"), "{md}");
        assert!(md.contains("\nInside a **div**.\n"), "{md}");
        assert!(!md.contains("cell-output"), "{md}");
    }

    #[test]
    fn a_table_label_keeps_the_cell_div_with_its_id() {
        // Q1 writes `::: {#tbl-gen .cell tbl-cap='…'}` around the generated pipe table.
        let md = present(
            &jupyter("output: asis\nlabel: tbl-gen\ntbl-cap: Generated table\n"),
            false,
            vec![stream("stdout", "| a | b |\n|---|---|\n| 1 | 2 |\n")],
        );
        assert!(md.starts_with("::: {#tbl-gen .cell"), "{md}");
        assert!(md.contains("\n| a | b |\n|---|---|\n| 1 | 2 |\n"), "{md}");
        assert!(!md.contains("cell-output"), "{md}");
    }

    #[test]
    fn output_true_and_unset_are_not_asis() {
        for yaml in ["output: true\n", "echo: false\n"] {
            let md = present(&jupyter(yaml), false, vec![stream("stdout", "**x**\n")]);
            assert!(md.contains("cell-output-stdout"), "{yaml}: {md}");
        }
    }

    #[test]
    fn a_cell_output_overrides_a_document_level_asis() {
        // Q1 (`exp-doc-level.q1.md`): `execute: output: asis`, overridden by a cell.
        let doc = options_to_config(quarto_yaml::parse(ASIS).unwrap()).0;
        let resolve = |cell: Option<&str>| {
            let cell = cell.map(|c| quarto_yaml::parse(c).unwrap());
            CellOptions::from_config(
                crate::presenter::visibility::resolve_cell_options(Some(&doc), cell).as_ref(),
            )
        };
        assert!(resolve(None).output_asis());
        assert!(resolve(Some("echo: false\n")).output_asis());
        assert!(!resolve(Some("output: true\n")).output_asis());
    }
}

/// OPR-7g: printed text goes through `ansi::output_text`; the record stays raw.
#[cfg(test)]
mod ansi_tests {
    use std::collections::HashMap;

    use super::*;
    use crate::presenter::figures::FsFigureSink;

    const RED: &str = "\x1b[31mred\x1b[0m";

    fn markdown(outputs: Vec<CellOutput>, options: &CellOptions) -> String {
        let dir = tempfile::tempdir().unwrap();
        let mut fig = FsFigureSink::new(&dir.path().join("doc.qmd"));
        fig.begin_cell();
        let result = CellResult {
            status: ExecuteStatus::Ok,
            outputs,
            execution_count: Some(1),
        };
        format_outputs_with(&result, &mut fig, true, options, &mut 0, 0)
    }

    fn text_output(variant_display: bool) -> CellOutput {
        let mut data = HashMap::new();
        data.insert("text/plain".to_string(), serde_json::json!(RED));
        let metadata = serde_json::json!({});
        if variant_display {
            CellOutput::DisplayData { data, metadata }
        } else {
            CellOutput::ExecuteResult {
                data,
                metadata,
                execution_count: 1,
            }
        }
    }

    fn every_text_site() -> Vec<CellOutput> {
        vec![
            CellOutput::Stream {
                name: "stdout".into(),
                text: RED.into(),
            },
            CellOutput::Stream {
                name: "stderr".into(),
                text: RED.into(),
            },
            text_output(true),
            text_output(false),
            CellOutput::Error {
                ename: "\x1b[1mE\x1b[0m".into(),
                evalue: RED.into(),
                traceback: vec![RED.into()],
            },
        ]
    }

    #[test]
    fn a_cell_with_no_resolved_options_is_presented_without_escapes() {
        let md = markdown(every_text_site(), &CellOptions::default());
        assert!(!md.contains('\x1b'), "{md:?}");
        assert_eq!(md.matches("red").count(), 6, "{md}");
    }

    #[test]
    fn a_cell_with_resolved_options_keeps_the_text_it_was_given() {
        let options = CellOptions::default().with_engine_text(true);
        let md = markdown(every_text_site()[..4].to_vec(), &options);
        assert_eq!(md.matches(RED).count(), 4, "{md:?}");
    }

    #[test]
    fn results_asis_goes_through_the_same_function() {
        let asis = CellOptions::from_knitr(&serde_json::json!({"results": "asis"}));
        let raw = markdown(
            every_text_site()[..1].to_vec(),
            &asis.clone().with_engine_text(true),
        );
        assert!(raw.contains('\x1b'), "{raw:?}");
        let stripped = markdown(every_text_site()[..1].to_vec(), &asis);
        assert!(
            !stripped.contains('\x1b') && stripped.contains("red"),
            "{stripped:?}"
        );
    }

    #[test]
    fn an_error_status_with_no_outputs_is_stripped() {
        let dir = tempfile::tempdir().unwrap();
        let mut fig = FsFigureSink::new(&dir.path().join("doc.qmd"));
        fig.begin_cell();
        let result = CellResult {
            status: ExecuteStatus::Error {
                ename: "E".into(),
                evalue: RED.into(),
                traceback: vec![RED.into()],
            },
            outputs: vec![],
            execution_count: None,
        };
        let md = format_outputs_with(&result, &mut fig, true, &CellOptions::default(), &mut 0, 0);
        assert!(!md.contains('\x1b') && md.contains("red"), "{md:?}");
    }

    #[test]
    fn presenting_does_not_touch_the_record_text() {
        let outputs = every_text_site();
        let _ = markdown(outputs.clone(), &CellOptions::default());
        match &outputs[0] {
            CellOutput::Stream { text, .. } => assert_eq!(text, RED),
            _ => unreachable!(),
        }
    }

    #[test]
    fn the_stored_record_stays_raw_through_presentation_write_and_read() {
        let mut value: serde_json::Value =
            serde_json::from_str(include_str!("../../tests/fixtures/records/basic.json")).unwrap();
        value["cells"][0]["outputs"] = serde_json::json!([
            {"output_type": "stream", "name": "stdout", "text": [RED]},
            {"output_type": "error", "ename": "E", "evalue": RED, "traceback": [RED]}
        ]);
        let record = crate::record::read_record_value(value).unwrap();
        let before = crate::record::write_record(&record).unwrap();

        let result = CellResult::from_nbformat(&record.cells[0].outputs, Some(1));
        let md = markdown(result.outputs, &CellOptions::default());
        assert!(!md.contains('\x1b') && md.contains("red"), "{md:?}");

        let written = crate::record::write_record(&record).unwrap();
        assert_eq!(written, before);
        assert!(written.contains("\\u001b[31mred"), "{written}");
        let reread = crate::record::read_record(&written).unwrap();
        assert_eq!(reread.cells[0].outputs, record.cells[0].outputs);
    }
}
