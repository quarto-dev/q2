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
//! - [`MetadataNormalizeTransform`] - Normalizes document metadata (adds pagetitle, etc.)
//! - [`ResourceCollectorTransform`] - Collects resource dependencies (images, etc.)
//!
//! These transforms implement [`AstTransform`](crate::transform::AstTransform) and
//! can be added to a [`TransformPipeline`](crate::transform::TransformPipeline).

mod callout;
mod metadata_normalize;
mod resource_collector;

pub use callout::CalloutTransform;
pub use metadata_normalize::MetadataNormalizeTransform;
pub use resource_collector::ResourceCollectorTransform;
