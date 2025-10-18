// Schema types for YAML validation
//
// This module defines the schema type system used for validation,
// closely matching Quarto's simplified JSON Schema subset.
//
// IMPORTANT: This module does NOT use serde deserialization for loading schemas
// from YAML because serde_yaml only supports YAML 1.1. We need YAML 1.2 support
// for consistency with user documents and to support Quarto extensions.
// See ../YAML-1.2-REQUIREMENT.md for details.
//
// Instead, schemas are parsed from YamlWithSourceInfo (quarto-yaml) which uses
// yaml-rust2 (YAML 1.2). See Schema::from_yaml() method below.

use crate::error::{SchemaError, SchemaResult};
use quarto_yaml::{SourceInfo, YamlWithSourceInfo};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use yaml_rust2::Yaml;

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

/// Annotations that can be attached to any schema
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SchemaAnnotations {
    /// Schema identifier for references
    #[serde(rename = "$id", skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    /// Short description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Detailed documentation
    #[serde(skip_serializing_if = "Option::is_none")]
    pub documentation: Option<String>,

    /// Custom error message to display on validation failure
    #[serde(rename = "errorMessage", skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,

    /// Whether this schema should be hidden in IDE completions
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hidden: Option<bool>,

    /// Completion suggestions for IDE support
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completions: Option<Vec<String>>,

    /// Tags for categorization (e.g., "engine: knitr")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<HashMap<String, serde_json::Value>>,
}

/// Boolean type schema
#[derive(Debug, Clone, PartialEq)]
pub struct BooleanSchema {
    pub annotations: SchemaAnnotations,
}

/// Number type schema (integer or float)
#[derive(Debug, Clone, PartialEq)]
pub struct NumberSchema {
    pub annotations: SchemaAnnotations,
    pub minimum: Option<f64>,
    pub maximum: Option<f64>,
    pub exclusive_minimum: Option<f64>,
    pub exclusive_maximum: Option<f64>,
    pub multiple_of: Option<f64>,
}

/// String type schema
#[derive(Debug, Clone, PartialEq)]
pub struct StringSchema {
    pub annotations: SchemaAnnotations,
    pub min_length: Option<usize>,
    pub max_length: Option<usize>,
    pub pattern: Option<String>,
}

/// Null type schema
#[derive(Debug, Clone, PartialEq)]
pub struct NullSchema {
    pub annotations: SchemaAnnotations,
}

/// Enum type schema
#[derive(Debug, Clone, PartialEq)]
pub struct EnumSchema {
    pub annotations: SchemaAnnotations,
    pub values: Vec<serde_json::Value>,
}

/// Any type schema (no validation)
#[derive(Debug, Clone, PartialEq)]
pub struct AnySchema {
    pub annotations: SchemaAnnotations,
}

/// AnyOf schema (validates if any subschema matches)
#[derive(Debug, Clone, PartialEq)]
pub struct AnyOfSchema {
    pub annotations: SchemaAnnotations,
    pub schemas: Vec<Schema>,
}

/// AllOf schema (validates if all subschemas match)
#[derive(Debug, Clone, PartialEq)]
pub struct AllOfSchema {
    pub annotations: SchemaAnnotations,
    pub schemas: Vec<Schema>,
}

/// Array type schema
#[derive(Debug, Clone, PartialEq)]
pub struct ArraySchema {
    pub annotations: SchemaAnnotations,
    pub items: Option<Box<Schema>>,
    pub min_items: Option<usize>,
    pub max_items: Option<usize>,
    pub unique_items: Option<bool>,
}

/// Object type schema
#[derive(Debug, Clone, PartialEq)]
pub struct ObjectSchema {
    pub annotations: SchemaAnnotations,
    pub properties: HashMap<String, Schema>,
    pub pattern_properties: HashMap<String, Schema>,
    pub additional_properties: Option<Box<Schema>>,
    pub required: Vec<String>,
    pub min_properties: Option<usize>,
    pub max_properties: Option<usize>,
    /// Quarto extension: if true, object cannot have properties not in schema
    pub closed: bool,
}

/// Reference to another schema
#[derive(Debug, Clone, PartialEq)]
pub struct RefSchema {
    pub annotations: SchemaAnnotations,
    pub reference: String,
}

