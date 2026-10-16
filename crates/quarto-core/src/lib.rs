//! Core rendering infrastructure for Quarto
//!
//! This crate contains the core rendering pipeline and functionality
//! that powers the Quarto CLI.
//!
//! # Architecture
//!
//! The render pipeline is organized around these key types:
//!
//! - [`ProjectContext`] - Project-level configuration and state
//! - [`RenderContext`] - Per-render mutable state passed through pipeline stages
//! - [`ArtifactStore`] - Unified storage for dependencies and intermediates
//! - [`Format`] - Output format specification
//!
//! # Example
//!
//! ```ignore
//! use quarto_core::{ProjectContext, RenderContext, Format, BinaryDependencies};
//! use quarto_system_runtime::NativeRuntime;
//!
//! // Create a runtime for system operations
//! let runtime = NativeRuntime::new();
//!
//! // Discover project from input file
//! let project = ProjectContext::discover("document.qmd", &runtime)?;
//!
//! // Get the document to render
//! let document = &project.files[0];
//!
//! // Set up render context
//! let format = Format::html();
//! let binaries = BinaryDependencies::discover(&runtime);
//! let mut ctx = RenderContext::new(&project, document, &format, &binaries);
//!
//! // Pipeline stages use ctx.artifacts for storing intermediates
//! // ...
//! ```

pub mod artifact;
pub mod artifact_flush;
pub mod ast_walk;
pub mod attribution;
#[cfg(not(target_arch = "wasm32"))]
pub mod book_download;
pub mod brand_fonts;
pub mod cell_options;
#[cfg(not(target_arch = "wasm32"))]
pub mod child_runner;
pub mod components;
pub mod config_sources;
pub mod crossref;
pub mod dates;
pub mod dependency;
pub mod diagnostic_policy;
pub mod document_profile;
pub mod editorial_marks;
pub mod engine;
pub mod error;
pub mod extension;
pub mod filter_resolve;
pub mod format;
pub mod format_resolution;
pub mod get_config;
pub mod glob;
pub mod import;
pub mod inline_css;
pub mod language;
pub mod lmd;
pub mod math_method;
pub mod metadata;
pub mod metadata_flag;
pub mod metadata_layers;
pub mod output_ext;
pub mod output_sink;
pub mod overlay_runtime;
pub mod pandoc_filters;
pub mod pandoc_formats;
pub mod pandoc_request;
pub mod pipeline;
#[cfg(not(target_arch = "wasm32"))]
pub mod preview_out;
pub mod project;
pub mod project_resources;
pub mod render;
#[cfg(not(target_arch = "wasm32"))]
pub mod render_to_file;
pub mod resource_copy_diagnostics;
pub mod resource_resolver;
pub mod resources;
pub mod revealjs;
pub mod stage;
pub mod template;
pub mod theme_diagnostic;
pub mod transform;
pub mod transforms;
pub mod typst_google_fonts;

// Re-export commonly used types
pub use artifact::{Artifact, ArtifactMergeConflict, ArtifactScope, ArtifactStore, MergeStats};
pub use artifact_flush::flush_artifacts_to_vfs;
pub use error::{ParseError, QuartoError, Result};
pub use format::{Format, FormatIdentifier};
pub use pipeline::{
    DEFAULT_CSS_ARTIFACT_PATH, HtmlRenderConfig, PreviewAstOutput, RenderOutput,
    build_html_pipeline, build_html_pipeline_stages, build_html_pipeline_with_stages,
    render_qmd_to_html, render_qmd_to_preview_ast,
};
pub use project::book::BookScope;
#[cfg(not(target_arch = "wasm32"))]
pub use project::book::render_chapter_alone;
pub use project::{DocumentInfo, ProjectConfig, ProjectContext, ProjectKind};
pub use render::{BinaryDependencies, RenderContext, RenderOptions, RenderResult};
pub use resource_resolver::ResourceResolverContext;
pub use transform::{AstTransform, TransformPipeline};
pub use transforms::{
    CalloutResolveTransform, CalloutTransform, MetadataNormalizeTransform,
    ResourceCollectorTransform, TitleBlockTransform,
};

/// Return the Quarto version this build reports.
///
/// Used by `build_engine_registry` to populate `HostGlobalConfig.quarto_version`.
/// `quarto-core`'s version tracks the overall Quarto release version; a
/// nightly build reports its `QUARTO_VERSION_OVERRIDE` instead
/// (bd-p4ljdp2e), which is why this goes through `quarto_util` rather
/// than reading `CARGO_PKG_VERSION` directly.
pub fn version() -> &'static str {
    quarto_util::cli_version()
}

// Pandoc binary resolution plus version gate (`Q-20-1`/`Q-20-2`), for callers
// outside the render pipeline (the preview server's import route).
#[cfg(not(target_arch = "wasm32"))]
pub use stage::stages::resolve_and_gate_pandoc;

// Re-export render-to-file types (native only)
#[cfg(not(target_arch = "wasm32"))]
pub use render_to_file::{
    RenderToFileOptions, RenderToFileResult, TypstTarget, render_document_to_file, render_to_file,
};
