# k-249: namingConvention Analysis

Created: 2025-10-27

## Problem Statement

Add support for the `namingConvention` field in object schemas. This is a Quarto extension that validates property names follow specific naming conventions (camelCase, snake_case, kebab-case, etc.).

## What is namingConvention?

`namingConvention` is a Quarto-specific extension that provides case convention validation for object property keys. It's mutually exclusive with `propertyNames`.

### Supported Values

From quarto-cli's from-yaml.ts, the field can be:
1. A string value (one convention)
2. An array of strings (multiple allowed conventions)
3. The special value "ignore" (disable convention checking)

### Canonical Forms

The TypeScript code normalizes various input formats to canonical forms:

| Input Variations | Canonical Form | Description |
|------------------|----------------|-------------|
| `camelCase`, `capitalizationCase`, `camel-case`, `camel_case`, `capitalization-case`, `capitalization_case` | `capitalizationCase` | myVariableName |
| `snakeCase`, `underscoreCase`, `snake-case`, `snake_case`, `underscore-case`, `underscore_case` | `underscore_case` | my_variable_name |
| `dashCase`, `kebabCase`, `dash-case`, `dash_case`, `kebab-case`, `kebab_case` | `dash-case` | my-variable-name |
| `ignore` | `ignore` | No validation |

## Relationship to propertyNames

From quarto-cli common.ts:

```typescript
// 2023-01-17: we no longer support propertyNames _and_ case convention detection.
// if propertyNames are defined, we don't add case convention detection.
if (propertyNames !== undefined) {
  console.warn(
    "Warning: propertyNames and case convention detection are mutually exclusive.",
  );
}
```

**Rule**: `namingConvention` and `propertyNames` are mutually exclusive. If both are specified, `propertyNames` wins and `namingConvention` should be ignored (with a warning).

## Real World Examples

### Example 1: Single convention
```yaml
object:
  properties:
    firstName: string
    lastName: string
  namingConvention: camelCase
```

### Example 2: Multiple conventions
```yaml
object:
  properties:
    first_name: string
    last-name: string
  namingConvention:
    - snake_case
    - kebab-case
```

### Example 3: Ignore (disable validation)
```yaml
object:
  properties:
    whatever_Case: string
    MixedCase: string
  namingConvention: ignore
```

### Example 4: From schema.yml
```yaml
object:
  properties:
    namingConvention:
      anyOf:
        - enum: ["ignore"]
        - arrayOf:
            enum:
              - camelCase
              - snake_case
              - kebab-case
  namingConvention: ignore  # This object itself uses ignore
```

## Current Implementation

In `src/schema/types.rs`, `ObjectSchema` doesn't have this field yet:

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
    pub property_names: Option<Box<Schema>>,
    // TODO: Add naming_convention field
}
```

## Implementation Plan

### Option 1: Parse and Store (Recommended)

Parse the YAML value and store it for future validation use.

**Type Definition**:
```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum NamingConvention {
    Single(String),
    Multiple(Vec<String>),
}

