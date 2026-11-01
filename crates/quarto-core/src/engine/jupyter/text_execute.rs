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

use quarto_source_map::SourceInfo;

use super::daemon::daemon;
use super::error::JupyterError;
use super::execute::{CellOutput, CellResult, ExecuteOpts, ExecuteStatus};
use super::inline::{
    InlineOutcome, classify_inline_result, escape_yaml_double, front_matter_key_at, span_is_owned,
    unescape_yaml_double, wrap_inline_code,
};
use super::kernelspec::{UnusableJupyterKey, kernel_from_ts_metadata, unusable_jupyter_key};
use super::router::{KernelName, SingleKernelRouter, SpliceKind, plan_units, splice_mixed};
use super::session::SessionKey;
use crate::cell_options::partition_cell_options;
use crate::engine::context::{ExecuteResult, ExecutionContext};
use crate::engine::diagnostics::{InlineSpanProblem, inline_span_diagnostic};
use crate::engine::error::ExecutionError;
use crate::engine::ts_protocol::TsMetadataValue;
use crate::engine::units::{
    CellUnit, EngineUnit, InlineUnit, Unit, runs_in_legacy_executor, units_for_engine,
    view_for_engine,
};
use crate::presenter::cell::{describe_location, render_cell};
use crate::presenter::figures::{FigureSink, FsFigureSink};
use crate::presenter::inline::{InlineTarget, render_inline};
use crate::presenter::visibility::{CellVisibility, resolve_cell_options, resolved_flag};
use quarto_pandoc_types::ConfigValue;

type JupyterResult<T> = std::result::Result<T, JupyterError>;

/// Execute code blocks in QMD input and return markdown with outputs.
///
/// This is the main entry point for text-based Jupyter execution.
pub fn execute_qmd(
    input: &str,
    ctx: &ExecutionContext,
) -> std::result::Result<ExecuteResult, ExecutionError> {
    // The document's units, relocated into this text (or scanned from it when
    // no stage supplied a shared list). Every braced cell and every span is in
    // the list; the view marks which cells jupyter owns, and `partition_cells`
    // classifies them. Spans are owned by language (EXR-3, below).
    let units = units_for_engine(&ctx.units, input);
    let handled = &ctx.handled_languages;
    let owns = |lang: &str| !handled.iter().any(|h| h.to_lowercase() == lang);
    let view = view_for_engine(&units, &owns);

    // Apply the handled_languages enforcement gate:
    //   - cells not owned by jupyter are ceded (dropped from the executable set)
    //   - cells jupyter owns but cannot execute:
    //     multi_engine=true  → loud Err(NoHandlerForLanguage)  (§10 case 4, P2-13)
    //     multi_engine=false → pass through unexecuted (display-only, P2-13)
    //   - remaining cells are executable
    let executable = partition_cells(&view, ctx.multi_engine)?;
    let spans: Vec<InlineUnit> = units
        .iter()
        .filter_map(|u| match u {
            Unit::Inline(i) => Some(i.clone()),
            Unit::Cell(_) => None,
        })
        .collect();

    // No cells and no spans: passthrough unchanged, no kernel needed.
    if executable.is_empty() && spans.is_empty() {
        return Ok(ExecuteResult::new(input));
    }

    // Determine the kernel from the first *executable* (owned) cell.
    // An explicit kernel (`jupyter:` key, then `engine_config.kernel`) wins.
    // Today one kernel runs every unit: the default router returns it for all
    // of them. JOL-2a/2b supply the choice behind the router; a reopened
    // JOL-3 (several kernels) can vary it per unit.
    let kernel_name = select_kernel(
        &ctx.metadata,
        ctx.engine_config.as_ref(),
        executable.first().map(|c| c.lang.as_str()),
    );

    // Divergence 6: the engine runs, the document has no cell at all (so nothing names a
    // kernel), no `jupyter:` key does either, and there is a span to evaluate.
    let has_cells = units.iter().any(|u| matches!(u, Unit::Cell(_)));
    if kernel_name.is_none() && !has_cells && !spans.is_empty() {
        return Err(no_kernel_for_spans());
    }

    // A span is owned when its language is in the engine's owned set or is the
    // language of the selected kernel (design decision 5). Reading the kernel's
    // language is the only await before the plan; a kernel that cannot be found
    // fails here with the same error the start would give.
    let kernel_language = match &kernel_name {
        Some(name) if !spans.is_empty() => Some(
            super::daemon::engine_runtime()
                .block_on(super::kernelspec::find_kernelspec(name))
                .map_err(|e| ExecutionError::execution_failed("jupyter", e.to_string()))?
                .language,
        ),
        _ => None,
    };
    let owned_spans: Vec<InlineUnit> = spans
        .into_iter()
        .filter(|s| span_is_owned(&s.lang, &ctx.owned_languages, kernel_language.as_deref()))
        .collect();

    if executable.is_empty() && owned_spans.is_empty() {
        // Only spans of languages nobody here owns: left literal.
        return Ok(ExecuteResult::new(input));
    }
    let Some(kernel_name) = kernel_name else {
        // Owned spans, but only ceded cells: still nothing names a kernel.
        return Err(no_kernel_for_spans());
    };
    let router = SingleKernelRouter::new(kernel_name);

    // Plan first (pure): which kernel runs each unit, in document order. A
    // router error is an `ExecutionError` and propagates as one.
    let mut run_units: Vec<Unit> = executable
        .iter()
        .cloned()
        .map(Unit::Cell)
        .chain(owned_spans.into_iter().map(Unit::Inline))
        .collect();
    run_units.sort_by_key(Unit::start);
    let plan = plan_units(&run_units, &router)?;

    // bd-hxhnnlzs: hold a kernel scope for the duration of this engine
    // run. If no outer scope (render invocation, preview server) is
    // open, the kernels spawned below are shut down when this guard
    // drops — no caller of the jupyter engine can leak a kernel.
    //
    // Placement matters: this sits *below* both early returns (the
    // `blocks.is_empty()` one above and the all-cells-ceded one just
    // above), so a pure passthrough never acquires a kernel scope.
    let _kernel_scope = super::daemon::kernel_scope();

    // Execute via async runtime. `execute_blocks_async` takes the full
    // `ExecutionContext` (main's #412 figure-emission threads it through).
    let result = execute_blocks_async(input, &run_units, &plan, ctx);

    let warning = jupyter_key_warning(&ctx.metadata);
    result
        .map(|mut r| {
            r.warnings.extend(warning);
            r
        })
        .map_err(|e| ExecutionError::execution_failed("jupyter", e.to_string()))
}

/// Divergence 6's error: spans to evaluate and no way to pick a kernel.
fn no_kernel_for_spans() -> ExecutionError {
    ExecutionError::execution_failed(
        "jupyter",
        "inline expressions need an executable cell or a `jupyter:` kernel: the document \
         has `{lang} expr` spans but names no kernel. \
         Add `jupyter: <kernel-name>` to the document's front matter.",
    )
}

/// The `Q-18-4` warning for a `jupyter:` value that names no kernel, if any.
///
/// Q1 errors on these forms; q2 warns and falls back to the next source
/// (`engine_config.kernel`, then the first owned cell's language).
pub(crate) fn jupyter_key_warning(
    metadata: &std::collections::HashMap<String, TsMetadataValue>,
) -> Option<quarto_error_reporting::DiagnosticMessage> {
    use quarto_error_reporting::DiagnosticMessageBuilder;
    let (problem, hint) = match unusable_jupyter_key(metadata)? {
        UnusableJupyterKey::MapWithoutKernel => (
            "the `jupyter:` map has no `kernel` key, so it names no kernel.",
            "Write `jupyter: <kernel-name>` or `jupyter: {kernel: <kernel-name>}`.",
        ),
        UnusableJupyterKey::KernelspecObject => (
            "the `kernelspec:` object form of `jupyter:` is not supported.",
            "Name the kernel instead: `jupyter: <kernel-name>`.",
        ),
    };
    Some(
        DiagnosticMessageBuilder::warning("Jupyter Kernel Not Named")
            .with_code("Q-18-4")
            .problem(problem)
            .add_detail("Falling back to the kernel for the language of the first executable cell.")
            .add_hint(hint)
            .build(),
    )
}

