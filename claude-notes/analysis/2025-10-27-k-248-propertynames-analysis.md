# k-248: propertyNames Analysis

Created: 2025-10-27

## Problem Statement

Add support for the `propertyNames` field in object schemas. This JSON Schema standard feature allows validating property keys against a schema pattern.

## What is propertyNames?

`propertyNames` is a schema that validates the **property names** (keys) of an object, not the values. It's commonly used to enforce naming conventions or patterns on property keys.

### Example Usage

```yaml
object:
  propertyNames:
    string:
      pattern: "^[a-z_]+$"  # Only lowercase with underscores
  additionalProperties: string
```

This validates that all property keys match the pattern `^[a-z_]+$`.

### Real World Examples from quarto-cli

**Example 1: No spaces in property names**
```yaml
object:
  propertyNames:
    string:
      pattern: "^[^\\s]+$"  # No whitespace in keys
```

**Example 2: Enum of allowed property names**
```yaml
object:
  propertyNames:
    enum:
      - name
      - schema
      - description
      - errorMessage
```

**Example 3: Reference to another schema**
```yaml
object:
  propertyNames:
    ref: schema/schema  # Property names must be valid schemas
```

## Current Implementation

In `src/schema/types.rs`, `ObjectSchema` already has a stub for this:

```rust
pub struct ObjectSchema {
    pub annotations: SchemaAnnotations,
    pub properties: HashMap<String, Schema>,
    pub pattern_properties: HashMap<String, Schema>,
    pub additional_properties: Option<Box<Schema>>,
    pub required: Vec<String>,
    pub min_properties: Option<usize>,
    pub max_properties: Option<usize>,
    pub closed: bool,
    // TODO: Add propertyNames field
}
```

We need to add:
```rust
pub property_names: Option<Box<Schema>>,
```

## quarto-cli Parsing Pattern

From `external-sources/quarto-cli/src/core/lib/yaml-schema/from-yaml.ts` (lines ~300-320):

```typescript
if (schema.propertyNames !== undefined) {
  params.propertyNames = convertFromYaml(schema.propertyNames);
}
```

It's a simple field that:
1. Is optional (can be undefined)
2. Contains a full schema (any schema type is valid)
3. Gets parsed recursively with `convertFromYaml`

## Implementation Plan

### 1. Update ObjectSchema Type

**File**: `src/schema/types.rs`

```rust
pub struct ObjectSchema {
    pub annotations: SchemaAnnotations,
    pub properties: HashMap<String, Schema>,
    pub pattern_properties: HashMap<String, Schema>,
    pub additional_properties: Option<Box<Schema>>,
    pub required: Vec<String>,
    pub min_properties: Option<usize>,
    pub max_properties: Option<usize>,
    pub closed: bool,
    /// Schema that property names (keys) must match
    pub property_names: Option<Box<Schema>>,
}
```

### 2. Update parse_object_schema

**File**: `src/schema/parsers/objects.rs`

Add parsing for the `propertyNames` field:

```rust
// Parse propertyNames if present
let property_names = if let Some(property_names_yaml) = yaml.get_hash_value("propertyNames") {
    Some(Box::new(from_yaml(property_names_yaml)?))
} else {
    None
};
```

Then include it in the ObjectSchema construction:

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
    property_names,  // NEW
}))
```

### 3. Update parse_record_schema

**File**: `src/schema/parsers/objects.rs`

The `record` form also creates ObjectSchema, so we need to handle it there too.

From quarto-cli, `record` is:
```yaml
record:
  keySchema: <schema>
  valueSchema: <schema>
```

Which translates to:
```yaml
object:
  propertyNames: <keySchema>
  additionalProperties: <valueSchema>
```

So `keySchema` becomes `property_names`. Update the parsing:

```rust
// Parse key schema (becomes property_names)
let property_names = if let Some(key_schema_yaml) = yaml.get_hash_value("keySchema") {
    Some(Box::new(from_yaml(key_schema_yaml)?))
} else {
    None
};
```

### 4. Add Tests

**Tests to add** (in `src/schema/mod.rs`):

```rust
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
    } else {
        panic!("Expected Object schema");
    }
}
```

### 5. Update Existing ObjectSchema Constructors

Search for all places where `ObjectSchema` is constructed and add `property_names: None` or the appropriate value.

Likely locations:
- `src/schema/parsers/objects.rs` - parse_object_schema
- `src/schema/parsers/objects.rs` - parse_record_schema
- Any test code that constructs ObjectSchema directly

## Scope

This is a **parsing-only** implementation. We're adding the field and parsing it correctly, but NOT implementing validation logic (that comes later when we build the validator).

## Compatibility

`propertyNames` is:
- ✅ JSON Schema standard feature
- ✅ Used in quarto-cli schemas (found in definitions.yml and schema.yml)
- ✅ Backward compatible (optional field)

## Relationship to Other Features

### vs patternProperties

- `patternProperties`: Validates **values** based on property name patterns
- `propertyNames`: Validates the **property names themselves**

They work together:
```yaml
object:
  propertyNames:
    string:
      pattern: "^prop_"  # All keys must start with "prop_"
  patternProperties:
    "^prop_num":
      number  # Values for keys starting with "prop_num" must be numbers
```

### vs closed

`closed: true` is a shorthand that:
1. Sets `propertyNames` to an enum of keys in `properties`
2. Disallows any property not explicitly defined

From quarto-cli:
```typescript
if (schema.closed === true) {
  const objectKeys = Object.keys(params.properties || {});
  params.closed = true;
}
```

So `closed` and `propertyNames` are mutually exclusive in practice.

### vs namingConvention (k-249)

From quarto-cli comments:
```typescript
// 2023-01-17: we no longer support propertyNames _and_ case convention detection.
// if propertyNames are defined, we don't add case convention detection.
if (propertyNames !== undefined) {
  console.warn(
    "Warning: propertyNames and case convention detection are mutually exclusive.",
  );
}
```

So `propertyNames` and `namingConvention` are mutually exclusive.

## Estimated Effort

- Update ObjectSchema type: 5 minutes
- Update parse_object_schema: 15 minutes
- Update parse_record_schema: 15 minutes
- Update existing constructors: 15 minutes
- Add 3 tests: 30 minutes
- Test with real schemas: 15 minutes
- Documentation: 10 minutes

**Total**: ~1.5-2 hours

## Files to Modify

1. `src/schema/types.rs` - Add `property_names` field to ObjectSchema
2. `src/schema/parsers/objects.rs` - Update parse_object_schema and parse_record_schema
3. `src/schema/mod.rs` - Add tests
4. SCHEMA-FROM-YAML.md - Document the feature

## Test Strategy

1. **Unit tests**: Test parsing with different propertyNames schemas (string/pattern, enum, ref)
2. **Integration tests**: Verify real quarto-cli schemas still parse (already have these)
3. **Record form**: Test that keySchema maps to property_names correctly

## Future Work (Validation)

When implementing validation:
- For each property key in the validated object, validate it against the `property_names` schema
- Error messages should clearly indicate which property name failed validation
- This validation happens BEFORE validating the property values

## References

- JSON Schema spec: https://json-schema.org/understanding-json-schema/reference/object.html#property-names
- quarto-cli from-yaml.ts: lines ~300-320
- Real examples: test-fixtures/schemas/definitions.yml and schema.yml
