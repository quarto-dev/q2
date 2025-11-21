//! Schema types for YAML validation
//!
//! This module defines the schema type system used for validation,
//! closely matching Quarto's simplified JSON Schema subset.
//!
//! IMPORTANT: This module does NOT use serde deserialization for loading schemas
//! from YAML because serde_yaml only supports YAML 1.1. We need YAML 1.2 support
//! for consistency with user documents and to support Quarto extensions.
//! See ../YAML-1.2-REQUIREMENT.md for details.
//!
//! Instead, schemas are parsed from YamlWithSourceInfo (quarto-yaml) which uses
//! yaml-rust2 (YAML 1.2). See Schema::from_yaml() method below.

use crate::error::SchemaResult;
use quarto_yaml::YamlWithSourceInfo;
use std::collections::HashMap;

// Internal modules
mod annotations;
mod helpers;
mod merge;
mod parser;
mod parsers;
mod types;

// Public re-exports
pub use merge::merge_object_schemas;
pub use types::{
    AllOfSchema, AnyOfSchema, AnySchema, ArraySchema, BooleanSchema, EnumSchema, NamingConvention,
    NullSchema, NumberSchema, ObjectSchema, RefSchema, SchemaAnnotations, StringSchema,
};

use annotations::EMPTY_ANNOTATIONS;

/// The main schema enum representing all possible schema types
#[derive(Debug, Clone, PartialEq)]
pub enum Schema {
    /// Always fails validation
    False,
    /// Always passes validation
    True,
    /// Boolean type schema
    Boolean(BooleanSchema),
    /// Number type schema (integer or float)
    Number(NumberSchema),
    /// String type schema
    String(StringSchema),
    /// Null type schema
    Null(NullSchema),
    /// Enum type schema (fixed set of values)
    Enum(EnumSchema),
    /// Any type schema (no validation)
    Any(AnySchema),
    /// AnyOf schema (validates if any subschema matches)
    AnyOf(AnyOfSchema),
    /// AllOf schema (validates if all subschemas match)
    AllOf(AllOfSchema),
    /// Array type schema
    Array(ArraySchema),
    /// Object type schema
    Object(ObjectSchema),
    /// Reference to another schema
    Ref(RefSchema),
}

impl Schema {
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
    pub fn from_yaml(yaml: &YamlWithSourceInfo) -> SchemaResult<Schema> {
        parser::from_yaml(yaml)
    }

    /// Get the annotations for this schema
    pub fn annotations(&self) -> &SchemaAnnotations {
        match self {
            Schema::False | Schema::True => &EMPTY_ANNOTATIONS,
            Schema::Boolean(s) => &s.annotations,
            Schema::Number(s) => &s.annotations,
            Schema::String(s) => &s.annotations,
            Schema::Null(s) => &s.annotations,
            Schema::Enum(s) => &s.annotations,
            Schema::Any(s) => &s.annotations,
            Schema::AnyOf(s) => &s.annotations,
            Schema::AllOf(s) => &s.annotations,
            Schema::Array(s) => &s.annotations,
            Schema::Object(s) => &s.annotations,
            Schema::Ref(s) => &s.annotations,
        }
    }

    /// Get a mutable reference to the annotations for this schema
    pub fn annotations_mut(&mut self) -> Option<&mut SchemaAnnotations> {
        match self {
            Schema::False | Schema::True => None,
            Schema::Boolean(s) => Some(&mut s.annotations),
            Schema::Number(s) => Some(&mut s.annotations),
            Schema::String(s) => Some(&mut s.annotations),
            Schema::Null(s) => Some(&mut s.annotations),
            Schema::Enum(s) => Some(&mut s.annotations),
            Schema::Any(s) => Some(&mut s.annotations),
            Schema::AnyOf(s) => Some(&mut s.annotations),
            Schema::AllOf(s) => Some(&mut s.annotations),
            Schema::Array(s) => Some(&mut s.annotations),
            Schema::Object(s) => Some(&mut s.annotations),
            Schema::Ref(s) => Some(&mut s.annotations),
        }
    }

    /// Replace annotations for this schema, returning the modified schema
    ///
    /// # Panics
    ///
    /// Panics if called on False or True schemas, as they don't support annotations.
    pub(crate) fn with_annotations(mut self, annotations: SchemaAnnotations) -> Self {
        match &mut self {
            Schema::False => panic!("Cannot set annotations on Schema::False"),
            Schema::True => panic!("Cannot set annotations on Schema::True"),
            Schema::Boolean(s) => s.annotations = annotations,
            Schema::Number(s) => s.annotations = annotations,
            Schema::String(s) => s.annotations = annotations,
            Schema::Null(s) => s.annotations = annotations,
            Schema::Enum(s) => s.annotations = annotations,
            Schema::Any(s) => s.annotations = annotations,
            Schema::AnyOf(s) => s.annotations = annotations,
            Schema::AllOf(s) => s.annotations = annotations,
            Schema::Array(s) => s.annotations = annotations,
            Schema::Object(s) => s.annotations = annotations,
            Schema::Ref(s) => s.annotations = annotations,
        }
        self
    }

