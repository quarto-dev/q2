/*
 * stage/stages/mod.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Concrete pipeline stage implementations.
 */

//! Concrete pipeline stage implementations.
//!
//! This module contains the actual stage implementations that make up
//! the Quarto render pipeline:
//!
//! - [`ParseDocumentStage`] - Parse QMD content to Pandoc AST
//! - [`EngineExecutionStage`] - Execute code cells via knitr/jupyter/markdown
//! - [`AstTransformsStage`] - Apply Quarto-specific AST transforms
//! - [`RenderHtmlBodyStage`] - Render AST to HTML body
//! - [`ApplyTemplateStage`] - Apply HTML template to rendered body

mod apply_template;
mod ast_transforms;
mod engine_execution;
mod parse_document;
mod render_html;

pub use apply_template::{ApplyTemplateConfig, ApplyTemplateStage};
pub use ast_transforms::AstTransformsStage;
pub use engine_execution::EngineExecutionStage;
pub use parse_document::ParseDocumentStage;
pub use render_html::RenderHtmlBodyStage;
