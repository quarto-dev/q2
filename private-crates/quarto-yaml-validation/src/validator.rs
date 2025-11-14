// YAML validation engine

use crate::error::{
    InstancePath, PathSegment, SchemaPath, ValidationError, ValidationErrorKind, ValidationResult,
};
use crate::schema::{Schema, SchemaRegistry};
use quarto_source_map::SourceContext;
use quarto_yaml::YamlWithSourceInfo;
use regex::Regex;
use std::collections::HashSet;
use yaml_rust2::Yaml;

/// Validates a YAML value against a schema
pub fn validate(
    value: &YamlWithSourceInfo,
    schema: &Schema,
    registry: &SchemaRegistry,
    source_ctx: &SourceContext,
) -> ValidationResult<()> {
    let mut context = ValidationContext::new(registry, source_ctx);
    validate_generic(value, schema, &mut context)
}

/// Validation context tracks state during validation
pub struct ValidationContext<'a> {
    /// Reference to the schema registry for $ref resolution
    registry: &'a SchemaRegistry,
    /// Source context for mapping offsets to line/column
    source_ctx: &'a SourceContext,
    /// Current instance path (e.g., ["format", "html", "toc"])
    instance_path: InstancePath,
    /// Current schema path (e.g., ["properties", "format"])
    schema_path: SchemaPath,
    /// Collected validation errors
    errors: Vec<ValidationError>,
}

impl<'a> ValidationContext<'a> {
    /// Create a new validation context
    pub fn new(registry: &'a SchemaRegistry, source_ctx: &'a SourceContext) -> Self {
        Self {
            registry,
            source_ctx,
            instance_path: InstancePath::new(),
            schema_path: SchemaPath::new(),
            errors: Vec::new(),
        }
    }

    /// Add an error to the context
    pub fn add_error(&mut self, kind: ValidationErrorKind, node: &YamlWithSourceInfo) {
        let error = ValidationError::new(kind, self.instance_path.clone())
            .with_schema_path(self.schema_path.clone())
            .with_yaml_node(node.clone(), self.source_ctx);
        self.errors.push(error);
    }

    /// Execute a function with a new instance path segment
    pub fn with_instance_path<F, R>(&mut self, segment: PathSegment, f: F) -> R
    where
        F: FnOnce(&mut Self) -> R,
    {
        self.instance_path.push_key(segment.to_string());
        let result = f(self);
        self.instance_path.pop();
        result
    }

    /// Execute a function with a new schema path segment
    pub fn with_schema_path<F, R>(&mut self, segment: impl Into<String>, f: F) -> R
    where
        F: FnOnce(&mut Self) -> R,
    {
        self.schema_path.push(segment);
        let result = f(self);
        self.schema_path.pop();
        result
    }

    /// Get the collected errors
    pub fn errors(&self) -> &[ValidationError] {
        &self.errors
    }

    /// Check if validation failed
    pub fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }
}

/// Navigate through a YamlWithSourceInfo tree using an instance path
///
/// This function is critical for error reporting - it finds the exact YAML node
/// corresponding to a validation error.
///
/// # Arguments
/// * `path` - The instance path to follow (e.g., ["format", "html", "toc"])
/// * `annotation` - The YAML tree to navigate
/// * `return_key` - If true, return the key node; if false, return the value node
/// * `path_index` - Current position in the path (used for recursion)
pub fn navigate<'a>(
    path: &InstancePath,
    annotation: &'a YamlWithSourceInfo,
    return_key: bool,
    path_index: usize,
) -> Option<&'a YamlWithSourceInfo> {
    // Base case: we've reached the end of the path
    if path_index >= path.segments().len() {
        return Some(annotation);
    }

    let segment = &path.segments()[path_index];

    // Check if this is a hash/mapping
    if let Some(entries) = annotation.as_hash() {
        // For mappings, search backwards (like TypeScript version)
        match segment {
            PathSegment::Key(search_key) => {
                for entry in entries.iter().rev() {
                    if let Yaml::String(ref key_str) = entry.key.yaml {
                        if key_str == search_key {
                            let target = if return_key && path_index == path.segments().len() - 1 {
                                &entry.key
                            } else {
                                &entry.value
                            };
                            return navigate(path, target, return_key, path_index + 1);
                        }
                    }
                }
                None
            }
            PathSegment::Index(_) => {
                // Index doesn't make sense for a mapping
                None
            }
        }
    }
    // Check if this is an array/sequence
    else if let Some(items) = annotation.as_array() {
        match segment {
            PathSegment::Index(index) => {
                if *index < items.len() {
                    navigate(path, &items[*index], return_key, path_index + 1)
                } else {
                    None
                }
            }
            PathSegment::Key(_) => {
                // Key doesn't make sense for a sequence
                None
            }
        }
    }
    // Scalar - can't navigate into it
    else {
        None
    }
}

