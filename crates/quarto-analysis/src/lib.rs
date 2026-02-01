//! Document analysis infrastructure for Quarto.
//!
//! This crate provides the shared infrastructure for analyzing Quarto documents
//! without performing full rendering. It is used by both:
//!
//! - **quarto-lsp-core** - For IDE features like document outline, diagnostics
//! - **quarto-core** - For analysis steps that run before full rendering
//!
//! # Key Concepts
//!
//! ## AnalysisContext
//!
//! The [`AnalysisContext`] trait defines the interface for analysis operations.
//! It provides access to document metadata, source location tracking, and
//! diagnostic reporting.
//!
//! Two implementations exist:
//! - [`DocumentAnalysisContext`] - Lightweight context for LSP operations
//! - `RenderContext` (in quarto-core) - Full context that also implements this trait
//!
//! ## Analysis Transforms
//!
//! Analysis transforms are AST transformations that can run at "LSP speed" -
//! they don't perform I/O, code execution, or other slow operations. Examples:
//!
//! - [`MetaShortcodeTransform`](transforms::MetaShortcodeTransform) - Resolves `{{< meta key >}}` shortcodes
//!
//! # Example
//!
//! ```rust,ignore
//! use quarto_analysis::{DocumentAnalysisContext, AnalysisContext};
//! use quarto_analysis::transforms::{run_analysis_transforms, MetaShortcodeTransform};
//!
//! // Create a lightweight analysis context
//! let mut ctx = DocumentAnalysisContext::new(metadata, source_context);
//!
//! // Run analysis transforms
//! let transforms: Vec<&dyn AnalysisTransform> = vec![&MetaShortcodeTransform];
//! run_analysis_transforms(&mut pandoc, &mut ctx, &transforms)?;
//!
//! // Check for diagnostics
//! for diagnostic in ctx.diagnostics() {
//!     println!("Warning: {}", diagnostic.title);
//! }
//! ```

mod context;
pub mod transforms;

pub use context::{AnalysisContext, DocumentAnalysisContext};
