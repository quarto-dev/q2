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
                    if let Yaml::String(ref key_str) = entry.key.yaml
                        && key_str == search_key {
                            let target = if return_key && path_index == path.segments().len() - 1 {
                                &entry.key
                            } else {
                                &entry.value
                            };
                            return navigate(path, target, return_key, path_index + 1);
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
    if let Some(min) = schema.minimum
        && num < min {
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

    // Check maximum
    if let Some(max) = schema.maximum
        && num > max {
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

    // Check exclusive minimum
    if let Some(min) = schema.exclusive_minimum
        && num <= min {
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

    // Check exclusive maximum
    if let Some(max) = schema.exclusive_maximum
        && num >= max {
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

    // Check multiple of
    if let Some(multiple) = schema.multiple_of
        && (num % multiple).abs() > f64::EPSILON {
            context.add_error(
                ValidationErrorKind::NumberNotMultipleOf {
                    value: num,
                    multiple_of: multiple,
                },
                value,
            );
            return Err(context.errors[0].clone());
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
    if let Some(min) = schema.min_length
        && s.len() < min {
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

    // Check max length
    if let Some(max) = schema.max_length
        && s.len() > max {
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

    for subschema in schema.schemas.iter() {
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
    if let Some(min) = schema.min_items
        && items.len() < min {
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

    // Check max items
    if let Some(max) = schema.max_items
        && items.len() > max {
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
    if let Some(min) = schema.min_properties
        && entries.len() < min {
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

    if let Some(max) = schema.max_properties
        && entries.len() > max {
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
                    .map_or(serde_json::Value::Null, serde_json::Value::Number)
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
    use crate::schema::{
        AllOfSchema, AnyOfSchema, AnySchema, ArraySchema, BooleanSchema, EnumSchema, NullSchema,
        NumberSchema, ObjectSchema, RefSchema, SchemaAnnotations, StringSchema,
    };
    use quarto_yaml::{SourceInfo, YamlHashEntry};
    use std::collections::HashMap;
    use yaml_rust2::Yaml;

    // Helper to create a simple YAML scalar
    fn yaml_scalar(yaml: Yaml) -> YamlWithSourceInfo {
        YamlWithSourceInfo::new_scalar(yaml, SourceInfo::default())
    }

    // Helper to create a YAML array
    fn yaml_array(items: Vec<Yaml>) -> YamlWithSourceInfo {
        let children: Vec<YamlWithSourceInfo> = items
            .into_iter()
            .map(|y| YamlWithSourceInfo::new_scalar(y, SourceInfo::default()))
            .collect();
        let yaml_items: Vec<Yaml> = children.iter().map(|c| c.yaml.clone()).collect();
        YamlWithSourceInfo::new_array(Yaml::Array(yaml_items), SourceInfo::default(), children)
    }

    // Helper to create a YAML object
    fn yaml_object(entries: Vec<(&str, Yaml)>) -> YamlWithSourceInfo {
        let hash_entries: Vec<YamlHashEntry> = entries
            .into_iter()
            .map(|(k, v)| YamlHashEntry {
                key: YamlWithSourceInfo::new_scalar(
                    Yaml::String(k.to_string()),
                    SourceInfo::default(),
                ),
                value: YamlWithSourceInfo::new_scalar(v, SourceInfo::default()),
                key_span: SourceInfo::default(),
                value_span: SourceInfo::default(),
                entry_span: SourceInfo::default(),
            })
            .collect();
        let mut yaml_hash = yaml_rust2::yaml::Hash::new();
        for entry in &hash_entries {
            if let Yaml::String(ref k) = entry.key.yaml {
                yaml_hash.insert(Yaml::String(k.clone()), entry.value.yaml.clone());
            }
        }
        YamlWithSourceInfo::new_hash(Yaml::Hash(yaml_hash), SourceInfo::default(), hash_entries)
    }

    // ==================== Boolean Tests ====================

    #[test]
    fn test_validate_boolean() {
        let registry = SchemaRegistry::new();
        let source_ctx = SourceContext::new();
        let schema = Schema::Boolean(BooleanSchema {
            annotations: SchemaAnnotations::default(),
        });

        let yaml = yaml_scalar(Yaml::Boolean(true));
        assert!(validate(&yaml, &schema, &registry, &source_ctx).is_ok());

        let yaml = yaml_scalar(Yaml::Boolean(false));
        assert!(validate(&yaml, &schema, &registry, &source_ctx).is_ok());
    }

    #[test]
    fn test_validate_boolean_wrong_type() {
        let registry = SchemaRegistry::new();
        let source_ctx = SourceContext::new();
        let schema = Schema::Boolean(BooleanSchema {
            annotations: SchemaAnnotations::default(),
        });

        let yaml = yaml_scalar(Yaml::String("not a boolean".to_string()));
        assert!(validate(&yaml, &schema, &registry, &source_ctx).is_err());
    }

    // ==================== Number Tests ====================

    #[test]
    fn test_validate_number_integer() {
        let registry = SchemaRegistry::new();
        let source_ctx = SourceContext::new();
        let schema = Schema::Number(NumberSchema {
            annotations: SchemaAnnotations::default(),
            minimum: None,
            maximum: None,
            exclusive_minimum: None,
            exclusive_maximum: None,
            multiple_of: None,
        });

        let yaml = yaml_scalar(Yaml::Integer(42));
        assert!(validate(&yaml, &schema, &registry, &source_ctx).is_ok());
    }

    #[test]
    fn test_validate_number_real() {
        let registry = SchemaRegistry::new();
        let source_ctx = SourceContext::new();
        let schema = Schema::Number(NumberSchema {
            annotations: SchemaAnnotations::default(),
            minimum: None,
            maximum: None,
            exclusive_minimum: None,
            exclusive_maximum: None,
            multiple_of: None,
        });

        let yaml = yaml_scalar(Yaml::Real("3.14".to_string()));
        assert!(validate(&yaml, &schema, &registry, &source_ctx).is_ok());
    }

    #[test]
    fn test_validate_number_wrong_type() {
        let registry = SchemaRegistry::new();
        let source_ctx = SourceContext::new();
        let schema = Schema::Number(NumberSchema {
            annotations: SchemaAnnotations::default(),
            minimum: None,
            maximum: None,
            exclusive_minimum: None,
            exclusive_maximum: None,
            multiple_of: None,
        });

        let yaml = yaml_scalar(Yaml::String("not a number".to_string()));
        assert!(validate(&yaml, &schema, &registry, &source_ctx).is_err());
    }

    #[test]
    fn test_validate_number_minimum() {
        let registry = SchemaRegistry::new();
        let source_ctx = SourceContext::new();
        let schema = Schema::Number(NumberSchema {
            annotations: SchemaAnnotations::default(),
            minimum: Some(10.0),
            maximum: None,
            exclusive_minimum: None,
            exclusive_maximum: None,
            multiple_of: None,
        });

        // Valid: at minimum
        let yaml = yaml_scalar(Yaml::Integer(10));
        assert!(validate(&yaml, &schema, &registry, &source_ctx).is_ok());

        // Valid: above minimum
        let yaml = yaml_scalar(Yaml::Integer(15));
        assert!(validate(&yaml, &schema, &registry, &source_ctx).is_ok());

        // Invalid: below minimum
        let yaml = yaml_scalar(Yaml::Integer(5));
        assert!(validate(&yaml, &schema, &registry, &source_ctx).is_err());
    }

    #[test]
    fn test_validate_number_maximum() {
        let registry = SchemaRegistry::new();
        let source_ctx = SourceContext::new();
        let schema = Schema::Number(NumberSchema {
            annotations: SchemaAnnotations::default(),
            minimum: None,
            maximum: Some(100.0),
            exclusive_minimum: None,
            exclusive_maximum: None,
            multiple_of: None,
        });

        // Valid: at maximum
        let yaml = yaml_scalar(Yaml::Integer(100));
        assert!(validate(&yaml, &schema, &registry, &source_ctx).is_ok());

        // Valid: below maximum
        let yaml = yaml_scalar(Yaml::Integer(50));
        assert!(validate(&yaml, &schema, &registry, &source_ctx).is_ok());

        // Invalid: above maximum
        let yaml = yaml_scalar(Yaml::Integer(150));
        assert!(validate(&yaml, &schema, &registry, &source_ctx).is_err());
    }

    #[test]
    fn test_validate_number_exclusive_minimum() {
        let registry = SchemaRegistry::new();
        let source_ctx = SourceContext::new();
        let schema = Schema::Number(NumberSchema {
            annotations: SchemaAnnotations::default(),
            minimum: None,
            maximum: None,
            exclusive_minimum: Some(10.0),
            exclusive_maximum: None,
            multiple_of: None,
        });

        // Valid: above exclusive minimum
        let yaml = yaml_scalar(Yaml::Integer(11));
        assert!(validate(&yaml, &schema, &registry, &source_ctx).is_ok());

        // Invalid: at exclusive minimum
        let yaml = yaml_scalar(Yaml::Integer(10));
        assert!(validate(&yaml, &schema, &registry, &source_ctx).is_err());

        // Invalid: below exclusive minimum
        let yaml = yaml_scalar(Yaml::Integer(5));
        assert!(validate(&yaml, &schema, &registry, &source_ctx).is_err());
    }

    #[test]
    fn test_validate_number_exclusive_maximum() {
        let registry = SchemaRegistry::new();
        let source_ctx = SourceContext::new();
        let schema = Schema::Number(NumberSchema {
            annotations: SchemaAnnotations::default(),
            minimum: None,
            maximum: None,
            exclusive_minimum: None,
            exclusive_maximum: Some(100.0),
            multiple_of: None,
        });

        // Valid: below exclusive maximum
        let yaml = yaml_scalar(Yaml::Integer(99));
        assert!(validate(&yaml, &schema, &registry, &source_ctx).is_ok());

        // Invalid: at exclusive maximum
        let yaml = yaml_scalar(Yaml::Integer(100));
        assert!(validate(&yaml, &schema, &registry, &source_ctx).is_err());

        // Invalid: above exclusive maximum
        let yaml = yaml_scalar(Yaml::Integer(150));
        assert!(validate(&yaml, &schema, &registry, &source_ctx).is_err());
    }

    #[test]
    fn test_validate_number_multiple_of() {
        let registry = SchemaRegistry::new();
        let source_ctx = SourceContext::new();
        let schema = Schema::Number(NumberSchema {
            annotations: SchemaAnnotations::default(),
            minimum: None,
            maximum: None,
            exclusive_minimum: None,
            exclusive_maximum: None,
            multiple_of: Some(5.0),
        });

        // Valid: multiple of 5
        let yaml = yaml_scalar(Yaml::Integer(15));
        assert!(validate(&yaml, &schema, &registry, &source_ctx).is_ok());

        // Valid: zero is multiple of anything
        let yaml = yaml_scalar(Yaml::Integer(0));
        assert!(validate(&yaml, &schema, &registry, &source_ctx).is_ok());

        // Invalid: not a multiple of 5
        let yaml = yaml_scalar(Yaml::Integer(7));
        assert!(validate(&yaml, &schema, &registry, &source_ctx).is_err());
    }

    // ==================== String Tests ====================

    #[test]
    fn test_validate_string() {
        let registry = SchemaRegistry::new();
        let source_ctx = SourceContext::new();
        let schema = Schema::String(StringSchema {
            annotations: SchemaAnnotations::default(),
            min_length: None,
            max_length: None,
            pattern: None,
        });

        let yaml = yaml_scalar(Yaml::String("hello".to_string()));
        assert!(validate(&yaml, &schema, &registry, &source_ctx).is_ok());
    }

    #[test]
    fn test_validate_string_wrong_type() {
        let registry = SchemaRegistry::new();
        let source_ctx = SourceContext::new();
        let schema = Schema::String(StringSchema {
            annotations: SchemaAnnotations::default(),
            min_length: None,
            max_length: None,
            pattern: None,
        });

        let yaml = yaml_scalar(Yaml::Integer(42));
        assert!(validate(&yaml, &schema, &registry, &source_ctx).is_err());
    }

    #[test]
    fn test_validate_string_min_length() {
        let registry = SchemaRegistry::new();
        let source_ctx = SourceContext::new();
        let schema = Schema::String(StringSchema {
            annotations: SchemaAnnotations::default(),
            min_length: Some(5),
            max_length: None,
            pattern: None,
        });

        // Valid: exactly min length
        let yaml = yaml_scalar(Yaml::String("hello".to_string()));
        assert!(validate(&yaml, &schema, &registry, &source_ctx).is_ok());

        // Valid: above min length
        let yaml = yaml_scalar(Yaml::String("hello world".to_string()));
        assert!(validate(&yaml, &schema, &registry, &source_ctx).is_ok());

        // Invalid: below min length
        let yaml = yaml_scalar(Yaml::String("hi".to_string()));
        assert!(validate(&yaml, &schema, &registry, &source_ctx).is_err());
    }

    #[test]
    fn test_validate_string_max_length() {
        let registry = SchemaRegistry::new();
        let source_ctx = SourceContext::new();
        let schema = Schema::String(StringSchema {
            annotations: SchemaAnnotations::default(),
            min_length: None,
            max_length: Some(10),
            pattern: None,
        });

        // Valid: exactly max length
        let yaml = yaml_scalar(Yaml::String("0123456789".to_string()));
        assert!(validate(&yaml, &schema, &registry, &source_ctx).is_ok());

        // Valid: below max length
        let yaml = yaml_scalar(Yaml::String("hello".to_string()));
        assert!(validate(&yaml, &schema, &registry, &source_ctx).is_ok());

        // Invalid: above max length
        let yaml = yaml_scalar(Yaml::String("this is too long".to_string()));
        assert!(validate(&yaml, &schema, &registry, &source_ctx).is_err());
    }

    #[test]
    fn test_validate_string_pattern() {
        let registry = SchemaRegistry::new();
        let source_ctx = SourceContext::new();
        let schema = Schema::String(StringSchema {
            annotations: SchemaAnnotations::default(),
            min_length: None,
            max_length: None,
            pattern: Some("^[a-z]+$".to_string()),
        });

        // Valid: matches pattern
        let yaml = yaml_scalar(Yaml::String("hello".to_string()));
        assert!(validate(&yaml, &schema, &registry, &source_ctx).is_ok());

        // Invalid: doesn't match pattern
        let yaml = yaml_scalar(Yaml::String("Hello123".to_string()));
        assert!(validate(&yaml, &schema, &registry, &source_ctx).is_err());
    }

    // ==================== Null Tests ====================

    #[test]
    fn test_validate_null() {
        let registry = SchemaRegistry::new();
        let source_ctx = SourceContext::new();
        let schema = Schema::Null(NullSchema {
            annotations: SchemaAnnotations::default(),
        });

        let yaml = yaml_scalar(Yaml::Null);
        assert!(validate(&yaml, &schema, &registry, &source_ctx).is_ok());
    }

    #[test]
    fn test_validate_null_wrong_type() {
        let registry = SchemaRegistry::new();
        let source_ctx = SourceContext::new();
        let schema = Schema::Null(NullSchema {
            annotations: SchemaAnnotations::default(),
        });

        let yaml = yaml_scalar(Yaml::String("not null".to_string()));
        assert!(validate(&yaml, &schema, &registry, &source_ctx).is_err());
    }

    // ==================== Enum Tests ====================

    #[test]
    fn test_validate_enum() {
        let registry = SchemaRegistry::new();
        let source_ctx = SourceContext::new();
        let schema = Schema::Enum(EnumSchema {
            annotations: SchemaAnnotations::default(),
            values: vec![
                serde_json::json!("red"),
                serde_json::json!("green"),
                serde_json::json!("blue"),
            ],
        });

        // Valid: matches enum value
        let yaml = yaml_scalar(Yaml::String("red".to_string()));
        assert!(validate(&yaml, &schema, &registry, &source_ctx).is_ok());

        let yaml = yaml_scalar(Yaml::String("green".to_string()));
        assert!(validate(&yaml, &schema, &registry, &source_ctx).is_ok());
    }

    #[test]
    fn test_validate_enum_invalid() {
        let registry = SchemaRegistry::new();
        let source_ctx = SourceContext::new();
        let schema = Schema::Enum(EnumSchema {
            annotations: SchemaAnnotations::default(),
            values: vec![
                serde_json::json!("red"),
                serde_json::json!("green"),
                serde_json::json!("blue"),
            ],
        });

        // Invalid: not in enum
        let yaml = yaml_scalar(Yaml::String("yellow".to_string()));
        assert!(validate(&yaml, &schema, &registry, &source_ctx).is_err());
    }

    #[test]
    fn test_validate_enum_integer() {
        let registry = SchemaRegistry::new();
        let source_ctx = SourceContext::new();
        let schema = Schema::Enum(EnumSchema {
            annotations: SchemaAnnotations::default(),
            values: vec![serde_json::json!(1), serde_json::json!(2), serde_json::json!(3)],
        });

        // Valid: matches enum value
        let yaml = yaml_scalar(Yaml::Integer(2));
        assert!(validate(&yaml, &schema, &registry, &source_ctx).is_ok());

        // Invalid: not in enum
        let yaml = yaml_scalar(Yaml::Integer(5));
        assert!(validate(&yaml, &schema, &registry, &source_ctx).is_err());
    }

    // ==================== Array Tests ====================

    #[test]
    fn test_validate_array() {
        let registry = SchemaRegistry::new();
        let source_ctx = SourceContext::new();
        let schema = Schema::Array(ArraySchema {
            annotations: SchemaAnnotations::default(),
            items: None,
            min_items: None,
            max_items: None,
            unique_items: None,
        });

        let yaml = yaml_array(vec![Yaml::Integer(1), Yaml::Integer(2), Yaml::Integer(3)]);
        assert!(validate(&yaml, &schema, &registry, &source_ctx).is_ok());
    }

    #[test]
    fn test_validate_array_wrong_type() {
        let registry = SchemaRegistry::new();
        let source_ctx = SourceContext::new();
        let schema = Schema::Array(ArraySchema {
            annotations: SchemaAnnotations::default(),
            items: None,
            min_items: None,
            max_items: None,
            unique_items: None,
        });

        let yaml = yaml_scalar(Yaml::String("not an array".to_string()));
        assert!(validate(&yaml, &schema, &registry, &source_ctx).is_err());
    }

    #[test]
    fn test_validate_array_min_items() {
        let registry = SchemaRegistry::new();
        let source_ctx = SourceContext::new();
        let schema = Schema::Array(ArraySchema {
            annotations: SchemaAnnotations::default(),
            items: None,
            min_items: Some(2),
            max_items: None,
            unique_items: None,
        });

        // Valid: at min items
        let yaml = yaml_array(vec![Yaml::Integer(1), Yaml::Integer(2)]);
        assert!(validate(&yaml, &schema, &registry, &source_ctx).is_ok());

        // Invalid: below min items
        let yaml = yaml_array(vec![Yaml::Integer(1)]);
        assert!(validate(&yaml, &schema, &registry, &source_ctx).is_err());
    }

    #[test]
    fn test_validate_array_max_items() {
        let registry = SchemaRegistry::new();
        let source_ctx = SourceContext::new();
        let schema = Schema::Array(ArraySchema {
            annotations: SchemaAnnotations::default(),
            items: None,
            min_items: None,
            max_items: Some(3),
            unique_items: None,
        });

        // Valid: at max items
        let yaml = yaml_array(vec![Yaml::Integer(1), Yaml::Integer(2), Yaml::Integer(3)]);
        assert!(validate(&yaml, &schema, &registry, &source_ctx).is_ok());

        // Invalid: above max items
        let yaml = yaml_array(vec![
            Yaml::Integer(1),
            Yaml::Integer(2),
            Yaml::Integer(3),
            Yaml::Integer(4),
        ]);
        assert!(validate(&yaml, &schema, &registry, &source_ctx).is_err());
    }

    #[test]
    fn test_validate_array_unique_items() {
        let registry = SchemaRegistry::new();
        let source_ctx = SourceContext::new();
        let schema = Schema::Array(ArraySchema {
            annotations: SchemaAnnotations::default(),
            items: None,
            min_items: None,
            max_items: None,
            unique_items: Some(true),
        });

        // Valid: all unique
        let yaml = yaml_array(vec![Yaml::Integer(1), Yaml::Integer(2), Yaml::Integer(3)]);
        assert!(validate(&yaml, &schema, &registry, &source_ctx).is_ok());

        // Invalid: duplicates
        let yaml = yaml_array(vec![Yaml::Integer(1), Yaml::Integer(2), Yaml::Integer(1)]);
        assert!(validate(&yaml, &schema, &registry, &source_ctx).is_err());
    }

    #[test]
    fn test_validate_array_items_schema() {
        let registry = SchemaRegistry::new();
        let source_ctx = SourceContext::new();
        let schema = Schema::Array(ArraySchema {
            annotations: SchemaAnnotations::default(),
            items: Some(Box::new(Schema::Number(NumberSchema {
                annotations: SchemaAnnotations::default(),
                minimum: None,
                maximum: None,
                exclusive_minimum: None,
                exclusive_maximum: None,
                multiple_of: None,
            }))),
            min_items: None,
            max_items: None,
            unique_items: None,
        });

        // Valid: all items are numbers
        let yaml = yaml_array(vec![Yaml::Integer(1), Yaml::Integer(2), Yaml::Integer(3)]);
        assert!(validate(&yaml, &schema, &registry, &source_ctx).is_ok());

        // Invalid: contains non-number
        let yaml = yaml_array(vec![
            Yaml::Integer(1),
            Yaml::String("not a number".to_string()),
        ]);
        assert!(validate(&yaml, &schema, &registry, &source_ctx).is_err());
    }

    // ==================== Object Tests ====================

    #[test]
    fn test_validate_object() {
        let registry = SchemaRegistry::new();
        let source_ctx = SourceContext::new();
        let schema = Schema::Object(ObjectSchema {
            annotations: SchemaAnnotations::default(),
            properties: HashMap::new(),
            pattern_properties: HashMap::new(),
            additional_properties: None,
            required: vec![],
            min_properties: None,
            max_properties: None,
            closed: false,
            property_names: None,
            naming_convention: None,
            base_schema: None,
        });

        let yaml = yaml_object(vec![("name", Yaml::String("test".to_string()))]);
        assert!(validate(&yaml, &schema, &registry, &source_ctx).is_ok());
    }

    #[test]
    fn test_validate_object_wrong_type() {
        let registry = SchemaRegistry::new();
        let source_ctx = SourceContext::new();
        let schema = Schema::Object(ObjectSchema {
            annotations: SchemaAnnotations::default(),
            properties: HashMap::new(),
            pattern_properties: HashMap::new(),
            additional_properties: None,
            required: vec![],
            min_properties: None,
            max_properties: None,
            closed: false,
            property_names: None,
            naming_convention: None,
            base_schema: None,
        });

        let yaml = yaml_scalar(Yaml::String("not an object".to_string()));
        assert!(validate(&yaml, &schema, &registry, &source_ctx).is_err());
    }

    #[test]
    fn test_validate_object_required() {
        let registry = SchemaRegistry::new();
        let source_ctx = SourceContext::new();
        let schema = Schema::Object(ObjectSchema {
            annotations: SchemaAnnotations::default(),
            properties: HashMap::new(),
            pattern_properties: HashMap::new(),
            additional_properties: None,
            required: vec!["name".to_string()],
            min_properties: None,
            max_properties: None,
            closed: false,
            property_names: None,
            naming_convention: None,
            base_schema: None,
        });

        // Valid: has required property
        let yaml = yaml_object(vec![("name", Yaml::String("test".to_string()))]);
        assert!(validate(&yaml, &schema, &registry, &source_ctx).is_ok());

        // Invalid: missing required property
        let yaml = yaml_object(vec![("other", Yaml::String("test".to_string()))]);
        assert!(validate(&yaml, &schema, &registry, &source_ctx).is_err());
    }

    #[test]
    fn test_validate_object_min_properties() {
        let registry = SchemaRegistry::new();
        let source_ctx = SourceContext::new();
        let schema = Schema::Object(ObjectSchema {
            annotations: SchemaAnnotations::default(),
            properties: HashMap::new(),
            pattern_properties: HashMap::new(),
            additional_properties: None,
            required: vec![],
            min_properties: Some(2),
            max_properties: None,
            closed: false,
            property_names: None,
            naming_convention: None,
            base_schema: None,
        });

        // Valid: at min properties
        let yaml = yaml_object(vec![("a", Yaml::Integer(1)), ("b", Yaml::Integer(2))]);
        assert!(validate(&yaml, &schema, &registry, &source_ctx).is_ok());

        // Invalid: below min properties
        let yaml = yaml_object(vec![("a", Yaml::Integer(1))]);
        assert!(validate(&yaml, &schema, &registry, &source_ctx).is_err());
    }

    #[test]
    fn test_validate_object_max_properties() {
        let registry = SchemaRegistry::new();
        let source_ctx = SourceContext::new();
        let schema = Schema::Object(ObjectSchema {
            annotations: SchemaAnnotations::default(),
            properties: HashMap::new(),
            pattern_properties: HashMap::new(),
            additional_properties: None,
            required: vec![],
            min_properties: None,
            max_properties: Some(2),
            closed: false,
            property_names: None,
            naming_convention: None,
            base_schema: None,
        });

        // Valid: at max properties
        let yaml = yaml_object(vec![("a", Yaml::Integer(1)), ("b", Yaml::Integer(2))]);
        assert!(validate(&yaml, &schema, &registry, &source_ctx).is_ok());

        // Invalid: above max properties
        let yaml = yaml_object(vec![
            ("a", Yaml::Integer(1)),
            ("b", Yaml::Integer(2)),
            ("c", Yaml::Integer(3)),
        ]);
        assert!(validate(&yaml, &schema, &registry, &source_ctx).is_err());
    }

    #[test]
    fn test_validate_object_closed() {
        let registry = SchemaRegistry::new();
        let source_ctx = SourceContext::new();

        let mut properties = HashMap::new();
        properties.insert(
            "name".to_string(),
            Schema::String(StringSchema {
                annotations: SchemaAnnotations::default(),
                min_length: None,
                max_length: None,
                pattern: None,
            }),
        );

        let schema = Schema::Object(ObjectSchema {
            annotations: SchemaAnnotations::default(),
            properties,
            pattern_properties: HashMap::new(),
            additional_properties: None,
            required: vec![],
            min_properties: None,
            max_properties: None,
            closed: true,
            property_names: None,
            naming_convention: None,
            base_schema: None,
        });

        // Valid: only known property
        let yaml = yaml_object(vec![("name", Yaml::String("test".to_string()))]);
        assert!(validate(&yaml, &schema, &registry, &source_ctx).is_ok());

        // Invalid: unknown property in closed object
        let yaml = yaml_object(vec![
            ("name", Yaml::String("test".to_string())),
            ("unknown", Yaml::Integer(42)),
        ]);
        assert!(validate(&yaml, &schema, &registry, &source_ctx).is_err());
    }

    #[test]
    fn test_validate_object_property_schema() {
        let registry = SchemaRegistry::new();
        let source_ctx = SourceContext::new();

        let mut properties = HashMap::new();
        properties.insert(
            "count".to_string(),
            Schema::Number(NumberSchema {
                annotations: SchemaAnnotations::default(),
                minimum: Some(0.0),
                maximum: None,
                exclusive_minimum: None,
                exclusive_maximum: None,
                multiple_of: None,
            }),
        );

        let schema = Schema::Object(ObjectSchema {
            annotations: SchemaAnnotations::default(),
            properties,
            pattern_properties: HashMap::new(),
            additional_properties: None,
            required: vec![],
            min_properties: None,
            max_properties: None,
            closed: false,
            property_names: None,
            naming_convention: None,
            base_schema: None,
        });

        // Valid: count is a valid number
        let yaml = yaml_object(vec![("count", Yaml::Integer(5))]);
        assert!(validate(&yaml, &schema, &registry, &source_ctx).is_ok());

        // Invalid: count is negative
        let yaml = yaml_object(vec![("count", Yaml::Integer(-1))]);
        assert!(validate(&yaml, &schema, &registry, &source_ctx).is_err());
    }

    #[test]
    fn test_validate_object_additional_properties() {
        let registry = SchemaRegistry::new();
        let source_ctx = SourceContext::new();

        let schema = Schema::Object(ObjectSchema {
            annotations: SchemaAnnotations::default(),
            properties: HashMap::new(),
            pattern_properties: HashMap::new(),
            additional_properties: Some(Box::new(Schema::Number(NumberSchema {
                annotations: SchemaAnnotations::default(),
                minimum: None,
                maximum: None,
                exclusive_minimum: None,
                exclusive_maximum: None,
                multiple_of: None,
            }))),
            required: vec![],
            min_properties: None,
            max_properties: None,
            closed: false,
            property_names: None,
            naming_convention: None,
            base_schema: None,
        });

        // Valid: additional property is a number
        let yaml = yaml_object(vec![("anything", Yaml::Integer(42))]);
        assert!(validate(&yaml, &schema, &registry, &source_ctx).is_ok());

        // Invalid: additional property is not a number
        let yaml = yaml_object(vec![("anything", Yaml::String("not a number".to_string()))]);
        assert!(validate(&yaml, &schema, &registry, &source_ctx).is_err());
    }

    // ==================== AnyOf Tests ====================

    #[test]
    fn test_validate_any_of() {
        let registry = SchemaRegistry::new();
        let source_ctx = SourceContext::new();
        let schema = Schema::AnyOf(AnyOfSchema {
            annotations: SchemaAnnotations::default(),
            schemas: vec![
                Schema::String(StringSchema {
                    annotations: SchemaAnnotations::default(),
                    min_length: None,
                    max_length: None,
                    pattern: None,
                }),
                Schema::Number(NumberSchema {
                    annotations: SchemaAnnotations::default(),
                    minimum: None,
                    maximum: None,
                    exclusive_minimum: None,
                    exclusive_maximum: None,
                    multiple_of: None,
                }),
            ],
        });

        // Valid: matches first schema (string)
        let yaml = yaml_scalar(Yaml::String("hello".to_string()));
        assert!(validate(&yaml, &schema, &registry, &source_ctx).is_ok());

        // Valid: matches second schema (number)
        let yaml = yaml_scalar(Yaml::Integer(42));
        assert!(validate(&yaml, &schema, &registry, &source_ctx).is_ok());

        // Invalid: matches neither
        let yaml = yaml_scalar(Yaml::Boolean(true));
        assert!(validate(&yaml, &schema, &registry, &source_ctx).is_err());
    }

    // ==================== AllOf Tests ====================

    #[test]
    fn test_validate_all_of() {
        let registry = SchemaRegistry::new();
        let source_ctx = SourceContext::new();
        let schema = Schema::AllOf(AllOfSchema {
            annotations: SchemaAnnotations::default(),
            schemas: vec![
                Schema::Number(NumberSchema {
                    annotations: SchemaAnnotations::default(),
                    minimum: Some(0.0),
                    maximum: None,
                    exclusive_minimum: None,
                    exclusive_maximum: None,
                    multiple_of: None,
                }),
                Schema::Number(NumberSchema {
                    annotations: SchemaAnnotations::default(),
                    minimum: None,
                    maximum: Some(100.0),
                    exclusive_minimum: None,
                    exclusive_maximum: None,
                    multiple_of: None,
                }),
            ],
        });

        // Valid: matches both schemas (0 <= x <= 100)
        let yaml = yaml_scalar(Yaml::Integer(50));
        assert!(validate(&yaml, &schema, &registry, &source_ctx).is_ok());

        // Invalid: fails first schema (< 0)
        let yaml = yaml_scalar(Yaml::Integer(-5));
        assert!(validate(&yaml, &schema, &registry, &source_ctx).is_err());

        // Invalid: fails second schema (> 100)
        let yaml = yaml_scalar(Yaml::Integer(150));
        assert!(validate(&yaml, &schema, &registry, &source_ctx).is_err());
    }

    // ==================== Schema::True and Schema::Any Tests ====================

    #[test]
    fn test_validate_true() {
        let registry = SchemaRegistry::new();
        let source_ctx = SourceContext::new();
        let schema = Schema::True;

        // True schema accepts anything
        let yaml = yaml_scalar(Yaml::String("anything".to_string()));
        assert!(validate(&yaml, &schema, &registry, &source_ctx).is_ok());

        let yaml = yaml_scalar(Yaml::Integer(42));
        assert!(validate(&yaml, &schema, &registry, &source_ctx).is_ok());

        let yaml = yaml_scalar(Yaml::Null);
        assert!(validate(&yaml, &schema, &registry, &source_ctx).is_ok());
    }

    #[test]
    fn test_validate_any() {
        let registry = SchemaRegistry::new();
        let source_ctx = SourceContext::new();
        let schema = Schema::Any(AnySchema {
            annotations: SchemaAnnotations::default(),
        });

        // Any schema accepts anything
        let yaml = yaml_scalar(Yaml::String("anything".to_string()));
        assert!(validate(&yaml, &schema, &registry, &source_ctx).is_ok());

        let yaml = yaml_scalar(Yaml::Integer(42));
        assert!(validate(&yaml, &schema, &registry, &source_ctx).is_ok());
    }

    // ==================== Ref Tests ====================

    #[test]
    fn test_validate_ref() {
        let mut registry = SchemaRegistry::new();

        // Register a schema
        registry.register(
            "string-schema".to_string(),
            Schema::String(StringSchema {
                annotations: SchemaAnnotations::default(),
                min_length: None,
                max_length: None,
                pattern: None,
            }),
        );

        let source_ctx = SourceContext::new();
        let schema = Schema::Ref(RefSchema {
            annotations: SchemaAnnotations::default(),
            reference: "string-schema".to_string(),
            eager: false,
        });

        // Valid: matches referenced schema
        let yaml = yaml_scalar(Yaml::String("hello".to_string()));
        assert!(validate(&yaml, &schema, &registry, &source_ctx).is_ok());

        // Invalid: doesn't match referenced schema
        let yaml = yaml_scalar(Yaml::Integer(42));
        assert!(validate(&yaml, &schema, &registry, &source_ctx).is_err());
    }

    #[test]
    fn test_validate_ref_unresolved() {
        let registry = SchemaRegistry::new();
        let source_ctx = SourceContext::new();
        let schema = Schema::Ref(RefSchema {
            annotations: SchemaAnnotations::default(),
            reference: "nonexistent".to_string(),
            eager: false,
        });

        // Error: unresolved reference
        let yaml = yaml_scalar(Yaml::String("anything".to_string()));
        assert!(validate(&yaml, &schema, &registry, &source_ctx).is_err());
    }

    // ==================== Navigate Tests ====================

    #[test]
    fn test_navigate_empty_path() {
        let yaml = yaml_scalar(Yaml::String("test".to_string()));
        let path = InstancePath::new();

        let result = navigate(&path, &yaml, false, 0);
        assert!(result.is_some());
    }

    #[test]
    fn test_navigate_object_key() {
        let yaml = yaml_object(vec![
            ("name", Yaml::String("test".to_string())),
            ("age", Yaml::Integer(30)),
        ]);

        let mut path = InstancePath::new();
        path.push_key("name".to_string());

        let result = navigate(&path, &yaml, false, 0);
        assert!(result.is_some());
        if let Some(node) = result {
            assert_eq!(node.yaml, Yaml::String("test".to_string()));
        }
    }

    #[test]
    fn test_navigate_array_index() {
        let yaml = yaml_array(vec![Yaml::Integer(1), Yaml::Integer(2), Yaml::Integer(3)]);

        let mut path = InstancePath::new();
        path.push_index(1);

        let result = navigate(&path, &yaml, false, 0);
        assert!(result.is_some());
        if let Some(node) = result {
            assert_eq!(node.yaml, Yaml::Integer(2));
        }
    }

    #[test]
    fn test_navigate_nested() {
        // Create nested structure: { "items": [1, 2, 3] }
        let items_array = yaml_array(vec![Yaml::Integer(1), Yaml::Integer(2), Yaml::Integer(3)]);
        let hash_entries = vec![YamlHashEntry {
            key: YamlWithSourceInfo::new_scalar(
                Yaml::String("items".to_string()),
                SourceInfo::default(),
            ),
            value: items_array,
            key_span: SourceInfo::default(),
            value_span: SourceInfo::default(),
            entry_span: SourceInfo::default(),
        }];
        let mut yaml_hash = yaml_rust2::yaml::Hash::new();
        yaml_hash.insert(
            Yaml::String("items".to_string()),
            Yaml::Array(vec![Yaml::Integer(1), Yaml::Integer(2), Yaml::Integer(3)]),
        );
        let yaml =
            YamlWithSourceInfo::new_hash(Yaml::Hash(yaml_hash), SourceInfo::default(), hash_entries);

        let mut path = InstancePath::new();
        path.push_key("items".to_string());
        path.push_index(2);

        let result = navigate(&path, &yaml, false, 0);
        assert!(result.is_some());
        if let Some(node) = result {
            assert_eq!(node.yaml, Yaml::Integer(3));
        }
    }

    #[test]
    fn test_navigate_key_not_found() {
        let yaml = yaml_object(vec![("name", Yaml::String("test".to_string()))]);

        let mut path = InstancePath::new();
        path.push_key("nonexistent".to_string());

        let result = navigate(&path, &yaml, false, 0);
        assert!(result.is_none());
    }

    #[test]
    fn test_navigate_index_out_of_bounds() {
        let yaml = yaml_array(vec![Yaml::Integer(1), Yaml::Integer(2)]);

        let mut path = InstancePath::new();
        path.push_index(10);

        let result = navigate(&path, &yaml, false, 0);
        assert!(result.is_none());
    }

    #[test]
    fn test_navigate_index_on_object() {
        let yaml = yaml_object(vec![("name", Yaml::String("test".to_string()))]);

        let mut path = InstancePath::new();
        path.push_index(0);

        let result = navigate(&path, &yaml, false, 0);
        assert!(result.is_none());
    }

    #[test]
    fn test_navigate_key_on_array() {
        let yaml = yaml_array(vec![Yaml::Integer(1), Yaml::Integer(2)]);

        let mut path = InstancePath::new();
        path.push_key("name".to_string());

        let result = navigate(&path, &yaml, false, 0);
        assert!(result.is_none());
    }

    #[test]
    fn test_navigate_into_scalar() {
        let yaml = yaml_scalar(Yaml::String("test".to_string()));

        let mut path = InstancePath::new();
        path.push_key("name".to_string());

        let result = navigate(&path, &yaml, false, 0);
        assert!(result.is_none());
    }

    // ==================== yaml_type_name Tests ====================

    #[test]
    fn test_yaml_type_name() {
        assert_eq!(yaml_type_name(&Yaml::Null), "null");
        assert_eq!(yaml_type_name(&Yaml::Boolean(true)), "boolean");
        assert_eq!(yaml_type_name(&Yaml::Integer(42)), "integer");
        assert_eq!(yaml_type_name(&Yaml::Real("3.14".to_string())), "float");
        assert_eq!(yaml_type_name(&Yaml::String("test".to_string())), "string");
        assert_eq!(yaml_type_name(&Yaml::Array(vec![])), "array");
        assert_eq!(
            yaml_type_name(&Yaml::Hash(yaml_rust2::yaml::Hash::new())),
            "object"
        );
        assert_eq!(yaml_type_name(&Yaml::BadValue), "null");
        assert_eq!(yaml_type_name(&Yaml::Alias(0)), "alias");
    }

    // ==================== yaml_to_json_value Tests ====================

    #[test]
    fn test_yaml_to_json_value() {
        assert_eq!(yaml_to_json_value(&Yaml::Null), serde_json::Value::Null);
        assert_eq!(
            yaml_to_json_value(&Yaml::Boolean(true)),
            serde_json::Value::Bool(true)
        );
        assert_eq!(
            yaml_to_json_value(&Yaml::Integer(42)),
            serde_json::json!(42)
        );
        assert_eq!(
            yaml_to_json_value(&Yaml::String("test".to_string())),
            serde_json::json!("test")
        );
        assert_eq!(yaml_to_json_value(&Yaml::BadValue), serde_json::Value::Null);
        assert_eq!(yaml_to_json_value(&Yaml::Alias(0)), serde_json::Value::Null);
    }

    #[test]
    fn test_yaml_to_json_value_real() {
        let result = yaml_to_json_value(&Yaml::Real("1.234".to_string()));
        if let serde_json::Value::Number(n) = result {
            assert!((n.as_f64().unwrap() - 1.234).abs() < 0.001);
        } else {
            panic!("Expected Number");
        }
    }

    #[test]
    fn test_yaml_to_json_value_array() {
        let yaml = Yaml::Array(vec![Yaml::Integer(1), Yaml::Integer(2)]);
        assert_eq!(yaml_to_json_value(&yaml), serde_json::json!([1, 2]));
    }

    #[test]
    fn test_yaml_to_json_value_hash() {
        let mut hash = yaml_rust2::yaml::Hash::new();
        hash.insert(Yaml::String("key".to_string()), Yaml::Integer(42));
        let yaml = Yaml::Hash(hash);
        assert_eq!(yaml_to_json_value(&yaml), serde_json::json!({"key": 42}));
    }
}
