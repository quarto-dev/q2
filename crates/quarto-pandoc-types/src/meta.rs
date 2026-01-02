/*
 * meta.rs
 * Copyright (c) 2025 Posit, PBC
 */

use crate::block::Blocks;
use crate::inline::Inlines;
use hashlink::LinkedHashMap;
use serde::{Deserialize, Serialize};

// Pandoc's MetaValue notably does not support numbers or nulls, so we don't either
// https://pandoc.org/lua-filters.html#type-metavalue
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum MetaValue {
    MetaString(String),
    MetaBool(bool),
    MetaInlines(Inlines),
    MetaBlocks(Blocks),
    MetaList(Vec<MetaValue>),
    MetaMap(LinkedHashMap<String, MetaValue>),
}

impl Default for MetaValue {
    fn default() -> Self {
        MetaValue::MetaMap(LinkedHashMap::new())
    }
}

pub type Meta = LinkedHashMap<String, MetaValue>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::Plain;
    use crate::inline::Str;
    use quarto_source_map::{FileId, Location, Range, SourceInfo};

    fn dummy_source_info() -> SourceInfo {
        SourceInfo::from_range(
            FileId(0),
            Range {
                start: Location {
                    offset: 0,
                    row: 0,
                    column: 0,
                },
                end: Location {
                    offset: 0,
                    row: 0,
                    column: 0,
                },
            },
        )
    }

    // === MetaValue enum tests ===

    #[test]
    fn test_meta_value_string() {
        let mv = MetaValue::MetaString("hello".to_string());
        match mv {
            MetaValue::MetaString(s) => assert_eq!(s, "hello"),
            _ => panic!("Expected MetaString"),
        }
    }

    #[test]
    fn test_meta_value_bool_true() {
        let mv = MetaValue::MetaBool(true);
        match mv {
            MetaValue::MetaBool(b) => assert!(b),
            _ => panic!("Expected MetaBool"),
        }
    }

    #[test]
    fn test_meta_value_bool_false() {
        let mv = MetaValue::MetaBool(false);
        match mv {
            MetaValue::MetaBool(b) => assert!(!b),
            _ => panic!("Expected MetaBool"),
        }
    }

    #[test]
    fn test_meta_value_inlines() {
        let inlines = vec![crate::inline::Inline::Str(Str {
            text: "test".to_string(),
            source_info: dummy_source_info(),
        })];
        let mv = MetaValue::MetaInlines(inlines);
        match mv {
            MetaValue::MetaInlines(i) => assert_eq!(i.len(), 1),
            _ => panic!("Expected MetaInlines"),
        }
    }

    #[test]
    fn test_meta_value_blocks() {
        let blocks = vec![crate::Block::Plain(Plain {
            content: vec![],
            source_info: dummy_source_info(),
        })];
        let mv = MetaValue::MetaBlocks(blocks);
        match mv {
            MetaValue::MetaBlocks(b) => assert_eq!(b.len(), 1),
            _ => panic!("Expected MetaBlocks"),
        }
    }

    #[test]
    fn test_meta_value_list() {
        let list = vec![
            MetaValue::MetaString("a".to_string()),
            MetaValue::MetaString("b".to_string()),
        ];
        let mv = MetaValue::MetaList(list);
        match mv {
            MetaValue::MetaList(l) => assert_eq!(l.len(), 2),
            _ => panic!("Expected MetaList"),
        }
    }

    #[test]
    fn test_meta_value_map() {
        let mut map = LinkedHashMap::new();
        map.insert(
            "key".to_string(),
            MetaValue::MetaString("value".to_string()),
        );
        let mv = MetaValue::MetaMap(map);
        match mv {
            MetaValue::MetaMap(m) => {
                assert_eq!(m.len(), 1);
                assert!(m.contains_key("key"));
            }
            _ => panic!("Expected MetaMap"),
        }
    }

    // === Default trait tests ===

    #[test]
    fn test_meta_value_default() {
        let mv = MetaValue::default();
        match mv {
            MetaValue::MetaMap(m) => assert!(m.is_empty()),
            _ => panic!("Expected empty MetaMap from default"),
        }
    }

    // === Clone tests ===

    #[test]
    fn test_meta_value_clone() {
        let original = MetaValue::MetaString("test".to_string());
        let cloned = original.clone();
        assert_eq!(original, cloned);
    }

    #[test]
    fn test_meta_value_clone_nested() {
        let mut inner_map = LinkedHashMap::new();
        inner_map.insert("inner".to_string(), MetaValue::MetaBool(true));
        let list = vec![MetaValue::MetaMap(inner_map)];
        let original = MetaValue::MetaList(list);
        let cloned = original.clone();
        assert_eq!(original, cloned);
    }

    // === PartialEq tests ===

    #[test]
    fn test_meta_value_eq() {
        let a = MetaValue::MetaString("same".to_string());
        let b = MetaValue::MetaString("same".to_string());
        assert_eq!(a, b);
    }

    #[test]
    fn test_meta_value_ne() {
        let a = MetaValue::MetaString("one".to_string());
        let b = MetaValue::MetaString("two".to_string());
        assert_ne!(a, b);
    }

    #[test]
    fn test_meta_value_different_variants_ne() {
        let a = MetaValue::MetaString("true".to_string());
        let b = MetaValue::MetaBool(true);
        assert_ne!(a, b);
    }

    // === Debug tests ===

    #[test]
    fn test_meta_value_debug() {
        let mv = MetaValue::MetaString("test".to_string());
        let debug = format!("{:?}", mv);
        assert!(debug.contains("MetaString"));
        assert!(debug.contains("test"));
    }

    // === Serialization tests ===

    #[test]
    fn test_meta_value_serialize_string() {
        let mv = MetaValue::MetaString("hello".to_string());
        let json = serde_json::to_string(&mv).unwrap();
        assert!(json.contains("MetaString"));
        assert!(json.contains("hello"));
    }

    #[test]
    fn test_meta_value_serialize_bool() {
        let mv = MetaValue::MetaBool(true);
        let json = serde_json::to_string(&mv).unwrap();
        assert!(json.contains("MetaBool"));
        assert!(json.contains("true"));
    }

    #[test]
    fn test_meta_value_serialize_list() {
        let list = vec![
            MetaValue::MetaString("a".to_string()),
            MetaValue::MetaBool(false),
        ];
        let mv = MetaValue::MetaList(list);
        let json = serde_json::to_string(&mv).unwrap();
        assert!(json.contains("MetaList"));
    }

    #[test]
    fn test_meta_value_roundtrip() {
        let mut map = LinkedHashMap::new();
        map.insert(
            "key".to_string(),
            MetaValue::MetaString("value".to_string()),
        );
        map.insert("flag".to_string(), MetaValue::MetaBool(true));
        let original = MetaValue::MetaMap(map);

        let json = serde_json::to_string(&original).unwrap();
        let deserialized: MetaValue = serde_json::from_str(&json).unwrap();

        assert_eq!(original, deserialized);
    }

    // === Meta type alias tests ===

    #[test]
    fn test_meta_type_alias() {
        let mut meta: Meta = LinkedHashMap::new();
        meta.insert(
            "title".to_string(),
            MetaValue::MetaString("My Doc".to_string()),
        );
        meta.insert("draft".to_string(), MetaValue::MetaBool(false));
        assert_eq!(meta.len(), 2);
        assert!(meta.contains_key("title"));
    }

    #[test]
    fn test_meta_nested() {
        let mut author_map = LinkedHashMap::new();
        author_map.insert(
            "name".to_string(),
            MetaValue::MetaString("Alice".to_string()),
        );

        let authors = MetaValue::MetaList(vec![MetaValue::MetaMap(author_map)]);

        let mut meta: Meta = LinkedHashMap::new();
        meta.insert("author".to_string(), authors);

        match meta.get("author") {
            Some(MetaValue::MetaList(list)) => {
                assert_eq!(list.len(), 1);
            }
            _ => panic!("Expected MetaList for author"),
        }
    }
}
