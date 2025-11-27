//! Citation processing engine using CSL (Citation Style Language) styles.
//!
//! This crate provides citation processing that takes:
//! - A parsed CSL [`Style`](quarto_csl::Style) from quarto-csl
//! - Bibliographic [`Reference`]s in CSL-JSON format
//! - [`Citation`] requests specifying which references to cite
//!
//! And produces formatted output as Pandoc `Inlines`.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────┐
//! │                         quarto-citeproc                             │
//! │                 (citation processing algorithm)                      │
//! │     References + Citations + Style → Formatted Pandoc Inlines       │
//! └───────────────────────────┬─────────────────────────────────────────┘
//!                             │
//!                             ▼
//! ┌─────────────────────────────────────────────────────────────────────┐
//! │                          quarto-csl                                  │
//! │                    (CSL semantics layer)                             │
//! │      XmlWithSourceInfo → Style, Element, Macro, Locale, etc.        │
//! └─────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Example
//!
//! ```rust,ignore
//! use quarto_citeproc::{Processor, Reference, Citation, CitationItem};
//! use quarto_csl::parse_csl;
//!
//! // Parse the CSL style
//! let style = parse_csl(csl_content)?;
//!
//! // Create processor with references
//! let mut processor = Processor::new(style);
//! processor.add_reference(reference);
//!
//! // Process a citation
//! let citation = Citation {
//!     items: vec![CitationItem { id: "smith2020".into(), ..Default::default() }],
//!     ..Default::default()
//! };
//! let formatted = processor.process_citation(&citation)?;
//! ```

pub mod error;
pub mod locale;
pub mod locale_parser;
pub mod output;
pub mod reference;
pub mod types;

mod eval;

// Re-export main types
pub use error::{Error, Result};
pub use reference::{Reference, Name, DateParts};
pub use types::{Citation, CitationItem, Processor};
