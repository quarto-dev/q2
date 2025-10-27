# k-246: Schema Inheritance Implementation Plan

**Created**: 2025-10-27
**Issue**: k-246 - Implement schema inheritance (super/baseSchema)
**Estimated Time**: 3-4 hours
**Priority**: P2 (Medium - needed before P2/P3 patterns are fully usable)

## Executive Summary

This plan implements quarto-cli's `super` field for object schema inheritance. This is a **critical missing piece** that blocks parsing of meta-schemas and many advanced quarto-cli schemas.

### What We're Building

```yaml
# quarto-cli schema with inheritance
- id: twitter-card-config
  object:
    super:
      resolveRef: social-metadata  # Inherit properties from social-metadata
    closed: true
    properties:
      card-style:
        enum: [summary, summary_large_image]
```

This expands to an object schema that **merges** the base schema's properties, required fields, and additional properties with the derived schema's specifications.

## Background: How Inheritance Works in quarto-cli

### TypeScript Implementation (common.ts:135-409)

The `objectSchema()` function in quarto-cli accepts a `baseSchema` parameter:

```typescript
export function objectSchema(params: {
  properties?: { [k: string]: Schema };
  patternProperties?: { [k: string]: Schema };
  required?: string[];
  additionalProperties?: Schema;
  baseSchema?: ObjectSchema | ObjectSchema[];  // ← Inheritance!
  // ... other fields
} = {}): ObjectSchema
```

### Merging Logic (common.ts:221-403)

When `baseSchema` is provided:

1. **Normalize to array**: `baseSchema` can be single or array → always convert to array
2. **Validate types**: All base schemas must be ObjectSchemas
3. **Merge properties**: `Object.assign({}, ...baseSchema.map(s => s.properties), properties)`
   - Base schema properties come first
   - Derived properties override base properties (last wins)
4. **Merge patternProperties**: Similar to properties
5. **Merge required**: Flatten and concatenate all required arrays
6. **Merge additionalProperties**: Use `allOf` to combine all additionalProperties schemas
7. **Merge propertyNames**: Use `anyOf` if multiple base schemas have propertyNames
8. **Merge closed**: Derived is closed if ANY base is closed OR derived specifies closed
9. **Remove $id**: Base schema $ids are not propagated to avoid duplicate IDs

### How `super` is Parsed (from-yaml.ts:407-413)

```typescript
if (schema["super"]) {
  if (Array.isArray(schema["super"])) {
    params.baseSchema = schema["super"].map((s) => convertFromYaml(s));
  } else {
    params.baseSchema = convertFromYaml(schema["super"]);
  }
}
```

**Key insight**: `super` field value is converted using `convertFromYaml()`, which means:
- It can be a single schema or array of schemas
- Most common pattern: `{ resolveRef: "schema-id" }`
- `resolveRef` immediately looks up and returns the actual schema
- The result is passed as `baseSchema` to `objectS()`

## Current State in Rust

### What's Already Implemented

✅ **resolveRef support**: `RefSchema` has `eager` field (k-247 completed)
```rust
pub struct RefSchema {
    pub reference: String,
    pub eager: bool,  // true for resolveRef
}
```

✅ **required: "all"**: Object parser expands "all" to property keys

✅ **Most object schema features**: properties, patternProperties, additionalProperties, etc.

### What's Missing

❌ **ObjectSchema.base_schema field**: No field to store inherited schemas
❌ **Parser support for `super`**: Object parser doesn't extract `super` field
❌ **Merging logic**: No implementation of schema combination

## Design Decisions

### Decision 1: When to Resolve References?

**Problem**: `resolveRef` in quarto-cli returns the actual schema immediately. But we don't have the registry during parsing.

**Options**:
A. Pass SchemaRegistry to `from_yaml()` - requires architectural changes
B. Store unresolved `Schema::Ref` with `eager: true` and resolve later
C. Create a two-phase parsing system

**CHOSEN: Option B** (defer resolution)

