/*
 * config_value.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Core configuration value types for Quarto.
 *
 * These types support configuration merging with source tracking.
 * They are used both for project configuration (_quarto.yml) and
 * document metadata (frontmatter).
 */

use crate::block::Blocks;
use crate::inline::{Inline, Inlines};
use quarto_source_map::SourceInfo;
use serde::{Deserialize, Serialize};
use yaml_rust2::Yaml;

/// Merge operation for a value.
///
/// Controls how values from different configuration layers are combined.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
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
/// Used during tag parsing to determine how to convert YAML strings.
/// The interpretation is resolved at conversion time:
/// - `Markdown` → converts to `PandocInlines` or `PandocBlocks`
/// - `PlainString` → keeps as `Scalar(Yaml::String)` or wraps in single Str inline
/// - `Path`, `Glob`, `Expr` → creates the corresponding ConfigValueKind variant
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

/// Context for interpreting string values in configuration/metadata.
///
/// This determines the default behavior for untagged strings:
/// - `DocumentMetadata`: Strings are parsed as markdown by default (use `!str` to keep literal)
/// - `ProjectConfig`: Strings are kept literal by default (use `!md` to parse as markdown)
///
/// Explicit tags (`!md`, `!str`, `!path`, `!glob`, `!expr`) always override the default.
///
/// # Example
///
/// ```yaml
/// # In document frontmatter (DocumentMetadata context):
/// title: "**Bold** Title"      # Parsed as markdown → PandocInlines
/// path: !str "raw/path.txt"    # Kept literal → Scalar(String)
///
/// # In _quarto.yml (ProjectConfig context):
/// output-dir: "_site"          # Kept literal → Scalar(String)
/// description: !md "**Bold**"  # Parsed as markdown → PandocInlines
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InterpretationContext {
    /// Document frontmatter: strings are parsed as markdown by default.
    ///
    /// Use `!str` tag to keep a string literal (no markdown parsing).
    #[default]
    DocumentMetadata,

    /// Project config (_quarto.yml): strings are kept literal by default.
    ///
    /// Use `!md` tag to parse a string as markdown.
    ProjectConfig,
}

/// A configuration value with explicit merge semantics.
///
/// This is the core type for configuration merging. It wraps a value with:
/// - Source location for error reporting
/// - Merge operation (prefer vs concat)
///
/// The interpretation (how strings are handled) is encoded in the `ConfigValueKind`:
/// - `!path` creates `Path(String)`
/// - `!glob` creates `Glob(String)`
/// - `!expr` creates `Expr(String)`
/// - `!md` creates `PandocInlines` or `PandocBlocks`
/// - `!str` creates `Scalar(Yaml::String)`
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConfigValue {
    /// The underlying value
    pub value: ConfigValueKind,

    /// Source location for this value
    pub source_info: SourceInfo,

    /// Merge operation (derived from tag or inferred)
    pub merge_op: MergeOp,
}

/// Map entry with key source tracking.
///
/// This allows error messages to point to the key location in the source file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConfigMapEntry {
    /// The key string
    pub key: String,

    /// Source location of the key
    pub key_source: SourceInfo,

    /// The value associated with this key
    pub value: ConfigValue,
}

/// The kind of configuration value.
///
/// This mirrors YAML/JSON value types plus Pandoc AST types for
/// already-interpreted values, plus deferred interpretation variants.
#[derive(Debug, Clone, PartialEq)]
pub enum ConfigValueKind {
    // === Scalar values (interpretation resolved or not applicable) ===
    /// Atomic values (String, Int, Float, Bool, Null).
    ///
    /// For strings, this means "keep as literal" (was `!str` or context default).
    /// Always uses "last wins" semantics regardless of MergeOp.
    Scalar(Yaml),

    // === Parsed content (interpretation happened at parse time) ===
    /// Pandoc inline content (for already-interpreted values).
    ///
    /// Created when `!md` tag is used or in document metadata context.
    /// Default: `!prefer` (last wins, no concatenation).
    /// Use `!concat` explicitly if concatenation is desired.
    PandocInlines(Inlines),

    /// Pandoc block content (for already-interpreted values).
    ///
    /// Default: `!prefer` (last wins, no concatenation).
    /// Use `!concat` explicitly if concatenation is desired.
    PandocBlocks(Blocks),

    // === Deferred interpretation (needs later processing) ===
    /// Path to resolve relative to source file (`!path` tag).
    Path(String),

    /// Glob pattern to expand (`!glob` tag).
    Glob(String),

    /// Runtime expression to evaluate (`!expr` tag).
    Expr(String),

    // === Compound values ===
    /// Arrays: merge_op controls concatenate vs replace.
    Array(Vec<ConfigValue>),

    /// Objects with key source tracking: merge_op controls field-wise merge vs replace.
    Map(Vec<ConfigMapEntry>),
}

// Custom serialization for ConfigValueKind to handle Yaml type
impl Serialize for ConfigValueKind {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeMap;

        match self {
            ConfigValueKind::Scalar(yaml) => {
                let mut map = serializer.serialize_map(Some(1))?;
                map.serialize_entry("Scalar", &yaml_to_serde_value(yaml))?;
                map.end()
            }
            ConfigValueKind::PandocInlines(inlines) => {
                let mut map = serializer.serialize_map(Some(1))?;
                map.serialize_entry("PandocInlines", inlines)?;
                map.end()
            }
            ConfigValueKind::PandocBlocks(blocks) => {
                let mut map = serializer.serialize_map(Some(1))?;
                map.serialize_entry("PandocBlocks", blocks)?;
                map.end()
            }
            ConfigValueKind::Path(s) => {
                let mut map = serializer.serialize_map(Some(1))?;
                map.serialize_entry("Path", s)?;
                map.end()
            }
            ConfigValueKind::Glob(s) => {
                let mut map = serializer.serialize_map(Some(1))?;
                map.serialize_entry("Glob", s)?;
                map.end()
            }
            ConfigValueKind::Expr(s) => {
                let mut map = serializer.serialize_map(Some(1))?;
                map.serialize_entry("Expr", s)?;
                map.end()
            }
            ConfigValueKind::Array(items) => {
                let mut map = serializer.serialize_map(Some(1))?;
                map.serialize_entry("Array", items)?;
                map.end()
            }
            ConfigValueKind::Map(entries) => {
                let mut map = serializer.serialize_map(Some(1))?;
                map.serialize_entry("Map", entries)?;
                map.end()
            }
        }
    }
}

