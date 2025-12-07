//! Cursor-based merged configuration navigation.
//!
//! This module provides lazy, zero-copy merging of configuration layers
//! with a cursor-based API for ergonomic navigation.
//!
//! # Design
//!
//! - `MergedConfig<'a>` holds borrowed references to config layers (zero-copy construction)
//! - `MergedCursor<'a>` provides path-based navigation without copying
//! - Resolution happens lazily when `as_*()` methods are called
//! - Merge semantics (`!prefer`/`!concat`) are applied at resolution time
//!
//! # Example
//!
//! ```rust,ignore
//! let project_config = parse_config("_quarto.yml");
//! let doc_config = parse_config("document.qmd");
//!
//! // Zero-copy construction
//! let merged = MergedConfig::new(vec![&project_config, &doc_config]);
//!
//! // Cursor-based navigation
//! let theme = merged.cursor()
//!     .at("format")
//!     .at("html")
//!     .at("theme")
//!     .as_scalar();
//! ```

use crate::types::{ConfigValue, ConfigValueKind, MergeOp};
use indexmap::IndexMap;

/// A lazily-evaluated merged configuration.
///
/// The lifetime parameter `'a` indicates that `MergedConfig` borrows from
/// existing `ConfigValue` data. This is zero-copy construction.
#[derive(Debug, Clone)]
pub struct MergedConfig<'a> {
    /// Ordered list of config layers (first = lowest priority, last = highest)
    layers: Vec<&'a ConfigValue>,
}

/// A cursor for navigating merged configuration.
///
/// The cursor is lightweight: it stores a reference to the config
/// and a path. Resolution happens lazily when you call `as_*()` methods.
#[derive(Debug, Clone)]
pub struct MergedCursor<'a> {
    config: &'a MergedConfig<'a>,
    path: Vec<String>,
}

/// A resolved scalar value with its source.
#[derive(Debug, Clone)]
pub struct MergedScalar<'a> {
    /// The resolved value
    pub value: &'a ConfigValue,
    /// Which layer this value came from (index into layers)
    pub layer_index: usize,
}

/// An item in a resolved array.
#[derive(Debug, Clone)]
pub struct MergedArrayItem<'a> {
    /// The item value
    pub value: &'a ConfigValue,
    /// Which layer this item came from
    pub layer_index: usize,
}

/// A resolved array with merge semantics applied.
#[derive(Debug, Clone)]
pub struct MergedArray<'a> {
    /// Items after applying prefer/concat semantics
    pub items: Vec<MergedArrayItem<'a>>,
}

/// A resolved map with merge semantics applied.
///
/// This is a "virtual" map that computes its keys from all layers
/// and provides cursor-based access to values.
#[derive(Debug, Clone)]
pub struct MergedMap<'a> {
    config: &'a MergedConfig<'a>,
    path: Vec<String>,
    /// Keys present in the merged map (computed from all layers)
    keys: Vec<String>,
}