impl Schema {
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
}

// Static empty annotations for False and True schemas
static EMPTY_ANNOTATIONS: SchemaAnnotations = SchemaAnnotations {
    id: None,
    description: None,
    documentation: None,
    error_message: None,
    hidden: None,
    completions: None,
    tags: None,
};

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
        match &yaml.yaml {
            // Short form: "boolean", "string", etc.
            Yaml::String(s) => Self::parse_short_form(s.as_str(), &yaml.source_info),

            // Object form: {boolean: {...}}, {enum: [...]}, etc.
            Yaml::Hash(_) => Self::parse_object_form(yaml),

            // Array form: [val1, val2, val3] - inline enum
            Yaml::Array(_) => Self::parse_inline_enum(yaml),

            // Null can be a schema type too
            Yaml::Null => Ok(Schema::Null(NullSchema {
                annotations: Default::default(),
            })),

            _ => Err(SchemaError::InvalidStructure {
                message: format!("Expected schema, got {:?}", yaml.yaml),
                location: yaml.source_info.clone(),
            }),
        }
    }

    /// Parse short form: "boolean", "string", "number", "any", "null", "path"
    fn parse_short_form(s: &str, _location: &SourceInfo) -> SchemaResult<Schema> {
        match s {
            "boolean" => Ok(Schema::Boolean(BooleanSchema {
                annotations: Default::default(),
            })),
            "number" => Ok(Schema::Number(NumberSchema {
                annotations: Default::default(),
                minimum: None,
                maximum: None,
                exclusive_minimum: None,
                exclusive_maximum: None,
                multiple_of: None,
            })),
            "string" | "path" => Ok(Schema::String(StringSchema {
                annotations: Default::default(),
                min_length: None,
                max_length: None,
                pattern: None,
            })),
            "null" => Ok(Schema::Null(NullSchema {
                annotations: Default::default(),
            })),
            "any" => Ok(Schema::Any(AnySchema {
                annotations: Default::default(),
            })),
            _ => Err(SchemaError::InvalidType(s.to_string())),
        }
    }

    /// Parse object form: {boolean: {...}}, {string: {...}}, etc.
    fn parse_object_form(yaml: &YamlWithSourceInfo) -> SchemaResult<Schema> {
        let entries = yaml.as_hash().ok_or_else(|| SchemaError::InvalidStructure {
            message: "Expected hash for object form schema".to_string(),
            location: yaml.source_info.clone(),
        })?;

        if entries.is_empty() {
            return Err(SchemaError::InvalidStructure {
                message: "Empty schema object".to_string(),
                location: yaml.source_info.clone(),
            });
        }

        // Peek at first key to determine schema type
        let first_entry = &entries[0];
        let key = first_entry.key.yaml.as_str().ok_or_else(|| {
            SchemaError::InvalidStructure {
                message: "Schema type key must be a string".to_string(),
                location: first_entry.key.source_info.clone(),
            }
        })?;

        match key {
            "boolean" => Self::parse_boolean_schema(&first_entry.value),
            "number" => Self::parse_number_schema(&first_entry.value),
            "string" | "path" => Self::parse_string_schema(&first_entry.value),
            "null" => Self::parse_null_schema(&first_entry.value),
            "enum" => Self::parse_enum_schema(&first_entry.value),
            "any" => Self::parse_any_schema(&first_entry.value),
            "anyOf" => Self::parse_anyof_schema(&first_entry.value),
            "allOf" => Self::parse_allof_schema(&first_entry.value),
            "array" => Self::parse_array_schema(&first_entry.value),
            "object" => Self::parse_object_schema(&first_entry.value),
            "ref" | "$ref" => Self::parse_ref_schema(&first_entry.value),
            _ => Err(SchemaError::InvalidType(key.to_string())),
        }
    }

    /// Parse inline enum array: [val1, val2, val3]
    fn parse_inline_enum(yaml: &YamlWithSourceInfo) -> SchemaResult<Schema> {
        let items = yaml.as_array().ok_or_else(|| SchemaError::InvalidStructure {
            message: "Expected array for inline enum".to_string(),
            location: yaml.source_info.clone(),
        })?;

        // Convert YamlWithSourceInfo items to serde_json::Value for enum values
        let values: SchemaResult<Vec<_>> = items
            .iter()
            .map(|item| yaml_to_json_value(&item.yaml, &item.source_info))
            .collect();

        Ok(Schema::Enum(EnumSchema {
            annotations: Default::default(),
            values: values?,
        }))
    }

    // Type-specific parsers

    fn parse_boolean_schema(yaml: &YamlWithSourceInfo) -> SchemaResult<Schema> {
        let annotations = parse_annotations(yaml)?;
        Ok(Schema::Boolean(BooleanSchema { annotations }))
    }

    fn parse_number_schema(yaml: &YamlWithSourceInfo) -> SchemaResult<Schema> {
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

    fn parse_string_schema(yaml: &YamlWithSourceInfo) -> SchemaResult<Schema> {
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

    fn parse_null_schema(yaml: &YamlWithSourceInfo) -> SchemaResult<Schema> {
        let annotations = parse_annotations(yaml)?;
        Ok(Schema::Null(NullSchema { annotations }))
    }

    fn parse_any_schema(yaml: &YamlWithSourceInfo) -> SchemaResult<Schema> {
        let annotations = parse_annotations(yaml)?;
        Ok(Schema::Any(AnySchema { annotations }))
    }

    fn parse_enum_schema(yaml: &YamlWithSourceInfo) -> SchemaResult<Schema> {
        let annotations = parse_annotations(yaml)?;

        // Handle both inline array and explicit object form
        let values = if let Some(values_yaml) = yaml.get_hash_value("values") {
            // Explicit form: enum: { values: [...] }
            let items = values_yaml.as_array().ok_or_else(|| SchemaError::InvalidStructure {
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
            let items = yaml.as_array().ok_or_else(|| SchemaError::InvalidStructure {
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

    fn parse_anyof_schema(yaml: &YamlWithSourceInfo) -> SchemaResult<Schema> {
        let annotations = parse_annotations(yaml)?;

        // Handle both array form and object form with schemas: field
        let schemas = if let Some(schemas_yaml) = yaml.get_hash_value("schemas") {
            // Explicit form: anyOf: { schemas: [...] }
            let items = schemas_yaml.as_array().ok_or_else(|| SchemaError::InvalidStructure {
                message: "anyOf schemas must be an array".to_string(),
                location: schemas_yaml.source_info.clone(),
            })?;

            let result: SchemaResult<Vec<_>> = items
                .iter()
                .map(|item| Schema::from_yaml(item))
                .collect();
            result?
        } else {
            // Inline form: anyOf: [schema1, schema2, ...]
            let items = yaml.as_array().ok_or_else(|| SchemaError::InvalidStructure {
                message: "Expected array for anyOf".to_string(),
                location: yaml.source_info.clone(),
            })?;

            let result: SchemaResult<Vec<_>> = items
                .iter()
                .map(|item| Schema::from_yaml(item))
                .collect();
            result?
        };

        Ok(Schema::AnyOf(AnyOfSchema {
            annotations,
            schemas,
        }))
    }

    fn parse_allof_schema(yaml: &YamlWithSourceInfo) -> SchemaResult<Schema> {
        let annotations = parse_annotations(yaml)?;

        // Similar to anyOf
        let schemas = if let Some(schemas_yaml) = yaml.get_hash_value("schemas") {
            let items = schemas_yaml.as_array().ok_or_else(|| SchemaError::InvalidStructure {
                message: "allOf schemas must be an array".to_string(),
                location: schemas_yaml.source_info.clone(),
            })?;

            let result: SchemaResult<Vec<_>> = items
                .iter()
                .map(|item| Schema::from_yaml(item))
                .collect();
            result?
        } else {
            let items = yaml.as_array().ok_or_else(|| SchemaError::InvalidStructure {
                message: "Expected array for allOf".to_string(),
                location: yaml.source_info.clone(),
            })?;

            let result: SchemaResult<Vec<_>> = items
                .iter()
                .map(|item| Schema::from_yaml(item))
                .collect();
            result?
        };

        Ok(Schema::AllOf(AllOfSchema {
            annotations,
            schemas,
        }))
    }

    fn parse_array_schema(yaml: &YamlWithSourceInfo) -> SchemaResult<Schema> {
        let annotations = parse_annotations(yaml)?;
        let items = if let Some(items_yaml) = yaml.get_hash_value("items") {
            Some(Box::new(Schema::from_yaml(items_yaml)?))
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

    fn parse_object_schema(yaml: &YamlWithSourceInfo) -> SchemaResult<Schema> {
        let annotations = parse_annotations(yaml)?;

        // Parse properties
        let properties = if let Some(props_yaml) = yaml.get_hash_value("properties") {
            let entries = props_yaml.as_hash().ok_or_else(|| SchemaError::InvalidStructure {
                message: "properties must be an object".to_string(),
                location: props_yaml.source_info.clone(),
            })?;

            let mut props = HashMap::new();
            for entry in entries {
                let key = entry.key.yaml.as_str().ok_or_else(|| SchemaError::InvalidStructure {
                    message: "property key must be a string".to_string(),
                    location: entry.key.source_info.clone(),
                })?;
                let schema = Schema::from_yaml(&entry.value)?;
                props.insert(key.to_string(), schema);
            }
            props
        } else {
            HashMap::new()
        };

        // Parse patternProperties
        let pattern_properties = if let Some(pattern_props_yaml) = yaml.get_hash_value("patternProperties") {
            let entries = pattern_props_yaml.as_hash().ok_or_else(|| SchemaError::InvalidStructure {
                message: "patternProperties must be an object".to_string(),
                location: pattern_props_yaml.source_info.clone(),
            })?;

            let mut props = HashMap::new();
            for entry in entries {
                let key = entry.key.yaml.as_str().ok_or_else(|| SchemaError::InvalidStructure {
                    message: "patternProperty key must be a string".to_string(),
                    location: entry.key.source_info.clone(),
                })?;
                let schema = Schema::from_yaml(&entry.value)?;
                props.insert(key.to_string(), schema);
            }
            props
        } else {
            HashMap::new()
        };

        // Parse additionalProperties
        let additional_properties = if let Some(additional_yaml) = yaml.get_hash_value("additionalProperties") {
            Some(Box::new(Schema::from_yaml(additional_yaml)?))
        } else {
            None
        };

        // Parse required
        let required = if let Some(required_yaml) = yaml.get_hash_value("required") {
            let items = required_yaml.as_array().ok_or_else(|| SchemaError::InvalidStructure {
                message: "required must be an array".to_string(),
                location: required_yaml.source_info.clone(),
            })?;

            let result: SchemaResult<Vec<_>> = items
                .iter()
                .map(|item| {
                    item.yaml.as_str()
                        .map(|s| s.to_string())
                        .ok_or_else(|| SchemaError::InvalidStructure {
                            message: "required items must be strings".to_string(),
                            location: item.source_info.clone(),
                        })
                })
                .collect();
            result?
        } else {
            Vec::new()
        };

        let min_properties = get_hash_usize(yaml, "minProperties")?;
        let max_properties = get_hash_usize(yaml, "maxProperties")?;
        let closed = get_hash_bool(yaml, "closed")?.unwrap_or(false);

        Ok(Schema::Object(ObjectSchema {
            annotations,
            properties,
            pattern_properties,
            additional_properties,
            required,
            min_properties,
            max_properties,
            closed,
        }))
    }

    fn parse_ref_schema(yaml: &YamlWithSourceInfo) -> SchemaResult<Schema> {
        let reference = yaml.yaml.as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| SchemaError::InvalidStructure {
                message: "ref must be a string".to_string(),
                location: yaml.source_info.clone(),
            })?;

        Ok(Schema::Ref(RefSchema {
            annotations: Default::default(),
            reference,
        }))
    }
}

// Helper functions for parsing

/// Parse common annotations from a schema object
fn parse_annotations(yaml: &YamlWithSourceInfo) -> SchemaResult<SchemaAnnotations> {
    Ok(SchemaAnnotations {
        id: get_hash_string(yaml, "$id")?,
        description: get_hash_string(yaml, "description")?,
        documentation: get_hash_string(yaml, "documentation")?,
        error_message: get_hash_string(yaml, "errorMessage")?,
        hidden: get_hash_bool(yaml, "hidden")?,
        completions: get_hash_string_array(yaml, "completions")?,
        tags: get_hash_tags(yaml)?,
    })
}

fn get_hash_string(yaml: &YamlWithSourceInfo, key: &str) -> SchemaResult<Option<String>> {
    if let Some(value) = yaml.get_hash_value(key) {
        if let Some(s) = value.yaml.as_str() {
            return Ok(Some(s.to_string()));
        }
        return Err(SchemaError::InvalidStructure {
            message: format!("Field '{}' must be a string", key),
            location: value.source_info.clone(),
        });
    }
    Ok(None)
}

fn get_hash_number(yaml: &YamlWithSourceInfo, key: &str) -> SchemaResult<Option<f64>> {
    if let Some(value) = yaml.get_hash_value(key) {
        match &value.yaml {
            Yaml::Integer(i) => return Ok(Some(*i as f64)),
            Yaml::Real(r) => {
                if let Ok(f) = r.parse::<f64>() {
                    return Ok(Some(f));
                }
            }
            _ => {}
        }
        return Err(SchemaError::InvalidStructure {
            message: format!("Field '{}' must be a number", key),
            location: value.source_info.clone(),
        });
    }
    Ok(None)
}

fn get_hash_usize(yaml: &YamlWithSourceInfo, key: &str) -> SchemaResult<Option<usize>> {
    if let Some(value) = yaml.get_hash_value(key) {
        if let Some(i) = value.yaml.as_i64() {
            if i >= 0 {
                return Ok(Some(i as usize));
            }
        }
        return Err(SchemaError::InvalidStructure {
            message: format!("Field '{}' must be a non-negative integer", key),
            location: value.source_info.clone(),
        });
    }
    Ok(None)
}

fn get_hash_bool(yaml: &YamlWithSourceInfo, key: &str) -> SchemaResult<Option<bool>> {
    if let Some(value) = yaml.get_hash_value(key) {
        if let Some(b) = value.yaml.as_bool() {
            return Ok(Some(b));
        }
        return Err(SchemaError::InvalidStructure {
            message: format!("Field '{}' must be a boolean", key),
            location: value.source_info.clone(),
        });
    }
    Ok(None)
}

fn get_hash_string_array(yaml: &YamlWithSourceInfo, key: &str) -> SchemaResult<Option<Vec<String>>> {
    if let Some(value) = yaml.get_hash_value(key) {
        let items = value.as_array().ok_or_else(|| SchemaError::InvalidStructure {
            message: format!("Field '{}' must be an array", key),
            location: value.source_info.clone(),
        })?;

        let result: SchemaResult<Vec<_>> = items
            .iter()
            .map(|item| {
                item.yaml.as_str()
                    .map(|s| s.to_string())
                    .ok_or_else(|| SchemaError::InvalidStructure {
                        message: format!("Field '{}' items must be strings", key),
                        location: item.source_info.clone(),
                    })
            })
            .collect();
        return Ok(Some(result?));
    }
    Ok(None)
}

fn get_hash_tags(yaml: &YamlWithSourceInfo) -> SchemaResult<Option<HashMap<String, serde_json::Value>>> {
    if let Some(value) = yaml.get_hash_value("tags") {
        let entries = value.as_hash().ok_or_else(|| SchemaError::InvalidStructure {
            message: "tags must be an object".to_string(),
            location: value.source_info.clone(),
        })?;

        let mut tags = HashMap::new();
        for entry in entries {
            let key = entry.key.yaml.as_str().ok_or_else(|| SchemaError::InvalidStructure {
                message: "tag key must be a string".to_string(),
                location: entry.key.source_info.clone(),
            })?;
            let value = yaml_to_json_value(&entry.value.yaml, &entry.value.source_info)?;
            tags.insert(key.to_string(), value);
        }
        return Ok(Some(tags));
    }
    Ok(None)
}

/// Convert yaml-rust2 Yaml to serde_json::Value (for enum values and tags)
fn yaml_to_json_value(yaml: &Yaml, location: &SourceInfo) -> SchemaResult<serde_json::Value> {
    match yaml {
        Yaml::String(s) => Ok(serde_json::Value::String(s.clone())),
        Yaml::Integer(i) => Ok(serde_json::Value::Number((*i).into())),
        Yaml::Real(r) => {
            if let Ok(f) = r.parse::<f64>() {
                if let Some(n) = serde_json::Number::from_f64(f) {
                    return Ok(serde_json::Value::Number(n));
                }
            }
            Err(SchemaError::InvalidStructure {
                message: format!("Invalid number: {}", r),
                location: location.clone(),
            })
        }
        Yaml::Boolean(b) => Ok(serde_json::Value::Bool(*b)),
        Yaml::Null => Ok(serde_json::Value::Null),
        _ => Err(SchemaError::InvalidStructure {
            message: "Unsupported YAML type for JSON conversion".to_string(),
            location: location.clone(),
        }),
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
        let yaml = quarto_yaml::parse(r#"
boolean:
  description: "A boolean value"
"#).unwrap();
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
        let yaml = quarto_yaml::parse(r#"
number:
  minimum: 0
  maximum: 100
  description: "A percentage"
"#).unwrap();
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
        let yaml = quarto_yaml::parse(r#"
string:
  pattern: "^[a-z]+$"
  minLength: 1
  maxLength: 50
"#).unwrap();
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
        let yaml = quarto_yaml::parse(r#"
enum: [foo, bar, baz]
"#).unwrap();
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
        let yaml = quarto_yaml::parse(r#"
enum:
  values: [red, green, blue]
  description: "Primary colors"
"#).unwrap();
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
        let yaml = quarto_yaml::parse(r#"
anyOf:
  - boolean
  - string
  - number
"#).unwrap();
        let schema = Schema::from_yaml(&yaml).unwrap();
        if let Schema::AnyOf(s) = schema {
            assert_eq!(s.schemas.len(), 3);
        } else {
            panic!("Expected AnyOf schema");
        }
    }

    #[test]
    fn test_from_yaml_anyof_object() {
        let yaml = quarto_yaml::parse(r#"
anyOf:
  schemas:
    - boolean
    - string
  description: "Either boolean or string"
"#).unwrap();
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
        let yaml = quarto_yaml::parse(r#"
allOf:
  - string
  - enum: [foo, bar]
"#).unwrap();
        let schema = Schema::from_yaml(&yaml).unwrap();
        if let Schema::AllOf(s) = schema {
            assert_eq!(s.schemas.len(), 2);
        } else {
            panic!("Expected AllOf schema");
        }
    }

    #[test]
    fn test_from_yaml_array() {
        let yaml = quarto_yaml::parse(r#"
array:
  items: string
  minItems: 1
  maxItems: 10
"#).unwrap();
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
        let yaml = quarto_yaml::parse(r#"
object:
  properties:
    name: string
    age: number
  required: [name]
"#).unwrap();
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
        let yaml = quarto_yaml::parse(r#"
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
"#).unwrap();
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
    fn test_from_yaml_ref() {
        let yaml = quarto_yaml::parse(r#"
ref: schema/base
"#).unwrap();
        let schema = Schema::from_yaml(&yaml).unwrap();
        if let Schema::Ref(s) = schema {
            assert_eq!(s.reference, "schema/base");
        } else {
            panic!("Expected Ref schema");
        }
    }

    #[test]
    fn test_from_yaml_nested() {
        let yaml = quarto_yaml::parse(r#"
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
"#).unwrap();
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
        if let Err(SchemaError::InvalidType(t)) = result {
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
        let yaml = quarto_yaml::parse(r#"
string:
  description: "A string field"
  hidden: true
  completions: [foo, bar]
  tags:
    category: input
"#).unwrap();
        let schema = Schema::from_yaml(&yaml).unwrap();
        if let Schema::String(s) = schema {
            assert_eq!(s.annotations.description, Some("A string field".to_string()));
            assert_eq!(s.annotations.hidden, Some(true));
            assert_eq!(s.annotations.completions, Some(vec!["foo".to_string(), "bar".to_string()]));
            assert!(s.annotations.tags.is_some());
        } else {
            panic!("Expected String schema");
        }
    }
}