**Rationale**:
1. Keeps parsing simple and stateless
2. Schema::Ref with eager=true preserves intent
3. Resolution can happen during validation or in a separate pass
4. Avoids circular dependency issues during parsing

**Implementation**:
- `base_schema: Option<Vec<Schema>>` can contain `Schema::Ref` or actual schemas
- When building a validator/registry, eager refs should be resolved first
- Merging logic will need to handle both resolved and unresolved refs

### Decision 2: Single or Multiple Base Schemas?

**quarto-cli**: Supports both single and array
```typescript
baseSchema?: ObjectSchema | ObjectSchema[]
```

**CHOSEN**: Always store as `Option<Vec<Schema>>`

**Rationale**:
1. Simpler to have one representation internally
2. Single value → wrapped in Vec during parsing
3. Merging logic works uniformly

### Decision 3: Validate Base Schema Types?

**Problem**: What if user provides `{ super: string }` (non-object)?

**Options**:
A. Validate during parsing - fail if not Object
B. Store any schema type, validate during merging
C. Don't validate - let it fail when used

**CHOSEN: Option B** (validate during merging)

**Rationale**:
1. Can't validate during parsing if we defer resolveRef resolution
2. Better error messages if we validate when actually using the schema
3. Keeps parser simple

## Implementation Plan

### Phase 1: Add base_schema Field (30 minutes)

#### 1.1 Update ObjectSchema Type

**File**: `private-crates/quarto-yaml-validation/src/schema/types.rs`

```rust
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
    pub closed: bool,
    pub property_names: Option<Box<Schema>>,
    pub naming_convention: Option<NamingConvention>,

    // NEW: Base schemas for inheritance
    /// Base schemas to inherit from (via `super` field in YAML).
    /// Can contain Schema::Ref with eager=true (to be resolved later)
    /// or actual ObjectSchema instances (if already resolved).
    pub base_schema: Option<Vec<Schema>>,
}
```

**Changes needed**:
- Add the field
- Update all ObjectSchema constructors in the codebase to include `base_schema: None`

#### 1.2 Update Object Parser

**File**: `private-crates/quarto-yaml-validation/src/schema/parsers/objects.rs`

Add parsing logic after line 221 (after naming_convention):

```rust
// Parse super/baseSchema
let base_schema = if let Some(super_yaml) = yaml.get_hash_value("super") {
    if let Some(arr) = super_yaml.as_array() {
        // Array form: super: [schema1, schema2]
        let schemas: SchemaResult<Vec<_>> = arr
            .iter()
            .map(|item| from_yaml(item))
            .collect();
        Some(schemas?)
    } else {
        // Single schema form: super: { resolveRef: ... }
        Some(vec![from_yaml(super_yaml)?])
    }
} else {
    None
};
```

Update ObjectSchema construction (line 223):

```rust
Ok(Schema::Object(ObjectSchema {
    annotations,
    properties,
    pattern_properties,
    additional_properties,
    required,
    min_properties,
    max_properties,
    closed,
    property_names,
    naming_convention,
    base_schema,  // NEW
}))
```

#### 1.3 Update Record Parser

**File**: Same file, `parse_record_schema()` function

Update both ObjectSchema constructions (lines 300 and 367) to add:
```rust
base_schema: None,
```

**Testing**:
```rust
#[test]
fn test_object_with_super_single() {
    let yaml = parse(r#"
object:
  super:
    resolveRef: base-schema
  properties:
    name: string
"#).unwrap();

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
        }
        _ => panic!("Expected Object schema"),
    }
}

#[test]
fn test_object_with_super_array() {
    let yaml = parse(r#"
object:
  super:
    - resolveRef: base1
    - resolveRef: base2
  properties:
    name: string
"#).unwrap();

    let schema = Schema::from_yaml(&yaml).unwrap();
    match schema {
        Schema::Object(obj) => {
            assert!(obj.base_schema.is_some());
            let bases = obj.base_schema.unwrap();
            assert_eq!(bases.len(), 2);
        }
        _ => panic!("Expected Object schema"),
    }
}
```

