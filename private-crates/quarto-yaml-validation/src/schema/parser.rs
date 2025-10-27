//! Schema parsing entry point
//!
//! This module provides the main entry point for parsing schemas from YAML:
//! - from_yaml(): Main parsing function
//! - parse_short_form(): Handle short string forms like "boolean", "string"
//! - parse_object_form(): Handle object forms like {boolean: {...}}
//! - parse_inline_enum(): Handle inline enum arrays like [val1, val2, val3]

use crate::error::{SchemaError, SchemaResult};
use quarto_yaml::{SourceInfo, YamlWithSourceInfo};
use yaml_rust2::Yaml;

use super::Schema;
use super::helpers::yaml_to_json_value;
use super::parsers::*;
use super::types::{EnumSchema, NullSchema};

/// Parse a Schema from YamlWithSourceInfo.
///
/// This supports all quarto-cli schema syntaxes:
/// - Short forms: "boolean", "string", "number", etc.
/// - Object forms: {boolean: {...}}, {string: {...}}, etc.
/// - Inline arrays: [val1, val2, val3] (for enums)
///
/// # Example
///
/// ```
/// use quarto_yaml_validation::Schema;
/// use quarto_yaml;
///
/// let yaml = quarto_yaml::parse("boolean").unwrap();
/// let schema = Schema::from_yaml(&yaml).unwrap();
/// ```
pub(super) fn from_yaml(yaml: &YamlWithSourceInfo) -> SchemaResult<Schema> {
    match &yaml.yaml {
        // Short form: "boolean", "string", etc.
        Yaml::String(s) => parse_short_form(s.as_str(), &yaml.source_info),

        // Object form: {boolean: {...}}, {enum: [...]}, etc.
        Yaml::Hash(_) => parse_object_form(yaml),

        // Array form: [val1, val2, val3] - inline enum
        Yaml::Array(_) => parse_inline_enum(yaml),

        // Null can be a schema type too
        Yaml::Null => Ok(Schema::Null(NullSchema {
            annotations: Default::default(),
        })),

        _ => Err(SchemaError::InvalidStructure {
            message: format!("Expected schema, got {:?}", yaml.yaml),
            location: yaml.source_info.clone(),
        }),
    }
}

/// Parse short form: "boolean", "string", "number", "any", "null", "path"
fn parse_short_form(s: &str, _location: &SourceInfo) -> SchemaResult<Schema> {
    match s {
        "boolean" => Ok(Schema::Boolean(super::types::BooleanSchema {
            annotations: Default::default(),
        })),
        "number" => Ok(Schema::Number(super::types::NumberSchema {
            annotations: Default::default(),
            minimum: None,
            maximum: None,
            exclusive_minimum: None,
            exclusive_maximum: None,
            multiple_of: None,
        })),
        "string" | "path" => Ok(Schema::String(super::types::StringSchema {
            annotations: Default::default(),
            min_length: None,
            max_length: None,
            pattern: None,
        })),
        "null" => Ok(Schema::Null(NullSchema {
            annotations: Default::default(),
        })),
        "any" => Ok(Schema::Any(super::types::AnySchema {
            annotations: Default::default(),
        })),
        _ => Err(SchemaError::InvalidType(s.to_string())),
    }
}

/// Parse object form: {boolean: {...}}, {string: {...}}, etc.
fn parse_object_form(yaml: &YamlWithSourceInfo) -> SchemaResult<Schema> {
    let entries = yaml
        .as_hash()
        .ok_or_else(|| SchemaError::InvalidStructure {
            message: "Expected hash for object form schema".to_string(),
            location: yaml.source_info.clone(),
        })?;

    if entries.is_empty() {
        return Err(SchemaError::InvalidStructure {
            message: "Empty schema object".to_string(),
            location: yaml.source_info.clone(),
        });
    }

    // Peek at first key to determine schema type
    let first_entry = &entries[0];
    let key = first_entry
        .key
        .yaml
        .as_str()
        .ok_or_else(|| SchemaError::InvalidStructure {
            message: "Schema type key must be a string".to_string(),
            location: first_entry.key.source_info.clone(),
        })?;

    match key {
        "boolean" => parse_boolean_schema(&first_entry.value),
        "number" => parse_number_schema(&first_entry.value),
        "string" | "path" => parse_string_schema(&first_entry.value),
        "null" => parse_null_schema(&first_entry.value),
        "enum" => parse_enum_schema(&first_entry.value),
        "any" => parse_any_schema(&first_entry.value),
        "anyOf" => parse_anyof_schema(&first_entry.value),
        "allOf" => parse_allof_schema(&first_entry.value),
        "array" => parse_array_schema(&first_entry.value),
        "arrayOf" => parse_arrayof_schema(&first_entry.value),
        "maybeArrayOf" => parse_maybe_arrayof_schema(&first_entry.value),
        "object" => parse_object_schema(&first_entry.value),
        "record" => parse_record_schema(&first_entry.value),
        "schema" => parse_schema_wrapper(yaml),  // Note: pass whole yaml, not just value
        "ref" | "$ref" => parse_ref_schema(&first_entry.value, false),  // Lazy reference
        "resolveRef" => parse_ref_schema(&first_entry.value, true),  // Eager reference
        _ => Err(SchemaError::InvalidType(key.to_string())),
    }
}

/// Parse inline enum array: [val1, val2, val3]
fn parse_inline_enum(yaml: &YamlWithSourceInfo) -> SchemaResult<Schema> {
    let items = yaml
        .as_array()
        .ok_or_else(|| SchemaError::InvalidStructure {
            message: "Expected array for inline enum".to_string(),
            location: yaml.source_info.clone(),
        })?;

    // Convert YamlWithSourceInfo items to serde_json::Value for enum values
    let values: SchemaResult<Vec<_>> = items
        .iter()
        .map(|item| yaml_to_json_value(&item.yaml, &item.source_info))
        .collect();

    Ok(Schema::Enum(EnumSchema {
        annotations: Default::default(),
        values: values?,
    }))
}
