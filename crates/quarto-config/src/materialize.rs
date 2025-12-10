//! Materialization of merged configuration into owned values.
//!
//! This module provides the ability to convert a lazily-evaluated `MergedConfig`
//! into an owned `ConfigValue` tree. This is useful for:
//!
//! - Serialization (sending to another process)
//! - Caching (storing resolved config)
//! - Cross-thread use (avoiding lifetime constraints)
//!
//! # Depth Limiting
//!
//! Materialization enforces a maximum depth to prevent stack overflow from
//! deeply nested or circular configurations. The default limit is 256 levels.
//!
//! # Example
//!
//! ```rust,ignore
//! let merged = MergedConfig::new(vec![&layer1, &layer2]);
//!
//! // Materialize with default options
//! let owned = merged.materialize()?;
//!
//! // Materialize with custom depth limit
//! let options = MaterializeOptions { max_depth: 64 };
//! let owned = merged.materialize_with_options(&options)?;
//! ```

use crate::merged::{MergedConfig, MergedCursor, MergedValue};
use crate::types::{ConfigError, ConfigValue, ConfigValueKind, MergeOp};
use indexmap::IndexMap;
use quarto_error_reporting::DiagnosticMessage;
use quarto_source_map::SourceInfo;

/// Options for materialization.
#[derive(Debug, Clone)]
pub struct MaterializeOptions {
    /// Maximum nesting depth (default: 256).
    ///
    /// Materialization will fail with `ConfigError::NestingTooDeep` if
    /// the configuration exceeds this depth.
    pub max_depth: usize,
}

impl Default for MaterializeOptions {
    fn default() -> Self {
        Self { max_depth: 256 }
    }
}

impl<'a> MergedConfig<'a> {
    /// Materialize with default options.
    ///
    /// Converts the lazily-evaluated merged config into an owned `ConfigValue`.
    /// Each value's `SourceInfo` is preserved, allowing validation errors to
    /// still point to the correct file and line.
    pub fn materialize(&self) -> Result<ConfigValue, ConfigError> {
        self.materialize_with_options(&MaterializeOptions::default())
    }

    /// Materialize with custom options.
    pub fn materialize_with_options(
        &self,
        options: &MaterializeOptions,
    ) -> Result<ConfigValue, ConfigError> {
        let cursor = self.cursor();
        materialize_cursor(&cursor, 0, options, &[])
    }
}

/// Materialize a cursor's value into an owned ConfigValue.
fn materialize_cursor(
    cursor: &MergedCursor<'_>,
    depth: usize,
    options: &MaterializeOptions,
    path: &[String],
) -> Result<ConfigValue, ConfigError> {
    // Check depth limit
    if depth > options.max_depth {
        return Err(ConfigError::NestingTooDeep {
            max_depth: options.max_depth,
            path: path.to_vec(),
        });
    }

    // Resolve the value type
    match cursor.as_value() {
        Some(MergedValue::Scalar(scalar)) => {
            // Clone the scalar value
            Ok(ConfigValue {
                value: scalar.value.value.clone(),
                source_info: scalar.value.source_info.clone(),
                merge_op: scalar.value.merge_op,
                interpretation: scalar.value.interpretation,
            })
        }
        Some(MergedValue::Array(array)) => {
            // Materialize array items
            // Note: We can't recursively materialize array items through cursors
            // because array items don't have paths. We just clone them directly.
            let items: Vec<ConfigValue> = array
                .items
                .iter()
                .map(|item| ConfigValue {
                    value: item.value.value.clone(),
                    source_info: item.value.source_info.clone(),
                    merge_op: item.value.merge_op,
                    interpretation: item.value.interpretation,
                })
                .collect();

            // Find source_info from the most recent layer that contributed
            let source_info = array
                .items
                .last()
                .map(|item| item.value.source_info.clone())
                .unwrap_or_default();

            Ok(ConfigValue {
                value: ConfigValueKind::Array(items),
                source_info,
                merge_op: MergeOp::Concat, // Materialized arrays don't have prefer semantics
                interpretation: None,
            })
        }
        Some(MergedValue::Map(map)) => {
            // Materialize map entries recursively
            let mut entries = IndexMap::new();
            let mut path = path.to_vec();

            for (key, child_cursor) in map.iter() {
                path.push(key.to_string());
                let child_value = materialize_cursor(&child_cursor, depth + 1, options, &path)?;
                path.pop();
                entries.insert(key.to_string(), child_value);
            }

            // Source info: use the cursor's path to find a representative source
            // In practice, maps from different layers may have different sources
            let source_info = cursor
                .as_map()
                .and_then(|m| {
                    m.iter()
                        .next()
                        .and_then(|(_, c)| c.as_value())
                        .map(|v| match v {
                            MergedValue::Scalar(s) => s.value.source_info.clone(),
                            MergedValue::Array(a) => a
                                .items
                                .first()
                                .map(|i| i.value.source_info.clone())
                                .unwrap_or_default(),
                            MergedValue::Map(_) => SourceInfo::default(),
                        })
                })
                .unwrap_or_default();

            Ok(ConfigValue {
                value: ConfigValueKind::Map(entries),
                source_info,
                merge_op: MergeOp::Concat,
                interpretation: None,
            })
        }
        None => {
            // Path doesn't exist - return null
            Ok(ConfigValue::null(SourceInfo::default()))
        }
    }
}

