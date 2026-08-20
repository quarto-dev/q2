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

use std::path::{Path, PathBuf};

use regex::Regex;

use quarto_pandoc_types::config_value::ConfigValue;
use quarto_source_map::SourceInfo;

use super::daemon::daemon;
use super::error::JupyterError;
use super::execute::{CellOutput, ExecuteResult as KernelExecuteResult, ExecuteStatus};
use super::session::SessionKey;
use crate::cell_options::{merge_cell_over_scope, options_to_config, partition_cell_options};
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
    /// Start byte offset of the code content (the capture between the
    /// fences) in the input — anchors per-cell source attribution.
    code_start: usize,
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
fn parse_code_blocks(input: &str) -> Vec<CodeBlock> {
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
    let mut fig = FigureWriter::new(&ctx.source_path);

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
    if fig.wrote_any {
        result = result.with_supporting_files(vec![fig.files_dir()]);
    }
    Ok(result)
}

/// One cell's options resolved against the document: the cell's `#|`
/// map merged *over* the document's `execute` scope, so a cell option
/// wins wherever both set the same key (Q1's `shouldInclude`, and the
/// scoped resolution bd-ohvl879u introduced for `error:`).
///
/// `None` when neither scope supplied anything — callers then fall
/// back to per-option defaults.
fn resolve_cell_options(
    doc_scope: Option<&ConfigValue>,
    options: Option<quarto_yaml::YamlWithSourceInfo>,
) -> Option<ConfigValue> {
    let cell_config = options.map(|o| {
        let (config, diagnostics) = options_to_config(o);
        for d in diagnostics {
            tracing::warn!("cell option conversion diagnostic: {d:?}");
        }
        config
    });
    merge_cell_over_scope(doc_scope, cell_config.as_ref())
}

/// Read a boolean option out of resolved cell options, falling back to
/// `default` when the key is absent or not a boolean.
fn resolved_flag(resolved: Option<&ConfigValue>, key: &str, default: bool) -> bool {
    resolved
        .and_then(|merged| merged.get(key))
        .and_then(|v| v.as_bool())
        .unwrap_or(default)
}

/// Which parts of a cell reach the output (bd-nn2fou8h).
///
/// Mirrors Q1's `shouldInclude` family (`src/core/jupyter/tags.ts`):
/// each key is resolved cell-over-document and defaults to `true`, so
/// a document that says nothing keeps today's fully-visible behaviour.
///
/// `include: false` is the master switch — Q1 bails on the whole cell
/// before consulting anything else (`mdFromCodeCell`'s
/// `if (!includeCell(...)) return []`), which is why [`Self::resolve`]
/// collapses the other flags rather than leaving callers to remember
/// the precedence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CellVisibility {
    /// Echo the cell's source.
    echo: bool,
    /// Emit the cell's outputs.
    output: bool,
    /// Keep stderr-stream outputs (Python warnings land there).
    warning: bool,
}

impl CellVisibility {
    /// Everything visible — the default for a cell with no options and
    /// a document with no `execute:` scope.
    const VISIBLE: Self = Self {
        echo: true,
        output: true,
        warning: true,
    };

    /// Nothing at all reaches the output (`include: false`).
    const HIDDEN: Self = Self {
        echo: false,
        output: false,
        warning: false,
    };

    fn resolve(resolved: Option<&ConfigValue>) -> Self {
        // Neither the cell nor the document said anything — the
        // everything-visible default, which is also what q2 did
        // before any of these options were honoured.
        if resolved.is_none() {
            return Self::VISIBLE;
        }
        if !resolved_flag(resolved, "include", true) {
            return Self::HIDDEN;
        }
        Self {
            echo: resolved_flag(resolved, "echo", true),
            output: resolved_flag(resolved, "output", true),
            // Q1 filters stderr under `output: false` too — the option
            // suppresses every output, warnings included.
            warning: resolved_flag(resolved, "warning", true)
                && resolved_flag(resolved, "output", true),
        }
    }
}

/// Whether an output is a "warning" for visibility purposes: a stderr
/// stream. Mirrors Q1's `isWarningOutput` in
/// `src/core/jupyter/jupyter.ts` — the kernel gives no richer signal,
/// so stderr is the whole definition.
fn is_warning_output(output: &CellOutput) -> bool {
    matches!(output, CellOutput::Stream { name, .. } if name == "stderr")
}

