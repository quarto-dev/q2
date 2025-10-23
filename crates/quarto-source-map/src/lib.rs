//! Source mapping for Quarto
//!
//! This crate provides unified source location tracking with support for
//! transformations (extraction, concatenation, normalization). It enables
//! precise error reporting and mapping positions back through transformation
//! chains to original source files.
//!
//! # Overview
//!
//! The core types are:
//! - [`SourceInfo`]: Enum tracking a location and its transformation history
//! - [`SourceContext`]: Manages files and provides content for mapping
//! - [`MappedLocation`]: Result of mapping through transformation chains
//!
//! # Example
//!
//! ```rust
//! use quarto_source_map::*;
//!
//! // Create a context and register a file
//! let mut ctx = SourceContext::new();
//! let file_id = ctx.add_file("main.qmd".into(), Some("# Hello\nWorld".into()));
//!
//! // Create a source location (stores only offsets)
//! let info = SourceInfo::original(file_id, 0, 7);
//!
//! // Map to get row/column information
//! let mapped = info.map_offset(0, &ctx).unwrap();
//! assert_eq!(mapped.location.row, 0);
//! assert_eq!(mapped.location.column, 0);
//! ```

pub mod context;
pub mod file_info;
pub mod mapping;
pub mod source_info;
pub mod types;
pub mod utils;

// Re-export main types
pub use context::{FileMetadata, SourceContext, SourceFile};
pub use file_info::FileInformation;
pub use mapping::MappedLocation;
pub use source_info::{SourceInfo, SourcePiece};
pub use types::{FileId, Location, Range};
pub use utils::{line_col_to_offset, offset_to_location, range_from_offsets};
