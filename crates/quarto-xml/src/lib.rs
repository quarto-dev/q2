//! Source-tracked XML parsing for Quarto.
//!
//! This crate provides XML parsing with source location tracking, analogous to
//! [`quarto-yaml`](../quarto_yaml/index.html). It wraps [`quick-xml`] to provide
//! a tree of [`XmlElement`]s where each element, attribute, and text node tracks
//! its position in the original source.
//!
//! # Overview
//!
//! The main types are:
//! - [`XmlWithSourceInfo`]: The parsed XML document with source tracking
//! - [`XmlElement`]: An XML element with name, attributes, children, and source info
//! - [`XmlAttribute`]: An attribute with name, value, and separate source info for each
//! - [`XmlChildren`]: Element content (elements, text, mixed, or empty)
//! - [`XmlParseContext`]: Context for collecting diagnostics during parsing
//!
//! # Example
//!
//! ```rust
//! use quarto_xml::parse;
//!
//! let xml = parse(r#"<style version="1.0">
//!   <macro name="author">
//!     <text variable="author"/>
//!   </macro>
//! </style>"#).unwrap();
//!
//! assert_eq!(xml.root.name, "style");
//! assert_eq!(xml.root.get_attribute("version"), Some("1.0"));
//!
//! let macros = xml.root.get_children("macro");
//! assert_eq!(macros.len(), 1);
//! assert_eq!(macros[0].get_attribute("name"), Some("author"));
//! ```
//!
//! # Source Location Tracking
//!
//! Every element tracks its source location using [`quarto_source_map::SourceInfo`]:
//!
//! ```rust
//! use quarto_xml::parse;
//!
//! let content = "<root><child/></root>";
//! let xml = parse(content).unwrap();
//!
//! // The root element spans the entire content
//! assert_eq!(xml.root.source_info.start_offset(), 0);
//! assert_eq!(xml.root.source_info.end_offset(), content.len());
//! ```
//!
//! # Diagnostic Collection
//!
//! For richer error reporting with Q-9-* error codes, use [`parse_with_context`]:
//!
//! ```rust
//! use quarto_xml::{parse_with_context, XmlParseContext};
//!
//! let mut ctx = XmlParseContext::new();
//! match parse_with_context("<root/>", &mut ctx) {
//!     Ok(xml) => {
//!         // Check for warnings even on success
//!         for diag in ctx.diagnostics() {
//!             eprintln!("{}", diag.title);
//!         }
//!     }
//!     Err(errors) => {
//!         for err in errors {
//!             eprintln!("Error: {}", err.title);
//!         }
//!     }
//! }
//! ```
//!
//! For XML embedded in other documents, use [`parse_with_parent`] to create
//! substring mappings that track back to the parent document.

pub mod context;
pub mod error;
pub mod parser;
pub mod types;

// Re-export main types
pub use context::XmlParseContext;
pub use error::{Error, ParseResult, Result};
pub use parser::{parse, parse_with_context, parse_with_file_id, parse_with_parent};
pub use quarto_source_map::SourceInfo;
pub use types::{XmlAttribute, XmlChild, XmlChildren, XmlElement, XmlWithSourceInfo};