/// Merge config layers, collecting diagnostics.
///
/// This function validates each layer and collects any errors or warnings.
/// If any layer has errors, the result's `config` will be `None`, but all
/// diagnostics will still be reported.
///
/// # Arguments
///
/// * `layers` - Config layers with their source info for error reporting
/// * `diagnostics` - Collector for errors and warnings
///
/// # Returns
///
/// A `MergeResult` containing the merged config (if successful) and all diagnostics.
pub fn merge_with_diagnostics<'a>(
    layers: Vec<&'a ConfigValue>,
    diagnostics: &mut Vec<DiagnosticMessage>,
) -> Option<MergedConfig<'a>> {
    // For now, we just validate that layers are well-formed
    // In the future, this could validate tag syntax more thoroughly
    let mut had_errors = false;

    for layer in &layers {
        // Validate the layer recursively
        if let Err(e) = validate_layer(layer, diagnostics) {
            diagnostics.push(
                quarto_error_reporting::DiagnosticMessageBuilder::error(
                    "Config layer validation failed",
                )
                .with_code("Q-1-23")
                .problem(format!("Failed to validate config layer: {}", e))
                .with_location(layer.source_info.clone())
                .build(),
            );
            had_errors = true;
        }
    }

    if had_errors {
        None
    } else {
        Some(MergedConfig::new(layers))
    }
}

