//! Core type definitions for configuration merging.

use indexmap::IndexMap;
use quarto_pandoc_types::{Blocks, Inlines};
use quarto_source_map::SourceInfo;
use thiserror::Error;
use yaml_rust2::Yaml;

/// Merge operation for a value.
///
/// Controls how values from different configuration layers are combined.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MergeOp {
    /// This value overrides/resets previous values (from `!prefer` tag).
    ///
    /// For arrays: clears all previous items
    /// For maps: replaces entire map (no field-wise merge)
    /// For scalars: replaces value (same as default)
    Prefer,

    /// This value concatenates with previous values (from `!concat` tag or default for arrays/maps).
    ///
    /// For arrays: appends items to previous arrays
    /// For maps: field-wise merge with previous maps
    /// For scalars: replaces value (same as Prefer)
    #[default]
    Concat,
}

/// Interpretation hint for string values.
///
/// Derived from YAML tags like `!md`, `!str`, `!path`, etc.
/// Actual interpretation happens later (in pampa) when converting to metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Interpretation {
    /// `!md` - Parse string as Markdown
    Markdown,

    /// `!str` - Keep as literal string (no markdown parsing)
    PlainString,

    /// `!path` - Resolve relative to source file
    Path,

    /// `!glob` - Treat as glob pattern
    Glob,

    /// `!expr` - Runtime expression (R/Python/Julia)
    Expr,
}

/// A configuration value with explicit merge semantics.
///
/// This is the core type for configuration merging. It wraps a value with:
/// - Source location for error reporting
/// - Merge operation (prefer vs concat)
/// - Interpretation hint (for string values)
#[derive(Debug, Clone)]
pub struct ConfigValue {
    /// The underlying value
    pub value: ConfigValueKind,

    /// Source location for this value
    pub source_info: SourceInfo,

    /// Merge operation (derived from tag or inferred)
    pub merge_op: MergeOp,

    /// Interpretation hint for string values (derived from tag)
    pub interpretation: Option<Interpretation>,
}

/// The kind of configuration value.
///
/// This mirrors YAML/JSON value types plus Pandoc AST types for
/// already-interpreted values.
#[derive(Debug, Clone)]
pub enum ConfigValueKind {
    /// Atomic values (String, Int, Float, Bool, Null).
    ///
    /// Always use "last wins" semantics regardless of MergeOp.
    Scalar(Yaml),

    /// Arrays: merge_op controls concatenate vs replace.
    Array(Vec<ConfigValue>),

    /// Objects: merge_op controls field-wise merge vs replace.
    Map(IndexMap<String, ConfigValue>),

    /// Pandoc inline content (for already-interpreted values).
    ///
    /// Default: `!prefer` (last wins, no concatenation).
    /// Use `!concat` explicitly if concatenation is desired.
    PandocInlines(Inlines),

    /// Pandoc block content (for already-interpreted values).
    ///
    /// Default: `!prefer` (last wins, no concatenation).
    /// Use `!concat` explicitly if concatenation is desired.
    PandocBlocks(Blocks),
}

impl ConfigValue {
    /// Create a new scalar ConfigValue with default merge semantics.
    pub fn new_scalar(yaml: Yaml, source_info: SourceInfo) -> Self {
        Self {
            value: ConfigValueKind::Scalar(yaml),
            source_info,
            merge_op: MergeOp::Concat,
            interpretation: None,
        }
    }

    /// Create a new array ConfigValue with default merge semantics.
    pub fn new_array(items: Vec<ConfigValue>, source_info: SourceInfo) -> Self {
        Self {
            value: ConfigValueKind::Array(items),
            source_info,
            merge_op: MergeOp::Concat,
            interpretation: None,
        }
    }

    /// Create a new map ConfigValue with default merge semantics.
    pub fn new_map(entries: IndexMap<String, ConfigValue>, source_info: SourceInfo) -> Self {
        Self {
            value: ConfigValueKind::Map(entries),
            source_info,
            merge_op: MergeOp::Concat,
            interpretation: None,
        }
    }

    /// Create a null ConfigValue.
    pub fn null(source_info: SourceInfo) -> Self {
        Self {
            value: ConfigValueKind::Scalar(Yaml::Null),
            source_info,
            merge_op: MergeOp::Concat,
            interpretation: None,
        }
    }

    /// Create a ConfigValue with Pandoc inlines (defaults to prefer semantics).
    pub fn new_inlines(inlines: Inlines, source_info: SourceInfo) -> Self {
        Self {
            value: ConfigValueKind::PandocInlines(inlines),
            source_info,
            merge_op: MergeOp::Prefer, // Default for markdown content
            interpretation: None,
        }
    }