/// Main validation dispatcher
fn validate_generic(
    value: &YamlWithSourceInfo,
    schema: &Schema,
    context: &mut ValidationContext,
) -> ValidationResult<()> {
    match schema {
        Schema::False => {
            context.add_error(ValidationErrorKind::SchemaFalse, value);
            Err(context.errors[0].clone())
        }
        Schema::True => Ok(()),
        Schema::Boolean(s) => {
            context.with_schema_path("boolean", |ctx| validate_boolean(value, s, ctx))
        }
        Schema::Number(s) => {
            context.with_schema_path("number", |ctx| validate_number(value, s, ctx))
        }
        Schema::String(s) => {
            context.with_schema_path("string", |ctx| validate_string(value, s, ctx))
        }
        Schema::Null(s) => context.with_schema_path("null", |ctx| validate_null(value, s, ctx)),
        Schema::Enum(s) => context.with_schema_path("enum", |ctx| validate_enum(value, s, ctx)),
        Schema::Any(_) => Ok(()),
        Schema::AnyOf(s) => context.with_schema_path("anyOf", |ctx| validate_any_of(value, s, ctx)),
        Schema::AllOf(s) => context.with_schema_path("allOf", |ctx| validate_all_of(value, s, ctx)),
        Schema::Array(s) => context.with_schema_path("array", |ctx| validate_array(value, s, ctx)),
        Schema::Object(s) => {
            context.with_schema_path("object", |ctx| validate_object(value, s, ctx))
        }
        Schema::Ref(s) => {
            // Resolve the reference
            if let Some(resolved) = context.registry.resolve(&s.reference) {
                validate_generic(value, resolved, context)
            } else {
                context.add_error(
                    ValidationErrorKind::UnresolvedReference {
                        ref_id: s.reference.clone(),
                    },
                    value,
                );
                Err(context.errors[0].clone())
            }
        }
    }
}

/// Validate a boolean value
fn validate_boolean(
    value: &YamlWithSourceInfo,
    _schema: &crate::schema::BooleanSchema,
    context: &mut ValidationContext,
) -> ValidationResult<()> {
    match &value.yaml {
        Yaml::Boolean(_) => Ok(()),
        _ => {
            context.add_error(
                ValidationErrorKind::TypeMismatch {
                    expected: "boolean".to_string(),
                    got: yaml_type_name(&value.yaml).to_string(),
                },
                value,
            );
            Err(context.errors[0].clone())
        }
    }
}

/// Validate a number value
fn validate_number(
    value: &YamlWithSourceInfo,
    schema: &crate::schema::NumberSchema,
    context: &mut ValidationContext,
) -> ValidationResult<()> {
    let num = match &value.yaml {
        Yaml::Integer(n) => *n as f64,
        Yaml::Real(s) => s.parse::<f64>().unwrap_or(f64::NAN),
        _ => {
            context.add_error(
                ValidationErrorKind::TypeMismatch {
                    expected: "number".to_string(),
                    got: yaml_type_name(&value.yaml).to_string(),
                },
                value,
            );
            return Err(context.errors[0].clone());
        }
    };

    // Check minimum
    if let Some(min) = schema.minimum {
        if num < min {
            context.add_error(
                ValidationErrorKind::NumberOutOfRange {
                    value: num,
                    minimum: Some(min),
                    maximum: None,
                    exclusive_minimum: None,
                    exclusive_maximum: None,
                },
                value,
            );
            return Err(context.errors[0].clone());
        }
    }

    // Check maximum
    if let Some(max) = schema.maximum {
        if num > max {
            context.add_error(
                ValidationErrorKind::NumberOutOfRange {
                    value: num,
                    minimum: None,
                    maximum: Some(max),
                    exclusive_minimum: None,
                    exclusive_maximum: None,
                },
                value,
            );
            return Err(context.errors[0].clone());
        }
    }

    // Check exclusive minimum
    if let Some(min) = schema.exclusive_minimum {
        if num <= min {
            context.add_error(
                ValidationErrorKind::NumberOutOfRange {
                    value: num,
                    minimum: None,
                    maximum: None,
                    exclusive_minimum: Some(min),
                    exclusive_maximum: None,
                },
                value,
            );
            return Err(context.errors[0].clone());
        }
    }

    // Check exclusive maximum
    if let Some(max) = schema.exclusive_maximum {
        if num >= max {
            context.add_error(
                ValidationErrorKind::NumberOutOfRange {
                    value: num,
                    minimum: None,
                    maximum: None,
                    exclusive_minimum: None,
                    exclusive_maximum: Some(max),
                },
                value,
            );
            return Err(context.errors[0].clone());
        }
    }

    // Check multiple of
    if let Some(multiple) = schema.multiple_of {
        if (num % multiple).abs() > f64::EPSILON {
            context.add_error(
                ValidationErrorKind::NumberNotMultipleOf {
                    value: num,
                    multiple_of: multiple,
                },
                value,
            );
            return Err(context.errors[0].clone());
        }
    }

    Ok(())
}