/// Choose the kernel for a document.
///
/// Order: the `jupyter:` document key, then `engine_config.kernel`
/// (`engine: [{jupyter: {kernel: x}}]`), then the kernel mapped from the
/// first owned cell's language. Returns `None` only when none of the three
/// exists; unlike `resolve_kernel`, it never defaults to Python, so a
/// cell-less document with no key stays unresolved.
pub(crate) fn select_kernel(
    metadata: &std::collections::HashMap<String, TsMetadataValue>,
    engine_config: Option<&ConfigValue>,
    first_owned_language: Option<&str>,
) -> Option<String> {
    kernel_from_ts_metadata(metadata)
        .or_else(|| {
            engine_config
                .and_then(|c| c.get("kernel"))
                .and_then(|k| k.as_plain_text())
        })
        .or_else(|| first_owned_language.map(map_language_to_kernel))
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

#[cfg(test)]
use crate::engine::units::CodeBlock;

/// The pre-scanner cell finder, kept only so tests can pin what it did and
/// prove the scanner reproduces it (EXR-2 Phase 0 characterization and the
/// Phase 4 corpus-equality test). Production goes through the scanner.
#[cfg(test)]
pub(super) fn parse_code_blocks(input: &str) -> Vec<CodeBlock> {
    let re = regex::Regex::new(&crate::engine::patterns::executor_fence_pattern())
        .expect("Invalid regex pattern");
    let mut blocks = Vec::new();
    for cap in re.captures_iter(input) {
        let full_match = cap.get(0).unwrap();
        let code_match = cap.get(2).unwrap();
        blocks.push(CodeBlock {
            start: full_match.start(),
            end: full_match.end(),
            code_start: code_match.start(),
            language: cap.get(1).unwrap().as_str().to_string(),
            code: code_match.as_str().to_string(),
        });
    }
    blocks
}

/// Partition braced `{lang}` cells into cells jupyter will **execute** vs.
/// cells it **cedes** verbatim, or return a loud failure.
///
/// Classification for each cell (by language `lang`):
///
/// * `lang ∈ handled_languages` → **cede**: jupyter leaves the cell verbatim
///   (it is owned by another engine or is a cell-handler language like `ojs`).
///   These cells are NOT returned in the `Ok` set.
/// * `lang ∉ handled_languages` **AND** jupyter can execute it
///   (`is_executable_language`) → **execute**: returned in `Ok`.
/// * `lang ∉ handled_languages` **AND** jupyter *cannot* execute it:
///   - `multi_engine=true`  (`|sequence| > 1`): jupyter **owns** this
///     language but has no kernel — **loud failure** `Err(NoHandlerForLanguage)`
///     (§10 case 4). The render halts with a named, actionable error.
///   - `multi_engine=false` (`|sequence| == 1`): **pass through unexecuted**.
///     The cell is left verbatim (like a ceded cell) so a single-kernel
///     doc can still contain display-only cells for other languages — the
///     Q1 `quartoMdToJupyter` display-only treatment.
///
/// `NoHandlerForLanguage` is a **clean refusal** and does NOT poison the
/// engine instance. See `ExecutionError::NoHandlerForLanguage`.
fn partition_cells(
    view: &[EngineUnit<'_>],
    multi_engine: bool,
) -> Result<Vec<CellUnit>, ExecutionError> {
    let mut executable = Vec::new();

    for engine_unit in view {
        let Unit::Cell(cell) = engine_unit.unit else {
            continue;
        };
        if !runs_in_legacy_executor(cell) {
            continue;
        }

        if !engine_unit.owned {
            // Ceded: jupyter does not execute this cell. It will remain
            // verbatim in the output (the splice copies the gap, and a ceded
            // cell is not in the executable set, so its text is part of the
            // gap).
            continue;
        }

        if is_executable_language(&cell.lang) {
            // Jupyter owns this language and can execute it.
            executable.push(cell.clone());
        } else if multi_engine {
            // Multi-engine sequence: jupyter owns this language (not in
            // handled_languages) but has no handler/kernel for it. Fail
            // loudly — §10 case 4 (P2-13).
            return Err(ExecutionError::no_handler_for_language(
                "jupyter",
                cell.lang.clone(),
            ));
        } else {
            // Single-engine sequence: pass the cell through unexecuted.
            // It is left verbatim just like a ceded cell — its text falls into
            // a copied gap. Q1's display-only treatment (P2-13).
            continue;
        }
    }

    Ok(executable)
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
            | "ggsql"
    )
}

/// Execute code blocks asynchronously and build output markdown.
fn execute_blocks_async(
    input: &str,
    units: &[Unit],
    plan: &[(usize, KernelName)],
    ctx: &ExecutionContext,
) -> JupyterResult<ExecuteResult> {
    // Drive the execution on the shared engine runtime — NOT a
    // per-call runtime. Kernel sessions (ZeroMQ sockets, the kernel
    // Child) are tokio resources bound to the runtime that created
    // them; a per-call runtime made every cross-call session reuse
    // fail with "Tokio context ... is being shutdown" (bd-hxhnnlzs).
    super::daemon::engine_runtime().block_on(execute_blocks_inner(input, units, plan, ctx))
}

/// Inner async function that does the actual execution.
async fn execute_blocks_inner(
    input: &str,
    units: &[Unit],
    plan: &[(usize, KernelName)],
    ctx: &ExecutionContext,
) -> JupyterResult<ExecuteResult> {
    let daemon = daemon();

    // Start or get an existing session for every kernel the plan names, once
    // each and before any cell runs (so a start failure surfaces before the
    // first cell, as it did with the single kernel).
    let mut sessions: std::collections::HashMap<KernelName, SessionKey> =
        std::collections::HashMap::new();
    for (_, kernel) in plan {
        if !sessions.contains_key(kernel) {
            let key = daemon
                .get_or_start_session(kernel.as_str(), &ctx.cwd, &ctx.project_env)
                .await?;
            sessions.insert(kernel.clone(), key);
        }
    }

    // Document-level defaults for cell options (bd-ohvl879u): the
    // pipeline's fully merged metadata (MetadataMergeStage runs before
    // engine execution) carries the `execute` map that cell options
    // merge over. The stage hands it to us directly (bd-nn2fou8h);
    // this used to be recovered by re-parsing the front matter of our
    // own serialized input, which only knitr's JSON-to-R boundary made
    // untenable to share.
    let doc_scope = ctx.execute_scope.clone();

    // The rendered replacement for each executed cell, spliced into the
    // input's gaps once every cell has run.
    let mut replacements: Vec<(std::ops::Range<usize>, String, SpliceKind)> = Vec::new();
    let mut warnings: Vec<quarto_error_reporting::DiagnosticMessage> = Vec::new();
    // Each kernel's language, to wrap a span for it (a `None` value on Python).
    let mut languages: std::collections::HashMap<KernelName, String> =
        std::collections::HashMap::new();
    for (kernel, key) in &sessions {
        let language = daemon
            .with_session(key, |s| s.language().to_string())
            .await
            .ok_or(JupyterError::NotConnected)?;
        languages.insert(kernel.clone(), language);
    }
    // Figure emission (bd-5t6wvu7m): image outputs are written under
    // `<stem>_files/figure-html/` next to the source document.
    let mut fig = FsFigureSink::new(&ctx.source_path);

    for ((_, kernel), unit) in plan.iter().zip(units) {
        let key = &sessions[kernel];
        let block = match unit {
            Unit::Cell(block) => block,
            Unit::Inline(span) => {
                if let Some(text) = evaluate_span(
                    &daemon,
                    key,
                    &languages[kernel],
                    span,
                    input,
                    ctx,
                    &mut warnings,
                )
                .await?
                {
                    replacements.push((span.start..span.end, text, SpliceKind::Inline));
                }
                continue;
            }
        };
        // Partition the cell into `#|` options + code (bd-ohvl879u).
        // `ctx.source_info` maps input offsets back to the original
        // files, so option spans and diagnostics resolve to real
        // source positions.
        let body_source = SourceInfo::substring(
            ctx.source_info.clone(),
            block.code_start,
            block.code_start + block.code.len(),
        );
        let cell =
            partition_cell_options(&block.lang, &block.code, body_source.clone()).map_err(|e| {
                let at = e
                    .location()
                    .and_then(|loc| describe_location(loc, 0, ctx))
                    .or_else(|| describe_location(&body_source, 0, ctx))
                    .map(|l| format!(" at {l}"))
                    .unwrap_or_default();
                JupyterError::InvalidCellOptions {
                    message: format!("cell options are not valid YAML{at}: {e}"),
                }
            })?;
        let resolved = resolve_cell_options(doc_scope.as_ref(), cell.options);
        // Q1's default `execute: error: false` — a raising cell aborts
        // unless something opts in.
        let allow_errors = resolved_flag(resolved.as_ref(), "error", false);
        let visibility = CellVisibility::resolve(resolved.as_ref());

        // Execute the code — the partitioned code only, without the
        // option lines (Q1 strips them too, so cell magics work).
        let exec_result = daemon
            .execute_in_session(key, &cell.code)
            .await
            .ok_or(JupyterError::NotConnected)??;

        // Error policy (bd-ohvl879u, knitr/Q1 parity): a raising cell
        // aborts the render unless the cell or the document allows
        // errors in output.
        if !allow_errors && let Some((ename, evalue)) = first_cell_error(&exec_result) {
            let at = describe_location(&body_source, 0, ctx)
                .map(|l| format!(" at {l}"))
                .unwrap_or_default();
            return Err(JupyterError::CellExecutionFailed {
                message: format!(
                    "code cell{at} raised {ename}: {evalue}\n\
                     Use `#| error: true` on the cell (or `execute: error: true` in the \
                     document metadata) to show the error in the output instead."
                ),
            });
        }

        // Emit the visible parts of the cell — echoed (option-stripped)
        // source plus outputs — in the Quarto-canonical `::: {.cell}`
        // shape. `begin_cell` runs for every *executed* cell, visible
        // or not, so figure file names stay keyed to a cell's position
        // in the document rather than shifting when an earlier cell is
        // hidden.
        fig.begin_cell();
        replacements.push((
            block.start..block.end,
            render_cell(&block.lang, &cell.code, &exec_result, &mut fig, visibility),
            SpliceKind::Block,
        ));
    }

    let output = splice_mixed(input, &replacements);

    // Report the `<stem>_files` dir when figures were written —
    // dir-level, mirroring knitr. `q2 render` copies it to the output
    // dir (bd-o8pr) and the preview capture embeds it (bd-qbhp2cvv).
    let mut result = ExecuteResult::new(output);
    result.warnings.extend(warnings);
    if fig.wrote_any() {
        result = result.with_supporting_files(vec![fig.files_dir()]);
    }
    Ok(result)
}

/// Run one inline expression and return the text that replaces the span, or `None` to
/// leave the span as written (an empty value, or no usable result with a warning pushed).
///
/// Errors are fatal: the kernel raised, the expression produced no value, or a front
/// matter value cannot be written back (design decision 2, divergences 1, 6 and 10).
async fn evaluate_span(
    daemon: &super::daemon::JupyterDaemon,
    key: &SessionKey,
    kernel_language: &str,
    span: &InlineUnit,
    input: &str,
    ctx: &ExecutionContext,
    warnings: &mut Vec<quarto_error_reporting::DiagnosticMessage>,
) -> JupyterResult<Option<String>> {
    let expr_source = SourceInfo::substring(
        ctx.source_info.clone(),
        span.code_start,
        span.code_start + span.code.len(),
    );
    let location = describe_location(&expr_source, 0, ctx);
    let diagnostic = |problem: &InlineSpanProblem| {
        inline_span_diagnostic(problem, &span.lang, &span.code, location.as_deref())
    };
    let fatal = |problem: &InlineSpanProblem| JupyterError::CellExecutionFailed {
        message: diagnostic(problem).to_text(None),
    };

    // The writer escaped a front matter value as a YAML double-quoted string; the
    // kernel must see the expression as the author wrote it.
    let expr = if span.in_front_matter {
        unescape_yaml_double(&span.code)
    } else {
        span.code.clone()
    };
    let result = daemon
        .execute_in_session_with(
            key,
            &wrap_inline_code(kernel_language, &expr),
            ExecuteOpts::INLINE,
        )
        .await
        .ok_or(JupyterError::NotConnected)??;

    let text = match classify_inline_result(&result) {
        InlineOutcome::Raised { ename, evalue } => {
            return Err(fatal(&InlineSpanProblem::Raised { ename, evalue }));
        }
        InlineOutcome::Silent => return Err(fatal(&InlineSpanProblem::NoValue)),
        InlineOutcome::NoTextValue => None,
        InlineOutcome::Value(bundle) => {
            render_inline(&bundle, InlineTarget::from_format(&ctx.format))
        }
    };
    let Some(text) = text else {
        warnings.push(diagnostic(&InlineSpanProblem::NoUsableResult));
        return Ok(None);
    };
    // Q1 leaves the span source in place for an empty value (divergence 7).
    if text.is_empty() {
        return Ok(None);
    }
    if !span.in_front_matter {
        return Ok(Some(text));
    }
    if text.contains(['\n', '\r']) {
        let key = front_matter_key_at(input, span.start).unwrap_or_else(|| "?".to_string());
        return Err(fatal(&InlineSpanProblem::FrontMatterValue {
            key,
            reason: "the value contains a newline, which a double-quoted YAML string would \
                     fold to a space"
                .to_string(),
        }));
    }
    Ok(Some(escape_yaml_double(&text)))
}

/// The first error a cell produced, from its outputs or its status.
fn first_cell_error(result: &CellResult) -> Option<(String, String)> {
    for output in &result.outputs {
        if let CellOutput::Error { ename, evalue, .. } = output {
            return Some((ename.clone(), evalue.clone()));
        }
    }
    if let ExecuteStatus::Error { ename, evalue, .. } = &result.status {
        return Some((ename.clone(), evalue.clone()));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::HANDLED_LANGUAGES;
    use crate::engine::units::legacy_cells;

    // ── select_kernel (JOL-2a) ───────────────────────────────────────────────

    fn jupyter_meta(name: &str) -> std::collections::HashMap<String, TsMetadataValue> {
        std::collections::HashMap::from([(
            "jupyter".to_string(),
            TsMetadataValue::String(name.to_string()),
        )])
    }

    fn kernel_config(name: &str) -> ConfigValue {
        use quarto_pandoc_types::config_value::ConfigMapEntry;
        let entry = ConfigMapEntry {
            key: "kernel".to_string(),
            key_source: SourceInfo::for_test(),
            value: ConfigValue::new_string(name, SourceInfo::for_test()),
        };
        ConfigValue::new_map(vec![entry], SourceInfo::for_test())
    }

    #[test]
    fn test_select_kernel_explicit_wins_over_first_cell_language() {
        let got = select_kernel(&jupyter_meta("sos"), None, Some("julia"));
        assert_eq!(got.as_deref(), Some("sos"));
    }

    #[test]
    fn test_select_kernel_falls_back_to_map_language_to_kernel() {
        let got = select_kernel(&Default::default(), None, Some("r"));
        assert_eq!(got.as_deref(), Some("ir"));
    }

    #[test]
    fn test_select_kernel_no_cells_with_key() {
        let got = select_kernel(&jupyter_meta("python3"), None, None);
        assert_eq!(got.as_deref(), Some("python3"));
    }

    #[test]
    fn test_select_kernel_no_cells_no_key_is_none() {
        assert_eq!(select_kernel(&Default::default(), None, None), None);
    }

    #[test]
    fn test_select_kernel_engine_config_kernel() {
        let cfg = kernel_config("sos");
        let got = select_kernel(&Default::default(), Some(&cfg), Some("python"));
        assert_eq!(got.as_deref(), Some("sos"));
    }

    #[test]
    fn test_select_kernel_jupyter_key_beats_engine_config() {
        let cfg = kernel_config("sos");
        let got = select_kernel(&jupyter_meta("julia"), Some(&cfg), Some("python"));
        assert_eq!(got.as_deref(), Some("julia"));
    }

    #[test]
    fn test_jupyter_key_warning_codes_and_absence() {
        assert!(jupyter_key_warning(&Default::default()).is_none());
        assert!(jupyter_key_warning(&jupyter_meta("python3")).is_none());
        let per_language = std::collections::HashMap::from([(
            "jupyter".to_string(),
            TsMetadataValue::Map(std::collections::HashMap::from([(
                "python".to_string(),
                TsMetadataValue::String("python3".to_string()),
            )])),
        )]);
        let w = jupyter_key_warning(&per_language).expect("map without kernel warns");
        assert_eq!(w.code.as_deref(), Some("Q-18-4"));
        let spec = std::collections::HashMap::from([(
            "jupyter".to_string(),
            TsMetadataValue::Map(std::collections::HashMap::from([(
                "kernelspec".to_string(),
                TsMetadataValue::Map(Default::default()),
            )])),
        )]);
        let w = jupyter_key_warning(&spec).expect("kernelspec object warns");
        assert!(
            w.problem.as_ref().unwrap().as_str().contains("kernelspec"),
            "{w:?}"
        );
    }

    /// A missing explicit kernel surfaces the existing rich
    /// `format_kernelspec_not_found` message: the name, the searched paths,
    /// the available kernels and the `JUPYTER_PATH` hint. The lookup fails
    /// before any kernel starts, so this needs no ipykernel.
    #[test]
    fn test_missing_explicit_kernel_gives_the_rich_error() {
        let ctx = ExecutionContext::new(
            std::env::temp_dir(),
            std::env::temp_dir(),
            std::path::PathBuf::from("doc.qmd"),
            "html",
        )
        .with_metadata(jupyter_meta("q2-no-such-kernel-jol2a"));
        let err = execute_qmd("```{python}\n1 + 1\n```\n", &ctx)
            .expect_err("an unknown kernel must fail")
            .to_string();
        for needle in [
            "q2-no-such-kernel-jol2a",
            "searched:",
            "available kernels",
            "JUPYTER_PATH",
        ] {
            assert!(err.contains(needle), "missing {needle:?} in: {err}");
        }
    }

    // ── Helper: build a CodeBlock for tests ──────────────────────────────────

    fn make_block(language: &str, code: &str) -> CodeBlock {
        CodeBlock {
            start: 0,
            end: language.len() + code.len() + 10, // approximate, not checked by tests
            code_start: language.len() + 5,        // approximate; not checked by tests
            language: language.to_string(),
            code: code.to_string(),
        }
    }

    /// Partition the cells of `input` the way `execute_qmd` does: scan once,
    /// let the engine's ownership rule mark the view, then classify.
    fn partition_text(
        input: &str,
        handled: &[String],
        multi_engine: bool,
    ) -> Result<Vec<CellUnit>, ExecutionError> {
        let units = crate::engine::units::scan_units(input);
        let owns = |lang: &str| !handled.iter().any(|h| h.to_lowercase() == lang);
        let view = view_for_engine(&units, &owns);
        partition_cells(&view, multi_engine)
    }

    /// `partition_text` over bare `{lang}` cells built from `(language, code)`.
    fn partition(
        blocks: Vec<CodeBlock>,
        handled: &[String],
        multi_engine: bool,
    ) -> Result<Vec<CellUnit>, ExecutionError> {
        let text: String = blocks
            .iter()
            .map(|b| format!("```{{{}}}\n{}\n```\n\n", b.language, b.code))
            .collect();
        partition_text(&text, handled, multi_engine)
    }

    // ── T14 (H1): mask's opener-widening reddens this ───────────────────────
    //
    // The cell finder is private, so this seam lives in this module
    // rather than exposing it (controller ruling R5). Runs the real
    // `nested_cell_mask::mask` end to end — parse a display block containing
    // a nested `{python}` opener, mask it, serialize the masked AST back to
    // qmd — and asserts jupyter's own partitioner finds nothing to execute
    // in the result. Reverting H1 (mask stops rewriting the opener) leaves
    // the literal `{python}` opener in the masked text, which
    // the scanner *would* then match — turning this RED. (Moved from the
    // old regex finder to `scan_units` in EXR-2 Phase 4.)
    #[test]
    fn t14_masked_display_block_yields_no_jupyter_cells() {
        let qmd = "````markdown\n```{python}\nprint(\"x\")\n````\n";
        let mut output = Vec::<u8>::new();
        let (doc, _ast_context, _warnings) =
            pampa::readers::qmd::read(qmd.as_bytes(), false, "test.qmd", &mut output, true, None)
                .expect("parse qmd fixture");

        let mut masked_doc = doc;
        let changed = crate::engine::nested_cell_mask::mask(&mut masked_doc);
        assert!(
            !changed.is_empty(),
            "fixture must actually get masked for this test to be meaningful"
        );

        let mut buf = Vec::new();
        pampa::writers::qmd::write(&masked_doc, &mut buf).expect("serialize masked qmd fixture");
        let masked_text = String::from_utf8(buf).expect("qmd writer must emit valid utf8");

        let cells = legacy_cells(&crate::engine::units::scan_units(&masked_text));
        assert!(
            cells.is_empty(),
            "masked text must yield 0 cells from jupyter's partitioner, got: {cells:?}"
        );
    }

    // EXR-2 Phase 2: the mask covers every non-cell display fence, so an opener
    // shown in a language-named fence (` ````text `, ` ````r `) is masked and the
    // executor finds no cell. Before the widening only bare and `markdown`
    // fences were masked and a ` ````text ` opener executed.
    #[test]
    fn exr2_p2_masked_language_named_display_fence_yields_no_jupyter_cells() {
        for info in ["markdown", "text", "r", "python", ""] {
            let qmd = format!("````{info}\n```{{python}}\nprint(1)\n```\n````\n");
            let mut output = Vec::<u8>::new();
            let (doc, _ctx, _warnings) =
                pampa::readers::qmd::read(qmd.as_bytes(), false, "t.qmd", &mut output, true, None)
                    .expect("parse");
            let mut masked = doc;
            assert!(
                !crate::engine::nested_cell_mask::mask(&mut masked).is_empty(),
                "info {info:?} must be masked"
            );
            let mut buf = Vec::new();
            pampa::writers::qmd::write(&masked, &mut buf).expect("write");
            let text = String::from_utf8(buf).expect("utf8");
            assert!(
                legacy_cells(&crate::engine::units::scan_units(&text)).is_empty(),
                "info {info:?}: executor must see no cell in\n{text}"
            );
        }
    }

    // ── partition_cells tests ────────────────────────────────────────────────

    // -----------------------------------------------------------------------
    // Row 4 (loud failure): {sql} cell owned by jupyter (sql ∉ handled_languages)
    // but jupyter has no SQL kernel → Err(NoHandlerForLanguage{engine="jupyter",
    // language="sql"}).
    //
    // P2-13a: the loud branch is gated on multi_engine=true (multi-engine
    // sequence). Vacuity revert: remove the loud branch ⇒ this test goes RED.
    // -----------------------------------------------------------------------
    #[test]
    fn test_partition_cells_owned_unrunnable_fails_loudly() {
        // handled_languages does NOT contain "sql" → jupyter owns sql.
        let handled: Vec<String> =
            vec!["ojs".to_string(), "mermaid".to_string(), "dot".to_string()];

        let blocks = vec![make_block("sql", "SELECT 1;")];
        // multi_engine=true: |sequence| > 1 → owned-but-unrunnable → loud error.
        let result = partition(blocks, &handled, true);

        // Must be a loud Err, not Ok.
        assert!(
            result.is_err(),
            "sql owned by jupyter but not executable in multi-engine sequence → must Err"
        );
        let err = result.unwrap_err();
        // Exact variant check (not just any error).
        assert!(
            matches!(err, ExecutionError::NoHandlerForLanguage { .. }),
            "error must be NoHandlerForLanguage, got: {err:?}"
        );
        // Both fields must be named correctly.
        if let ExecutionError::NoHandlerForLanguage { engine, language } = err {
            assert_eq!(engine, "jupyter", "error must name the engine");
            assert_eq!(language, "sql", "error must name the language");
        }
    }

    // -----------------------------------------------------------------------
    // P2-13b (NEW): single-engine sequence with owned-but-unrunnable cell
    // passes through verbatim — no error, not in executable set.
    //
    // Binding: gated on multi_engine flag (NOT unconditional raise).
    // Vacuity revert: remove the |sequence|>1 gate (always raise) ⇒ this
    // test goes RED because single-engine now raises.
    // -----------------------------------------------------------------------
    #[test]
    fn test_partition_cells_single_engine_owned_unrunnable_passthrough() {
        // handled_languages does NOT contain "sql" → jupyter owns sql.
        let handled: Vec<String> =
            vec!["ojs".to_string(), "mermaid".to_string(), "dot".to_string()];

        let blocks = vec![make_block("sql", "SELECT 1;")];
        // multi_engine=false: |sequence| == 1 → owned-but-unrunnable → pass through, no error.
        let result = partition(blocks, &handled, false);

        assert!(
            result.is_ok(),
            "single-engine: owned-but-unrunnable must pass through unexecuted, not error; \
             got: {:?}",
            result.unwrap_err()
        );
        let executable = result.unwrap();
        assert!(
            executable.is_empty(),
            "single-engine: sql must NOT be in the executable set \
             (it passes through verbatim in the surrounding text); len={}",
            executable.len()
        );
    }

    // Vacuity guard for row 4: sql is genuinely not executable (would be
    // silently passthrough-ed before this enforcement). Without the gate,
    // a sql cell would produce no output and the user would never know.
    #[test]
    fn test_sql_is_not_executable_language() {
        assert!(
            !is_executable_language("sql"),
            "sql must NOT be in is_executable_language — \
             if this fails, the row-4 loud-failure guard becomes vacuous \
             (sql would execute normally and the Err path is unreachable)"
        );
    }

    // -----------------------------------------------------------------------
    // Cede (§5 enforcement): {python} cell + {sql} cell where sql IS in
    // handled_languages (knitr owns it) → sql is ceded, NOT errored.
    // Only python is returned in the executable set.
    // -----------------------------------------------------------------------
    #[test]
    fn test_partition_cells_cede_when_in_handled_languages() {
        // sql is in handled_languages → jupyter cedes it (knitr owns it).
        let handled: Vec<String> = vec![
            "ojs".to_string(),
            "mermaid".to_string(),
            "dot".to_string(),
            "sql".to_string(),
        ];

        let python_block = make_block("python", "x = 1");
        let sql_block = make_block("sql", "SELECT 1;");
        // multi_engine doesn't matter here (cede path, not the loud-error path).
        let result = partition(vec![python_block, sql_block], &handled, false);

        assert!(result.is_ok(), "should not error — sql is ceded, not owned");
        let executable = result.unwrap();

        // Only the python cell is executable.
        assert_eq!(executable.len(), 1, "only python should be executable");
        assert_eq!(
            executable[0].lang, "python",
            "the executable cell must be python"
        );

        // sql must NOT appear in the executable set (vacuity: list is non-empty).
        assert!(
            !executable.iter().any(|b| b.lang == "sql"),
            "sql must be ceded (not in executable set)"
        );
    }

    // -----------------------------------------------------------------------
    // Default passthrough regression: {ojs} cell with default handled_languages
    // (ojs/mermaid/dot) → ceded, no error, not in executable set.
    //
    // Regression guard: ensures the HANDLED_LANGUAGES default never triggers
    // the loud failure for cell-handler languages.
    // -----------------------------------------------------------------------
    #[test]
    fn test_partition_cells_ojs_default_passthrough() {
        // Use the real HANDLED_LANGUAGES default (ojs/mermaid/dot).
        let handled: Vec<String> = HANDLED_LANGUAGES.iter().map(|s| s.to_string()).collect();

        let ojs_block = make_block("ojs", "viewof slider = Inputs.range([0, 100])");
        // multi_engine doesn't matter here (cede path, not the loud-error path).
        let result = partition(vec![ojs_block], &handled, false);

        assert!(
            result.is_ok(),
            "ojs with default handled_languages must not error"
        );
        let executable = result.unwrap();
        assert!(
            executable.is_empty(),
            "ojs must be ceded (executable set must be empty)"
        );
    }

    // mermaid and dot follow the same path.
    #[test]
    fn test_partition_cells_mermaid_dot_default_passthrough() {
        let handled: Vec<String> = HANDLED_LANGUAGES.iter().map(|s| s.to_string()).collect();

        // multi_engine doesn't matter here (cede path, not the loud-error path).
        let result = partition(
            vec![
                make_block("mermaid", "graph TD; A-->B"),
                make_block("dot", "digraph G { a -> b }"),
            ],
            &handled,
            false,
        );

        assert!(result.is_ok(), "mermaid/dot must not error");
        assert!(result.unwrap().is_empty(), "mermaid and dot must be ceded");
    }

    // -----------------------------------------------------------------------
    // Vacuity / path-exercised check for row 4:
    //
    // Prove that "sql ∉ jupyter's handled_languages" is exactly what the real
    // resolver produces for an explicit [knitr, jupyter] doc with {r}+{sql}.
    // This pins the ownership routing that the row-4 test relies on.
    //
    // Gated cfg(not(wasm32)) because KnitrEngine and JupyterEngine are native-only.
    // -----------------------------------------------------------------------
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn test_vacuity_sql_owned_by_jupyter_in_explicit_knitr_jupyter() {
        use std::sync::Arc;

        use hashlink::LinkedHashMap;
        use quarto_pandoc_types::attr::AttrSourceInfo;
        use quarto_pandoc_types::block::CodeBlock as AstCodeBlock;
        use quarto_pandoc_types::{Block, Pandoc};
        use quarto_source_map::SourceInfo;

        use crate::engine::jupyter::JupyterEngine;
        use crate::engine::knitr::KnitrEngine;
        use crate::engine::registry::EngineRegistry;
        use crate::engine::resolution::resolve_engines;

        // Build a registry with real knitr + jupyter engines.
        let mut registry = EngineRegistry::empty();
        registry.register(Arc::new(KnitrEngine::new()));
        registry.register(Arc::new(JupyterEngine::new()));

        // Metadata: explicit [knitr, jupyter].
        let si = SourceInfo::for_test();
        let knitr_cv = quarto_pandoc_types::ConfigValue::new_string("knitr", si.clone());
        let jupyter_cv = quarto_pandoc_types::ConfigValue::new_string("jupyter", si.clone());
        let engine_array =
            quarto_pandoc_types::ConfigValue::new_array(vec![knitr_cv, jupyter_cv], si.clone());
        let meta = quarto_pandoc_types::ConfigValue::new_map(
            vec![quarto_pandoc_types::config_value::ConfigMapEntry {
                key: "engine".to_string(),
                key_source: si.clone(),
                value: engine_array,
            }],
            si.clone(),
        );

        // AST: one {r} cell and one {sql} cell.
        let make_cell = |lang: &str| {
            Block::CodeBlock(AstCodeBlock {
                attr: (
                    String::new(),
                    vec![format!("{{{lang}}}")],
                    LinkedHashMap::new(),
                ),
                text: format!("-- {lang} code"),
                source_info: si.clone(),
                attr_source: AttrSourceInfo::empty(),
            })
        };
        let ast = Pandoc {
            meta: meta.clone(),
            blocks: vec![make_cell("r"), make_cell("sql")],
        };

        let resolution = resolve_engines(&meta, &ast, &registry, None);

        // sql must be owned by jupyter (T2 explicit Fallback > knitr's T3 Interop).
        assert_eq!(
            resolution.ownership.get("sql").map(|s| s.as_str()),
            Some("jupyter"),
            "explicit [knitr, jupyter] + {{sql}} → sql owned by jupyter via T2"
        );

        // handled_languages_for("jupyter") must NOT contain "sql"
        // (sql is jupyter's own language, not something it cedes).
        let jupyter_handled = resolution.handled_languages_for("jupyter");
        assert!(
            !jupyter_handled.contains(&"sql".to_string()),
            "sql must NOT be in jupyter's handled_languages — \
             it is owned by jupyter, so jupyter should (try to) execute it, \
             not cede it. This is precisely the condition the row-4 loud \
             failure fires on."
        );

        // Confirm: r is in jupyter's handled_languages (r is owned by knitr).
        assert!(
            jupyter_handled.contains(&"r".to_string()),
            "r (owned by knitr) must be in jupyter's handled_languages"
        );
    }

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

    // ── EXR-2 Phase 0: characterization of `parse_code_blocks` ──────────────
    //
    // These pin TODAY's behavior, quirks included, so the unit scanner
    // (Phase 2) and the caller migration (Phase 4) can reproduce or
    // deliberately change it. Where a case differs from q1 the comment says
    // so; the strands are listed in the EXR-2 plan's Risks section.

    fn langs(input: &str) -> Vec<String> {
        parse_code_blocks(input)
            .into_iter()
            .map(|b| b.language)
            .collect()
    }

    #[test]
    fn exr2_p0_bare_fence_offsets() {
        let input = "```{python}\nx=1\n```\n";
        let blocks = parse_code_blocks(input);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].language, "python");
        assert_eq!(blocks[0].code, "x=1\n");
        assert_eq!(blocks[0].start, 0);
        assert_eq!(blocks[0].code_start, "```{python}\n".len());
        // `end` stops after the closing fence's trailing whitespace; the
        // final `\n` is whitespace the closer's `\s*` consumes.
        assert_eq!(&input[blocks[0].end..], "");
    }

    #[test]
    fn exr2_p0_trailing_spaces_after_opener() {
        assert_eq!(langs("```{python}  \nx=1\n```\n"), ["python"]);
    }

    #[test]
    fn exr2_p0_four_backtick_fence_skipped() {
        assert!(langs("````{python}\nx=1\n````\n").is_empty());
    }

    #[test]
    fn exr2_p0_indented_fence_skipped() {
        assert!(langs("  ```{python}\n  x=1\n  ```\n").is_empty());
    }

    #[test]
    fn exr2_p0_raw_format_and_attribute_forms() {
        // Raw-format fence, class fence: never cells.
        assert!(langs("```{=html}\n<b>x</b>\n```\n").is_empty());
        assert!(langs("```{.python}\nx=1\n```\n").is_empty());
        // Knitr-style options inside the braces: not matched (pampa: Q-2-36).
        assert!(langs("```{python echo=false}\nx=1\n```\n").is_empty());
        // Attribute tail (a space inside the braces): still not matched. q1
        // matches it (bd-rh4cce9e).
        assert!(langs("```{python .marimo}\nx=1\n```\n").is_empty());
        // Dotted and hyphenated tokens: matched since EXR-2 Phase 1 (the
        // token class is pampa's; before, `\w` excluded `.` and `-`).
        assert_eq!(langs("```{python.marimo}\nx=1\n```\n"), ["python.marimo"]);
        assert_eq!(langs("```{py-thon}\nx=1\n```\n"), ["py-thon"]);
    }

    #[test]
    fn exr2_p0_language_case_and_leading_digit() {
        // Case is preserved (q1 lowercases; bd-7gvl455a).
        assert_eq!(langs("```{Python}\nx=1\n```\n"), ["Python"]);
        // A leading digit is rejected, as pampa does (Q-2-36). Before EXR-2
        // Phase 1 the executor's `\w` accepted `{1abc}`.
        assert!(langs("```{1abc}\nx=1\n```\n").is_empty());
        assert!(langs("```{_x}\nx=1\n```\n").is_empty());
    }

    // (a) The match end swallows blank lines after the closing fence.
    #[test]
    fn exr2_p0_quirk_a_end_swallows_blank_lines() {
        let input = "```{python}\nx=1\n```\n\n\nText\n";
        let blocks = parse_code_blocks(input);
        assert_eq!(blocks.len(), 1);
        assert_eq!(&input[blocks[0].end..], "\nText\n");
    }

    // (b) `\s*` between the fence and the brace spans newlines.
    #[test]
    fn exr2_p0_quirk_b_newline_between_fence_and_brace() {
        let input = "```\n{python}\nx=1\n```\n";
        let blocks = parse_code_blocks(input);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].language, "python");
        assert_eq!(blocks[0].code, "x=1\n");
        assert_eq!(blocks[0].start, 0);
    }

    // (c) CRLF: the `\r` is consumed as whitespace or left in the code.
    #[test]
    fn exr2_p0_quirk_c_crlf() {
        let input = "```{python}\r\nx=1\r\n```\r\nText\r\n";
        let blocks = parse_code_blocks(input);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].code, "x=1\r\n");
        // The match ends before the `\n` that follows the closer's `\r`.
        assert_eq!(&input[blocks[0].end..], "\nText\r\n");
    }

    // (d) The token is ASCII since EXR-2 Phase 1: `{é}` no longer runs here
    // (pampa rejects it too). Before, the Unicode `\w` matched it.
    #[test]
    fn exr2_p0_quirk_d_unicode_language() {
        assert!(langs("```{é}\nx=1\n```\n").is_empty());
    }

    // EXR-2 Phase 1 consequence of the wider token class: a `{python.marimo}`
    // cell now reaches `partition_cells`, finds no executable language, and in
    // a multi-engine document fails loudly (before, the executor never saw it).
    #[test]
    fn exr2_p1_dotted_cell_reaches_partition_and_fails_in_multi_engine() {
        let input = "```{python.marimo}\nx=1\n```\n";
        assert_eq!(parse_code_blocks(input).len(), 1);
        let multi = partition_text(input, &[], true);
        assert!(
            matches!(multi, Err(ExecutionError::NoHandlerForLanguage { .. })),
            "got {multi:?}"
        );
        let single = partition_text(input, &[], false).expect("single-engine passes through");
        assert!(single.is_empty());
    }

    // (e) Nesting is invisible to the line-start regex; see also the writer
    // round-trip test below.
    #[test]
    fn exr2_p0_quirk_e_indent_and_nesting_skipped() {
        assert!(langs("- item\n\n  ```{python}\n  x=1\n  ```\n").is_empty());
        assert!(langs("> ```{python}\n> x=1\n> ```\n").is_empty());
    }

    // (f) Blank lines after the opener are swallowed, shifting `code_start`.
    #[test]
    fn exr2_p0_quirk_f_leading_blank_lines_swallowed() {
        let input = "```{python}\n\n\nx=1\n```\n";
        let blocks = parse_code_blocks(input);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].code, "x=1\n");
        assert_eq!(blocks[0].code_start, "```{python}\n\n\n".len());
    }

    // (g) An unterminated cell runs to the next bare closer and swallows the
    // next cell's opener.
    #[test]
    fn exr2_p0_quirk_g_unterminated_swallows_next_opener() {
        let input = "```{python}\nx=1\n```{r}\ny=2\n```\n";
        let blocks = parse_code_blocks(input);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].language, "python");
        assert_eq!(blocks[0].code, "x=1\n```{r}\ny=2\n");
    }

    // (h) The closer is exactly three backticks plus whitespace.
    #[test]
    fn exr2_p0_quirk_h_closer_shape() {
        assert!(langs("```{python}\nx=1\n```  trailing\n").is_empty());
        assert!(langs("```{python}\nx=1\n````\n").is_empty());
        assert_eq!(langs("```{python}\nx=1\n```   \n"), ["python"]);
    }

    // (i) A column-0 opener inside an unbraced, longer fence executes; only
    // the nested-cell mask prevents it (T14 covers the masked case).
    #[test]
    fn exr2_p0_quirk_i_opener_inside_longer_fence_runs_unmasked() {
        let input = "````markdown\n```{python}\nx=1\n```\n````\n";
        assert_eq!(langs(input), ["python"]);
    }

    // (j) A space between the backticks and the brace is accepted.
    #[test]
    fn exr2_p0_quirk_j_space_before_brace() {
        assert_eq!(langs("``` {python}\nx=1\n```\n"), ["python"]);
    }

    // The writer indents list-nested fences and prefixes blockquote cells with
    // `> ` (so the executor skips them); div-nested cells are not indented (so
    // it runs them). This is the root of bd-9j6z311f.
    #[test]
    fn exr2_p0_writer_roundtrip_nesting() {
        fn roundtrip(qmd: &str) -> String {
            let mut sink = Vec::<u8>::new();
            let (doc, _ctx, _warnings) =
                pampa::readers::qmd::read(qmd.as_bytes(), false, "t.qmd", &mut sink, true, None)
                    .expect("parse fixture");
            let mut buf = Vec::new();
            pampa::writers::qmd::write(&doc, &mut buf).expect("write fixture");
            String::from_utf8(buf).expect("utf8")
        }

        let list = roundtrip("- item\n\n  ```{python}\n  x=1\n  ```\n");
        assert!(
            list.contains("  ```{python}"),
            "writer must indent list-nested fences; got:\n{list}"
        );
        assert!(parse_code_blocks(&list).is_empty(), "got:\n{list}");

        let quote = roundtrip("> ```{python}\n> x=1\n> ```\n");
        assert!(
            quote.contains("> ```{python}"),
            "writer must prefix blockquote cells; got:\n{quote}"
        );
        assert!(parse_code_blocks(&quote).is_empty(), "got:\n{quote}");

        let div = roundtrip("::: {.callout-note}\n```{python}\nx=1\n```\n:::\n");
        assert!(
            div.contains("\n```{python}") || div.starts_with("```{python}"),
            "div-nested fences must not be indented; got:\n{div}"
        );
        assert_eq!(parse_code_blocks(&div).len(), 1, "got:\n{div}");
    }

    // ── EXR-2 Phase 2: the unit scanner agrees with the legacy executor ─────

    /// Cells the scanner marks `runs_in_legacy_executor` must match
    /// `parse_code_blocks` byte for byte, quirks included, so Phase 4 can
    /// swap the executor onto the scanner without moving a single splice.
    fn legacy_cells_from_scanner(input: &str) -> Vec<(usize, usize, usize, String, String)> {
        use crate::engine::units::{Unit, runs_in_legacy_executor, scan_units};
        scan_units(input)
            .into_iter()
            .filter_map(|u| match u {
                Unit::Cell(c) if runs_in_legacy_executor(&c) => {
                    Some((c.start, c.end, c.code_start, c.lang, c.code))
                }
                _ => None,
            })
            .collect()
    }

    fn legacy_cells_from_executor(input: &str) -> Vec<(usize, usize, usize, String, String)> {
        parse_code_blocks(input)
            .into_iter()
            .map(|b| (b.start, b.end, b.code_start, b.language, b.code))
            .collect()
    }

    #[test]
    fn exr2_p2_scanner_matches_the_executor_on_the_pinned_inputs() {
        let corpus = [
            "```{python}\nx=1\n```\n",
            "```{python}  \nx=1\n```\n",
            "``` {python}\nx=1\n```\n",
            "```{python.marimo}\nx=1\n```\n",
            "```{py-thon}\nx=1\n```\n",
            "```{Python}\nx=1\n```\n",
            // (a) blank lines after the closer are swallowed
            "```{python}\nx=1\n```\n\n\nText\n",
            // (c) CRLF
            "```{python}\r\nx=1\r\n```\r\nText\r\n",
            // (f) blank lines after the opener are swallowed
            "```{python}\n\n\nx=1\n```\n",
            // (g) an unterminated first cell swallows the next opener
            "```{python}\nx=1\n```{r}\ny=2\n```\n",
            // (h) closer shape
            "```{python}\nx=1\n```   \n",
            "```{python}\nx=1\n```  trailing\n",
            // two cells and prose
            "---\ntitle: t\n---\n\nText.\n\n```{python}\nx=1\n```\n\nMore.\n\n```{r}\ny=2\n```\n",
            // empty body
            "```{python}\n```\n",
            // not cells for either
            "````{python}\nx=1\n````\n",
            "  ```{python}\n  x=1\n  ```\n",
            "```{=html}\nx\n```\n",
            "```{python .marimo}\nx=1\n```\n",
            "```{1abc}\nx=1\n```\n",
        ];
        for input in corpus {
            assert_eq!(
                legacy_cells_from_scanner(input),
                legacy_cells_from_executor(input),
                "scanner and executor disagree on {input:?}"
            );
        }
    }

    // The one place the scanner still differs from the old cell finder on a
    // column-0 three-backtick cell, and it is not reachable in the pipeline for
    // a code block: (i) a column-0 opener inside a longer unbraced fence. The
    // old finder runs it unless the mask rewrote it; the scanner treats it as
    // the longer fence's content. (Quirks (b) and (h) were the other two; Phase
    // 4 fixed them in the executor, see the `exr2_p4_quirk_*` tests.)
    #[test]
    fn exr2_p2_known_divergences_from_the_executor() {
        let i = "````markdown\n```{python}\nx=1\n```\n````\n";
        assert_eq!(legacy_cells_from_executor(i).len(), 1);
        assert!(legacy_cells_from_scanner(i).is_empty());
    }

    // ── EXR-2 Phase 4: quirks (b) and (h) are fixed, not emulated ───────────
    // (epic decision 26, amended by Gordon 2026-10-31). Asserted through the
    // executor's own classification (`partition_text`: scan, view, partition),
    // and against the old cell finder to show what changed.

    // (b) A closing fence followed by a `{python}` line is not a cell. The old
    // regex let `\s*` span the newline and started a cell at the closer.
    #[test]
    fn exr2_p4_quirk_b_closer_then_brace_line_is_not_a_cell() {
        let input = "```text\nhello\n```\n{python}\nx = 1\n```\n";
        assert_eq!(
            parse_code_blocks(input).len(),
            1,
            "the old finder misread the closer as an opener"
        );
        let run = partition_text(input, &[], false).unwrap();
        assert!(run.is_empty(), "no cell runs: {run:?}");
        // And the plain form the Phase 0 pin used.
        assert!(
            partition_text("```\n{python}\nx=1\n```\n", &[], false)
                .unwrap()
                .is_empty()
        );
    }

    // (h) A four-backtick line closes a three-backtick cell (CommonMark: a
    // closing fence is at least as long as the opener).
    #[test]
    fn exr2_p4_quirk_h_longer_closer_closes_the_cell() {
        let input = "```{python}\nx=1\n````\n\nText\n";
        assert!(
            parse_code_blocks(input).is_empty(),
            "the old finder needed exactly three"
        );
        let run = partition_text(input, &[], false).unwrap();
        assert_eq!(run.len(), 1, "{run:?}");
        assert_eq!(run[0].lang, "python");
        assert_eq!(run[0].code, "x=1\n");
        // The trailing-text closer is still not a closer for either.
        assert!(
            partition_text("```{python}\nx=1\n```  trailing\n", &[], false)
                .unwrap()
                .is_empty()
        );
    }

    // The Phase 3 corpus: the cells the scanner hands the executor equal what
    // the old finder found, except where the old finder was wrong or
    // unreachable. Each exception is named with its reason.
    #[test]
    fn exr2_p4_scanner_filtered_by_the_legacy_rule_equals_the_old_finder_on_the_corpus() {
        const EXCEPTIONS: &[(&str, &str)] = &[(
            "cell_inside_display_fence",
            "quirk (i): the old finder runs an opener shown inside a longer fence; \
                 the mask hides it in the pipeline, the scanner never claims it",
        )];
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/units");
        let mut checked = 0;
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_none_or(|e| e != "qmd") {
                continue;
            }
            let name = path.file_stem().unwrap().to_string_lossy().to_string();
            let text = std::fs::read_to_string(&path).unwrap();
            if let Some((_, why)) = EXCEPTIONS.iter().find(|(n, _)| *n == name) {
                assert_ne!(
                    legacy_cells_from_scanner(&text),
                    legacy_cells_from_executor(&text),
                    "{name} is listed as an exception ({why}) but now agrees: drop it"
                );
                continue;
            }
            assert_eq!(
                legacy_cells_from_scanner(&text),
                legacy_cells_from_executor(&text),
                "{name}: scanner and old finder disagree"
            );
            checked += 1;
        }
        assert!(checked >= 20, "corpus went missing: {checked}");
    }

    // `partition_cells` cedes case-insensitively: a `{Python}` cell with
    // handled = ["python"] is ceded (today's tests never pin this).
    #[test]
    fn exr2_p0_partition_cedes_case_insensitively() {
        let handled = vec!["python".to_string()];
        let result = partition(vec![make_block("Python", "x = 1")], &handled, false)
            .expect("ceded cells are not an error");
        assert!(result.is_empty(), "{{Python}} must be ceded by \"python\"");
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
        assert!(is_executable_language("ggsql"));
        assert!(is_executable_language("GGSQL"));
        assert!(!is_executable_language("json"));
        assert!(!is_executable_language("markdown"));
    }
}