/// Convert Yaml to a serde-serializable value
fn yaml_to_serde_value(yaml: &Yaml) -> serde_json::Value {
    match yaml {
        Yaml::String(s) => serde_json::Value::String(s.clone()),
        Yaml::Integer(i) => serde_json::Value::Number((*i).into()),
        Yaml::Real(s) => {
            if let Ok(f) = s.parse::<f64>() {
                serde_json::Number::from_f64(f)
                    .map(serde_json::Value::Number)
                    .unwrap_or(serde_json::Value::String(s.clone()))
            } else {
                serde_json::Value::String(s.clone())
            }
        }
        Yaml::Boolean(b) => serde_json::Value::Bool(*b),
        Yaml::Null => serde_json::Value::Null,
        Yaml::Array(arr) => serde_json::Value::Array(arr.iter().map(yaml_to_serde_value).collect()),
        Yaml::Hash(hash) => {
            let mut map = serde_json::Map::new();
            for (k, v) in hash {
                if let Yaml::String(key) = k {
                    map.insert(key.clone(), yaml_to_serde_value(v));
                }
            }
            serde_json::Value::Object(map)
        }
        Yaml::Alias(_) => serde_json::Value::Null, // Aliases are resolved during parsing
        Yaml::BadValue => serde_json::Value::Null,
    }
}

impl<'de> Deserialize<'de> for ConfigValueKind {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::{MapAccess, Visitor};
        use std::fmt;

        struct ConfigValueKindVisitor;

        impl<'de> Visitor<'de> for ConfigValueKindVisitor {
            type Value = ConfigValueKind;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a ConfigValueKind variant")
            }

            fn visit_map<M>(self, mut map: M) -> Result<Self::Value, M::Error>
            where
                M: MapAccess<'de>,
            {
                let key: String = map
                    .next_key()?
                    .ok_or_else(|| serde::de::Error::custom("expected variant key"))?;

                match key.as_str() {
                    "Scalar" => {
                        let value: serde_json::Value = map.next_value()?;
                        Ok(ConfigValueKind::Scalar(serde_value_to_yaml(&value)))
                    }
                    "PandocInlines" => {
                        let inlines: Inlines = map.next_value()?;
                        Ok(ConfigValueKind::PandocInlines(inlines))
                    }
                    "PandocBlocks" => {
                        let blocks: Blocks = map.next_value()?;
                        Ok(ConfigValueKind::PandocBlocks(blocks))
                    }
                    "Path" => {
                        let s: String = map.next_value()?;
                        Ok(ConfigValueKind::Path(s))
                    }
                    "Glob" => {
                        let s: String = map.next_value()?;
                        Ok(ConfigValueKind::Glob(s))
                    }
                    "Expr" => {
                        let s: String = map.next_value()?;
                        Ok(ConfigValueKind::Expr(s))
                    }
                    "Array" => {
                        let items: Vec<ConfigValue> = map.next_value()?;
                        Ok(ConfigValueKind::Array(items))
                    }
                    "Map" => {
                        let entries: Vec<ConfigMapEntry> = map.next_value()?;
                        Ok(ConfigValueKind::Map(entries))
                    }
                    other => Err(serde::de::Error::unknown_variant(
                        other,
                        &[
                            "Scalar",
                            "PandocInlines",
                            "PandocBlocks",
                            "Path",
                            "Glob",
                            "Expr",
                            "Array",
                            "Map",
                        ],
                    )),
                }
            }
        }

        deserializer.deserialize_map(ConfigValueKindVisitor)
    }
}

/// Convert serde_json::Value to Yaml
fn serde_value_to_yaml(value: &serde_json::Value) -> Yaml {
    match value {
        serde_json::Value::String(s) => Yaml::String(s.clone()),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Yaml::Integer(i)
            } else if let Some(f) = n.as_f64() {
                Yaml::Real(f.to_string())
            } else {
                Yaml::String(n.to_string())
            }
        }
        serde_json::Value::Bool(b) => Yaml::Boolean(*b),
        serde_json::Value::Null => Yaml::Null,
        serde_json::Value::Array(arr) => Yaml::Array(arr.iter().map(serde_value_to_yaml).collect()),
        serde_json::Value::Object(obj) => {
            let mut hash = yaml_rust2::yaml::Hash::new();
            for (k, v) in obj {
                hash.insert(Yaml::String(k.clone()), serde_value_to_yaml(v));
            }
            Yaml::Hash(hash)
        }
    }
}

impl Default for ConfigValue {
    /// Default is an empty Map, matching the convention for document metadata.
    fn default() -> Self {
        Self {
            value: ConfigValueKind::Map(vec![]),
            source_info: SourceInfo::default(),
            merge_op: MergeOp::Concat,
        }
    }
}

impl ConfigValue {
    /// Create a new scalar ConfigValue with default merge semantics.
    pub fn new_scalar(yaml: Yaml, source_info: SourceInfo) -> Self {
        Self {
            value: ConfigValueKind::Scalar(yaml),
            source_info,
            merge_op: MergeOp::Concat,
        }
    }

    /// Create a new string scalar ConfigValue.
    ///
    /// This is a convenience method that wraps a string in `Yaml::String`.
    /// Use this when you need to create a string value without importing yaml_rust2.
    pub fn new_string(s: impl Into<String>, source_info: SourceInfo) -> Self {
        Self {
            value: ConfigValueKind::Scalar(Yaml::String(s.into())),
            source_info,
            merge_op: MergeOp::Concat,
        }
    }

    /// Create a new boolean scalar ConfigValue.
    ///
    /// This is a convenience method that wraps a bool in `Yaml::Boolean`.
    /// Use this when you need to create a boolean value without importing yaml_rust2.
    pub fn new_bool(b: bool, source_info: SourceInfo) -> Self {
        Self {
            value: ConfigValueKind::Scalar(Yaml::Boolean(b)),
            source_info,
            merge_op: MergeOp::Concat,
        }
    }

    /// Create a new array ConfigValue with default merge semantics.
    pub fn new_array(items: Vec<ConfigValue>, source_info: SourceInfo) -> Self {
        Self {
            value: ConfigValueKind::Array(items),
            source_info,
            merge_op: MergeOp::Concat,
        }
    }

    /// Create a new map ConfigValue with default merge semantics.
    pub fn new_map(entries: Vec<ConfigMapEntry>, source_info: SourceInfo) -> Self {
        Self {
            value: ConfigValueKind::Map(entries),
            source_info,
            merge_op: MergeOp::Concat,
        }
    }

