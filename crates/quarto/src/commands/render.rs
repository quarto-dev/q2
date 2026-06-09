/*
 * render.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Render command implementation
 */

//! Render command implementation.
//!
//! This module implements the `quarto render` command, which renders
//! QMD files to various output formats.
//!
//! The actual render logic is in `quarto_core::render_to_file`. This module
//! handles CLI-specific concerns: argument parsing, console output, target
//! classification, and `--clean-cache` execution.
//!
//! ## Phase 8.4 surface
//!
//! - [`classify_inputs`] turns raw CLI argument strings into a
//!   [`RenderTarget`] (Mode A / Mode B / single-doc / error). Pure
//!   over a `&dyn SystemRuntime`, easy to unit-test.
//! - [`run_clean_cache`] wipes the Pass-1 profile cache and the
//!   nav-config-hash sentinel before a render. No-op if the runtime
//!   has no cache directory configured.
//! - [`render_summary_line`] computes the trailing
//!   `"N of M rendered"` line, suppressed for single-file renders.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use tracing::info;

use quarto_core::attribution::AttributionMode;
use quarto_core::project::orchestrator::{
    DiagnosticCounts, ProjectPipeline, RenderMode, project_type_for,
};
use quarto_core::{Format, ProjectContext, QuartoError, RenderToFileOptions};
use quarto_error_reporting::{
    DiagnosticMessage, DiagnosticMessageBuilder, JsonDiagnostic, JsonPass1Failure,
    diagnostic_to_json, with_source_file,
};
use quarto_source_map::SourceContext;
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

/// Arguments for the render command.
#[derive(Debug)]
pub struct RenderArgs {
    /// Input paths (zero or more). Each is a `.qmd` file, a
    /// directory, or (after the shell has expanded it) the result of
    /// a glob. An empty `inputs` means "render the project rooted at
    /// the current working directory."
    pub inputs: Vec<String>,
    /// Output format.
    pub to: Option<String>,
    /// Output file path.
    pub output: Option<String>,
    /// Output directory.
    pub output_dir: Option<String>,
    /// Wipe the Pass-1 profile cache (`<project>/.quarto/cache/profiles/`)
    /// and the `nav-config-hash` sentinel before rendering. The `sass/`
    /// namespace is preserved.
    pub clean_cache: bool,
    /// Suppress console output.
    pub quiet: bool,
    /// Replay engine output from a recorded trace file instead of
    /// running the real engine (bd-45yw). When `Some`, the path is
    /// loaded via `quarto_trace::read::read_trace` and the recorded
    /// `EngineCapture` is plumbed through `RenderToFileOptions`.
    /// Falls through to the `QUARTO_REPLAY` env var if `None`.
    pub replay: Option<String>,
    /// Leave intermediate files (not yet implemented).
    #[allow(dead_code)]
    pub debug: bool,
    /// Resolved attribution mode from the `--attribution=<value>`
    /// flag. `None` means "absent — defer to YAML." Phase 3c.
    pub attribution: Option<AttributionMode>,
    /// Emit diagnostics as NDJSON on stderr instead of human-readable
    /// ariadne text (bd-iey8o). Each emitted line carries a `$schema`
    /// field; the schema docs live in
    /// `crates/quarto-error-reporting/schemas/`. Exit codes are
    /// unchanged.
    pub json_errors: bool,
    /// Stop rendering as soon as the first per-document error occurs
    /// (Pass-1 or Pass-2), instead of best-effort rendering everything.
    /// Threaded into the orchestrator via
    /// [`ProjectPipeline::with_fail_fast`]. Useful for an iterative
    /// fix loop. Exit code is unchanged (any failure → non-zero).
    pub fail_fast: bool,
}

/// What to render after argument classification.
///
/// Produced by [`classify_inputs`]; consumed by [`execute`] to pick a
/// dispatch path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RenderTarget {
    /// A single `.qmd` file with no surrounding `_quarto.yml`. Falls
    /// through to the existing single-document render path.
    SingleDoc(PathBuf),
    /// Render the entire project rooted at `project_dir` (Mode A in
    /// the Phase 8 vocabulary).
    FullProject { project_dir: PathBuf },
    /// Render exactly `targets` inside `project_dir` (Mode B). Each
    /// path in `targets` is absolute and corresponds to a member of
    /// the project's render-list-filtered file list.
    Subset {
        project_dir: PathBuf,
        targets: Vec<PathBuf>,
    },
}

/// Errors surfaced by [`classify_inputs`].
///
/// Each variant carries enough structured context that the CLI layer
/// can format a clear, actionable message. Tests pin the variants
/// rather than the rendered strings.
#[derive(Debug)]
pub enum DispatchError {
    /// One of the input paths does not exist on disk.
    PathNotFound(PathBuf),
    /// No path arguments were provided and the cwd is not a project
    /// (no `_quarto.yml` found walking upward). The user has to name
    /// at least one file.
    NoInputAndNoProject(PathBuf),
    /// More than one input was provided and none are inside a
    /// `_quarto.yml`-rooted project. We require "one project per
    /// render" — see Phase 8.4 design log.
    MultiArgNonProject,
    /// Inputs resolved to two different project roots. We render one
    /// project at a time.
    MultiProjectArgs { first: PathBuf, second: PathBuf },
    /// An explicit `.qmd` argument is excluded from the project's
    /// render list (`project.render` glob match, or excluded by the
    /// underscore/hidden/README rules in [`discover_project_files`]).
    NotInRenderList { path: PathBuf, project_dir: PathBuf },
    /// A directory or glob argument expanded to zero renderable files
    /// after the project's render list is applied.
    NoRenderableMatches { path: PathBuf },
    /// Project discovery failed for unrelated reasons (parse error in
    /// `_quarto.yml`, I/O error, etc.).
    Discover(String),
}

impl std::fmt::Display for DispatchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DispatchError::PathNotFound(p) => {
                write!(f, "Input path does not exist: {}", p.display())
            }
            DispatchError::NoInputAndNoProject(cwd) => write!(
                f,
                "No input given and no `_quarto.yml` found at or above {}",
                cwd.display()
            ),
            DispatchError::MultiArgNonProject => write!(
                f,
                "Multiple input paths require a project (a `_quarto.yml` rooted directory). \
                 To render a single standalone file, pass exactly one path."
            ),
            DispatchError::MultiProjectArgs { first, second } => write!(
                f,
                "Input paths span more than one project: {} and {}. \
                 Render one project at a time.",
                first.display(),
                second.display()
            ),
            DispatchError::NotInRenderList { path, project_dir } => write!(
                f,
                "{} is excluded from the render list of project {} \
                 (check `project.render` in `_quarto.yml` and the \
                 underscore/hidden file conventions).",
                path.display(),
                project_dir.display()
            ),
            DispatchError::NoRenderableMatches { path } => {
                write!(f, "No renderable `.qmd` files matched: {}", path.display())
            }
            DispatchError::Discover(msg) => write!(f, "Project discovery failed: {msg}"),
        }
    }
}

impl std::error::Error for DispatchError {}