/// The first error a cell produced, from its outputs or its status.
fn first_cell_error(result: &KernelExecuteResult) -> Option<(String, String)> {
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

/// Render `info` + `offset` as `path:line:column` (1-based) through
/// the execution context's source map, when it resolves.
fn describe_location(info: &SourceInfo, offset: usize, ctx: &ExecutionContext) -> Option<String> {
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
fn render_cell(
    language: &str,
    code: &str,
    result: &KernelExecuteResult,
    fig: &mut FigureWriter,
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
fn fenced_output_div(classes: &str, text: &str) -> String {
    let ticks = ticks_for_code(text);
    format!("\n::: {{{classes}}}\n\n{ticks}\n{text}\n{ticks}\n\n:::\n")
}

/// Writes figure files for one document run (bd-5t6wvu7m).
///
/// Image outputs (`image/png`, `image/jpeg`, `image/svg+xml`) are
/// written to `<doc_dir>/<stem>_files/figure-html/` — the same layout
/// Q1's jupyter engine (`mdImageOutput` + `figuresDir("html")`) and
/// q2's knitr engine produce, and the layout
/// [`JupyterEngine::intermediate_files`](super::JupyterEngine) already
/// declares. Files are named `cell-<cell#>-output-<out#>.<ext>` (Q1's
/// shape). When anything was written, the caller reports the
/// `<stem>_files` directory in `ExecuteResult::supporting_files` —
/// dir-level, mirroring knitr — which is what `q2 render` copies to
/// the output dir and what the preview capture transport
/// (bd-qbhp2cvv) embeds for browser replay.
struct FigureWriter {
    /// Absolute directory of the source document.
    doc_dir: PathBuf,
    /// `<stem>_files` (single path component).
    files_dir_name: String,
    /// 1-based index of the cell currently being rendered. Set by the
    /// block loop before each `render_cell`.
    cell_index: usize,
    /// 1-based index of the next output within the current cell.
    /// Reset by `begin_cell`.
    output_index: usize,
    /// Whether any figure file was successfully written.
    wrote_any: bool,
}

impl FigureWriter {
    fn new(source_path: &Path) -> Self {
        let doc_dir = source_path.parent().unwrap_or(Path::new(".")).to_path_buf();
        let stem = source_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("document");
        Self {
            doc_dir,
            files_dir_name: format!("{stem}_files"),
            cell_index: 0,
            output_index: 0,
            wrote_any: false,
        }
    }

    /// Start figure numbering for the next cell.
    fn begin_cell(&mut self) {
        self.cell_index += 1;
        self.output_index = 0;
    }

    /// The `<stem>_files` directory as an absolute path (for
    /// supporting-files reporting).
    fn files_dir(&self) -> PathBuf {
        self.doc_dir.join(&self.files_dir_name)
    }

    /// Write one image output. Returns the doc-relative, forward-slash
    /// path to emit in markdown, or `None` when the payload is
    /// malformed or the write fails (caller falls back — fail-soft,
    /// matching the engine's other output paths).
    fn write_image(&mut self, mime: &str, value: &serde_json::Value) -> Option<String> {
        let ext = match mime {
            "image/png" => "png",
            "image/jpeg" => "jpeg",
            "image/svg+xml" => "svg",
            _ => return None,
        };
        let content = extract_text_content(value);
        // Q1 rule (`mdImageOutput`): SVG payloads that are literal
        // `<svg` markup are written as text; everything else is
        // base64 (strip the nbformat multiline newlines first).
        let bytes: Vec<u8> = if ext == "svg" && content.trim_start().starts_with("<svg") {
            content.into_bytes()
        } else {
            use base64::Engine as _;
            match base64::engine::general_purpose::STANDARD.decode(content.replace('\n', "")) {
                Ok(bytes) => bytes,
                Err(e) => {
                    tracing::warn!(mime, error = %e, "jupyter figure: invalid base64; skipping");
                    return None;
                }
            }
        };

        self.output_index += 1;
        let file_name = format!(
            "cell-{}-output-{}.{ext}",
            self.cell_index, self.output_index
        );
        let rel = format!("{}/figure-html/{file_name}", self.files_dir_name);
        let abs = self.doc_dir.join(&self.files_dir_name).join("figure-html");
        if let Err(e) = std::fs::create_dir_all(&abs) {
            tracing::warn!(dir = %abs.display(), error = %e, "jupyter figure: cannot create figures dir");
            return None;
        }
        if let Err(e) = std::fs::write(abs.join(&file_name), &bytes) {
            tracing::warn!(file = %file_name, error = %e, "jupyter figure: write failed");
            return None;
        }
        self.wrote_any = true;
        Some(rel)
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
/// `image/svg+xml` → `image/png` → `image/jpeg` → `text/plain`
/// **last** (a matplotlib bundle carries an image plus a text
/// fallback; the image must win). `text/markdown` and jupyter-widget
/// payloads remain unhandled (follow-up).
fn format_outputs(
    result: &KernelExecuteResult,
    fig: &mut FigureWriter,
    show_warnings: bool,
) -> String {
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

    // === bd-nn2fou8h: execute-visibility resolution ===
    //
    // Resolution and emission are unit-testable without a kernel; the
    // end-to-end matrix (both engines, doc and cell scope) lives in
    // `tests/integration/engine_visibility.rs`.

    /// Build resolved cell options from YAML source, as
    /// `resolve_cell_options` would after merging.
    fn resolved(yaml: &str) -> Option<ConfigValue> {
        let parsed = quarto_yaml::parse(yaml).expect("test YAML parses");
        let (config, _) = options_to_config(parsed);
        Some(config)
    }

    fn result_with(outputs: Vec<CellOutput>) -> KernelExecuteResult {
        KernelExecuteResult {
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
        let mut fig = FigureWriter::new(Path::new("/nonexistent/doc.qmd"));
        render_cell("python", code, &result_with(outputs), &mut fig, visibility)
    }

    #[test]
    fn test_visibility_defaults_to_fully_visible() {
        assert_eq!(CellVisibility::resolve(None), CellVisibility::VISIBLE);
        assert_eq!(
            CellVisibility::resolve(resolved("error: true").as_ref()),
            CellVisibility::VISIBLE,
            "an unrelated option must not change visibility"
        );
    }

    #[test]
    fn test_visibility_reads_each_key() {
        assert!(!CellVisibility::resolve(resolved("echo: false").as_ref()).echo);
        assert!(!CellVisibility::resolve(resolved("output: false").as_ref()).output);
        assert!(!CellVisibility::resolve(resolved("warning: false").as_ref()).warning);
        // Independence: hiding one must not hide the others.
        let v = CellVisibility::resolve(resolved("echo: false").as_ref());
        assert!(v.output && v.warning);
    }

    #[test]
    fn test_include_false_hides_everything() {
        assert_eq!(
            CellVisibility::resolve(resolved("include: false").as_ref()),
            CellVisibility::HIDDEN
        );
        // include: false wins even when another key says "show me".
        assert_eq!(
            CellVisibility::resolve(resolved("include: false\necho: true").as_ref()),
            CellVisibility::HIDDEN
        );
    }

    /// `output: false` suppresses every output, warnings included — so
    /// a cell must not leak stderr through the warning channel after
    /// its outputs were switched off.
    #[test]
    fn test_output_false_implies_warnings_hidden() {
        let v = CellVisibility::resolve(resolved("output: false\nwarning: true").as_ref());
        assert!(!v.warning, "output: false must also silence warnings");
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

    /// A `FigureWriter` rooted in a fresh temp dir, positioned at cell 1.
    /// The temp dir is leaked for the process lifetime — tests that
    /// assert on written files create their own named `TempDir` instead.
    fn test_fig() -> FigureWriter {
        let dir = tempfile::tempdir().unwrap();
        let mut fig = FigureWriter::new(&dir.path().join("doc.qmd"));
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
        let mut fig = FigureWriter::new(&tmp.path().join("pyfig.qmd"));
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
        assert!(fig.wrote_any);
        assert_eq!(fig.files_dir(), tmp.path().join("pyfig_files"));
    }

    #[test]
    fn test_svg_markup_written_as_text_with_svg_extension() {
        // Q1's mdImageOutput rule: literal `<svg` payloads are written
        // as text, not base64-decoded.
        let tmp = tempfile::tempdir().unwrap();
        let mut fig = FigureWriter::new(&tmp.path().join("doc.qmd"));
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

    #[test]
    fn test_two_outputs_in_one_cell_get_distinct_names() {
        let tmp = tempfile::tempdir().unwrap();
        let mut fig = FigureWriter::new(&tmp.path().join("doc.qmd"));
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