/// EXR-3 Phase 2: the span loop, driven by the fake kernel through the real session path
/// (`execute_qmd` -> daemon -> `TestKernel`), plus the kernel-free passthrough cases.
#[cfg(test)]
mod span_tests {
    use super::*;
    use crate::engine::jupyter::fake_kernel::{FakeKernel, FakeKernelHome, FakeOutput, Script};
    use quarto_source_map::SourceContext;
    use serde_json::json;
    use std::sync::Arc;

    fn py(expr: &str) -> String {
        wrap_inline_code("python", expr)
    }

    fn markdown(text: &str) -> FakeOutput {
        FakeOutput::Result(json!({ "text/markdown": text, "text/plain": "ignored" }))
    }

    fn jupyter_meta(name: &str) -> std::collections::HashMap<String, TsMetadataValue> {
        std::collections::HashMap::from([(
            "jupyter".to_string(),
            TsMetadataValue::String(name.to_string()),
        )])
    }

    /// A context whose source map holds `text` as `doc.qmd`, so locations resolve.
    fn ctx_for(text: &str) -> ExecutionContext {
        let mut sc = SourceContext::new();
        let id = sc.add_file("doc.qmd".to_string(), Some(text.to_string()));
        ExecutionContext::new(
            std::env::temp_dir(),
            std::env::temp_dir(),
            std::path::PathBuf::from("doc.qmd"),
            "html",
        )
        .with_source_info(SourceInfo::original(id, 0, text.len()), Arc::new(sc))
    }