    /// Create a null ConfigValue.
    pub fn null(source_info: SourceInfo) -> Self {
        Self {
            value: ConfigValueKind::Scalar(Yaml::Null),
            source_info,
            merge_op: MergeOp::Concat,
        }
    }

    /// Create a ConfigValue with Pandoc inlines (defaults to prefer semantics).
    pub fn new_inlines(inlines: Inlines, source_info: SourceInfo) -> Self {
        Self {
            value: ConfigValueKind::PandocInlines(inlines),
            source_info,
            merge_op: MergeOp::Prefer, // Default for markdown content
        }
    }

    /// Create a ConfigValue with Pandoc blocks (defaults to prefer semantics).
    pub fn new_blocks(blocks: Blocks, source_info: SourceInfo) -> Self {
        Self {
            value: ConfigValueKind::PandocBlocks(blocks),
            source_info,
            merge_op: MergeOp::Prefer, // Default for markdown content
        }
    }

    /// Create a ConfigValue for a path (`!path` tag).
    pub fn new_path(path: String, source_info: SourceInfo) -> Self {
        Self {
            value: ConfigValueKind::Path(path),
            source_info,
            merge_op: MergeOp::Concat,
        }
    }

    /// Create a ConfigValue for a glob pattern (`!glob` tag).
    pub fn new_glob(pattern: String, source_info: SourceInfo) -> Self {
        Self {
            value: ConfigValueKind::Glob(pattern),
            source_info,
            merge_op: MergeOp::Concat,
        }
    }

    /// Create a ConfigValue for an expression (`!expr` tag).
    pub fn new_expr(expr: String, source_info: SourceInfo) -> Self {
        Self {
            value: ConfigValueKind::Expr(expr),
            source_info,
            merge_op: MergeOp::Concat,
        }
    }

    /// Create a nested map structure from a path and string value.
    ///
    /// This is useful for programmatically creating configuration, e.g., in WASM
    /// to inject settings without parsing YAML.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let config = ConfigValue::from_path(&["format", "html", "source-location"], "full");
    /// // Creates: { format: { html: { source-location: "full" } } }
    /// ```
    pub fn from_path(path: &[&str], value: &str) -> Self {
        let source_info = SourceInfo::default();

        if path.is_empty() {
            return Self::new_string(value, source_info);
        }

        // Start with the leaf value
        let mut result = Self::new_string(value, source_info.clone());

        // Build up the nested map structure from right to left
        for key in path.iter().rev() {
            let entry = ConfigMapEntry {
                key: (*key).to_string(),
                key_source: source_info.clone(),
                value: result,
            };
            result = Self::new_map(vec![entry], source_info.clone());
        }

        result
    }

    /// Set the merge operation.
    pub fn with_merge_op(mut self, merge_op: MergeOp) -> Self {
        self.merge_op = merge_op;
        self
    }

