//! Helper functions for parsing YAML schemas
//!
//! This module contains utility functions for extracting specific types
//! of values from YamlWithSourceInfo structures, with proper error handling.

use crate::error::{SchemaError, SchemaResult};
use quarto_yaml::{SourceInfo, YamlWithSourceInfo};
use std::collections::HashMap;
use yaml_rust2::Yaml;

/// Get a string value from a hash by key
pub(super) fn get_hash_string(
    yaml: &YamlWithSourceInfo,
    key: &str,
) -> SchemaResult<Option<String>> {
    if let Some(value) = yaml.get_hash_value(key) {
        if let Some(s) = value.yaml.as_str() {
            return Ok(Some(s.to_string()));
        }
        return Err(SchemaError::InvalidStructure {
            message: format!("Field '{}' must be a string", key),
            location: value.source_info.clone(),
        });
    }
    Ok(None)
}

/// Get a number value from a hash by key
pub(super) fn get_hash_number(yaml: &YamlWithSourceInfo, key: &str) -> SchemaResult<Option<f64>> {
    if let Some(value) = yaml.get_hash_value(key) {
        match &value.yaml {
            Yaml::Integer(i) => return Ok(Some(*i as f64)),
            Yaml::Real(r) => {
                if let Ok(f) = r.parse::<f64>() {
                    return Ok(Some(f));
                }
            }
            _ => {}
        }
        return Err(SchemaError::InvalidStructure {
            message: format!("Field '{}' must be a number", key),
            location: value.source_info.clone(),
        });
    }
    Ok(None)
}

/// Get a usize value from a hash by key
pub(super) fn get_hash_usize(yaml: &YamlWithSourceInfo, key: &str) -> SchemaResult<Option<usize>> {
    if let Some(value) = yaml.get_hash_value(key) {
        if let Some(i) = value.yaml.as_i64()
            && i >= 0
        {
            return Ok(Some(i as usize));
        }
        return Err(SchemaError::InvalidStructure {
            message: format!("Field '{}' must be a non-negative integer", key),
            location: value.source_info.clone(),
        });
    }
    Ok(None)
}

/// Get a boolean value from a hash by key
pub(super) fn get_hash_bool(yaml: &YamlWithSourceInfo, key: &str) -> SchemaResult<Option<bool>> {
    if let Some(value) = yaml.get_hash_value(key) {
        if let Some(b) = value.yaml.as_bool() {
            return Ok(Some(b));
        }
        return Err(SchemaError::InvalidStructure {
            message: format!("Field '{}' must be a boolean", key),
            location: value.source_info.clone(),
        });
    }
    Ok(None)
}

/// Get an array of strings from a hash by key
pub(super) fn get_hash_string_array(
    yaml: &YamlWithSourceInfo,
    key: &str,
) -> SchemaResult<Option<Vec<String>>> {
    if let Some(value) = yaml.get_hash_value(key) {
        let items = value
            .as_array()
            .ok_or_else(|| SchemaError::InvalidStructure {
                message: format!("Field '{}' must be an array", key),
                location: value.source_info.clone(),
            })?;

        let result: SchemaResult<Vec<_>> = items
            .iter()
            .map(|item| {
                item.yaml.as_str().map(|s| s.to_string()).ok_or_else(|| {
                    SchemaError::InvalidStructure {
                        message: format!("Field '{}' items must be strings", key),
                        location: item.source_info.clone(),
                    }
                })
            })
            .collect();
        return Ok(Some(result?));
    }
    Ok(None)
}

/// Get tags (a hash of key-value pairs) from a schema
pub(super) fn get_hash_tags(
    yaml: &YamlWithSourceInfo,
) -> SchemaResult<Option<HashMap<String, serde_json::Value>>> {
    if let Some(value) = yaml.get_hash_value("tags") {
        let entries = value
            .as_hash()
            .ok_or_else(|| SchemaError::InvalidStructure {
                message: "tags must be an object".to_string(),
                location: value.source_info.clone(),
            })?;

        let mut tags = HashMap::new();
        for entry in entries {
            let key = entry
                .key
                .yaml
                .as_str()
                .ok_or_else(|| SchemaError::InvalidStructure {
                    message: "tag key must be a string".to_string(),
                    location: entry.key.source_info.clone(),
                })?;
            let value = yaml_to_json_value(&entry.value.yaml, &entry.value.source_info)?;
            tags.insert(key.to_string(), value);
        }
        return Ok(Some(tags));
    }
    Ok(None)
}