/// Classify CLI input strings into a [`RenderTarget`].
///
/// `cwd` is the working directory the CLI was invoked from. When
/// `inputs` is empty, project discovery starts there. When `inputs`
/// is non-empty, each is resolved relative to `cwd` and canonicalized
/// before further processing.
///
/// The function is pure over the runtime — no global state, no cache
/// or output writes. All filesystem access goes through `runtime`.
pub fn classify_inputs(
    inputs: &[String],
    cwd: &Path,
    runtime: &dyn SystemRuntime,
) -> std::result::Result<RenderTarget, DispatchError> {
    if inputs.is_empty() {
        return classify_no_inputs(cwd, runtime);
    }

    // Step 1: canonicalize each input and verify it exists.
    let resolved: Vec<PathBuf> = inputs
        .iter()
        .map(|s| {
            let p = path_relative_to(s, cwd);
            let exists = runtime
                .path_exists(&p, None)
                .map_err(|e| DispatchError::Discover(e.to_string()))?;
            if !exists {
                return Err(DispatchError::PathNotFound(p));
            }
            runtime
                .canonicalize(&p)
                .map_err(|e| DispatchError::Discover(e.to_string()))
        })
        .collect::<std::result::Result<Vec<_>, _>>()?;

    // Step 2: discover each input's project root and verify uniformity.
    // For each input, walk up looking for `_quarto.yml`. None ⇒ no
    // project. We require all inputs to either share one project, or
    // (single-arg case) sit alone outside any project.
    let mut shared_project: Option<PathBuf> = None;
    let mut any_outside_project = false;
    for r in &resolved {
        let ctx = ProjectContext::discover(r, runtime)
            .map_err(|e| DispatchError::Discover(e.to_string()))?;
        // `is_single_file` only fires for *file* inputs with no
        // surrounding `_quarto.yml`. A *directory* input never trips
        // it, even when no config exists — so we re-check the
        // filesystem for a real project marker.
        if !is_real_project(&ctx, runtime) {
            any_outside_project = true;
            if let Some(p) = &shared_project {
                return Err(DispatchError::MultiProjectArgs {
                    first: p.clone(),
                    second: r.clone(),
                });
            }
        } else {
            match &shared_project {
                None => shared_project = Some(ctx.dir.clone()),
                Some(p) if p == &ctx.dir => {}
                Some(p) => {
                    return Err(DispatchError::MultiProjectArgs {
                        first: p.clone(),
                        second: ctx.dir.clone(),
                    });
                }
            }
        }
    }

    // If any input lives outside a project, it must be the *only*
    // input. (Mixed "in-project" and "outside-project" inputs were
    // rejected by the MultiProjectArgs check above when the in-project
    // case appeared first; symmetric rejection here for the order
    // where outside-project comes first.)
    if any_outside_project {
        if resolved.len() > 1 {
            return Err(DispatchError::MultiArgNonProject);
        }
        // Single arg outside any project: must be a `.qmd` file
        // (single-doc fallthrough). Directories outside any project
        // are not a meaningful render target — we error.
        let only = &resolved[0];
        let is_dir = runtime
            .is_dir(only)
            .map_err(|e| DispatchError::Discover(e.to_string()))?;
        if is_dir {
            return Err(DispatchError::NoRenderableMatches { path: only.clone() });
        }
        return Ok(RenderTarget::SingleDoc(only.clone()));
    }

    // All inputs are inside one project.
    let project_dir = shared_project.expect("shared_project set when no inputs are outside");

    // Re-discover from the project root to get the full
    // render-list-filtered file list. (Per-input `discover` only fills
    // `files` with that one input.)
    let project = ProjectContext::discover(&project_dir, runtime)
        .map_err(|e| DispatchError::Discover(e.to_string()))?;
    let project_files: Vec<PathBuf> = project.files.iter().map(|f| f.input.clone()).collect();

    // Step 3: expand each input into the set of project files it
    // covers, intersected with the render list.
    let mut targets: Vec<PathBuf> = Vec::new();
    let mut seen: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();
    let mut full_project_arg = false;
    for r in &resolved {
        let is_dir = runtime
            .is_dir(r)
            .map_err(|e| DispatchError::Discover(e.to_string()))?;
        if is_dir {
            // Directory pointing at the project root → Mode A.
            if r == &project_dir {
                full_project_arg = true;
                continue;
            }
            // Subdirectory → render-list-filtered set under it.
            let mut matched = 0usize;
            for f in &project_files {
                if path_starts_with(f, r) && seen.insert(f.clone()) {
                    targets.push(f.clone());
                    matched += 1;
                }
            }
            if matched == 0 {
                return Err(DispatchError::NoRenderableMatches { path: r.clone() });
            }
        } else {
            // File arg. Must be in the project's render-list-filtered
            // file list.
            if !project_files.iter().any(|p| p == r) {
                return Err(DispatchError::NotInRenderList {
                    path: r.clone(),
                    project_dir: project_dir.clone(),
                });
            }
            if seen.insert(r.clone()) {
                targets.push(r.clone());
            }
        }
    }

    // If any arg pointed at the project root, treat the whole call as
    // Mode A. (Mixing "project-root" and "subset" args is a confused
    // request; the project-root arg dominates because it's the more
    // inclusive intent.)
    if full_project_arg {
        return Ok(RenderTarget::FullProject { project_dir });
    }

    // If the targets cover the entire project, also collapse to Mode A.
    // Mode-B-with-all-files is functionally identical but goes through
    // the dependency-graph augmentation, which is wasted work.
    if targets.len() == project_files.len() {
        return Ok(RenderTarget::FullProject { project_dir });
    }

    Ok(RenderTarget::Subset {
        project_dir,
        targets,
    })
}

fn classify_no_inputs(
    cwd: &Path,
    runtime: &dyn SystemRuntime,
) -> std::result::Result<RenderTarget, DispatchError> {
    let cwd_canon = runtime
        .canonicalize(cwd)
        .map_err(|e| DispatchError::Discover(e.to_string()))?;

    // Check cheaply for an ancestor `_quarto.yml` before delegating
    // to `ProjectContext::discover`. Without this short-circuit
    // (bd-nmkmi), `discover` would fall through to
    // `discover_project_files`, which recursively walks the cwd
    // collecting `.qmd` files — a multi-second freeze in any
    // directory containing a large unrelated tree (e.g. `target/`),
    // and the walk's output is then thrown away when we discover
    // there is no project anyway.
    let Some(project_root) = find_project_root_upward(&cwd_canon, runtime)? else {
        return Err(DispatchError::NoInputAndNoProject(cwd_canon));
    };

    let project = ProjectContext::discover(&project_root, runtime)
        .map_err(|e| DispatchError::Discover(e.to_string()))?;
    Ok(RenderTarget::FullProject {
        project_dir: project.dir,
    })
}

/// Walk `start` and its ancestors looking for a project config
/// (`_quarto.yml` or `_quarto.yaml`). Returns the directory that
/// contains the first match, or `None` if the filesystem root is
/// reached without one.
///
/// This is the cheap half of `ProjectContext::find_project_config`
/// (parsing-free, `path_exists`-only). `classify_no_inputs` uses it
/// to gate the expensive directory walk that `ProjectContext::discover`
/// would otherwise perform on a project-less cwd.
fn find_project_root_upward(
    start: &Path,
    runtime: &dyn SystemRuntime,
) -> std::result::Result<Option<PathBuf>, DispatchError> {
    let mut current = start.to_path_buf();
    loop {
        for name in ["_quarto.yml", "_quarto.yaml"] {
            let candidate = current.join(name);
            let exists = runtime
                .path_exists(&candidate, None)
                .map_err(|e| DispatchError::Discover(e.to_string()))?;
            if exists {
                return Ok(Some(current));
            }
        }
        match current.parent() {
            Some(parent) => current = parent.to_path_buf(),
            None => return Ok(None),
        }
    }
}

/// True when a real `_quarto.yml` (or `_quarto.yaml`) sits at the
/// discovered project root. `ProjectContext::discover(dir)` returns
/// `is_single_file = false` for any *directory* input, even one with
/// no config — so `is_single_file` alone can't tell us whether we're
/// inside a real project. We re-check the filesystem.
fn is_real_project(project: &ProjectContext, runtime: &dyn SystemRuntime) -> bool {
    let yml = project.dir.join("_quarto.yml");
    let yaml = project.dir.join("_quarto.yaml");
    runtime.path_exists(&yml, None).unwrap_or(false)
        || runtime.path_exists(&yaml, None).unwrap_or(false)
}

fn path_relative_to(s: &str, cwd: &Path) -> PathBuf {
    let p = PathBuf::from(s);
    if p.is_absolute() { p } else { cwd.join(p) }
}

fn path_starts_with(child: &Path, ancestor: &Path) -> bool {
    let mut c = child.components();
    for a in ancestor.components() {
        match c.next() {
            Some(seg) if seg == a => {}
            _ => return false,
        }
    }
    true
}

/// Wipe the Pass-1 profile cache and the nav-config-hash sentinel.
///
/// Preserves the `sass/` namespace (Phase 5 SCSS pre-compile cache —
/// expensive and almost never the source of incorrectness). No-op if
/// the runtime has no cache directory.
///
/// `project_dir` is the project root; the nav-config-hash file lives
/// at `<project_dir>/.quarto/cache/nav-config-hash`. Failure to remove
/// a missing nav-config-hash file is not an error.
pub fn run_clean_cache(
    runtime: &dyn SystemRuntime,
    project_dir: &Path,
) -> std::result::Result<(), String> {
    pollster::block_on(async {
        runtime
            .cache_clear_namespace("profiles")
            .await
            .map_err(|e| format!("failed to clear profile cache: {e}"))?;
        let hash_path = project_dir.join(".quarto/cache/nav-config-hash");
        let exists = runtime
            .path_exists(&hash_path, None)
            .map_err(|e| format!("failed to check nav-config-hash path: {e}"))?;
        if exists {
            runtime
                .file_remove(&hash_path)
                .map_err(|e| format!("failed to remove nav-config-hash: {e}"))?;
        }
        Ok::<(), String>(())
    })
}

