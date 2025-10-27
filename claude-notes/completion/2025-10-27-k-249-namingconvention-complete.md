# k-249: namingConvention Support - Implementation Complete

Date: 2025-10-27
Issue: k-249
Status: ✅ Complete

## Summary

Successfully implemented `namingConvention` support for object schemas. This Quarto extension allows validating that property names follow specific naming conventions (camelCase, snake_case, kebab-case). Includes normalization of multiple input formats to canonical forms.

## Implementation Details

### 1. Defined NamingConvention Enum

**File**: `src/schema/types.rs`

Added new enum type:
```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum NamingConvention {
    /// Single naming convention
    Single(String),
    /// Multiple allowed naming conventions (property must match at least one)
    Multiple(Vec<String>),
}
```

Added field to ObjectSchema:
```rust
pub struct ObjectSchema {
    // ... existing fields ...
    pub naming_convention: Option<NamingConvention>,
}
```

### 2. Created Normalization Function

**File**: `src/schema/parsers/objects.rs`

Added `normalize_convention()` function that accepts multiple input formats and normalizes them:

| Input Variations | Canonical Form |
|------------------|----------------|
| `camelCase`, `capitalizationCase`, `camel-case`, `camel_case` | `capitalizationCase` |
| `snakeCase`, `underscoreCase`, `snake-case`, `snake_case` | `underscore_case` |
| `dashCase`, `kebabCase`, `dash-case`, `dash_case` | `dash-case` |
| `ignore` | `ignore` |

### 3. Updated parse_object_schema

**File**: `src/schema/parsers/objects.rs`

Added parsing logic that:
1. Detects if `namingConvention` is a string or array
2. Normalizes each convention value
3. Creates appropriate `NamingConvention` enum variant
4. Returns error for unknown conventions

```rust
// Parse namingConvention
let naming_convention = if let Some(nc_yaml) = yaml.get_hash_value("namingConvention") {
    if let Some(s) = nc_yaml.yaml.as_str() {
        // Single string value
        Some(NamingConvention::Single(normalize_convention(s, &nc_yaml.source_info)?))
    } else if let Some(arr) = nc_yaml.as_array() {
        // Array of strings
        let conventions: SchemaResult<Vec<_>> = arr
            .iter()
            .map(|item| {
                item.yaml.as_str()
                    .ok_or_else(|| SchemaError::InvalidStructure { ... })
                    .and_then(|s| normalize_convention(s, &item.source_info))
            })
            .collect();
        Some(NamingConvention::Multiple(conventions?))
    } else {
        return Err(SchemaError::InvalidStructure { ... });
    }
} else {
    None
};
```

### 4. Added Tests

**File**: `src/schema/mod.rs`

Added 4 comprehensive tests:
1. `test_from_yaml_naming_convention_single` - Single convention parsing
2. `test_from_yaml_naming_convention_multiple` - Multiple conventions
3. `test_from_yaml_naming_convention_ignore` - Special "ignore" value
4. `test_from_yaml_naming_convention_normalization` - Tests all format variants normalize correctly

### 5. Exported Type

**File**: `src/schema/mod.rs`

Added `NamingConvention` to public exports so it's available to tests and external users.

## Test Results

All tests passing:
- **Unit tests**: 55 passed (up from 51)
- **Integration tests (comprehensive_schemas)**: 5 passed
- **Integration tests (real_schemas)**: 6 passed
- **Doc tests**: 2 passed

**Total**: 68 tests, 0 failures

## Usage Examples

### Example 1: Single convention
```yaml
object:
  properties:
    firstName: string
    lastName: string
  namingConvention: camelCase  # Normalized to "capitalizationCase"
```

### Example 2: Multiple conventions
```yaml
object:
  namingConvention:
    - snake_case   # Normalized to "underscore_case"
    - kebab-case   # Normalized to "dash-case"
```

### Example 3: Ignore validation
```yaml
object:
  namingConvention: ignore  # Special value to disable validation
```

### Example 4: Variant normalization
All of these normalize to `capitalizationCase`:
- `camelCase`
- `capitalizationCase`
- `camel-case`
- `camel_case`
- `capitalization-case`
- `capitalization_case`

## Normalization Details

The normalization function accepts 3 canonical forms plus various aliases:

**1. capitalizationCase (camelCase)**
- Accepts: `camelCase`, `capitalizationCase`, `camel-case`, `camel_case`, `capitalization-case`, `capitalization_case`
- Pattern (future): `^[a-z][a-zA-Z0-9]*$`

**2. underscore_case (snake_case)**
- Accepts: `snakeCase`, `underscoreCase`, `snake-case`, `snake_case`, `underscore-case`, `underscore_case`
- Pattern (future): `^[a-z][a-z0-9_]*$`

**3. dash-case (kebab-case)**
- Accepts: `dashCase`, `kebabCase`, `dash-case`, `dash_case`, `kebab-case`, `kebab_case`
- Pattern (future): `^[a-z][a-z0-9-]*$`

**4. ignore**
- Accepts: `ignore`
- Disables naming convention validation

## Relationship to propertyNames

From quarto-cli, `namingConvention` and `propertyNames` are mutually exclusive:
- If both are specified, `propertyNames` takes precedence
- A warning should be issued (future validation phase)

For now (parsing only), both fields can coexist in the parsed schema.

## Future Work (Validation)

When validation is implemented:
1. Check if both `property_names` and `naming_convention` are set
   - If so, warn and use `property_names`
2. For `NamingConvention::Single(convention)`:
   - Validate each property key matches the convention
   - Use regex patterns based on canonical form
3. For `NamingConvention::Multiple(conventions)`:
   - Property key must match at least one convention
4. For `"ignore"`:
   - Skip all naming convention validation

## Files Modified

1. `src/schema/types.rs` - Added NamingConvention enum and field
2. `src/schema/parsers/objects.rs` - Added normalize_convention and parsing logic
3. `src/schema/mod.rs` - Added 4 tests and exported NamingConvention

## Actual Time

- Analysis: 15 minutes (created analysis document)
- Implementation: 1 hour
- Testing: 15 minutes
- **Total**: ~1.5 hours (matched estimate)

## Compatibility

✅ 100% backward compatible - all existing schemas continue to work
✅ 100% quarto-cli compatible - supports all convention variants and normalization
✅ Quarto extension - this is a Quarto-specific feature, not in JSON Schema standard