    /// Check if this is a scalar value.
    pub fn is_scalar(&self) -> bool {
        matches!(
            self.value,
            ConfigValueKind::Scalar(_)
                | ConfigValueKind::PandocInlines(_)
                | ConfigValueKind::PandocBlocks(_)
                | ConfigValueKind::Path(_)
                | ConfigValueKind::Glob(_)
                | ConfigValueKind::Expr(_)
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

    /// Get as a Yaml scalar if this is a Scalar.
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
    pub fn as_map_entries(&self) -> Option<&[ConfigMapEntry]> {
        match &self.value {
            ConfigValueKind::Map(entries) => Some(entries),
            _ => None,
        }
    }

    /// Get a value by key if this is a Map.
    pub fn get(&self, key: &str) -> Option<&ConfigValue> {
        match &self.value {
            ConfigValueKind::Map(entries) => {
                entries.iter().find(|e| e.key == key).map(|e| &e.value)
            }
            _ => None,
        }
    }

    /// Check if a key exists if this is a Map.
    pub fn contains_key(&self, key: &str) -> bool {
        self.get(key).is_some()
    }

    /// Check if this Map is empty.
    pub fn is_empty(&self) -> bool {
        match &self.value {
            ConfigValueKind::Map(entries) => entries.is_empty(),
            ConfigValueKind::Array(items) => items.is_empty(),
            _ => false,
        }
    }

    /// Get the raw string value if this is any string-like variant.
    ///
    /// Works for Scalar(String), Path, Glob, and Expr.
    pub fn as_str(&self) -> Option<&str> {
        match &self.value {
            ConfigValueKind::Scalar(Yaml::String(s)) => Some(s),
            ConfigValueKind::Path(s) => Some(s),
            ConfigValueKind::Glob(s) => Some(s),
            ConfigValueKind::Expr(s) => Some(s),
            _ => None,
        }
    }

    /// Get the boolean value if this is a boolean scalar.
    pub fn as_bool(&self) -> Option<bool> {
        match &self.value {
            ConfigValueKind::Scalar(Yaml::Boolean(b)) => Some(*b),
            _ => None,
        }
    }

    /// Get the integer value if this is an integer scalar.
    pub fn as_int(&self) -> Option<i64> {
        match &self.value {
            ConfigValueKind::Scalar(Yaml::Integer(i)) => Some(*i),
            _ => None,
        }
    }

    /// Check if this is a null/empty value.
    pub fn is_null(&self) -> bool {
        matches!(&self.value, ConfigValueKind::Scalar(Yaml::Null))
    }

    /// Get a value by path (e.g., `["format", "html", "toc"]`).
    ///
    /// Navigates through nested maps to find the value at the given path.
    /// Returns `None` if any key in the path doesn't exist or if
    /// an intermediate value is not a Map.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let config = ...; // { format: { html: { toc: true } } }
    /// let toc = config.get_path(&["format", "html", "toc"]);
    /// assert_eq!(toc.unwrap().as_bool(), Some(true));
    /// ```
    pub fn get_path(&self, path: &[&str]) -> Option<&ConfigValue> {
        let mut current = self;
        for key in path {
            current = current.get(key)?;
        }
        Some(current)
    }

    /// Check if a path exists in the nested structure.
    ///
    /// Returns `true` if the path can be fully traversed through nested maps.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let config = ...; // { format: { html: { toc: true } } }
    /// assert!(config.contains_path(&["format", "html", "toc"]));
    /// assert!(!config.contains_path(&["format", "html", "sidebar"]));
    /// ```
    pub fn contains_path(&self, path: &[&str]) -> bool {
        self.get_path(path).is_some()
    }

    /// Get a mutable reference to a value by path.
    ///
    /// Navigates through nested maps to find the value at the given path.
    /// Returns `None` if any key in the path doesn't exist or if
    /// an intermediate value is not a Map.
    pub fn get_path_mut(&mut self, path: &[&str]) -> Option<&mut ConfigValue> {
        let mut current = self;
        for key in path {
            current = current.get_mut(key)?;
        }
        Some(current)
    }

    /// Get a mutable reference to a value by key if this is a Map.
    pub fn get_mut(&mut self, key: &str) -> Option<&mut ConfigValue> {
        match &mut self.value {
            ConfigValueKind::Map(entries) => entries
                .iter_mut()
                .find(|e| e.key == key)
                .map(|e| &mut e.value),
            _ => None,
        }
    }

    /// Insert a value at a path, creating intermediate maps as needed.
    ///
    /// This will create any missing intermediate map entries along the path.
    /// If an intermediate value exists but is not a Map, it will be replaced
    /// with a Map.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let mut config = ConfigValue::default();
    /// config.insert_path(
    ///     &["navigation", "toc", "title"],
    ///     ConfigValue::new_string("Contents", SourceInfo::default()),
    /// );
    /// // config is now: { navigation: { toc: { title: "Contents" } } }
    /// ```
    pub fn insert_path(&mut self, path: &[&str], value: ConfigValue) {
        if path.is_empty() {
            // Replace self with value
            *self = value;
            return;
        }

        let first_key = path[0];
        let rest = &path[1..];

        // Ensure self is a Map
        if !self.is_map() {
            *self = ConfigValue::new_map(vec![], self.source_info.clone());
        }

        if let ConfigValueKind::Map(entries) = &mut self.value {
            if rest.is_empty() {
                // This is the final key - insert or update
                if let Some(entry) = entries.iter_mut().find(|e| e.key == first_key) {
                    entry.value = value;
                } else {
                    entries.push(ConfigMapEntry {
                        key: first_key.to_string(),
                        key_source: value.source_info.clone(),
                        value,
                    });
                }
            } else {
                // Need to recurse
                if let Some(entry) = entries.iter_mut().find(|e| e.key == first_key) {
                    // Entry exists - recurse into it
                    entry.value.insert_path(rest, value);
                } else {
                    // Entry doesn't exist - create an empty map and recurse
                    let mut new_map = ConfigValue::new_map(vec![], SourceInfo::default());
                    new_map.insert_path(rest, value);
                    entries.push(ConfigMapEntry {
                        key: first_key.to_string(),
                        key_source: SourceInfo::default(),
                        value: new_map,
                    });
                }
            }
        }
    }

    /// Check if this ConfigValue represents a string with a specific value.
    ///
    /// This handles:
    /// - `Scalar(Yaml::String(s))` where s == expected
    /// - `Path(s)`, `Glob(s)`, `Expr(s)` where s == expected
    /// - `PandocInlines` with a single Str inline where text == expected
    ///
    /// This is needed because YAML strings may be parsed as markdown
    /// and become PandocInlines containing a single Str node.
    pub fn is_string_value(&self, expected: &str) -> bool {
        match &self.value {
            ConfigValueKind::Scalar(Yaml::String(s)) => s == expected,
            ConfigValueKind::Path(s) => s == expected,
            ConfigValueKind::Glob(s) => s == expected,
            ConfigValueKind::Expr(s) => s == expected,
            ConfigValueKind::PandocInlines(inlines) if inlines.len() == 1 => {
                if let Inline::Str(str_node) = &inlines[0] {
                    return str_node.text == expected;
                }
                false
            }
            _ => false,
        }
    }
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

    #[test]
    fn test_path_variant() {
        let value = ConfigValue::new_path("./data/file.csv".to_string(), SourceInfo::default());
        assert!(value.is_scalar()); // Path is considered scalar-like
        assert_eq!(value.as_str(), Some("./data/file.csv"));
    }

    #[test]
    fn test_glob_variant() {
        let value = ConfigValue::new_glob("*.qmd".to_string(), SourceInfo::default());
        assert!(value.is_scalar());
        assert_eq!(value.as_str(), Some("*.qmd"));
    }

    #[test]
    fn test_expr_variant() {
        let value = ConfigValue::new_expr("params$threshold".to_string(), SourceInfo::default());
        assert!(value.is_scalar());
        assert_eq!(value.as_str(), Some("params$threshold"));
    }

    #[test]
    fn test_map_get() {
        let entries = vec![
            ConfigMapEntry {
                key: "foo".to_string(),
                key_source: SourceInfo::default(),
                value: ConfigValue::new_scalar(Yaml::String("bar".into()), SourceInfo::default()),
            },
            ConfigMapEntry {
                key: "baz".to_string(),
                key_source: SourceInfo::default(),
                value: ConfigValue::new_scalar(Yaml::Integer(42), SourceInfo::default()),
            },
        ];
        let map = ConfigValue::new_map(entries, SourceInfo::default());

        assert!(map.contains_key("foo"));
        assert!(map.contains_key("baz"));
        assert!(!map.contains_key("qux"));

        let foo = map.get("foo").unwrap();
        assert_eq!(foo.as_yaml().unwrap().as_str(), Some("bar"));
    }

    #[test]
    fn test_is_string_value() {
        let scalar = ConfigValue::new_scalar(Yaml::String("hello".into()), SourceInfo::default());
        assert!(scalar.is_string_value("hello"));
        assert!(!scalar.is_string_value("world"));

        let path = ConfigValue::new_path("./file.txt".to_string(), SourceInfo::default());
        assert!(path.is_string_value("./file.txt"));
    }

    // === Tests for constructor methods ===

    #[test]
    fn test_new_string() {
        let value = ConfigValue::new_string("hello", SourceInfo::default());
        assert!(value.is_scalar());
        assert_eq!(value.as_str(), Some("hello"));
        assert_eq!(value.merge_op, MergeOp::Concat);
    }

    #[test]
    fn test_new_bool() {
        let value_true = ConfigValue::new_bool(true, SourceInfo::default());
        assert!(value_true.is_scalar());
        assert_eq!(value_true.as_bool(), Some(true));

        let value_false = ConfigValue::new_bool(false, SourceInfo::default());
        assert_eq!(value_false.as_bool(), Some(false));
    }

    #[test]
    fn test_null() {
        let value = ConfigValue::null(SourceInfo::default());
        assert!(value.is_scalar());
        assert!(value.is_null());
        assert_eq!(value.merge_op, MergeOp::Concat);
    }

    #[test]
    fn test_default() {
        let value = ConfigValue::default();
        assert!(value.is_map());
        assert!(value.is_empty());
        assert_eq!(value.merge_op, MergeOp::Concat);
    }

    // === Tests for accessor methods ===

    #[test]
    fn test_as_bool_non_bool() {
        // Test that as_bool returns None for non-boolean values
        let string_val = ConfigValue::new_string("true", SourceInfo::default());
        assert_eq!(string_val.as_bool(), None);

        let int_val = ConfigValue::new_scalar(Yaml::Integer(1), SourceInfo::default());
        assert_eq!(int_val.as_bool(), None);
    }

    #[test]
    fn test_as_int() {
        let value = ConfigValue::new_scalar(Yaml::Integer(42), SourceInfo::default());
        assert_eq!(value.as_int(), Some(42));

        let value_neg = ConfigValue::new_scalar(Yaml::Integer(-100), SourceInfo::default());
        assert_eq!(value_neg.as_int(), Some(-100));
    }

    #[test]
    fn test_as_int_non_int() {
        // Test that as_int returns None for non-integer values
        let string_val = ConfigValue::new_string("42", SourceInfo::default());
        assert_eq!(string_val.as_int(), None);

        let bool_val = ConfigValue::new_bool(true, SourceInfo::default());
        assert_eq!(bool_val.as_int(), None);
    }

    #[test]
    fn test_is_null() {
        let null_val = ConfigValue::null(SourceInfo::default());
        assert!(null_val.is_null());

        let string_val = ConfigValue::new_string("", SourceInfo::default());
        assert!(!string_val.is_null());

        let int_val = ConfigValue::new_scalar(Yaml::Integer(0), SourceInfo::default());
        assert!(!int_val.is_null());
    }

    #[test]
    fn test_is_empty() {
        // Empty map
        let empty_map = ConfigValue::new_map(vec![], SourceInfo::default());
        assert!(empty_map.is_empty());

        // Non-empty map
        let non_empty_map = ConfigValue::new_map(
            vec![ConfigMapEntry {
                key: "k".to_string(),
                key_source: SourceInfo::default(),
                value: ConfigValue::null(SourceInfo::default()),
            }],
            SourceInfo::default(),
        );
        assert!(!non_empty_map.is_empty());

        // Empty array
        let empty_arr = ConfigValue::new_array(vec![], SourceInfo::default());
        assert!(empty_arr.is_empty());

        // Non-empty array
        let non_empty_arr = ConfigValue::new_array(
            vec![ConfigValue::null(SourceInfo::default())],
            SourceInfo::default(),
        );
        assert!(!non_empty_arr.is_empty());

        // Scalar is not empty (by definition)
        let scalar = ConfigValue::new_string("", SourceInfo::default());
        assert!(!scalar.is_empty());
    }

    #[test]
    fn test_as_yaml() {
        let scalar = ConfigValue::new_scalar(Yaml::Integer(123), SourceInfo::default());
        let yaml = scalar.as_yaml().unwrap();
        assert_eq!(yaml.as_i64(), Some(123));

        // Non-scalar returns None
        let array = ConfigValue::new_array(vec![], SourceInfo::default());
        assert!(array.as_yaml().is_none());
    }

    #[test]
    fn test_as_array_non_array() {
        let scalar = ConfigValue::new_string("test", SourceInfo::default());
        assert!(scalar.as_array().is_none());

        let map = ConfigValue::new_map(vec![], SourceInfo::default());
        assert!(map.as_array().is_none());
    }

    #[test]
    fn test_as_map_entries_non_map() {
        let scalar = ConfigValue::new_string("test", SourceInfo::default());
        assert!(scalar.as_map_entries().is_none());

        let array = ConfigValue::new_array(vec![], SourceInfo::default());
        assert!(array.as_map_entries().is_none());
    }

    #[test]
    fn test_get_non_map() {
        let scalar = ConfigValue::new_string("test", SourceInfo::default());
        assert!(scalar.get("key").is_none());
    }

    // === Tests for from_path ===

    #[test]
    fn test_from_path_empty() {
        // Empty path returns the value directly as a string
        let value = ConfigValue::from_path(&[], "hello");
        assert!(value.is_scalar());
        assert_eq!(value.as_str(), Some("hello"));
    }

    #[test]
    fn test_from_path_single_key() {
        let value = ConfigValue::from_path(&["title"], "My Title");
        assert!(value.is_map());

        let entries = value.as_map_entries().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].key, "title");
        assert_eq!(entries[0].value.as_str(), Some("My Title"));
    }

