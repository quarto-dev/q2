/*
 * render_to_file.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * High-level render orchestration that writes output to files.
 */

//! High-level render-to-file orchestration.
//!
//! This module provides the complete render pipeline that:
//! 1. Reads input document
//! 2. Creates output directory and resources (CSS, etc.)
//! 3. Runs the render pipeline
//! 4. Writes output files
//!
//! This is the function that both the CLI and test infrastructure use,
//! ensuring consistent behavior across all render paths.
//!
//! # Simple Usage
//!
//! For simple cases (single file, auto-discover project):
//!
//! ```ignore
//! use quarto_core::render_to_file::{render_to_file, RenderToFileOptions};
//! use quarto_system_runtime::NativeRuntime;
//! use std::sync::Arc;
//!
//! let runtime = Arc::new(NativeRuntime::new());
//! let options = RenderToFileOptions::default();
//!
//! let result = render_to_file(
//!     Path::new("document.qmd"),
//!     "html",
//!     &options,
//!     runtime,
//! )?;
//! ```
//!
//! # Advanced Usage (CLI)
//!
//! For multi-file projects where you want to discover once and render many:
//!
//! ```ignore
//! use quarto_core::render_to_file::{render_document_to_file, RenderToFileOptions};
//! use quarto_core::project::ProjectContext;
//!
//! // Discover project once
//! let project = ProjectContext::discover(&input_path, &runtime)?;
//!
//! // Render each document
//! for doc in &project.files {
//!     let result = render_document_to_file(
//!         &doc.input,
//!         "html",
//!         &options,
//!         Some(&project),
//!         runtime.clone(),
//!     )?;
//! }
//! ```

use std::path::{Path, PathBuf};
use std::sync::Arc;

use tracing::debug;

use quarto_system_runtime::SystemRuntime;

use crate::Result;
use crate::artifact::{ArtifactScope, ArtifactStore};
use crate::artifact_flush::{enqueue_artifacts, route_drained_project_artifacts};
use crate::error::QuartoError;
use crate::format::Format;
use crate::output_sink::{OutputSink, OutputSinkError};
use crate::pipeline::{HtmlRenderConfig, RenderOutput, render_qmd_to_html};
use crate::project::index::ProjectIndex;
use crate::project::orchestrator::project_type_for;
use crate::project::{DocumentInfo, ProjectContext};
use crate::render::{BinaryDependencies, RenderContext};
use crate::resource_resolver::ResourceResolverContext;
use crate::resources;

/// Options for rendering a document to a file.
#[derive(Debug, Clone, Default)]
pub struct RenderToFileOptions {
    /// Explicit output path. If None, derived from input path.
    pub output_path: Option<PathBuf>,
    /// Explicit output directory. If None, same directory as input.
    pub output_dir: Option<PathBuf>,
    /// Suppress informational messages (logging).
    pub quiet: bool,
    /// Replay engine captures loaded from a trace file (bd-45yw,
    /// extended to a sequence by bd-5yff4).
    ///
    /// When non-empty, the render substitutes a
    /// [`crate::engine::ReplayEngine`] for each recorded engine
    /// (one capture per engine, in execution order) in the
    /// pipeline's registry. Activated out-of-band by the
    /// orchestrator/CLI (`--replay <trace>` / `QUARTO_REPLAY=...`);
    /// the document under investigation does not need to know.
    pub replay_captures: Vec<quarto_trace::EngineCapture>,

    /// Direct override for the engine registry the pipeline uses
    /// (bd-45yw, primarily for tests).
    ///
    /// When `Some`, takes precedence over `replay_captures`: the
    /// caller hands the pipeline an arbitrary registry. Tests use
    /// this seam to register probe engines that capture the QMD
    /// `EngineExecutionStage` hands to `execute()` (so a replay
    /// trace can be fabricated against the real pipeline rather
    /// than a synthetic context). Production callers should prefer
    /// `replay_captures`.
    pub engine_registry_override: Option<crate::engine::EngineRegistry>,