### Phase 2: Implement Merging Logic (1.5 hours)

This is the complex part. We need to implement the quarto-cli merging semantics.

#### 2.1 Create Merging Module

**File**: `private-crates/quarto-yaml-validation/src/schema/merge.rs` (new file)

```rust
//! Schema merging logic for inheritance
//!
//! Implements quarto-cli's schema inheritance semantics when combining
//! base schemas with derived schemas via the `super` field.

use std::collections::HashMap;
use crate::error::{SchemaError, SchemaResult};
use crate::schema::{Schema, ObjectSchema, SchemaRegistry};
use crate::schema::types::SchemaAnnotations;

/// Resolve a base schema reference if it's an eager ref
///
/// Returns the resolved schema if it's a Ref with eager=true,
/// otherwise returns the schema as-is.
fn resolve_base_schema(
    schema: &Schema,
    registry: &SchemaRegistry,
) -> SchemaResult<Schema> {
    match schema {
        Schema::Ref(ref_schema) if ref_schema.eager => {
            // Eager resolution - look up in registry
            registry.resolve(&ref_schema.reference)
                .cloned()
                .ok_or_else(|| SchemaError::InvalidStructure {
                    message: format!(
                        "Cannot resolve reference '{}' - not found in registry",
                        ref_schema.reference
                    ),
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
    let base_objects: SchemaResult<Vec<_>> = resolved_bases
        .iter()
        .map(expect_object_schema)
        .collect();
    let base_objects = base_objects?;

    if base_objects.is_empty() {
        return Err(SchemaError::InvalidStructure {
            message: "base schema cannot be empty list".to_string(),
            location: quarto_yaml::SourceInfo::default(),
        });
    }

    // Start with the first base schema (shallow copy of its annotations)
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
        base_schema: None,  // Don't propagate base_schema (already merged)
    };

    // Apply remaining base schemas
    for base in base_objects.iter().skip(1) {
        // Merge annotations (later bases override earlier)
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
            result.pattern_properties.insert(key.clone(), schema.clone());
        }
    }
    for (key, schema) in &derived.pattern_properties {
        result.pattern_properties.insert(key.clone(), schema.clone());
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
        Some(Box::new(additional_props_schemas.into_iter().next().unwrap()))
    } else {
        // Combine with allOf
        Some(Box::new(Schema::AllOf(crate::schema::types::AllOfSchema {
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
                Schema::String(s) => s.annotations.tags.as_ref()
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
        Some(Box::new(Schema::AnyOf(crate::schema::types::AnyOfSchema {
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
        let mut registry = SchemaRegistry::new();

        // Create base schema
        let mut base_props = HashMap::new();
        base_props.insert("id".to_string(), Schema::String(StringSchema {
            annotations: Default::default(),
            min_length: None,
            max_length: None,
            pattern: None,
        }));

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
        derived_props.insert("name".to_string(), Schema::String(StringSchema {
            annotations: Default::default(),
            min_length: None,
            max_length: None,
            pattern: None,
        }));

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
        let merged = merge_object_schemas(
            &[Schema::Object(base)],
            &derived,
            &registry,
        ).unwrap();

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
        base_props.insert("base_field".to_string(), Schema::String(StringSchema {
            annotations: Default::default(),
            min_length: None,
            max_length: None,
            pattern: None,
        }));

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
        derived_props.insert("derived_field".to_string(), Schema::Boolean(BooleanSchema {
            annotations: Default::default(),
        }));

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
        let merged = merge_object_schemas(
            &[base_ref],
            &derived,
            &registry,
        ).unwrap();

        // Verify
        assert!(merged.properties.contains_key("base_field"));
        assert!(merged.properties.contains_key("derived_field"));
        assert_eq!(merged.required.len(), 1);
        assert_eq!(merged.required[0], "base_field");
    }
}
```

#### 2.2 Register Merge Module

**File**: `private-crates/quarto-yaml-validation/src/schema/mod.rs`

Add to the module declarations near the top:

```rust
mod merge;
pub use merge::merge_object_schemas;
```

