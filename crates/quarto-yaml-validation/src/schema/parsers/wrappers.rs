//! Schema wrapper parsers
//!
//! This module handles meta-schema patterns that wrap other schemas:
//! - schema: Explicit schema wrapper for adding properties without type nesting
//! - pattern: (Future) Pattern-based string matching as a schema type

use crate::error::SchemaResult;
use quarto_yaml::YamlWithSourceInfo;

use crate::schema::Schema;
use crate::schema::annotations::{merge_annotations, parse_annotations};
use crate::schema::parser::from_yaml;

/// Parse a schema wrapper
///
/// The `schema` key allows adding properties (description, completions, etc.)
/// to a schema without nesting under a type key.
///
/// Format:
/// ```yaml
/// schema:
///   anyOf:
///     - boolean
///     - string
/// description: "A boolean or string"
/// completions: ["true", "false", "auto"]
/// ```
///
/// This is equivalent to:
/// ```yaml
/// anyOf:
///   - boolean
///   - string
/// description: "A boolean or string"
/// completions: ["true", "false", "auto"]
/// ```
///
/// But allows cleaner separation when the schema is complex.
pub(in crate::schema) fn parse_schema_wrapper(yaml: &YamlWithSourceInfo) -> SchemaResult<Schema> {
    // Extract the inner schema
    let schema_yaml = yaml.get_hash_value("schema").ok_or_else(|| {
        crate::error::SchemaError::InvalidStructure {
            message: "schema wrapper requires 'schema' key".to_string(),
            location: yaml.source_info.clone(),
        }
    })?;

    // Parse the inner schema (gets inner annotations)
    let inner_schema = from_yaml(schema_yaml)?;

    // Parse annotations from the OUTER wrapper
    let outer_annotations = parse_annotations(yaml)?;

    // Merge outer with inner (outer overrides inner)
    let inner_annotations = inner_schema.annotations().clone();
    let merged_annotations = merge_annotations(inner_annotations, outer_annotations);

    // Apply merged annotations to the schema
    Ok(inner_schema.with_annotations(merged_annotations))
}
