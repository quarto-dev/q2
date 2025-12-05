//! Array schema parser
//!
//! This module handles parsing of array schemas which validate array/list values.
//! Arrays can have:
//! - items: Schema for array elements
//! - minItems/maxItems: Length constraints
//! - uniqueItems: Whether elements must be unique
//!
//! Also handles quarto-cli's arrayOf shorthand syntax:
//! - arrayOf: <schema> - Simple form
//! - arrayOf: { schema: <schema>, length: N } - Fixed-length arrays

use crate::error::SchemaResult;
use quarto_yaml::YamlWithSourceInfo;

use crate::schema::Schema;
use crate::schema::annotations::parse_annotations;
use crate::schema::helpers::{get_hash_bool, get_hash_usize};
use crate::schema::parser::from_yaml;
use crate::schema::types::ArraySchema;

/// Parse an array schema
///
/// Format:
/// ```yaml
/// array:
///   items: <schema>
///   minItems: 1
///   maxItems: 10
///   uniqueItems: true
/// ```
pub(in crate::schema) fn parse_array_schema(yaml: &YamlWithSourceInfo) -> SchemaResult<Schema> {
    let annotations = parse_annotations(yaml)?;
    let items = if let Some(items_yaml) = yaml.get_hash_value("items") {
        Some(Box::new(from_yaml(items_yaml)?))
    } else {
        None
    };
    let min_items = get_hash_usize(yaml, "minItems")?;
    let max_items = get_hash_usize(yaml, "maxItems")?;
    let unique_items = get_hash_bool(yaml, "uniqueItems")?;

    Ok(Schema::Array(ArraySchema {
        annotations,
        items,
        min_items,
        max_items,
        unique_items,
    }))
}

/// Parse arrayOf schema (quarto-cli shorthand)
///
/// Simple form:
/// ```yaml
/// arrayOf: string
/// ```
///
/// Complex form with length:
/// ```yaml
/// arrayOf:
///   schema: string
///   length: 2
/// ```
///
/// The `length` property sets both minItems and maxItems to the same value.
pub(in crate::schema) fn parse_arrayof_schema(yaml: &YamlWithSourceInfo) -> SchemaResult<Schema> {
    let annotations = parse_annotations(yaml)?;

    // Check if this is the complex form with a `schema` key
    if let Some(schema_yaml) = yaml.get_hash_value("schema") {
        // Complex form: arrayOf: { schema: <schema>, length: N }
        let items = Some(Box::new(from_yaml(schema_yaml)?));
        let length = get_hash_usize(yaml, "length")?;

        // If length is specified, set both min_items and max_items
        let (min_items, max_items) = if let Some(len) = length {
            (Some(len), Some(len))
        } else {
            (None, None)
        };

        Ok(Schema::Array(ArraySchema {
            annotations,
            items,
            min_items,
            max_items,
            unique_items: None,
        }))
    } else {
        // Simple form: arrayOf: <schema>
        // The entire YAML value is the schema
        let items = Some(Box::new(from_yaml(yaml)?));

        Ok(Schema::Array(ArraySchema {
            annotations: Default::default(),
            items,
            min_items: None,
            max_items: None,
            unique_items: None,
        }))
    }
}
