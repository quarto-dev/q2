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
//! - [`ListingItemInfoStage`] - Auto-fill `meta.listing-item.*` (L1, `bd-izqh`)
//! - [`DocumentProfileStage`] - Extract a static profile at the pipeline checkpoint
//! - [`UnwrapProfileStage`] - Hand the AST back to downstream stages
//! - [`EngineExecutionStage`] - Execute code cells via knitr/jupyter/markdown
//! - [`UserFiltersStage`] - Apply user-specified filters (Lua, JSON, citeproc)
//! - [`AstTransformsStage`] - Apply Quarto-specific AST transforms
//! - [`RenderHtmlBodyStage`] - Render AST to HTML body
//! - [`ApplyTemplateStage`] - Apply HTML template to rendered body

mod apply_template;
mod ast_transforms;
// Bootstrap JS is a native-only stage: hub-client's iframe-per-render
// preview blows away stateful Bootstrap components, and excluding the
// stage on WASM keeps the 80KB bundle out of the WASM binary. See the
// module's own docs for the full rationale.
#[cfg(not(target_arch = "wasm32"))]
mod bootstrap_js;
mod code_highlight;
mod compile_theme_css;
mod document_profile;
mod engine_execution;
mod include_expansion;
mod include_resolve;
mod link_resolution;
mod listing_item_info;
// Math-mode stage: injects a math-rendering JS engine (MathJax / KaTeX)
// when the document contains Math elements. Included on both native
// and WASM pipelines (math display is safe under iframe reinit).
mod math_js;
mod metadata_merge;
mod parse_document;
mod pre_engine_sugaring;
mod render_html;
mod resource_report;
mod unwrap_profile;
mod user_filters;

pub use apply_template::{ApplyTemplateConfig, ApplyTemplateStage};
pub use ast_transforms::AstTransformsStage;
#[cfg(not(target_arch = "wasm32"))]
pub use bootstrap_js::BootstrapJsStage;
pub use code_highlight::CodeHighlightStage;
pub use compile_theme_css::CompileThemeCssStage;
pub use document_profile::DocumentProfileStage;
pub use engine_execution::{ENGINE_CAPTURE_KIND, EngineExecutionStage};
pub use include_expansion::IncludeExpansionStage;
pub use include_resolve::IncludeResolveStage;
pub use link_resolution::LinkResolutionStage;
pub use listing_item_info::ListingItemInfoStage;
pub use math_js::{DEFAULT_KATEX_URL_BASE, DEFAULT_MATHJAX_URL, MathEngine, MathJsStage};
pub use metadata_merge::MetadataMergeStage;
pub use parse_document::ParseDocumentStage;
pub use pre_engine_sugaring::PreEngineSugaringStage;
pub use render_html::RenderHtmlBodyStage;
pub use resource_report::ResourceReportStage;
pub use unwrap_profile::UnwrapProfileStage;
pub use user_filters::UserFiltersStage;
