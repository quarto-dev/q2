//! Schema merging logic for inheritance
//!
//! Implements quarto-cli's schema inheritance semantics when combining
//! base schemas with derived schemas via the `super` field.
//!
//! This module provides the `merge_object_schemas()` function which merges
//! base object schemas with a derived object schema according to quarto-cli's
//! merging rules (from common.ts:221-403).

use crate::error::{SchemaError, SchemaResult};
use crate::schema::types::{AllOfSchema, AnyOfSchema, ObjectSchema, SchemaAnnotations};
use crate::schema::{Schema, SchemaRegistry};
use std::collections::HashMap;

/// Resolve a base schema reference if it's an eager ref
///
/// Returns the resolved schema if it's a Ref with eager=true,
/// otherwise returns the schema as-is.
fn resolve_base_schema(schema: &Schema, registry: &SchemaRegistry) -> SchemaResult<Schema> {
    match schema {
        Schema::Ref(ref_schema) if ref_schema.eager => {
            // Eager resolution - look up in registry
            registry
                .resolve(&ref_schema.reference)
                .cloned()
                .ok_or_else(|| SchemaError::InvalidStructure {
                    message: format!(
                        "Cannot resolve reference '{}' - not found in registry",
                        ref_schema.reference
                    ),
                    // Schema structure error - not tied to specific source location
                    location: quarto_yaml::SourceInfo::default(),
                })
        }
        _ => Ok(schema.clone()),
    }
}

/// Validate that a schema is an ObjectSchema
///
/// Returns the ObjectSchema if valid, error otherwise
fn expect_object_schema(schema: &Schema) -> SchemaResult<&ObjectSchema> {
    match schema {
        Schema::Object(obj) => Ok(obj),
        _ => Err(SchemaError::InvalidStructure {
            message: format!(
                "Base schema must be an object schema, got {}",
                schema.type_name()
            ),
            // Schema structure error - not tied to specific source location
            location: quarto_yaml::SourceInfo::default(),
        }),
    }
}