/// Compute the trailing summary line for a render.
///
/// Returns `None` for single-file renders: the per-file output path is
/// already obvious. Multi-file projects always get a summary line, even
/// when zero pages rendered (e.g. all skipped due to errors).
pub fn render_summary_line(
    is_single_file: bool,
    total_files: usize,
    rendered: usize,
    output_dir: &Path,
) -> Option<String> {
    if is_single_file {
        return None;
    }
    Some(format!(
        "Rendered {rendered} of {total_files} files to {}",
        output_dir.display()
    ))
}

/// Format the trailing "N errors, M warnings" clause for a render
/// summary (bd-ooleh), or `None` when the run was clean.
///
/// Counts are diagnostic-level (see [`DiagnosticCounts`]) — an estimate
/// of how many problems remain before a clean build. A zero count drops
/// its clause entirely: `"3 errors"` on its own when there are no
/// warnings, `"5 warnings"` on its own when there are no errors.
///
/// When `color` is true the error count is rendered red and the warning
/// count yellow (raw ANSI SGR codes, matching the diagnostic palette).
/// Callers pass `false` for non-tty, `NO_COLOR`, or `--json-errors`.
fn format_counts_clause(counts: &DiagnosticCounts, color: bool) -> Option<String> {
    if counts.is_empty() {
        return None;
    }
    let mut parts: Vec<String> = Vec::new();
    if counts.errors > 0 {
        parts.push(colorize(
            &count_phrase(counts.errors, "error"),
            ANSI_RED,
            color,
        ));
    }
    if counts.warnings > 0 {
        parts.push(colorize(
            &count_phrase(counts.warnings, "warning"),
            ANSI_YELLOW,
            color,
        ));
    }
    Some(parts.join(", "))
}

/// `"1 error"` / `"2 errors"` — English pluralization for a count noun.
fn count_phrase(n: usize, noun: &str) -> String {
    if n == 1 {
        format!("1 {noun}")
    } else {
        format!("{n} {noun}s")
    }
}

const ANSI_RED: &str = "31";
const ANSI_YELLOW: &str = "33";

/// Wrap `text` in an ANSI SGR color code when `color` is true; return it
/// unchanged otherwise.
fn colorize(text: &str, sgr_code: &str, color: bool) -> String {
    if color {
        format!("\x1b[{sgr_code}m{text}\x1b[0m")
    } else {
        text.to_string()
    }
}

/// Whether the error/warning summary should use ANSI color on stderr:
/// stderr is a terminal and `NO_COLOR` is unset. (`--json-errors`
/// suppression is handled separately at the call site.)
fn stderr_use_color() -> bool {
    use std::io::IsTerminal;
    std::env::var_os("NO_COLOR").is_none() && std::io::stderr().is_terminal()
}

/// bd-45yw: resolve the replay activation source and load the trace.
///
/// `cli_replay` is the value of `--replay`. If `None`, falls through
/// to the `QUARTO_REPLAY` environment variable. If neither is set,
/// returns `Ok(None)` — replay is inactive.
///
/// Returns a hard error (no fallback) if:
/// - The trace file cannot be read (missing path, permission denied,
///   malformed JSON).
/// - The trace lacks any `engine_captures` — replaying a trace that was
///   recorded without engine capture would silently behave like the
///   markdown engine, which is exactly the kind of "silent degradation"
///   the hard-fail policy guards against.
///
/// Returns the captures in execution order (one per engine that ran);
/// an empty vec means replay is inactive (no `--replay` / `QUARTO_REPLAY`).
fn load_replay_captures(cli_replay: Option<&str>) -> Result<Vec<quarto_trace::EngineCapture>> {
    let path_str = match cli_replay {
        Some(p) => Some(p.to_string()),
        None => std::env::var("QUARTO_REPLAY").ok(),
    };
    let Some(path_str) = path_str else {
        return Ok(Vec::new());
    };

    let path = PathBuf::from(&path_str);
    let trace = quarto_trace::read::read_trace(&path).with_context(|| {
        format!(
            "Failed to load replay trace from {}. \
             Pass a path to a quarto trace JSON file (the kind produced by `trace: true`).",
            path.display()
        )
    })?;

    if trace.engine_captures.is_empty() {
        return Err(anyhow::anyhow!(
            "Trace file {} has no `engine_captures`. \
             Re-record the trace from a real engine run with `trace: true` set.",
            path.display()
        ));
    }

    Ok(trace.engine_captures)
}

/// Execute the render command.
pub fn execute(args: RenderArgs) -> Result<()> {
    // Create the system runtime
    let runtime = NativeRuntime::new();

    let cwd = runtime
        .cwd()
        .map_err(|e| anyhow::anyhow!("Failed to get current directory: {}", e))?;

    // Determine the target format. An explicit `--to` always wins; otherwise
    // resolve from the document's front-matter `format:` (matching Quarto 1,
    // which reads the front-matter to choose output formats). Falling back to
    // `"html"` only when neither is present. Single-format-per-render for now;
    // multi-format/project resolution is a later phase
    // (claude-notes/plans/2026-06-08-revealjs-presentations.md, decision 5).
    let format_str: String = match args.to.as_deref() {
        Some(to) => to.to_string(),
        None => detect_single_input_format(&args.inputs).unwrap_or_else(|| "html".to_string()),
    };
    let format = resolve_format(&format_str)?;

    // Native formats (HTML, revealjs) render in-process; non-native formats
    // still need Pandoc and are not yet supported.
    if !format.identifier.is_native() {
        anyhow::bail!(
            "Format '{}' is not yet supported. Only HTML and revealjs are available in this version.",
            format.identifier
        );
    }

    // Classify inputs into a render target. Under --json-errors,
    // surface the structured DispatchError as a JsonDiagnostic on
    // stderr before exiting so machine consumers can discriminate
    // by code (Q-7-2..8).
    let target = match classify_inputs(&args.inputs, &cwd, &runtime) {
        Ok(t) => t,
        Err(e) => {
            if args.json_errors {
                emit_dispatch_error_json(&e);
                std::process::exit(1);
            }
            return Err(anyhow::anyhow!("{}", e));
        }
    };

    // bd-45yw: load replay capture if --replay <path> or
    // QUARTO_REPLAY=<path> is set. Hard-fail on read errors and on
    // traces missing engine_capture so investigators don't waste
    // time on a silently-degraded replay.
    let replay_captures = load_replay_captures(args.replay.as_deref())?;

    // Set up render options
    let options = RenderToFileOptions {
        output_path: args.output.as_ref().map(PathBuf::from),
        output_dir: args.output_dir.as_ref().map(PathBuf::from),
        quiet: args.quiet,
        replay_captures,
        engine_registry_override: None,
        // Phase 3c: CLI override-only for v1 (YAML reading is deferred
        // until project YAML schema work; the resolver matrix is still
        // exercised end-to-end by Phase 0 test #9b).
        attribution: args.attribution,
    };

    match target {
        RenderTarget::SingleDoc(input) => execute_single_doc(input, &args, &options, format),
        RenderTarget::FullProject { project_dir } => {
            execute_project(project_dir, None, &args, &options, format, &format_str)
        }
        RenderTarget::Subset {
            project_dir,
            targets,
        } => execute_project(
            project_dir,
            Some(targets),
            &args,
            &options,
            format,
            &format_str,
        ),
    }
}

fn execute_single_doc(
    input: PathBuf,
    args: &RenderArgs,
    options: &RenderToFileOptions,
    format: Format,
) -> Result<()> {
    let runtime_arc: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let mut project = ProjectContext::discover(&input, runtime_arc.as_ref())
        .context("Failed to discover project context")?;

    quarto_util::user_status!(args.quiet, "Rendering single file: {}", input.display());

    let project_type = project_type_for(&project);
    let format_str = format.identifier.to_string();
    let mut pipeline = ProjectPipeline::new(
        &mut project,
        project_type,
        format,
        &format_str,
        options,
        runtime_arc.clone(),
    )
    .with_format_override(args.to.clone())
    .with_fail_fast(args.fail_fast);

    let summary = match pollster::block_on(pipeline.run()) {
        Ok(s) => s,
        Err(QuartoError::Parse(parse_error)) => {
            if args.json_errors {
                emit_parse_error_json(&parse_error, Some(&input));
            } else {
                eprintln!("{}", parse_error);
            }
            std::process::exit(1);
        }
        Err(e) => return Err(anyhow::anyhow!("{}", e)),
    };

    print_render_diagnostics(&summary, args);

    // bd-ooleh: a single-file render has no "Rendered N of M" line to
    // augment, so the error/warning counts get their own line — printed
    // only when there is something to report (clean runs stay silent)
    // and never under the machine-readable `--json-errors` path.
    if !args.json_errors {
        if let Some(clause) = format_counts_clause(&summary.diagnostic_counts(), stderr_use_color())
        {
            quarto_util::user_status!(args.quiet, "{}", clause);
        }
    }

    if should_exit_nonzero(&summary) {
        std::process::exit(1);
    }
    Ok(())
}