/// Validate a string value
fn validate_string(
    value: &YamlWithSourceInfo,
    schema: &crate::schema::StringSchema,
    context: &mut ValidationContext,
) -> ValidationResult<()> {
    let s = match &value.yaml {
        Yaml::String(s) => s,
        _ => {
            context.add_error(
                ValidationErrorKind::TypeMismatch {
                    expected: "string".to_string(),
                    got: yaml_type_name(&value.yaml).to_string(),
                },
                value,
            );
            return Err(context.errors[0].clone());
        }
    };

    // Check min length
    if let Some(min) = schema.min_length {
        if s.len() < min {
            context.add_error(
                ValidationErrorKind::StringLengthInvalid {
                    length: s.len(),
                    min_length: Some(min),
                    max_length: None,
                },
                value,
            );
            return Err(context.errors[0].clone());
        }
    }

    // Check max length
    if let Some(max) = schema.max_length {
        if s.len() > max {
            context.add_error(
                ValidationErrorKind::StringLengthInvalid {
                    length: s.len(),
                    min_length: None,
                    max_length: Some(max),
                },
                value,
            );
            return Err(context.errors[0].clone());
        }
    }

    // Check pattern
    if let Some(pattern) = &schema.pattern {
        let re = Regex::new(pattern).map_err(|e| {
            // Invalid regex is a schema error, not a validation error.
            // This is a programming error in the schema definition itself.
            // We use Other here because this isn't really a validation failure
            // of the YAML document - it's a problem with the schema.
            ValidationError::new(
                ValidationErrorKind::Other {
                    message: format!("Invalid regex pattern '{}': {}", pattern, e),
                },
                context.instance_path.clone(),
            )
        })?;

        if !re.is_match(s) {
            context.add_error(
                ValidationErrorKind::StringPatternMismatch {
                    value: s.clone(),
                    pattern: pattern.clone(),
                },
                value,
            );
            return Err(context.errors[0].clone());
        }
    }

    Ok(())
}

/// Validate a null value
fn validate_null(
    value: &YamlWithSourceInfo,
    _schema: &crate::schema::NullSchema,
    context: &mut ValidationContext,
) -> ValidationResult<()> {
    match &value.yaml {
        Yaml::Null => Ok(()),
        _ => {
            context.add_error(
                ValidationErrorKind::TypeMismatch {
                    expected: "null".to_string(),
                    got: yaml_type_name(&value.yaml).to_string(),
                },
                value,
            );
            Err(context.errors[0].clone())
        }
    }
}

/// Validate an enum value
fn validate_enum(
    value: &YamlWithSourceInfo,
    schema: &crate::schema::EnumSchema,
    context: &mut ValidationContext,
) -> ValidationResult<()> {
    // Convert YAML value to JSON value for comparison
    let json_value = yaml_to_json_value(&value.yaml);

    for allowed in &schema.values {
        if &json_value == allowed {
            return Ok(());
        }
    }

    context.add_error(
        ValidationErrorKind::InvalidEnumValue {
            value: format!("{}", json_value),
            allowed: schema.values.iter().map(|v| format!("{}", v)).collect(),
        },
        value,
    );
    Err(context.errors[0].clone())
}

