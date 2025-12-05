//! Enum schema parser
//!
//! This module handles parsing of enum schemas which define a fixed set
//! of allowed values. Supports both inline and explicit forms:
//! - Inline: enum: [val1, val2, val3]
//! - Explicit: enum: { values: [...], description: "..." }

use crate::error::{SchemaError, SchemaResult};
use quarto_yaml::YamlWithSourceInfo;

use crate::schema::Schema;
use crate::schema::annotations::parse_annotations;
use crate::schema::helpers::yaml_to_json_value;
use crate::schema::types::EnumSchema;

/// Parse an enum schema
///
/// Handles both inline array form and explicit object form with annotations
pub(in crate::schema) fn parse_enum_schema(yaml: &YamlWithSourceInfo) -> SchemaResult<Schema> {
    let annotations = parse_annotations(yaml)?;

    // Handle both inline array and explicit object form
    let values = if let Some(values_yaml) = yaml.get_hash_value("values") {
        // Explicit form: enum: { values: [...] }
        let items = values_yaml
            .as_array()
            .ok_or_else(|| SchemaError::InvalidStructure {
                message: "enum values must be an array".to_string(),
                location: values_yaml.source_info.clone(),
            })?;

        let result: SchemaResult<Vec<_>> = items
            .iter()
            .map(|item| yaml_to_json_value(&item.yaml, &item.source_info))
            .collect();
        result?
    } else {
        // Inline form: enum: [val1, val2, val3]
        let items = yaml
            .as_array()
            .ok_or_else(|| SchemaError::InvalidStructure {
                message: "Expected array for inline enum".to_string(),
                location: yaml.source_info.clone(),
            })?;

        let result: SchemaResult<Vec<_>> = items
            .iter()
            .map(|item| yaml_to_json_value(&item.yaml, &item.source_info))
            .collect();
        result?
    };

    Ok(Schema::Enum(EnumSchema {
        annotations,
        values,
    }))
}
