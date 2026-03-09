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
//! - [`MetadataMergeStage`] - Merge project/directory/document/runtime metadata
//! - [`AstTransformsStage`] - Apply Quarto-specific AST transforms
//! - [`RenderHtmlBodyStage`] - Render AST to HTML body
//! - [`ApplyTemplateStage`] - Apply HTML template to rendered body

mod apply_template;
mod ast_transforms;
mod compile_theme_css;
mod engine_execution;
mod metadata_merge;
mod parse_document;
mod render_html;

pub use apply_template::{ApplyTemplateConfig, ApplyTemplateStage};
pub use ast_transforms::AstTransformsStage;
pub use compile_theme_css::CompileThemeCssStage;
pub use engine_execution::EngineExecutionStage;
pub use metadata_merge::MetadataMergeStage;
pub use parse_document::ParseDocumentStage;
pub use render_html::RenderHtmlBodyStage;
