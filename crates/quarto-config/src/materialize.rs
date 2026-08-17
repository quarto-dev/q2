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
use crate::types::{ConfigError, ConfigMapEntry, ConfigValue, ConfigValueKind, MergeOp};
use quarto_error_reporting::DiagnosticMessage;
use quarto_source_map::{By, SourceInfo};

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

/// The span to stamp on a materialized container (map or array).
///
/// Delegates to [`MergedCursor::container_source`], which returns the
/// real span from the highest-priority layer holding a value at this
/// path.
///
/// # Why not `unwrap_or_default()`
///
/// This used to fall back to `SourceInfo::default()`, which is
/// `Original { file_id: FileId(0), 0..0 }` — a *well-formed* span that
/// renders as a genuine location at the first byte of file 0. A
/// fallback that fabricates a plausible location is worse than one that
/// admits ignorance: it turns "we don't know where this came from" into
/// a confident wrong answer that no downstream consumer can detect.
/// `By::unknown()` is the sanctioned "we don't know" marker
/// (`claude-notes/designs/provenance-contract.md` §10).
///
/// Reaching the fallback means no layer had a value at a path we just
/// resolved a container for, which should not happen; it is defensive
/// rather than expected.
fn container_source(cursor: &MergedCursor<'_>) -> SourceInfo {
    cursor
        .container_source()
        .cloned()
        .unwrap_or_else(|| SourceInfo::generated(By::unknown()))
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
                })
                .collect();

            Ok(ConfigValue {
                value: ConfigValueKind::Array(items),
                source_info: container_source(cursor),
                merge_op: MergeOp::Concat, // Materialized arrays don't have prefer semantics
            })
        }
        Some(MergedValue::Map(map)) => {
            // Materialize map entries recursively
            let mut entries: Vec<ConfigMapEntry> = Vec::new();
            let mut path = path.to_vec();

            for (key, child_cursor) in map.iter() {
                path.push(key.to_string());
                let child_value = materialize_cursor(&child_cursor, depth + 1, options, &path)?;
                path.pop();
                entries.push(ConfigMapEntry {
                    key: key.to_string(),
                    // The key's real span, from the layer that supplies
                    // the winning value. Materialization used to discard
                    // this and stamp a programmatic-config sentinel,
                    // which silently disabled every diagnostic anchored
                    // on a key — see `container_source` (bd-2mxo).
                    key_source: cursor
                        .key_source(key)
                        .cloned()
                        .unwrap_or_else(|| SourceInfo::generated(By::unknown())),
                    value: child_value,
                });
            }

            Ok(ConfigValue {
                value: ConfigValueKind::Map(entries),
                source_info: container_source(cursor),
                merge_op: MergeOp::Concat,
            })
        }
        None => {
            // Path doesn't exist - return null with the "no real source"
            // sentinel.
            Ok(ConfigValue::null(SourceInfo::generated(By::unknown())))
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
            for entry in entries {
                validate_layer(&entry.value, _diagnostics)?;
            }
        }
        _ => {
            // Scalars, Pandoc types, and deferred interpretations (Path/Glob/Expr) are always valid
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use yaml_rust2::Yaml;

    // Helpers
    fn scalar(s: &str) -> ConfigValue {
        ConfigValue::new_scalar(Yaml::String(s.into()), SourceInfo::for_test())
    }

    fn array(items: Vec<ConfigValue>) -> ConfigValue {
        ConfigValue::new_array(items, SourceInfo::for_test())
    }

    fn map(entries: Vec<(&str, ConfigValue)>) -> ConfigValue {
        let map_entries: Vec<ConfigMapEntry> = entries
            .into_iter()
            .map(|(k, v)| ConfigMapEntry {
                key: k.to_string(),
                key_source: SourceInfo::for_test(),
                value: v,
            })
            .collect();
        ConfigValue::new_map(map_entries, SourceInfo::for_test())
    }

    #[test]
    fn test_materialize_scalar() {
        let config = map(vec![("title", scalar("Hello"))]);
        let merged = MergedConfig::new(vec![&config]);

        let result = merged.materialize().unwrap();
        assert!(result.is_map());

        let title = result.get("title").unwrap();
        assert_eq!(title.as_yaml().unwrap().as_str(), Some("Hello"));
    }

    #[test]
    fn test_materialize_array() {
        let config = map(vec![("items", array(vec![scalar("a"), scalar("b")]))]);
        let merged = MergedConfig::new(vec![&config]);

        let result = merged.materialize().unwrap();
        let items = result.get("items").unwrap();
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
        let format = result.get("format").unwrap();
        let html = format.get("html").unwrap();
        let theme = html.get("theme").unwrap();
        assert_eq!(theme.as_yaml().unwrap().as_str(), Some("cosmo"));
    }

    #[test]
    fn test_materialize_merged_layers() {
        let layer1 = map(vec![("a", scalar("1")), ("b", scalar("2"))]);
        let layer2 = map(vec![("b", scalar("3")), ("c", scalar("4"))]);
        let merged = MergedConfig::new(vec![&layer1, &layer2]);

        let result = merged.materialize().unwrap();

        // a from layer1
        assert_eq!(
            result.get("a").unwrap().as_yaml().unwrap().as_str(),
            Some("1")
        );
        // b overridden by layer2
        assert_eq!(
            result.get("b").unwrap().as_yaml().unwrap().as_str(),
            Some("3")
        );
        // c from layer2
        assert_eq!(
            result.get("c").unwrap().as_yaml().unwrap().as_str(),
            Some("4")
        );
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
    fn test_materialize_empty_map() {
        let config = map(vec![]);
        let merged = MergedConfig::new(vec![&config]);

        let result = merged.materialize().unwrap();
        assert!(result.is_map());
        assert!(result.is_empty());
    }

    #[test]
    fn test_materialize_empty_array() {
        let config = map(vec![("items", array(vec![]))]);
        let merged = MergedConfig::new(vec![&config]);

        let result = merged.materialize().unwrap();
        let items = result.get("items").unwrap();
        assert!(items.is_array());
        assert!(items.as_array().unwrap().is_empty());
    }

    // ── source-span preservation (bd-2mxo / bd-9yh3pzfu) ────────────
    //
    // The helpers above stamp `SourceInfo::for_test()` on everything,
    // which is why the span defect was invisible here for so long: with
    // synthetic inputs, a synthesized output span looks identical to a
    // preserved one. These tests parse real YAML so the spans mean
    // something, and assert on the *text* each span covers.
    mod spans {
        use super::*;
        use crate::span_assert::{ResolvedSpan, resolve_span};
        use quarto_source_map::SourceContext;

        /// Parse real YAML into a `ConfigValue` whose spans point into
        /// `text`, plus a context that can resolve them.
        fn layer(name: &str, text: &str) -> (ConfigValue, SourceContext) {
            let parsed = quarto_yaml::parse_file(text, name).expect("valid yaml");
            let mut diags = Vec::new();
            let cv = crate::config_value_from_yaml(parsed, &mut diags);
            (cv, crate::span_assert::context_for(name, text))
        }

        fn span_of(value: &ConfigValue, ctx: &SourceContext) -> ResolvedSpan {
            resolve_span(&value.source_info, ctx)
                .unwrap_or_else(|p| panic!("span should resolve, got: {p}"))
        }

        const DOC: &str = "\
listing:
    sort: false
    template: t.ejs
";

        #[test]
        fn map_container_span_covers_the_mapping_not_its_first_value() {
            let (cv, ctx) = layer("doc.yml", DOC);
            let merged = MergedConfig::new(vec![&cv]).materialize().unwrap();

            let listing = merged.get("listing").expect("listing key");
            let span = span_of(listing, &ctx);

            // The bug: the container borrowed its first entry's *value*
            // span, so this was exactly "false".
            assert_ne!(
                span.text, "false",
                "map container span is still borrowing its first entry's value"
            );
            // quarto-yaml spans a mapping from its first key to
            // MappingEnd, so the whole block is the correct answer.
            assert!(
                span.text.starts_with("sort:") && span.text.contains("template: t.ejs"),
                "expected the whole mapping, got {:?}",
                span.text
            );
        }

        #[test]
        fn map_entries_keep_their_real_key_spans() {
            let (cv, ctx) = layer("doc.yml", DOC);
            let merged = MergedConfig::new(vec![&cv]).materialize().unwrap();

            let listing = merged.get("listing").expect("listing key");
            let ConfigValueKind::Map(entries) = &listing.value else {
                panic!("expected a map");
            };

            for entry in entries {
                let span = resolve_span(&entry.key_source, &ctx).unwrap_or_else(|p| {
                    panic!(
                        "key `{}` should have a real key_source, got: {p}",
                        entry.key
                    )
                });
                assert_eq!(
                    span.text, entry.key,
                    "key_source should cover the key itself"
                );
            }
        }

        #[test]
        fn array_container_span_is_not_merely_its_last_item() {
            const ARR: &str = "\
contents:
    - ./a.qmd
    - ./b.qmd
";
            let (cv, ctx) = layer("arr.yml", ARR);
            let merged = MergedConfig::new(vec![&cv]).materialize().unwrap();

            let contents = merged.get("contents").expect("contents key");
            let span = span_of(contents, &ctx);

            // The bug: `.items.last()` gave exactly "./b.qmd".
            assert_ne!(
                span.text, "./b.qmd",
                "array container span is still borrowing its last item"
            );
            assert!(
                span.text.contains("./a.qmd") && span.text.contains("./b.qmd"),
                "expected the whole sequence, got {:?}",
                span.text
            );
        }

        #[test]
        fn nested_map_first_child_no_longer_collapses_to_a_sentinel() {
            // When the first entry was itself a map, the old code gave
            // up entirely and stamped a programmatic-config sentinel.
            const NESTED: &str = "\
outer:
    inner:
        a: 1
    sibling: 2
";
            let (cv, ctx) = layer("nested.yml", NESTED);
            let merged = MergedConfig::new(vec![&cv]).materialize().unwrap();

            let outer = merged.get("outer").expect("outer key");
            let span = span_of(outer, &ctx);
            assert!(
                span.text.starts_with("inner:"),
                "expected the outer mapping's real span, got {:?}",
                span.text
            );
        }

        #[test]
        fn winning_layer_supplies_the_span_when_layers_disagree() {
            // Values are last-wins; the span should follow the value.
            let (base, _base_ctx) = layer("base.yml", "listing:\n    sort: true\n");
            let (over, over_ctx) = layer("over.yml", "listing:\n    sort: false\n");

            let merged = MergedConfig::new(vec![&base, &over]).materialize().unwrap();
            let listing = merged.get("listing").expect("listing key");
            let sort = listing.get("sort").expect("sort key");

            // `over.yml` won the value, so its span must resolve there.
            let span = resolve_span(&sort.source_info, &over_ctx)
                .expect("winning value's span should resolve in the winning layer");
            assert_eq!(span.path, "over.yml");
            assert_eq!(span.text, "false");
        }
    }
}
