//! Configuration merging with source tracking for Quarto.
//!
//! This crate provides the infrastructure for merging configuration from multiple
//! sources (project-level, directory-level, document-level) while preserving source
//! location information for error reporting.
//!
//! # Key Features
//!
//! - **Source location preservation**: Every value carries its `SourceInfo`
//! - **Explicit merge semantics**: `!prefer` and `!concat` YAML tags control behavior
//! - **Lazy evaluation**: `MergedConfig<'a>` avoids unnecessary copying
//! - **Associativity**: `(a <> b) <> c == a <> (b <> c)` for any configs
//!
//! # Architecture
//!
//! The crate is organized around these core concepts:
//!
//! - [`ConfigValue`]: A configuration value with explicit merge semantics
//! - [`MergeOp`]: Controls whether values prefer (override) or concat (append)
//! - [`Interpretation`]: Hints for how strings should be interpreted (`!md`, `!str`, etc.)
//!
//! # Example
//!
//! ```rust,no_run
//! use quarto_config::{ConfigValue, MergeOp, MergedConfig};
//!
//! // Parse config layers (from YAML files)
//! let project_config: ConfigValue = /* ... */ todo!();
//! let doc_config: ConfigValue = /* ... */ todo!();
//!
//! // Create merged config (zero-copy)
//! let merged = MergedConfig::new(vec![&project_config, &doc_config]);
//!
//! // Access merged values
//! if let Some(theme) = merged.get_scalar(&["format", "html", "theme"]) {
//!     println!("Theme: {:?}", theme.value);
//! }
//! ```

mod types;
mod tag;
mod convert;
mod merged;
mod materialize;

pub use types::{
    ConfigError,
    ConfigValue,
    ConfigValueKind,
    Interpretation,
    MergeOp,
};

pub use tag::{
    ParsedTag,
    parse_tag,
};

pub use convert::config_value_from_yaml;

pub use merged::{
    MergedArray,
    MergedArrayItem,
    MergedConfig,
    MergedCursor,
    MergedMap,
    MergedScalar,
    MergedValue,
};

pub use materialize::{
    MaterializeOptions,
    merge_with_diagnostics,
};

// Re-export for convenience
pub use quarto_source_map::SourceInfo;