/// Validate a config layer.
///
/// Currently this is a basic validation that the structure is well-formed.
/// Returns Ok(()) if valid, Err with a description if invalid.
fn validate_layer(
    layer: &ConfigValue,
    _diagnostics: &mut Vec<DiagnosticMessage>,
) -> Result<(), String> {
    // Recursively validate children
    match &layer.value {
        ConfigValueKind::Array(items) => {
            for item in items {
                validate_layer(item, _diagnostics)?;
            }
        }
        ConfigValueKind::Map(entries) => {
            for value in entries.values() {
                validate_layer(value, _diagnostics)?;
            }
        }
        _ => {
            // Scalars and Pandoc types are always valid
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Interpretation;
    use yaml_rust2::Yaml;

    // Helpers
    fn scalar(s: &str) -> ConfigValue {
        ConfigValue::new_scalar(Yaml::String(s.into()), SourceInfo::default())
    }

    fn array(items: Vec<ConfigValue>) -> ConfigValue {
        ConfigValue::new_array(items, SourceInfo::default())
    }

    fn map(entries: Vec<(&str, ConfigValue)>) -> ConfigValue {
        let map: IndexMap<String, ConfigValue> = entries
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect();
        ConfigValue::new_map(map, SourceInfo::default())
    }

    #[test]
    fn test_materialize_scalar() {
        let config = map(vec![("title", scalar("Hello"))]);
        let merged = MergedConfig::new(vec![&config]);

        let result = merged.materialize().unwrap();
        assert!(result.is_map());

        let map = result.as_map().unwrap();
        let title = map.get("title").unwrap();
        assert_eq!(title.as_yaml().unwrap().as_str(), Some("Hello"));
    }

    #[test]
    fn test_materialize_array() {
        let config = map(vec![("items", array(vec![scalar("a"), scalar("b")]))]);
        let merged = MergedConfig::new(vec![&config]);

        let result = merged.materialize().unwrap();
        let map = result.as_map().unwrap();
        let items = map.get("items").unwrap();
        assert!(items.is_array());
        assert_eq!(items.as_array().unwrap().len(), 2);
    }

    #[test]
    fn test_materialize_nested_map() {
        let config = map(vec![(
            "format",
            map(vec![("html", map(vec![("theme", scalar("cosmo"))]))]),
        )]);
        let merged = MergedConfig::new(vec![&config]);

        let result = merged.materialize().unwrap();
        let format = result.as_map().unwrap().get("format").unwrap();
        let html = format.as_map().unwrap().get("html").unwrap();
        let theme = html.as_map().unwrap().get("theme").unwrap();
        assert_eq!(theme.as_yaml().unwrap().as_str(), Some("cosmo"));
    }

    #[test]
    fn test_materialize_merged_layers() {
        let layer1 = map(vec![("a", scalar("1")), ("b", scalar("2"))]);
        let layer2 = map(vec![("b", scalar("3")), ("c", scalar("4"))]);
        let merged = MergedConfig::new(vec![&layer1, &layer2]);

        let result = merged.materialize().unwrap();
        let map = result.as_map().unwrap();

        // a from layer1
        assert_eq!(map.get("a").unwrap().as_yaml().unwrap().as_str(), Some("1"));
        // b overridden by layer2
        assert_eq!(map.get("b").unwrap().as_yaml().unwrap().as_str(), Some("3"));
        // c from layer2
        assert_eq!(map.get("c").unwrap().as_yaml().unwrap().as_str(), Some("4"));
    }

    #[test]
    fn test_depth_limit_exceeded() {
        // Create a deeply nested structure
        fn deep_map(depth: usize) -> ConfigValue {
            if depth == 0 {
                scalar("leaf")
            } else {
                map(vec![("nested", deep_map(depth - 1))])
            }
        }

        let config = deep_map(10);
        let merged = MergedConfig::new(vec![&config]);

        // With depth limit of 5, should fail
        let options = MaterializeOptions { max_depth: 5 };
        let result = merged.materialize_with_options(&options);

        assert!(result.is_err());
        match result.unwrap_err() {
            ConfigError::NestingTooDeep { max_depth, path } => {
                assert_eq!(max_depth, 5);
                assert!(!path.is_empty());
            }
            _ => panic!("expected NestingTooDeep error"),
        }
    }

    #[test]
    fn test_depth_limit_ok() {
        // Create a structure within limits
        fn deep_map(depth: usize) -> ConfigValue {
            if depth == 0 {
                scalar("leaf")
            } else {
                map(vec![("nested", deep_map(depth - 1))])
            }
        }

        let config = deep_map(10);
        let merged = MergedConfig::new(vec![&config]);

        // With default depth limit (256), should succeed
        let result = merged.materialize();
        assert!(result.is_ok());
    }

    #[test]
    fn test_merge_with_diagnostics_success() {
        let layer1 = map(vec![("a", scalar("1"))]);
        let layer2 = map(vec![("b", scalar("2"))]);

        let mut diagnostics = Vec::new();
        let result = merge_with_diagnostics(vec![&layer1, &layer2], &mut diagnostics);

        assert!(result.is_some());
        assert!(diagnostics.is_empty());

        let merged = result.unwrap();
        assert!(merged.contains(&["a"]));
        assert!(merged.contains(&["b"]));
    }

    #[test]
    fn test_materialize_preserves_interpretation() {
        let mut config = scalar("**bold**");
        config.interpretation = Some(Interpretation::Markdown);

        let wrapper = map(vec![("content", config)]);
        let merged = MergedConfig::new(vec![&wrapper]);

        let result = merged.materialize().unwrap();
        let content = result.as_map().unwrap().get("content").unwrap();
        assert_eq!(content.interpretation, Some(Interpretation::Markdown));
    }

    #[test]
    fn test_materialize_empty_map() {
        let config = map(vec![]);
        let merged = MergedConfig::new(vec![&config]);

        let result = merged.materialize().unwrap();
        assert!(result.is_map());
        assert!(result.as_map().unwrap().is_empty());
    }

    #[test]
    fn test_materialize_empty_array() {
        let config = map(vec![("items", array(vec![]))]);
        let merged = MergedConfig::new(vec![&config]);

        let result = merged.materialize().unwrap();
        let items = result.as_map().unwrap().get("items").unwrap();
        assert!(items.is_array());
        assert!(items.as_array().unwrap().is_empty());
    }
}