fn execute_project(
    project_dir: PathBuf,
    targets: Option<Vec<PathBuf>>,
    args: &RenderArgs,
    options: &RenderToFileOptions,
    format: Format,
    format_str: &str,
) -> Result<()> {
    // Build a runtime with cache_dir wired up — same as the pre-Phase-8
    // single-file vs project shape decision.
    let runtime_arc: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::with_cache_dir(
        project_dir.join(".quarto/cache"),
    ));

    if args.clean_cache {
        run_clean_cache(runtime_arc.as_ref(), &project_dir).map_err(|e| anyhow::anyhow!("{e}"))?;
    }

    let mut project = ProjectContext::discover(&project_dir, runtime_arc.as_ref())
        .context("Failed to discover project context")?;

    quarto_util::user_status!(
        args.quiet,
        "Rendering project: {} (type: {})",
        project.dir.display(),
        project.project_kind().as_str()
    );

    let project_type = project_type_for(&project);
    let total_files = project.files.len();

    let mut pipeline = ProjectPipeline::new(
        &mut project,
        project_type,
        format,
        format_str,
        options,
        runtime_arc.clone(),
    )
    .with_format_override(args.to.clone())
    .with_fail_fast(args.fail_fast);

    if let Some(target_set) = targets {
        let set: std::collections::HashSet<PathBuf> = target_set.into_iter().collect();
        pipeline = pipeline.with_mode(RenderMode::Subset(set));
    }

    let summary = match pollster::block_on(pipeline.run()) {
        Ok(s) => s,
        Err(QuartoError::Parse(parse_error)) => {
            if args.json_errors {
                emit_parse_error_json(&parse_error, None);
            } else {
                eprintln!("{}", parse_error);
            }
            std::process::exit(1);
        }
        Err(e) => return Err(anyhow::anyhow!("{}", e)),
    };

    print_render_diagnostics(&summary, args);

    let rendered = summary.outputs.len();
    if let Some(mut line) = render_summary_line(false, total_files, rendered, &project.output_dir) {
        // bd-ooleh: augment the existing summary line with the total
        // error/warning counts when there is something to report.
        // Suppressed under `--json-errors` (machine consumers tally for
        // themselves); the clean-run case drops the clause entirely.
        if !args.json_errors {
            if let Some(clause) =
                format_counts_clause(&summary.diagnostic_counts(), stderr_use_color())
            {
                line = format!("{line} — {clause}");
            }
        }
        quarto_util::user_status!(args.quiet, "{}", line);
    }

    if should_exit_nonzero(&summary) {
        std::process::exit(1);
    }
    Ok(())
}

/// Strict exit policy for `quarto render` (Decision D1, bd-creo).
///
/// Any file that failed Pass-1 (parse / metadata error) or Pass-2
/// (renderer error) forces a non-zero exit. The orchestrator itself
/// stays policy-free — partial-progress leniency lives in the
/// `quarto preview` / hub-client consumer, not here.
///
/// Project-level diagnostics with [`DiagnosticKind::Error`] severity
/// (e.g. `Q-PROJECT-EMPTY` when the render set is empty — bd-h736)
/// also force a non-zero exit; the user almost certainly wanted the
/// previous behavior of "render the named file" rather than a silent
/// zero-byte success. Warning / Info severity diagnostics are
/// printed but do not affect the exit code.
fn should_exit_nonzero(summary: &quarto_core::project::orchestrator::ProjectRenderSummary) -> bool {
    if !summary.pass1_failures.is_empty() || !summary.pass2_failures.is_empty() {
        return true;
    }
    summary
        .project_diagnostics
        .iter()
        .any(|d| d.kind == quarto_error_reporting::DiagnosticKind::Error)
}

fn print_render_diagnostics(
    summary: &quarto_core::project::orchestrator::ProjectRenderSummary,
    args: &RenderArgs,
) {
    if args.json_errors {
        print_render_diagnostics_json(summary);
    } else {
        print_render_diagnostics_text(summary, args.quiet);
    }

    // bd-c5u2g: emit per-process engine-discovery counters when
    // QUARTO_PERF_STATS=1. No-op otherwise. Placed before any
    // subsequent process::exit so the gauge always lands.
    quarto_core::engine::print_discovery_stats_if_enabled();
    // bd-m7x9s: pass-1 docs / threads_used / wall_ms gauge.
    quarto_core::project::orchestrator::print_pass1_stats_if_enabled();
    // bd-3gj56: pass-2 docs / threads_used / wall_ms gauge.
    quarto_core::project::orchestrator::print_pass2_stats_if_enabled();
}

/// Text path: the existing ariadne-formatted output. Kept verbatim
/// from before bd-iey8o; the only change is that the perf-stats
/// emission moved one level up into `print_render_diagnostics`.
fn print_render_diagnostics_text(
    summary: &quarto_core::project::orchestrator::ProjectRenderSummary,
    quiet: bool,
) {
    for failure in &summary.pass1_failures {
        eprintln!(
            "warning: profile-pass skipped {}: {}",
            failure.input.display(),
            failure.error
        );
    }
    // bd-9hlja: coalesce pass2_failures whose structured diagnostics
    // share a source location, so a single bad key in _quarto.yml
    // emits one report listing affected pages rather than N copies.
    // Non-structured failures (no diagnostics) take the legacy
    // single-line form as before.
    //
    // Per-page warning diagnostics from successful renders
    // (`outputs[i].render_output.diagnostics`) are not coalesced
    // here in v1 because `RenderToFileResult` does not currently
    // carry the input path; the theme-error use case lives entirely
    // in `pass2_failures`. A follow-up could add `input_path` to
    // `RenderToFileResult` and route the successful-render
    // diagnostics through the coalescer too.
    {
        use quarto_error_reporting::coalesce_by_source;

        let mut legacy_failures: Vec<&_> = Vec::new();
        let entries = summary
            .pass2_failures
            .iter()
            .filter_map(|f| {
                if f.diagnostics.is_empty() {
                    legacy_failures.push(f);
                    None
                } else {
                    Some(f)
                }
            })
            .flat_map(|f| {
                f.diagnostics
                    .iter()
                    .map(move |d| (f.input.clone(), d.clone(), f.source_context.clone()))
            });
        for group in coalesce_by_source(entries) {
            eprintln!("{}", group.to_text());
        }
        for failure in legacy_failures {
            eprintln!("error: {}: {}", failure.input.display(), failure.error);
        }
    }
    for diagnostic in &summary.project_diagnostics {
        eprintln!("{}", diagnostic.to_text(None));
    }

    for result in &summary.outputs {
        if !quiet && !result.render_output.diagnostics.is_empty() {
            for diagnostic in &result.render_output.diagnostics {
                eprintln!(
                    "{}",
                    diagnostic.to_text(Some(&result.render_output.source_context))
                );
            }
        }

        // Per-file output line stays on `tracing::info!` (opt-in via
        // `-v`); enumerating every file is too noisy for a large
        // site, and the post-render summary covers the common case.
        if !quiet {
            info!("Output: {}", result.output_path.display());
        }
    }

    // bd-gi25b: when `--fail-fast` cut the render short, tell the user
    // the report isn't exhaustive. Only the text path carries this
    // note — the `--json-errors` path gets no extra field (a
    // programmatic caller passing `--fail-fast` already knows the
    // semantics).
    if summary.stopped_early {
        eprintln!("note: stopped at first error (--fail-fast); remaining files not checked");
    }
}

/// Resolve format string to Format (without metadata)
fn resolve_format(format_str: &str) -> Result<Format> {
    Format::from_format_string(format_str).map_err(|e| anyhow::anyhow!("{}", e))
}

/// When `--to` is absent, detect the target format from a single `.qmd`
/// input's front-matter `format:` key. Returns `None` for project renders
/// (multiple inputs or a directory) — per-file format inside a project is a
/// later phase. Best-effort: any read/parse failure yields `None` (caller
/// falls back to `"html"`).
fn detect_single_input_format(inputs: &[String]) -> Option<String> {
    if inputs.len() != 1 {
        return None;
    }
    let path = std::path::Path::new(&inputs[0]);
    if path.extension().and_then(|e| e.to_str()) != Some("qmd") || !path.is_file() {
        return None;
    }
    let content = std::fs::read_to_string(path).ok()?;
    // Shared with the project pipeline's per-document format resolution so the
    // single-file and project paths agree on how `format:` is read.
    quarto_core::format::format_key_from_frontmatter(&content)
}