    /// Create a ConfigValue with Pandoc blocks (defaults to prefer semantics).
    pub fn new_blocks(blocks: Blocks, source_info: SourceInfo) -> Self {
        Self {
            value: ConfigValueKind::PandocBlocks(blocks),
            source_info,
            merge_op: MergeOp::Prefer, // Default for markdown content
            interpretation: None,
        }
    }

    /// Set the merge operation.
    pub fn with_merge_op(mut self, merge_op: MergeOp) -> Self {
        self.merge_op = merge_op;
        self
    }

    /// Set the interpretation hint.
    pub fn with_interpretation(mut self, interpretation: Interpretation) -> Self {
        self.interpretation = Some(interpretation);
        self
    }

    /// Check if this is a scalar value.
    pub fn is_scalar(&self) -> bool {
        matches!(
            self.value,
            ConfigValueKind::Scalar(_)
                | ConfigValueKind::PandocInlines(_)
                | ConfigValueKind::PandocBlocks(_)
        )
    }

    /// Check if this is an array value.
    pub fn is_array(&self) -> bool {
        matches!(self.value, ConfigValueKind::Array(_))
    }

    /// Check if this is a map value.
    pub fn is_map(&self) -> bool {
        matches!(self.value, ConfigValueKind::Map(_))
    }

    /// Get as a Yaml scalar if this is a scalar.
    pub fn as_yaml(&self) -> Option<&Yaml> {
        match &self.value {
            ConfigValueKind::Scalar(yaml) => Some(yaml),
            _ => None,
        }
    }

    /// Get as array items if this is an array.
    pub fn as_array(&self) -> Option<&[ConfigValue]> {
        match &self.value {
            ConfigValueKind::Array(items) => Some(items),
            _ => None,
        }
    }

    /// Get as map entries if this is a map.
    pub fn as_map(&self) -> Option<&IndexMap<String, ConfigValue>> {
        match &self.value {
            ConfigValueKind::Map(entries) => Some(entries),
            _ => None,
        }
    }
}

/// Errors that can occur during configuration operations.
#[derive(Debug, Clone, Error)]
pub enum ConfigError {
    /// Configuration nesting exceeds maximum depth.
    #[error("Config nesting too deep (max depth: {max_depth}) at path: {}", path.join("."))]
    NestingTooDeep {
        /// Maximum allowed depth
        max_depth: usize,
        /// Path where the limit was exceeded
        path: Vec<String>,
    },

    /// Tag parsing error.
    #[error("Invalid tag: {message}")]
    InvalidTag {
        /// Error message
        message: String,
        /// Source location of the tag
        source_info: SourceInfo,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_merge_op_default() {
        assert_eq!(MergeOp::default(), MergeOp::Concat);
    }

    #[test]
    fn test_config_value_scalar() {
        let value = ConfigValue::new_scalar(
            Yaml::String("test".into()),
            SourceInfo::default(),
        );

        assert!(value.is_scalar());
        assert!(!value.is_array());
        assert!(!value.is_map());
        assert_eq!(value.merge_op, MergeOp::Concat);
    }

    #[test]
    fn test_config_value_array() {
        let items = vec![
            ConfigValue::new_scalar(Yaml::String("a".into()), SourceInfo::default()),
            ConfigValue::new_scalar(Yaml::String("b".into()), SourceInfo::default()),
        ];
        let value = ConfigValue::new_array(items, SourceInfo::default());

        assert!(value.is_array());
        assert_eq!(value.as_array().unwrap().len(), 2);
        assert_eq!(value.merge_op, MergeOp::Concat);
    }

    #[test]
    fn test_config_value_map() {
        let mut entries = IndexMap::new();
        entries.insert(
            "key".to_string(),
            ConfigValue::new_scalar(Yaml::String("value".into()), SourceInfo::default()),
        );
        let value = ConfigValue::new_map(entries, SourceInfo::default());

        assert!(value.is_map());
        assert_eq!(value.as_map().unwrap().len(), 1);
        assert_eq!(value.merge_op, MergeOp::Concat);
    }

    #[test]
    fn test_config_value_with_merge_op() {
        let value = ConfigValue::new_scalar(Yaml::String("test".into()), SourceInfo::default())
            .with_merge_op(MergeOp::Prefer);

        assert_eq!(value.merge_op, MergeOp::Prefer);
    }

    #[test]
    fn test_pandoc_inlines_default_prefer() {
        let value = ConfigValue::new_inlines(vec![], SourceInfo::default());
        assert_eq!(value.merge_op, MergeOp::Prefer);
    }

    #[test]
    fn test_pandoc_blocks_default_prefer() {
        let value = ConfigValue::new_blocks(vec![], SourceInfo::default());
        assert_eq!(value.merge_op, MergeOp::Prefer);
    }
}
