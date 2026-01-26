/*
 * transforms/mod.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Essential AST transforms for the render pipeline.
 */

//! Essential AST transforms for the render pipeline.
//!
//! This module contains the core transforms used in the Quarto render pipeline:
//!
//! - [`CalloutTransform`] - Converts callout Divs to CustomNodes
//! - [`CalloutResolveTransform`] - Resolves Callout CustomNodes to standard Div structure
//! - [`MetadataNormalizeTransform`] - Normalizes document metadata (adds pagetitle, etc.)
//! - [`ResourceCollectorTransform`] - Collects resource dependencies (images, etc.)
//! - [`SectionizeTransform`] - Wraps headers in section Divs (analogous to Pandoc's --section-divs)
//! - [`TitleBlockTransform`] - Adds title header from metadata if not present
//!
//! These transforms implement [`AstTransform`](crate::transform::AstTransform) and
//! can be added to a [`TransformPipeline`](crate::transform::TransformPipeline).

mod callout;
mod callout_resolve;
mod metadata_normalize;
mod resource_collector;
mod sectionize;
mod title_block;

pub use callout::CalloutTransform;
pub use callout_resolve::CalloutResolveTransform;
pub use metadata_normalize::MetadataNormalizeTransform;
pub use resource_collector::ResourceCollectorTransform;
pub use sectionize::SectionizeTransform;
pub use title_block::TitleBlockTransform;
