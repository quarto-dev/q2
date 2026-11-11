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
use super::execute::ExecuteOpts;
use super::inline::{
    InlineOutcome, classify_inline_result, span_is_owned, unescape_yaml_double, wrap_inline_code,
};
use super::kernel_select::{
    KernelSpecSummary, choose_kernel, deprecated_alias, explicit_kernel, lookup_language,
};
use super::kernelspec::{UnusableJupyterKey, list_kernelspecs, unusable_jupyter_key};
use super::router::{KernelName, SingleKernelRouter, plan_units};
use super::session::SessionKey;
use crate::cell_options::partition_cell_options;
use crate::engine::context::{ExecuteResult, ExecutionContext};
use crate::engine::diagnostics::{
    InlineSpanProblem, deprecated_alias_diagnostic, no_runnable_kernel_diagnostic, note_diagnostic,
    span_diagnostic,
};
use crate::engine::error::ExecutionError;
use crate::engine::ts_protocol::TsMetadataValue;
use crate::engine::units::{
    CellUnit, EngineUnit, InlineUnit, Unit, runs_in_legacy_executor, units_for_engine,
    view_for_engine,
};
use crate::presenter::cell::describe_location;
use crate::presenter::document::{PresentError, present_document};
use crate::presenter::figures::FsFigureSink;
use crate::presenter::inline::InlineTarget;
use crate::presenter::output::{CellOutput, CellResult, ExecuteStatus};
use crate::presenter::visibility::{resolve_cell_options, resolved_flag};
use crate::record::{
    AuthorTexts, EngineEntryInput, HashDefinition, RecordBuilder, UnitResult, fill_ids,
};

type JupyterResult<T> = std::result::Result<T, JupyterError>;

/// Execute code blocks in QMD input and return markdown with outputs, with the
/// record (when `ctx.want_record`) hashed under the Jupyter engine's own
/// declaration. The engine itself calls [`execute_qmd_with`] with the
/// declaration it returns from `hash_definition()`; this form is for tests.
#[cfg(test)]
pub(super) fn execute_qmd(
    input: &str,
    ctx: &ExecutionContext,
) -> std::result::Result<ExecuteResult, ExecutionError> {
    execute_qmd_with(input, ctx, &super::jupyter_hash_definition())
}

/// Execute code blocks in QMD input and return markdown with outputs.
///
/// This is the main entry point for text-based Jupyter execution. The hash
/// declaration is the engine's (`ExecutionEngine::hash_definition()`; epic
/// decision 35): the record stores the whole declaration and never branches
/// on an engine name.
pub fn execute_qmd_with(
    input: &str,
    ctx: &ExecutionContext,
    hash_definition: &HashDefinition,
) -> std::result::Result<ExecuteResult, ExecutionError> {
    // The document's units, relocated into this text (or scanned from it when
    // no stage supplied a shared list). Every braced cell and every span is in
    // the list; the view marks which cells jupyter owns. A cell is jupyter's
    // iff its language is in `ctx.owned_languages`, the resolver's positive
    // ownership set. Spans are owned by language (EXR-3, below).
    let units = units_for_engine(&ctx.units, input);
    let owned = &ctx.owned_languages;
    let owns = |lang: &str| owned.iter().any(|o| o.eq_ignore_ascii_case(lang));
    let view = view_for_engine(&units, &owns);

    let explicit = explicit_kernel(&ctx.metadata, ctx.engine_config.as_ref());
    // The kernelspec list is only read when there is no explicit kernel to
    // take every owned cell.
    let specs: Vec<KernelSpecSummary> = if explicit.is_none() && has_owned_cell(&view) {
        super::daemon::engine_runtime()
            .block_on(list_kernelspecs())
            .iter()
            .map(KernelSpecSummary::from)
            .collect()
    } else {
        Vec::new()
    };
    let virtual_env = std::env::var("VIRTUAL_ENV").ok();
    let doc = plan_document(
        &view,
        explicit,
        &specs,
        virtual_env.as_deref(),
        ctx.multi_engine,
        &|cell| cell_eval_disabled(cell, ctx),
    )?;
    let mut warnings = doc.warnings;
    let executable = doc.executable;
    let kernel_name = doc.kernel;
    let spans: Vec<InlineUnit> = units
        .iter()
        .filter_map(|u| match u {
            Unit::Inline(i) => Some(i.clone()),
            Unit::Cell(_) => None,
        })
        .collect();
    // A passthrough still carries the report (the unrunnable-language and
    // deprecated-alias warnings), and an empty record when one is wanted.
    let passthrough = |warnings: Vec<quarto_error_reporting::DiagnosticMessage>| {
        let mut result = empty_record_passthrough(input, ctx, hash_definition)?;
        result.warnings.extend(warnings);
        Ok(result)
    };

    // No cells and no spans: passthrough unchanged, no kernel needed.
    if executable.is_empty() && spans.is_empty() {
        return passthrough(warnings);
    }

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
        return passthrough(warnings);
    }
    let Some(kernel_name) = kernel_name else {
        // Owned spans, but only ceded cells: still nothing names a kernel.
        return Err(no_kernel_for_spans());
    };
    // One kernel serves every unit today: the router returns it for all of
    // them. A reopened JOL-3 (several kernels) would vary it per unit.
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
    // Placement matters: this sits *below* the early return above, so a pure
    // passthrough never acquires a kernel scope.
    let _kernel_scope = super::daemon::kernel_scope();

    // Execute via async runtime. `execute_blocks_async` takes the full
    // `ExecutionContext` (main's #412 figure-emission threads it through).
    let result = execute_blocks_async(input, &run_units, &plan, ctx, hash_definition);

    warnings.extend(jupyter_key_warning(&ctx.metadata));
    result
        .map(|mut r| {
            r.warnings.extend(warnings);
            r
        })
        .map_err(|e| ExecutionError::execution_failed("jupyter", e.to_string()))
}

/// The input unchanged, with an empty record when one is wanted: the record is a side
/// product of every run (EXR-5), a run that executes nothing included.
fn empty_record_passthrough(
    input: &str,
    ctx: &ExecutionContext,
    hash_definition: &HashDefinition,
) -> std::result::Result<ExecuteResult, ExecutionError> {
    let mut result = ExecuteResult::new(input);
    if ctx.want_record {
        let builder = RecordBuilder::new(JUPYTER, hash_definition, &ctx.units, &ctx.author_texts)
            .map_err(record_error)?;
        result.record = Some(
            builder
                .finish(entry_input(ctx, None))
                .map_err(record_error)?,
        );
    }
    Ok(result)
}

/// The engine name the record carries.
const JUPYTER: &str = "jupyter";

fn record_error(e: impl std::fmt::Display) -> ExecutionError {
    ExecutionError::execution_failed(
        JUPYTER,
        JupyterError::RecordFailed {
            message: e.to_string(),
        }
        .to_string(),
    )
}

/// What the Jupyter engine contributes to its record entry besides cells and hashes.
/// Side data (includes, filters, preserve, ...) is the stage's to fold, so it is left
/// empty here. `kernel` is the kernelspec the run used, with its language; provenance
/// only, in no hash.
fn entry_input(ctx: &ExecutionContext, kernel: Option<(&str, &str)>) -> EngineEntryInput {
    let mut provenance = std::collections::BTreeMap::new();
    if let Some((name, language)) = kernel {
        provenance.insert(
            "kernelspec".to_string(),
            serde_json::json!({ "name": name, "language": language }),
        );
    }
    EngineEntryInput {
        provenance,
        can_freeze: true,
        owned_languages: ctx.owned_languages.clone(),
        document_metadata: ctx.document_metadata_json(),
        ..Default::default()
    }
}

/// The position of `unit` in the stage's list, by the document-wide counter the
/// scanner and `locate_units` preserve (`index` for a cell, `span_index` for a span).
/// `None` for a unit an earlier engine's output introduced: it is in no document
/// list, so the record (which `coalesce` orders by that list) does not carry it.
fn stage_index(stage_units: &[Unit], unit: &Unit) -> Option<usize> {
    stage_units.iter().position(|s| match (s, unit) {
        (Unit::Cell(a), Unit::Cell(b)) => a.index == b.index,
        (Unit::Inline(a), Unit::Inline(b)) => a.span_index == b.span_index,
        _ => false,
    })
}

/// Record one executed unit: its raw nbformat outputs, the count the kernel reported
/// and the kernel that ran it. Nothing about the source or options comes from here;
/// the builder takes those from the stage's unit and author text.
fn push_unit_result(
    builder: &mut RecordBuilder<'_>,
    stage_units: &[Unit],
    unit: &Unit,
    result: &CellResult,
    kernel: &str,
) -> JupyterResult<()> {
    let Some(index) = stage_index(stage_units, unit) else {
        return Ok(());
    };
    builder
        .push(index, unit_result(result, kernel))
        .map_err(record_failed)
}

