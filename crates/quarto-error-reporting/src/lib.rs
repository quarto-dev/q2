//! Error reporting and diagnostic messages for Quarto.
//!
//! This crate provides a structured approach to error reporting, inspired by:
//! - **ariadne**: Visual compiler-quality error messages with source context
//! - **R cli package**: Semantic, structured text output
//! - **Tidyverse style guide**: Best practices for error message content
//!
//! # Architecture
//!
//! The crate is organized into several phases:
//!
//! ## Phase 1: Core Types (Current)
//! - [`DiagnosticMessage`]: The main error message structure
//! - [`MessageContent`]: Content representation (Plain, Markdown, or Pandoc AST)
//! - [`DetailItem`]: Individual detail bullets with error/info/note kinds
//! - [`DiagnosticKind`]: Error, Warning, Info, etc.
//!
//! ## Phase 2: Rendering (Planned)
//! - Integration with ariadne for visual terminal output
//! - JSON serialization for machine-readable output
//!
//! ## Phase 3: Console Helpers (Planned)
//! - High-level console output primitives
//! - ANSI writer for Pandoc AST (requires discussion)
//!
//! ## Phase 4: Builder API (Planned)
//! - Tidyverse-style builder methods (`.problem()`, `.add_detail()`, `.add_hint()`)
//!
//! # Design Decisions
//!
//! - **Markdown-first**: Messages use Markdown strings, converted to Pandoc AST internally
//! - **Semantic markup**: Use Pandoc span syntax for semantic classes: `` `text`{.class} ``
//! - **Multiple outputs**: ANSI terminal, HTML, and JSON formats
//! - **Rust-idiomatic**: Designed for Rust ergonomics (WASM for cross-language if needed)
//!
//! # Example Usage (Future)
//!
//! ```ignore
//! use quarto_error_reporting::DiagnosticMessage;
//!
//! let error = DiagnosticMessage::builder()
//!     .error("Unclosed code block")
//!     .problem("Code block started but never closed")
//!     .add_detail("The code block starting with `` ```{python} `` was never closed")
//!     .at_location(opening_span)
//!     .add_hint("Did you forget the closing `` ``` ``?")
//!     .build()?;
//!
//! console.error(&error);
//! ```

// Phase 1: Core error types
pub mod diagnostic;

// Error code catalog
pub mod catalog;

// Phase 4: Builder API
pub mod builder;

// Macros for convenient error creation
pub mod macros;

// Re-export main types for convenience
pub use builder::DiagnosticMessageBuilder;
pub use catalog::{ERROR_CATALOG, ErrorCodeInfo, get_docs_url, get_error_info, get_subsystem};
pub use diagnostic::{DetailItem, DetailKind, DiagnosticKind, DiagnosticMessage, MessageContent};
