//! Object schema parser
//!
//! This module handles parsing of object schemas which validate key-value mappings.
//! Objects can have:
//! - properties: Named property schemas
//! - patternProperties: Pattern-based property schemas
//! - additionalProperties: Schema for unspecified properties
//! - required: List of required property names
//! - closed: Quarto extension - disallow properties not in schema
//! - minProperties/maxProperties: Property count constraints
//! - namingConvention: Quarto extension - naming convention for property keys
//!
//! Also handles quarto-cli's record syntax, which is shorthand for a closed object
//! with all properties required

use crate::error::{SchemaError, SchemaResult};
use quarto_yaml::YamlWithSourceInfo;
use std::collections::HashMap;

use crate::schema::Schema;
use crate::schema::annotations::parse_annotations;
use crate::schema::helpers::{get_hash_bool, get_hash_usize};
use crate::schema::parser::from_yaml;
use crate::schema::types::{NamingConvention, ObjectSchema};

/// Normalize naming convention string to canonical form
///
/// Supports multiple input formats and normalizes them to one of:
/// - "capitalizationCase" (camelCase)
/// - "underscore_case" (snake_case)
/// - "dash-case" (kebab-case)
/// - "ignore"
fn normalize_convention(input: &str, location: &quarto_yaml::SourceInfo) -> SchemaResult<String> {
    match input {
        "ignore" => Ok("ignore".to_string()),

        // camelCase / capitalizationCase variants
        "camelCase"
        | "capitalizationCase"
        | "camel-case"
        | "camel_case"
        | "capitalization-case"
        | "capitalization_case" => Ok("capitalizationCase".to_string()),

        // snake_case / underscoreCase variants
        "snakeCase" | "underscoreCase" | "snake-case" | "snake_case" | "underscore-case"
        | "underscore_case" => Ok("underscore_case".to_string()),

        // kebab-case / dashCase variants
        "dashCase" | "kebabCase" | "dash-case" | "dash_case" | "kebab-case" | "kebab_case" => {
            Ok("dash-case".to_string())
        }

        _ => Err(SchemaError::InvalidStructure {
            message: format!("Unknown naming convention: '{}'", input),
            location: location.clone(),
        }),
    }
}