pub struct ObjectSchema {
    // ... existing fields ...
    pub property_names: Option<Box<Schema>>,
    /// Quarto extension: naming convention for property names
    pub naming_convention: Option<NamingConvention>,
}
```

**Parsing**:
```rust
// Parse namingConvention if present
let naming_convention = if let Some(nc_yaml) = yaml.get_hash_value("namingConvention") {
    if let Some(s) = nc_yaml.yaml.as_str() {
        // Single string value
        Some(NamingConvention::Single(normalize_convention(s)?))
    } else if let Some(arr) = nc_yaml.as_array() {
        // Array of strings
        let conventions: SchemaResult<Vec<_>> = arr
            .iter()
            .map(|item| {
                item.yaml.as_str()
                    .map(|s| normalize_convention(s))
                    .transpose()
                    .ok_or_else(|| SchemaError::InvalidStructure {
                        message: "namingConvention items must be strings".to_string(),
                        location: item.source_info.clone(),
                    })?
            })
            .collect();
        Some(NamingConvention::Multiple(conventions?))
    } else {
        return Err(SchemaError::InvalidStructure {
            message: "namingConvention must be a string or array of strings".to_string(),
            location: nc_yaml.source_info.clone(),
        });
    }
} else {
    None
};
```

**Normalization Function**:
```rust
fn normalize_convention(input: &str) -> SchemaResult<String> {
    match input {
        "ignore" => Ok("ignore".to_string()),

        // camelCase / capitalizationCase variants
        "camelCase" | "capitalizationCase" | "camel-case" | "camel_case" |
        "capitalization-case" | "capitalization_case" => Ok("capitalizationCase".to_string()),

        // snake_case / underscoreCase variants
        "snakeCase" | "underscoreCase" | "snake-case" | "snake_case" |
        "underscore-case" | "underscore_case" => Ok("underscore_case".to_string()),

        // kebab-case / dashCase variants
        "dashCase" | "kebabCase" | "dash-case" | "dash_case" |
        "kebab-case" | "kebab_case" => Ok("dash-case".to_string()),

        _ => Err(SchemaError::InvalidStructure {
            message: format!("Unknown naming convention: {}", input),
            location: SourceInfo::default(), // Will need actual location
        }),
    }
}
```

### Option 2: Store Raw (Simpler)

Just store the raw YAML value and defer normalization to validation time.

**Pros**: Simpler parsing
**Cons**: Validation logic needs to handle all variants

**Not recommended** - better to normalize once during parsing.

## Validation of Mutual Exclusivity

Should we enforce that `propertyNames` and `namingConvention` are mutually exclusive?

**From quarto-cli**: They only warn, they don't error. The behavior is:
- If `propertyNames` is defined, `namingConvention` is ignored
- A warning is issued

**Recommendation**: For parsing phase, just store both. When validation is implemented, we can:
1. Warn if both are present
2. Use `propertyNames` and ignore `namingConvention`

For now (parsing only), no validation needed.

## Tests to Add

```rust
#[test]
fn test_naming_convention_single_camel_case() {
    let yaml = quarto_yaml::parse(
        r#"
object:
  properties:
    firstName: string
    lastName: string
  namingConvention: camelCase
"#,
    ).unwrap();
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
fn test_naming_convention_multiple() {
    let yaml = quarto_yaml::parse(
        r#"
object:
  namingConvention:
    - snake_case
    - kebab-case
"#,
    ).unwrap();
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
fn test_naming_convention_ignore() {
    let yaml = quarto_yaml::parse(
        r#"
object:
  namingConvention: ignore
"#,
    ).unwrap();
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
fn test_naming_convention_normalization() {
    // Test that variants are normalized
    let variants = vec![
        ("camelCase", "capitalizationCase"),
        ("snake_case", "underscore_case"),
        ("kebab-case", "dash-case"),
        ("camel-case", "capitalizationCase"),
        ("underscore-case", "underscore_case"),
    ];

    for (input, expected) in variants {
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
        }
    }
}
```

## Files to Modify

1. `src/schema/types.rs` - Add `NamingConvention` enum and field to ObjectSchema
2. `src/schema/parsers/objects.rs` - Add parsing logic with normalization
3. `src/schema/mod.rs` - Add tests
4. SCHEMA-FROM-YAML.md - Document the feature

## Estimated Effort

- Define NamingConvention type: 10 minutes
- Add normalize_convention function: 20 minutes
- Update ObjectSchema: 5 minutes
- Update parse_object_schema: 20 minutes
- Add 4 tests: 40 minutes
- Test with real schemas: 15 minutes
- Documentation: 10 minutes

**Total**: ~2 hours

## Future Work (Validation)

When implementing validation:
1. Check if both `property_names` and `naming_convention` are set
   - If so, warn and use `property_names`
2. If `naming_convention` is set:
   - For each property key, check if it matches the convention(s)
   - For `Multiple`, the key must match at least one convention
   - For `"ignore"`, skip validation
3. Define validation regex for each convention:
   - `capitalizationCase`: `^[a-z][a-zA-Z0-9]*$` (starts lowercase, then mixed case)
   - `underscore_case`: `^[a-z][a-z0-9_]*$` (lowercase with underscores)
   - `dash-case`: `^[a-z][a-z0-9-]*$` (lowercase with dashes)

## Open Questions

1. **Should we error on unknown conventions during parsing?**
   - **Yes** - Better to fail fast with a clear error message
   - This matches JSON Schema philosophy of strict validation

2. **Should we validate mutual exclusivity with propertyNames?**
   - **No** - Just store both during parsing
   - Validation phase can handle the warning/priority

3. **Should we support PascalCase?**
   - quarto-cli doesn't explicitly support it
   - Stick to the three conventions they support

## References

- quarto-cli from-yaml.ts: lines ~280-330
- quarto-cli common.ts: ObjectSchema type definition
- Real example: test-fixtures/schemas/schema.yml (uses `namingConvention: ignore`)
