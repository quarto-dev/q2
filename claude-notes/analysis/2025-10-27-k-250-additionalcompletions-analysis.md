# k-250: additionalCompletions Analysis

Created: 2025-10-27

## Problem Statement

Add support for the `additionalCompletions` annotation. This is a Quarto extension that allows specifying completions that should be **merged** with existing completions, as opposed to `completions` which **overwrites** existing completions.

## What is additionalCompletions?

`additionalCompletions` is an annotation (like `description`, `completions`, `hidden`) that provides IDE completion suggestions. The key difference from `completions`:

- `completions`: **Overwrites** any existing completions
- `additionalCompletions`: **Merges** with existing completions (appends to them)

## How It Works in quarto-cli

From `external-sources/quarto-cli/src/core/lib/yaml-schema/from-yaml.ts`:

```typescript
function setBaseSchemaProperties(yaml: any, schema: ConcreteSchema): ConcreteSchema {
  if (yaml.additionalCompletions) {
    schema = completeSchema(schema, ...yaml.additionalCompletions);
  }
  if (yaml.completions) {
    schema = completeSchemaOverwrite(schema, ...yaml.completions);
  }
  // ... other properties
}
```

From `common.ts`:

```typescript
export function completeSchema<T extends ConcreteSchema>(
  schema: T,
  ...completions: string[]
): T {
  const result = Object.assign({}, schema);
  const prevCompletions = (schema.completions || []).slice();
  prevCompletions.push(...completions);
  result.completions = prevCompletions;
  return result;
}

export function completeSchemaOverwrite<T extends ConcreteSchema>(
  schema: T,
  ...completions: string[]
): T {
  const result = Object.assign({}, schema);
  result.completions = completions;
  return result;
}
```

**Key points:**
1. `additionalCompletions` is processed **before** `completions`
2. `additionalCompletions` **appends** to existing completions
3. `completions` **replaces** all completions (including those from `additionalCompletions`)
4. Both are arrays of strings

## Order of Application

The order matters:

```yaml
# Example 1: additionalCompletions only
string:
  completions: ["a", "b"]
additionalCompletions: ["c", "d"]
# Result: ["a", "b", "c", "d"]

# Example 2: Both (completions overwrites)
string:
  completions: ["a", "b"]
additionalCompletions: ["c", "d"]
completions: ["e", "f"]
# Result: ["e", "f"]  (completions overwrites everything)

# Example 3: Inner + outer additionalCompletions
schema:
  string:
    completions: ["a", "b"]
additionalCompletions: ["c", "d"]
# Result: ["a", "b", "c", "d"]
```

## Real World Examples

### Example from schema.yml

```yaml
object:
  properties:
    additionalCompletions:
      arrayOf: string
    completions:
      arrayOf: string
```

This is in the schema definition file itself - it shows that `additionalCompletions` is a valid annotation that takes an array of strings.

### Practical Usage Example

```yaml
# Base schema has some built-in completions
string:
  completions: ["default", "common"]

# User schema adds more without replacing the built-in ones
additionalCompletions: ["custom1", "custom2"]
# Final completions: ["default", "common", "custom1", "custom2"]
```

## Current Implementation

In `src/schema/types.rs`, `SchemaAnnotations` has:

```rust
pub struct SchemaAnnotations {
    pub id: Option<String>,
    pub description: Option<String>,
    pub documentation: Option<String>,
    pub error_message: Option<String>,
    pub hidden: Option<bool>,
    pub completions: Option<Vec<String>>,  // Exists
    pub tags: Option<HashMap<String, serde_json::Value>>,
    // TODO: Add additional_completions field
}
```

## Implementation Plan

### Option 1: Store Separately (Recommended)

Store `additional_completions` as a separate field in `SchemaAnnotations`. During annotation merging (like in schema wrapper), handle the merging logic.

**Type Addition**:
```rust
pub struct SchemaAnnotations {
    // ... existing fields ...
    pub completions: Option<Vec<String>>,
    /// Additional completions to merge with existing completions
    pub additional_completions: Option<Vec<String>>,
    pub tags: Option<HashMap<String, serde_json::Value>>,
}
```

**Parsing**:
```rust
// In src/schema/annotations.rs
pub(super) fn parse_annotations(yaml: &YamlWithSourceInfo) -> SchemaResult<SchemaAnnotations> {
    Ok(SchemaAnnotations {
        id: get_hash_string(yaml, "$id")?,
        description: get_hash_string(yaml, "description")?,
        documentation: get_hash_string(yaml, "documentation")?,
        error_message: get_hash_string(yaml, "errorMessage")?,
        hidden: get_hash_bool(yaml, "hidden")?,
        completions: get_hash_string_array(yaml, "completions")?,
        additional_completions: get_hash_string_array(yaml, "additionalCompletions")?,
        tags: get_hash_tags(yaml)?,
    })
}
```

**Merging Logic** (in `merge_annotations`):
```rust
pub(super) fn merge_annotations(
    inner: SchemaAnnotations,
    outer: SchemaAnnotations,
) -> SchemaAnnotations {
    // First, merge additional_completions with base completions
    let mut merged_completions = inner.completions.clone().unwrap_or_default();

    // Add inner additional_completions
    if let Some(add_comp) = inner.additional_completions {
        merged_completions.extend(add_comp);
    }

    // Add outer additional_completions
    if let Some(add_comp) = outer.additional_completions {
        merged_completions.extend(add_comp);
    }

    // If outer completions is set, it overwrites everything
    let final_completions = if outer.completions.is_some() {
        outer.completions
    } else if !merged_completions.is_empty() {
        Some(merged_completions)
    } else {
        None
    };

    SchemaAnnotations {
        id: outer.id.or(inner.id),
        description: outer.description.or(inner.description),
        documentation: outer.documentation.or(inner.documentation),
        error_message: outer.error_message.or(inner.error_message),
        hidden: outer.hidden.or(inner.hidden),
        completions: final_completions,
        additional_completions: None,  // Clear after merging
        tags: merge_tags(inner.tags, outer.tags),
    }
}
```