/// A resolved value of any type.
///
/// Use this when the type is not known ahead of time.
#[derive(Debug, Clone)]
pub enum MergedValue<'a> {
    Scalar(MergedScalar<'a>),
    Array(MergedArray<'a>),
    Map(MergedMap<'a>),
}

impl<'a> MergedConfig<'a> {
    /// Create a merged config from multiple layers.
    ///
    /// Layers are ordered by priority: first = lowest priority, last = highest.
    /// When values conflict, higher-priority layers win (subject to merge semantics).
    pub fn new(layers: Vec<&'a ConfigValue>) -> Self {
        MergedConfig { layers }
    }

    /// Create an empty merged config.
    pub fn empty() -> Self {
        MergedConfig { layers: Vec::new() }
    }

    /// Add a new layer (returns new MergedConfig, doesn't mutate).
    ///
    /// The new layer has higher priority than existing layers.
    pub fn with_layer(&self, layer: &'a ConfigValue) -> MergedConfig<'a> {
        let mut new_layers = self.layers.clone();
        new_layers.push(layer);
        MergedConfig { layers: new_layers }
    }

    /// Get the number of layers.
    pub fn layer_count(&self) -> usize {
        self.layers.len()
    }

    /// Get a cursor at the root.
    pub fn cursor(&'a self) -> MergedCursor<'a> {
        MergedCursor {
            config: self,
            path: Vec::new(),
        }
    }

    // Convenience methods that delegate to cursor

    /// Get a scalar value at a path.
    pub fn get_scalar(&'a self, path: &[&str]) -> Option<MergedScalar<'a>> {
        self.cursor().at_path(path).as_scalar()
    }

    /// Get an array value at a path.
    pub fn get_array(&'a self, path: &[&str]) -> Option<MergedArray<'a>> {
        self.cursor().at_path(path).as_array()
    }

    /// Get a map value at a path.
    pub fn get_map(&'a self, path: &[&str]) -> Option<MergedMap<'a>> {
        self.cursor().at_path(path).as_map()
    }

    /// Check if a path exists in any layer.
    pub fn contains(&'a self, path: &[&str]) -> bool {
        self.cursor().at_path(path).exists()
    }
}

impl<'a> MergedCursor<'a> {
    /// Navigate to a child key.
    ///
    /// Returns a new cursor at the child path. The cursor is valid even if
    /// the path doesn't exist - you'll get `None` when resolving.
    pub fn at(&self, key: &str) -> MergedCursor<'a> {
        let mut path = self.path.clone();
        path.push(key.to_string());
        MergedCursor {
            config: self.config,
            path,
        }
    }

    /// Navigate to a path (multiple keys at once).
    pub fn at_path(&self, path: &[&str]) -> MergedCursor<'a> {
        let mut new_path = self.path.clone();
        new_path.extend(path.iter().map(|s| s.to_string()));
        MergedCursor {
            config: self.config,
            path: new_path,
        }
    }

    /// Get the current path.
    pub fn path(&self) -> &[String] {
        &self.path
    }

    /// Check if this path exists in any layer.
    pub fn exists(&self) -> bool {
        self.config
            .layers
            .iter()
            .any(|layer| self.navigate_to(layer).is_some())
    }

    /// Get child keys at this path (union across layers, respecting merge semantics).
    ///
    /// Returns keys in a deterministic order: keys from earlier layers first,
    /// then keys from later layers that weren't already present.
    pub fn keys(&self) -> Vec<String> {
        let mut seen_keys = IndexMap::new();
        let mut reset_point = 0;

        // Walk layers in order, tracking reset points from !prefer
        for (layer_idx, layer) in self.config.layers.iter().enumerate() {
            if let Some(value) = self.navigate_to(layer) {
                // Check if this layer resets the map
                if value.merge_op == MergeOp::Prefer {
                    seen_keys.clear();
                    reset_point = layer_idx;
                }

                // Add keys from this layer's map
                if let ConfigValueKind::Map(map) = &value.value {
                    for key in map.keys() {
                        seen_keys.entry(key.clone()).or_insert(layer_idx);
                    }
                }
            }
        }

        // Only return keys from layers at or after the reset point
        seen_keys
            .into_iter()
            .filter(|(_, idx)| *idx >= reset_point)
            .map(|(key, _)| key)
            .collect()
    }

    /// Resolve as any value type.
    ///
    /// Use this when the type is not known ahead of time.
    pub fn as_value(&self) -> Option<MergedValue<'a>> {
        // Find the highest-priority layer that has this path
        for (i, layer) in self.config.layers.iter().enumerate().rev() {
            if let Some(value) = self.navigate_to(layer) {
                return match &value.value {
                    ConfigValueKind::Scalar(_)
                    | ConfigValueKind::PandocInlines(_)
                    | ConfigValueKind::PandocBlocks(_) => {
                        Some(MergedValue::Scalar(MergedScalar {
                            value,
                            layer_index: i,
                        }))
                    }
                    ConfigValueKind::Array(_) => self.as_array().map(MergedValue::Array),
                    ConfigValueKind::Map(_) => self.as_map().map(MergedValue::Map),
                };
            }
        }
        None
    }

    /// Resolve as scalar (last-wins semantics).
    ///
    /// Scalars, PandocInlines, and PandocBlocks all default to `!prefer` (last wins).
    pub fn as_scalar(&self) -> Option<MergedScalar<'a>> {
        // Walk layers in reverse (highest priority first)
        for (i, layer) in self.config.layers.iter().enumerate().rev() {
            if let Some(value) = self.navigate_to(layer) {
                if matches!(
                    value.value,
                    ConfigValueKind::Scalar(_)
                        | ConfigValueKind::PandocInlines(_)
                        | ConfigValueKind::PandocBlocks(_)
                ) {
                    return Some(MergedScalar {
                        value,
                        layer_index: i,
                    });
                }
            }
        }
        None
    }

    /// Resolve as array (applying prefer/concat semantics).
    ///
    /// - `!prefer`: Clears all previous items, uses only this layer's items
    /// - `!concat` (default): Appends to previous items
    pub fn as_array(&self) -> Option<MergedArray<'a>> {
        let mut items: Vec<MergedArrayItem<'a>> = Vec::new();
        let mut found_any = false;

        // Walk layers in order (lowest priority first)
        for (i, layer) in self.config.layers.iter().enumerate() {
            if let Some(value) = self.navigate_to(layer) {
                if let ConfigValueKind::Array(arr) = &value.value {
                    found_any = true;

                    // Apply merge semantics
                    match value.merge_op {
                        MergeOp::Prefer => {
                            // Reset: discard all previous items
                            items.clear();
                        }
                        MergeOp::Concat => {
                            // Concatenate: keep existing items
                        }
                    }

                    // Add items from this layer
                    for item in arr {
                        items.push(MergedArrayItem {
                            value: item,
                            layer_index: i,
                        });
                    }
                }
            }
        }

        if found_any {
            Some(MergedArray { items })
        } else {
            None
        }
    }

    /// Resolve as map (applying prefer/concat semantics).
    ///
    /// - `!prefer`: Replaces entire map (no field-wise merge)
    /// - `!concat` (default): Field-wise merge with previous maps
    pub fn as_map(&self) -> Option<MergedMap<'a>> {
        // Check if any layer has a map at this path
        let keys = self.keys();
        if keys.is_empty() {
            // Check if there's actually a map here (could be empty map)
            let has_map = self.config.layers.iter().any(|layer| {
                self.navigate_to(layer)
                    .map(|v| matches!(v.value, ConfigValueKind::Map(_)))
                    .unwrap_or(false)
            });

            if !has_map {
                return None;
            }
        }

        Some(MergedMap {
            config: self.config,
            path: self.path.clone(),
            keys,
        })
    }

    /// Navigate to a path within a single layer.
    fn navigate_to(&self, root: &'a ConfigValue) -> Option<&'a ConfigValue> {
        let mut current = root;
        for key in &self.path {
            match &current.value {
                ConfigValueKind::Map(map) => {
                    current = map.get(key)?;
                }
                _ => return None,
            }
        }
        Some(current)
    }
}

impl<'a> MergedMap<'a> {
    /// Get the keys in this map.
    pub fn keys(&self) -> &[String] {
        &self.keys
    }

    /// Check if the map is empty.
    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    /// Get the number of keys.
    pub fn len(&self) -> usize {
        self.keys.len()
    }

    /// Get a cursor for a specific key.
    pub fn get(&self, key: &str) -> Option<MergedCursor<'a>> {
        if self.keys.iter().any(|k| k == key) {
            let mut path = self.path.clone();
            path.push(key.to_string());
            Some(MergedCursor {
                config: self.config,
                path,
            })
        } else {
            None
        }
    }

    /// Check if the map contains a key.
    pub fn contains_key(&self, key: &str) -> bool {
        self.keys.iter().any(|k| k == key)
    }

    /// Iterate over (key, cursor) pairs.
    pub fn iter(&self) -> impl Iterator<Item = (&str, MergedCursor<'a>)> {
        self.keys.iter().map(move |key| {
            let mut path = self.path.clone();
            path.push(key.clone());
            (
                key.as_str(),
                MergedCursor {
                    config: self.config,
                    path,
                },
            )
        })
    }
}

impl<'a> MergedArray<'a> {
    /// Get the number of items.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Check if the array is empty.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Get an item by index.
    pub fn get(&self, index: usize) -> Option<&MergedArrayItem<'a>> {
        self.items.get(index)
    }

    /// Iterate over items.
    pub fn iter(&self) -> impl Iterator<Item = &MergedArrayItem<'a>> {
        self.items.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_source_map::SourceInfo;
    use yaml_rust2::Yaml;

    // Helper to create a scalar ConfigValue
    fn scalar(s: &str) -> ConfigValue {
        ConfigValue::new_scalar(Yaml::String(s.into()), SourceInfo::default())
    }

    // Helper to create an array ConfigValue
    fn array(items: Vec<ConfigValue>) -> ConfigValue {
        ConfigValue::new_array(items, SourceInfo::default())
    }

    // Helper to create a map ConfigValue
    fn map(entries: Vec<(&str, ConfigValue)>) -> ConfigValue {
        let map: IndexMap<String, ConfigValue> = entries
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect();
        ConfigValue::new_map(map, SourceInfo::default())
    }

    // Helper to create a map with prefer semantics
    fn map_prefer(entries: Vec<(&str, ConfigValue)>) -> ConfigValue {
        map(entries).with_merge_op(MergeOp::Prefer)
    }

    // Helper to create an array with prefer semantics
    fn array_prefer(items: Vec<ConfigValue>) -> ConfigValue {
        array(items).with_merge_op(MergeOp::Prefer)
    }

    #[test]
    fn test_empty_config() {
        let merged = MergedConfig::empty();
        assert_eq!(merged.layer_count(), 0);
        assert!(!merged.contains(&["foo"]));
    }

    #[test]
    fn test_single_layer_scalar() {
        let config = map(vec![("title", scalar("Hello"))]);
        let merged = MergedConfig::new(vec![&config]);

        let result = merged.get_scalar(&["title"]);
        assert!(result.is_some());
        assert_eq!(
            result.unwrap().value.as_yaml().unwrap().as_str(),
            Some("Hello")
        );
    }

    #[test]
    fn test_scalar_override() {
        let layer1 = map(vec![("title", scalar("First"))]);
        let layer2 = map(vec![("title", scalar("Second"))]);
        let merged = MergedConfig::new(vec![&layer1, &layer2]);

        let result = merged.get_scalar(&["title"]).unwrap();
        assert_eq!(result.value.as_yaml().unwrap().as_str(), Some("Second"));
        assert_eq!(result.layer_index, 1);
    }

    #[test]
    fn test_nested_path() {
        let config = map(vec![(
            "format",
            map(vec![("html", map(vec![("theme", scalar("cosmo"))]))]),
        )]);
        let merged = MergedConfig::new(vec![&config]);

        let result = merged.get_scalar(&["format", "html", "theme"]);
        assert!(result.is_some());
        assert_eq!(
            result.unwrap().value.as_yaml().unwrap().as_str(),
            Some("cosmo")
        );
    }

    #[test]
    fn test_cursor_chaining() {
        let config = map(vec![(
            "format",
            map(vec![("html", map(vec![("theme", scalar("cosmo"))]))]),
        )]);
        let merged = MergedConfig::new(vec![&config]);

        let result = merged
            .cursor()
            .at("format")
            .at("html")
            .at("theme")
            .as_scalar();
        assert!(result.is_some());
        assert_eq!(
            result.unwrap().value.as_yaml().unwrap().as_str(),
            Some("cosmo")
        );
    }

    #[test]
    fn test_array_concat_default() {
        let layer1 = map(vec![("items", array(vec![scalar("a"), scalar("b")]))]);
        let layer2 = map(vec![("items", array(vec![scalar("c")]))]);
        let merged = MergedConfig::new(vec![&layer1, &layer2]);

        let result = merged.get_array(&["items"]).unwrap();
        assert_eq!(result.len(), 3);

        let values: Vec<_> = result
            .iter()
            .map(|item| item.value.as_yaml().unwrap().as_str().unwrap())
            .collect();
        assert_eq!(values, vec!["a", "b", "c"]);
    }

    #[test]
    fn test_array_prefer_resets() {
        let layer1 = map(vec![("items", array(vec![scalar("a"), scalar("b")]))]);
        let layer2 = map(vec![("items", array_prefer(vec![scalar("c")]))]);
        let merged = MergedConfig::new(vec![&layer1, &layer2]);

        let result = merged.get_array(&["items"]).unwrap();
        assert_eq!(result.len(), 1);

        let values: Vec<_> = result
            .iter()
            .map(|item| item.value.as_yaml().unwrap().as_str().unwrap())
            .collect();
        assert_eq!(values, vec!["c"]);
    }

    #[test]
    fn test_map_field_wise_merge() {
        let layer1 = map(vec![
            ("format", map(vec![("html", scalar("default"))])),
        ]);
        let layer2 = map(vec![
            ("format", map(vec![("pdf", scalar("article"))])),
        ]);
        let merged = MergedConfig::new(vec![&layer1, &layer2]);

        // Both keys should be present
        let format_map = merged.get_map(&["format"]).unwrap();
        assert!(format_map.contains_key("html"));
        assert!(format_map.contains_key("pdf"));
        assert_eq!(format_map.len(), 2);
    }

    #[test]
    fn test_map_prefer_resets() {
        let layer1 = map(vec![
            ("format", map(vec![("html", scalar("default"))])),
        ]);
        let layer2 = map(vec![
            ("format", map_prefer(vec![("pdf", scalar("article"))])),
        ]);
        let merged = MergedConfig::new(vec![&layer1, &layer2]);

        // Only pdf should be present (prefer resets)
        let format_map = merged.get_map(&["format"]).unwrap();
        assert!(!format_map.contains_key("html"));
        assert!(format_map.contains_key("pdf"));
        assert_eq!(format_map.len(), 1);
    }

    #[test]
    fn test_map_iter() {
        let config = map(vec![
            ("a", scalar("1")),
            ("b", scalar("2")),
            ("c", scalar("3")),
        ]);
        let merged = MergedConfig::new(vec![&config]);

        let root_map = merged.get_map(&[]).unwrap();
        let pairs: Vec<_> = root_map.iter().collect();
        assert_eq!(pairs.len(), 3);

        // Check we can resolve through the cursors
        for (key, cursor) in pairs {
            let value = cursor.as_scalar().unwrap();
            match key {
                "a" => assert_eq!(value.value.as_yaml().unwrap().as_str(), Some("1")),
                "b" => assert_eq!(value.value.as_yaml().unwrap().as_str(), Some("2")),
                "c" => assert_eq!(value.value.as_yaml().unwrap().as_str(), Some("3")),
                _ => panic!("unexpected key: {}", key),
            }
        }
    }

    #[test]
    fn test_exists() {
        let config = map(vec![("title", scalar("Hello"))]);
        let merged = MergedConfig::new(vec![&config]);

        assert!(merged.contains(&["title"]));
        assert!(!merged.contains(&["missing"]));
        assert!(!merged.contains(&["title", "nested"]));
    }

    #[test]
    fn test_as_value_scalar() {
        let config = map(vec![("title", scalar("Hello"))]);
        let merged = MergedConfig::new(vec![&config]);

        match merged.cursor().at("title").as_value() {
            Some(MergedValue::Scalar(s)) => {
                assert_eq!(s.value.as_yaml().unwrap().as_str(), Some("Hello"));
            }
            _ => panic!("expected scalar"),
        }
    }

    #[test]
    fn test_as_value_array() {
        let config = map(vec![("items", array(vec![scalar("a")]))]);
        let merged = MergedConfig::new(vec![&config]);

        match merged.cursor().at("items").as_value() {
            Some(MergedValue::Array(a)) => {
                assert_eq!(a.len(), 1);
            }
            _ => panic!("expected array"),
        }
    }

    #[test]
    fn test_as_value_map() {
        let config = map(vec![("format", map(vec![("html", scalar("default"))]))]);
        let merged = MergedConfig::new(vec![&config]);

        match merged.cursor().at("format").as_value() {
            Some(MergedValue::Map(m)) => {
                assert!(m.contains_key("html"));
            }
            _ => panic!("expected map"),
        }
    }

    #[test]
    fn test_with_layer() {
        let layer1 = map(vec![("a", scalar("1"))]);
        let layer2 = map(vec![("b", scalar("2"))]);

        let merged1 = MergedConfig::new(vec![&layer1]);
        let merged2 = merged1.with_layer(&layer2);

        assert_eq!(merged1.layer_count(), 1);
        assert_eq!(merged2.layer_count(), 2);

        assert!(merged2.contains(&["a"]));
        assert!(merged2.contains(&["b"]));
    }

    #[test]
    fn test_path_accessor() {
        let config = map(vec![("a", scalar("1"))]);
        let merged = MergedConfig::new(vec![&config]);

        let cursor = merged.cursor().at("foo").at("bar");
        assert_eq!(cursor.path(), &["foo", "bar"]);
    }

    // Associativity tests
    #[test]
    fn test_associativity_scalars() {
        // For scalars: (a <> b) <> c == a <> (b <> c)
        // Both should give c's value
        let a = map(vec![("x", scalar("a"))]);
        let b = map(vec![("x", scalar("b"))]);
        let c = map(vec![("x", scalar("c"))]);

        // (a <> b) <> c
        let merged_left = MergedConfig::new(vec![&a, &b, &c]);

        // a <> (b <> c) - same thing since we're just listing layers
        let merged_right = MergedConfig::new(vec![&a, &b, &c]);

        let left_val = merged_left.get_scalar(&["x"]).unwrap();
        let right_val = merged_right.get_scalar(&["x"]).unwrap();

        assert_eq!(
            left_val.value.as_yaml().unwrap().as_str(),
            right_val.value.as_yaml().unwrap().as_str()
        );
        assert_eq!(left_val.value.as_yaml().unwrap().as_str(), Some("c"));
    }

    #[test]
    fn test_associativity_arrays_concat() {
        // For arrays with concat: (a <> b) <> c == a <> (b <> c)
        // Both should give [a_items, b_items, c_items]
        let a = map(vec![("arr", array(vec![scalar("a")]))]);
        let b = map(vec![("arr", array(vec![scalar("b")]))]);
        let c = map(vec![("arr", array(vec![scalar("c")]))]);

        let merged = MergedConfig::new(vec![&a, &b, &c]);

        let result = merged.get_array(&["arr"]).unwrap();
        let values: Vec<_> = result
            .iter()
            .map(|item| item.value.as_yaml().unwrap().as_str().unwrap())
            .collect();

        assert_eq!(values, vec!["a", "b", "c"]);
    }

    #[test]
    fn test_associativity_arrays_with_prefer() {
        // For arrays with prefer in middle: prefer should reset regardless of grouping
        let a = map(vec![("arr", array(vec![scalar("a")]))]);
        let b = map(vec![("arr", array_prefer(vec![scalar("b")]))]); // !prefer
        let c = map(vec![("arr", array(vec![scalar("c")]))]);

        let merged = MergedConfig::new(vec![&a, &b, &c]);

        let result = merged.get_array(&["arr"]).unwrap();
        let values: Vec<_> = result
            .iter()
            .map(|item| item.value.as_yaml().unwrap().as_str().unwrap())
            .collect();

        // b's !prefer discards a, then c concatenates
        assert_eq!(values, vec!["b", "c"]);
    }

    #[test]
    fn test_deep_nesting() {
        let config = map(vec![(
            "a",
            map(vec![(
                "b",
                map(vec![("c", map(vec![("d", map(vec![("e", scalar("deep"))]))]))]),
            )]),
        )]);
        let merged = MergedConfig::new(vec![&config]);

        let result = merged.get_scalar(&["a", "b", "c", "d", "e"]);
        assert!(result.is_some());
        assert_eq!(
            result.unwrap().value.as_yaml().unwrap().as_str(),
            Some("deep")
        );
    }
}
