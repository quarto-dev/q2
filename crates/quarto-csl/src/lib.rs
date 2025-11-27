//! CSL (Citation Style Language) parsing with source tracking for Quarto.
//!
//! This crate provides CSL parsing that produces semantic Rust types while
//! preserving source location information for error reporting. It builds
//! on [`quarto_xml`] for XML parsing with source tracking.
//!
//! # Overview
//!
//! The main types are:
//! - [`Style`]: A complete parsed CSL style
//! - [`Element`]: A formatting element (text, names, date, etc.)
//! - [`Macro`]: A reusable macro definition
//! - [`Locale`]: Locale-specific terms and date formats
//!
//! # Example
//!
//! ```rust
//! use quarto_csl::parse_csl;
//!
//! let csl = r#"<?xml version="1.0" encoding="utf-8"?>
//! <style xmlns="http://purl.org/net/xbiblio/csl" class="in-text" version="1.0">
//!   <info><title>Test Style</title></info>
//!   <citation><layout><text variable="title"/></layout></citation>
//! </style>"#;
//!
//! let style = parse_csl(csl).unwrap();
//! assert_eq!(style.version, "1.0");
//! ```
//!
//! # Error Reporting
//!
//! All errors include source location information for precise error messages:
//!
//! ```rust
//! use quarto_csl::parse_csl;
//!
//! let result = parse_csl("<style/>");  // Missing required attributes
//! assert!(result.is_err());
//!
//! let err = result.unwrap_err();
//! let diagnostic = err.to_diagnostic();
//! assert!(diagnostic.code.is_some());  // Has Q-9-xxx error code
//! ```

pub mod error;
pub mod parser;
pub mod types;

// Re-export main types
pub use error::{Error, Result};
pub use parser::parse_csl;
pub use types::*;