    /// Get a human-readable name for this schema type
    pub fn type_name(&self) -> &'static str {
        match self {
            Schema::False => "false",
            Schema::True => "true",
            Schema::Boolean(_) => "boolean",
            Schema::Number(_) => "number",
            Schema::String(_) => "string",
            Schema::Null(_) => "null",
            Schema::Enum(_) => "enum",
            Schema::Any(_) => "any",
            Schema::AnyOf(_) => "anyOf",
            Schema::AllOf(_) => "allOf",
            Schema::Array(_) => "array",
            Schema::Object(_) => "object",
            Schema::Ref(_) => "$ref",
        }
    }

    /// Compile a schema by resolving eager references and merging inheritance.
    ///
    /// This creates a structurally complete schema suitable for validation.
    /// Lazy references (eager=false) are kept as references and resolved
    /// during validation to support circular dependencies.
    ///
    /// # Two-Phase Processing
    ///
    /// Schemas go through two phases:
    /// 1. **Parsing** (stateless, no registry): YAML → Schema AST
    /// 2. **Compilation** (with registry): Schema AST → Compiled Schema
    ///
    /// Compilation resolves:
    /// - Eager references (`resolveRef`, `eager: true`) - must resolve for schema completeness
    /// - Object inheritance (`base_schema`) - merges properties from base schemas
    /// - Nested schemas recursively
    ///
    /// Compilation preserves:
    /// - Lazy references (`ref`, `eager: false`) - resolved during validation
    ///
    /// # Arguments
    /// * `registry` - Schema registry for resolving references
    ///
    /// # Returns
    /// A compiled schema with all eager references resolved and inheritance merged
    ///
    /// # Errors
    /// Returns error if:
    /// - An eager reference cannot be resolved (not in registry)
    /// - Base schema is not an ObjectSchema
    /// - Circular eager references detected (future enhancement)
    ///
    /// # Example
    ///
    /// ```
    /// use quarto_yaml_validation::{Schema, SchemaRegistry};
    /// use quarto_yaml;
    ///
    /// let mut registry = SchemaRegistry::new();
    ///
    /// // Parse and register base schema
    /// let base_yaml = quarto_yaml::parse(r#"
    /// object:
    ///   properties:
    ///     id: string
    /// "#).unwrap();
    /// let base = Schema::from_yaml(&base_yaml).unwrap();
    /// registry.register("base".to_string(), base);
    ///
    /// // Parse derived schema with inheritance
    /// let derived_yaml = quarto_yaml::parse(r#"
    /// object:
    ///   super:
    ///     resolveRef: base
    ///   properties:
    ///     name: string
    /// "#).unwrap();
    /// let derived = Schema::from_yaml(&derived_yaml).unwrap();
    ///
    /// // Compile - merges base and derived
    /// let compiled = derived.compile(&registry).unwrap();
    ///
    /// // Compiled schema now has both 'id' and 'name' properties
    /// ```
    pub fn compile(&self, registry: &SchemaRegistry) -> SchemaResult<Schema> {
        match self {
            // Object with inheritance - must merge base schemas
            Schema::Object(obj) if obj.base_schema.is_some() => {
                // Compile base schemas first (recursive)
                let base_schemas = obj.base_schema.as_ref().unwrap();
                let compiled_bases: SchemaResult<Vec<_>> =
                    base_schemas.iter().map(|s| s.compile(registry)).collect();
                let compiled_bases = compiled_bases?;

                // Merge with derived schema
                let merged = merge_object_schemas(&compiled_bases, obj, registry)?;

                // Result has no base_schema (it's been merged)
                Ok(Schema::Object(merged))
            }

            // Eager reference - must resolve now
            Schema::Ref(r) if r.eager => {
                let resolved = registry.resolve(&r.reference).ok_or_else(|| {
                    crate::error::SchemaError::InvalidStructure {
                        message: format!(
                            "Cannot resolve eager reference '{}' - not found in registry",
                            r.reference
                        ),
                        // Schema structure error - not tied to specific source location
                        location: quarto_yaml::SourceInfo::default(),
                    }
                })?;

                // Recursively compile the resolved schema
                resolved.compile(registry)
            }

            // Lazy reference - keep as is for validation time
            Schema::Ref(_) => Ok(self.clone()),

            // Recursively compile nested schemas in containers
            Schema::AnyOf(anyof) => {
                let compiled_schemas: SchemaResult<Vec<_>> =
                    anyof.schemas.iter().map(|s| s.compile(registry)).collect();
                Ok(Schema::AnyOf(AnyOfSchema {
                    annotations: anyof.annotations.clone(),
                    schemas: compiled_schemas?,
                }))
            }

            Schema::AllOf(allof) => {
                let compiled_schemas: SchemaResult<Vec<_>> =
                    allof.schemas.iter().map(|s| s.compile(registry)).collect();
                Ok(Schema::AllOf(AllOfSchema {
                    annotations: allof.annotations.clone(),
                    schemas: compiled_schemas?,
                }))
            }

            Schema::Array(arr) => {
                let compiled_items = if let Some(items) = &arr.items {
                    Some(Box::new(items.compile(registry)?))
                } else {
                    None
                };
                Ok(Schema::Array(ArraySchema {
                    annotations: arr.annotations.clone(),
                    items: compiled_items,
                    min_items: arr.min_items,
                    max_items: arr.max_items,
                    unique_items: arr.unique_items,
                }))
            }

            Schema::Object(obj) => {
                // Object without inheritance - compile nested property schemas
                let mut compiled_properties = HashMap::new();
                for (key, prop_schema) in &obj.properties {
                    compiled_properties.insert(key.clone(), prop_schema.compile(registry)?);
                }

                let mut compiled_pattern_properties = HashMap::new();
                for (pattern, prop_schema) in &obj.pattern_properties {
                    compiled_pattern_properties
                        .insert(pattern.clone(), prop_schema.compile(registry)?);
                }

                let compiled_additional = if let Some(ap) = &obj.additional_properties {
                    Some(Box::new(ap.compile(registry)?))
                } else {
                    None
                };

                let compiled_property_names = if let Some(pn) = &obj.property_names {
                    Some(Box::new(pn.compile(registry)?))
                } else {
                    None
                };

                Ok(Schema::Object(ObjectSchema {
                    annotations: obj.annotations.clone(),
                    properties: compiled_properties,
                    pattern_properties: compiled_pattern_properties,
                    additional_properties: compiled_additional,
                    required: obj.required.clone(),
                    min_properties: obj.min_properties,
                    max_properties: obj.max_properties,
                    closed: obj.closed,
                    property_names: compiled_property_names,
                    naming_convention: obj.naming_convention.clone(),
                    base_schema: None, // No inheritance at this level
                }))
            }

            // Primitives don't need compilation
            Schema::False
            | Schema::True
            | Schema::Boolean(_)
            | Schema::Number(_)
            | Schema::String(_)
            | Schema::Null(_)
            | Schema::Enum(_)
            | Schema::Any(_) => Ok(self.clone()),
        }
    }
}