/// Validate anyOf (at least one schema must match)
fn validate_any_of(
    value: &YamlWithSourceInfo,
    schema: &crate::schema::AnyOfSchema,
    context: &mut ValidationContext,
) -> ValidationResult<()> {
    let original_error_count = context.errors.len();

    for (_i, subschema) in schema.schemas.iter().enumerate() {
        let mut sub_context = ValidationContext::new(context.registry, context.source_ctx);
        sub_context.instance_path = context.instance_path.clone();
        sub_context.schema_path = context.schema_path.clone();

        if validate_generic(value, subschema, &mut sub_context).is_ok() {
            // Success! Clear any errors from failed attempts
            context.errors.truncate(original_error_count);
            return Ok(());
        }

        // This subschema failed, but continue trying others
        context.errors.extend(sub_context.errors);
    }

    // All subschemas failed
    // TODO: Implement error pruning to select the "best" error
    Err(context.errors[original_error_count].clone())
}

/// Validate allOf (all schemas must match)
fn validate_all_of(
    value: &YamlWithSourceInfo,
    schema: &crate::schema::AllOfSchema,
    context: &mut ValidationContext,
) -> ValidationResult<()> {
    for subschema in &schema.schemas {
        validate_generic(value, subschema, context)?;
    }
    Ok(())
}

/// Validate an array value
fn validate_array(
    value: &YamlWithSourceInfo,
    schema: &crate::schema::ArraySchema,
    context: &mut ValidationContext,
) -> ValidationResult<()> {
    let items = match value.as_array() {
        Some(items) => items,
        None => {
            context.add_error(
                ValidationErrorKind::TypeMismatch {
                    expected: "array".to_string(),
                    got: yaml_type_name(&value.yaml).to_string(),
                },
                value,
            );
            return Err(context.errors[0].clone());
        }
    };

    // Check min items
    if let Some(min) = schema.min_items {
        if items.len() < min {
            context.add_error(
                ValidationErrorKind::ArrayLengthInvalid {
                    length: items.len(),
                    min_items: Some(min),
                    max_items: None,
                },
                value,
            );
            return Err(context.errors[0].clone());
        }
    }

    // Check max items
    if let Some(max) = schema.max_items {
        if items.len() > max {
            context.add_error(
                ValidationErrorKind::ArrayLengthInvalid {
                    length: items.len(),
                    min_items: None,
                    max_items: Some(max),
                },
                value,
            );
            return Err(context.errors[0].clone());
        }
    }

    // Check unique items
    if let Some(true) = schema.unique_items {
        let mut seen = HashSet::new();
        for item in items {
            let json_value = yaml_to_json_value(&item.yaml);
            if !seen.insert(format!("{:?}", json_value)) {
                context.add_error(ValidationErrorKind::ArrayItemsNotUnique, value);
                return Err(context.errors[0].clone());
            }
        }
    }

    // Validate each item
    if let Some(item_schema) = &schema.items {
        for (i, item) in items.iter().enumerate() {
            context.with_instance_path(PathSegment::Index(i), |ctx| {
                validate_generic(item, item_schema, ctx)
            })?;
        }
    }

    Ok(())
}

