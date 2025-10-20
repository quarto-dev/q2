//! # quarto-yaml
//!
//! YAML parsing with source location tracking.
//!
//! This crate provides `YamlWithSourceInfo`, which wraps `yaml-rust2::Yaml` with
//! source location information for every node in the YAML tree. This enables
//! precise error reporting and source tracking through transformations.
//!
//! ## Design
//!
//! Uses the **owned data approach**: wraps owned `Yaml` values with a parallel
//! children structure for source tracking. Trade-off: ~3x memory overhead for
//! simplicity and compatibility with config merging across different lifetimes.
//!
//! Follows rust-analyzer's precedent of using owned data with reference counting
//! for tree structures.
//!
//! ## Example
//!
//! ```rust,no_run
//! use quarto_yaml::parse;
//!
//! let content = r#"
//! title: My Document
//! author: John Doe
//! "#;
//!
//! let yaml = parse(content).unwrap();
//! // Access with source location tracking
//! if let Some(title) = yaml.get_hash_value("title") {
//!     println!("Title at offset {}", title.source_info.range.start.offset);
//! }
//! ```

mod error;
mod parser;
mod yaml_with_source_info;

pub use error::{Error, Result};
pub use parser::{parse, parse_file, parse_with_parent};
pub use quarto_source_map::SourceInfo; // Re-export from quarto-source-map
pub use yaml_with_source_info::{YamlHashEntry, YamlWithSourceInfo};