/// Schema registry for managing schemas with $ref resolution
#[derive(Debug, Default)]
pub struct SchemaRegistry {
    schemas: HashMap<String, Schema>,
}

impl SchemaRegistry {
    /// Create a new empty schema registry
    pub fn new() -> Self {
        Self {
            schemas: HashMap::new(),
        }
    }

    /// Register a schema with an ID
    pub fn register(&mut self, id: String, schema: Schema) {
        self.schemas.insert(id, schema);
    }

    /// Resolve a schema reference
    pub fn resolve(&self, reference: &str) -> Option<&Schema> {
        self.schemas.get(reference)
    }

    /// Get all registered schema IDs
    pub fn ids(&self) -> impl Iterator<Item = &String> {
        self.schemas.keys()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_schema_type_name() {
        assert_eq!(Schema::False.type_name(), "false");
        assert_eq!(Schema::True.type_name(), "true");
        assert_eq!(
            Schema::Boolean(BooleanSchema {
                annotations: Default::default()
            })
            .type_name(),
            "boolean"
        );
    }

    #[test]
    fn test_schema_registry() {
        let mut registry = SchemaRegistry::new();
        let schema = Schema::Boolean(BooleanSchema {
            annotations: SchemaAnnotations {
                id: Some("test-bool".to_string()),
                ..Default::default()
            },
        });

        registry.register("test-bool".to_string(), schema.clone());

        let resolved = registry.resolve("test-bool");
        assert!(resolved.is_some());
        assert_eq!(resolved.unwrap(), &schema);
    }

    // Tests for Schema::from_yaml()

    #[test]
    fn test_from_yaml_boolean_short() {
        let yaml = quarto_yaml::parse("boolean").unwrap();
        let schema = Schema::from_yaml(&yaml).unwrap();
        assert!(matches!(schema, Schema::Boolean(_)));
        assert_eq!(schema.type_name(), "boolean");
    }

    #[test]
    fn test_from_yaml_boolean_long() {
        let yaml = quarto_yaml::parse(
            r#"
boolean:
  description: "A boolean value"
"#,
        )
        .unwrap();
        let schema = Schema::from_yaml(&yaml).unwrap();
        if let Schema::Boolean(s) = schema {
            assert_eq!(
                s.annotations.description,
                Some("A boolean value".to_string())
            );
        } else {
            panic!("Expected Boolean schema");
        }
    }

    #[test]
    fn test_from_yaml_number_short() {
        let yaml = quarto_yaml::parse("number").unwrap();
        let schema = Schema::from_yaml(&yaml).unwrap();
        assert_eq!(schema.type_name(), "number");
    }

    #[test]
    fn test_from_yaml_number_long() {
        let yaml = quarto_yaml::parse(
            r#"
number:
  minimum: 0
  maximum: 100
  description: "A percentage"
"#,
        )
        .unwrap();
        let schema = Schema::from_yaml(&yaml).unwrap();
        if let Schema::Number(s) = schema {
            assert_eq!(s.minimum, Some(0.0));
            assert_eq!(s.maximum, Some(100.0));
            assert_eq!(s.annotations.description, Some("A percentage".to_string()));
        } else {
            panic!("Expected Number schema");
        }
    }

    #[test]
    fn test_from_yaml_string_short() {
        let yaml = quarto_yaml::parse("string").unwrap();
        let schema = Schema::from_yaml(&yaml).unwrap();
        assert_eq!(schema.type_name(), "string");
    }

    #[test]
    fn test_from_yaml_path() {
        let yaml = quarto_yaml::parse("path").unwrap();
        let schema = Schema::from_yaml(&yaml).unwrap();
        assert_eq!(schema.type_name(), "string");
    }

    #[test]
    fn test_from_yaml_string_long() {
        let yaml = quarto_yaml::parse(
            r#"
string:
  pattern: "^[a-z]+$"
  minLength: 1
  maxLength: 50
"#,
        )
        .unwrap();
        let schema = Schema::from_yaml(&yaml).unwrap();
        if let Schema::String(s) = schema {
            assert_eq!(s.pattern, Some("^[a-z]+$".to_string()));
            assert_eq!(s.min_length, Some(1));
            assert_eq!(s.max_length, Some(50));
        } else {
            panic!("Expected String schema");
        }
    }

    #[test]
    fn test_from_yaml_null() {
        let yaml = quarto_yaml::parse("null").unwrap();
        let schema = Schema::from_yaml(&yaml).unwrap();
        assert_eq!(schema.type_name(), "null");
    }

    #[test]
    fn test_from_yaml_any() {
        let yaml = quarto_yaml::parse("any").unwrap();
        let schema = Schema::from_yaml(&yaml).unwrap();
        assert_eq!(schema.type_name(), "any");
    }

    #[test]
    fn test_from_yaml_enum_inline() {
        let yaml = quarto_yaml::parse(
            r#"
enum: [foo, bar, baz]
"#,
        )
        .unwrap();
        let schema = Schema::from_yaml(&yaml).unwrap();
        if let Schema::Enum(s) = schema {
            assert_eq!(s.values.len(), 3);
        } else {
            panic!("Expected Enum schema");
        }
    }

    #[test]
    fn test_from_yaml_enum_inline_array() {
        let yaml = quarto_yaml::parse("[foo, bar, baz]").unwrap();
        let schema = Schema::from_yaml(&yaml).unwrap();
        if let Schema::Enum(s) = schema {
            assert_eq!(s.values.len(), 3);
        } else {
            panic!("Expected Enum schema");
        }
    }

    #[test]
    fn test_from_yaml_enum_explicit() {
        let yaml = quarto_yaml::parse(
            r#"
enum:
  values: [red, green, blue]
  description: "Primary colors"
"#,
        )
        .unwrap();
        let schema = Schema::from_yaml(&yaml).unwrap();
        if let Schema::Enum(s) = schema {
            assert_eq!(s.values.len(), 3);
            assert_eq!(
                s.annotations.description,
                Some("Primary colors".to_string())
            );
        } else {
            panic!("Expected Enum schema");
        }
    }

    #[test]
    fn test_from_yaml_anyof_array() {
        let yaml = quarto_yaml::parse(
            r#"
anyOf:
  - boolean
  - string
  - number
"#,
        )
        .unwrap();
        let schema = Schema::from_yaml(&yaml).unwrap();
        if let Schema::AnyOf(s) = schema {
            assert_eq!(s.schemas.len(), 3);
        } else {
            panic!("Expected AnyOf schema");
        }
    }

    #[test]
    fn test_from_yaml_anyof_object() {
        let yaml = quarto_yaml::parse(
            r#"
anyOf:
  schemas:
    - boolean
    - string
  description: "Either boolean or string"
"#,
        )
        .unwrap();
        let schema = Schema::from_yaml(&yaml).unwrap();
        if let Schema::AnyOf(s) = schema {
            assert_eq!(s.schemas.len(), 2);
            assert_eq!(
                s.annotations.description,
                Some("Either boolean or string".to_string())
            );
        } else {
            panic!("Expected AnyOf schema");
        }
    }

    #[test]
    fn test_from_yaml_allof() {
        let yaml = quarto_yaml::parse(
            r#"
allOf:
  - string
  - enum: [foo, bar]
"#,
        )
        .unwrap();
        let schema = Schema::from_yaml(&yaml).unwrap();
        if let Schema::AllOf(s) = schema {
            assert_eq!(s.schemas.len(), 2);
        } else {
            panic!("Expected AllOf schema");
        }
    }

    #[test]
    fn test_from_yaml_array() {
        let yaml = quarto_yaml::parse(
            r#"
array:
  items: string
  minItems: 1
  maxItems: 10
"#,
        )
        .unwrap();
        let schema = Schema::from_yaml(&yaml).unwrap();
        if let Schema::Array(s) = schema {
            assert!(s.items.is_some());
            assert_eq!(s.min_items, Some(1));
            assert_eq!(s.max_items, Some(10));
        } else {
            panic!("Expected Array schema");
        }
    }

    #[test]
    fn test_from_yaml_object_simple() {
        let yaml = quarto_yaml::parse(
            r#"
object:
  properties:
    name: string
    age: number
  required: [name]
"#,
        )
        .unwrap();
        let schema = Schema::from_yaml(&yaml).unwrap();
        if let Schema::Object(s) = schema {
            assert_eq!(s.properties.len(), 2);
            assert!(s.properties.contains_key("name"));
            assert!(s.properties.contains_key("age"));
            assert_eq!(s.required.len(), 1);
            assert_eq!(s.required[0], "name");
        } else {
            panic!("Expected Object schema");
        }
    }

    #[test]
    fn test_from_yaml_object_complex() {
        let yaml = quarto_yaml::parse(
            r#"
object:
  properties:
    foo: string
    bar: number
  patternProperties:
    "^x-": string
  additionalProperties: boolean
  required: [foo]
  closed: true
  minProperties: 1
  maxProperties: 10
  description: "A complex object"
"#,
        )
        .unwrap();
        let schema = Schema::from_yaml(&yaml).unwrap();
        if let Schema::Object(s) = schema {
            assert_eq!(s.properties.len(), 2);
            assert_eq!(s.pattern_properties.len(), 1);
            assert!(s.additional_properties.is_some());
            assert_eq!(s.required.len(), 1);
            assert!(s.closed);
            assert_eq!(s.min_properties, Some(1));
            assert_eq!(s.max_properties, Some(10));
            assert_eq!(
                s.annotations.description,
                Some("A complex object".to_string())
            );
        } else {
            panic!("Expected Object schema");
        }
    }

    #[test]
    fn test_from_yaml_object_required_all() {
        let yaml = quarto_yaml::parse(
            r#"
object:
  properties:
    foo: string
    bar: number
    baz: boolean
  required: all
"#,
        )
        .unwrap();
        let schema = Schema::from_yaml(&yaml).unwrap();
        if let Schema::Object(s) = schema {
            assert_eq!(s.properties.len(), 3);
            assert_eq!(s.required.len(), 3);
            // All properties should be in required list
            assert!(s.required.contains(&"foo".to_string()));
            assert!(s.required.contains(&"bar".to_string()));
            assert!(s.required.contains(&"baz".to_string()));
        } else {
            panic!("Expected Object schema");
        }
    }

    #[test]
    fn test_from_yaml_object_property_names_pattern() {
        let yaml = quarto_yaml::parse(
            r#"
object:
  propertyNames:
    string:
      pattern: "^[a-z_]+$"
  additionalProperties: string
"#,
        )
        .unwrap();
        let schema = Schema::from_yaml(&yaml).unwrap();
        if let Schema::Object(s) = schema {
            assert!(s.property_names.is_some());
            if let Some(property_names) = s.property_names {
                if let Schema::String(str_schema) = *property_names {
                    assert_eq!(str_schema.pattern, Some("^[a-z_]+$".to_string()));
                } else {
                    panic!("Expected String schema for propertyNames");
                }
            }
        } else {
            panic!("Expected Object schema");
        }
    }

    #[test]
    fn test_from_yaml_object_property_names_enum() {
        let yaml = quarto_yaml::parse(
            r#"
object:
  propertyNames:
    enum:
      - name
      - schema
      - description
"#,
        )
        .unwrap();
        let schema = Schema::from_yaml(&yaml).unwrap();
        if let Schema::Object(s) = schema {
            assert!(s.property_names.is_some());
            if let Some(property_names) = s.property_names {
                assert!(matches!(*property_names, Schema::Enum(_)));
            }
        } else {
            panic!("Expected Object schema");
        }
    }

    #[test]
    fn test_from_yaml_record_with_key_schema() {
        let yaml = quarto_yaml::parse(
            r#"
record:
  keySchema:
    string:
      pattern: "^[a-z]+$"
  valueSchema: number
"#,
        )
        .unwrap();
        let schema = Schema::from_yaml(&yaml).unwrap();
        if let Schema::Object(s) = schema {
            // keySchema becomes property_names
            assert!(s.property_names.is_some());
            if let Some(property_names) = s.property_names {
                if let Schema::String(str_schema) = *property_names {
                    assert_eq!(str_schema.pattern, Some("^[a-z]+$".to_string()));
                } else {
                    panic!("Expected String schema for property_names");
                }
            }
            // valueSchema becomes additional_properties
            assert!(s.additional_properties.is_some());
            if let Some(additional_properties) = s.additional_properties {
                assert!(matches!(*additional_properties, Schema::Number(_)));
            }
        } else {
            panic!("Expected Object schema");
        }
    }

    #[test]
    fn test_from_yaml_naming_convention_single() {
        let yaml = quarto_yaml::parse(
            r#"
object:
  namingConvention: camelCase
"#,
        )
        .unwrap();
        let schema = Schema::from_yaml(&yaml).unwrap();
        if let Schema::Object(s) = schema {
            assert_eq!(
                s.naming_convention,
                Some(NamingConvention::Single("capitalizationCase".to_string()))
            );
        } else {
            panic!("Expected Object schema");
        }
    }

    #[test]
    fn test_from_yaml_naming_convention_multiple() {
        let yaml = quarto_yaml::parse(
            r#"
object:
  namingConvention:
    - snake_case
    - kebab-case
"#,
        )
        .unwrap();
        let schema = Schema::from_yaml(&yaml).unwrap();
        if let Schema::Object(s) = schema {
            if let Some(NamingConvention::Multiple(conventions)) = s.naming_convention {
                assert_eq!(conventions.len(), 2);
                assert!(conventions.contains(&"underscore_case".to_string()));
                assert!(conventions.contains(&"dash-case".to_string()));
            } else {
                panic!("Expected Multiple naming convention");
            }
        } else {
            panic!("Expected Object schema");
        }
    }

    #[test]
    fn test_from_yaml_naming_convention_ignore() {
        let yaml = quarto_yaml::parse(
            r#"
object:
  namingConvention: ignore
"#,
        )
        .unwrap();
        let schema = Schema::from_yaml(&yaml).unwrap();
        if let Schema::Object(s) = schema {
            assert_eq!(
                s.naming_convention,
                Some(NamingConvention::Single("ignore".to_string()))
            );
        } else {
            panic!("Expected Object schema");
        }
    }

    #[test]
    fn test_from_yaml_naming_convention_normalization() {
        // Test various input formats normalize correctly
        let test_cases = vec![
            ("camelCase", "capitalizationCase"),
            ("snake_case", "underscore_case"),
            ("kebab-case", "dash-case"),
            ("camel-case", "capitalizationCase"),
            ("underscore-case", "underscore_case"),
            ("dashCase", "dash-case"),
        ];

        for (input, expected) in test_cases {
            let yaml_str = format!("object:\n  namingConvention: {}", input);
            let yaml = quarto_yaml::parse(&yaml_str).unwrap();
            let schema = Schema::from_yaml(&yaml).unwrap();
            if let Schema::Object(s) = schema {
                assert_eq!(
                    s.naming_convention,
                    Some(NamingConvention::Single(expected.to_string())),
                    "Failed for input: {}",
                    input
                );
            } else {
                panic!("Expected Object schema for input: {}", input);
            }
        }
    }

    #[test]
    fn test_from_yaml_ref() {
        let yaml = quarto_yaml::parse(
            r#"
ref: schema/base
"#,
        )
        .unwrap();
        let schema = Schema::from_yaml(&yaml).unwrap();
        if let Schema::Ref(s) = schema {
            assert_eq!(s.reference, "schema/base");
            assert_eq!(s.eager, false); // ref is lazy
        } else {
            panic!("Expected Ref schema");
        }
    }

    #[test]
    fn test_from_yaml_dollar_ref() {
        let yaml = quarto_yaml::parse(
            r#"
$ref: schema/base
"#,
        )
        .unwrap();
        let schema = Schema::from_yaml(&yaml).unwrap();
        if let Schema::Ref(s) = schema {
            assert_eq!(s.reference, "schema/base");
            assert_eq!(s.eager, false); // $ref is also lazy
        } else {
            panic!("Expected Ref schema");
        }
    }

    #[test]
    fn test_from_yaml_resolve_ref() {
        let yaml = quarto_yaml::parse(
            r#"
resolveRef: schema/base
"#,
        )
        .unwrap();
        let schema = Schema::from_yaml(&yaml).unwrap();
        if let Schema::Ref(s) = schema {
            assert_eq!(s.reference, "schema/base");
            assert_eq!(s.eager, true); // resolveRef is eager
        } else {
            panic!("Expected Ref schema");
        }
    }

    #[test]
    fn test_from_yaml_nested() {
        let yaml = quarto_yaml::parse(
            r#"
object:
  properties:
    status:
      anyOf:
        - boolean
        - enum: [active, inactive, pending]
    config:
      object:
        properties:
          enabled: boolean
"#,
        )
        .unwrap();
        let schema = Schema::from_yaml(&yaml).unwrap();
        if let Schema::Object(s) = schema {
            assert_eq!(s.properties.len(), 2);
            // Check nested anyOf
            if let Some(Schema::AnyOf(anyof)) = s.properties.get("status") {
                assert_eq!(anyof.schemas.len(), 2);
            } else {
                panic!("Expected AnyOf schema for status");
            }
            // Check nested object
            if let Some(Schema::Object(obj)) = s.properties.get("config") {
                assert_eq!(obj.properties.len(), 1);
            } else {
                panic!("Expected Object schema for config");
            }
        } else {
            panic!("Expected Object schema");
        }
    }

    #[test]
    fn test_from_yaml_error_invalid_type() {
        let yaml = quarto_yaml::parse("invalid_type").unwrap();
        let result = Schema::from_yaml(&yaml);
        assert!(result.is_err());
        if let Err(crate::error::SchemaError::InvalidType(t)) = result {
            assert_eq!(t, "invalid_type");
        } else {
            panic!("Expected InvalidType error");
        }
    }

    #[test]
    fn test_from_yaml_error_empty_object() {
        let yaml = quarto_yaml::parse("{}").unwrap();
        let result = Schema::from_yaml(&yaml);
        assert!(result.is_err());
    }

    #[test]
    fn test_from_yaml_with_annotations() {
        let yaml = quarto_yaml::parse(
            r#"
string:
  description: "A string field"
  hidden: true
  completions: [foo, bar]
  tags:
    category: input
"#,
        )
        .unwrap();
        let schema = Schema::from_yaml(&yaml).unwrap();
        if let Schema::String(s) = schema {
            assert_eq!(
                s.annotations.description,
                Some("A string field".to_string())
            );
            assert_eq!(s.annotations.hidden, Some(true));
            assert_eq!(
                s.annotations.completions,
                Some(vec!["foo".to_string(), "bar".to_string()])
            );
            assert!(s.annotations.tags.is_some());
        } else {
            panic!("Expected String schema");
        }
    }

    #[test]
    fn test_arrayof_simple() {
        let yaml = quarto_yaml::parse("arrayOf: string").unwrap();
        let schema = Schema::from_yaml(&yaml).unwrap();

        match schema {
            Schema::Array(arr) => {
                assert!(arr.items.is_some());
                match arr.items.as_ref().unwrap().as_ref() {
                    Schema::String(_) => {}
                    _ => panic!("Expected String schema in items"),
                }
            }
            _ => panic!("Expected Array schema"),
        }
    }

    #[test]
    fn test_arrayof_nested() {
        // Test nested arrayOf like quarto-cli uses: arrayOf: { arrayOf: { schema: string, length: 2 } }
        let yaml_str = r#"
arrayOf:
  arrayOf:
    schema: string
    length: 2
"#;
        let yaml = quarto_yaml::parse(yaml_str).unwrap();
        let schema = Schema::from_yaml(&yaml).unwrap();

        match schema {
            Schema::Array(outer) => {
                assert!(outer.items.is_some());
                match outer.items.as_ref().unwrap().as_ref() {
                    Schema::Array(inner) => {
                        assert!(inner.items.is_some());
                        assert_eq!(inner.min_items, Some(2));
                        assert_eq!(inner.max_items, Some(2));
                        match inner.items.as_ref().unwrap().as_ref() {
                            Schema::String(_) => {}
                            _ => panic!("Expected String schema in nested items"),
                        }
                    }
                    _ => panic!("Expected Array schema in items"),
                }
            }
            _ => panic!("Expected Array schema"),
        }
    }

    #[test]
    fn test_arrayof_with_length() {
        let yaml_str = r#"
arrayOf:
  schema: string
  length: 5
"#;
        let yaml = quarto_yaml::parse(yaml_str).unwrap();
        let schema = Schema::from_yaml(&yaml).unwrap();

        match schema {
            Schema::Array(arr) => {
                assert!(arr.items.is_some());
                assert_eq!(arr.min_items, Some(5));
                assert_eq!(arr.max_items, Some(5));
                match arr.items.as_ref().unwrap().as_ref() {
                    Schema::String(_) => {}
                    _ => panic!("Expected String schema"),
                }
            }
            _ => panic!("Expected Array schema"),
        }
    }

    #[test]
    fn test_maybe_arrayof() {
        let yaml = quarto_yaml::parse("maybeArrayOf: string").unwrap();
        let schema = Schema::from_yaml(&yaml).unwrap();

        match schema {
            Schema::AnyOf(anyof) => {
                // Should have 2 schemas: the scalar and the array
                assert_eq!(anyof.schemas.len(), 2);

                // First should be string
                match &anyof.schemas[0] {
                    Schema::String(_) => {}
                    _ => panic!("Expected String schema as first option"),
                }

                // Second should be array of string
                match &anyof.schemas[1] {
                    Schema::Array(arr) => {
                        assert!(arr.items.is_some());
                        match arr.items.as_ref().unwrap().as_ref() {
                            Schema::String(_) => {}
                            _ => panic!("Expected String schema in array"),
                        }
                    }
                    _ => panic!("Expected Array schema as second option"),
                }

                // Should have complete-from tag
                assert!(anyof.annotations.tags.is_some());
                let tags = anyof.annotations.tags.as_ref().unwrap();
                assert!(tags.contains_key("complete-from"));
            }
            _ => panic!("Expected AnyOf schema"),
        }
    }

    #[test]
    fn test_record_form1() {
        let yaml_str = r#"
record:
  properties:
    type:
      enum: [citeproc]
"#;
        let yaml = quarto_yaml::parse(yaml_str).unwrap();
        let schema = Schema::from_yaml(&yaml).unwrap();

        match schema {
            Schema::Object(obj) => {
                assert!(obj.closed);
                assert_eq!(obj.properties.len(), 1);
                assert!(obj.properties.contains_key("type"));
                assert_eq!(obj.required.len(), 1);
                assert!(obj.required.contains(&"type".to_string()));
            }
            _ => panic!("Expected Object schema"),
        }
    }

    #[test]
    fn test_record_form2() {
        let yaml_str = r#"
record:
  name: string
  age: number
"#;
        let yaml = quarto_yaml::parse(yaml_str).unwrap();
        let schema = Schema::from_yaml(&yaml).unwrap();

        match schema {
            Schema::Object(obj) => {
                assert!(obj.closed);
                assert_eq!(obj.properties.len(), 2);
                assert!(obj.properties.contains_key("name"));
                assert!(obj.properties.contains_key("age"));
                assert_eq!(obj.required.len(), 2);
                assert!(obj.required.contains(&"name".to_string()));
                assert!(obj.required.contains(&"age".to_string()));
            }
            _ => panic!("Expected Object schema"),
        }
    }

    #[test]
    fn test_schema_wrapper() {
        let yaml_str = r#"
schema:
  anyOf:
    - boolean
    - string
"#;
        let yaml = quarto_yaml::parse(yaml_str).unwrap();
        let schema = Schema::from_yaml(&yaml).unwrap();

        match schema {
            Schema::AnyOf(anyof) => {
                assert_eq!(anyof.schemas.len(), 2);
                match &anyof.schemas[0] {
                    Schema::Boolean(_) => {}
                    _ => panic!("Expected Boolean schema"),
                }
                match &anyof.schemas[1] {
                    Schema::String(_) => {}
                    _ => panic!("Expected String schema"),
                }
            }
            _ => panic!("Expected AnyOf schema"),
        }
    }

    #[test]
    fn test_schema_wrapper_with_outer_annotations() {
        let yaml_str = r#"
schema:
  anyOf:
    - boolean
    - string
description: "Outer description"
completions: ["value1", "value2", "value3"]
hidden: true
"#;
        let yaml = quarto_yaml::parse(yaml_str).unwrap();
        let schema = Schema::from_yaml(&yaml).unwrap();

        match schema {
            Schema::AnyOf(s) => {
                assert_eq!(
                    s.annotations.description,
                    Some("Outer description".to_string())
                );
                assert_eq!(
                    s.annotations.completions,
                    Some(vec![
                        "value1".to_string(),
                        "value2".to_string(),
                        "value3".to_string()
                    ])
                );
                assert_eq!(s.annotations.hidden, Some(true));
            }
            _ => panic!("Expected AnyOf schema"),
        }
    }

    #[test]
    fn test_schema_wrapper_annotation_override() {
        let yaml_str = r#"
schema:
  string:
    description: "Inner description"
    completions: ["inner1", "inner2"]
description: "Outer description"
completions: ["outer1", "outer2"]
"#;
        let yaml = quarto_yaml::parse(yaml_str).unwrap();
        let schema = Schema::from_yaml(&yaml).unwrap();

        match schema {
            Schema::String(s) => {
                // Outer should override inner
                assert_eq!(
                    s.annotations.description,
                    Some("Outer description".to_string())
                );
                assert_eq!(
                    s.annotations.completions,
                    Some(vec!["outer1".to_string(), "outer2".to_string()])
                );
            }
            _ => panic!("Expected String schema"),
        }
    }

    #[test]
    fn test_schema_wrapper_tag_merging() {
        let yaml_str = r#"
schema:
  string:
    tags:
      category: input
      inner-only: true
description: "Test"
tags:
  category: output
  outer-only: true
"#;
        let yaml = quarto_yaml::parse(yaml_str).unwrap();
        let schema = Schema::from_yaml(&yaml).unwrap();

        match schema {
            Schema::String(s) => {
                let tags = s.annotations.tags.as_ref().unwrap();
                // Outer "category" should override inner
                assert_eq!(tags.get("category"), Some(&serde_json::json!("output")));
                // Both inner-only and outer-only should be present
                assert_eq!(tags.get("inner-only"), Some(&serde_json::json!(true)));
                assert_eq!(tags.get("outer-only"), Some(&serde_json::json!(true)));
            }
            _ => panic!("Expected String schema"),
        }
    }

    #[test]
    fn test_additional_completions_basic() {
        let yaml = quarto_yaml::parse(
            r#"
schema:
  string:
    completions: ["a", "b"]
additionalCompletions: ["c", "d"]
"#,
        )
        .unwrap();
        let schema = Schema::from_yaml(&yaml).unwrap();
        if let Schema::String(s) = schema {
            assert_eq!(
                s.annotations.completions,
                Some(vec![
                    "a".to_string(),
                    "b".to_string(),
                    "c".to_string(),
                    "d".to_string()
                ])
            );
            // additional_completions should be cleared after merge
            assert_eq!(s.annotations.additional_completions, None);
        } else {
            panic!("Expected String schema");
        }
    }

    #[test]
    fn test_additional_completions_overwrite() {
        let yaml = quarto_yaml::parse(
            r#"
schema:
  string:
    completions: ["a", "b"]
additionalCompletions: ["c", "d"]
completions: ["e", "f"]
"#,
        )
        .unwrap();
        let schema = Schema::from_yaml(&yaml).unwrap();
        if let Schema::String(s) = schema {
            // completions overwrites everything
            assert_eq!(
                s.annotations.completions,
                Some(vec!["e".to_string(), "f".to_string()])
            );
        } else {
            panic!("Expected String schema");
        }
    }

    #[test]
    fn test_additional_completions_without_wrapper() {
        let yaml = quarto_yaml::parse(
            r#"
string:
  additionalCompletions: ["x", "y"]
"#,
        )
        .unwrap();
        let schema = Schema::from_yaml(&yaml).unwrap();
        if let Schema::String(s) = schema {
            // Without schema wrapper, additionalCompletions is stored but not merged
            assert_eq!(s.annotations.completions, None);
            assert_eq!(
                s.annotations.additional_completions,
                Some(vec!["x".to_string(), "y".to_string()])
            );
        } else {
            panic!("Expected String schema");
        }
    }

    // Tests for schema inheritance (super field)

    #[test]
    fn test_object_with_super_single() {
        let yaml = quarto_yaml::parse(
            r#"
object:
  super:
    resolveRef: base-schema
  properties:
    name: string
"#,
        )
        .unwrap();

        let schema = Schema::from_yaml(&yaml).unwrap();
        match schema {
            Schema::Object(obj) => {
                assert!(obj.base_schema.is_some());
                let bases = obj.base_schema.unwrap();
                assert_eq!(bases.len(), 1);
                match &bases[0] {
                    Schema::Ref(r) => {
                        assert_eq!(r.reference, "base-schema");
                        assert_eq!(r.eager, true);
                    }
                    _ => panic!("Expected Ref schema"),
                }
                assert!(obj.properties.contains_key("name"));
            }
            _ => panic!("Expected Object schema"),
        }
    }

    #[test]
    fn test_object_with_super_array() {
        let yaml = quarto_yaml::parse(
            r#"
object:
  super:
    - resolveRef: base1
    - resolveRef: base2
  properties:
    name: string
"#,
        )
        .unwrap();

        let schema = Schema::from_yaml(&yaml).unwrap();
        match schema {
            Schema::Object(obj) => {
                assert!(obj.base_schema.is_some());
                let bases = obj.base_schema.unwrap();
                assert_eq!(bases.len(), 2);
                match &bases[0] {
                    Schema::Ref(r) => {
                        assert_eq!(r.reference, "base1");
                        assert_eq!(r.eager, true);
                    }
                    _ => panic!("Expected Ref schema for base1"),
                }
                match &bases[1] {
                    Schema::Ref(r) => {
                        assert_eq!(r.reference, "base2");
                        assert_eq!(r.eager, true);
                    }
                    _ => panic!("Expected Ref schema for base2"),
                }
            }
            _ => panic!("Expected Object schema"),
        }
    }

    #[test]
    fn test_object_without_super() {
        let yaml = quarto_yaml::parse(
            r#"
object:
  properties:
    name: string
"#,
        )
        .unwrap();

        let schema = Schema::from_yaml(&yaml).unwrap();
        match schema {
            Schema::Object(obj) => {
                assert!(obj.base_schema.is_none());
                assert!(obj.properties.contains_key("name"));
            }
            _ => panic!("Expected Object schema"),
        }
    }

    #[test]
    fn test_super_with_inline_schema() {
        let yaml = quarto_yaml::parse(
            r#"
object:
  super:
    object:
      properties:
        base_field: string
  properties:
    derived_field: number
"#,
        )
        .unwrap();

        let schema = Schema::from_yaml(&yaml).unwrap();
        match schema {
            Schema::Object(obj) => {
                assert!(obj.base_schema.is_some());
                let bases = obj.base_schema.unwrap();
                assert_eq!(bases.len(), 1);
                match &bases[0] {
                    Schema::Object(base_obj) => {
                        assert!(base_obj.properties.contains_key("base_field"));
                    }
                    _ => panic!("Expected Object schema for base"),
                }
                assert!(obj.properties.contains_key("derived_field"));
            }
            _ => panic!("Expected Object schema"),
        }
    }
}