    #[test]
    fn test_from_path_nested() {
        let value = ConfigValue::from_path(&["format", "html", "source-location"], "full");
        assert!(value.is_map());

        // Navigate to format
        let format = value.get("format").unwrap();
        assert!(format.is_map());

        // Navigate to html
        let html = format.get("html").unwrap();
        assert!(html.is_map());

        // Navigate to source-location
        let source_loc = html.get("source-location").unwrap();
        assert_eq!(source_loc.as_str(), Some("full"));
    }

    // === Tests for is_string_value edge cases ===

    #[test]
    fn test_is_string_value_glob() {
        let glob = ConfigValue::new_glob("*.md".to_string(), SourceInfo::default());
        assert!(glob.is_string_value("*.md"));
        assert!(!glob.is_string_value("*.txt"));
    }

    #[test]
    fn test_is_string_value_expr() {
        let expr = ConfigValue::new_expr("params$x".to_string(), SourceInfo::default());
        assert!(expr.is_string_value("params$x"));
        assert!(!expr.is_string_value("params$y"));
    }

    #[test]
    fn test_is_string_value_pandoc_inlines_single_str() {
        use crate::inline::{Inline, Str};

        let str_node = Inline::Str(Str {
            text: "hello".to_string(),
            source_info: SourceInfo::default(),
        });
        let value = ConfigValue::new_inlines(vec![str_node], SourceInfo::default());

        assert!(value.is_string_value("hello"));
        assert!(!value.is_string_value("world"));
    }

