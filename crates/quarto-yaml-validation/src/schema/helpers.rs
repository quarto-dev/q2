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
