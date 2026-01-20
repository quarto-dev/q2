//! Transport-agnostic language analysis for Quarto documents.
//!
//! This crate provides the core analysis logic for QMD files without any
//! LSP protocol dependencies. It is designed to compile to both native
//! and WASM targets.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────┐
//! │                        quarto-lsp-core                          │
//! │   (Transport-agnostic analysis: symbols, diagnostics, hover)    │
//! └─────────────────────────────────────────────────────────────────┘
//!             │                                    │
//!             ▼                                    ▼
//! ┌───────────────────────┐          ┌─────────────────────────────┐
//! │     quarto-lsp        │          │   wasm-quarto-hub-client    │
//! │  (Native LSP server)  │          │   (Browser WASM module)     │
//! └───────────────────────┘          └─────────────────────────────┘
//! ```
//!
//! # Usage
//!
//! ```rust,ignore
//! use quarto_lsp_core::{Document, get_diagnostics, get_symbols};
//!
//! // Create a document from content
//! let doc = Document::new("example.qmd", content);
//!
//! // Get diagnostics
//! let diagnostics = get_diagnostics(&doc);
//!
//! // Get document symbols (outline)
//! let symbols = get_symbols(&doc);
//! ```

pub mod diagnostics;
pub mod document;
pub mod symbols;
pub mod types;

// Re-export main types for convenience
pub use diagnostics::get_diagnostics;
pub use document::Document;
pub use symbols::get_symbols;
pub use types::{Diagnostic, DiagnosticSeverity, Position, Range, Symbol, SymbolKind};
