//! YAML value with source location tracking.

use crate::SourceInfo;
use yaml_rust2::Yaml;

/// A YAML value with source location information.
///
/// This structure wraps a `yaml-rust2::Yaml` value with source location tracking
/// for the value itself and all its children. Uses the **owned data approach**:
/// stores an owned `Yaml` value with a parallel `Children` structure for source
/// tracking.
///
/// ## Design Trade-offs
///
/// - **Memory**: ~3x overhead (owned Yaml + source-tracked children)
/// - **Simplicity**: No lifetime parameters, clean API
/// - **Config merging**: Can merge configs from different lifetimes
/// - **LSP caching**: Can serialize/deserialize for caching
///
/// Follows rust-analyzer's precedent of using owned data for tree structures.
///
/// ## Example
///
/// ```rust,no_run
/// use quarto_yaml::{parse, YamlWithSourceInfo};
/// use yaml_rust2::Yaml;
///
/// let yaml = parse("title: My Document").unwrap();
/// if let Some(title) = yaml.get_hash_value("title") {
///     println!("Title: {:?}", title.yaml);
///     println!("Location: offset {}", title.source_info.range.start.offset);
/// }
/// ```
#[derive(Debug, Clone)]
pub struct YamlWithSourceInfo {
    /// The complete yaml-rust2::Yaml value (owned).
    ///
    /// This provides direct access to the raw Yaml for code that doesn't
    /// need source tracking. It's a complete, independent Yaml tree.
    pub yaml: Yaml,

    /// Source location for this node.
    pub source_info: SourceInfo,

    /// YAML tag information (e.g., !path, !glob, !str).
    ///
    /// If present, contains the tag suffix (e.g., "path" for !path) and
    /// the source location of the tag itself. Used to bypass markdown parsing
    /// for tagged strings and enable error reporting on tags.
    pub tag: Option<(String, SourceInfo)>,

    /// Source-tracked children (parallel structure).
    ///
    /// This mirrors the structure of `yaml` but includes source location
    /// information for each child. The structure matches the `yaml` field:
    /// - None for scalars and Null
    /// - Array for sequences
    /// - Hash for mappings
    children: Children,
}

/// Source-tracked children of a YAML node.
///
/// This is a parallel structure to the children in `Yaml`, providing
/// source location information for each child element.
#[derive(Debug, Clone)]
enum Children {
    /// No children (for scalars, Null, BadValue)
    None,

    /// Array elements with source tracking
    Array(Vec<YamlWithSourceInfo>),

    /// Hash entries with source tracking
    Hash(Vec<YamlHashEntry>),
}

/// A key-value pair in a YAML hash/mapping with source tracking.
///
/// Tracks source locations for the key, value, and the entire entry.
#[derive(Debug, Clone)]
pub struct YamlHashEntry {
    /// The key with source tracking
    pub key: YamlWithSourceInfo,

    /// The value with source tracking
    pub value: YamlWithSourceInfo,

    /// Source location of just the key
    pub key_span: SourceInfo,

    /// Source location of just the value
    pub value_span: SourceInfo,

    /// Source location of the entire entry (key + value)
    pub entry_span: SourceInfo,
}

impl YamlWithSourceInfo {
    /// Create a new YamlWithSourceInfo for a scalar or leaf node.
    pub fn new_scalar(yaml: Yaml, source_info: SourceInfo) -> Self {
        Self {
            yaml,
            source_info,
            tag: None,
            children: Children::None,
        }
    }

    /// Create a new YamlWithSourceInfo for a scalar with tag information.
    pub fn new_scalar_with_tag(
        yaml: Yaml,
        source_info: SourceInfo,
        tag: Option<(String, SourceInfo)>,
    ) -> Self {
        Self {
            yaml,
            source_info,
            tag,
            children: Children::None,
        }
    }

    /// Create a new YamlWithSourceInfo for an array/sequence.
    pub fn new_array(
        yaml: Yaml,
        source_info: SourceInfo,
        children: Vec<YamlWithSourceInfo>,
    ) -> Self {
        Self {
            yaml,
            source_info,
            tag: None,
            children: Children::Array(children),
        }
    }

    /// Create a new YamlWithSourceInfo for a hash/mapping.
    pub fn new_hash(yaml: Yaml, source_info: SourceInfo, entries: Vec<YamlHashEntry>) -> Self {
        Self {
            yaml,
            source_info,
            tag: None,
            children: Children::Hash(entries),
        }
    }

    /// Check if this is a scalar value (not array or hash).
    pub fn is_scalar(&self) -> bool {
        matches!(self.children, Children::None)
    }

    /// Check if this is an array.
    pub fn is_array(&self) -> bool {
        matches!(self.children, Children::Array(_))
    }