// ====================================================================
// --json-errors emission (bd-iey8o)
//
// All functions in this section write one JSON object per line to
// stderr. The wire shapes are `JsonDiagnostic` and `JsonPass1Failure`
// from `quarto-error-reporting`; each emitted object carries a
// `$schema` field pointing at the JSON Schema in
// `crates/quarto-error-reporting/schemas/`.
//
// Stdout is NEVER written by this module — the render outputs still
// go to disk (or to stdout via `--output -`); the diagnostics stream
// is stderr-only so machine consumers can split clearly.
// ====================================================================

/// JSON branch of `print_render_diagnostics`. Mirrors the structure
/// of the text branch (one source of diagnostics at a time) but
/// emits NDJSON instead.
fn print_render_diagnostics_json(
    summary: &quarto_core::project::orchestrator::ProjectRenderSummary,
) {
    // Pass-1 failures: emit one JsonPass1Failure per failure. If the
    // failure has structured diagnostics + a source context, attach
    // them in JsonDiagnostic form (each tagged with the source
    // file). If not, the wrapper still carries the source_file and
    // error string so the agent can route the error.
    for failure in &summary.pass1_failures {
        let source_file = failure.input.display().to_string();
        let diagnostics: Vec<JsonDiagnostic> = match &failure.source_context {
            Some(ctx) => failure
                .diagnostics
                .iter()
                .map(|d| with_source_file(diagnostic_to_json(d, ctx), source_file.clone()))
                .collect(),
            None => Vec::new(),
        };
        emit_json_line(&JsonPass1Failure::new(
            source_file,
            failure.error.clone(),
            diagnostics,
        ));
    }

    // Pass-2 failures: emit each structured diagnostic as its own
    // JsonDiagnostic line. For failures with no structured
    // diagnostics, synthesize a single JsonDiagnostic from the
    // error string and the input path.
    for failure in &summary.pass2_failures {
        if failure.diagnostics.is_empty() {
            let source_file = failure.input.display().to_string();
            let diag = DiagnosticMessageBuilder::error("Render failed")
                .problem(failure.error.clone())
                .build();
            let json = with_source_file(
                diagnostic_to_json(&diag, &SourceContext::new()),
                source_file,
            );
            emit_json_line(&json);
            continue;
        }
        let ctx_owned;
        let ctx_ref: &SourceContext = match &failure.source_context {
            Some(c) => c,
            None => {
                ctx_owned = SourceContext::new();
                &ctx_owned
            }
        };
        let source_file = failure.input.display().to_string();
        for d in &failure.diagnostics {
            let json = with_source_file(diagnostic_to_json(d, ctx_ref), source_file.clone());
            emit_json_line(&json);
        }
    }

    // Project-level diagnostics (e.g. Q-PROJECT-EMPTY): no source
    // context, no source-file attribution — pure project-scope.
    let empty_ctx = SourceContext::new();
    for diagnostic in &summary.project_diagnostics {
        emit_json_line(&diagnostic_to_json(diagnostic, &empty_ctx));
    }

    // Per-page render diagnostics on successful outputs. The
    // RenderToFileResult does not carry the input path (see the
    // comment in the text branch), so these are emitted without a
    // source_file tag — agents have to attribute them via the
    // location.file_id encoded in the diagnostic.
    for result in &summary.outputs {
        for d in &result.render_output.diagnostics {
            emit_json_line(&diagnostic_to_json(d, &result.render_output.source_context));
        }
    }
}

/// Emit a `QuartoError::Parse` as one or more JSON diagnostics on
/// stderr. Each attached structured diagnostic becomes its own
/// `JsonDiagnostic` line (tagged with `source_file` when an input
/// path is known). If the parse error has no structured
/// diagnostics, a single synthetic diagnostic is emitted from
/// the error string so callers still get a JSON line.
fn emit_parse_error_json(parse_error: &quarto_core::ParseError, input: Option<&Path>) {
    let source_file = input.map(|p| p.display().to_string());
    if parse_error.diagnostics.is_empty() {
        let mut diag = DiagnosticMessageBuilder::error("Parse error")
            .problem(parse_error.to_string())
            .build();
        // Preserve any source context we can; ParseError owns one.
        let mut json = diagnostic_to_json(&diag, &parse_error.source_context);
        if let Some(sf) = source_file {
            json = with_source_file(json, sf);
        }
        emit_json_line(&json);
        // Touch `diag` so clippy doesn't warn about the unused
        // builder var on the trivial path.
        let _ = &mut diag;
        return;
    }
    for d in &parse_error.diagnostics {
        let mut json = diagnostic_to_json(d, &parse_error.source_context);
        if let Some(sf) = &source_file {
            json = with_source_file(json, sf.clone());
        }
        emit_json_line(&json);
    }
}

/// Map a `DispatchError` to a `DiagnosticMessage` carrying its
/// `Q-7-N` code, then emit on stderr as one JSON line.
///
/// Codes are catalog entries in
/// `crates/quarto-error-reporting/error_catalog.json` — keep them
/// in sync.
fn emit_dispatch_error_json(e: &DispatchError) {
    emit_json_line(&diagnostic_to_json(
        &dispatch_error_to_diagnostic(e),
        &SourceContext::new(),
    ));
}

fn dispatch_error_to_diagnostic(e: &DispatchError) -> DiagnosticMessage {
    match e {
        DispatchError::PathNotFound(p) => DiagnosticMessageBuilder::error("Input Path Not Found")
            .with_code("Q-7-2")
            .problem(format!("Input path does not exist: {}", p.display()))
            .add_hint("Check the path is correct and exists relative to the current directory.")
            .build(),
        DispatchError::NoInputAndNoProject(cwd) => {
            DiagnosticMessageBuilder::error("No Input and No Project")
                .with_code("Q-7-3")
                .problem(format!(
                    "No input given and no `_quarto.yml` found at or above {}",
                    cwd.display()
                ))
                .add_hint("Pass at least one `.qmd` path, or run from inside a Quarto project.")
                .build()
        }
        DispatchError::MultiArgNonProject => {
            DiagnosticMessageBuilder::error("Multiple Inputs Without a Project")
                .with_code("Q-7-4")
                .problem(
                    "Multiple input paths require a project (a `_quarto.yml`-rooted directory). \
                     To render a single standalone file, pass exactly one path."
                        .to_string(),
                )
                .build()
        }
        DispatchError::MultiProjectArgs { first, second } => {
            DiagnosticMessageBuilder::error("Inputs Span Multiple Projects")
                .with_code("Q-7-5")
                .problem(format!(
                    "Input paths span more than one project: {} and {}. \
                     Render one project at a time.",
                    first.display(),
                    second.display()
                ))
                .build()
        }
        DispatchError::NotInRenderList { path, project_dir } => DiagnosticMessageBuilder::error(
            "Input Excluded From Render List",
        )
        .with_code("Q-7-6")
        .problem(format!(
            "{} is excluded from the render list of project {}.",
            path.display(),
            project_dir.display()
        ))
        .add_hint(
            "Check `project.render` in `_quarto.yml` and the underscore/hidden file conventions.",
        )
        .build(),
        DispatchError::NoRenderableMatches { path } => {
            DiagnosticMessageBuilder::error("No Renderable Files Matched")
                .with_code("Q-7-7")
                .problem(format!(
                    "No renderable `.qmd` files matched: {}",
                    path.display()
                ))
                .build()
        }
        DispatchError::Discover(msg) => DiagnosticMessageBuilder::error("Project Discovery Failed")
            .with_code("Q-7-8")
            .problem(format!("Project discovery failed: {msg}"))
            .build(),
    }
}

