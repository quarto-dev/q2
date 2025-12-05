//! Primitive type schema parsers
//!
//! This module contains parsers for basic/primitive schema types:
//! - boolean
//! - number
//! - string (including "path" alias)
//! - null
//! - any

use crate::error::SchemaResult;
use quarto_yaml::YamlWithSourceInfo;

use crate::schema::Schema;
use crate::schema::annotations::parse_annotations;
use crate::schema::helpers::{get_hash_number, get_hash_string, get_hash_usize};
use crate::schema::types::{AnySchema, BooleanSchema, NullSchema, NumberSchema, StringSchema};

/// Parse a boolean schema
pub(in crate::schema) fn parse_boolean_schema(yaml: &YamlWithSourceInfo) -> SchemaResult<Schema> {
    let annotations = parse_annotations(yaml)?;
    Ok(Schema::Boolean(BooleanSchema { annotations }))
}

/// Parse a number schema (integer or float)
pub(in crate::schema) fn parse_number_schema(yaml: &YamlWithSourceInfo) -> SchemaResult<Schema> {
    let annotations = parse_annotations(yaml)?;
    let minimum = get_hash_number(yaml, "minimum")?;
    let maximum = get_hash_number(yaml, "maximum")?;
    let exclusive_minimum = get_hash_number(yaml, "exclusiveMinimum")?;
    let exclusive_maximum = get_hash_number(yaml, "exclusiveMaximum")?;
    let multiple_of = get_hash_number(yaml, "multipleOf")?;

    Ok(Schema::Number(NumberSchema {
        annotations,
        minimum,
        maximum,
        exclusive_minimum,
        exclusive_maximum,
        multiple_of,
    }))
}

/// Parse a string schema (also handles "path" alias)
pub(in crate::schema) fn parse_string_schema(yaml: &YamlWithSourceInfo) -> SchemaResult<Schema> {
    let annotations = parse_annotations(yaml)?;
    let min_length = get_hash_usize(yaml, "minLength")?;
    let max_length = get_hash_usize(yaml, "maxLength")?;
    let pattern = get_hash_string(yaml, "pattern")?;

    Ok(Schema::String(StringSchema {
        annotations,
        min_length,
        max_length,
        pattern,
    }))
}

/// Parse a null schema
pub(in crate::schema) fn parse_null_schema(yaml: &YamlWithSourceInfo) -> SchemaResult<Schema> {
    let annotations = parse_annotations(yaml)?;
    Ok(Schema::Null(NullSchema { annotations }))
}

/// Parse an any schema (accepts any value)
pub(in crate::schema) fn parse_any_schema(yaml: &YamlWithSourceInfo) -> SchemaResult<Schema> {
    let annotations = parse_annotations(yaml)?;
    Ok(Schema::Any(AnySchema { annotations }))
}