/// Parse an object schema
///
/// Format:
/// ```yaml
/// object:
///   properties:
///     name: string
///     age: number
///   patternProperties:
///     "^x-": string
///   additionalProperties: boolean
///   required: [name]
///   closed: true
///   minProperties: 1
///   maxProperties: 10
/// ```
pub(in crate::schema) fn parse_object_schema(yaml: &YamlWithSourceInfo) -> SchemaResult<Schema> {
    let annotations = parse_annotations(yaml)?;

    // Parse properties
    let properties = if let Some(props_yaml) = yaml.get_hash_value("properties") {
        let entries = props_yaml
            .as_hash()
            .ok_or_else(|| SchemaError::InvalidStructure {
                message: "properties must be an object".to_string(),
                location: props_yaml.source_info.clone(),
            })?;

        let mut props = HashMap::new();
        for entry in entries {
            let key = entry
                .key
                .yaml
                .as_str()
                .ok_or_else(|| SchemaError::InvalidStructure {
                    message: "property key must be a string".to_string(),
                    location: entry.key.source_info.clone(),
                })?;
            let schema = from_yaml(&entry.value)?;
            props.insert(key.to_string(), schema);
        }
        props
    } else {
        HashMap::new()
    };

    // Parse patternProperties
    let pattern_properties =
        if let Some(pattern_props_yaml) = yaml.get_hash_value("patternProperties") {
            let entries =
                pattern_props_yaml
                    .as_hash()
                    .ok_or_else(|| SchemaError::InvalidStructure {
                        message: "patternProperties must be an object".to_string(),
                        location: pattern_props_yaml.source_info.clone(),
                    })?;

            let mut props = HashMap::new();
            for entry in entries {
                let key = entry
                    .key
                    .yaml
                    .as_str()
                    .ok_or_else(|| SchemaError::InvalidStructure {
                        message: "patternProperty key must be a string".to_string(),
                        location: entry.key.source_info.clone(),
                    })?;
                let schema = from_yaml(&entry.value)?;
                props.insert(key.to_string(), schema);
            }
            props
        } else {
            HashMap::new()
        };

    // Parse additionalProperties
    let additional_properties =
        if let Some(additional_yaml) = yaml.get_hash_value("additionalProperties") {
            Some(Box::new(from_yaml(additional_yaml)?))
        } else {
            None
        };

    // Parse required
    let required = if let Some(required_yaml) = yaml.get_hash_value("required") {
        // Check if it's the string "all"
        if let Some(req_str) = required_yaml.yaml.as_str() {
            if req_str == "all" {
                // Expand to all property keys
                properties.keys().cloned().collect()
            } else {
                return Err(SchemaError::InvalidStructure {
                    message: format!(
                        "Invalid required value: '{}' (expected 'all' or array)",
                        req_str
                    ),
                    location: required_yaml.source_info.clone(),
                });
            }
        } else {
            // Handle array form
            let items = required_yaml
                .as_array()
                .ok_or_else(|| SchemaError::InvalidStructure {
                    message: "required must be 'all' or an array".to_string(),
                    location: required_yaml.source_info.clone(),
                })?;

            let result: SchemaResult<Vec<_>> = items
                .iter()
                .map(|item| {
                    item.yaml.as_str().map(|s| s.to_string()).ok_or_else(|| {
                        SchemaError::InvalidStructure {
                            message: "required items must be strings".to_string(),
                            location: item.source_info.clone(),
                        }
                    })
                })
                .collect();
            result?
        }
    } else {
        Vec::new()
    };

    let min_properties = get_hash_usize(yaml, "minProperties")?;
    let max_properties = get_hash_usize(yaml, "maxProperties")?;
    let closed = get_hash_bool(yaml, "closed")?.unwrap_or(false);

    // Parse propertyNames
    let property_names = if let Some(property_names_yaml) = yaml.get_hash_value("propertyNames") {
        Some(Box::new(from_yaml(property_names_yaml)?))
    } else {
        None
    };

    // Parse namingConvention
    let naming_convention = if let Some(nc_yaml) = yaml.get_hash_value("namingConvention") {
        if let Some(s) = nc_yaml.yaml.as_str() {
            // Single string value
            Some(NamingConvention::Single(normalize_convention(
                s,
                &nc_yaml.source_info,
            )?))
        } else if let Some(arr) = nc_yaml.as_array() {
            // Array of strings
            let conventions: SchemaResult<Vec<_>> = arr
                .iter()
                .map(|item| {
                    item.yaml
                        .as_str()
                        .ok_or_else(|| SchemaError::InvalidStructure {
                            message: "namingConvention items must be strings".to_string(),
                            location: item.source_info.clone(),
                        })
                        .and_then(|s| normalize_convention(s, &item.source_info))
                })
                .collect();
            Some(NamingConvention::Multiple(conventions?))
        } else {
            return Err(SchemaError::InvalidStructure {
                message: "namingConvention must be a string or array of strings".to_string(),
                location: nc_yaml.source_info.clone(),
            });
        }
    } else {
        None
    };

    // Parse super/baseSchema for inheritance
    let base_schema = if let Some(super_yaml) = yaml.get_hash_value("super") {
        if let Some(arr) = super_yaml.as_array() {
            // Array form: super: [schema1, schema2]
            let schemas: SchemaResult<Vec<_>> = arr.iter().map(|item| from_yaml(item)).collect();
            Some(schemas?)
        } else {
            // Single schema form: super: { resolveRef: ... }
            Some(vec![from_yaml(super_yaml)?])
        }
    } else {
        None
    };

    Ok(Schema::Object(ObjectSchema {
        annotations,
        properties,
        pattern_properties,
        additional_properties,
        required,
        min_properties,
        max_properties,
        closed,
        property_names,
        naming_convention,
        base_schema,
    }))
}