    #[test]
    fn test_is_string_value_pandoc_inlines_multiple() {
        use crate::inline::{Inline, Str};

        let str1 = Inline::Str(Str {
            text: "hello".to_string(),
            source_info: SourceInfo::default(),
        });
        let str2 = Inline::Str(Str {
            text: " world".to_string(),
            source_info: SourceInfo::default(),
        });
        let value = ConfigValue::new_inlines(vec![str1, str2], SourceInfo::default());

        // Multiple inlines should not match
        assert!(!value.is_string_value("hello world"));
        assert!(!value.is_string_value("hello"));
    }

    #[test]
    fn test_is_string_value_pandoc_inlines_non_str() {
        use crate::inline::{Inline, Space};

        let space = Inline::Space(Space {
            source_info: SourceInfo::default(),
        });
        let value = ConfigValue::new_inlines(vec![space], SourceInfo::default());

        // Non-Str inline should not match
        assert!(!value.is_string_value(" "));
        assert!(!value.is_string_value(""));
    }

    #[test]
    fn test_is_string_value_non_string_scalar() {
        let int_val = ConfigValue::new_scalar(Yaml::Integer(42), SourceInfo::default());
        assert!(!int_val.is_string_value("42"));

        let bool_val = ConfigValue::new_bool(true, SourceInfo::default());
        assert!(!bool_val.is_string_value("true"));

        let null_val = ConfigValue::null(SourceInfo::default());
        assert!(!null_val.is_string_value(""));
        assert!(!null_val.is_string_value("null"));
    }

    // === Tests for serialization/deserialization ===

    #[test]
    fn test_yaml_to_serde_value_string() {
        let yaml = Yaml::String("test".to_string());
        let json = super::yaml_to_serde_value(&yaml);
        assert_eq!(json, serde_json::Value::String("test".to_string()));
    }

    #[test]
    fn test_yaml_to_serde_value_integer() {
        let yaml = Yaml::Integer(42);
        let json = super::yaml_to_serde_value(&yaml);
        assert_eq!(json, serde_json::json!(42));
    }

    #[test]
    fn test_yaml_to_serde_value_real() {
        let yaml = Yaml::Real("2.5".to_string());
        let json = super::yaml_to_serde_value(&yaml);
        assert_eq!(json, serde_json::json!(2.5));
    }

    #[test]
    fn test_yaml_to_serde_value_real_invalid() {
        // Invalid real falls back to string
        let yaml = Yaml::Real("not_a_number".to_string());
        let json = super::yaml_to_serde_value(&yaml);
        assert_eq!(json, serde_json::Value::String("not_a_number".to_string()));
    }

    #[test]
    fn test_yaml_to_serde_value_boolean() {
        let yaml_true = Yaml::Boolean(true);
        assert_eq!(
            super::yaml_to_serde_value(&yaml_true),
            serde_json::json!(true)
        );

        let yaml_false = Yaml::Boolean(false);
        assert_eq!(
            super::yaml_to_serde_value(&yaml_false),
            serde_json::json!(false)
        );
    }

    #[test]
    fn test_yaml_to_serde_value_null() {
        let yaml = Yaml::Null;
        assert_eq!(super::yaml_to_serde_value(&yaml), serde_json::Value::Null);
    }

    #[test]
    fn test_yaml_to_serde_value_array() {
        let yaml = Yaml::Array(vec![Yaml::Integer(1), Yaml::String("two".to_string())]);
        let json = super::yaml_to_serde_value(&yaml);
        assert_eq!(json, serde_json::json!([1, "two"]));
    }

    #[test]
    fn test_yaml_to_serde_value_hash() {
        let mut hash = yaml_rust2::yaml::Hash::new();
        hash.insert(Yaml::String("key".to_string()), Yaml::Integer(123));
        let yaml = Yaml::Hash(hash);
        let json = super::yaml_to_serde_value(&yaml);
        assert_eq!(json, serde_json::json!({"key": 123}));
    }

    #[test]
    fn test_yaml_to_serde_value_alias_and_bad() {
        // Alias and BadValue become null
        let alias = Yaml::Alias(0);
        assert_eq!(super::yaml_to_serde_value(&alias), serde_json::Value::Null);

        let bad = Yaml::BadValue;
        assert_eq!(super::yaml_to_serde_value(&bad), serde_json::Value::Null);
    }

    #[test]
    fn test_serde_value_to_yaml_string() {
        let json = serde_json::Value::String("test".to_string());
        let yaml = super::serde_value_to_yaml(&json);
        assert_eq!(yaml, Yaml::String("test".to_string()));
    }

    #[test]
    fn test_serde_value_to_yaml_integer() {
        let json = serde_json::json!(42);
        let yaml = super::serde_value_to_yaml(&json);
        assert_eq!(yaml, Yaml::Integer(42));
    }

    #[test]
    fn test_serde_value_to_yaml_float() {
        let json = serde_json::json!(2.5);
        let yaml = super::serde_value_to_yaml(&json);
        // Floats become Real
        if let Yaml::Real(s) = yaml {
            assert!(s.contains("2.5"));
        } else {
            panic!("Expected Yaml::Real");
        }
    }

    #[test]
    fn test_serde_value_to_yaml_bool() {
        let json_true = serde_json::json!(true);
        assert_eq!(super::serde_value_to_yaml(&json_true), Yaml::Boolean(true));

        let json_false = serde_json::json!(false);
        assert_eq!(
            super::serde_value_to_yaml(&json_false),
            Yaml::Boolean(false)
        );
    }

    #[test]
    fn test_serde_value_to_yaml_null() {
        let json = serde_json::Value::Null;
        assert_eq!(super::serde_value_to_yaml(&json), Yaml::Null);
    }

    #[test]
    fn test_serde_value_to_yaml_array() {
        let json = serde_json::json!([1, "two"]);
        let yaml = super::serde_value_to_yaml(&json);
        if let Yaml::Array(arr) = yaml {
            assert_eq!(arr.len(), 2);
            assert_eq!(arr[0], Yaml::Integer(1));
            assert_eq!(arr[1], Yaml::String("two".to_string()));
        } else {
            panic!("Expected Yaml::Array");
        }
    }

    #[test]
    fn test_serde_value_to_yaml_object() {
        let json = serde_json::json!({"key": 123});
        let yaml = super::serde_value_to_yaml(&json);
        if let Yaml::Hash(hash) = yaml {
            assert_eq!(hash.len(), 1);
            assert_eq!(
                hash.get(&Yaml::String("key".to_string())),
                Some(&Yaml::Integer(123))
            );
        } else {
            panic!("Expected Yaml::Hash");
        }
    }

    // === Tests for ConfigValueKind serialization round-trip ===

