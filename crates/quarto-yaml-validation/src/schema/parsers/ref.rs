//! Reference schema parser
//!
//! This module handles parsing of $ref schemas which reference other
//! schemas by their $id.
//!
//! Formats:
//! - ref: "schema-id" or $ref: "schema-id" - Lazy reference (resolved during validation)
//! - resolveRef: "schema-id" - Eager reference (resolved during parsing)

use crate::error::{SchemaError, SchemaResult};
use quarto_yaml::YamlWithSourceInfo;

use crate::schema::Schema;
use crate::schema::types::RefSchema;

/// Parse a reference schema
///
/// References are simple string values pointing to another schema's $id.
/// The `eager` parameter indicates whether this is a resolveRef (true) or ref/$ref (false).
pub(in crate::schema) fn parse_ref_schema(
    yaml: &YamlWithSourceInfo,
    eager: bool,
) -> SchemaResult<Schema> {
    let reference =
        yaml.yaml
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| SchemaError::InvalidStructure {
                message: "ref must be a string".to_string(),
                location: yaml.source_info.clone(),
            })?;

    Ok(Schema::Ref(RefSchema {
        annotations: Default::default(),
        reference,
        eager,
    }))
}
