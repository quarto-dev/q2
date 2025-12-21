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
//!
//! // Discover project from input file
//! let project = ProjectContext::discover("document.qmd")?;
//!
//! // Get the document to render
//! let document = &project.files[0];
//!
//! // Set up render context
//! let format = Format::html();
//! let binaries = BinaryDependencies::discover();
//! let mut ctx = RenderContext::new(&project, document, &format, &binaries);
//!
//! // Pipeline stages use ctx.artifacts for storing intermediates
//! // ...
//! ```

pub mod artifact;
pub mod error;
pub mod format;
pub mod project;
pub mod render;
pub mod transform;
pub mod transforms;

// Re-export commonly used types
pub use artifact::{Artifact, ArtifactStore};
pub use error::{QuartoError, Result};
pub use format::{Format, FormatIdentifier};
pub use project::{DocumentInfo, ProjectConfig, ProjectContext, ProjectType};
pub use render::{BinaryDependencies, RenderContext, RenderOptions, RenderResult};
pub use transform::{AstTransform, TransformPipeline};
pub use transforms::{MetadataNormalizeTransform, ResourceCollectorTransform};