/// Parse a record schema (quarto-cli shorthand)
///
/// This is syntactic sugar that expands to a closed object with all properties required.
///
/// Form 1:
/// ```yaml
/// record:
///   properties:
///     key1: string
///     key2: number
/// ```
///
/// Form 2 (shorthand):
/// ```yaml
/// record:
///   key1: string
///   key2: number
/// ```
///
/// Form 3 (with keySchema/valueSchema):
/// ```yaml
/// record:
///   keySchema:
///     string:
///       pattern: "^[a-z]+$"
///   valueSchema: number
/// ```
///
/// Forms 1 & 2 expand to:
/// ```yaml
/// object:
///   properties: { ... }
///   closed: true
///   required: all  # All property keys
/// ```
///
/// Form 3 expands to:
/// ```yaml
/// object:
///   propertyNames: <keySchema>
///   additionalProperties: <valueSchema>
/// ```
pub(in crate::schema) fn parse_record_schema(yaml: &YamlWithSourceInfo) -> SchemaResult<Schema> {
    let annotations = parse_annotations(yaml)?;

    // Check for keySchema/valueSchema form first (Form 3)
    let has_key_schema = yaml.get_hash_value("keySchema").is_some();
    let has_value_schema = yaml.get_hash_value("valueSchema").is_some();

    if has_key_schema || has_value_schema {
        // Form 3: record with keySchema and/or valueSchema
        let property_names = if let Some(key_schema_yaml) = yaml.get_hash_value("keySchema") {
            Some(Box::new(from_yaml(key_schema_yaml)?))
        } else {
            None
        };

        let additional_properties =
            if let Some(value_schema_yaml) = yaml.get_hash_value("valueSchema") {
                Some(Box::new(from_yaml(value_schema_yaml)?))
            } else {
                None
            };

        return Ok(Schema::Object(ObjectSchema {
            annotations,
            properties: HashMap::new(),
            pattern_properties: HashMap::new(),
            additional_properties,
            required: Vec::new(),
            min_properties: None,
            max_properties: None,
            closed: false,
            property_names,
            naming_convention: None,
            base_schema: None,
        }));
    }

    // Check if this is Form 1 (has "properties" key) or Form 2 (direct properties)
    let properties = if let Some(props_yaml) = yaml.get_hash_value("properties") {
        // Form 1: record: { properties: { ... } }
        let entries = props_yaml
            .as_hash()
            .ok_or_else(|| SchemaError::InvalidStructure {
                message: "record properties must be an object".to_string(),
                location: props_yaml.source_info.clone(),
            })?;

        let mut props = HashMap::new();
        for entry in entries {
            let key = entry
                .key
                .yaml
                .as_str()
                .ok_or_else(|| SchemaError::InvalidStructure {
                    message: "property key must be a string".to_string(),
                    location: entry.key.source_info.clone(),
                })?;
            let schema = from_yaml(&entry.value)?;
            props.insert(key.to_string(), schema);
        }
        props
    } else {
        // Form 2: record: { key1: schema1, key2: schema2 }
        // The entire yaml value is the properties hash
        let entries = yaml
            .as_hash()
            .ok_or_else(|| SchemaError::InvalidStructure {
                message: "record must be an object".to_string(),
                location: yaml.source_info.clone(),
            })?;

        let mut props = HashMap::new();
        for entry in entries {
            let key = entry
                .key
                .yaml
                .as_str()
                .ok_or_else(|| SchemaError::InvalidStructure {
                    message: "property key must be a string".to_string(),
                    location: entry.key.source_info.clone(),
                })?;
            let schema = from_yaml(&entry.value)?;
            props.insert(key.to_string(), schema);
        }
        props
    };

    // All properties are required
    let required: Vec<String> = properties.keys().cloned().collect();

    Ok(Schema::Object(ObjectSchema {
        annotations,
        properties,
        pattern_properties: HashMap::new(),
        additional_properties: None,
        required,
        min_properties: None,
        max_properties: None,
        closed: true, // Records are always closed
        property_names: None,
        naming_convention: None,
        base_schema: None,
    }))
}