### Phase 3: Integration and Testing (1 hour)

#### 3.1 Create Test File with Real Examples

**File**: `private-crates/quarto-yaml-validation/tests/schema_inheritance.rs` (new)

```rust
use quarto_yaml_validation::{Schema, SchemaRegistry};
use quarto_yaml;

#[test]
fn test_inheritance_from_quarto_cli() {
    // This is based on definitions.yml:
    // - id: social-metadata
    //   object:
    //     properties:
    //       title: string
    //       description: string
    //
    // - id: twitter-card-config
    //   object:
    //     super:
    //       resolveRef: social-metadata
    //     properties:
    //       card-style:
    //         enum: [summary, summary_large_image]

    let mut registry = SchemaRegistry::new();

    // Register base schema
    let base_yaml = quarto_yaml::parse(r#"
object:
  properties:
    title:
      string:
        description: "Title for social media"
    description:
      string:
        description: "Description for social media"
  required: [title]
"#).unwrap();

    let base_schema = Schema::from_yaml(&base_yaml).unwrap();
    registry.register("social-metadata".to_string(), base_schema);

    // Parse derived schema
    let derived_yaml = quarto_yaml::parse(r#"
object:
  super:
    resolveRef: social-metadata
  closed: true
  properties:
    card-style:
      enum: [summary, summary_large_image]
"#).unwrap();

    let derived_schema = Schema::from_yaml(&derived_yaml).unwrap();

    // Extract base_schema and merge
    match derived_schema {
        Schema::Object(ref obj) => {
            assert!(obj.base_schema.is_some());

            let merged = quarto_yaml_validation::merge_object_schemas(
                obj.base_schema.as_ref().unwrap(),
                obj,
                &registry,
            ).unwrap();

            // Verify merged schema has properties from both
            assert!(merged.properties.contains_key("title"));
            assert!(merged.properties.contains_key("description"));
            assert!(merged.properties.contains_key("card-style"));

            // Verify required from base
            assert!(merged.required.contains(&"title".to_string()));

            // Verify closed from derived
            assert!(merged.closed);
        }
        _ => panic!("Expected Object schema"),
    }
}

#[test]
fn test_multiple_inheritance() {
    let mut registry = SchemaRegistry::new();

    // Register base1
    let base1_yaml = quarto_yaml::parse(r#"
object:
  properties:
    field1: string
  required: [field1]
"#).unwrap();
    registry.register("base1".to_string(), Schema::from_yaml(&base1_yaml).unwrap());

    // Register base2
    let base2_yaml = quarto_yaml::parse(r#"
object:
  properties:
    field2: number
  required: [field2]
"#).unwrap();
    registry.register("base2".to_string(), Schema::from_yaml(&base2_yaml).unwrap());

    // Parse derived with multiple bases
    let derived_yaml = quarto_yaml::parse(r#"
object:
  super:
    - resolveRef: base1
    - resolveRef: base2
  properties:
    field3: boolean
"#).unwrap();

    let derived_schema = Schema::from_yaml(&derived_yaml).unwrap();

    match derived_schema {
        Schema::Object(ref obj) => {
            let merged = quarto_yaml_validation::merge_object_schemas(
                obj.base_schema.as_ref().unwrap(),
                obj,
                &registry,
            ).unwrap();

            assert_eq!(merged.properties.len(), 3);
            assert!(merged.properties.contains_key("field1"));
            assert!(merged.properties.contains_key("field2"));
            assert!(merged.properties.contains_key("field3"));

            assert_eq!(merged.required.len(), 2);
            assert!(merged.required.contains(&"field1".to_string()));
            assert!(merged.required.contains(&"field2".to_string()));
        }
        _ => panic!("Expected Object schema"),
    }
}

#[test]
fn test_property_override() {
    let mut registry = SchemaRegistry::new();

    // Base has 'name' as string
    let base_yaml = quarto_yaml::parse(r#"
object:
  properties:
    name:
      string:
        description: "Base description"
"#).unwrap();
    registry.register("base".to_string(), Schema::from_yaml(&base_yaml).unwrap());

    // Derived overrides 'name' with different constraints
    let derived_yaml = quarto_yaml::parse(r#"
object:
  super:
    resolveRef: base
  properties:
    name:
      string:
        pattern: "^[A-Z]"
        description: "Derived description"
"#).unwrap();

    let derived_schema = Schema::from_yaml(&derived_yaml).unwrap();

    match derived_schema {
        Schema::Object(ref obj) => {
            let merged = quarto_yaml_validation::merge_object_schemas(
                obj.base_schema.as_ref().unwrap(),
                obj,
                &registry,
            ).unwrap();

            // Derived should win
            match merged.properties.get("name") {
                Some(Schema::String(s)) => {
                    assert_eq!(s.pattern, Some("^[A-Z]".to_string()));
                    assert_eq!(s.annotations.description, Some("Derived description".to_string()));
                }
                _ => panic!("Expected string schema for name"),
            }
        }
        _ => panic!("Expected Object schema"),
    }
}
```

