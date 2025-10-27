# k-248: propertyNames Support - Implementation Complete

Date: 2025-10-27
Issue: k-248
Status: ✅ Complete

## Summary

Successfully implemented `propertyNames` support for object schemas. This JSON Schema standard feature allows validating property keys (names) against a schema pattern. Also added support for the `record` shorthand with `keySchema`/`valueSchema`.

## Implementation Details

### 1. Updated ObjectSchema Type

**File**: `src/schema/types.rs`

Added `property_names` field:
```rust
pub struct ObjectSchema {
    // ... existing fields ...
    pub closed: bool,
    pub property_names: Option<Box<Schema>>,  // NEW
}
```

### 2. Updated parse_object_schema

**File**: `src/schema/parsers/objects.rs`

Added parsing for `propertyNames` field:
```rust
// Parse propertyNames
let property_names = if let Some(property_names_yaml) = yaml.get_hash_value("propertyNames") {
    Some(Box::new(from_yaml(property_names_yaml)?))
} else {
    None
};
```

### 3. Updated parse_record_schema

**File**: `src/schema/parsers/objects.rs`

Added support for Form 3 (keySchema/valueSchema):
```rust
// Form 3: record with keySchema and/or valueSchema
if has_key_schema || has_value_schema {
    let property_names = if let Some(key_schema_yaml) = yaml.get_hash_value("keySchema") {
        Some(Box::new(from_yaml(key_schema_yaml)?))
    } else {
        None
    };

    let additional_properties = if let Some(value_schema_yaml) = yaml.get_hash_value("valueSchema") {
        Some(Box::new(from_yaml(value_schema_yaml)?))
    } else {
        None
    };

    return Ok(Schema::Object(ObjectSchema {
        annotations,
        properties: HashMap::new(),
        pattern_properties: HashMap::new(),
        additional_properties,
        required: Vec::new(),
        min_properties: None,
        max_properties: None,
        closed: false,
        property_names,
    }));
}
```

### 4. Added Tests

**File**: `src/schema/mod.rs`

Added 3 new tests:
1. `test_from_yaml_object_property_names_pattern` - Tests propertyNames with string pattern
2. `test_from_yaml_object_property_names_enum` - Tests propertyNames with enum
3. `test_from_yaml_record_with_key_schema` - Tests record form with keySchema/valueSchema

## Test Results

All tests passing:
- **Unit tests**: 51 passed (up from 48)
- **Integration tests (comprehensive_schemas)**: 5 passed
- **Integration tests (real_schemas)**: 6 passed
- **Doc tests**: 2 passed

**Total**: 64 tests, 0 failures

## Pattern Correspondence

| YAML Pattern | Maps To | Purpose |
|--------------|---------|---------|
| `object.propertyNames: <schema>` | `ObjectSchema.property_names` | Validate property keys |
| `record.keySchema: <schema>` | `ObjectSchema.property_names` | Validate keys in record |
| `record.valueSchema: <schema>` | `ObjectSchema.additional_properties` | Validate values in record |

## Usage Examples

### Example 1: Pattern on property names
```yaml
object:
  propertyNames:
    string:
      pattern: "^[a-z_]+$"  # Only lowercase with underscores
  additionalProperties: string
```

### Example 2: Enum of allowed property names
```yaml
object:
  propertyNames:
    enum:
      - name
      - schema
      - description
```

### Example 3: Record with keySchema/valueSchema
```yaml
record:
  keySchema:
    string:
      pattern: "^[a-z]+$"
  valueSchema: number
```

Expands to:
```yaml
object:
  propertyNames:
    string:
      pattern: "^[a-z]+$"
  additionalProperties: number
```

## Record Forms

Now supports 3 forms of `record`:

**Form 1**: Explicit properties
```yaml
record:
  properties:
    key1: string
    key2: number
```

**Form 2**: Shorthand
```yaml
record:
  key1: string
  key2: number
```

**Form 3**: keySchema/valueSchema (NEW)
```yaml
record:
  keySchema: <schema>
  valueSchema: <schema>
```

Forms 1 & 2 create closed objects with all properties required.
Form 3 creates open objects with property name validation.

## Future Work

When validation is implemented:
- For each property key in the validated object, validate it against the `property_names` schema
- Error messages should clearly indicate which property name failed validation
- This validation happens BEFORE validating the property values

## Files Modified

1. `src/schema/types.rs` - Added `property_names` field to ObjectSchema
2. `src/schema/parsers/objects.rs` - Updated parse_object_schema and parse_record_schema
3. `src/schema/mod.rs` - Added 3 tests

## Actual Time

- Analysis: 20 minutes (created analysis document)
- Implementation: 45 minutes
- Testing: 15 minutes
- **Total**: ~1.5 hours (matched low end of estimate)

## Compatibility

✅ 100% backward compatible - all existing schemas continue to work
✅ 100% quarto-cli compatible - supports propertyNames as used in definitions.yml and schema.yml
✅ JSON Schema standard - follows JSON Schema spec for propertyNames