    /// A document context with a fake python kernel installed and named by `jupyter:`.
    fn with_kernel(text: &str, script: &Script) -> (FakeKernelHome, ExecutionContext) {
        let home = FakeKernelHome::new();
        let FakeKernel { name, env } = home.install("fake-py", "python", script);
        let ctx = ctx_for(text)
            .with_metadata(jupyter_meta(&name))
            .with_owned_languages(vec!["python".to_string()])
            .with_project_env(env);
        (home, ctx)
    }

    fn run(
        text: &str,
        script: &Script,
    ) -> (String, Vec<quarto_error_reporting::DiagnosticMessage>) {
        let (_home, ctx) = with_kernel(text, script);
        let r = execute_qmd(text, &ctx).expect("execution succeeds");
        (r.markdown, r.warnings)
    }

    fn run_err(text: &str, script: &Script) -> String {
        let (_home, ctx) = with_kernel(text, script);
        execute_qmd(text, &ctx)
            .expect_err("execution fails")
            .to_string()
    }

    // ── values ───────────────────────────────────────────────────────────

    #[test]
    fn span_value_replaces_the_span_in_prose() {
        let script = Script::from([(py("1 + 1"), vec![FakeOutput::plain_result("2")])]);
        let (out, warnings) = run("A `{python} 1 + 1` B\n", &script);
        assert_eq!(out, "A 2 B\n");
        assert!(warnings.is_empty());
    }

