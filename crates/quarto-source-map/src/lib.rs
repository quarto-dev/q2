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
//! - [`SourceInfo`]: Tracks a location with its transformation history
//! - [`SourceMapping`]: Enum describing how content was transformed
//! - [`SourceContext`]: Manages files and provides content for mapping
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
//! // Create a source location
//! let range = Range {
//!     start: Location { offset: 0, row: 0, column: 0 },
//!     end: Location { offset: 7, row: 0, column: 7 },
//! };
//! let info = SourceInfo::original(file_id, range.clone());
//!
//! // Verify the source info was created correctly
//! assert_eq!(info.range, range);
//! ```

pub mod types;
pub mod source_info;
pub mod context;
pub mod mapping;
pub mod utils;

// Re-export main types
pub use types::{FileId, Location, Range};
pub use source_info::{SourceInfo, SourceMapping, SourcePiece, RangeMapping};
pub use context::{SourceContext, SourceFile, FileMetadata};
pub use mapping::MappedLocation;
pub use utils::{offset_to_location, line_col_to_offset, range_from_offsets};