    /// Check if this is a hash.
    pub fn is_hash(&self) -> bool {
        matches!(self.children, Children::Hash(_))
    }

    /// Get array children if this is an array.
    pub fn as_array(&self) -> Option<&[YamlWithSourceInfo]> {
        match &self.children {
            Children::Array(items) => Some(items),
            _ => None,
        }
    }

    /// Get hash entries if this is a hash.
    pub fn as_hash(&self) -> Option<&[YamlHashEntry]> {
        match &self.children {
            Children::Hash(entries) => Some(entries),
            _ => None,
        }
    }

    /// Get a value from a hash by key (string comparison).
    ///
    /// This searches through hash entries and compares keys as strings.
    /// Returns None if this is not a hash or the key is not found.
    pub fn get_hash_value(&self, key: &str) -> Option<&YamlWithSourceInfo> {
        match &self.children {
            Children::Hash(entries) => entries.iter().find_map(|entry| {
                if entry.key.yaml.as_str() == Some(key) {
                    Some(&entry.value)
                } else {
                    None
                }
            }),
            _ => None,
        }
    }

    /// Get an array element by index.
    pub fn get_array_item(&self, index: usize) -> Option<&YamlWithSourceInfo> {
        match &self.children {
            Children::Array(items) => items.get(index),
            _ => None,
        }
    }

    /// Get the number of children (array length or hash entry count).
    pub fn len(&self) -> usize {
        match &self.children {
            Children::None => 0,
            Children::Array(items) => items.len(),
            Children::Hash(entries) => entries.len(),
        }
    }

    /// Check if this node has no children.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Consume self and return array children if this is an array.
    ///
    /// Returns a tuple of (items, source_info) where items are the owned
    /// YamlWithSourceInfo elements and source_info is the SourceInfo for
    /// the whole array.
    pub fn into_array(self) -> Option<(Vec<YamlWithSourceInfo>, SourceInfo)> {
        match self.children {
            Children::Array(items) => Some((items, self.source_info)),
            _ => None,
        }
    }

    /// Consume self and return hash entries if this is a hash.
    ///
    /// Returns a tuple of (entries, source_info) where entries are the owned
    /// YamlHashEntry elements and source_info is the SourceInfo for
    /// the whole hash.
    pub fn into_hash(self) -> Option<(Vec<YamlHashEntry>, SourceInfo)> {
        match self.children {
            Children::Hash(entries) => Some((entries, self.source_info)),
            _ => None,
        }
    }
}

impl YamlHashEntry {
    /// Create a new YamlHashEntry.
    pub fn new(
        key: YamlWithSourceInfo,
        value: YamlWithSourceInfo,
        key_span: SourceInfo,
        value_span: SourceInfo,
        entry_span: SourceInfo,
    ) -> Self {
        Self {
            key,
            value,
            key_span,
            value_span,
            entry_span,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scalar_creation() {
        let yaml = Yaml::String("test".into());
        let info = SourceInfo::default();
        let node = YamlWithSourceInfo::new_scalar(yaml.clone(), info.clone());

        assert_eq!(node.yaml, yaml);
        assert_eq!(node.source_info, info);
        assert!(node.is_scalar());
        assert!(!node.is_array());
        assert!(!node.is_hash());
        assert_eq!(node.len(), 0);
    }

    #[test]
    fn test_array_creation() {
        let child1 =
            YamlWithSourceInfo::new_scalar(Yaml::String("a".into()), SourceInfo::default());
        let child2 =
            YamlWithSourceInfo::new_scalar(Yaml::String("b".into()), SourceInfo::default());

        let yaml = Yaml::Array(vec![Yaml::String("a".into()), Yaml::String("b".into())]);
        let node = YamlWithSourceInfo::new_array(yaml, SourceInfo::default(), vec![child1, child2]);

        assert!(node.is_array());
        assert_eq!(node.len(), 2);
        assert!(node.as_array().is_some());
        assert_eq!(node.as_array().unwrap().len(), 2);
    }

    #[test]
    fn test_get_array_item() {
        let child1 =
            YamlWithSourceInfo::new_scalar(Yaml::String("a".into()), SourceInfo::default());
        let child2 =
            YamlWithSourceInfo::new_scalar(Yaml::String("b".into()), SourceInfo::default());

        let yaml = Yaml::Array(vec![Yaml::String("a".into()), Yaml::String("b".into())]);
        let node = YamlWithSourceInfo::new_array(yaml, SourceInfo::default(), vec![child1, child2]);

        assert_eq!(node.get_array_item(0).unwrap().yaml.as_str(), Some("a"));
        assert_eq!(node.get_array_item(1).unwrap().yaml.as_str(), Some("b"));
        assert!(node.get_array_item(2).is_none());
    }
}
