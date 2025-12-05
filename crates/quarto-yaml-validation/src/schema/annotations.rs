//! Schema annotation parsing
//!
//! This module handles parsing of common schema annotations that can be
//! attached to any schema type (description, documentation, error messages, etc.)

use crate::error::SchemaResult;
use quarto_yaml::YamlWithSourceInfo;
use std::collections::HashMap;

use super::helpers::{get_hash_bool, get_hash_string, get_hash_string_array, get_hash_tags};
use super::types::SchemaAnnotations;

/// Static empty annotations for False and True schemas
pub(super) static EMPTY_ANNOTATIONS: SchemaAnnotations = SchemaAnnotations {
    id: None,
    description: None,
    documentation: None,
    error_message: None,
    hidden: None,
    completions: None,
    additional_completions: None,
    tags: None,
};

/// Parse common annotations from a schema object
pub(super) fn parse_annotations(yaml: &YamlWithSourceInfo) -> SchemaResult<SchemaAnnotations> {
    Ok(SchemaAnnotations {
        id: get_hash_string(yaml, "$id")?,
        description: get_hash_string(yaml, "description")?,
        documentation: get_hash_string(yaml, "documentation")?,
        error_message: get_hash_string(yaml, "errorMessage")?,
        hidden: get_hash_bool(yaml, "hidden")?,
        completions: get_hash_string_array(yaml, "completions")?,
        additional_completions: get_hash_string_array(yaml, "additionalCompletions")?,
        tags: get_hash_tags(yaml)?,
    })
}

/// Merge outer annotations with inner annotations
///
/// Outer annotations override inner ones, following quarto-cli semantics:
/// - id, description, documentation, error_message, hidden: outer overrides inner
/// - completions: complex merging with additionalCompletions (see below)
/// - tags: outer merges with inner (outer values override inner values for same keys)
///
/// Completion merging follows quarto-cli's setBaseSchemaProperties:
/// 1. Start with inner.completions
/// 2. Append inner.additional_completions
/// 3. Append outer.additional_completions
/// 4. If outer.completions exists, it overwrites everything
pub(super) fn merge_annotations(
    inner: SchemaAnnotations,
    outer: SchemaAnnotations,
) -> SchemaAnnotations {
    // Merge completions according to quarto-cli semantics
    let mut merged_completions = inner.completions.unwrap_or_default();

    // Add inner additional completions
    if let Some(add_comp) = inner.additional_completions {
        merged_completions.extend(add_comp);
    }

    // Add outer additional completions
    if let Some(add_comp) = &outer.additional_completions {
        merged_completions.extend(add_comp.iter().cloned());
    }

    // Outer completions overwrites everything if present
    let final_completions = if outer.completions.is_some() {
        outer.completions
    } else if !merged_completions.is_empty() {
        Some(merged_completions)
    } else {
        None
    };

    SchemaAnnotations {
        id: outer.id.or(inner.id),
        description: outer.description.or(inner.description),
        documentation: outer.documentation.or(inner.documentation),
        error_message: outer.error_message.or(inner.error_message),
        hidden: outer.hidden.or(inner.hidden),
        completions: final_completions,
        additional_completions: None, // Clear after merging
        tags: merge_tags(inner.tags, outer.tags),
    }
}

/// Merge tag maps, with outer tags overriding inner tags for the same key
fn merge_tags(
    inner: Option<HashMap<String, serde_json::Value>>,
    outer: Option<HashMap<String, serde_json::Value>>,
) -> Option<HashMap<String, serde_json::Value>> {
    match (inner, outer) {
        (None, None) => None,
        (Some(i), None) => Some(i),
        (None, Some(o)) => Some(o),
        (Some(mut i), Some(o)) => {
            // Outer tags override inner tags for same keys
            for (k, v) in o {
                i.insert(k, v);
            }
            Some(i)
        }
    }
}
