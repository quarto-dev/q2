//! Combinator schema parsers
//!
//! This module handles parsing of schema combinators:
//! - anyOf: Validates if any subschema matches
//! - allOf: Validates if all subschemas match
//! - maybeArrayOf: Quarto extension that expands to anyOf(T, arrayOf(T))
//!
//! Both support inline array form and explicit object form with annotations.

use crate::error::{SchemaError, SchemaResult};
use quarto_yaml::YamlWithSourceInfo;
use std::collections::HashMap;

use crate::schema::Schema;
use crate::schema::annotations::parse_annotations;
use crate::schema::parser::from_yaml;
use crate::schema::types::{AllOfSchema, AnyOfSchema, ArraySchema, SchemaAnnotations};

/// Parse an anyOf schema
///
/// Validates if any of the subschemas matches. Supports:
/// - Inline form: anyOf: [schema1, schema2, ...]
/// - Explicit form: anyOf: { schemas: [...], description: "..." }
pub(in crate::schema) fn parse_anyof_schema(yaml: &YamlWithSourceInfo) -> SchemaResult<Schema> {
    let annotations = parse_annotations(yaml)?;

    // Handle both array form and object form with schemas: field
    let schemas = if let Some(schemas_yaml) = yaml.get_hash_value("schemas") {
        // Explicit form: anyOf: { schemas: [...] }
        let items = schemas_yaml
            .as_array()
            .ok_or_else(|| SchemaError::InvalidStructure {
                message: "anyOf schemas must be an array".to_string(),
                location: schemas_yaml.source_info.clone(),
            })?;

        let result: SchemaResult<Vec<_>> = items.iter().map(from_yaml).collect();
        result?
    } else {
        // Inline form: anyOf: [schema1, schema2, ...]
        let items = yaml
            .as_array()
            .ok_or_else(|| SchemaError::InvalidStructure {
                message: "Expected array for anyOf".to_string(),
                location: yaml.source_info.clone(),
            })?;

        let result: SchemaResult<Vec<_>> = items.iter().map(from_yaml).collect();
        result?
    };

    Ok(Schema::AnyOf(AnyOfSchema {
        annotations,
        schemas,
    }))
}

/// Parse an allOf schema
///
/// Validates if all of the subschemas match. Supports:
/// - Inline form: allOf: [schema1, schema2, ...]
/// - Explicit form: allOf: { schemas: [...], description: "..." }
pub(in crate::schema) fn parse_allof_schema(yaml: &YamlWithSourceInfo) -> SchemaResult<Schema> {
    let annotations = parse_annotations(yaml)?;

    // Similar to anyOf
    let schemas = if let Some(schemas_yaml) = yaml.get_hash_value("schemas") {
        let items = schemas_yaml
            .as_array()
            .ok_or_else(|| SchemaError::InvalidStructure {
                message: "allOf schemas must be an array".to_string(),
                location: schemas_yaml.source_info.clone(),
            })?;

        let result: SchemaResult<Vec<_>> = items.iter().map(from_yaml).collect();
        result?
    } else {
        let items = yaml
            .as_array()
            .ok_or_else(|| SchemaError::InvalidStructure {
                message: "Expected array for allOf".to_string(),
                location: yaml.source_info.clone(),
            })?;

        let result: SchemaResult<Vec<_>> = items.iter().map(from_yaml).collect();
        result?
    };

    Ok(Schema::AllOf(AllOfSchema {
        annotations,
        schemas,
    }))
}

/// Parse a maybeArrayOf schema (quarto-cli extension)
///
/// This expands `maybeArrayOf: T` into `anyOf([T, arrayOf(T)])` with a special tag
/// to indicate completions should come from the first option (the scalar form).
///
/// Format:
/// ```yaml
/// maybeArrayOf: string
/// ```
///
/// Expands to:
/// ```yaml
/// anyOf:
///   - string
///   - arrayOf: string
/// tags:
///   complete-from: ["anyOf", 0]
/// ```
pub(in crate::schema) fn parse_maybe_arrayof_schema(
    yaml: &YamlWithSourceInfo,
) -> SchemaResult<Schema> {
    // Parse the inner schema
    let inner_schema = from_yaml(yaml)?;

    // Create arrayOf version of the schema
    let array_schema = Schema::Array(ArraySchema {
        annotations: Default::default(),
        items: Some(Box::new(inner_schema.clone())),
        min_items: None,
        max_items: None,
        unique_items: None,
    });

    // Create anyOf with both versions
    let schemas = vec![inner_schema, array_schema];

    // Add "complete-from" tag
    let mut tags = HashMap::new();
    tags.insert(
        "complete-from".to_string(),
        serde_json::json!(["anyOf", 0]),
    );

    let annotations = SchemaAnnotations {
        tags: Some(tags),
        ..Default::default()
    };

    Ok(Schema::AnyOf(AnyOfSchema {
        annotations,
        schemas,
    }))
}