    /// Resolved attribution mode (CLI override merged with YAML).
    /// `Some(AttributionMode::Git)` installs a [`GitBlameProvider`]
    /// on the outer `RenderContext`; `Some(AttributionMode::Off)` or
    /// `None` leaves the provider slot empty so the unflagged code
    /// path is taken.
    ///
    /// [`GitBlameProvider`]: crate::attribution::GitBlameProvider
    pub attribution: Option<crate::attribution::AttributionMode>,
}

/// Result of rendering a document to a file.
#[derive(Debug)]
pub struct RenderToFileResult {
    /// Path to the output file.
    pub output_path: PathBuf,
    /// Path to the resources directory (e.g., `document_files/`).
    pub resources_dir: PathBuf,
    /// The full render output including HTML and diagnostics.
    pub render_output: RenderOutput,
    /// Per-document resource report (`bd-o8pr` Phase 2). Contains
    /// engine-emitted (and Phase 3+ Lua-filter-emitted) supporting
    /// files. Drained by the project orchestrator after the per-doc
    /// render completes; resolved against the project root and
    /// merged with the static-channel list before the copy step.
    /// Empty for renders that produced no engine/filter resources.
    pub resource_report: crate::project_resources::DocumentResourceReport,
}

/// Render a QMD document to a file (simple API).
///
/// This is the simplest entry point for rendering documents. It automatically
/// discovers the project context and handles all setup.
///
/// For multi-file projects where you want to discover once and render many files,
/// use [`render_document_to_file`] instead.
///
/// # Arguments
///
/// * `input_path` - Path to the input QMD file
/// * `format` - Output format name (e.g., "html")
/// * `options` - Render options
/// * `runtime` - System runtime for file operations
///
/// # Returns
///
/// Returns the render result with output path and diagnostics.
#[cfg(not(target_arch = "wasm32"))]
pub fn render_to_file(
    input_path: &Path,
    format: &str,
    options: &RenderToFileOptions,
    runtime: Arc<dyn SystemRuntime>,
) -> Result<RenderToFileResult> {
    // Standalone: pass `None` for project_artifacts so the
    // function flushes Project-scoped artifacts via the resolver.
    // `format_override = None`: the caller passed an explicit format string,
    // which is authoritative for this entry point.
    render_document_to_file(input_path, format, options, None, runtime, None, None, None)
}

