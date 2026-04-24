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
//! - [`MetadataMergeStage`] - Merge project/directory/document/runtime metadata
//! - [`DocumentProfileStage`] - Extract a static profile at the pipeline checkpoint
//! - [`UnwrapProfileStage`] - Hand the AST back to downstream stages
//! - [`EngineExecutionStage`] - Execute code cells via knitr/jupyter/markdown
//! - [`UserFiltersStage`] - Apply user-specified filters (Lua, JSON, citeproc)
//! - [`AstTransformsStage`] - Apply Quarto-specific AST transforms
//! - [`RenderHtmlBodyStage`] - Render AST to HTML body
//! - [`ApplyTemplateStage`] - Apply HTML template to rendered body

mod apply_template;
mod ast_transforms;
mod code_highlight;
mod compile_theme_css;
mod document_profile;
mod engine_execution;
mod include_expansion;
mod metadata_merge;
mod parse_document;
mod pre_engine_sugaring;
mod render_html;
mod unwrap_profile;
mod user_filters;

pub use apply_template::{ApplyTemplateConfig, ApplyTemplateStage};
pub use ast_transforms::AstTransformsStage;
pub use code_highlight::CodeHighlightStage;
pub use compile_theme_css::CompileThemeCssStage;
pub use document_profile::DocumentProfileStage;
pub use engine_execution::EngineExecutionStage;
pub use include_expansion::IncludeExpansionStage;
pub use metadata_merge::MetadataMergeStage;
pub use parse_document::ParseDocumentStage;
pub use pre_engine_sugaring::PreEngineSugaringStage;
pub use render_html::RenderHtmlBodyStage;
pub use unwrap_profile::UnwrapProfileStage;
pub use user_filters::UserFiltersStage;