#### 3.2 Add Example to Documentation

**File**: `private-crates/quarto-yaml-validation/README.md` (or create if missing)

Add section explaining inheritance:

```markdown
## Schema Inheritance

Object schemas can inherit from base schemas using the `super` field:

```yaml
object:
  super:
    resolveRef: base-schema-id
  properties:
    derived-field: string
```

This merges the base schema's properties, required fields, and constraints
with the derived schema.

### Multiple Inheritance

You can inherit from multiple base schemas:

```yaml
object:
  super:
    - resolveRef: base1
    - resolveRef: base2
  properties:
    my-field: string
```

### Usage in Rust

```rust
use quarto_yaml_validation::{Schema, SchemaRegistry, merge_object_schemas};

let mut registry = SchemaRegistry::new();
registry.register("base".to_string(), base_schema);

// Parse schema with super field
let schema = Schema::from_yaml(&yaml)?;

// Merge inheritance
match schema {
    Schema::Object(ref obj) if obj.base_schema.is_some() => {
        let merged = merge_object_schemas(
            obj.base_schema.as_ref().unwrap(),
            obj,
            &registry,
        )?;
        // Use merged schema
    }
    _ => {}
}
```
```

### Phase 4: Edge Cases and Polish (30 minutes)

#### 4.1 Handle Edge Cases

Add tests for:

1. **Missing reference**:
```rust
#[test]
fn test_missing_reference_error() {
    let registry = SchemaRegistry::new();  // Empty

    let yaml = quarto_yaml::parse(r#"
object:
  super:
    resolveRef: non-existent
  properties:
    field: string
"#).unwrap();

    let schema = Schema::from_yaml(&yaml).unwrap();
    match schema {
        Schema::Object(ref obj) => {
            let result = quarto_yaml_validation::merge_object_schemas(
                obj.base_schema.as_ref().unwrap(),
                obj,
                &registry,
            );
            assert!(result.is_err());
            assert!(result.unwrap_err().to_string().contains("not found in registry"));
        }
        _ => panic!("Expected Object schema"),
    }
}
```

2. **Non-object base**:
```rust
#[test]
fn test_non_object_base_error() {
    let mut registry = SchemaRegistry::new();

    // Register a STRING schema (not object)
    registry.register("not-object".to_string(), Schema::String(StringSchema {
        annotations: Default::default(),
        min_length: None,
        max_length: None,
        pattern: None,
    }));

    let yaml = quarto_yaml::parse(r#"
object:
  super:
    resolveRef: not-object
  properties:
    field: string
"#).unwrap();

    let schema = Schema::from_yaml(&yaml).unwrap();
    match schema {
        Schema::Object(ref obj) => {
            let result = quarto_yaml_validation::merge_object_schemas(
                obj.base_schema.as_ref().unwrap(),
                obj,
                &registry,
            );
            assert!(result.is_err());
            assert!(result.unwrap_err().to_string().contains("must be an object schema"));
        }
        _ => panic!("Expected Object schema"),
    }
}
```