    #[test]
    fn markdown_value_is_pasted_as_is_and_quoted_text_is_escaped() {
        let script = Script::from([
            (py("m"), vec![markdown("**b**")]),
            (py("s"), vec![FakeOutput::plain_result("'hello_world'")]),
        ]);
        let (out, _) = run("`{python} m` and `{python} s`\n", &script);
        assert_eq!(out, "**b** and hello\\_world\n");
    }

    #[test]
    fn a_none_value_renders_as_text() {
        // On a python kernel the wrapper turns `None` into the string 'None'.
        let script = Script::from([(py("n"), vec![FakeOutput::plain_result("'None'")])]);
        let (out, _) = run("v=`{python} n`\n", &script);
        assert_eq!(out, "v=None\n");
    }

    #[test]
    fn empty_string_value_leaves_the_span_source() {
        let script = Script::from([(py("e"), vec![FakeOutput::plain_result("''")])]);
        let (out, warnings) = run("a `{python} e` b\n", &script);
        assert_eq!(out, "a `{python} e` b\n");
        assert!(warnings.is_empty());
    }

    #[test]
    fn no_usable_result_warns_and_leaves_the_span_source() {
        let script = Script::from([(
            py("img"),
            vec![FakeOutput::Result(json!({ "image/png": "AAAA" }))],
        )]);
        let (out, warnings) = run("a `{python} img` b\n", &script);
        assert_eq!(out, "a `{python} img` b\n");
        assert_eq!(warnings.len(), 1);
        let text = warnings[0].to_text(None);
        assert!(text.contains("doc.qmd:1:"), "{text}");
    }