/// A unit's kernel result as the record keeps it: raw nbformat outputs, the count the
/// kernel reported and the kernel that ran it.
fn unit_result(result: &CellResult, kernel: &str) -> UnitResult {
    UnitResult {
        outputs: result.nbformat_outputs(),
        execution_count: result.execution_count.map(u64::from),
        kernel: Some(kernel.to_string()),
    }
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
/// (`engine_config.kernel`, then the kernel for the first owned language
/// that has one).
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
            .add_detail(
                "Falling back to the kernel for the first language in the document that has one.",
            )
            .add_hint(hint)
            .build(),
    )
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

/// Owned cells the legacy executor can run, in document order.
fn owned_cells<'a>(view: &'a [EngineUnit<'_>]) -> impl Iterator<Item = &'a CellUnit> {
    view.iter().filter_map(|u| match u.unit {
        Unit::Cell(cell) if u.owned && runs_in_legacy_executor(cell) => Some(cell),
        _ => None,
    })
}

fn has_owned_cell(view: &[EngineUnit<'_>]) -> bool {
    owned_cells(view).next().is_some()
}

/// Whether the cell's resolved `eval` option is `false` (a cell that never
/// needs a kernel). Unparseable options count as enabled; the executor
/// reports them when it reaches the cell.
fn cell_eval_disabled(cell: &CellUnit, ctx: &ExecutionContext) -> bool {
    let body_source = SourceInfo::substring(
        ctx.source_info.clone(),
        cell.code_start,
        cell.code_start + cell.code.len(),
    );
    let Ok(parsed) = partition_cell_options(&cell.lang, &cell.code, body_source) else {
        return false;
    };
    let resolved = resolve_cell_options(ctx.execute_scope.as_ref(), parsed.options);
    !resolved_flag(resolved.as_ref(), "eval", true)
}

/// What jupyter will do with a document's cells.
#[derive(Debug)]
struct DocumentPlan {
    /// The cells to run, in document order.
    executable: Vec<CellUnit>,
    /// The kernel that runs them: the explicit one, else the derived one when
    /// a cell can run; `None` otherwise.
    kernel: Option<String>,
    /// Warnings to attach to the result: the unrunnable-language warning and
    /// the deprecated-alias warnings.
    warnings: Vec<quarto_error_reporting::DiagnosticMessage>,
}

/// Decide which owned cells run, on which kernel, and what to report.
///
/// * A cell not owned by jupyter (ceded, or owned by nobody) is left
///   verbatim, with no diagnostic.
/// * An owned cell runs on the document kernel. With an explicit kernel every
///   owned cell runs. Otherwise a cell whose language has no installed
///   kernelspec is *unrunnable*: it is left verbatim and reported once per
///   document (`Q-18-5`), as an `Err(NoHandlerForLanguage)` when
///   `multi_engine` (an `engine:` list means the author said who runs what)
///   and as a warning otherwise. Cells with `eval: false` are exempt from the
///   report.
///
/// `NoHandlerForLanguage` is a **clean refusal** and does NOT poison the
/// engine instance. See `ExecutionError::NoHandlerForLanguage`. The error
/// happens before any kernel starts.
fn plan_document(
    view: &[EngineUnit<'_>],
    explicit: Option<String>,
    specs: &[KernelSpecSummary],
    virtual_env: Option<&str>,
    multi_engine: bool,
    eval_disabled: &dyn Fn(&CellUnit) -> bool,
) -> Result<DocumentPlan, ExecutionError> {
    let has_explicit = explicit.is_some();
    let cells: Vec<&CellUnit> = owned_cells(view).collect();
    let lookup: Vec<String> = cells.iter().map(|c| lookup_language(&c.lang)).collect();
    let choice = choose_kernel(explicit, &lookup, specs, virtual_env);

    let mut executable = Vec::new();
    let mut reported: Vec<String> = Vec::new();
    for (cell, lang) in cells.into_iter().zip(&lookup) {
        if choice.unrunnable.contains(lang) {
            if !eval_disabled(cell) && !reported.contains(lang) {
                reported.push(lang.clone());
            }
        } else {
            executable.push(cell.clone());
        }
    }

    if multi_engine && let Some(first) = reported.first() {
        return Err(ExecutionError::no_handler_for_language(
            "jupyter",
            first.clone(),
        ));
    }

    let mut warnings = Vec::new();
    if !reported.is_empty() {
        warnings.push(no_runnable_kernel_diagnostic("jupyter", &reported, false));
    }
    let mut aliases: Vec<_> = Vec::new();
    for cell in &executable {
        if let Some(alias) = deprecated_alias(&cell.lang)
            && !aliases.contains(&alias)
        {
            aliases.push(alias);
        }
    }
    warnings.extend(
        aliases
            .into_iter()
            .map(|a| deprecated_alias_diagnostic(a.alias, a.canonical)),
    );

    Ok(DocumentPlan {
        // An explicit kernel stands without cells (spans can use it); a derived
        // one needs a cell it can run.
        kernel: choice
            .kernel
            .filter(|_| has_explicit || !executable.is_empty()),
        executable,
        warnings,
    })
}

/// Execute code blocks asynchronously and build output markdown.
fn execute_blocks_async(
    input: &str,
    units: &[Unit],
    plan: &[(usize, KernelName)],
    ctx: &ExecutionContext,
    hash_definition: &HashDefinition,
) -> JupyterResult<ExecuteResult> {
    // Drive the execution on the shared engine runtime — NOT a
    // per-call runtime. Kernel sessions (ZeroMQ sockets, the kernel
    // Child) are tokio resources bound to the runtime that created
    // them; a per-call runtime made every cross-call session reuse
    // fail with "Tokio context ... is being shutdown" (bd-hxhnnlzs).
    super::daemon::engine_runtime().block_on(execute_blocks_inner(
        input,
        units,
        plan,
        ctx,
        hash_definition,
    ))
}

/// Inner async function that does the actual execution.
///
/// EXR-6 (the flip): the kernel results of every unit go into a record, and the
/// markdown is `present_document` over that record and the input, so the live path
/// and a replayed record present through the one driver. Execution semantics stay
/// here: the `error:` abort policy for cells, and a span that raised or produced no
/// value, fail the render before anything is presented.
async fn execute_blocks_inner(
    input: &str,
    units: &[Unit],
    plan: &[(usize, KernelName)],
    ctx: &ExecutionContext,
    hash_definition: &HashDefinition,
) -> JupyterResult<ExecuteResult> {
    let daemon = daemon();

    // EXR-5: the exported execution record, built over the stage's unit list and
    // author texts (epic decisions 36 and 38) only when the stage asked for one; a
    // run driven with no stage in front of it has neither, and a record cannot be
    // built from a local scan.
    let mut record = if ctx.want_record {
        if ctx.units.is_empty() {
            return Err(JupyterError::RecordFailed {
                message: "the stage's unit list is empty; a record needs \
                          `ExecutionContext.units` with ids filled and `author_texts`"
                    .to_string(),
            });
        }
        Some(
            RecordBuilder::new(JUPYTER, hash_definition, &ctx.units, &ctx.author_texts)
                .map_err(record_failed)?,
        )
    } else {
        None
    };

    // EXR-6: what this run presents from. Every executed unit goes in, including one
    // an earlier engine's output introduced (it has no stage index, so the exported
    // record above cannot carry it). Its ids are local to this run, filled from the
    // units' scanned text, and `present_document` matches on them.
    let mut presented_units = units.to_vec();
    let presented_texts = AuthorTexts::from_scanned(&presented_units);
    fill_ids(&mut presented_units, &presented_texts).map_err(record_failed)?;
    let mut presentation =
        RecordBuilder::new(JUPYTER, hash_definition, &presented_units, &presented_texts)
            .map_err(record_failed)?;

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

    // GH #1131: the per-document execute environment. Kernels are shared by
    // every document of a project, so these values cannot be given when the
    // kernel starts; a silent request sets them inside the kernel before the
    // document's first cell (Quarto 1 sets `QUARTO_FIG_*` the same way). It
    // changes only the kernel's environment, never q2's.
    if !ctx.execute_env.is_empty() {
        for (kernel, key) in &sessions {
            let language = &languages[kernel];
            let Some(code) =
                crate::engine::execute_info::jupyter_setup_code(language, &ctx.execute_env)
            else {
                tracing::debug!(
                    kernel = %kernel.as_str(),
                    language = %language,
                    "no execute-environment setup for this kernel language"
                );
                continue;
            };
            let result = daemon
                .execute_in_session_with(key, &code, ExecuteOpts::SETUP)
                .await
                .ok_or(JupyterError::NotConnected)??;
            if let ExecuteStatus::Error { ename, evalue, .. } = &result.status {
                tracing::warn!(
                    kernel = %kernel.as_str(),
                    "setting the execute environment in the kernel failed: {ename}: {evalue}"
                );
            }
        }
    }

    for (position, ((_, kernel), unit)) in plan.iter().zip(units).enumerate() {
        let key = &sessions[kernel];
        let result = match unit {
            Unit::Inline(span) => {
                let result = run_span(&daemon, key, &languages[kernel], span).await?;
                span_fatal(&result, span, ctx)?;
                result
            }
            Unit::Cell(block) => {
                // Partition the cell into `#|` options + code (bd-ohvl879u):
                // the kernel runs the code alone (Q1 strips the option lines
                // too, so cell magics work), and the `error:` policy reads the
                // options. `ctx.source_info` maps input offsets back to the
                // original files, so diagnostics resolve to real source
                // positions.
                let body_source = SourceInfo::substring(
                    ctx.source_info.clone(),
                    block.code_start,
                    block.code_start + block.code.len(),
                );
                let cell = partition_cell_options(&block.lang, &block.code, body_source.clone())
                    .map_err(|e| {
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
                exec_result
            }
        };

        // Both records keep every executed unit, hidden ones included, with its raw
        // outputs; visibility is resolved at present time.
        if let Some(builder) = record.as_mut() {
            push_unit_result(builder, &ctx.units, unit, &result, kernel.as_str())?;
        }
        presentation
            .push(position, unit_result(&result, kernel.as_str()))
            .map_err(record_failed)?;
    }

    // Present: the units' visible parts in the Quarto-canonical `::: {.cell}` shape,
    // spans' values in place, figures written under `<stem>_files/figure-html/` next
    // to the source document (bd-5t6wvu7m).
    let cells = presentation
        .finish(EngineEntryInput::default())
        .map_err(record_failed)?
        .cells;
    let mut fig = FsFigureSink::new(&ctx.source_path);
    let presented = present_document(
        &cells,
        input,
        &presented_units,
        doc_scope.as_ref(),
        InlineTarget::from_format(&ctx.format),
        &ctx.format,
        None,
        &mut fig,
    )
    .map_err(|e| present_error(e, &presented_units, ctx))?;

    // Report the `<stem>_files` dir when figures were written —
    // dir-level, mirroring knitr. `q2 render` copies it to the output
    // dir (bd-o8pr) and the preview capture embeds it (bd-qbhp2cvv).
    let mut result = ExecuteResult::new(presented.markdown);
    result.warnings.extend(
        presented
            .notes
            .iter()
            .map(|note| note_diagnostic(note, &presented_units, ctx)),
    );
    if !presented.files_written.is_empty() {
        result = result.with_supporting_files(presented.files_written);
    }
    if let Some(builder) = record {
        // One kernel runs every unit today (JOL-3, deferred, could vary it per
        // unit; each cell already names its own).
        let kernel = plan
            .first()
            .map(|(_, k)| (k.as_str(), languages[k].as_str()));
        result.record = Some(
            builder
                .finish(entry_input(ctx, kernel))
                .map_err(record_failed)?,
        );
    }
    Ok(result)
}

fn record_failed(e: impl std::fmt::Display) -> JupyterError {
    JupyterError::RecordFailed {
        message: e.to_string(),
    }
}

/// Run one inline expression on the kernel and return its raw result.
///
/// The writer escaped a front matter value as a YAML double-quoted string; the kernel
/// must see the expression as the author wrote it.
async fn run_span(
    daemon: &super::daemon::JupyterDaemon,
    key: &SessionKey,
    kernel_language: &str,
    span: &InlineUnit,
) -> JupyterResult<CellResult> {
    let expr = if span.in_front_matter {
        unescape_yaml_double(&span.code)
    } else {
        span.code.clone()
    };
    daemon
        .execute_in_session_with(
            key,
            &wrap_inline_code(kernel_language, &expr),
            ExecuteOpts::INLINE,
        )
        .await
        .ok_or(JupyterError::NotConnected)?
}

/// Fail the render for a span that raised or produced no value (design decision 2,
/// divergences 1 and 6). What a span with a value presents to, or that it is left in
/// place with a warning, is `present_document`'s to decide.
fn span_fatal(result: &CellResult, span: &InlineUnit, ctx: &ExecutionContext) -> JupyterResult<()> {
    let problem = match classify_inline_result(result) {
        InlineOutcome::Raised { ename, evalue } => InlineSpanProblem::Raised { ename, evalue },
        InlineOutcome::Silent => InlineSpanProblem::NoValue,
        InlineOutcome::Value(_) | InlineOutcome::NoTextValue => return Ok(()),
    };
    Err(span_error(&problem, span, ctx))
}

fn span_error(
    problem: &InlineSpanProblem,
    span: &InlineUnit,
    ctx: &ExecutionContext,
) -> JupyterError {
    JupyterError::CellExecutionFailed {
        message: span_diagnostic(problem, span, ctx).to_text(None),
    }
}

/// A presenter error as the engine's: a front matter value that cannot be written
/// back is the span's fatal diagnostic (divergence 10); invalid cell options cannot
/// reach here (the loop above rejects them before the cell runs) but map anyway.
fn present_error(err: PresentError, units: &[Unit], ctx: &ExecutionContext) -> JupyterError {
    match err {
        PresentError::FrontMatterValue { unit, key, reason } => match units.get(unit) {
            Some(Unit::Inline(span)) => span_error(
                &InlineSpanProblem::FrontMatterValue { key, reason },
                span,
                ctx,
            ),
            _ => JupyterError::CellExecutionFailed {
                message: format!(
                    "the front matter value `{key}` cannot hold the inline result: {reason}"
                ),
            },
        },
        PresentError::InvalidCellOptions { message, .. } => {
            JupyterError::InvalidCellOptions { message }
        }
    }
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
    use crate::engine::units::legacy_cells;

    // ── select_kernel (JOL-2a) ───────────────────────────────────────────────

    fn jupyter_meta(name: &str) -> std::collections::HashMap<String, TsMetadataValue> {
        std::collections::HashMap::from([(
            "jupyter".to_string(),
            TsMetadataValue::String(name.to_string()),
        )])
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
        .with_metadata(jupyter_meta("q2-no-such-kernel-jol2a"))
        .with_owned_languages(vec!["python".to_string()]);
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

    // ── Helpers ──────────────────────────────────────────────────────────────

    fn spec(name: &str, language: &str) -> KernelSpecSummary {
        KernelSpecSummary {
            name: name.to_string(),
            language: language.to_string(),
            path: std::path::PathBuf::from(format!("/k/{name}")),
        }
    }

    /// Kernelspecs for python, julia and R (the Ark spec declares `"R"`).
    fn installed() -> Vec<KernelSpecSummary> {
        vec![
            spec("python3", "python"),
            spec("julia-1.11", "julia"),
            spec("ark", "R"),
        ]
    }

    /// Plan the cells of `input` the way `execute_qmd` does: scan once, mark
    /// the view with the owned set, then plan against the injected kernelspecs.
    fn plan_text(
        input: &str,
        owned: &[&str],
        multi_engine: bool,
        explicit: Option<&str>,
        specs: &[KernelSpecSummary],
        eval_disabled: &dyn Fn(&CellUnit) -> bool,
    ) -> Result<DocumentPlan, ExecutionError> {
        let units = crate::engine::units::scan_units(input);
        let owns = |lang: &str| owned.iter().any(|o| o.eq_ignore_ascii_case(lang));
        let view = view_for_engine(&units, &owns);
        plan_document(
            &view,
            explicit.map(str::to_string),
            specs,
            None,
            multi_engine,
            eval_disabled,
        )
    }

    /// `plan_text` for bare `{lang}` cells over the `installed()` kernelspecs.
    fn plan(
        languages: &[&str],
        owned: &[&str],
        multi_engine: bool,
    ) -> Result<DocumentPlan, ExecutionError> {
        let text: String = languages
            .iter()
            .map(|l| format!("```{{{l}}}\ncode\n```\n\n"))
            .collect();
        plan_text(&text, owned, multi_engine, None, &installed(), &|_| false)
    }

    /// The cells that run when jupyter owns `python` (single-engine, over
    /// the `installed()` kernelspecs).
    fn ran(input: &str) -> Vec<CellUnit> {
        plan_text(input, &["python"], false, None, &installed(), &|_| false)
            .unwrap()
            .executable
    }

    fn executed_languages(p: &DocumentPlan) -> Vec<&str> {
        p.executable.iter().map(|c| c.lang.as_str()).collect()
    }

    fn codes(p: &DocumentPlan) -> Vec<Option<&str>> {
        p.warnings.iter().map(|w| w.code.as_deref()).collect()
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

    // ── plan_document tests ──────────────────────────────────────────────────

    // Multi-engine: an owned language with no kernelspec is a loud, clean
    // refusal, raised before anything runs. Vacuity revert: drop the
    // multi-engine branch ⇒ RED.
    #[test]
    fn test_plan_owned_unrunnable_fails_loudly_in_multi_engine() {
        let err = plan(&["sql"], &["sql"], true).unwrap_err();
        let ExecutionError::NoHandlerForLanguage { engine, language } = err else {
            panic!("expected NoHandlerForLanguage, got {err:?}");
        };
        assert_eq!(engine, "jupyter");
        assert_eq!(language, "sql");
    }

    // Single-engine: the cell passes through, the call succeeds, and one
    // Q-18-5 warning names the language and carries the hint.
    #[test]
    fn test_plan_single_engine_owned_unrunnable_warns_and_passes_through() {
        let p = plan(&["sql"], &["sql"], false).expect("single-engine must not error");
        assert!(p.executable.is_empty(), "sql must not run");
        assert_eq!(p.kernel, None);
        assert_eq!(codes(&p), [Some("Q-18-5")]);
        let text = p.warnings[0].to_text(None);
        assert!(text.contains("`sql`"), "{text}");
        assert!(text.contains("jupyter kernelspec list"), "{text}");
        assert!(text.contains("jupyter: <kernel>"), "{text}");
    }

    // The error and the warning share the code and the problem text.
    #[test]
    fn test_unrunnable_error_and_warning_share_code_and_text() {
        use crate::engine::diagnostics::engine_error_diagnostic;
        let err = plan(&["sql"], &["sql"], true).unwrap_err();
        let as_error = engine_error_diagnostic(&err);
        let warning = plan(&["sql"], &["sql"], false).unwrap().warnings.remove(0);
        assert_eq!(as_error.code.as_deref(), Some("Q-18-5"));
        assert_eq!(as_error.code, warning.code);
        assert_eq!(as_error.title, warning.title);
        assert_eq!(as_error.problem, warning.problem);
        assert_eq!(as_error.hints, warning.hints);
    }

    // One warning per document, listing every unrunnable language once.
    #[test]
    fn test_plan_reports_unrunnable_languages_once() {
        let p = plan(
            &["python", "sql", "sql", "sos"],
            &["python", "sql", "sos"],
            false,
        )
        .unwrap();
        assert_eq!(executed_languages(&p), ["python"]);
        assert_eq!(codes(&p), [Some("Q-18-5")]);
        let text = p.warnings[0].to_text(None);
        assert!(text.contains("`sql`") && text.contains("`sos`"), "{text}");
        assert_eq!(text.matches("`sql`").count(), 1, "{text}");
    }

    // Not owned (ceded, or owned by nobody): untouched, no diagnostic.
    #[test]
    fn test_plan_not_owned_cells_pass_through_silently() {
        let p = plan(&["python", "sql", "julia"], &["python"], true).unwrap();
        assert_eq!(executed_languages(&p), ["python"]);
        assert!(p.warnings.is_empty(), "{:?}", p.warnings);
    }

    // `ggsql`, `bash` and a made-up language execute when owned and a
    // kernelspec declares them, with no q2 source change.
    #[test]
    fn test_plan_runs_any_owned_language_that_has_a_kernelspec() {
        let mut specs = installed();
        specs.extend([
            spec("ggsql", "ggsql"),
            spec("bash", "bash"),
            spec("zz", "zzlang"),
        ]);
        let text = "```{ggsql}\nq\n```\n\n```{bash}\nls\n```\n\n```{zzlang}\nx\n```\n";
        let p = plan_text(
            text,
            &["ggsql", "bash", "zzlang"],
            true,
            None,
            &specs,
            &|_| false,
        )
        .unwrap();
        assert_eq!(executed_languages(&p), ["ggsql", "bash", "zzlang"]);
        assert_eq!(p.kernel.as_deref(), Some("ggsql"));
        assert!(p.warnings.is_empty());
    }

    #[test]
    fn test_plan_matches_owned_set_ignoring_case() {
        // The Ark kernelspec declares "R"; the resolver may record `r` or `R`.
        let p = plan(&["r"], &["R"], true).unwrap();
        assert_eq!(executed_languages(&p), ["r"]);
        assert_eq!(p.kernel.as_deref(), Some("ark"));
    }

    // sql owned by knitr is not in jupyter's owned set: it is left alone even
    // though jupyter has no sql kernel.
    #[test]
    fn test_sql_not_owned_by_jupyter_when_knitr_owns_it() {
        let p = plan(&["python", "sql"], &["python"], true).unwrap();
        assert_eq!(executed_languages(&p), ["python"]);
        assert!(p.warnings.is_empty());
    }

    // ojs, mermaid and dot are never in the owned set.
    #[test]
    fn test_plan_cell_handler_languages_pass_through() {
        let p = plan(&["ojs", "mermaid", "dot"], &[], true).unwrap();
        assert!(p.executable.is_empty());
        assert!(p.warnings.is_empty());
        assert_eq!(p.kernel, None);
    }

    // eval: false needs no kernel: exempt from the warning and the error.
    #[test]
    fn test_plan_eval_false_cell_is_exempt_from_the_report() {
        let listing = "```{bash}\n#| eval: false\nls\n```\n";
        let eval_off = |c: &CellUnit| c.code.contains("eval: false");
        for multi in [false, true] {
            let p = plan_text(listing, &["bash"], multi, None, &installed(), &eval_off)
                .expect("an eval: false cell must not error");
            assert!(p.executable.is_empty());
            assert!(p.warnings.is_empty(), "multi={multi}: {:?}", p.warnings);
        }
        // Not exempt when eval is on: same text, no override.
        assert!(plan_text(listing, &["bash"], true, None, &installed(), &|_| false).is_err());
    }

    // The resolver sees cells in blockquotes and lists; the executor does not.
    // A language present only there is owned but never run, and must not
    // trigger Q-18-5 or drive kernel selection.
    #[test]
    fn test_plan_owned_language_only_in_a_blockquote_is_ignored() {
        let text = "> ```{bash}\n> ls\n> ```\n\n- item\n\n  ```{bash}\n  ls\n  ```\n";
        let p = plan_text(text, &["bash"], true, None, &installed(), &|_| false).unwrap();
        assert!(p.executable.is_empty());
        assert!(p.warnings.is_empty());
        assert_eq!(p.kernel, None);
    }

    // The explicit kernel takes every owned cell and nothing is checked
    // against the kernelspec list (`jupyter: sos` with a `{ggsql}` cell).
    #[test]
    fn test_plan_explicit_kernel_runs_every_owned_cell() {
        let p = plan_text(
            "```{python}\nx\n```\n\n```{ggsql}\nq\n```\n",
            &["python", "ggsql"],
            true,
            Some("sos"),
            &[],
            &|_| false,
        )
        .unwrap();
        assert_eq!(executed_languages(&p), ["python", "ggsql"]);
        assert_eq!(p.kernel.as_deref(), Some("sos"));
        assert!(p.warnings.is_empty());
    }

    // No owned cell: an explicit kernel stands (EXR-3 evaluates spans on it);
    // a derived one does not exist.
    #[test]
    fn test_plan_no_cells_keeps_only_an_explicit_kernel() {
        let p = plan_text("just text\n", &["python"], true, Some("sos"), &[], &|_| {
            false
        })
        .unwrap();
        assert!(p.executable.is_empty());
        assert_eq!(p.kernel.as_deref(), Some("sos"));
        let p = plan_text(
            "just text\n",
            &["python"],
            true,
            None,
            &installed(),
            &|_| false,
        )
        .unwrap();
        assert_eq!(p.kernel, None);
    }

    // Deprecated aliases still run this release and warn with the canonical
    // name. Remove with the alias table (dated follow-up in the plan).
    #[test]
    fn test_plan_deprecated_aliases_run_and_warn() {
        let p = plan(&["py", "jl", "py"], &["py", "jl"], false).unwrap();
        assert_eq!(executed_languages(&p), ["py", "jl", "py"]);
        assert_eq!(p.kernel.as_deref(), Some("python3"));
        assert_eq!(p.warnings.len(), 2, "one per distinct alias");
        let text: String = p.warnings.iter().map(|w| w.to_text(None)).collect();
        assert!(
            text.contains("{python}") && text.contains("{julia}"),
            "{text}"
        );
        assert!(codes(&p).iter().all(Option::is_none));
    }

    // The kernel is the first owned language, in document order, with a
    // kernelspec; the cells of the other languages still run on it (one
    // kernel serves the document).
    #[test]
    fn test_plan_picks_first_language_with_a_kernelspec() {
        let p = plan(&["julia", "python"], &["julia", "python"], true).unwrap();
        assert_eq!(p.kernel.as_deref(), Some("julia-1.11"));
        assert_eq!(executed_languages(&p), ["julia", "python"]);
    }

    // Pins the ownership routing the multi-engine failure relies on: for an
    // explicit [knitr, jupyter] document with {r}+{sql}, the real resolver
    // gives `sql` to jupyter and `r` to knitr, and `owned_languages_for`
    // is exactly the set the gate reads.
    //
    // Gated cfg(not(wasm32)) because KnitrEngine and JupyterEngine are native-only.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn test_resolver_owned_languages_for_jupyter_in_explicit_knitr_jupyter() {
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

        let mut registry = EngineRegistry::empty();
        registry.register(Arc::new(KnitrEngine::new()));
        registry.register(Arc::new(JupyterEngine::new()));

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

        assert_eq!(
            resolution.ownership.get("sql").map(|s| s.as_str()),
            Some("jupyter"),
            "explicit [knitr, jupyter] + {{sql}} → sql owned by jupyter via T2"
        );
        assert_eq!(resolution.owned_languages_for("jupyter"), ["sql"]);
        assert_eq!(resolution.owned_languages_for("knitr"), ["r"]);
        // The complement still holds.
        assert!(
            resolution
                .handled_languages_for("jupyter")
                .contains(&"r".to_string())
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
    // cell reaches the view. When jupyter owns the language but no kernelspec
    // declares it, a multi-engine document fails loudly; a single-engine one
    // warns and passes through. Not owned, it is ignored.
    #[test]
    fn exr2_p1_dotted_cell_reaches_the_plan() {
        let input = "```{python.marimo}\nx=1\n```\n";
        assert_eq!(parse_code_blocks(input).len(), 1);
        let one =
            |owned: &[&str], multi| plan_text(input, owned, multi, None, &installed(), &|_| false);
        assert!(matches!(
            one(&["python.marimo"], true),
            Err(ExecutionError::NoHandlerForLanguage { .. })
        ));
        let single = one(&["python.marimo"], false).expect("single-engine passes through");
        assert!(single.executable.is_empty());
        assert_eq!(codes(&single), [Some("Q-18-5")]);
        assert!(one(&[], true).unwrap().warnings.is_empty());
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
        let run = ran(input);
        assert!(run.is_empty(), "no cell runs: {run:?}");
        // And the plain form the Phase 0 pin used.
        assert!(ran("```\n{python}\nx=1\n```\n").is_empty());
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
        let run = ran(input);
        assert_eq!(run.len(), 1, "{run:?}");
        assert_eq!(run[0].lang, "python");
        assert_eq!(run[0].code, "x=1\n");
        // The trailing-text closer is still not a closer for either.
        assert!(ran("```{python}\nx=1\n```  trailing\n").is_empty());
    }

    // bd-f63cqoke: a backtick run of three or more anywhere in a cell's source
    // makes the qmd writer lengthen the fence (`determine_backticks`), so the
    // text the engine receives opens with ````{python}. That cell must run, as
    // it does in Quarto 1 (`jupyter.ts` opens on ```+). Before the fix it was
    // silently left as an inert code block.
    #[test]
    fn bd_f63cqoke_cell_with_a_backtick_run_survives_the_qmd_round_trip() {
        let qmd = "```{python}\nprint(\"a ``` b\")\n```\n\n```{python}\nprint(\"control\")\n```\n";
        let mut output = Vec::<u8>::new();
        let (doc, _ctx, _warnings) =
            pampa::readers::qmd::read(qmd.as_bytes(), false, "t.qmd", &mut output, true, None)
                .expect("parse");
        let mut buf = Vec::new();
        pampa::writers::qmd::write(&doc, &mut buf).expect("serialize");
        let text = String::from_utf8(buf).expect("utf8");
        assert!(
            text.contains("````{python}\nprint(\"a ``` b\")\n````"),
            "the writer lengthens this fence; if it stops doing so this test \
             no longer covers the bug:\n{text}"
        );

        let run = ran(&text);
        assert_eq!(
            run.iter().map(|c| c.code.as_str()).collect::<Vec<_>>(),
            ["print(\"a ``` b\")\n", "print(\"control\")\n"],
            "both cells run"
        );
    }

    #[test]
    fn bd_f63cqoke_longer_opener_runs() {
        for fence in ["````", "`````"] {
            let run = ran(&format!("{fence}{{python}}\nx=1\n{fence}\n"));
            assert_eq!(run.len(), 1, "{fence}: {run:?}");
            assert_eq!(run[0].code, "x=1\n");
        }
    }

    // The Phase 3 corpus: the cells the scanner hands the executor equal what
    // the old finder found, except where the old finder was wrong or
    // unreachable. Each exception is named with its reason.
    #[test]
    fn exr2_p4_scanner_filtered_by_the_legacy_rule_equals_the_old_finder_on_the_corpus() {
        const EXCEPTIONS: &[(&str, &str)] = &[
            (
                "cell_inside_display_fence",
                "quirk (i): the old finder runs an opener shown inside a longer fence; \
                 the mask hides it in the pipeline, the scanner never claims it",
            ),
            (
                "cells_nested",
                "bd-f63cqoke: a four-backtick `{python}` cell runs (Quarto 1 parity); \
                 the old finder needed exactly three",
            ),
        ];
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

    // Ownership is case-insensitive: a `{Python}` cell runs when `python` is
    // owned and is left alone when it is not.
    #[test]
    fn exr2_p0_ownership_is_case_insensitive() {
        let p = plan(&["Python"], &["python"], false).unwrap();
        assert_eq!(executed_languages(&p), ["Python"]);
        let p = plan(&["Python"], &["julia"], false).unwrap();
        assert!(p.executable.is_empty());
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

    pub(super) fn jupyter_meta(name: &str) -> std::collections::HashMap<String, TsMetadataValue> {
        std::collections::HashMap::from([(
            "jupyter".to_string(),
            TsMetadataValue::String(name.to_string()),
        )])
    }

    /// A context whose source map holds `text` as `doc.qmd`, so locations resolve.
    pub(super) fn ctx_for(text: &str) -> ExecutionContext {
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
    pub(super) fn with_kernel(text: &str, script: &Script) -> (FakeKernelHome, ExecutionContext) {
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

#[cfg(test)]
#[path = "text_execute_e2e.rs"]
mod e2e_tests;

#[cfg(test)]
mod record_tests {
    //! EXR-5 Phase 0: the execution record the unit loop builds beside its unchanged
    //! markdown. The kernel-free seam is EXR-3's fake kernel (`fake_kernel.rs`): canned
    //! results travel through the real daemon and session, so these tests exercise the
    //! same loop production runs, with no `ipykernel`.

    use super::span_tests::{ctx_for, jupyter_meta, with_kernel};
    use super::*;
    use crate::engine::jupyter::fake_kernel::{FakeKernel, FakeKernelHome, FakeOutput, Script};
    use crate::engine::units::{UnitId, scan_units};
    use crate::presenter::cell::render_cell;
    use crate::presenter::visibility::CellVisibility;
    use crate::record::{
        AuthorText, AuthorTexts, EngineRecord, UnitKind, content_hash, fill_ids, span_content_hash,
    };
    use serde_json::json;

    pub(super) fn py(expr: &str) -> String {
        wrap_inline_code("python", expr)
    }

    pub(super) fn stdout(text: &str) -> Vec<FakeOutput> {
        vec![FakeOutput::Stdout(text.to_string())]
    }

    /// The code the kernel receives for a cell: the option-stripped body keeps the
    /// newline before the closing fence.
    pub(super) fn cell(code: &str) -> String {
        format!("{code}\n")
    }

    /// What a staged context keeps alive: the fake kernel home and the directory any
    /// figure the presenter writes lands in (never the crate directory).
    pub(super) struct Keep {
        _home: FakeKernelHome,
        _dir: tempfile::TempDir,
    }

    /// A context the stage would hand the engine: units scanned once, ids filled, the
    /// author texts aligned, and a record wanted.
    pub(super) fn staged(text: &str, script: &Script) -> (Keep, ExecutionContext) {
        let (home, mut ctx) = with_kernel(text, script);
        let dir = tempfile::tempdir().unwrap();
        ctx.source_path = dir.path().join("doc.qmd");
        let mut units = scan_units(text);
        let texts = AuthorTexts::from_scanned(&units);
        fill_ids(&mut units, &texts).unwrap();
        ctx.units = units;
        ctx.author_texts = texts;
        ctx.want_record = true;
        (
            Keep {
                _home: home,
                _dir: dir,
            },
            ctx,
        )
    }

    fn run(text: &str, script: &Script) -> (ExecuteResult, EngineRecord) {
        let (_keep, ctx) = staged(text, script);
        let result = execute_qmd(text, &ctx).expect("execution succeeds");
        let record = result
            .record
            .clone()
            .expect("a record is built when the context wants one");
        (result, record)
    }

    fn unit_ids(ctx: &ExecutionContext) -> Vec<String> {
        ctx.units
            .iter()
            .map(|u| match u {
                Unit::Cell(c) => c.id.clone().unwrap().0,
                Unit::Inline(i) => i.id.clone().unwrap().0,
            })
            .collect()
    }

    // ── 0a: the seam ────────────────────────────────────────────────────

    /// The fake kernel drives the real loop: a cell's markdown is exactly what
    /// `render_cell` makes of the kernel's outputs.
    #[test]
    fn execute_blocks_through_the_fake_kernel_match_render_cell_shape() {
        let text = "```{python}\nprint('hi')\n```\n";
        let script = Script::from([(cell("print('hi')"), stdout("hi\n"))]);
        let (_home, ctx) = with_kernel(text, &script);
        let out = execute_qmd(text, &ctx).unwrap().markdown;

        let result = CellResult {
            status: ExecuteStatus::Ok,
            outputs: vec![CellOutput::Stream {
                name: "stdout".to_string(),
                text: "hi\n".to_string(),
            }],
            execution_count: None,
        };
        let mut fig = FsFigureSink::new(&ctx.source_path);
        let expected = render_cell(
            "python",
            "print('hi')",
            &result,
            &mut fig,
            CellVisibility::resolve(None),
        );
        assert_eq!(out.trim_end(), expected.trim_end());
        assert!(out.contains("hi\n"), "{out}");
    }

    // ── 0b: builder call sites ──────────────────────────────────────────

    #[test]
    fn no_record_is_built_when_not_wanted() {
        let text = "```{python}\nx\n```\n";
        let (_keep, mut ctx) = staged(text, &Script::new());
        ctx.want_record = false;
        let r = execute_qmd(text, &ctx).unwrap();
        assert!(r.record.is_none());
        assert!(r.markdown.contains("::: {.cell"), "{}", r.markdown);
    }

    #[test]
    fn record_has_one_cell_per_executed_unit_in_document_order() {
        let text = "```{python}\na\n```\n\nv `{python} e`\n\n```{python}\nb\n```\n";
        let script = Script::from([
            (cell("a"), stdout("A\n")),
            (py("e"), vec![FakeOutput::plain_result("E")]),
            (cell("b"), stdout("B\n")),
        ]);
        let (_keep, ctx) = staged(text, &script);
        let record = execute_qmd(text, &ctx).unwrap().record.unwrap();

        let ids: Vec<&str> = record
            .cells
            .iter()
            .map(|c| c.metadata.quarto_cell_id.as_str())
            .collect();
        assert_eq!(ids, unit_ids(&ctx));
        let kinds: Vec<UnitKind> = record
            .cells
            .iter()
            .map(|c| c.metadata.quarto_kind)
            .collect();
        assert_eq!(kinds, [UnitKind::Cell, UnitKind::Inline, UnitKind::Cell]);
        assert_eq!(record.engine.name, "jupyter");
    }

    /// Positron's hash of the lines between the fences, option lines included, checked
    /// against EXR-4's conformance vectors.
    #[test]
    fn record_cell_ids_are_content_hashes_including_option_lines() {
        let vectors: Vec<serde_json::Value> = serde_json::from_str(include_str!(
            "../../../tests/fixtures/records/content-hash-vectors.json"
        ))
        .unwrap();
        let vector = vectors
            .iter()
            .find(|v| v["name"] == "label-line-and-unicode")
            .expect("the labeled vector");
        let body = vector["input"].as_str().unwrap();
        let expected = vector["expected"].as_str().unwrap();
        assert_eq!(body, "#| label: fig-é\nx <- 'é😀'");

        let text = format!("```{{python}}\n{body}\n```\n");
        let (_, record) = run(&text, &Script::new());
        let cell = &record.cells[0];
        assert_eq!(cell.id, expected);
        assert_eq!(cell.metadata.quarto_content_hash, expected);
        assert_eq!(cell.metadata.quarto_label.as_deref(), Some("fig-é"));
        assert_eq!(
            cell.metadata.quarto_cell_id,
            format!("0-{}-fig-é", &expected[..8])
        );
        // The option line is part of the hash: the code alone hashes differently.
        assert_ne!(cell.id, content_hash("x <- 'é😀'"));
    }

    /// The builder uses the stage's ids as given and scans nothing itself (epic decision 38).
    #[test]
    fn record_ids_come_from_the_stage_units() {
        let text = "```{python}\nx\n```\n\n`{python} y`\n";
        let script = Script::from([(py("y"), vec![FakeOutput::plain_result("1")])]);
        let (_keep, mut ctx) = staged(text, &script);
        for (n, unit) in ctx.units.iter_mut().enumerate() {
            let id = Some(UnitId(format!("stub-{n}")));
            match unit {
                Unit::Cell(c) => c.id = id,
                Unit::Inline(i) => i.id = id,
            }
        }
        let record = execute_qmd(text, &ctx).unwrap().record.unwrap();
        let ids: Vec<&str> = record
            .cells
            .iter()
            .map(|c| c.metadata.quarto_cell_id.as_str())
            .collect();
        assert_eq!(ids, ["stub-0", "stub-1"]);
    }

    /// Crossref sugaring strips the label and caption lines from what the engine sees;
    /// the record hashes the author's text the stage mapped (epic decision 36).
    #[test]
    fn record_content_hash_uses_author_text_for_labeled_cell() {
        // What the engine sees: the sugared text.
        let text = "```{python}\nx = 1\n```\n";
        // What the author wrote.
        let author = "#| label: fig-x\n#| fig-cap: A cap\nx = 1";
        let script = Script::from([(cell("x = 1"), vec![])]);
        let (_home, mut ctx) = with_kernel(text, &script);
        let mut units = scan_units(text);
        let texts = AuthorTexts(vec![AuthorText {
            text: author.to_string(),
            content_hash: content_hash(author),
        }]);
        fill_ids(&mut units, &texts).unwrap();
        ctx.units = units;
        ctx.author_texts = texts;
        ctx.want_record = true;

        let result = execute_qmd(text, &ctx).unwrap();
        let cell = &result.record.as_ref().unwrap().cells[0];
        let hash = content_hash(author);
        assert_eq!(cell.metadata.quarto_content_hash, hash);
        assert_eq!(cell.metadata.quarto_label.as_deref(), Some("fig-x"));
        assert_eq!(
            cell.metadata.quarto_cell_id,
            format!("0-{}-fig-x", &hash[..8])
        );
        assert_eq!(
            cell.source,
            ["#| label: fig-x\n", "#| fig-cap: A cap\n", "x = 1"]
        );
        // The live path is untouched: it echoes the text it was given.
        assert!(result.markdown.contains("x = 1"), "{}", result.markdown);
        assert!(!result.markdown.contains("fig-cap"), "{}", result.markdown);
    }

    /// The engine's whole declaration lands in the entry (epic decision 35); no engine
    /// name is matched on the way.
    #[test]
    fn record_engine_entry_carries_full_hash_definition() {
        let text = "```{python}\nx\n```\n";
        let (_keep, ctx) = staged(text, &Script::new());

        let mut custom = HashDefinition::conservative("custom-rule");
        custom.version = 7;
        custom.presentation_cell_options = vec!["foo".to_string()];
        custom.document_keys = vec!["zzz".to_string()];
        let record = execute_qmd_with(text, &ctx, &custom)
            .unwrap()
            .record
            .unwrap();
        assert_eq!(record.engine.hash_definition, custom);

        let record = execute_qmd(text, &ctx).unwrap().record.unwrap();
        assert_eq!(
            record.engine.hash_definition,
            crate::engine::jupyter::jupyter_hash_definition()
        );
    }

    /// Figure names are keyed to executed-cell position, so hidden cells stay in the
    /// record, in order, outputs included.
    #[test]
    fn record_keeps_hidden_cells() {
        let text = "```{python}\n#| include: false\na\n```\n\n\
                    ```{python}\n#| echo: false\nb\n```\n\n\
                    ```{python}\n#| output: false\nc\n```\n";
        let script = Script::from([
            (cell("a"), stdout("A\n")),
            (cell("b"), stdout("B\n")),
            (cell("c"), stdout("C\n")),
        ]);
        let (result, record) = run(text, &script);
        let texts: Vec<&str> = record
            .cells
            .iter()
            .map(|c| c.outputs[0]["text"].as_str().unwrap())
            .collect();
        assert_eq!(texts, ["A\n", "B\n", "C\n"]);
        // Visibility is a presentation option, resolved from the document at present
        // time (epic decision 6): it is in no cell's execution-relevant options.
        assert!(
            record
                .cells
                .iter()
                .all(|c| c.metadata.options().count() == 0)
        );
        // The live markdown hides what it hid before.
        assert!(!result.markdown.contains("A\n"), "{}", result.markdown);
        assert!(result.markdown.contains("B\n"), "{}", result.markdown);
        assert!(!result.markdown.contains("C\n"), "{}", result.markdown);
    }

    /// Outputs are nbformat-shaped and lossless: the raw mime bundle, not the text the
    /// presenter chose for one format.
    #[test]
    fn record_outputs_are_verbatim() {
        let text = "```{python}\ns\n```\n\n```{python}\nr\n```\n\n\
                    ```{python}\nd\n```\n\n```{python}\n#| error: true\ne\n```\n";
        let script = Script::from([
            (cell("s"), stdout("hi\nthere\n")),
            (
                cell("r"),
                vec![FakeOutput::Result(
                    json!({ "text/plain": "'v'", "text/html": "<b>v</b>" }),
                )],
            ),
            (
                cell("d"),
                vec![FakeOutput::Display(
                    json!({ "image/png": "iVBORw0KGgo=", "text/plain": "<Figure>" }),
                )],
            ),
            (
                cell("e"),
                vec![FakeOutput::Error {
                    ename: "ValueError".to_string(),
                    evalue: "bad".to_string(),
                }],
            ),
        ]);
        let (_, record) = run(text, &script);
        let outputs: Vec<&Vec<serde_json::Value>> =
            record.cells.iter().map(|c| &c.outputs).collect();
        assert_eq!(
            outputs[0],
            &vec![json!({ "output_type": "stream", "name": "stdout", "text": "hi\nthere\n" })]
        );
        assert_eq!(
            outputs[1],
            &vec![json!({
                "output_type": "execute_result",
                "execution_count": 1,
                "data": { "text/plain": "'v'", "text/html": "<b>v</b>" },
                "metadata": {},
            })]
        );
        assert_eq!(
            outputs[2],
            &vec![json!({
                "output_type": "display_data",
                "data": { "image/png": "iVBORw0KGgo=", "text/plain": "<Figure>" },
                "metadata": {},
            })]
        );
        assert_eq!(
            outputs[3],
            &vec![json!({
                "output_type": "error",
                "ename": "ValueError",
                "evalue": "bad",
                "traceback": [],
            })]
        );
    }

    #[test]
    fn record_notes_kernel_per_cell() {
        let text = "```{python}\nx\n```\n\n`{python} y`\n";
        let script = Script::from([(py("y"), vec![FakeOutput::plain_result("1")])]);
        let (_, record) = run(text, &script);
        for cell in &record.cells {
            assert_eq!(cell.metadata.quarto_kernel.as_deref(), Some("fake-py"));
            assert_eq!(cell.metadata.quarto_lang, "python");
        }
        let spec = &record.engine.provenance["kernelspec"];
        assert_eq!(spec["name"], "fake-py");
        assert_eq!(spec["language"], "python");
    }

    /// A ceded `{ojs}` cell and a span of a language nobody owns are units, not cells.
    #[test]
    fn ceded_and_unowned_fences_are_not_record_cells() {
        let text = "```{python}\nx\n```\n\n```{ojs}\nviewof a = 1\n```\n\nsee `{r} 1`\n";
        let (_keep, ctx) = staged(text, &Script::new());
        assert_eq!(ctx.units.len(), 3);
        let result = execute_qmd(text, &ctx).unwrap();
        let record = result.record.unwrap();
        assert_eq!(record.cells.len(), 1);
        assert_eq!(record.cells[0].metadata.quarto_cell_id, unit_ids(&ctx)[0]);
        assert!(result.markdown.contains("```{ojs}"), "{}", result.markdown);
        assert!(result.markdown.contains("`{r} 1`"), "{}", result.markdown);
    }

    /// A span is a one-line cell: its source is the expression as written (not the
    /// kernel wrapper), its id is `{n}-{hash8}-inline`, and its output is the raw bundle.
    #[test]
    fn inline_units_are_recorded_as_one_line_cells() {
        let text = "A `{python} 1 + 1` B\n";
        let script = Script::from([(py("1 + 1"), vec![FakeOutput::plain_result("2")])]);
        let (result, record) = run(text, &script);
        assert_eq!(result.markdown, "A 2 B\n");

        assert_eq!(record.cells.len(), 1);
        let cell = &record.cells[0];
        let hash = span_content_hash("python", "1 + 1");
        assert_eq!(cell.metadata.quarto_kind, UnitKind::Inline);
        assert_eq!(
            cell.metadata.quarto_cell_id,
            format!("0-{}-inline", &hash[..8])
        );
        assert_eq!(cell.metadata.quarto_content_hash, hash);
        assert_eq!(cell.source, ["1 + 1"]);
        assert_eq!(
            cell.outputs,
            vec![json!({
                "output_type": "execute_result",
                "execution_count": 1,
                "data": { "text/plain": "2" },
                "metadata": {},
            })]
        );
    }

    /// Owned spans with no cell at all still make a record (the kernel comes from `jupyter:`).
    #[test]
    fn a_span_only_document_records_its_span() {
        let text = "`{python} a`\n";
        let script = Script::from([(py("a"), vec![FakeOutput::plain_result("1")])]);
        let home = FakeKernelHome::new();
        let FakeKernel { name, env } = home.install("fake-py", "python", &script);
        let mut units = scan_units(text);
        let texts = AuthorTexts::from_scanned(&units);
        fill_ids(&mut units, &texts).unwrap();
        let ctx = ctx_for(text)
            .with_metadata(jupyter_meta(&name))
            .with_project_env(env)
            .with_units(units)
            .with_author_texts(texts)
            .with_want_record(true);
        let record = execute_qmd(text, &ctx).unwrap().record.unwrap();
        assert_eq!(record.cells.len(), 1);
        assert_eq!(record.cells[0].metadata.quarto_kind, UnitKind::Inline);
    }

    /// EXR-6 Phase 3: a notebook replayed from its stored outputs and the same cells
    /// executed live (the fake kernel answering with those outputs) present through
    /// one driver and produce identical markdown.
    #[test]
    fn ipynb_replay_and_live_share_one_presenter() {
        use crate::engine::content_processors::ORIGINAL_FILE_ID;
        use crate::engine::content_processors::ipynb::convert_notebook;
        use crate::engine::ipynb_replay::IpynbReplayEngine;
        use crate::engine::traits::ExecutionEngine;
        use quarto_source_map::SourceContext;
        use std::sync::Arc;

        const NB: &str = r##"{"cells":[{"cell_type":"markdown","metadata":{},"source":["# Shared\n\nProse before.\n"]},{"cell_type":"code","execution_count":1,"metadata":{},"outputs":[{"output_type":"stream","name":"stdout","text":["hello\n"]}],"source":["print('hello')\n"]},{"cell_type":"markdown","metadata":{},"source":["Between.\n"]},{"cell_type":"code","execution_count":2,"metadata":{},"outputs":[{"output_type":"execute_result","execution_count":2,"data":{"text/plain":["42"]},"metadata":{}}],"source":["#| echo: false\n6 * 7\n"]}],"metadata":{"kernelspec":{"name":"python3","language":"python"}},"nbformat":4,"nbformat_minor":5}"##;

        let dir = tempfile::tempdir().unwrap();
        let nb_path = dir.path().join("notebook.ipynb");
        let converted = convert_notebook(&nb_path, NB).unwrap();
        let markdown = converted.markdown.clone();

        // Replay: the production-shaped context (what `ParseDocumentStage` registers).
        let mut sc = SourceContext::new();
        sc.add_file(
            "<notebook.ipynb (converted by ipynb)>".to_string(),
            Some(markdown.clone()),
        );
        // lint:allow(add-file-with-id) single-notebook registration mirroring production
        sc.add_file_with_id(
            ORIGINAL_FILE_ID,
            nb_path.display().to_string(),
            Some(NB.to_string()),
        );
        for cell_file in &converted.files {
            sc.add_file(cell_file.label.clone(), Some(cell_file.text.clone()));
        }
        let replay_ctx = ExecutionContext::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            nb_path.clone(),
            "html",
        )
        .with_source_info(converted.source_info, Arc::new(sc));
        let replayed = IpynbReplayEngine::new()
            .execute(&markdown, &replay_ctx)
            .unwrap()
            .markdown;

        // Live: the fake kernel answers each cell with the notebook's stored outputs.
        let script = Script::from([
            (cell("print('hello')"), stdout("hello\n")),
            (cell("6 * 7"), vec![FakeOutput::plain_result("42")]),
        ]);
        let (_home, mut live_ctx) = with_kernel(&markdown, &script);
        live_ctx.source_path = nb_path;
        let live = execute_qmd(&markdown, &live_ctx).unwrap().markdown;

        assert_eq!(live, replayed);
        assert!(live.contains("hello"), "{live}");
        assert!(live.contains("42"), "{live}");
        assert!(
            !live.contains("6 * 7"),
            "echo: false hides the source:\n{live}"
        );
    }

    /// EXR-6 Phase 3b: a trace recorded from a live run (the fake kernel) replays
    /// through the presenter to the live markdown, for the same target format, and the
    /// record the replay returns is the recorded one byte for byte.
    #[test]
    fn replay_jupyter_trace_presents_same_markdown_as_live() {
        use crate::engine::replay::ReplayEngine;
        use crate::engine::traits::ExecutionEngine;
        use crate::record::coalesce;
        use quarto_trace::EngineCapture;

        let text = "Before.\n\n```{python}\na\n```\n\nValue `{python} e` after.\n\n```{python}\n#| echo: false\nb\n```\n";
        let script = Script::from([
            (cell("a"), stdout("A\n")),
            (py("e"), vec![FakeOutput::plain_result("E")]),
            (cell("b"), stdout("B\n")),
        ]);
        let (_keep, ctx) = staged(text, &script);
        let live = execute_qmd(text, &ctx).unwrap();
        let engine_record = live.record.clone().unwrap();
        // What the stage emits for the trace: the one-engine record notebook.
        let record = coalesce(std::slice::from_ref(&engine_record), &ctx.units).unwrap();
        let capture = EngineCapture {
            engine_name: "jupyter".into(),
            input_qmd: text.into(),
            result: serde_json::to_value(&live).unwrap(),
            files: Vec::new(),
            record: Some(serde_json::to_value(&record).unwrap()),
        };

        let replayed = ReplayEngine::new(capture).execute(text, &ctx).unwrap();
        assert_eq!(replayed.markdown, live.markdown);
        assert!(
            replayed.markdown.contains("Value E after."),
            "{}",
            replayed.markdown
        );
        assert!(
            !replayed.markdown.contains("\nb\n"),
            "{}",
            replayed.markdown
        );
        // `replay_engine_returns_byte_identical_record`, on a real run.
        let record_back = replayed.record.expect("the record travels on the result");
        assert_eq!(record_back.cells, engine_record.cells);
        assert_eq!(
            serde_json::to_value(&record_back.engine).unwrap(),
            serde_json::to_value(&engine_record.engine).unwrap()
        );
    }
}

#[cfg(test)]
mod owned_e2e_tests {
    //! JOL-5 Phase 3: kernel selection and language ownership, end to end through
    //! `execute_qmd`, the real daemon and the fake kernel (`fake_kernel.rs`). No
    //! `ipykernel`, no external install. The languages are made up (`alpha`, `beta`,
    //! `zzz`), which is itself the point of `test_owned_language_executes_without_allowlist`:
    //! no q2 source names them.

    use super::span_tests::{ctx_for, jupyter_meta};
    use super::*;
    use crate::engine::diagnostics::engine_error_diagnostic;
    use crate::engine::jupyter::fake_kernel::{FakeKernelHome, FakeOutput, Script, answering_as};

    /// The code a kernel receives for a cell body: the closing newline is kept.
    fn body(code: &str) -> String {
        format!("{code}\n")
    }

    #[test]
    fn test_jupyter_key_selects_kernel() {
        let text = "```{alpha}\nwho\n```\n";
        let home = FakeKernelHome::new();
        home.install("fake-a", "alpha", &answering_as("fake-a", &["who\n"]));
        home.install("fake-b", "beta", &answering_as("fake-b", &["who\n"]));
        // Each kernelspec answers with its own name, so the output says who ran it.
        let ctx = ctx_for(text)
            .with_metadata(jupyter_meta("fake-b"))
            .with_owned_languages(vec!["alpha".to_string()])
            .with_project_env(home.env_all());
        let out = execute_qmd(text, &ctx).unwrap().markdown;
        assert!(out.contains("fake-b"), "{out}");
        assert!(!out.contains("fake-a"), "{out}");
    }

    #[test]
    fn test_jupyter_key_owns_span_in_kernel_language() {
        let text = "---\ntitle: t\njupyter: fake-b\n---\n\nValue `{beta} who`.\n";
        let home = FakeKernelHome::new();
        let script = Script::from([("who".to_string(), vec![FakeOutput::plain_result("fake-b")])]);
        let b = home.install("fake-b", "beta", &script);
        // No cell and no owned set: the key alone makes the span run, on `fake-b`.
        let ctx = ctx_for(text)
            .with_metadata(jupyter_meta("fake-b"))
            .with_project_env(b.env);
        let out = execute_qmd(text, &ctx).unwrap().markdown;
        assert!(out.contains("Value fake-b."), "{out}");
    }

    #[test]
    fn test_owned_language_executes_without_allowlist() {
        let text = "```{zzz}\nhello\n```\n";
        let home = FakeKernelHome::new();
        let script = Script::from([(
            body("hello"),
            vec![FakeOutput::Stdout("zzz says hello\n".to_string())],
        )]);
        let k = home.install("fake-zzz", "zzz", &script);
        // No `jupyter:` key: the kernel is found by the cell's language.
        let ctx = ctx_for(text)
            .with_owned_languages(vec!["zzz".to_string()])
            .with_project_env(k.env);
        let r = execute_qmd(text, &ctx).unwrap();
        assert!(r.markdown.contains("zzz says hello"), "{}", r.markdown);
        assert!(r.warnings.is_empty(), "{:?}", r.warnings);
    }

    #[test]
    fn test_owned_by_nobody_passes_through() {
        // `engine: knitr` leaves jupyter's owned set empty. A kernel exists, but nothing
        // may start it.
        let text = "---\nengine: knitr\n---\n\n```{zzz}\nhello\n```\n";
        let home = FakeKernelHome::new();
        let k = home.install("fake-zzz", "zzz", &Script::new());
        let ctx = ctx_for(text).with_project_env(k.env);
        let r = execute_qmd(text, &ctx).unwrap();
        assert_eq!(r.markdown, text);
        assert!(r.warnings.is_empty(), "{:?}", r.warnings);
    }

    #[test]
    fn test_missing_kernel_single_engine_warns() {
        let text = "```{zzz}\nhello\n```\n";
        let _home = FakeKernelHome::new(); // empty JUPYTER_PATH: no kernelspec for `zzz`
        let ctx = ctx_for(text).with_owned_languages(vec!["zzz".to_string()]);
        let r = execute_qmd(text, &ctx).expect("a single-engine document still renders");
        assert_eq!(r.markdown, text, "the cell stays unexecuted");
        assert_eq!(r.warnings.len(), 1, "{:?}", r.warnings);
        assert_eq!(r.warnings[0].code.as_deref(), Some("Q-18-5"));
        let msg = r.warnings[0].to_text(None);
        assert!(msg.contains("`zzz`"), "{msg}");
        assert!(msg.contains("jupyter kernelspec list"), "{msg}");
    }

    #[test]
    fn test_missing_kernel_multi_engine_errors() {
        let text = "```{zzz}\nhello\n```\n";
        let _home = FakeKernelHome::new();
        let mut ctx = ctx_for(text).with_owned_languages(vec!["zzz".to_string()]);
        ctx.multi_engine = true;
        let err = execute_qmd(text, &ctx).expect_err("a multi-engine document fails");
        assert!(
            matches!(err, ExecutionError::NoHandlerForLanguage { .. }),
            "refused before any kernel starts, got {err:?}"
        );
        let msg = engine_error_diagnostic(&err).to_text(None);
        assert!(msg.contains("`zzz`"), "{msg}");
        assert!(msg.contains("jupyter kernelspec list"), "{msg}");
    }

    /// JOL-2a's real-kernel check: `jupyter: python3` sends a cell written in another
    /// language to the Python kernel, and does not look at the cell's language.
    /// Needs `ipykernel` and a `python3` kernelspec, so it is ignored by default:
    /// `cargo nextest run -p quarto-core --run-ignored only -E 'test(real_ipykernel)'`.
    #[test]
    #[ignore = "requires ipykernel"]
    fn test_jupyter_key_python3_beats_first_cell_language_real_ipykernel() {
        let text = "```{julia}\nimport sys\nprint('ran on python', sys.version_info.major)\n```\n";
        let ctx = ctx_for(text)
            .with_metadata(jupyter_meta("python3"))
            .with_owned_languages(vec!["julia".to_string()]);
        let r = execute_qmd(text, &ctx).expect("the python3 kernel runs the julia-fenced cell");
        assert!(r.markdown.contains("ran on python 3"), "{}", r.markdown);
        assert!(r.warnings.is_empty(), "{:?}", r.warnings);
    }

    #[test]
    fn test_ceded_language_not_executed() {
        let text = "```{alpha}\nwho\n```\n\n```{zzz}\nhello\n```\n";
        let home = FakeKernelHome::new();
        let a = home.install("fake-a", "alpha", &answering_as("fake-a", &["who\n"]));
        // `zzz` belongs to another engine: not in jupyter's owned set.
        let ctx = ctx_for(text)
            .with_owned_languages(vec!["alpha".to_string()])
            .with_project_env(a.env);
        let out = execute_qmd(text, &ctx).unwrap().markdown;
        assert!(out.contains("fake-a"), "the owned cell ran: {out}");
        assert!(
            out.contains("```{zzz}\nhello\n```"),
            "the ceded cell is verbatim: {out}"
        );
    }
}
