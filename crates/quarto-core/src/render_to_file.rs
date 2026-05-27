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
use crate::error::QuartoError;
use crate::format::Format;
use crate::output_sink::OutputSink;
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
    render_document_to_file(input_path, format, options, None, runtime, None, None)
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
    let render_output = pollster::block_on(render_qmd_to_html(
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

    let has_shared_lib = !project_type.lib_dir().is_empty();
    match (project_artifacts, has_shared_lib) {
        (Some(dest), true) => {
            // Real multi-doc project (e.g. website): drain into
            // the orchestrator's accumulator; post_render flushes.
            dest.merge_into_project(drained).map_err(|e| {
                QuartoError::other(format!(
                    "Project-scoped artifact merge failed for {}: {}",
                    input_path.display(),
                    e
                ))
            })?;
        }
        _ => {
            // Default project or standalone call: enqueue Project-
            // scoped artifacts via the resolver. For lib_dir == ""
            // the resolver routes them under `{stem}_files/`,
            // preserving pre-Phase-5 layout.
            enqueue_artifacts(&drained, &resolver, ArtifactScope::Project, &mut sink)?;
        }
    }

    // Drain user-resource copy intents (images etc. collected by
    // `ResourceCollectorTransform`) into the sink. The sink will
    // skip ops whose src and dest canonicalize equal — the common
    // single-doc case where output_dir == input_dir and the
    // resource is already where the HTML expects it.
    let resource_copies = std::mem::take(&mut ctx.resource_copies);
    for (src, dest) in resource_copies {
        sink.copy(src, dest).map_err(QuartoError::from)?;
    }

    // Output HTML also goes through the sink so the whole render's
    // destructive output is validated and committed atomically.
    sink.write(output_path.clone(), render_output.html.as_bytes().to_vec())
        .map_err(QuartoError::from)?;

    sink.flush(runtime.as_ref()).map_err(QuartoError::from)?;

    debug!("Output: {}", output_path.display());

    let resource_report = std::mem::take(&mut ctx.resource_report);
    Ok(RenderToFileResult {
        output_path,
        resources_dir: resource_paths.resource_dir,
        render_output,
        resource_report,
    })
}

/// Enqueue every artifact in `store` whose scope matches `scope_filter`
/// into `sink` at its resolver-determined on-disk location. Skips
/// artifacts without a `path`.
///
/// The caller owns the sink lifecycle (construct, enqueue producers,
/// flush). Used by `render_document_to_file` (Page scope, per-doc;
/// Project scope when standalone) and by
/// `WebsiteProjectType::post_render` for project-shared artifacts
/// (via [`crate::project::website_post_render::flush_site_libs`]).
///
/// Iteration is sorted-key so the resulting flush order is
/// deterministic across runs / platforms.
#[cfg(not(target_arch = "wasm32"))]
pub fn enqueue_artifacts(
    store: &ArtifactStore,
    resolver: &ResourceResolverContext,
    scope_filter: ArtifactScope,
    sink: &mut OutputSink,
) -> Result<()> {
    let mut entries: Vec<(&str, &crate::artifact::Artifact)> = store
        .iter()
        .filter(|(_, a)| a.scope == scope_filter)
        .collect();
    entries.sort_by(|a, b| a.0.cmp(b.0));

    for (_, artifact) in entries {
        let Some(path) = &artifact.path else { continue };
        let on_disk = resolver.on_disk_path_for(artifact.scope, path);
        sink.write(on_disk, artifact.content.clone())
            .map_err(QuartoError::from)?;
    }
    Ok(())
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
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."));

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
    Format::from_format_string(name).map_err(|e| QuartoError::other(e).into())
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
        )
        .unwrap();

        assert!(result.output_path.exists());
    }
}