/// Render a QMD document to a file (advanced API).
///
/// This function accepts an optional pre-discovered `ProjectContext`, which is
/// useful when rendering multiple files in a project - you discover once and
/// render many times without re-discovering for each file.
///
/// # Arguments
///
/// * `input_path` - Path to the input QMD file
/// * `format` - Output format name (e.g., "html")
/// * `options` - Render options
/// * `project` - Optional pre-discovered project context. If None, discovers automatically.
/// * `runtime` - System runtime for file operations
///
/// # Returns
///
/// Returns the render result with output path and diagnostics.
///
/// # Errors
///
/// Returns an error if:
/// - The input file cannot be read
/// - Project discovery fails (when project is None)
/// - Resource writing fails
/// - The render pipeline fails
/// - The output file cannot be written
#[cfg(not(target_arch = "wasm32"))]
pub fn render_document_to_file(
    input_path: &Path,
    format: &str,
    options: &RenderToFileOptions,
    project: Option<&ProjectContext>,
    runtime: Arc<dyn SystemRuntime>,
    project_index: Option<Arc<ProjectIndex>>,
    project_artifacts: Option<&mut ArtifactStore>,
    format_override: Option<&str>,
) -> Result<RenderToFileResult> {
    debug!("Rendering: {}", input_path.display());

    // Read input file
    let input_bytes = runtime.file_read(input_path).map_err(|e| {
        QuartoError::other(format!(
            "Failed to read input file {}: {}",
            input_path.display(),
            e
        ))
    })?;

    // Use provided project or discover
    let discovered_project;
    let project = match project {
        Some(p) => p,
        None => {
            discovered_project = ProjectContext::discover(input_path, runtime.as_ref())?;
            &discovered_project
        }
    };

    // Per-document format resolution (bd-l6itt34u minimal slice). The
    // effective format key is a prefer-merge of the `format:` declarations:
    // project config → this document's front matter → `--to` (synthesized as
    // `format: !prefer`). So a `format: revealjs` deck inside an otherwise-HTML
    // `type: default` project — or a project-level `format: revealjs` — renders
    // as a real deck, while an explicit `--to` still wins for every document.
    // The result drives BOTH the transform pipeline (reveal assembly) and the
    // output paths/extension, so it must be resolved here, before the pipeline
    // is built. `format` is the caller's fallback default (e.g. "html").
    let project_format = project
        .config
        .metadata
        .as_ref()
        .and_then(|m| m.get("format"))
        .and_then(crate::format::format_key_from_config_value);
    let document_format = std::str::from_utf8(&input_bytes)
        .ok()
        .and_then(crate::format::format_key_from_frontmatter);
    let resolved_format = crate::format::resolve_format_key(
        project_format.as_deref(),
        document_format.as_deref(),
        format_override,
        format,
    );
    let format: &str = &resolved_format;

    // Determine output paths. If the options don't specify an
    // explicit output path / dir, fall back to the project's
    // `output-dir` (e.g. websites render into `_site/`). Single-file
    // and default-kind projects leave `output_dir == dir`, preserving
    // the pre-Phase-1 "beside the input" behavior.
    let effective_options = apply_project_output_dir_to_options(options, project, input_path);
    let (output_path, output_dir, output_stem) =
        determine_output_paths(input_path, format, &effective_options)?;

    // Create output directory
    runtime.dir_create(&output_dir, true).map_err(|e| {
        QuartoError::other(format!(
            "Failed to create output directory {}: {}",
            output_dir.display(),
            e
        ))
    })?;

    // Prepare resource directory (creates {stem}_files/ but does not write CSS)
    let resource_paths =
        resources::prepare_html_resources(&output_dir, &output_stem, runtime.as_ref())?;

    // Set up render context
    let doc_info = DocumentInfo::from_path(input_path);
    let render_format = format_from_name(format)?;
    // Discover binaries from the runtime so the git path (used by
    // `GitBlameProvider`) is populated alongside pandoc/typst/etc.
    let binaries = BinaryDependencies::discover(runtime.as_ref());
    let mut ctx = RenderContext::new(project, &doc_info, &render_format, &binaries);
    if let Some(index) = project_index {
        ctx.project_index = Some(index);
    }
    // Install the attribution provider when the CLI/YAML resolved
    // mode is `Git`. `Off` and `None` leave the slot empty, which is
    // the unflagged default code path.
    if matches!(
        options.attribution,
        Some(crate::attribution::AttributionMode::Git)
    ) {
        ctx.attribution_provider = Some(std::sync::Arc::new(
            crate::attribution::GitBlameProvider::new(),
        ));
    }

    // Phase 5: build a scope-aware resolver from the doc's
    // output location and the project's lib dir. Single-doc /
    // default projects pass `lib_dir == ""` so Project-scope
    // artifacts resolve under the per-page resource dir
    // (preserving pre-Phase-5 byte-identical behavior). Website
    // projects pass `lib_dir == "site_libs"` so Project-scope
    // artifacts resolve under `{output_dir}/site_libs/`.
    let project_type = project_type_for(project);
    let resolver = ResourceResolverContext::website(
        &project.output_dir,
        &output_path,
        project_type.lib_dir(),
        &output_stem,
    );
    // Phase 6: make the same resolver available to AST transforms
    // via `RenderContext::resource_resolver` so the body-link
    // rewriter (`LinkRewriteTransform`) can compute page-relative
    // URLs the same way Phase 5's `ApplyTemplateStage` does for
    // shared assets.
    ctx.resource_resolver = Some(resolver.clone());
    let mut config = HtmlRenderConfig::with_resolver(resolver.clone());
    // bd-45yw / bd-5yff4: pick the engine registry the pipeline will
    // use. engine_registry_override takes precedence (tests / probe
    // engines). Otherwise replay_captures (one per recorded engine)
    // build the registry. Otherwise the pipeline builds its own default
    // registry.
    if let Some(reg) = options.engine_registry_override.clone() {
        config.engine_registry = Some(reg);
    } else if !options.replay_captures.is_empty() {
        config.engine_registry = Some(crate::engine::EngineRegistry::with_replay_many(
            options.replay_captures.clone(),
        ));
    }

    // Run the render pipeline
    let mut render_output = pollster::block_on(render_qmd_to_html(
        &input_bytes,
        &input_path.to_string_lossy(),
        &mut ctx,
        &config,
        runtime.clone(),
    ))?;

    // bd-cfl67: one sink per render owns every destructive write.
    // Construct it from the resolver's declared output roots so
    // any escape (e.g. an absolute artifact path that bypassed
    // `scope_root.join`) is refused before the disk is touched.
    let mut sink = OutputSink::new(resolver.allowed_output_roots());

    // Phase 5: enqueue Page-scoped artifacts at their per-doc
    // locations. Project-scoped artifacts are drained out of the
    // per-doc store and either:
    // - merged into the orchestrator's project-wide artifact
    //   accumulator (when running under a project type that has
    //   a shared lib dir, e.g. websites), so
    //   `WebsiteProjectType::post_render` can flush them once
    //   for the whole project; OR
    // - flushed in-place via the resolver (otherwise — either
    //   no orchestrator is involved, or the orchestrator's
    //   project type has no shared lib dir, e.g. default
    //   single-doc / loose-directory projects).
    enqueue_artifacts(&ctx.artifacts, &resolver, ArtifactScope::Page, &mut sink)?;
    let drained = ctx.artifacts.drain_project_scoped();

    // Shared with both Pass-2 renderers (bd-gdhk). Project-scope
    // artifacts either accumulate for a once-per-project flush, or —
    // for `lib_dir == ""` / standalone calls — enqueue into this
    // render's sink, where the resolver routes them under
    // `{stem}_files/` (pre-Phase-5 layout).
    let has_shared_lib = !project_type.lib_dir().is_empty();
    route_drained_project_artifacts(
        drained,
        project_artifacts,
        has_shared_lib,
        &resolver,
        &mut sink,
        input_path,
    )?;

    // Drain user-resource copy intents (images etc. collected by
    // `ResourceCollectorTransform`) into the sink. The sink will
    // skip ops whose src and dest canonicalize equal — the common
    // single-doc case where output_dir == input_dir and the
    // resource is already where the HTML expects it.
    //
    // bd-bxrkxblx: a referenced resource whose *source* is missing is
    // a user-content problem — emit a `Q-5-6` warning (located at the
    // reference) and skip it, rather than aborting the render on the
    // ENOENT at flush. Genuine flush-time copy faults (permission,
    // disk full) become the `Q-5-7` error below.
    let resource_copies = std::mem::take(&mut ctx.resource_copies);
    let copy_warnings = crate::resource_copy_diagnostics::enqueue_resource_copies(
        resource_copies,
        &mut sink,
        runtime.as_ref(),
    )?;
    render_output.diagnostics.extend(copy_warnings);

    // Output HTML also goes through the sink so the whole render's
    // destructive output is validated and committed atomically.
    sink.write(output_path.clone(), render_output.html.as_bytes().to_vec())
        .map_err(QuartoError::from)?;

    // Distinguish a resource-copy fault (the bytes the user referenced
    // couldn't be placed — Q-5-7) from any other destructive write in
    // the sink (HTML output, artifact dir creation), which keep their
    // existing error path.
    sink.flush(runtime.as_ref()).map_err(|e| match &e {
        OutputSinkError::Copy { .. } => crate::resource_copy_diagnostics::copy_failure_error(&e),
        _ => QuartoError::from(e),
    })?;

    debug!("Output: {}", output_path.display());

    let resource_report = std::mem::take(&mut ctx.resource_report);
    Ok(RenderToFileResult {
        output_path,
        resources_dir: resource_paths.resource_dir,
        render_output,
        resource_report,
    })
}

