//! Core type definitions for configuration merging.
//!
//! The main types (ConfigValue, ConfigValueKind, MergeOp, Interpretation) are defined
//! in quarto-pandoc-types and re-exported here for convenience.

use quarto_source_map::SourceInfo;
use thiserror::Error;

// Re-export core types from quarto-pandoc-types
pub use quarto_pandoc_types::{
    ConfigMapEntry, ConfigValue, ConfigValueKind, Interpretation, InterpretationContext, MergeOp,
};

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
    use yaml_rust2::Yaml;

    #[test]
    fn test_merge_op_default() {
        assert_eq!(MergeOp::default(), MergeOp::Concat);
    }

    #[test]
    fn test_config_value_scalar() {
        let value = ConfigValue::new_scalar(Yaml::String("test".into()), SourceInfo::default());

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
        let entries = vec![ConfigMapEntry {
            key: "key".to_string(),
            key_source: SourceInfo::default(),
            value: ConfigValue::new_scalar(Yaml::String("value".into()), SourceInfo::default()),
        }];
        let value = ConfigValue::new_map(entries, SourceInfo::default());

        assert!(value.is_map());
        assert_eq!(value.as_map_entries().unwrap().len(), 1);
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