    #[test]
    fn test_config_value_kind_serialize_scalar_string() {
        let kind = ConfigValueKind::Scalar(Yaml::String("hello".to_string()));
        let json = serde_json::to_string(&kind).unwrap();
        let deserialized: ConfigValueKind = serde_json::from_str(&json).unwrap();
        assert_eq!(kind, deserialized);
    }

    #[test]
    fn test_config_value_kind_serialize_scalar_int() {
        let kind = ConfigValueKind::Scalar(Yaml::Integer(42));
        let json = serde_json::to_string(&kind).unwrap();
        let deserialized: ConfigValueKind = serde_json::from_str(&json).unwrap();
        assert_eq!(kind, deserialized);
    }

    #[test]
    fn test_config_value_kind_serialize_path() {
        let kind = ConfigValueKind::Path("./data.csv".to_string());
        let json = serde_json::to_string(&kind).unwrap();
        let deserialized: ConfigValueKind = serde_json::from_str(&json).unwrap();
        assert_eq!(kind, deserialized);
    }

    #[test]
    fn test_config_value_kind_serialize_glob() {
        let kind = ConfigValueKind::Glob("*.qmd".to_string());
        let json = serde_json::to_string(&kind).unwrap();
        let deserialized: ConfigValueKind = serde_json::from_str(&json).unwrap();
        assert_eq!(kind, deserialized);
    }

    #[test]
    fn test_config_value_kind_serialize_expr() {
        let kind = ConfigValueKind::Expr("params$x".to_string());
        let json = serde_json::to_string(&kind).unwrap();
        let deserialized: ConfigValueKind = serde_json::from_str(&json).unwrap();
        assert_eq!(kind, deserialized);
    }

    #[test]
    fn test_config_value_kind_serialize_array() {
        let kind = ConfigValueKind::Array(vec![
            ConfigValue::new_string("a", SourceInfo::default()),
            ConfigValue::new_string("b", SourceInfo::default()),
        ]);
        let json = serde_json::to_string(&kind).unwrap();
        let deserialized: ConfigValueKind = serde_json::from_str(&json).unwrap();
        assert_eq!(kind, deserialized);
    }

    #[test]
    fn test_config_value_kind_serialize_map() {
        let kind = ConfigValueKind::Map(vec![ConfigMapEntry {
            key: "key".to_string(),
            key_source: SourceInfo::default(),
            value: ConfigValue::new_string("value", SourceInfo::default()),
        }]);
        let json = serde_json::to_string(&kind).unwrap();
        let deserialized: ConfigValueKind = serde_json::from_str(&json).unwrap();
        assert_eq!(kind, deserialized);
    }

    #[test]
    fn test_config_value_kind_deserialize_unknown_variant() {
        let json = r#"{"UnknownVariant": "value"}"#;
        let result: Result<ConfigValueKind, _> = serde_json::from_str(json);
        assert!(result.is_err());
    }

    #[test]
    fn test_config_value_kind_deserialize_empty_map() {
        let json = r#"{}"#;
        let result: Result<ConfigValueKind, _> = serde_json::from_str(json);
        assert!(result.is_err());
    }

    // === Full ConfigValue serialization ===

    #[test]
    fn test_config_value_serialize_roundtrip() {
        let value =
            ConfigValue::new_string("test", SourceInfo::default()).with_merge_op(MergeOp::Prefer);

        let json = serde_json::to_string(&value).unwrap();
        let deserialized: ConfigValue = serde_json::from_str(&json).unwrap();

        assert_eq!(value.merge_op, deserialized.merge_op);
        assert_eq!(value.as_str(), deserialized.as_str());
    }

    #[test]
    fn test_config_map_entry_serialize_roundtrip() {
        let entry = ConfigMapEntry {
            key: "test_key".to_string(),
            key_source: SourceInfo::default(),
            value: ConfigValue::new_bool(true, SourceInfo::default()),
        };

        let json = serde_json::to_string(&entry).unwrap();
        let deserialized: ConfigMapEntry = serde_json::from_str(&json).unwrap();

        assert_eq!(entry.key, deserialized.key);
        assert_eq!(entry.value.as_bool(), deserialized.value.as_bool());
    }

    // === Additional edge cases ===

    #[test]
    fn test_as_str_returns_none_for_non_strings() {
        let int_val = ConfigValue::new_scalar(Yaml::Integer(42), SourceInfo::default());
        assert!(int_val.as_str().is_none());

        let bool_val = ConfigValue::new_bool(true, SourceInfo::default());
        assert!(bool_val.as_str().is_none());

        let null_val = ConfigValue::null(SourceInfo::default());
        assert!(null_val.as_str().is_none());

        let array_val = ConfigValue::new_array(vec![], SourceInfo::default());
        assert!(array_val.as_str().is_none());

        let map_val = ConfigValue::new_map(vec![], SourceInfo::default());
        assert!(map_val.as_str().is_none());
    }

    #[test]
    fn test_is_scalar_for_all_scalar_types() {
        // All these should be considered scalar
        assert!(ConfigValue::new_string("s", SourceInfo::default()).is_scalar());
        assert!(ConfigValue::new_bool(true, SourceInfo::default()).is_scalar());
        assert!(ConfigValue::new_scalar(Yaml::Integer(1), SourceInfo::default()).is_scalar());
        assert!(ConfigValue::null(SourceInfo::default()).is_scalar());
        assert!(ConfigValue::new_path("p".to_string(), SourceInfo::default()).is_scalar());
        assert!(ConfigValue::new_glob("g".to_string(), SourceInfo::default()).is_scalar());
        assert!(ConfigValue::new_expr("e".to_string(), SourceInfo::default()).is_scalar());
        assert!(ConfigValue::new_inlines(vec![], SourceInfo::default()).is_scalar());
        assert!(ConfigValue::new_blocks(vec![], SourceInfo::default()).is_scalar());

        // These should not be scalar
        assert!(!ConfigValue::new_array(vec![], SourceInfo::default()).is_scalar());
        assert!(!ConfigValue::new_map(vec![], SourceInfo::default()).is_scalar());
    }

    // === Tests for path-based navigation methods ===

    #[test]
    fn test_get_path_empty() {
        let value = ConfigValue::new_string("hello", SourceInfo::default());
        // Empty path returns self
        let result = value.get_path(&[]);
        assert_eq!(result.unwrap().as_str(), Some("hello"));
    }

