/*
 * lib.rs
 * Copyright (c) 2025 Posit, PBC
 */

//! Pandoc-compatible document template engine for Quarto.
//!
//! This crate provides a template engine that is compatible with Pandoc's
//! [doctemplates](https://github.com/jgm/doctemplates) library. It supports:
//!
//! - Variable interpolation: `$variable$` or `${variable}`
//! - Nested field access: `$employee.salary$`
//! - Conditionals: `$if(var)$...$else$...$endif$`
//! - For loops: `$for(items)$...$sep$...$endfor$`
//! - Partials: `$partial()$` or `$var:partial()$`
//! - Pipes: `$var/uppercase$`, `$var/left 20 "" ""$`
//! - Nesting directive: `$^$` for indentation control
//! - Breakable spaces: `$~$...$~$`
//! - Comments: `$-- comment`
//!
//! # Architecture
//!
//! The template engine is **independent of Pandoc AST types**. It defines its own
//! [`TemplateValue`] and [`TemplateContext`] types. Conversion from Pandoc's
//! `MetaValue` to `TemplateValue` happens in the writer layer (not in this crate).
//!
//! # Example
//!
//! ```ignore
//! use quarto_doctemplate::{Template, TemplateContext, TemplateValue};
//!
//! // Parse a template
//! let template = Template::compile("Hello, $name$!")?;
//!
//! // Create a context with variables
//! let mut ctx = TemplateContext::new();
//! ctx.insert("name", TemplateValue::String("World".to_string()));
//!
//! // Render the template
//! let output = template.render(&ctx)?;
//! assert_eq!(output, "Hello, World!");
//! ```

pub mod ast;
pub mod context;
pub mod doc;
pub mod error;
pub mod eval_context;
pub mod evaluator;
pub mod parser;
pub mod resolver;

// Re-export main types at crate root
pub use ast::{
    BreakableSpace, Comment, Conditional, ForLoop, Literal, Nesting, Partial, Pipe, PipeArg,
    TemplateNode, VariableRef,
};
pub use context::{TemplateContext, TemplateValue};
pub use doc::Doc;
pub use error::TemplateError;
pub use eval_context::{DiagnosticCollector, EvalContext};
pub use parser::Template;
pub use resolver::{FileSystemResolver, MemoryResolver, NullResolver, PartialResolver};