### Option 2: Store Combined (Simpler but Less Clear)

Don't store `additional_completions` separately - just merge it into `completions` during parsing.

**Cons**: Loses the distinction during parsing, harder to debug, harder to round-trip serialize.

**Not recommended**.

## Implementation Details

### 1. Update SchemaAnnotations Type

**File**: `src/schema/types.rs`

```rust
pub struct SchemaAnnotations {
    // ... existing fields ...
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completions: Option<Vec<String>>,

    /// Additional completions to merge with existing completions (Quarto extension)
    #[serde(rename = "additionalCompletions", skip_serializing_if = "Option::is_none")]
    pub additional_completions: Option<Vec<String>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<HashMap<String, serde_json::Value>>,
}
```

### 2. Update parse_annotations

**File**: `src/schema/annotations.rs`

```rust
pub(super) fn parse_annotations(yaml: &YamlWithSourceInfo) -> SchemaResult<SchemaAnnotations> {
    Ok(SchemaAnnotations {
        id: get_hash_string(yaml, "$id")?,
        description: get_hash_string(yaml, "description")?,
        documentation: get_hash_string(yaml, "documentation")?,
        error_message: get_hash_string(yaml, "errorMessage")?,
        hidden: get_hash_bool(yaml, "hidden")?,
        completions: get_hash_string_array(yaml, "completions")?,
        additional_completions: get_hash_string_array(yaml, "additionalCompletions")?,
        tags: get_hash_tags(yaml)?,
    })
}
```

### 3. Update EMPTY_ANNOTATIONS

**File**: `src/schema/annotations.rs`

```rust
pub(super) static EMPTY_ANNOTATIONS: SchemaAnnotations = SchemaAnnotations {
    id: None,
    description: None,
    documentation: None,
    error_message: None,
    hidden: None,
    completions: None,
    additional_completions: None,  // NEW
    tags: None,
};
```

### 4. Update merge_annotations

**File**: `src/schema/annotations.rs`

This is the key logic - handle the merging properly:

```rust
pub(super) fn merge_annotations(
    inner: SchemaAnnotations,
    outer: SchemaAnnotations,
) -> SchemaAnnotations {
    // Merge completions according to quarto-cli semantics:
    // 1. Start with inner.completions
    // 2. Append inner.additional_completions
    // 3. Append outer.additional_completions
    // 4. If outer.completions exists, it overwrites everything

    let mut merged_completions = inner.completions.unwrap_or_default();

    // Add inner additional completions
    if let Some(add_comp) = inner.additional_completions {
        merged_completions.extend(add_comp);
    }

    // Add outer additional completions
    if let Some(add_comp) = &outer.additional_completions {
        merged_completions.extend(add_comp.iter().cloned());
    }

    // Outer completions overwrites everything if present
    let final_completions = if outer.completions.is_some() {
        outer.completions
    } else if !merged_completions.is_empty() {
        Some(merged_completions)
    } else {
        None
    };

    SchemaAnnotations {
        id: outer.id.or(inner.id),
        description: outer.description.or(inner.description),
        documentation: outer.documentation.or(inner.documentation),
        error_message: outer.error_message.or(inner.error_message),
        hidden: outer.hidden.or(inner.hidden),
        completions: final_completions,
        additional_completions: None,  // Clear after merging
        tags: merge_tags(inner.tags, outer.tags),
    }
}
```

### 5. Add Tests

**File**: `src/schema/mod.rs`

```rust
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
            Some(vec!["a".to_string(), "b".to_string(), "c".to_string(), "d".to_string()])
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
fn test_additional_completions_only() {
    let yaml = quarto_yaml::parse(
        r#"
string:
  additionalCompletions: ["x", "y"]
"#,
    )
    .unwrap();
    let schema = Schema::from_yaml(&yaml).unwrap();
    if let Schema::String(s) = schema {
        // Without base completions, additional should become completions
        assert_eq!(
            s.annotations.completions,
            Some(vec!["x".to_string(), "y".to_string()])
        );
    } else {
        panic!("Expected String schema");
    }
}
```

## Files to Modify

1. `src/schema/types.rs` - Add `additional_completions` field
2. `src/schema/annotations.rs` - Parse and merge logic
3. `src/schema/mod.rs` - Add tests

## Estimated Effort

- Update SchemaAnnotations type: 5 minutes
- Update parse_annotations: 5 minutes
- Update EMPTY_ANNOTATIONS: 2 minutes
- Update merge_annotations: 30 minutes (needs careful logic)
- Add 3 tests: 30 minutes
- Test with real schemas: 15 minutes
- Documentation: 10 minutes

**Total**: ~1.5 hours

## Important Notes

1. **Order matters**: `additionalCompletions` is applied before `completions`
2. **Merging clears the field**: After merging, `additional_completions` should be `None`
3. **Only applies to wrapper/outer contexts**: The merging logic is mainly used in schema wrappers where outer and inner annotations are merged

## Future Work (Validation/IDE)

When implementing IDE completion support:
- Use the final merged `completions` array
- `additional_completions` should always be `None` after merging
- Completions can be used for autocomplete suggestions

## References

- quarto-cli from-yaml.ts: `setBaseSchemaProperties` function
- quarto-cli common.ts: `completeSchema` and `completeSchemaOverwrite` functions
- Real example: test-fixtures/schemas/schema.yml (schema definition itself has `additionalCompletions` as a field)