    #[test]
    fn several_spans_run_in_document_order_without_block_padding() {
        let script = Script::from([
            (py("a"), vec![FakeOutput::plain_result("1")]),
            (py("b"), vec![FakeOutput::plain_result("2")]),
        ]);
        let (out, _) = run("x `{python} a`\n`{python} b` y\n", &script);
        assert_eq!(out, "x 1\n2 y\n");
    }

    #[test]
    fn spans_and_cells_share_one_pass() {
        let script = Script::from([
            (py("a"), vec![FakeOutput::plain_result("1")]),
            ("x = 1".to_string(), vec![]),
        ]);
        let (out, _) = run("```{python}\nx = 1\n```\n\nv `{python} a`\n", &script);
        assert!(out.contains("::: {.cell"), "{out}");
        assert!(out.ends_with("v 1\n"), "{out}");
    }

    #[test]
    fn supplied_units_with_ids_are_used_as_they_are() {
        use crate::engine::units::{UnitId, scan_units};
        let text = "A `{python} 1 + 1` B\n";
        let script = Script::from([(py("1 + 1"), vec![FakeOutput::plain_result("2")])]);
        let (_home, mut ctx) = with_kernel(text, &script);
        ctx.units = scan_units(text)
            .into_iter()
            .map(|u| match u {
                Unit::Inline(mut i) => {
                    i.id = Some(UnitId("stub".to_string()));
                    Unit::Inline(i)
                }
                other => other,
            })
            .collect();
        assert_eq!(execute_qmd(text, &ctx).unwrap().markdown, "A 2 B\n");
    }