/// When `options` has no explicit `output_path` / `output_dir`, fall
/// back to the project's output dir so e.g. `_site/index.html` is
/// produced rather than `index.html` beside the input.
///
/// Preserves the input's subdirectory under `project_dir` so
/// `docs/api.qmd` in a website project renders to `_site/docs/api.html`.
fn apply_project_output_dir_to_options(
    options: &RenderToFileOptions,
    project: &ProjectContext,
    input_path: &Path,
) -> RenderToFileOptions {
    if options.output_path.is_some() || options.output_dir.is_some() {
        return options.clone();
    }
    if project.output_dir == project.dir {
        return options.clone();
    }
    let relative = input_path
        .strip_prefix(&project.dir)
        .ok()
        .and_then(|r| r.parent())
        .map(|p| p.to_path_buf())
        .unwrap_or_default();
    let resolved = project.output_dir.join(relative);
    let mut next = options.clone();
    next.output_dir = Some(resolved);
    next
}

/// Determine output paths from input path and options.
fn determine_output_paths(
    input_path: &Path,
    format: &str,
    options: &RenderToFileOptions,
) -> Result<(PathBuf, PathBuf, String)> {
    // Determine file extension using the base format (strips extension prefix)
    let render_format = Format::from_format_string(format).unwrap_or_else(|_| Format::html());
    let extension = match render_format.identifier.as_str() {
        "html" => "html",
        "pdf" => "pdf",
        "docx" => "docx",
        "typst" => "typ",
        // Note: "latex"/"tex" was previously handled here but FormatIdentifier
        // has no Latex variant yet. Add one when latex output is supported.
        _ => "html",
    };

    // Get input stem
    let stem = input_path
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| QuartoError::other("Could not determine input filename stem"))?
        .to_string();

    // Determine output path
    let output_path = if let Some(ref explicit_path) = options.output_path {
        explicit_path.clone()
    } else {
        let base_dir = options
            .output_dir
            .clone()
            .or_else(|| input_path.parent().map(|p| p.to_path_buf()))
            .unwrap_or_else(|| PathBuf::from("."));

        base_dir.join(format!("{}.{}", stem, extension))
    };

    // Determine output directory
    let output_dir = output_path
        .parent()
        .map_or_else(|| PathBuf::from("."), |p| p.to_path_buf());

    // Get output stem (may differ from input stem if explicit output path given)
    let output_stem = output_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(&stem)
        .to_string();

    Ok((output_path, output_dir, output_stem))
}

