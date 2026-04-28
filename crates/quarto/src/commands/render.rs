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

use quarto_core::project::orchestrator::{ProjectPipeline, RenderMode, project_type_for};
use quarto_core::{Format, ProjectContext, QuartoError, RenderToFileOptions};
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
    /// Leave intermediate files (not yet implemented).
    #[allow(dead_code)]
    pub debug: bool,
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
    let project = ProjectContext::discover(&cwd_canon, runtime)
        .map_err(|e| DispatchError::Discover(e.to_string()))?;
    if !is_real_project(&project, runtime) {
        return Err(DispatchError::NoInputAndNoProject(cwd_canon));
    }
    Ok(RenderTarget::FullProject {
        project_dir: project.dir,
    })
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
/// Returns `None` for single-file renders: the line `"1 of 1 rendered"`
/// would be noise. Multi-file projects always get a summary line, even
/// when zero pages rendered (e.g. all skipped due to errors).
pub fn render_summary_line(
    is_single_file: bool,
    total_files: usize,
    rendered: usize,
) -> Option<String> {
    if is_single_file {
        return None;
    }
    Some(format!("{rendered} of {total_files} rendered"))
}

/// Execute the render command.
pub fn execute(args: RenderArgs) -> Result<()> {
    // Create the system runtime
    let runtime = NativeRuntime::new();

    let cwd = runtime
        .cwd()
        .map_err(|e| anyhow::anyhow!("Failed to get current directory: {}", e))?;

    // Determine format
    let format_str = args.to.as_deref().unwrap_or("html");
    let format = resolve_format(format_str)?;

    // Only HTML is supported in MVP
    if !format.identifier.is_native() {
        anyhow::bail!(
            "Format '{}' is not yet supported. Only HTML is available in this version.",
            format.identifier
        );
    }

    // Classify inputs into a render target.
    let target =
        classify_inputs(&args.inputs, &cwd, &runtime).map_err(|e| anyhow::anyhow!("{}", e))?;

    // Set up render options
    let options = RenderToFileOptions {
        output_path: args.output.as_ref().map(PathBuf::from),
        output_dir: args.output_dir.as_ref().map(PathBuf::from),
        quiet: args.quiet,
    };

    match target {
        RenderTarget::SingleDoc(input) => execute_single_doc(input, &args, &options, format),
        RenderTarget::FullProject { project_dir } => {
            execute_project(project_dir, None, &args, &options, format, format_str)
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
            format_str,
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

    if !args.quiet {
        info!("Rendering single file: {}", input.display());
    }

    let project_type = project_type_for(&project);
    let format_str = format.identifier.to_string();
    let mut pipeline = ProjectPipeline::new(
        &mut project,
        project_type,
        format,
        &format_str,
        options,
        runtime_arc.clone(),
    );

    let summary = match pollster::block_on(pipeline.run()) {
        Ok(s) => s,
        Err(QuartoError::Parse(parse_error)) => {
            eprintln!("{}", parse_error);
            std::process::exit(1);
        }
        Err(e) => return Err(anyhow::anyhow!("{}", e)),
    };

    print_render_diagnostics(&summary, args.quiet);

    if !summary.pass2_failures.is_empty() {
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

    if !args.quiet {
        info!(
            "Rendering project: {} (type: {})",
            project.dir.display(),
            project.project_kind().as_str()
        );
    }

    let project_type = project_type_for(&project);
    let total_files = project.files.len();

    let mut pipeline = ProjectPipeline::new(
        &mut project,
        project_type,
        format,
        format_str,
        options,
        runtime_arc.clone(),
    );

    if let Some(target_set) = targets {
        let set: std::collections::HashSet<PathBuf> = target_set.into_iter().collect();
        pipeline = pipeline.with_mode(RenderMode::Subset(set));
    }

    let summary = match pollster::block_on(pipeline.run()) {
        Ok(s) => s,
        Err(QuartoError::Parse(parse_error)) => {
            eprintln!("{}", parse_error);
            std::process::exit(1);
        }
        Err(e) => return Err(anyhow::anyhow!("{}", e)),
    };

    print_render_diagnostics(&summary, args.quiet);

    let rendered = summary.outputs.len();
    if let Some(line) = render_summary_line(false, total_files, rendered)
        && !args.quiet
    {
        info!("{}", line);
    }

    if !summary.pass2_failures.is_empty() {
        std::process::exit(1);
    }
    Ok(())
}

fn print_render_diagnostics(
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
    for failure in &summary.pass2_failures {
        eprintln!("error: {}: {}", failure.input.display(), failure.error);
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

        if !quiet {
            info!("Output: {}", result.output_path.display());
        }
    }
}

/// Resolve format string to Format (without metadata)
fn resolve_format(format_str: &str) -> Result<Format> {
    Format::from_format_string(format_str).map_err(|e| anyhow::anyhow!("{}", e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_core::FormatIdentifier;
    use quarto_system_runtime::NativeRuntime;
    use tempfile::TempDir;

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
        assert_eq!(render_summary_line(true, 1, 1), None);
    }

    #[test]
    fn summary_line_for_multi_file_project_full() {
        assert_eq!(
            render_summary_line(false, 5, 5),
            Some("5 of 5 rendered".to_string())
        );
    }

    #[test]
    fn summary_line_for_multi_file_project_partial() {
        assert_eq!(
            render_summary_line(false, 5, 1),
            Some("1 of 5 rendered".to_string())
        );
    }

    #[test]
    fn summary_line_zero_rendered_still_emits() {
        // All pages failed Pass-2 — rendered=0. Still informative.
        assert_eq!(
            render_summary_line(false, 3, 0),
            Some("0 of 3 rendered".to_string())
        );
    }
}