3. **Empty base list**:
```rust
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

    let result = quarto_yaml_validation::merge_object_schemas(
        &[],  // Empty array
        &derived,
        &registry,
    );
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("cannot be empty list"));
}
```

#### 4.2 Update Error Messages

Ensure error messages are helpful and reference the quarto-cli pattern:

```rust
SchemaError::InvalidStructure {
    message: format!(
        "Base schema '{}' must be an object schema (got {}). \
         The 'super' field expects object schemas for inheritance.",
        ref_name,
        schema.type_name()
    ),
    location,
}
```

## Testing Strategy

### Unit Tests
- ✅ Parse `super` field (single and array)
- ✅ Merge simple properties
- ✅ Merge with resolveRef
- ✅ Multiple inheritance
- ✅ Property override
- ✅ Required field merging
- ✅ Closed flag inheritance
- ✅ Error cases

### Integration Tests
- ✅ Real quarto-cli schema examples
- ✅ Meta-schema parsing (schema.yml)
- ✅ Definition schemas (definitions.yml)

### Regression Tests
- Ensure existing tests still pass
- No changes to non-inheritance schemas

## Success Criteria

1. ✅ `ObjectSchema.base_schema` field exists
2. ✅ Parser extracts `super` field correctly
3. ✅ Single and array `super` values work
4. ✅ `merge_object_schemas()` implements quarto-cli semantics
5. ✅ Properties merge correctly (derived overrides base)
6. ✅ Required fields concatenate
7. ✅ Closed flag inherits correctly
8. ✅ AdditionalProperties combine with allOf
9. ✅ PropertyNames combine with anyOf
10. ✅ Errors for invalid base schemas
11. ✅ Real quarto-cli schemas parse and merge correctly
12. ✅ All tests pass

## Known Limitations

1. **No automatic merging**: Users must explicitly call `merge_object_schemas()`
   - Future: Could add a convenience method on Schema or validator

2. **Registry required**: Can't merge if base is resolveRef without registry
   - This matches quarto-cli behavior (registry is always available there)

3. **No cycle detection**: Circular inheritance will cause infinite loops
   - Future enhancement: Add visited set to detect cycles

4. **No $id propagation**: Base schema $ids are stripped
   - Matches quarto-cli behavior

## Future Enhancements

1. **Automatic merging in validator**: When validating, automatically merge inherited schemas
2. **Cycle detection**: Detect and report circular inheritance
3. **Conflict detection**: Warn if multiple base schemas define same property
4. **Source location preservation**: Track which base schema contributed which property
5. **Lazy merging**: Only merge when needed for validation

## File Checklist

- [ ] `src/schema/types.rs` - Add `base_schema` field
- [ ] `src/schema/parsers/objects.rs` - Parse `super` field (2 functions)
- [ ] `src/schema/merge.rs` - NEW: Implement merging logic
- [ ] `src/schema/mod.rs` - Export merge module
- [ ] `tests/schema_inheritance.rs` - NEW: Comprehensive tests
- [ ] Update README or docs with usage examples

## Estimated Timeline

- Phase 1 (Add field): 30 minutes
- Phase 2 (Merging logic): 1.5 hours
- Phase 3 (Integration): 1 hour
- Phase 4 (Edge cases): 30 minutes
- **Total**: 3.5 hours

Add 30 minutes buffer for unexpected issues = **4 hours total**

## References

- **quarto-cli source**: `external-sources/quarto-cli/src/core/lib/yaml-schema/common.ts:135-409`
- **from-yaml parsing**: `external-sources/quarto-cli/src/core/lib/yaml-schema/from-yaml.ts:407-413`
- **Examples**: `external-sources/quarto-cli/src/resources/schema/definitions.yml` (search for "super:")
- **Audit report**: `claude-notes/audits/2025-10-27-bd-8-phase1-schema-from-yaml-audit.md`
- **k-247 analysis**: `claude-notes/analysis/2025-10-27-k-247-resolveref-analysis.md`