/// Validate an object value
fn validate_object(
    value: &YamlWithSourceInfo,
    schema: &crate::schema::ObjectSchema,
    context: &mut ValidationContext,
) -> ValidationResult<()> {
    let entries = match value.as_hash() {
        Some(entries) => entries,
        None => {
            context.add_error(
                ValidationErrorKind::TypeMismatch {
                    expected: "object".to_string(),
                    got: yaml_type_name(&value.yaml).to_string(),
                },
                value,
            );
            return Err(context.errors[0].clone());
        }
    };

    // Extract keys
    let mut keys = HashSet::new();
    for entry in entries {
        if let Yaml::String(ref key) = entry.key.yaml {
            keys.insert(key.clone());
        }
    }

    // Check required properties
    for required in &schema.required {
        if !keys.contains(required) {
            context.add_error(
                ValidationErrorKind::MissingRequiredProperty {
                    property: required.clone(),
                },
                value,
            );
            return Err(context.errors[0].clone());
        }
    }

    // Check min/max properties
    if let Some(min) = schema.min_properties {
        if entries.len() < min {
            context.add_error(
                ValidationErrorKind::ObjectPropertyCountInvalid {
                    count: entries.len(),
                    min_properties: Some(min),
                    max_properties: None,
                },
                value,
            );
            return Err(context.errors[0].clone());
        }
    }

    if let Some(max) = schema.max_properties {
        if entries.len() > max {
            context.add_error(
                ValidationErrorKind::ObjectPropertyCountInvalid {
                    count: entries.len(),
                    min_properties: None,
                    max_properties: Some(max),
                },
                value,
            );
            return Err(context.errors[0].clone());
        }
    }

    // Validate each property
    for entry in entries {
        if let Yaml::String(ref key) = entry.key.yaml {
            // Check if property is defined in schema
            let property_schema = schema.properties.get(key);

            if let Some(prop_schema) = property_schema {
                context.with_instance_path(PathSegment::Key(key.clone()), |ctx| {
                    validate_generic(&entry.value, prop_schema, ctx)
                })?;
            } else if schema.closed {
                // Closed object - no additional properties allowed
                context.add_error(
                    ValidationErrorKind::UnknownProperty {
                        property: key.clone(),
                    },
                    value,
                );
                return Err(context.errors[0].clone());
            } else if let Some(additional) = &schema.additional_properties {
                // Validate against additional properties schema
                context.with_instance_path(PathSegment::Key(key.clone()), |ctx| {
                    validate_generic(&entry.value, additional, ctx)
                })?;
            }
        }
    }

    Ok(())
}

/// Get a human-readable type name for a YAML value
fn yaml_type_name(value: &Yaml) -> &'static str {
    match value {
        Yaml::Null | Yaml::BadValue => "null",
        Yaml::Boolean(_) => "boolean",
        Yaml::Integer(_) => "integer",
        Yaml::Real(_) => "float",
        Yaml::String(_) => "string",
        Yaml::Array(_) => "array",
        Yaml::Hash(_) => "object",
        Yaml::Alias(_) => "alias",
    }
}

/// Convert YAML value to JSON value for comparison
fn yaml_to_json_value(value: &Yaml) -> serde_json::Value {
    match value {
        Yaml::Null | Yaml::BadValue => serde_json::Value::Null,
        Yaml::Boolean(b) => serde_json::Value::Bool(*b),
        Yaml::Integer(n) => serde_json::Value::Number((*n).into()),
        Yaml::Real(s) => {
            if let Ok(f) = s.parse::<f64>() {
                serde_json::Number::from_f64(f)
                    .map(serde_json::Value::Number)
                    .unwrap_or(serde_json::Value::Null)
            } else {
                serde_json::Value::Null
            }
        }
        Yaml::String(s) => serde_json::Value::String(s.clone()),
        Yaml::Array(items) => {
            serde_json::Value::Array(items.iter().map(yaml_to_json_value).collect())
        }
        Yaml::Hash(entries) => {
            let mut map = serde_json::Map::new();
            for (key, value) in entries {
                if let Yaml::String(key_str) = key {
                    map.insert(key_str.clone(), yaml_to_json_value(value));
                }
            }
            serde_json::Value::Object(map)
        }
        Yaml::Alias(_) => serde_json::Value::Null, // Aliases should be resolved before validation
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{BooleanSchema, SchemaAnnotations};
    use quarto_yaml::SourceInfo;
    use yaml_rust2::Yaml;

    #[test]
    fn test_validate_boolean() {
        let registry = SchemaRegistry::new();
        let source_ctx = SourceContext::new();
        let schema = Schema::Boolean(BooleanSchema {
            annotations: SchemaAnnotations::default(),
        });

        let yaml = YamlWithSourceInfo::new_scalar(Yaml::Boolean(true), SourceInfo::default());

        assert!(validate(&yaml, &schema, &registry, &source_ctx).is_ok());
    }

    #[test]
    fn test_validate_boolean_wrong_type() {
        let registry = SchemaRegistry::new();
        let source_ctx = SourceContext::new();
        let schema = Schema::Boolean(BooleanSchema {
            annotations: SchemaAnnotations::default(),
        });

        let yaml = YamlWithSourceInfo::new_scalar(
            Yaml::String("not a boolean".to_string()),
            SourceInfo::default(),
        );

        assert!(validate(&yaml, &schema, &registry, &source_ctx).is_err());
    }
}