/// Write one `serde_json` value to stderr followed by a newline.
/// Compact (single-line) JSON — that's the NDJSON contract.
fn emit_json_line<T: serde::Serialize>(value: &T) {
    match serde_json::to_string(value) {
        Ok(line) => eprintln!("{line}"),
        Err(e) => {
            // Should be impossible: our wire types are derived Serialize
            // structs with String / Option / Vec fields. Surface the
            // failure as plain text on stderr so it's still observable.
            eprintln!("(internal: failed to serialize diagnostic for --json-errors: {e})");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_core::FormatIdentifier;
    use quarto_system_runtime::NativeRuntime;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tempfile::TempDir;

    /// Forwards every [`SystemRuntime`] method to an inner
    /// [`NativeRuntime`] but counts `dir_list` invocations.
    ///
    /// Used by `classify_no_inputs_does_not_walk_cwd` (bd-nmkmi) to
    /// assert that the no-project short-circuit avoids the recursive
    /// `.qmd` scan. Pattern adapted from
    /// `quarto_core::project::listing::post_render_upgrade::substitute::tests::CountingRuntime`.
    struct RecordingRuntime {
        inner: NativeRuntime,
        dir_list_count: Arc<AtomicUsize>,
    }

    impl RecordingRuntime {
        fn new() -> Self {
            Self {
                inner: NativeRuntime::new(),
                dir_list_count: Arc::new(AtomicUsize::new(0)),
            }
        }

        fn dir_list_count(&self) -> usize {
            self.dir_list_count.load(Ordering::SeqCst)
        }
    }

    #[async_trait::async_trait]
    impl quarto_system_runtime::SystemRuntime for RecordingRuntime {
        fn file_read(&self, p: &Path) -> quarto_system_runtime::RuntimeResult<Vec<u8>> {
            self.inner.file_read(p)
        }
        fn file_write(&self, p: &Path, c: &[u8]) -> quarto_system_runtime::RuntimeResult<()> {
            self.inner.file_write(p, c)
        }
        fn path_exists(
            &self,
            p: &Path,
            k: Option<quarto_system_runtime::PathKind>,
        ) -> quarto_system_runtime::RuntimeResult<bool> {
            self.inner.path_exists(p, k)
        }
        fn canonicalize(&self, p: &Path) -> quarto_system_runtime::RuntimeResult<PathBuf> {
            self.inner.canonicalize(p)
        }
        fn path_metadata(
            &self,
            p: &Path,
        ) -> quarto_system_runtime::RuntimeResult<quarto_system_runtime::PathMetadata> {
            self.inner.path_metadata(p)
        }
        fn file_copy(&self, s: &Path, d: &Path) -> quarto_system_runtime::RuntimeResult<()> {
            self.inner.file_copy(s, d)
        }
        fn path_rename(&self, o: &Path, n: &Path) -> quarto_system_runtime::RuntimeResult<()> {
            self.inner.path_rename(o, n)
        }
        fn file_remove(&self, p: &Path) -> quarto_system_runtime::RuntimeResult<()> {
            self.inner.file_remove(p)
        }
        fn dir_create(&self, p: &Path, r: bool) -> quarto_system_runtime::RuntimeResult<()> {
            self.inner.dir_create(p, r)
        }
        fn dir_remove(&self, p: &Path, r: bool) -> quarto_system_runtime::RuntimeResult<()> {
            self.inner.dir_remove(p, r)
        }
        fn dir_list(&self, p: &Path) -> quarto_system_runtime::RuntimeResult<Vec<PathBuf>> {
            self.dir_list_count.fetch_add(1, Ordering::SeqCst);
            self.inner.dir_list(p)
        }
        fn cwd(&self) -> quarto_system_runtime::RuntimeResult<PathBuf> {
            self.inner.cwd()
        }
        fn temp_dir(
            &self,
            t: &str,
        ) -> quarto_system_runtime::RuntimeResult<quarto_system_runtime::TempDir> {
            self.inner.temp_dir(t)
        }
        fn exec_pipe(
            &self,
            c: &str,
            a: &[&str],
            s: &[u8],
        ) -> quarto_system_runtime::RuntimeResult<Vec<u8>> {
            self.inner.exec_pipe(c, a, s)
        }
        fn exec_command(
            &self,
            c: &str,
            a: &[&str],
            s: Option<&[u8]>,
        ) -> quarto_system_runtime::RuntimeResult<quarto_system_runtime::CommandOutput> {
            self.inner.exec_command(c, a, s)
        }
        fn env_get(&self, n: &str) -> quarto_system_runtime::RuntimeResult<Option<String>> {
            self.inner.env_get(n)
        }
        fn env_all(
            &self,
        ) -> quarto_system_runtime::RuntimeResult<std::collections::HashMap<String, String>>
        {
            self.inner.env_all()
        }
        async fn fetch_url(
            &self,
            u: &str,
        ) -> quarto_system_runtime::RuntimeResult<(Vec<u8>, String)> {
            self.inner.fetch_url(u).await
        }
        fn os_name(&self) -> &'static str {
            self.inner.os_name()
        }
        fn arch(&self) -> &'static str {
            self.inner.arch()
        }
        fn cpu_time(&self) -> quarto_system_runtime::RuntimeResult<u64> {
            self.inner.cpu_time()
        }
        fn xdg_dir(
            &self,
            k: quarto_system_runtime::XdgDirKind,
            s: Option<&Path>,
        ) -> quarto_system_runtime::RuntimeResult<PathBuf> {
            self.inner.xdg_dir(k, s)
        }
        fn stdout_write(&self, d: &[u8]) -> quarto_system_runtime::RuntimeResult<()> {
            self.inner.stdout_write(d)
        }
        fn stderr_write(&self, d: &[u8]) -> quarto_system_runtime::RuntimeResult<()> {
            self.inner.stderr_write(d)
        }
    }

    // === Format helpers (pre-existing) =====================================

    #[test]
    fn test_resolve_format_html() {
        let format = resolve_format("html").unwrap();
        assert_eq!(format.identifier, FormatIdentifier::Html);
        assert_eq!(format.output_extension, "html");
        assert!(format.native_pipeline);
    }

    #[test]
    fn test_resolve_format_pdf() {
        let format = resolve_format("pdf").unwrap();
        assert_eq!(format.identifier, FormatIdentifier::Pdf);
        assert_eq!(format.output_extension, "pdf");
        assert!(!format.native_pipeline);
    }

    #[test]
    fn test_resolve_format_unknown() {
        // from_format_string returns an error for unknown formats
        assert!(resolve_format("unknown").is_err());
    }

    // === classify_inputs tests =============================================

    fn canonical(p: &Path) -> PathBuf {
        p.canonicalize().unwrap_or_else(|_| p.to_path_buf())
    }

    fn write_file(path: &Path, contents: &str) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, contents).unwrap();
    }

    /// Build a website project with the listed `.qmd` paths (each
    /// path is relative to the project root). Optional `render`
    /// patterns go into `_quarto.yml`.
    fn make_project(temp: &TempDir, files: &[&str], render_patterns: Option<&[&str]>) -> PathBuf {
        let dir = canonical(temp.path());
        let yml = if let Some(globs) = render_patterns {
            let listed = globs
                .iter()
                .map(|g| format!("    - {g}"))
                .collect::<Vec<_>>()
                .join("\n");
            format!("project:\n  type: website\n  output-dir: _site\n  render:\n{listed}\n")
        } else {
            "project:\n  type: website\n  output-dir: _site\n".to_string()
        };
        write_file(&dir.join("_quarto.yml"), &yml);
        for f in files {
            write_file(
                &dir.join(f),
                &format!("---\ntitle: {f}\n---\n\nContent of {f}.\n"),
            );
        }
        dir
    }

    /// Build a `type: default` project (no `output-dir`, so the
    /// resolved output dir equals the project root). Mirrors the
    /// minimal `_quarto.yml` users hand-write.
    fn make_default_project(temp: &TempDir, files: &[&str]) -> PathBuf {
        let dir = canonical(temp.path());
        write_file(&dir.join("_quarto.yml"), "project:\n  type: default\n");
        for f in files {
            write_file(
                &dir.join(f),
                &format!("---\ntitle: {f}\n---\n\nContent of {f}.\n"),
            );
        }
        dir
    }

    /// Build a loose directory (no `_quarto.yml`) with the listed
    /// `.qmd` files.
    fn make_loose_dir(temp: &TempDir, files: &[&str]) -> PathBuf {
        let dir = canonical(temp.path());
        for f in files {
            write_file(
                &dir.join(f),
                &format!("---\ntitle: {f}\n---\n\nContent of {f}.\n"),
            );
        }
        dir
    }

    #[test]
    fn classify_no_args_in_project_returns_full_project() {
        let temp = TempDir::new().unwrap();
        let project = make_project(&temp, &["index.qmd", "about.qmd"], None);
        let runtime = NativeRuntime::new();
        let target = classify_inputs(&[], &project, &runtime).unwrap();
        assert_eq!(
            target,
            RenderTarget::FullProject {
                project_dir: project.clone()
            }
        );
    }

    #[test]
    fn classify_no_args_outside_project_errors() {
        let temp = TempDir::new().unwrap();
        let dir = make_loose_dir(&temp, &["foo.qmd"]);
        let runtime = NativeRuntime::new();
        let err = classify_inputs(&[], &dir, &runtime).unwrap_err();
        assert!(
            matches!(err, DispatchError::NoInputAndNoProject(_)),
            "expected NoInputAndNoProject, got {err:?}"
        );
    }

    /// bd-nmkmi regression: `q2 render` with no args in a directory
    /// that has no ancestor `_quarto.yml` must not walk the cwd
    /// looking for `.qmd` files. The walk's output is discarded; the
    /// result is always `NoInputAndNoProject`. In a large tree this
    /// walk was the user-observed multi-second "freeze."
    ///
    /// The assertion is that `dir_list` is never invoked — a
    /// wall-clock-independent expression of "no directory scan
    /// happened."
    #[test]
    fn classify_no_inputs_does_not_walk_cwd() {
        let temp = TempDir::new().unwrap();
        let dir = canonical(temp.path());
        // Populate a subdirectory so a stray walker would have
        // something to descend into. Pre-fix, `dir_list_count`
        // observes at least 2 (cwd + `sub/`). Post-fix it's exactly 0.
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        write_file(&dir.join("sub/decoy.qmd"), "---\ntitle: decoy\n---\n");

        let runtime = RecordingRuntime::new();
        let err = classify_inputs(&[], &dir, &runtime).unwrap_err();
        assert!(
            matches!(err, DispatchError::NoInputAndNoProject(_)),
            "expected NoInputAndNoProject, got {err:?}"
        );
        assert_eq!(
            runtime.dir_list_count(),
            0,
            "classify_no_inputs should short-circuit before walking the cwd \
             when no ancestor `_quarto.yml` exists; observed {} dir_list call(s)",
            runtime.dir_list_count(),
        );
    }

    /// bd-nmkmi regression sanity: when `q2 render` is run with no
    /// args from a subdirectory of a real project, classification
    /// still walks up to the project root and returns `FullProject`.
    /// The short-circuit must not break this case.
    #[test]
    fn classify_no_args_from_project_subdir_returns_full_project() {
        let temp = TempDir::new().unwrap();
        let project = make_project(&temp, &["index.qmd", "sub/a.qmd", "sub/sub2/b.qmd"], None);
        let nested = project.join("sub").join("sub2");
        let runtime = NativeRuntime::new();
        let target = classify_inputs(&[], &nested, &runtime).unwrap();
        assert_eq!(
            target,
            RenderTarget::FullProject {
                project_dir: project,
            }
        );
    }

    #[test]
    fn classify_one_qmd_in_project_returns_subset() {
        let temp = TempDir::new().unwrap();
        let project = make_project(&temp, &["index.qmd", "about.qmd"], None);
        let runtime = NativeRuntime::new();
        let target = classify_inputs(&["about.qmd".into()], &project, &runtime).unwrap();
        assert_eq!(
            target,
            RenderTarget::Subset {
                project_dir: project.clone(),
                targets: vec![project.join("about.qmd")],
            }
        );
    }

    #[test]
    fn classify_one_qmd_outside_project_returns_single_doc() {
        let temp = TempDir::new().unwrap();
        let dir = make_loose_dir(&temp, &["foo.qmd"]);
        let runtime = NativeRuntime::new();
        let target = classify_inputs(&["foo.qmd".into()], &dir, &runtime).unwrap();
        assert_eq!(target, RenderTarget::SingleDoc(dir.join("foo.qmd")));
    }

    #[test]
    fn classify_directory_pointing_at_project_root_returns_full_project() {
        let temp = TempDir::new().unwrap();
        let project = make_project(&temp, &["index.qmd", "about.qmd"], None);
        let cwd = canonical(temp.path()); // any cwd
        let runtime = NativeRuntime::new();
        let target =
            classify_inputs(&[project.to_string_lossy().into_owned()], &cwd, &runtime).unwrap();
        assert_eq!(
            target,
            RenderTarget::FullProject {
                project_dir: project
            }
        );
    }

    #[test]
    fn classify_subdirectory_returns_subset() {
        let temp = TempDir::new().unwrap();
        let project = make_project(&temp, &["top.qmd", "sub/a.qmd", "sub/b.qmd"], None);
        let runtime = NativeRuntime::new();
        let target = classify_inputs(&["sub".into()], &project, &runtime).unwrap();
        match target {
            RenderTarget::Subset {
                project_dir,
                targets,
            } => {
                assert_eq!(project_dir, project);
                let mut names: Vec<_> = targets
                    .iter()
                    .map(|p| {
                        p.strip_prefix(&project)
                            .unwrap()
                            .to_string_lossy()
                            .replace('\\', "/")
                    })
                    .collect();
                names.sort();
                assert_eq!(names, vec!["sub/a.qmd", "sub/b.qmd"]);
            }
            other => panic!("expected Subset, got {other:?}"),
        }
    }

    #[test]
    fn classify_multiple_qmd_returns_subset_union() {
        let temp = TempDir::new().unwrap();
        let project = make_project(&temp, &["a.qmd", "b.qmd", "c.qmd"], None);
        let runtime = NativeRuntime::new();
        let target =
            classify_inputs(&["a.qmd".into(), "b.qmd".into()], &project, &runtime).unwrap();
        match target {
            RenderTarget::Subset {
                project_dir,
                targets,
            } => {
                assert_eq!(project_dir, project);
                let mut names: Vec<_> = targets
                    .iter()
                    .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
                    .collect();
                names.sort();
                assert_eq!(names, vec!["a.qmd", "b.qmd"]);
            }
            other => panic!("expected Subset, got {other:?}"),
        }
    }

    #[test]
    fn classify_qmd_excluded_by_render_list_errors() {
        let temp = TempDir::new().unwrap();
        let project = make_project(&temp, &["a.qmd", "b.qmd"], Some(&["a.qmd"]));
        let runtime = NativeRuntime::new();
        let err = classify_inputs(&["b.qmd".into()], &project, &runtime).unwrap_err();
        match err {
            DispatchError::NotInRenderList { path, project_dir } => {
                assert_eq!(path, project.join("b.qmd"));
                assert_eq!(project_dir, project);
            }
            other => panic!("expected NotInRenderList, got {other:?}"),
        }
    }

    #[test]
    fn classify_underscore_qmd_errors() {
        // `_partial.qmd` is excluded by discovery's underscore rule
        // and should surface as NotInRenderList — same UX as
        // render-list-excluded.
        let temp = TempDir::new().unwrap();
        let project = make_project(&temp, &["main.qmd", "_partial.qmd"], None);
        let runtime = NativeRuntime::new();
        let err = classify_inputs(&["_partial.qmd".into()], &project, &runtime).unwrap_err();
        assert!(
            matches!(err, DispatchError::NotInRenderList { .. }),
            "expected NotInRenderList, got {err:?}"
        );
    }

    #[test]
    fn classify_directory_with_no_qmd_errors() {
        let temp = TempDir::new().unwrap();
        let project = make_project(&temp, &["main.qmd"], None);
        // Empty subdirectory.
        std::fs::create_dir_all(project.join("empty")).unwrap();
        let runtime = NativeRuntime::new();
        let err = classify_inputs(&["empty".into()], &project, &runtime).unwrap_err();
        match err {
            DispatchError::NoRenderableMatches { path } => {
                assert_eq!(path, project.join("empty"));
            }
            other => panic!("expected NoRenderableMatches, got {other:?}"),
        }
    }

    #[test]
    fn classify_multi_arg_spans_two_projects_errors() {
        let temp1 = TempDir::new().unwrap();
        let temp2 = TempDir::new().unwrap();
        let p1 = make_project(&temp1, &["a.qmd"], None);
        let p2 = make_project(&temp2, &["b.qmd"], None);
        let cwd = canonical(temp1.path());
        let runtime = NativeRuntime::new();
        let err = classify_inputs(
            &[
                p1.join("a.qmd").to_string_lossy().into_owned(),
                p2.join("b.qmd").to_string_lossy().into_owned(),
            ],
            &cwd,
            &runtime,
        )
        .unwrap_err();
        assert!(
            matches!(err, DispatchError::MultiProjectArgs { .. }),
            "expected MultiProjectArgs, got {err:?}"
        );
    }

    #[test]
    fn classify_multi_arg_outside_project_errors() {
        let temp = TempDir::new().unwrap();
        let dir = make_loose_dir(&temp, &["a.qmd", "b.qmd"]);
        let runtime = NativeRuntime::new();
        let err = classify_inputs(&["a.qmd".into(), "b.qmd".into()], &dir, &runtime).unwrap_err();
        assert!(
            matches!(err, DispatchError::MultiArgNonProject),
            "expected MultiArgNonProject, got {err:?}"
        );
    }

    #[test]
    fn classify_nonexistent_path_errors() {
        let temp = TempDir::new().unwrap();
        let dir = canonical(temp.path());
        let runtime = NativeRuntime::new();
        let err = classify_inputs(&["does-not-exist.qmd".into()], &dir, &runtime).unwrap_err();
        assert!(
            matches!(err, DispatchError::PathNotFound(_)),
            "expected PathNotFound, got {err:?}"
        );
    }

    #[test]
    fn classify_one_qmd_in_default_project_succeeds() {
        // Regression for the post-websites-merge bug: a
        // `_quarto.yml` with `project: { type: default }` (no
        // explicit render list, no output-dir) used to make
        // discovery exclude every file because `output_dir` defaults
        // to the project root and the walker rejected anything that
        // started with the output dir. After the fix, naming the only
        // `.qmd` in the project should classify successfully — and
        // because that file is the entire project file list, it
        // collapses to FullProject.
        let temp = TempDir::new().unwrap();
        let project = make_default_project(&temp, &["index.qmd"]);
        let runtime = NativeRuntime::new();
        let target = classify_inputs(&["index.qmd".into()], &project, &runtime).unwrap();
        assert_eq!(
            target,
            RenderTarget::FullProject {
                project_dir: project,
            },
        );
    }

    #[test]
    fn classify_no_args_in_default_project_returns_full_project() {
        // Companion to `classify_one_qmd_in_default_project_succeeds`:
        // running `q2 render` with no args inside a default project
        // returns FullProject. Pre-fix this also "succeeded" but with
        // an empty file list, so the subsequent render silently
        // produced no output. The classification step itself looks
        // identical; the empty-render-set diagnostic in Phase 2 will
        // catch the silent-no-output case downstream.
        let temp = TempDir::new().unwrap();
        let project = make_default_project(&temp, &["index.qmd", "about.qmd"]);
        let runtime = NativeRuntime::new();
        let target = classify_inputs(&[], &project, &runtime).unwrap();
        assert_eq!(
            target,
            RenderTarget::FullProject {
                project_dir: project,
            },
        );
    }

    #[test]
    fn classify_subset_covering_all_files_collapses_to_full_project() {
        // If the user names every file in the project explicitly, we
        // collapse to FullProject (Mode A). It's functionally
        // identical and avoids the dependency-graph augmentation cost.
        let temp = TempDir::new().unwrap();
        let project = make_project(&temp, &["a.qmd", "b.qmd"], None);
        let runtime = NativeRuntime::new();
        let target =
            classify_inputs(&["a.qmd".into(), "b.qmd".into()], &project, &runtime).unwrap();
        assert_eq!(
            target,
            RenderTarget::FullProject {
                project_dir: project
            }
        );
    }

    // === run_clean_cache tests =============================================

    /// Helper: write a fake profile-cache file under
    /// `<project>/.quarto/cache/profiles/<key>` so we can verify it's
    /// gone after `run_clean_cache`.
    fn populate_profile_cache(project_dir: &Path, key: &str, bytes: &[u8]) {
        let p = project_dir.join(".quarto/cache/profiles").join(key);
        write_file(&p, "");
        std::fs::write(&p, bytes).unwrap();
    }

    fn populate_sass_cache(project_dir: &Path, key: &str, bytes: &[u8]) {
        let p = project_dir.join(".quarto/cache/sass").join(key);
        write_file(&p, "");
        std::fs::write(&p, bytes).unwrap();
    }

    fn write_nav_config_hash(project_dir: &Path, contents: &str) {
        let p = project_dir.join(".quarto/cache/nav-config-hash");
        write_file(&p, contents);
    }

    #[test]
    fn clean_cache_wipes_profiles_namespace() {
        let temp = TempDir::new().unwrap();
        let project = canonical(temp.path());
        populate_profile_cache(&project, "abc123", b"profile-bytes");
        let profile_path = project.join(".quarto/cache/profiles/abc123");
        assert!(
            profile_path.exists(),
            "fixture: profile cache file should exist"
        );

        let runtime = NativeRuntime::with_cache_dir(project.join(".quarto/cache"));
        run_clean_cache(&runtime, &project).unwrap();

        assert!(
            !profile_path.exists(),
            "profile cache file should be removed"
        );
    }

    #[test]
    fn clean_cache_removes_nav_config_hash_when_present() {
        let temp = TempDir::new().unwrap();
        let project = canonical(temp.path());
        write_nav_config_hash(&project, "deadbeef");
        let hash_path = project.join(".quarto/cache/nav-config-hash");
        assert!(hash_path.exists());

        let runtime = NativeRuntime::with_cache_dir(project.join(".quarto/cache"));
        run_clean_cache(&runtime, &project).unwrap();

        assert!(!hash_path.exists(), "nav-config-hash should be removed");
    }

    #[test]
    fn clean_cache_no_op_on_missing_nav_config_hash() {
        let temp = TempDir::new().unwrap();
        let project = canonical(temp.path());
        // No nav-config-hash file written.
        let runtime = NativeRuntime::with_cache_dir(project.join(".quarto/cache"));
        // Should succeed without error.
        run_clean_cache(&runtime, &project).unwrap();
    }

    #[test]
    fn clean_cache_preserves_sass_namespace() {
        let temp = TempDir::new().unwrap();
        let project = canonical(temp.path());
        populate_profile_cache(&project, "p1", b"p");
        populate_sass_cache(&project, "s1", b"s");

        let runtime = NativeRuntime::with_cache_dir(project.join(".quarto/cache"));
        run_clean_cache(&runtime, &project).unwrap();

        assert!(
            !project.join(".quarto/cache/profiles/p1").exists(),
            "profiles entry should be removed"
        );
        assert!(
            project.join(".quarto/cache/sass/s1").exists(),
            "sass entry should be preserved"
        );
    }

    // === render_summary_line tests =========================================

    #[test]
    fn summary_line_none_for_single_file() {
        assert_eq!(render_summary_line(true, 1, 1, Path::new("/tmp/out")), None);
    }

    #[test]
    fn summary_line_for_multi_file_project_full() {
        assert_eq!(
            render_summary_line(false, 5, 5, Path::new("/tmp/_site")),
            Some("Rendered 5 of 5 files to /tmp/_site".to_string())
        );
    }

    #[test]
    fn summary_line_for_multi_file_project_partial() {
        assert_eq!(
            render_summary_line(false, 5, 1, Path::new("/tmp/_site")),
            Some("Rendered 1 of 5 files to /tmp/_site".to_string())
        );
    }

    #[test]
    fn summary_line_zero_rendered_still_emits() {
        // All pages failed Pass-2 — rendered=0. Still informative.
        assert_eq!(
            render_summary_line(false, 3, 0, Path::new("/tmp/_site")),
            Some("Rendered 0 of 3 files to /tmp/_site".to_string())
        );
    }

    // === bd-ooleh: error/warning counts clause ============================

    fn counts(errors: usize, warnings: usize) -> DiagnosticCounts {
        DiagnosticCounts { errors, warnings }
    }

    #[test]
    fn counts_clause_none_when_clean() {
        assert_eq!(format_counts_clause(&counts(0, 0), false), None);
        assert_eq!(format_counts_clause(&counts(0, 0), true), None);
    }

    #[test]
    fn counts_clause_errors_only() {
        assert_eq!(
            format_counts_clause(&counts(1, 0), false),
            Some("1 error".to_string())
        );
        assert_eq!(
            format_counts_clause(&counts(3, 0), false),
            Some("3 errors".to_string())
        );
    }

    #[test]
    fn counts_clause_warnings_only() {
        assert_eq!(
            format_counts_clause(&counts(0, 1), false),
            Some("1 warning".to_string())
        );
        assert_eq!(
            format_counts_clause(&counts(0, 5), false),
            Some("5 warnings".to_string())
        );
    }

    #[test]
    fn counts_clause_both_errors_and_warnings() {
        assert_eq!(
            format_counts_clause(&counts(3, 5), false),
            Some("3 errors, 5 warnings".to_string())
        );
    }

    #[test]
    fn counts_clause_singular_plural_edges() {
        assert_eq!(
            format_counts_clause(&counts(1, 1), false),
            Some("1 error, 1 warning".to_string())
        );
        assert_eq!(
            format_counts_clause(&counts(2, 1), false),
            Some("2 errors, 1 warning".to_string())
        );
    }

    #[test]
    fn counts_clause_color_wraps_counts_in_ansi() {
        // Errors red (31), warnings yellow (33), each reset (0).
        let clause = format_counts_clause(&counts(2, 1), true).unwrap();
        assert!(
            clause.contains("\x1b[31m2 errors\x1b[0m"),
            "got: {clause:?}"
        );
        assert!(
            clause.contains("\x1b[33m1 warning\x1b[0m"),
            "got: {clause:?}"
        );
    }

    #[test]
    fn counts_clause_no_color_has_no_ansi() {
        let clause = format_counts_clause(&counts(2, 1), false).unwrap();
        assert!(!clause.contains('\x1b'), "got: {clause:?}");
    }
}
