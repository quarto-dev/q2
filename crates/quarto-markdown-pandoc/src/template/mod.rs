/*
 * template/mod.rs
 * Copyright (c) 2025 Posit, PBC
 */

// These re-exports form the public API for template consumers.
// They may not be used within this crate but are available for external use.
#![allow(unused_imports)]

//! Document template support for quarto-markdown-pandoc.
//!
//! This module provides template rendering capabilities using the quarto-doctemplate
//! engine. Templates are Pandoc-compatible and support variable interpolation,
//! conditionals, loops, and partials.
//!
//! # Design Principles
//!
//! Templates are pure functions of metadata plus body. The only implicitly-provided
//! variable is `$body$` (the rendered document content). All other variables must
//! come from document metadata.
//!
//! This design maximizes composability and WASM compatibility by avoiding hidden
//! environmental dependencies. See `docs/template-variables.md` for full rationale.
//!
//! # Template Bundles
//!
//! Templates can be provided as self-contained bundles (JSON format):
//!
//! ```json
//! {
//!   "version": "1.0.0",
//!   "main": "<!DOCTYPE html><html>$body$</html>",
//!   "partials": {
//!     "header": "<header>$title$</header>"
//!   }
//! }
//! ```
//!
//! # Feature Flags
//!
//! - `template-fs`: Enables filesystem-based template resolution. Disabled for WASM.

pub mod builtin;
pub mod bundle;
pub mod context;
pub mod render;

// Re-export main types for convenience
pub use builtin::{BUILTIN_TEMPLATE_NAMES, get_builtin_template, is_builtin_template};
pub use bundle::TemplateBundle;
pub use context::{MetaWriter, meta_to_template_value, pandoc_to_context};
pub use render::{BodyFormat, TemplateRenderError, render_with_bundle, render_with_resolver};

// Re-export quarto-doctemplate types that users may need
pub use quarto_doctemplate::{
    MemoryResolver, NullResolver, PartialResolver, Template, TemplateContext, TemplateValue,
};

// Feature-gated filesystem support
#[cfg(feature = "template-fs")]
pub use quarto_doctemplate::FileSystemResolver;
