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
pub mod engine;
pub mod error;
pub mod format;
pub mod pipeline;
pub mod project;
pub mod render;
pub mod resources;
pub mod stage;
pub mod template;
pub mod transform;
pub mod transforms;

// Re-export commonly used types
pub use artifact::{Artifact, ArtifactStore};
pub use error::{ParseError, QuartoError, Result};
pub use format::{Format, FormatIdentifier, extract_format_metadata};
pub use pipeline::{
    DEFAULT_CSS_ARTIFACT_PATH, HtmlRenderConfig, RenderOutput, build_html_pipeline,
    build_html_pipeline_stages, build_html_pipeline_with_stages, build_wasm_html_pipeline,
    render_qmd_to_html,
};
pub use project::{DocumentInfo, ProjectConfig, ProjectContext, ProjectType};
pub use render::{BinaryDependencies, RenderContext, RenderOptions, RenderResult};
pub use transform::{AstTransform, TransformPipeline};
pub use transforms::{
    CalloutResolveTransform, CalloutTransform, MetadataNormalizeTransform,
    ResourceCollectorTransform, TitleBlockTransform,
};
