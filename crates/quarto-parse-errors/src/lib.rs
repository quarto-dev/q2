/*
 * lib.rs
 * Copyright (c) 2025 Posit, PBC
 */

//! # quarto-parse-errors
//!
//! Generic error reporting infrastructure for tree-sitter based parsers.
//!
//! This crate provides a complete system for generating high-quality error messages
//! from tree-sitter parse failures using the "generating syntax errors from examples"
//! approach (Jeffery, TOPLAS 2003).
//!
//! ## Overview
//!
//! The error reporting system consists of several components:
//!
//! 1. **Error Corpus**: JSON files mapping parser states `(LR state, lookahead symbol)` to
//!    human-readable error messages with context.
//! 2. **TreeSitterLogObserver**: Captures parser state during failed parses by observing
//!    tree-sitter's internal logging.
//! 3. **Error Table**: Compile-time embedded error message database generated from the corpus.
//! 4. **Error Generation**: Converts captured parser states to user-friendly diagnostic messages.
//!
//! ## Usage
//!
//! ### Basic Integration
//!
//! ```ignore
//! use quarto_parse_errors::{TreeSitterLogObserver, TreeSitterLogObserverTrait, produce_diagnostic_messages};
//!
//! // Set up your parser with logging
//! let mut parser = tree_sitter::Parser::new();
//! parser.set_language(your_language)?;
//!
//! let mut observer = TreeSitterLogObserver::default();
//! parser.set_logger(Some(Box::new(|log_type, message| {
//!     observer.log(log_type, message);
//! })));
//!
//! // Parse with error observation
//! let tree = parser.parse(source_code, None)?;
//!
//! // Generate diagnostics if errors occurred
//! if observer.had_errors() {
//!     let diagnostics = produce_diagnostic_messages(
//!         source_code.as_bytes(),
//!         &observer,
//!         &error_table,  // From include_error_table! macro
//!         "filename.ext",
//!         &source_context,
//!     );
//!
//!     for diagnostic in diagnostics {
//!         // Report errors to user
//!         println!("{}", diagnostic);
//!     }
//! }
//! ```
//!
//! ### Creating an Error Corpus
//!
//! Error corpus files use JSON format with test cases:
//!
//! ```json
//! {
//!   "code": "E-001",
//!   "title": "Unclosed Bracket",
//!   "message": "Expected closing bracket ']' before end of line",
//!   "notes": [{
//!     "message": "Opening bracket is here",
//!     "label": "open-bracket",
//!     "noteType": "simple"
//!   }],
//!   "cases": [{
//!     "name": "simple",
//!     "content": "foo [bar\n",
//!     "captures": [{
//!       "label": "open-bracket",
//!       "row": 0,
//!       "column": 4,
//!       "size": 1
//!     }]
//!   }]
//! }
//! ```
//!
//! ### Generating the Error Table
//!
//! Use the provided build script to generate the error table:
//!
//! ```bash
//! ./scripts/build_error_table.ts \
//!   --cmd 'target/debug/my-parser --_internal-report-error-state -i' \
//!   --corpus resources/error-corpus \
//!   --output resources/error-corpus/_autogen-table.json \
//!   --extension .ext
//! ```
//!
//! ### Embedding the Error Table
//!
//! Use the `include_error_table!` macro in your parser:
//!
//! ```ignore
//! use error_message_macros::include_error_table;
//!
//! pub fn get_error_table() -> &'static [ErrorTableEntry] {
//!     include_error_table!(
//!         "./resources/error-corpus/_autogen-table.json",
//!         "crate::error_table"
//!     )
//! }
//! ```

pub mod error_generation;
pub mod error_table;
pub mod tree_sitter_log;

// Re-export commonly used types
pub use error_table::{
    ErrorCapture, ErrorInfo, ErrorNote, ErrorTableEntry, lookup_error_entry, lookup_error_message,
};

pub use tree_sitter_log::{
    ConsumedToken, ProcessMessage, TreeSitterLogObserver, TreeSitterLogObserverFast,
    TreeSitterLogObserverTrait, TreeSitterLogState, TreeSitterParseLog, TreeSitterProcessLog,
};

pub use error_generation::{
    collect_error_node_ranges, diagnostic_score, get_outer_error_nodes,
    produce_diagnostic_messages, prune_diagnostics_by_error_nodes,
};
