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
//! use quarto_lsp_core::{Document, analyze_document};
//!
//! // Create a document from content
//! let doc = Document::new("example.qmd", content);
//!
//! // Analyze the document (most efficient - single parse)
//! let analysis = analyze_document(&doc);
//! println!("Symbols: {}", analysis.symbols.len());
//! println!("Folding ranges: {}", analysis.folding_ranges.len());
//! println!("Diagnostics: {}", analysis.diagnostics.len());
//!
//! // Or use convenience functions for individual data:
//! let symbols = get_symbols(&doc);
//! let diagnostics = get_diagnostics(&doc);
//! let folding_ranges = get_folding_ranges(&doc);
//! ```

pub mod analysis;
pub mod diagnostics;
pub mod document;
pub mod symbols;
pub mod types;

// Re-export main types and functions for convenience
pub use analysis::analyze_document;
pub use diagnostics::get_diagnostics;
pub use document::Document;
pub use symbols::{get_folding_ranges, get_symbols};
pub use types::{
    Diagnostic, DiagnosticSeverity, DocumentAnalysis, DocumentAnalysisJson, FoldingRange,
    FoldingRangeKind, Position, Range, Symbol, SymbolKind,
};