    // ── errors and locations ─────────────────────────────────────────────

    #[test]
    fn a_raising_span_is_fatal_and_names_its_location() {
        let script = Script::from([(
            py("boom"),
            vec![FakeOutput::Error {
                ename: "NameError".to_string(),
                evalue: "name 'boom' is not defined".to_string(),
            }],
        )]);
        let err = run_err("line one\nsee `{python} boom` here\n", &script);
        assert!(err.contains("raised NameError"), "{err}");
        assert!(err.contains("doc.qmd:2:"), "{err}");
    }

    #[test]
    fn a_statement_span_is_fatal() {
        let script = Script::from([(py("x = 1"), vec![])]);
        let err = run_err("`{python} x = 1`\n", &script);
        assert!(err.contains("produced no value"), "{err}");
    }

    // ── front matter ─────────────────────────────────────────────────────

    #[test]
    fn front_matter_span_is_unescaped_before_it_runs() {
        // The writer re-emits the title double-quoted: `f("a")` arrives as `f(\"a\")`.
        let text = "---\ntitle: \"T `{python} f(\\\"a\\\")` end\"\n---\n\nbody\n";
        let script = Script::from([(py("f(\"a\")"), vec![markdown("X")])]);
        let (out, _) = run(text, &script);
        assert_eq!(out, "---\ntitle: \"T X end\"\n---\n\nbody\n");
    }