/// Convert yaml-rust2 Yaml to serde_json::Value (for enum values and tags)
pub(super) fn yaml_to_json_value(
    yaml: &Yaml,
    location: &SourceInfo,
) -> SchemaResult<serde_json::Value> {
    match yaml {
        Yaml::String(s) => Ok(serde_json::Value::String(s.clone())),
        Yaml::Integer(i) => Ok(serde_json::Value::Number((*i).into())),
        Yaml::Real(r) => {
            if let Ok(f) = r.parse::<f64>()
                && let Some(n) = serde_json::Number::from_f64(f)
            {
                return Ok(serde_json::Value::Number(n));
            }
            Err(SchemaError::InvalidStructure {
                message: format!("Invalid number: {}", r),
                location: location.clone(),
            })
        }
        Yaml::Boolean(b) => Ok(serde_json::Value::Bool(*b)),
        Yaml::Null => Ok(serde_json::Value::Null),
        _ => Err(SchemaError::InvalidStructure {
            message: "Unsupported YAML type for JSON conversion".to_string(),
            location: location.clone(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_yaml::YamlHashEntry;
    use yaml_rust2::yaml::Hash;

    fn source_info() -> SourceInfo {
        SourceInfo::default()
    }

    /// Create a YamlWithSourceInfo hash with a single key-value pair
    fn make_hash(key: &str, value: Yaml) -> YamlWithSourceInfo {
        let mut hash = Hash::new();
        hash.insert(Yaml::String(key.to_string()), value.clone());

        let key_node = YamlWithSourceInfo::new_scalar(Yaml::String(key.to_string()), source_info());
        let value_node = YamlWithSourceInfo::new_scalar(value, source_info());

        let entry = YamlHashEntry::new(
            key_node,
            value_node,
            source_info(),
            source_info(),
            source_info(),
        );

        YamlWithSourceInfo::new_hash(Yaml::Hash(hash), source_info(), vec![entry])
    }

    /// Create a YamlWithSourceInfo hash with a key pointing to an array
    fn make_hash_with_array(key: &str, items: Vec<Yaml>) -> YamlWithSourceInfo {
        let mut hash = Hash::new();
        hash.insert(Yaml::String(key.to_string()), Yaml::Array(items.clone()));

        let key_node = YamlWithSourceInfo::new_scalar(Yaml::String(key.to_string()), source_info());

        let children: Vec<YamlWithSourceInfo> = items
            .into_iter()
            .map(|y| YamlWithSourceInfo::new_scalar(y, source_info()))
            .collect();
        let value_node =
            YamlWithSourceInfo::new_array(Yaml::Array(vec![]), source_info(), children);

        let entry = YamlHashEntry::new(
            key_node,
            value_node,
            source_info(),
            source_info(),
            source_info(),
        );

        YamlWithSourceInfo::new_hash(Yaml::Hash(hash), source_info(), vec![entry])
    }

    /// Create a YamlWithSourceInfo hash with a key pointing to a nested hash
    fn make_hash_with_nested_hash(
        outer_key: &str,
        inner_entries: Vec<(&str, Yaml)>,
    ) -> YamlWithSourceInfo {
        let mut outer_hash = Hash::new();
        let mut inner_hash = Hash::new();
        let mut inner_hash_entries = Vec::new();

        for (k, v) in inner_entries {
            inner_hash.insert(Yaml::String(k.to_string()), v.clone());

            let inner_key_node =
                YamlWithSourceInfo::new_scalar(Yaml::String(k.to_string()), source_info());
            let inner_value_node = YamlWithSourceInfo::new_scalar(v, source_info());

            inner_hash_entries.push(YamlHashEntry::new(
                inner_key_node,
                inner_value_node,
                source_info(),
                source_info(),
                source_info(),
            ));
        }

        outer_hash.insert(
            Yaml::String(outer_key.to_string()),
            Yaml::Hash(inner_hash.clone()),
        );

        let outer_key_node =
            YamlWithSourceInfo::new_scalar(Yaml::String(outer_key.to_string()), source_info());
        let inner_hash_node = YamlWithSourceInfo::new_hash(
            Yaml::Hash(inner_hash),
            source_info(),
            inner_hash_entries,
        );

        let entry = YamlHashEntry::new(
            outer_key_node,
            inner_hash_node,
            source_info(),
            source_info(),
            source_info(),
        );

        YamlWithSourceInfo::new_hash(Yaml::Hash(outer_hash), source_info(), vec![entry])
    }

    // ==================== get_hash_string tests ====================

    #[test]
    fn test_get_hash_string_valid() {
        let yaml = make_hash("name", Yaml::String("hello".to_string()));
        let result = get_hash_string(&yaml, "name").unwrap();
        assert_eq!(result, Some("hello".to_string()));
    }

    #[test]
    fn test_get_hash_string_missing_key() {
        let yaml = make_hash("name", Yaml::String("hello".to_string()));
        let result = get_hash_string(&yaml, "other").unwrap();
        assert_eq!(result, None);
    }

    #[test]
    fn test_get_hash_string_not_a_string() {
        let yaml = make_hash("name", Yaml::Integer(42));
        let result = get_hash_string(&yaml, "name");
        assert!(result.is_err());
        let err = result.unwrap_err();
        match err {
            SchemaError::InvalidStructure { message, .. } => {
                assert!(message.contains("'name' must be a string"));
            }
            _ => panic!("Expected InvalidStructure error"),
        }
    }

    #[test]
    fn test_get_hash_string_boolean_not_string() {
        let yaml = make_hash("flag", Yaml::Boolean(true));
        let result = get_hash_string(&yaml, "flag");
        assert!(result.is_err());
    }

    // ==================== get_hash_number tests ====================

    #[test]
    fn test_get_hash_number_integer() {
        let yaml = make_hash("count", Yaml::Integer(42));
        let result = get_hash_number(&yaml, "count").unwrap();
        assert_eq!(result, Some(42.0));
    }

    #[test]
    fn test_get_hash_number_real() {
        let yaml = make_hash("value", Yaml::Real("3.14".to_string()));
        let result = get_hash_number(&yaml, "value").unwrap();
        assert_eq!(result, Some(3.14));
    }

    #[test]
    fn test_get_hash_number_missing_key() {
        let yaml = make_hash("count", Yaml::Integer(42));
        let result = get_hash_number(&yaml, "other").unwrap();
        assert_eq!(result, None);
    }

    #[test]
    fn test_get_hash_number_not_a_number() {
        let yaml = make_hash("count", Yaml::String("not a number".to_string()));
        let result = get_hash_number(&yaml, "count");
        assert!(result.is_err());
        let err = result.unwrap_err();
        match err {
            SchemaError::InvalidStructure { message, .. } => {
                assert!(message.contains("'count' must be a number"));
            }
            _ => panic!("Expected InvalidStructure error"),
        }
    }

    #[test]
    fn test_get_hash_number_invalid_real() {
        // A Real that cannot be parsed as f64
        let yaml = make_hash("value", Yaml::Real("not_a_float".to_string()));
        let result = get_hash_number(&yaml, "value");
        assert!(result.is_err());
    }

    // ==================== get_hash_usize tests ====================

    #[test]
    fn test_get_hash_usize_valid() {
        let yaml = make_hash("size", Yaml::Integer(10));
        let result = get_hash_usize(&yaml, "size").unwrap();
        assert_eq!(result, Some(10));
    }

    #[test]
    fn test_get_hash_usize_zero() {
        let yaml = make_hash("size", Yaml::Integer(0));
        let result = get_hash_usize(&yaml, "size").unwrap();
        assert_eq!(result, Some(0));
    }

    #[test]
    fn test_get_hash_usize_missing_key() {
        let yaml = make_hash("size", Yaml::Integer(10));
        let result = get_hash_usize(&yaml, "other").unwrap();
        assert_eq!(result, None);
    }

    #[test]
    fn test_get_hash_usize_negative() {
        let yaml = make_hash("size", Yaml::Integer(-5));
        let result = get_hash_usize(&yaml, "size");
        assert!(result.is_err());
        let err = result.unwrap_err();
        match err {
            SchemaError::InvalidStructure { message, .. } => {
                assert!(message.contains("'size' must be a non-negative integer"));
            }
            _ => panic!("Expected InvalidStructure error"),
        }
    }

    #[test]
    fn test_get_hash_usize_not_an_integer() {
        let yaml = make_hash("size", Yaml::String("large".to_string()));
        let result = get_hash_usize(&yaml, "size");
        assert!(result.is_err());
    }

    #[test]
    fn test_get_hash_usize_real_number() {
        let yaml = make_hash("size", Yaml::Real("3.14".to_string()));
        let result = get_hash_usize(&yaml, "size");
        assert!(result.is_err());
    }

    // ==================== get_hash_bool tests ====================

    #[test]
    fn test_get_hash_bool_true() {
        let yaml = make_hash("enabled", Yaml::Boolean(true));
        let result = get_hash_bool(&yaml, "enabled").unwrap();
        assert_eq!(result, Some(true));
    }

    #[test]
    fn test_get_hash_bool_false() {
        let yaml = make_hash("enabled", Yaml::Boolean(false));
        let result = get_hash_bool(&yaml, "enabled").unwrap();
        assert_eq!(result, Some(false));
    }

    #[test]
    fn test_get_hash_bool_missing_key() {
        let yaml = make_hash("enabled", Yaml::Boolean(true));
        let result = get_hash_bool(&yaml, "other").unwrap();
        assert_eq!(result, None);
    }

    #[test]
    fn test_get_hash_bool_not_a_boolean() {
        let yaml = make_hash("enabled", Yaml::String("yes".to_string()));
        let result = get_hash_bool(&yaml, "enabled");
        assert!(result.is_err());
        let err = result.unwrap_err();
        match err {
            SchemaError::InvalidStructure { message, .. } => {
                assert!(message.contains("'enabled' must be a boolean"));
            }
            _ => panic!("Expected InvalidStructure error"),
        }
    }

    #[test]
    fn test_get_hash_bool_integer_not_boolean() {
        let yaml = make_hash("enabled", Yaml::Integer(1));
        let result = get_hash_bool(&yaml, "enabled");
        assert!(result.is_err());
    }

    // ==================== get_hash_string_array tests ====================

    #[test]
    fn test_get_hash_string_array_valid() {
        let yaml = make_hash_with_array(
            "items",
            vec![
                Yaml::String("a".to_string()),
                Yaml::String("b".to_string()),
            ],
        );
        let result = get_hash_string_array(&yaml, "items").unwrap();
        assert_eq!(result, Some(vec!["a".to_string(), "b".to_string()]));
    }

    #[test]
    fn test_get_hash_string_array_empty() {
        let yaml = make_hash_with_array("items", vec![]);
        let result = get_hash_string_array(&yaml, "items").unwrap();
        assert_eq!(result, Some(vec![]));
    }

    #[test]
    fn test_get_hash_string_array_missing_key() {
        let yaml = make_hash_with_array("items", vec![Yaml::String("a".to_string())]);
        let result = get_hash_string_array(&yaml, "other").unwrap();
        assert_eq!(result, None);
    }

    #[test]
    fn test_get_hash_string_array_not_an_array() {
        let yaml = make_hash("items", Yaml::String("not an array".to_string()));
        let result = get_hash_string_array(&yaml, "items");
        assert!(result.is_err());
        let err = result.unwrap_err();
        match err {
            SchemaError::InvalidStructure { message, .. } => {
                assert!(message.contains("'items' must be an array"));
            }
            _ => panic!("Expected InvalidStructure error"),
        }
    }

    #[test]
    fn test_get_hash_string_array_non_string_items() {
        let yaml = make_hash_with_array("items", vec![Yaml::Integer(1), Yaml::Integer(2)]);
        let result = get_hash_string_array(&yaml, "items");
        assert!(result.is_err());
        let err = result.unwrap_err();
        match err {
            SchemaError::InvalidStructure { message, .. } => {
                assert!(message.contains("'items' items must be strings"));
            }
            _ => panic!("Expected InvalidStructure error"),
        }
    }

    #[test]
    fn test_get_hash_string_array_mixed_items() {
        let yaml = make_hash_with_array(
            "items",
            vec![Yaml::String("valid".to_string()), Yaml::Integer(42)],
        );
        let result = get_hash_string_array(&yaml, "items");
        assert!(result.is_err());
    }

    // ==================== get_hash_tags tests ====================

    #[test]
    fn test_get_hash_tags_valid() {
        let yaml = make_hash_with_nested_hash(
            "tags",
            vec![
                ("key1", Yaml::String("value1".to_string())),
                ("key2", Yaml::Integer(42)),
            ],
        );
        let result = get_hash_tags(&yaml).unwrap();
        assert!(result.is_some());
        let tags = result.unwrap();
        assert_eq!(tags.get("key1"), Some(&serde_json::json!("value1")));
        assert_eq!(tags.get("key2"), Some(&serde_json::json!(42)));
    }

    #[test]
    fn test_get_hash_tags_missing() {
        let yaml = make_hash("other", Yaml::String("value".to_string()));
        let result = get_hash_tags(&yaml).unwrap();
        assert_eq!(result, None);
    }

    #[test]
    fn test_get_hash_tags_not_an_object() {
        let yaml = make_hash("tags", Yaml::String("not an object".to_string()));
        let result = get_hash_tags(&yaml);
        assert!(result.is_err());
        let err = result.unwrap_err();
        match err {
            SchemaError::InvalidStructure { message, .. } => {
                assert!(message.contains("tags must be an object"));
            }
            _ => panic!("Expected InvalidStructure error"),
        }
    }

    #[test]
    fn test_get_hash_tags_non_string_key() {
        // Create a hash with an integer key (which YAML allows but our API doesn't)
        let mut outer_hash = Hash::new();
        let mut inner_hash = Hash::new();
        inner_hash.insert(Yaml::Integer(123), Yaml::String("value".to_string()));

        outer_hash.insert(
            Yaml::String("tags".to_string()),
            Yaml::Hash(inner_hash.clone()),
        );

        // Create the inner key-value entry with integer key
        let inner_key_node = YamlWithSourceInfo::new_scalar(Yaml::Integer(123), source_info());
        let inner_value_node =
            YamlWithSourceInfo::new_scalar(Yaml::String("value".to_string()), source_info());
        let inner_entry = YamlHashEntry::new(
            inner_key_node,
            inner_value_node,
            source_info(),
            source_info(),
            source_info(),
        );

        let inner_hash_node =
            YamlWithSourceInfo::new_hash(Yaml::Hash(inner_hash), source_info(), vec![inner_entry]);

        let outer_key_node =
            YamlWithSourceInfo::new_scalar(Yaml::String("tags".to_string()), source_info());
        let outer_entry = YamlHashEntry::new(
            outer_key_node,
            inner_hash_node,
            source_info(),
            source_info(),
            source_info(),
        );

        let yaml = YamlWithSourceInfo::new_hash(Yaml::Hash(outer_hash), source_info(), vec![outer_entry]);

        let result = get_hash_tags(&yaml);
        assert!(result.is_err());
        let err = result.unwrap_err();
        match err {
            SchemaError::InvalidStructure { message, .. } => {
                assert!(message.contains("tag key must be a string"));
            }
            _ => panic!("Expected InvalidStructure error"),
        }
    }

    #[test]
    fn test_get_hash_tags_with_boolean() {
        let yaml = make_hash_with_nested_hash("tags", vec![("flag", Yaml::Boolean(true))]);
        let result = get_hash_tags(&yaml).unwrap();
        assert!(result.is_some());
        let tags = result.unwrap();
        assert_eq!(tags.get("flag"), Some(&serde_json::json!(true)));
    }

    #[test]
    fn test_get_hash_tags_with_null() {
        let yaml = make_hash_with_nested_hash("tags", vec![("empty", Yaml::Null)]);
        let result = get_hash_tags(&yaml).unwrap();
        assert!(result.is_some());
        let tags = result.unwrap();
        assert_eq!(tags.get("empty"), Some(&serde_json::Value::Null));
    }

    // ==================== yaml_to_json_value tests ====================

    #[test]
    fn test_yaml_to_json_value_string() {
        let yaml = Yaml::String("hello".to_string());
        let result = yaml_to_json_value(&yaml, &source_info()).unwrap();
        assert_eq!(result, serde_json::json!("hello"));
    }

    #[test]
    fn test_yaml_to_json_value_integer() {
        let yaml = Yaml::Integer(42);
        let result = yaml_to_json_value(&yaml, &source_info()).unwrap();
        assert_eq!(result, serde_json::json!(42));
    }

    #[test]
    fn test_yaml_to_json_value_negative_integer() {
        let yaml = Yaml::Integer(-100);
        let result = yaml_to_json_value(&yaml, &source_info()).unwrap();
        assert_eq!(result, serde_json::json!(-100));
    }

    #[test]
    fn test_yaml_to_json_value_real() {
        let yaml = Yaml::Real("3.14159".to_string());
        let result = yaml_to_json_value(&yaml, &source_info()).unwrap();
        assert_eq!(result, serde_json::json!(3.14159));
    }

    #[test]
    fn test_yaml_to_json_value_boolean_true() {
        let yaml = Yaml::Boolean(true);
        let result = yaml_to_json_value(&yaml, &source_info()).unwrap();
        assert_eq!(result, serde_json::json!(true));
    }

    #[test]
    fn test_yaml_to_json_value_boolean_false() {
        let yaml = Yaml::Boolean(false);
        let result = yaml_to_json_value(&yaml, &source_info()).unwrap();
        assert_eq!(result, serde_json::json!(false));
    }

    #[test]
    fn test_yaml_to_json_value_null() {
        let yaml = Yaml::Null;
        let result = yaml_to_json_value(&yaml, &source_info()).unwrap();
        assert_eq!(result, serde_json::Value::Null);
    }

    #[test]
    fn test_yaml_to_json_value_invalid_real() {
        let yaml = Yaml::Real("not_a_number".to_string());
        let result = yaml_to_json_value(&yaml, &source_info());
        assert!(result.is_err());
        let err = result.unwrap_err();
        match err {
            SchemaError::InvalidStructure { message, .. } => {
                assert!(message.contains("Invalid number"));
            }
            _ => panic!("Expected InvalidStructure error"),
        }
    }

    #[test]
    fn test_yaml_to_json_value_array_unsupported() {
        let yaml = Yaml::Array(vec![Yaml::Integer(1), Yaml::Integer(2)]);
        let result = yaml_to_json_value(&yaml, &source_info());
        assert!(result.is_err());
        let err = result.unwrap_err();
        match err {
            SchemaError::InvalidStructure { message, .. } => {
                assert!(message.contains("Unsupported YAML type"));
            }
            _ => panic!("Expected InvalidStructure error"),
        }
    }

    #[test]
    fn test_yaml_to_json_value_hash_unsupported() {
        let yaml = Yaml::Hash(Hash::new());
        let result = yaml_to_json_value(&yaml, &source_info());
        assert!(result.is_err());
        let err = result.unwrap_err();
        match err {
            SchemaError::InvalidStructure { message, .. } => {
                assert!(message.contains("Unsupported YAML type"));
            }
            _ => panic!("Expected InvalidStructure error"),
        }
    }

    #[test]
    fn test_yaml_to_json_value_infinity() {
        // f64::INFINITY cannot be represented in JSON
        let yaml = Yaml::Real("inf".to_string());
        let result = yaml_to_json_value(&yaml, &source_info());
        // "inf" parses to f64::INFINITY, but serde_json::Number::from_f64 returns None
        assert!(result.is_err());
    }

    #[test]
    fn test_yaml_to_json_value_nan() {
        // NaN cannot be represented in JSON
        let yaml = Yaml::Real("nan".to_string());
        let result = yaml_to_json_value(&yaml, &source_info());
        // "nan" parses to f64::NAN, but serde_json::Number::from_f64 returns None
        assert!(result.is_err());
    }
}