/// Merge base schemas with derived schema
///
/// Implements quarto-cli's objectSchema() merging logic from common.ts:221-403
///
/// # Arguments
/// * `base_schemas` - List of base schemas (may contain unresolved refs)
/// * `derived` - The derived object schema
/// * `registry` - Schema registry for resolving references
///
/// # Returns
/// A new ObjectSchema with merged properties
pub fn merge_object_schemas(
    base_schemas: &[Schema],
    derived: &ObjectSchema,
    registry: &SchemaRegistry,
) -> SchemaResult<ObjectSchema> {
    // Resolve all base schema references
    let resolved_bases: SchemaResult<Vec<_>> = base_schemas
        .iter()
        .map(|s| resolve_base_schema(s, registry))
        .collect();
    let resolved_bases = resolved_bases?;

    // Validate all are object schemas
    let base_objects: SchemaResult<Vec<_>> =
        resolved_bases.iter().map(expect_object_schema).collect();
    let base_objects = base_objects?;

    if base_objects.is_empty() {
        return Err(SchemaError::InvalidStructure {
            message: "base schema cannot be empty list".to_string(),
            // Schema structure error - not tied to specific source location
            location: quarto_yaml::SourceInfo::default(),
        });
    }

    // Start with annotations from first base schema
    let mut result = ObjectSchema {
        annotations: base_objects[0].annotations.clone(),
        properties: HashMap::new(),
        pattern_properties: HashMap::new(),
        additional_properties: None,
        required: Vec::new(),
        min_properties: None,
        max_properties: None,
        closed: false,
        property_names: None,
        naming_convention: None,
        base_schema: None, // Don't propagate base_schema (already merged)
    };

    // Apply remaining base schema annotations (later bases override earlier)
    for base in base_objects.iter().skip(1) {
        if base.annotations.id.is_some() {
            result.annotations.id = base.annotations.id.clone();
        }
        if base.annotations.description.is_some() {
            result.annotations.description = base.annotations.description.clone();
        }
        if base.annotations.documentation.is_some() {
            result.annotations.documentation = base.annotations.documentation.clone();
        }
        if base.annotations.error_message.is_some() {
            result.annotations.error_message = base.annotations.error_message.clone();
        }
        if base.annotations.hidden.is_some() {
            result.annotations.hidden = base.annotations.hidden;
        }
        if base.annotations.completions.is_some() {
            result.annotations.completions = base.annotations.completions.clone();
        }
        if base.annotations.additional_completions.is_some() {
            result.annotations.additional_completions =
                base.annotations.additional_completions.clone();
        }
        if base.annotations.tags.is_some() {
            result.annotations.tags = base.annotations.tags.clone();
        }
    }

    // Remove $id to avoid duplicate IDs (quarto-cli line 243-245)
    result.annotations.id = None;

    // Merge properties (base properties first, then derived overrides)
    for base in &base_objects {
        for (key, schema) in &base.properties {
            result.properties.insert(key.clone(), schema.clone());
        }
    }
    for (key, schema) in &derived.properties {
        result.properties.insert(key.clone(), schema.clone());
    }

    // Merge patternProperties
    for base in &base_objects {
        for (key, schema) in &base.pattern_properties {
            result
                .pattern_properties
                .insert(key.clone(), schema.clone());
        }
    }
    for (key, schema) in &derived.pattern_properties {
        result
            .pattern_properties
            .insert(key.clone(), schema.clone());
    }

    // Merge required (flatten all)
    for base in &base_objects {
        result.required.extend(base.required.iter().cloned());
    }
    result.required.extend(derived.required.iter().cloned());

    // Merge additionalProperties using allOf
    let mut additional_props_schemas = Vec::new();
    for base in &base_objects {
        if let Some(ref ap) = base.additional_properties {
            additional_props_schemas.push((**ap).clone());
        }
    }
    if let Some(ref ap) = derived.additional_properties {
        additional_props_schemas.push((**ap).clone());
    }

    result.additional_properties = if additional_props_schemas.is_empty() {
        None
    } else if additional_props_schemas.len() == 1 {
        Some(Box::new(
            additional_props_schemas.into_iter().next().unwrap(),
        ))
    } else {
        // Combine with allOf
        Some(Box::new(Schema::AllOf(AllOfSchema {
            annotations: SchemaAnnotations::default(),
            schemas: additional_props_schemas,
        })))
    };

    // Merge propertyNames using anyOf (but skip case-detection ones)
    let mut property_names_schemas = Vec::new();
    for base in &base_objects {
        if let Some(ref pn) = base.property_names {
            // Check if this is a case-detection schema (has tags.case-detection)
            let is_case_detection = match pn.as_ref() {
                Schema::String(s) => s
                    .annotations
                    .tags
                    .as_ref()
                    .and_then(|tags| tags.get("case-detection"))
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false),
                _ => false,
            };

            if !is_case_detection {
                property_names_schemas.push((**pn).clone());
            }
        }
    }

    result.property_names = if property_names_schemas.is_empty() {
        None
    } else if property_names_schemas.len() == 1 {
        Some(Box::new(property_names_schemas.into_iter().next().unwrap()))
    } else {
        // Combine with anyOf
        Some(Box::new(Schema::AnyOf(AnyOfSchema {
            annotations: SchemaAnnotations::default(),
            schemas: property_names_schemas,
        })))
    };

    // Merge closed (true if ANY base or derived is closed)
    result.closed = base_objects.iter().any(|b| b.closed) || derived.closed;

    // Apply derived-specific fields (override bases)
    if derived.min_properties.is_some() {
        result.min_properties = derived.min_properties;
    }
    if derived.max_properties.is_some() {
        result.max_properties = derived.max_properties;
    }
    if derived.naming_convention.is_some() {
        result.naming_convention = derived.naming_convention.clone();
    }

    // Apply derived description if present (override base)
    if derived.annotations.description.is_some() {
        result.annotations.description = derived.annotations.description.clone();
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::types::*;

    #[test]
    fn test_merge_simple_properties() {
        let registry = SchemaRegistry::new();

        // Create base schema
        let mut base_props = HashMap::new();
        base_props.insert(
            "id".to_string(),
            Schema::String(StringSchema {
                annotations: Default::default(),
                min_length: None,
                max_length: None,
                pattern: None,
            }),
        );

        let base = ObjectSchema {
            annotations: Default::default(),
            properties: base_props,
            pattern_properties: HashMap::new(),
            additional_properties: None,
            required: vec!["id".to_string()],
            min_properties: None,
            max_properties: None,
            closed: false,
            property_names: None,
            naming_convention: None,
            base_schema: None,
        };

        // Create derived schema
        let mut derived_props = HashMap::new();
        derived_props.insert(
            "name".to_string(),
            Schema::String(StringSchema {
                annotations: Default::default(),
                min_length: None,
                max_length: None,
                pattern: None,
            }),
        );

        let derived = ObjectSchema {
            annotations: Default::default(),
            properties: derived_props,
            pattern_properties: HashMap::new(),
            additional_properties: None,
            required: vec!["name".to_string()],
            min_properties: None,
            max_properties: None,
            closed: false,
            property_names: None,
            naming_convention: None,
            base_schema: None,
        };

        // Merge
        let merged = merge_object_schemas(&[Schema::Object(base)], &derived, &registry).unwrap();

        // Verify merged has both properties
        assert!(merged.properties.contains_key("id"));
        assert!(merged.properties.contains_key("name"));
        assert_eq!(merged.required.len(), 2);
        assert!(merged.required.contains(&"id".to_string()));
        assert!(merged.required.contains(&"name".to_string()));
    }

    #[test]
    fn test_merge_with_ref() {
        let mut registry = SchemaRegistry::new();

        // Register base schema
        let mut base_props = HashMap::new();
        base_props.insert(
            "base_field".to_string(),
            Schema::String(StringSchema {
                annotations: Default::default(),
                min_length: None,
                max_length: None,
                pattern: None,
            }),
        );

        let base = Schema::Object(ObjectSchema {
            annotations: SchemaAnnotations {
                id: Some("base-schema".to_string()),
                ..Default::default()
            },
            properties: base_props,
            pattern_properties: HashMap::new(),
            additional_properties: None,
            required: vec!["base_field".to_string()],
            min_properties: None,
            max_properties: None,
            closed: false,
            property_names: None,
            naming_convention: None,
            base_schema: None,
        });

        registry.register("base-schema".to_string(), base);

        // Create ref to base
        let base_ref = Schema::Ref(RefSchema {
            annotations: Default::default(),
            reference: "base-schema".to_string(),
            eager: true,
        });

        // Create derived
        let mut derived_props = HashMap::new();
        derived_props.insert(
            "derived_field".to_string(),
            Schema::Boolean(BooleanSchema {
                annotations: Default::default(),
            }),
        );

        let derived = ObjectSchema {
            annotations: Default::default(),
            properties: derived_props,
            pattern_properties: HashMap::new(),
            additional_properties: None,
            required: Vec::new(),
            min_properties: None,
            max_properties: None,
            closed: false,
            property_names: None,
            naming_convention: None,
            base_schema: None,
        };

        // Merge
        let merged = merge_object_schemas(&[base_ref], &derived, &registry).unwrap();

        // Verify
        assert!(merged.properties.contains_key("base_field"));
        assert!(merged.properties.contains_key("derived_field"));
        assert_eq!(merged.required.len(), 1);
        assert_eq!(merged.required[0], "base_field");
    }

    #[test]
    fn test_property_override() {
        let registry = SchemaRegistry::new();

        // Base has 'name' as string with no constraints
        let mut base_props = HashMap::new();
        base_props.insert(
            "name".to_string(),
            Schema::String(StringSchema {
                annotations: SchemaAnnotations {
                    description: Some("Base description".to_string()),
                    ..Default::default()
                },
                min_length: None,
                max_length: None,
                pattern: None,
            }),
        );

        let base = ObjectSchema {
            annotations: Default::default(),
            properties: base_props,
            pattern_properties: HashMap::new(),
            additional_properties: None,
            required: Vec::new(),
            min_properties: None,
            max_properties: None,
            closed: false,
            property_names: None,
            naming_convention: None,
            base_schema: None,
        };

        // Derived overrides 'name' with pattern
        let mut derived_props = HashMap::new();
        derived_props.insert(
            "name".to_string(),
            Schema::String(StringSchema {
                annotations: SchemaAnnotations {
                    description: Some("Derived description".to_string()),
                    ..Default::default()
                },
                min_length: None,
                max_length: None,
                pattern: Some("^[A-Z]".to_string()),
            }),
        );

        let derived = ObjectSchema {
            annotations: Default::default(),
            properties: derived_props,
            pattern_properties: HashMap::new(),
            additional_properties: None,
            required: Vec::new(),
            min_properties: None,
            max_properties: None,
            closed: false,
            property_names: None,
            naming_convention: None,
            base_schema: None,
        };

        // Merge
        let merged = merge_object_schemas(&[Schema::Object(base)], &derived, &registry).unwrap();

        // Derived should win
        match merged.properties.get("name") {
            Some(Schema::String(s)) => {
                assert_eq!(s.pattern, Some("^[A-Z]".to_string()));
                assert_eq!(
                    s.annotations.description,
                    Some("Derived description".to_string())
                );
            }
            _ => panic!("Expected string schema for name"),
        }
    }

    #[test]
    fn test_closed_inheritance() {
        let registry = SchemaRegistry::new();

        // Base is closed
        let base = ObjectSchema {
            annotations: Default::default(),
            properties: HashMap::new(),
            pattern_properties: HashMap::new(),
            additional_properties: None,
            required: Vec::new(),
            min_properties: None,
            max_properties: None,
            closed: true,
            property_names: None,
            naming_convention: None,
            base_schema: None,
        };

        // Derived is not closed
        let derived = ObjectSchema {
            annotations: Default::default(),
            properties: HashMap::new(),
            pattern_properties: HashMap::new(),
            additional_properties: None,
            required: Vec::new(),
            min_properties: None,
            max_properties: None,
            closed: false,
            property_names: None,
            naming_convention: None,
            base_schema: None,
        };

        // Merge - should be closed
        let merged = merge_object_schemas(&[Schema::Object(base)], &derived, &registry).unwrap();

        assert!(merged.closed);
    }

    #[test]
    fn test_missing_reference_error() {
        let registry = SchemaRegistry::new(); // Empty

        let base_ref = Schema::Ref(RefSchema {
            annotations: Default::default(),
            reference: "non-existent".to_string(),
            eager: true,
        });

        let derived = ObjectSchema {
            annotations: Default::default(),
            properties: HashMap::new(),
            pattern_properties: HashMap::new(),
            additional_properties: None,
            required: Vec::new(),
            min_properties: None,
            max_properties: None,
            closed: false,
            property_names: None,
            naming_convention: None,
            base_schema: None,
        };

        let result = merge_object_schemas(&[base_ref], &derived, &registry);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("not found in registry")
        );
    }

    #[test]
    fn test_non_object_base_error() {
        let mut registry = SchemaRegistry::new();

        // Register a STRING schema (not object)
        registry.register(
            "not-object".to_string(),
            Schema::String(StringSchema {
                annotations: Default::default(),
                min_length: None,
                max_length: None,
                pattern: None,
            }),
        );

        let base_ref = Schema::Ref(RefSchema {
            annotations: Default::default(),
            reference: "not-object".to_string(),
            eager: true,
        });

        let derived = ObjectSchema {
            annotations: Default::default(),
            properties: HashMap::new(),
            pattern_properties: HashMap::new(),
            additional_properties: None,
            required: Vec::new(),
            min_properties: None,
            max_properties: None,
            closed: false,
            property_names: None,
            naming_convention: None,
            base_schema: None,
        };

        let result = merge_object_schemas(&[base_ref], &derived, &registry);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("must be an object schema")
        );
    }

    #[test]
    fn test_empty_base_list_error() {
        let registry = SchemaRegistry::new();

        let derived = ObjectSchema {
            annotations: Default::default(),
            properties: HashMap::new(),
            pattern_properties: HashMap::new(),
            additional_properties: None,
            required: Vec::new(),
            min_properties: None,
            max_properties: None,
            closed: false,
            property_names: None,
            naming_convention: None,
            base_schema: None,
        };

        let result = merge_object_schemas(&[], &derived, &registry);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("cannot be empty list")
        );
    }
}