    #[test]
    fn test_get_path_single_key() {
        let value = ConfigValue::from_path(&["title"], "My Title");
        let result = value.get_path(&["title"]);
        assert!(result.is_some());
        assert_eq!(result.unwrap().as_str(), Some("My Title"));
    }

    #[test]
    fn test_get_path_nested() {
        let value = ConfigValue::from_path(&["format", "html", "toc"], "true");

        // Full path
        let result = value.get_path(&["format", "html", "toc"]);
        assert!(result.is_some());
        assert_eq!(result.unwrap().as_str(), Some("true"));

        // Partial path
        let result = value.get_path(&["format", "html"]);
        assert!(result.is_some());
        assert!(result.unwrap().is_map());

        // Root path
        let result = value.get_path(&["format"]);
        assert!(result.is_some());
        assert!(result.unwrap().is_map());
    }

    #[test]
    fn test_get_path_missing_key() {
        let value = ConfigValue::from_path(&["format", "html"], "test");

        // Missing intermediate key
        assert!(value.get_path(&["format", "pdf"]).is_none());

        // Missing root key
        assert!(value.get_path(&["other"]).is_none());

        // Path too deep
        assert!(value.get_path(&["format", "html", "extra"]).is_none());
    }

    #[test]
    fn test_get_path_non_map_intermediate() {
        // Create a structure where an intermediate value is not a map
        let value = ConfigValue::from_path(&["format"], "not-a-map");

        // Can't traverse through a non-map
        assert!(value.get_path(&["format", "html"]).is_none());
    }

    #[test]
    fn test_contains_path() {
        let value = ConfigValue::from_path(&["format", "html", "toc"], "true");

        assert!(value.contains_path(&["format"]));
        assert!(value.contains_path(&["format", "html"]));
        assert!(value.contains_path(&["format", "html", "toc"]));
        assert!(!value.contains_path(&["format", "pdf"]));
        assert!(!value.contains_path(&["other"]));
    }

    #[test]
    fn test_contains_path_empty() {
        let value = ConfigValue::new_string("hello", SourceInfo::default());
        // Empty path always exists (returns self)
        assert!(value.contains_path(&[]));
    }

    #[test]
    fn test_insert_path_empty() {
        let mut value = ConfigValue::new_string("old", SourceInfo::default());
        value.insert_path(&[], ConfigValue::new_string("new", SourceInfo::default()));
        assert_eq!(value.as_str(), Some("new"));
    }

    #[test]
    fn test_insert_path_single_key() {
        let mut value = ConfigValue::default(); // Empty map
        value.insert_path(
            &["title"],
            ConfigValue::new_string("My Title", SourceInfo::default()),
        );

        assert!(value.contains_path(&["title"]));
        assert_eq!(
            value.get_path(&["title"]).unwrap().as_str(),
            Some("My Title")
        );
    }

    #[test]
    fn test_insert_path_nested_creates_intermediates() {
        let mut value = ConfigValue::default(); // Empty map
        value.insert_path(
            &["navigation", "toc", "title"],
            ConfigValue::new_string("Contents", SourceInfo::default()),
        );

        // Verify intermediate maps were created
        assert!(value.contains_path(&["navigation"]));
        assert!(value.contains_path(&["navigation", "toc"]));
        assert!(value.contains_path(&["navigation", "toc", "title"]));
        assert_eq!(
            value
                .get_path(&["navigation", "toc", "title"])
                .unwrap()
                .as_str(),
            Some("Contents")
        );
    }

    #[test]
    fn test_insert_path_overwrites_existing() {
        let mut value = ConfigValue::from_path(&["format", "html", "toc"], "old");

        value.insert_path(
            &["format", "html", "toc"],
            ConfigValue::new_string("new", SourceInfo::default()),
        );

        assert_eq!(
            value.get_path(&["format", "html", "toc"]).unwrap().as_str(),
            Some("new")
        );
    }

    #[test]
    fn test_insert_path_adds_sibling() {
        let mut value = ConfigValue::from_path(&["format", "html", "toc"], "true");

        value.insert_path(
            &["format", "html", "toc-depth"],
            ConfigValue::new_scalar(Yaml::Integer(3), SourceInfo::default()),
        );

        // Original still exists
        assert!(value.contains_path(&["format", "html", "toc"]));
        assert_eq!(
            value.get_path(&["format", "html", "toc"]).unwrap().as_str(),
            Some("true")
        );

        // New sibling exists
        assert!(value.contains_path(&["format", "html", "toc-depth"]));
        assert_eq!(
            value
                .get_path(&["format", "html", "toc-depth"])
                .unwrap()
                .as_int(),
            Some(3)
        );
    }

    #[test]
    fn test_insert_path_replaces_non_map_intermediate() {
        let mut value = ConfigValue::from_path(&["format"], "scalar-value");

        // Insert at a path that goes through the scalar - it should replace it with a map
        value.insert_path(
            &["format", "html", "toc"],
            ConfigValue::new_bool(true, SourceInfo::default()),
        );

        // format is now a map, not a scalar
        assert!(value.get_path(&["format"]).unwrap().is_map());
        assert!(value.contains_path(&["format", "html", "toc"]));
        assert_eq!(
            value
                .get_path(&["format", "html", "toc"])
                .unwrap()
                .as_bool(),
            Some(true)
        );
    }

    #[test]
    fn test_get_mut() {
        let mut value = ConfigValue::from_path(&["title"], "Old Title");

        if let Some(title) = value.get_mut("title") {
            *title = ConfigValue::new_string("New Title", SourceInfo::default());
        }

        assert_eq!(
            value.get_path(&["title"]).unwrap().as_str(),
            Some("New Title")
        );
    }

    #[test]
    fn test_get_mut_non_map() {
        let mut value = ConfigValue::new_string("hello", SourceInfo::default());
        assert!(value.get_mut("key").is_none());
    }

    #[test]
    fn test_get_path_mut() {
        let mut value = ConfigValue::from_path(&["format", "html", "toc"], "false");

        if let Some(toc) = value.get_path_mut(&["format", "html", "toc"]) {
            *toc = ConfigValue::new_bool(true, SourceInfo::default());
        }

        assert_eq!(
            value
                .get_path(&["format", "html", "toc"])
                .unwrap()
                .as_bool(),
            Some(true)
        );
    }

    #[test]
    fn test_get_path_mut_missing() {
        let mut value = ConfigValue::from_path(&["format", "html"], "test");
        assert!(value.get_path_mut(&["format", "pdf"]).is_none());
    }
}