    #[test]
    fn front_matter_value_is_re_escaped_so_the_yaml_stays_valid() {
        let text = "---\ntitle: \"T `{python} v` end\"\n---\n\nbody\n";
        let script = Script::from([(py("v"), vec![markdown("q\"b\\s*")])]);
        let (out, _) = run(text, &script);
        assert_eq!(out, "---\ntitle: \"T q\\\"b\\\\s* end\"\n---\n\nbody\n");
        let yaml: serde_yaml::Value =
            serde_yaml::from_str(out.split("---\n").nth(1).unwrap()).expect("valid YAML");
        assert_eq!(yaml["title"].as_str(), Some("T q\"b\\s* end"));
    }

    #[test]
    fn front_matter_value_with_a_newline_is_fatal_and_names_key_and_location() {
        let text = "---\ntitle: \"T `{python} v` end\"\n---\n\nbody\n";
        let script = Script::from([(py("v"), vec![markdown("a\nb")])]);
        let err = run_err(text, &script);
        assert!(err.contains("front matter key `title`"), "{err}");
        assert!(err.contains("doc.qmd:2:"), "{err}");
    }

    #[test]
    fn a_failing_front_matter_span_names_the_line_it_is_on() {
        let text = "---\ntitle: \"T `{python} v` end\"\n---\n\nbody\n";
        let script = Script::from([(
            py("v"),
            vec![FakeOutput::Error {
                ename: "NameError".to_string(),
                evalue: "v".to_string(),
            }],
        )]);
        let err = run_err(text, &script);
        assert!(err.contains("doc.qmd:2:"), "{err}");
    }

    /// The mask turns a span in a value the document does not own (`_quarto.yml`,
    /// `_metadata.yml`, a format default) into `` `q2-nested-executable {lang} expr` ``;
    /// the engine must not see a span there. (The masking itself is covered in
    /// `nested_cell_mask`.)
    #[test]
    fn quarto_yml_front_matter_span_stays_literal() {
        let text = "---\ntitle: \"T `q2-nested-executable {python} 1+1` end\"\n---\n\nbody\n";
        let (_home, ctx) = with_kernel(text, &Script::new());
        let r = execute_qmd(text, &ctx).unwrap();
        assert_eq!(r.markdown, text);
    }

    // ── ownership ────────────────────────────────────────────────────────

    #[test]
    fn a_span_only_document_owns_the_selected_kernels_language() {
        let text = "`{python} a` and `{r} b`\n";
        let script = Script::from([(py("a"), vec![FakeOutput::plain_result("1")])]);
        let home = FakeKernelHome::new();
        let kernel = home.install("fake-py", "python", &script);
        // No owned set: ownership comes from the kernel named by `jupyter:`.
        let ctx = ctx_for(text)
            .with_metadata(jupyter_meta(&kernel.name))
            .with_project_env(kernel.env);
        assert_eq!(execute_qmd(text, &ctx).unwrap().markdown, "1 and `{r} b`\n");
    }

    #[test]
    fn spans_of_an_unowned_language_are_untouched_and_start_no_kernel() {
        let text = "x `{r} 1+1` y\n";
        let (_home, mut ctx) = with_kernel(text, &Script::new());
        ctx.owned_languages = Vec::new();
        let r = execute_qmd(text, &ctx).unwrap();
        assert_eq!(r.markdown, text);
    }

    #[test]
    fn jupyter_engine_declared_spans_only_without_kernel_errors() {
        let text = "---\ntitle: t\n---\n\nSee `{python} 1`.\n";
        let ctx = ctx_for(text).with_owned_languages(vec!["python".to_string()]);
        let err = execute_qmd(text, &ctx)
            .expect_err("no kernel named")
            .to_string();
        assert!(err.contains("`jupyter:` kernel"), "{err}");

        // A span of a language nobody here owns counts too: there is no kernel to ask.
        let r_span = "---\ntitle: t\n---\n\nSee `{r} 1`.\n";
        let ctx = ctx_for(r_span);
        assert!(execute_qmd(r_span, &ctx).is_err());

        // With zero spans there is nothing to evaluate: not an error.
        let plain = "---\ntitle: t\n---\n\nNo spans.\n";
        let ctx = ctx_for(plain).with_owned_languages(vec!["python".to_string()]);
        assert_eq!(execute_qmd(plain, &ctx).unwrap().markdown, plain);
    }

    #[test]
    fn spans_in_fences_and_comments_are_left_alone() {
        let text = "```text\n`{python} 1`\n```\n\n<!-- `{python} 2` -->\n\n\
                    <!--\nmulti `{python} 3`\n-->\n\n`    ```{python}\n";
        let ctx = ctx_for(text).with_owned_languages(vec!["python".to_string()]);
        assert_eq!(execute_qmd(text, &ctx).unwrap().markdown, text);
    }
}
