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

use regex::Regex;

use quarto_source_map::SourceInfo;

use super::daemon::daemon;
use super::error::JupyterError;
use super::execute::{CellOutput, CellResult, ExecuteStatus};
use super::session::SessionKey;
use crate::cell_options::partition_cell_options;
use crate::engine::context::{ExecuteResult, ExecutionContext};
use crate::engine::error::ExecutionError;
use crate::presenter::cell::{describe_location, render_cell};
use crate::presenter::figures::{FigureSink, FsFigureSink};
use crate::presenter::visibility::{CellVisibility, resolve_cell_options, resolved_flag};

type JupyterResult<T> = std::result::Result<T, JupyterError>;

/// A parsed code block from the input markdown.
#[derive(Debug)]
pub(super) struct CodeBlock {
    /// Start byte offset in the input.
    pub(super) start: usize,
    /// End byte offset in the input (exclusive).
    pub(super) end: usize,
    /// Start byte offset of the code content (the capture between the
    /// fences) in the input — anchors per-cell source attribution.
    pub(super) code_start: usize,
    /// The language/engine specifier (e.g., "python", "julia").
    pub(super) language: String,
    /// The code content.
    pub(super) code: String,
}

/// Execute code blocks in QMD input and return markdown with outputs.
///
/// This is the main entry point for text-based Jupyter execution.
pub fn execute_qmd(
    input: &str,
    ctx: &ExecutionContext,
) -> std::result::Result<ExecuteResult, ExecutionError> {
    // Parse ALL braced {lang} cells from input.
    let all_blocks = parse_code_blocks(input);

    if all_blocks.is_empty() {
        // No braced cells at all - passthrough.
        return Ok(ExecuteResult::new(input));
    }

    // Apply the handled_languages enforcement gate:
    //   - cells in handled_languages are ceded (dropped from the executable set)
    //   - cells jupyter owns but cannot execute:
    //     multi_engine=true  → loud Err(NoHandlerForLanguage)  (§10 case 4, P2-13)
    //     multi_engine=false → pass through unexecuted (display-only, P2-13)
    //   - remaining cells are executable
    let executable = partition_cells(all_blocks, &ctx.handled_languages, ctx.multi_engine)?;

    if executable.is_empty() {
        // All cells were ceded — passthrough unchanged.
        return Ok(ExecuteResult::new(input));
    }

    // Determine the kernel from the first *executable* (owned) cell.
    let kernel_name = map_language_to_kernel(&executable[0].language);

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
    let result = execute_blocks_async(input, &executable, &kernel_name, ctx);

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

/// Parse ALL braced `{lang}` code blocks from markdown input.
///
/// Returns every fenced code block whose fence is ```` ```{lang} ```` (e.g.
/// `{python}`, `{sql}`, `{ojs}`). Plain highlight fences (` ```r `) and
/// raw-format fences (` ```{=html} `) are excluded by the regex — they have
/// no word-only `{lang}` form.
///
/// Quarto 2 is strict: only a bare `{lang}` is accepted. The Quarto 1
/// variant `{python echo=false}` (fence-attached options) is not
/// supported — per-cell directives live inside the block as
/// `#| key: value` YAML comments. Dropping fence-attached options
/// keeps the tree-sitter grammar for qmd tractable.
///
/// Callers that need only executable cells should pass the result through
/// [`partition_cells`].
pub(super) fn parse_code_blocks(input: &str) -> Vec<CodeBlock> {
    // Match ```{language} ... ``` blocks (no fence options allowed).
    // `\w+` excludes `=` so raw-format fences like ` ```{=html} ` are never
    // matched.
    let pattern = r"(?m)^```\s*\{(\w+)\}\s*\n([\s\S]*?)^```\s*$";
    let re = Regex::new(pattern).expect("Invalid regex pattern");

    let mut blocks = Vec::new();

    for cap in re.captures_iter(input) {
        let full_match = cap.get(0).unwrap();
        let language = cap.get(1).unwrap().as_str().to_string();
        let code_match = cap.get(2).unwrap();
        let code = code_match.as_str().to_string();

        // Collect ALL braced cells ungated (branch cede/claim model):
        // `partition_cells` performs the executable/cede/loud-failure
        // classification downstream, so the parse-level executable gate is
        // removed. `code_start` (main #412) anchors per-cell source
        // attribution for figure emission.
        blocks.push(CodeBlock {
            start: full_match.start(),
            end: full_match.end(),
            code_start: code_match.start(),
            language,
            code,
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
    blocks: Vec<CodeBlock>,
    handled_languages: &[String],
    multi_engine: bool,
) -> Result<Vec<CodeBlock>, ExecutionError> {
    let mut executable = Vec::new();

    for block in blocks {
        let lang_lower = block.language.to_lowercase();

        // Is this language in the leave-alone set? (Case-insensitive match.)
        let is_ceded = handled_languages
            .iter()
            .any(|h| h.to_lowercase() == lang_lower);

        if is_ceded {
            // Ceded: jupyter does not execute this cell. It will remain
            // verbatim in the output (the surrounding text passthrough in
            // `execute_blocks_inner` handles that because ceded cells are not
            // in the executable set, so `last_end` never advances past them
            // and the block text is included in the `input[last_end..]` tail).
            continue;
        }

        if is_executable_language(&block.language) {
            // Jupyter owns this language and can execute it.
            executable.push(block);
        } else if multi_engine {
            // Multi-engine sequence: jupyter owns this language (not in
            // handled_languages) but has no handler/kernel for it. Fail
            // loudly — §10 case 4 (P2-13).
            return Err(ExecutionError::no_handler_for_language(
                "jupyter",
                block.language,
            ));
        } else {
            // Single-engine sequence: pass the cell through unexecuted.
            // It is left verbatim just like a ceded cell — the block text
            // falls into the `input[last_end..]` passthrough in
            // `execute_blocks_inner`. Q1's display-only treatment (P2-13).
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
    blocks: &[CodeBlock],
    kernel_name: &str,
    ctx: &ExecutionContext,
) -> JupyterResult<ExecuteResult> {
    // Drive the execution on the shared engine runtime — NOT a
    // per-call runtime. Kernel sessions (ZeroMQ sockets, the kernel
    // Child) are tokio resources bound to the runtime that created
    // them; a per-call runtime made every cross-call session reuse
    // fail with "Tokio context ... is being shutdown" (bd-hxhnnlzs).
    super::daemon::engine_runtime().block_on(execute_blocks_inner(input, blocks, kernel_name, ctx))
}

/// Inner async function that does the actual execution.
async fn execute_blocks_inner(
    input: &str,
    blocks: &[CodeBlock],
    kernel_name: &str,
    ctx: &ExecutionContext,
) -> JupyterResult<ExecuteResult> {
    let daemon = daemon();

    // Start or get existing kernel session
    let key: SessionKey = daemon
        .get_or_start_session(kernel_name, &ctx.cwd, &ctx.project_env)
        .await?;

    // Document-level defaults for cell options (bd-ohvl879u): the
    // pipeline's fully merged metadata (MetadataMergeStage runs before
    // engine execution) carries the `execute` map that cell options
    // merge over. The stage hands it to us directly (bd-nn2fou8h);
    // this used to be recovered by re-parsing the front matter of our
    // own serialized input, which only knitr's JSON-to-R boundary made
    // untenable to share.
    let doc_scope = ctx.execute_scope.clone();

    // Build output by processing blocks in order
    let mut output = String::new();
    let mut last_end = 0;
    // Figure emission (bd-5t6wvu7m): image outputs are written under
    // `<stem>_files/figure-html/` next to the source document.
    let mut fig = FsFigureSink::new(&ctx.source_path);

    for block in blocks {
        // Append content before this block
        output.push_str(&input[last_end..block.start]);

        // A fenced div cannot interrupt a paragraph — make sure the
        // `::: {.cell}` opener starts its own block even when the
        // source had no blank line before the cell.
        if output.ends_with('\n') && !output.ends_with("\n\n") {
            output.push('\n');
        }

        // Partition the cell into `#|` options + code (bd-ohvl879u).
        // `ctx.source_info` maps input offsets back to the original
        // files, so option spans and diagnostics resolve to real
        // source positions.
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
            .execute_in_session(&key, &cell.code)
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
        output.push_str(&render_cell(
            &block.language,
            &cell.code,
            &exec_result,
            &mut fig,
            visibility,
        ));

        last_end = block.end;
    }

    // Append any remaining content after the last block
    output.push_str(&input[last_end..]);

    // Report the `<stem>_files` dir when figures were written —
    // dir-level, mirroring knitr. `q2 render` copies it to the output
    // dir (bd-o8pr) and the preview capture embeds it (bd-qbhp2cvv).
    let mut result = ExecuteResult::new(output);
    if fig.wrote_any() {
        result = result.with_supporting_files(vec![fig.files_dir()]);
    }
    Ok(result)
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

    // ── T14 (H1): mask's opener-widening reddens this ───────────────────────
    //
    // `parse_code_blocks` is private, so this seam lives in this module
    // rather than exposing it (controller ruling R5). Runs the real
    // `nested_cell_mask::mask` end to end — parse a display block containing
    // a nested `{python}` opener, mask it, serialize the masked AST back to
    // qmd — and asserts jupyter's own partitioner finds nothing to execute
    // in the result. Reverting H1 (mask stops rewriting the opener) leaves
    // the literal `{python}` opener in the masked text, which
    // `parse_code_blocks` *would* then match — turning this RED.
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

        let cells = parse_code_blocks(&masked_text);
        assert!(
            cells.is_empty(),
            "masked text must yield 0 cells from jupyter's partitioner, got: {cells:?}"
        );
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
        let result = partition_cells(blocks, &handled, true);

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
        let result = partition_cells(blocks, &handled, false);

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
        let result = partition_cells(vec![python_block, sql_block], &handled, false);

        assert!(result.is_ok(), "should not error — sql is ceded, not owned");
        let executable = result.unwrap();

        // Only the python cell is executable.
        assert_eq!(executable.len(), 1, "only python should be executable");
        assert_eq!(
            executable[0].language, "python",
            "the executable cell must be python"
        );

        // sql must NOT appear in the executable set (vacuity: list is non-empty).
        assert!(
            !executable.iter().any(|b| b.language == "sql"),
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
        let result = partition_cells(vec![ojs_block], &handled, false);

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
        let result = partition_cells(
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