/// Convert a format name to a Format instance.
fn format_from_name(name: &str) -> Result<Format> {
    Format::from_format_string(name).map_err(QuartoError::other)
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_system_runtime::NativeRuntime;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_determine_output_paths_default() {
        let input = Path::new("/project/doc.qmd");
        let options = RenderToFileOptions::default();

        let (output, dir, stem) = determine_output_paths(input, "html", &options).unwrap();

        assert_eq!(output, PathBuf::from("/project/doc.html"));
        assert_eq!(dir, PathBuf::from("/project"));
        assert_eq!(stem, "doc");
    }

    #[test]
    fn test_determine_output_paths_explicit_output() {
        let input = Path::new("/project/doc.qmd");
        let options = RenderToFileOptions {
            output_path: Some(PathBuf::from("/out/custom.html")),
            ..Default::default()
        };

        let (output, dir, stem) = determine_output_paths(input, "html", &options).unwrap();

        assert_eq!(output, PathBuf::from("/out/custom.html"));
        assert_eq!(dir, PathBuf::from("/out"));
        assert_eq!(stem, "custom");
    }

    #[test]
    fn test_determine_output_paths_output_dir() {
        let input = Path::new("/project/doc.qmd");
        let options = RenderToFileOptions {
            output_dir: Some(PathBuf::from("/out")),
            ..Default::default()
        };

        let (output, dir, stem) = determine_output_paths(input, "html", &options).unwrap();

        assert_eq!(output, PathBuf::from("/out/doc.html"));
        assert_eq!(dir, PathBuf::from("/out"));
        assert_eq!(stem, "doc");
    }

    /// bd-9ez3ngt1: `reference-location` set in YAML front matter must be
    /// honored end-to-end. In document-metadata context a bare YAML string is
    /// parsed as markdown and stored as `ConfigValueKind::PandocInlines`, so
    /// the transform's `meta.get("reference-location").as_str()` returned
    /// `None` and silently fell back to the `document` default. With
    /// `as_plain_text()` the value resolves, so `reference-location: margin`
    /// produces a `margin-note` reference and NO document footnotes section.
    #[test]
    fn test_render_to_file_honors_margin_reference_location() {
        let temp = TempDir::new().unwrap();
        let input_path = temp.path().join("margin.qmd");

        fs::write(
            &input_path,
            "---\nformat: html\nreference-location: margin\n---\nRef.^[the body]\n",
        )
        .unwrap();

        let runtime = Arc::new(NativeRuntime::new());
        let options = RenderToFileOptions::default();

        let result = render_to_file(&input_path, "html", &options, runtime).unwrap();
        let html = fs::read_to_string(&result.output_path).unwrap();

        assert!(
            html.contains("margin-note"),
            "expected margin-note class for reference-location: margin; got:\n{html}"
        );
        assert!(
            !html.contains("doc-endnotes"),
            "margin mode must NOT emit a document footnotes section; got:\n{html}"
        );
    }

    // ── bd-y89ihf0i: front-matter string options must resolve through
    //    `as_plain_text()` ───────────────────────────────────────────────
    //
    // Each option below is a user-authored metadata string that, in
    // document-metadata context, is parsed as markdown and stored as
    // `ConfigValueKind::PandocInlines`. Reading it with `as_str()` returns
    // `None`, so the feature is silently dropped. These end-to-end tests write
    // the option as a bare front-matter string and assert the rendered HTML
    // reflects it. They FAIL on `as_str()` and PASS on `as_plain_text()`.

    /// `license: <string>` (bare) must produce the appendix "Reuse" section.
    /// With `as_str()` the bare value is `PandocInlines`, so
    /// `create_license_section` drops the section entirely.
    #[test]
    fn test_render_to_file_honors_license_string() {
        let temp = TempDir::new().unwrap();
        let input_path = temp.path().join("license.qmd");
        fs::write(
            &input_path,
            "---\nformat: html\nlicense: CC BY-SA 4.0\n---\nBody.\n",
        )
        .unwrap();

        let runtime = Arc::new(NativeRuntime::new());
        let result = render_to_file(
            &input_path,
            "html",
            &RenderToFileOptions::default(),
            runtime,
        )
        .unwrap();
        let html = fs::read_to_string(&result.output_path).unwrap();

        assert!(
            html.contains("quarto-reuse"),
            "expected a quarto-reuse license section for bare `license:` string; got:\n{html}"
        );
        assert!(
            html.contains("CC BY-SA 4.0"),
            "expected the license text in output; got:\n{html}"
        );
    }

    /// `license:` with a nested `text:` bare string must resolve.
    #[test]
    fn test_render_to_file_honors_license_nested_text() {
        let temp = TempDir::new().unwrap();
        let input_path = temp.path().join("license_nested.qmd");
        fs::write(
            &input_path,
            "---\nformat: html\nlicense:\n  text: My Custom Reuse Terms\n---\nBody.\n",
        )
        .unwrap();

        let runtime = Arc::new(NativeRuntime::new());
        let result = render_to_file(
            &input_path,
            "html",
            &RenderToFileOptions::default(),
            runtime,
        )
        .unwrap();
        let html = fs::read_to_string(&result.output_path).unwrap();

        assert!(
            html.contains("My Custom Reuse Terms"),
            "expected nested license.text in output; got:\n{html}"
        );
    }

    /// `copyright: <string>` (bare) must produce the appendix "Copyright" section.
    #[test]
    fn test_render_to_file_honors_copyright_string() {
        let temp = TempDir::new().unwrap();
        let input_path = temp.path().join("copyright.qmd");
        fs::write(
            &input_path,
            "---\nformat: html\ncopyright: Copyright 2026 ACME Corp\n---\nBody.\n",
        )
        .unwrap();

        let runtime = Arc::new(NativeRuntime::new());
        let result = render_to_file(
            &input_path,
            "html",
            &RenderToFileOptions::default(),
            runtime,
        )
        .unwrap();
        let html = fs::read_to_string(&result.output_path).unwrap();

        assert!(
            html.contains("quarto-copyright"),
            "expected a quarto-copyright section for bare `copyright:` string; got:\n{html}"
        );
        assert!(
            html.contains("Copyright 2026 ACME Corp"),
            "expected the copyright text in output; got:\n{html}"
        );
    }

    /// `citation:` with a nested `url:` bare string must produce the
    /// "For attribution" citation link.
    #[test]
    fn test_render_to_file_honors_citation_url() {
        let temp = TempDir::new().unwrap();
        let input_path = temp.path().join("citation.qmd");
        fs::write(
            &input_path,
            "---\nformat: html\ncitation:\n  url: https://example.com/cite\n---\nBody.\n",
        )
        .unwrap();

        let runtime = Arc::new(NativeRuntime::new());
        let result = render_to_file(
            &input_path,
            "html",
            &RenderToFileOptions::default(),
            runtime,
        )
        .unwrap();
        let html = fs::read_to_string(&result.output_path).unwrap();

        assert!(
            html.contains("For attribution"),
            "expected the citation-url attribution text; got:\n{html}"
        );
        assert!(
            html.contains("example.com/cite"),
            "expected the citation url in output; got:\n{html}"
        );
    }

    /// `appendix-style: plain` (bare) must set the appendix container class to
    /// `plain`. A user `::: {.appendix}` div forces a container regardless of
    /// other metadata. With `as_str()` the bare value falls back to the
    /// `default` style.
    #[test]
    fn test_render_to_file_honors_appendix_style_plain() {
        let temp = TempDir::new().unwrap();
        let input_path = temp.path().join("appendix_style.qmd");
        fs::write(
            &input_path,
            "---\nformat: html\nappendix-style: plain\n---\nBody.\n\n::: {.appendix}\nExtra material.\n:::\n",
        )
        .unwrap();

        let runtime = Arc::new(NativeRuntime::new());
        let result = render_to_file(
            &input_path,
            "html",
            &RenderToFileOptions::default(),
            runtime,
        )
        .unwrap();
        let html = fs::read_to_string(&result.output_path).unwrap();

        assert!(
            html.contains("quarto-appendix"),
            "expected an appendix container; got:\n{html}"
        );
        assert!(
            html.contains(r#"id="quarto-appendix" class="plain""#)
                || html.contains(r#"class="plain" id="quarto-appendix""#),
            "expected appendix container with class `plain` for bare `appendix-style: plain`; got:\n{html}"
        );
    }

    /// `toc: auto` (bare string) must trigger TOC generation. With `as_str()`
    /// the bare value is `PandocInlines`, so the `== Some("auto")` comparison
    /// fails and no TOC is generated.
    #[test]
    fn test_render_to_file_honors_toc_auto_string() {
        let temp = TempDir::new().unwrap();
        let input_path = temp.path().join("toc_auto.qmd");
        fs::write(
            &input_path,
            "---\nformat: html\ntoc: auto\n---\nIntro.\n\n## A Section\n\nText.\n",
        )
        .unwrap();

        let runtime = Arc::new(NativeRuntime::new());
        let result = render_to_file(
            &input_path,
            "html",
            &RenderToFileOptions::default(),
            runtime,
        )
        .unwrap();
        let html = fs::read_to_string(&result.output_path).unwrap();

        assert!(
            html.contains("nav-link"),
            "expected a generated TOC (nav-link) for `toc: auto`; got:\n{html}"
        );
    }

    /// `toc-title: <string>` (bare) must appear as the TOC heading. With
    /// `as_str()` the bare value is `PandocInlines`, so it falls back to the
    /// default "Table of Contents".
    #[test]
    fn test_render_to_file_honors_toc_title() {
        let temp = TempDir::new().unwrap();
        let input_path = temp.path().join("toc_title.qmd");
        fs::write(
            &input_path,
            "---\nformat: html\ntoc: true\ntoc-title: My Custom Contents\n---\nIntro.\n\n## A Section\n\nText.\n",
        )
        .unwrap();

        let runtime = Arc::new(NativeRuntime::new());
        let result = render_to_file(
            &input_path,
            "html",
            &RenderToFileOptions::default(),
            runtime,
        )
        .unwrap();
        let html = fs::read_to_string(&result.output_path).unwrap();

        assert!(
            html.contains("My Custom Contents"),
            "expected the custom toc-title in output; got:\n{html}"
        );
        assert!(
            !html.contains("Table of Contents"),
            "custom toc-title must replace the default; got:\n{html}"
        );
    }

    #[test]
    fn test_render_to_file_creates_output() {
        let temp = TempDir::new().unwrap();
        let input_path = temp.path().join("test.qmd");

        // Create a minimal QMD file
        fs::write(
            &input_path,
            r#"---
title: Test
---

Hello world.
"#,
        )
        .unwrap();

        let runtime = Arc::new(NativeRuntime::new());
        let options = RenderToFileOptions::default();

        let result = render_to_file(&input_path, "html", &options, runtime).unwrap();

        // Check output file was created
        assert!(result.output_path.exists());
        assert!(result.output_path.ends_with("test.html"));

        // Check resources directory was created
        assert!(result.resources_dir.exists());
        assert!(result.resources_dir.ends_with("test_files"));

        // Check HTML contains expected content
        let html = fs::read_to_string(&result.output_path).unwrap();
        assert!(html.contains("Hello world"));
        assert!(html.contains("<title>"));
    }

    /// bd-po3gn41h: end-to-end check that a named/reference-style
    /// footnote (`[^id]` reference + a `::: ^id … :::` block definition)
    /// resolves through the real `q2 render` file path. This drives
    /// `render_to_file`, the exact entry point the native CLI pass-2
    /// renderer uses (`project/pass2_renderer.rs` -> `render_document_to_file`).
    ///
    /// Before the fix, the body kept an empty
    /// `<span class="quarto-note-reference" data-reference-id="bk">` and
    /// the definition's text ("Note.") was dropped with no footnotes
    /// section emitted.
    #[test]
    fn test_render_to_file_resolves_named_footnote_reference() {
        let temp = TempDir::new().unwrap();
        let input_path = temp.path().join("named-fn.qmd");

        fs::write(
            &input_path,
            "---\nformat: html\n---\nRef.[^bk]\n\n::: ^bk\nNote.\n:::\n",
        )
        .unwrap();

        let runtime = Arc::new(NativeRuntime::new());
        let options = RenderToFileOptions::default();

        let result = render_to_file(&input_path, "html", &options, runtime).unwrap();
        let html = fs::read_to_string(&result.output_path).unwrap();

        // The footnote reference must resolve to the standard fnref link...
        assert!(
            html.contains("footnote-ref"),
            "expected a footnote-ref link for the resolved [^bk] reference; got:\n{html}"
        );
        // ...and a footnotes section must be emitted carrying the body.
        assert!(
            html.contains("doc-endnotes"),
            "expected a doc-endnotes footnotes section; got:\n{html}"
        );
        assert!(
            html.contains("Note."),
            "expected the definition's text (\"Note.\") in the footnotes section; got:\n{html}"
        );
        // The unresolved lowering span must be gone.
        assert!(
            !html.contains("quarto-note-reference"),
            "the empty quarto-note-reference span should have been resolved away; got:\n{html}"
        );
    }

    #[test]
    fn test_render_to_file_with_theme() {
        let temp = TempDir::new().unwrap();
        let input_path = temp.path().join("themed.qmd");

        // Create a QMD file with theme
        fs::write(
            &input_path,
            r#"---
title: Themed Doc
format:
  html:
    theme: cosmo
---

Themed content.
"#,
        )
        .unwrap();

        let runtime = Arc::new(NativeRuntime::new());
        let options = RenderToFileOptions::default();

        let result = render_to_file(&input_path, "html", &options, runtime).unwrap();

        // Check CSS was written
        let css_path = result.resources_dir.join("styles.css");
        assert!(css_path.exists());

        let css = fs::read_to_string(&css_path).unwrap();
        assert!(
            css.contains(".btn"),
            "CSS should contain compiled Bootstrap from cosmo theme"
        );
    }

    #[test]
    fn test_render_document_to_file_with_project() {
        let temp = TempDir::new().unwrap();
        let input_path = temp.path().join("doc.qmd");

        fs::write(
            &input_path,
            r#"---
title: Doc
---

Content.
"#,
        )
        .unwrap();

        let runtime = Arc::new(NativeRuntime::new());

        // Pre-discover project
        let project = ProjectContext::discover(&input_path, runtime.as_ref()).unwrap();

        let options = RenderToFileOptions::default();

        // Render with pre-discovered project (standalone call: no
        // project_artifacts accumulator, so the function flushes
        // Project-scoped artifacts via the resolver itself).
        let result = render_document_to_file(
            &input_path,
            "html",
            &options,
            Some(&project),
            runtime,
            None,
            None,
            None,
        )
        .unwrap();

        assert!(result.output_path.exists());
    }
}
