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

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_yaml::YamlWithSourceInfo;
    use yaml_rust2::Yaml;
    use yaml_rust2::yaml::Hash;

    fn source_info() -> quarto_yaml::SourceInfo {
        quarto_yaml::SourceInfo::default()
    }

    // ==================== parse_anyof_schema tests ====================

    #[test]
    fn test_anyof_inline_array_valid() {
        // anyOf: [string, boolean]
        let yaml = quarto_yaml::parse(
            r#"
anyOf:
  - string
  - boolean
"#,
        )
        .unwrap();

        let anyof_value = yaml.get_hash_value("anyOf").unwrap();
        let result = parse_anyof_schema(anyof_value).unwrap();

        if let Schema::AnyOf(s) = result {
            assert_eq!(s.schemas.len(), 2);
        } else {
            panic!("Expected AnyOf schema");
        }
    }

    #[test]
    fn test_anyof_explicit_form_valid() {
        // anyOf: { schemas: [string, boolean], description: "test" }
        let yaml = quarto_yaml::parse(
            r#"
anyOf:
  schemas:
    - string
    - boolean
  description: "Either string or boolean"
"#,
        )
        .unwrap();

        let anyof_value = yaml.get_hash_value("anyOf").unwrap();
        let result = parse_anyof_schema(anyof_value).unwrap();

        if let Schema::AnyOf(s) = result {
            assert_eq!(s.schemas.len(), 2);
            assert_eq!(
                s.annotations.description,
                Some("Either string or boolean".to_string())
            );
        } else {
            panic!("Expected AnyOf schema");
        }
    }

    #[test]
    fn test_anyof_schemas_not_array_error() {
        // anyOf: { schemas: "not an array" }
        let yaml = quarto_yaml::parse(
            r#"
anyOf:
  schemas: "not an array"
"#,
        )
        .unwrap();

        let anyof_value = yaml.get_hash_value("anyOf").unwrap();
        let result = parse_anyof_schema(anyof_value);

        assert!(result.is_err());
        let err = result.unwrap_err();
        match err {
            SchemaError::InvalidStructure { message, .. } => {
                assert!(message.contains("anyOf schemas must be an array"));
            }
            _ => panic!("Expected InvalidStructure error"),
        }
    }

    #[test]
    fn test_anyof_inline_not_array_error() {
        // anyOf: "not an array" (inline form but not an array)
        // This requires constructing a YAML where anyOf points to a scalar
        let mut hash = Hash::new();
        hash.insert(
            Yaml::String("anyOf".to_string()),
            Yaml::String("not an array".to_string()),
        );

        let key_node =
            YamlWithSourceInfo::new_scalar(Yaml::String("anyOf".to_string()), source_info());
        let value_node =
            YamlWithSourceInfo::new_scalar(Yaml::String("not an array".to_string()), source_info());

        let entry = quarto_yaml::YamlHashEntry::new(
            key_node,
            value_node,
            source_info(),
            source_info(),
            source_info(),
        );

        let yaml = YamlWithSourceInfo::new_hash(Yaml::Hash(hash), source_info(), vec![entry]);
        let anyof_value = yaml.get_hash_value("anyOf").unwrap();

        let result = parse_anyof_schema(anyof_value);

        assert!(result.is_err());
        let err = result.unwrap_err();
        match err {
            SchemaError::InvalidStructure { message, .. } => {
                assert!(message.contains("Expected array for anyOf"));
            }
            _ => panic!("Expected InvalidStructure error"),
        }
    }

    // ==================== parse_allof_schema tests ====================

    #[test]
    fn test_allof_inline_array_valid() {
        // allOf: [string, number]
        let yaml = quarto_yaml::parse(
            r#"
allOf:
  - string
  - number
"#,
        )
        .unwrap();

        let allof_value = yaml.get_hash_value("allOf").unwrap();
        let result = parse_allof_schema(allof_value).unwrap();

        if let Schema::AllOf(s) = result {
            assert_eq!(s.schemas.len(), 2);
        } else {
            panic!("Expected AllOf schema");
        }
    }

    #[test]
    fn test_allof_explicit_form_valid() {
        // allOf: { schemas: [string, number], description: "test" }
        let yaml = quarto_yaml::parse(
            r#"
allOf:
  schemas:
    - string
    - number
  description: "Must match both string and number constraints"
"#,
        )
        .unwrap();

        let allof_value = yaml.get_hash_value("allOf").unwrap();
        let result = parse_allof_schema(allof_value).unwrap();

        if let Schema::AllOf(s) = result {
            assert_eq!(s.schemas.len(), 2);
            assert_eq!(
                s.annotations.description,
                Some("Must match both string and number constraints".to_string())
            );
        } else {
            panic!("Expected AllOf schema");
        }
    }

    #[test]
    fn test_allof_schemas_not_array_error() {
        // allOf: { schemas: "not an array" }
        let yaml = quarto_yaml::parse(
            r#"
allOf:
  schemas: "not an array"
"#,
        )
        .unwrap();

        let allof_value = yaml.get_hash_value("allOf").unwrap();
        let result = parse_allof_schema(allof_value);

        assert!(result.is_err());
        let err = result.unwrap_err();
        match err {
            SchemaError::InvalidStructure { message, .. } => {
                assert!(message.contains("allOf schemas must be an array"));
            }
            _ => panic!("Expected InvalidStructure error"),
        }
    }

    #[test]
    fn test_allof_inline_not_array_error() {
        // allOf: "not an array" (inline form but not an array)
        let mut hash = Hash::new();
        hash.insert(
            Yaml::String("allOf".to_string()),
            Yaml::String("not an array".to_string()),
        );

        let key_node =
            YamlWithSourceInfo::new_scalar(Yaml::String("allOf".to_string()), source_info());
        let value_node =
            YamlWithSourceInfo::new_scalar(Yaml::String("not an array".to_string()), source_info());

        let entry = quarto_yaml::YamlHashEntry::new(
            key_node,
            value_node,
            source_info(),
            source_info(),
            source_info(),
        );

        let yaml = YamlWithSourceInfo::new_hash(Yaml::Hash(hash), source_info(), vec![entry]);
        let allof_value = yaml.get_hash_value("allOf").unwrap();

        let result = parse_allof_schema(allof_value);

        assert!(result.is_err());
        let err = result.unwrap_err();
        match err {
            SchemaError::InvalidStructure { message, .. } => {
                assert!(message.contains("Expected array for allOf"));
            }
            _ => panic!("Expected InvalidStructure error"),
        }
    }

    // ==================== parse_maybe_arrayof_schema tests ====================

    #[test]
    fn test_maybe_arrayof_basic() {
        // maybeArrayOf: string -> anyOf([string, arrayOf(string)])
        let yaml = quarto_yaml::parse("maybeArrayOf: string").unwrap();
        let maybearray_value = yaml.get_hash_value("maybeArrayOf").unwrap();

        let result = parse_maybe_arrayof_schema(maybearray_value).unwrap();

        if let Schema::AnyOf(s) = result {
            assert_eq!(s.schemas.len(), 2);
            // First schema should be string
            assert!(matches!(s.schemas[0], Schema::String(_)));
            // Second schema should be array of string
            assert!(matches!(s.schemas[1], Schema::Array(_)));
        } else {
            panic!("Expected AnyOf schema");
        }
    }

    #[test]
    fn test_maybe_arrayof_has_complete_from_tag() {
        // maybeArrayOf should add "complete-from": ["anyOf", 0] tag
        let yaml = quarto_yaml::parse("maybeArrayOf: boolean").unwrap();
        let maybearray_value = yaml.get_hash_value("maybeArrayOf").unwrap();

        let result = parse_maybe_arrayof_schema(maybearray_value).unwrap();

        if let Schema::AnyOf(s) = result {
            assert!(s.annotations.tags.is_some());
            let tags = s.annotations.tags.as_ref().unwrap();
            assert!(tags.contains_key("complete-from"));
            assert_eq!(
                tags.get("complete-from"),
                Some(&serde_json::json!(["anyOf", 0]))
            );
        } else {
            panic!("Expected AnyOf schema");
        }
    }

    #[test]
    fn test_maybe_arrayof_with_complex_schema() {
        // maybeArrayOf with a more complex inner schema (number with constraints)
        let yaml = quarto_yaml::parse(
            r#"
maybeArrayOf:
  number:
    minimum: 0
"#,
        )
        .unwrap();
        let maybearray_value = yaml.get_hash_value("maybeArrayOf").unwrap();

        let result = parse_maybe_arrayof_schema(maybearray_value).unwrap();

        if let Schema::AnyOf(s) = result {
            assert_eq!(s.schemas.len(), 2);
            // First schema should be a number schema
            assert!(matches!(s.schemas[0], Schema::Number(_)));
            // Second schema should be array
            if let Schema::Array(arr) = &s.schemas[1] {
                // Array items should be the same number schema
                assert!(arr.items.is_some());
            } else {
                panic!("Expected Array schema as second option");
            }
        } else {
            panic!("Expected AnyOf schema");
        }
    }

    #[test]
    fn test_maybe_arrayof_array_schema_has_no_constraints() {
        // The arrayOf part should have no min/max items or uniqueItems constraints
        let yaml = quarto_yaml::parse("maybeArrayOf: string").unwrap();
        let maybearray_value = yaml.get_hash_value("maybeArrayOf").unwrap();

        let result = parse_maybe_arrayof_schema(maybearray_value).unwrap();

        if let Schema::AnyOf(s) = result {
            if let Schema::Array(arr) = &s.schemas[1] {
                assert!(arr.min_items.is_none());
                assert!(arr.max_items.is_none());
                assert!(arr.unique_items.is_none());
            } else {
                panic!("Expected Array schema");
            }
        } else {
            panic!("Expected AnyOf schema");
        }
    }
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
    tags.insert("complete-from".to_string(), serde_json::json!(["anyOf", 0]));

    let annotations = SchemaAnnotations {
        tags: Some(tags),
        ..Default::default()
    };

    Ok(Schema::AnyOf(AnyOfSchema {
        annotations,
        schemas,
    }))
}
