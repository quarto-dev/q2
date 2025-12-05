//! Schema type definitions
//!
//! This module contains all the schema struct definitions that represent
//! different validation types in Quarto's simplified JSON Schema subset.
//!
//! Each schema type struct contains:
//! - annotations: Common metadata like description, documentation, etc.
//! - type-specific fields: Constraints and validation rules specific to that type

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::Schema;

/// Naming convention for object property names (Quarto extension)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum NamingConvention {
    /// Single naming convention
    Single(String),
    /// Multiple allowed naming conventions (property must match at least one)
    Multiple(Vec<String>),
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

    /// Additional completions to merge with existing completions (Quarto extension)
    #[serde(
        rename = "additionalCompletions",
        skip_serializing_if = "Option::is_none"
    )]
    pub additional_completions: Option<Vec<String>>,

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
    /// Schema that property names (keys) must match
    pub property_names: Option<Box<Schema>>,
    /// Quarto extension: naming convention(s) for property names
    pub naming_convention: Option<NamingConvention>,
    /// Base schemas for inheritance (via `super` field in YAML)
    ///
    /// Can contain Schema::Ref with eager=true (to be resolved during compilation)
    /// or actual ObjectSchema instances (if already resolved).
    /// When present, schemas should be merged during compilation phase.
    pub base_schema: Option<Vec<Schema>>,
}

/// Reference to another schema
#[derive(Debug, Clone, PartialEq)]
pub struct RefSchema {
    pub annotations: SchemaAnnotations,
    pub reference: String,
    /// Whether this reference should be resolved eagerly (true for resolveRef, false for ref/$ref)
    pub eager: bool,
}
