# k-245: Nested Property Extraction Analysis

Created: 2025-10-27

## Problem Statement

The current `parse_schema_wrapper()` implementation parses the inner schema but does NOT apply the outer-level annotations to it. This means annotations like `description`, `completions`, etc. defined at the wrapper level are ignored.

## The Pattern in quarto-cli

From `external-sources/quarto-cli/src/core/lib/yaml-schema/from-yaml.ts`:

```typescript
function convertFromSchema(yaml: any): ConcreteSchema {
  const schema = convertFromYaml(yaml.schema);  // Parse inner schema
  return setBaseSchemaProperties(yaml, schema);  // Apply outer properties to it
}
```

The `setBaseSchemaProperties` function (lines 59-107) applies annotations from a YAML object to a schema:
- `additionalCompletions` → merged with existing completions
- `completions` → overwrites existing completions
- `id` → sets schema ID
- `hidden` → overwrites completions, adds hidden tag
- `tags` → merges with existing tags
- `description` → sets both tag and documentation
- `errorMessage` → sets error message

### Double Application Examples

**Example 1: Schema wrapper**
```yaml
schema:
  anyOf:
    - boolean
    - string
description: "Outer description"  # Applied to the anyOf schema
completions: ["true", "false", "auto"]  # Applied to the anyOf schema
```

**Example 2: String with pattern (double nesting)**
```yaml
string:
  pattern: "^[a-z]+$"
  description: "Inner description"
description: "Outer description"  # Overrides inner
completions: ["value1", "value2"]
```

In the second example (lines 124-138 of from-yaml.ts), there are TWO calls to `setBaseSchemaProperties`:
1. Apply properties from `yaml["string"]` to the regex schema
2. Apply properties from `yaml` to the result

The outer properties **override** inner ones.

## Current Implementation

In `src/schema/parsers/wrappers.rs`:

```rust
pub(in crate::schema) fn parse_schema_wrapper(yaml: &YamlWithSourceInfo) -> SchemaResult<Schema> {
    let schema_yaml = yaml.get_hash_value("schema").ok_or_else(|| {
        // ...
    })?;

    let schema = from_yaml(schema_yaml)?;  // Parse inner

    // TODO: Apply outer annotations!

    Ok(schema)  // Just return inner schema
}
```

**Problem**: The annotations from the outer `yaml` object are never extracted or applied.

## What Needs to Happen

### Step 1: Parse Outer Annotations

```rust
// Parse annotations from the OUTER yaml object
let outer_annotations = parse_annotations(yaml)?;
```

This extracts `description`, `completions`, `hidden`, `tags`, etc. from the outer level.

### Step 2: Merge with Inner Schema Annotations

Since `Schema` is an enum with variants that each have an `annotations` field, we need to:
1. Extract the inner schema's annotations
2. Merge outer annotations with inner (outer overrides inner)
3. Update the schema with merged annotations

### Step 3: Annotation Merging Logic

**Override semantics** (from quarto-cli):
- `id`: Outer overrides inner
- `description`: Outer overrides inner
- `documentation`: Outer overrides inner
- `error_message`: Outer overrides inner
- `hidden`: Outer overrides inner
- `completions`: Outer **overwrites** inner completely (not merge)
- `tags`: Outer **merges** with inner (outer values override inner values for same keys)

**Rust implementation**:
```rust
fn merge_annotations(inner: SchemaAnnotations, outer: SchemaAnnotations) -> SchemaAnnotations {
    SchemaAnnotations {
        id: outer.id.or(inner.id),
        description: outer.description.or(inner.description),
        documentation: outer.documentation.or(inner.documentation),
        error_message: outer.error_message.or(inner.error_message),
        hidden: outer.hidden.or(inner.hidden),
        completions: outer.completions.or(inner.completions),  // Outer overwrites
        tags: merge_tags(inner.tags, outer.tags),  // Merge tags
    }
}

fn merge_tags(
    inner: Option<HashMap<String, serde_json::Value>>,
    outer: Option<HashMap<String, serde_json::Value>>,
) -> Option<HashMap<String, serde_json::Value>> {
    match (inner, outer) {
        (None, None) => None,
        (Some(i), None) => Some(i),
        (None, Some(o)) => Some(o),
        (Some(mut i), Some(o)) => {
            // Outer tags override inner tags for same keys
            for (k, v) in o {
                i.insert(k, v);
            }
            Some(i)
        }
    }
}
```

### Step 4: Update Schema with Merged Annotations

This is the tedious part - we need to match on every Schema variant and update its annotations:

```rust
fn apply_annotations(mut schema: Schema, annotations: SchemaAnnotations) -> Schema {
    match schema {
        Schema::Boolean(ref mut s) => s.annotations = annotations,
        Schema::Number(ref mut s) => s.annotations = annotations,
        Schema::String(ref mut s) => s.annotations = annotations,
        Schema::Null(ref mut s) => s.annotations = annotations,
        Schema::Any(ref mut s) => s.annotations = annotations,
        Schema::Enum(ref mut s) => s.annotations = annotations,
        Schema::Array(ref mut s) => s.annotations = annotations,
        Schema::Object(ref mut s) => s.annotations = annotations,
        Schema::AnyOf(ref mut s) => s.annotations = annotations,
        Schema::AllOf(ref mut s) => s.annotations = annotations,
        Schema::Ref(ref mut s) => s.annotations = annotations,
    }
    schema
}
```

## Implementation Plan

### 1. Add Helper Functions (in `annotations.rs`)

```rust
/// Merge outer annotations with inner annotations
/// Outer annotations override inner ones
pub(super) fn merge_annotations(
    inner: SchemaAnnotations,
    outer: SchemaAnnotations,
) -> SchemaAnnotations {
    // Implementation as shown above
}
```

### 2. Add Schema Update Method (in `mod.rs` or new `helpers.rs`)

```rust
impl Schema {
    /// Apply annotations to this schema, replacing existing annotations
    pub(crate) fn with_annotations(mut self, annotations: SchemaAnnotations) -> Self {
        match &mut self {
            Schema::Boolean(s) => s.annotations = annotations,
            Schema::Number(s) => s.annotations = annotations,
            Schema::String(s) => s.annotations = annotations,
            Schema::Null(s) => s.annotations = annotations,
            Schema::Any(s) => s.annotations = annotations,
            Schema::Enum(s) => s.annotations = annotations,
            Schema::Array(s) => s.annotations = annotations,
            Schema::Object(s) => s.annotations = annotations,
            Schema::AnyOf(s) => s.annotations = annotations,
            Schema::AllOf(s) => s.annotations = annotations,
            Schema::Ref(s) => s.annotations = annotations,
        }
        self
    }

    /// Get a reference to this schema's annotations
    pub(crate) fn annotations(&self) -> &SchemaAnnotations {
        match self {
            Schema::Boolean(s) => &s.annotations,
            Schema::Number(s) => &s.annotations,
            Schema::String(s) => &s.annotations,
            Schema::Null(s) => &s.annotations,
            Schema::Any(s) => &s.annotations,
            Schema::Enum(s) => &s.annotations,
            Schema::Array(s) => &s.annotations,
            Schema::Object(s) => &s.annotations,
            Schema::AnyOf(s) => &s.annotations,
            Schema::AllOf(s) => &s.annotations,
            Schema::Ref(s) => &s.annotations,
        }
    }
}
```

### 3. Update `parse_schema_wrapper()` (in `parsers/wrappers.rs`)

```rust
pub(in crate::schema) fn parse_schema_wrapper(yaml: &YamlWithSourceInfo) -> SchemaResult<Schema> {
    // Extract the inner schema
    let schema_yaml = yaml.get_hash_value("schema").ok_or_else(|| {
        crate::error::SchemaError::InvalidStructure {
            message: "schema wrapper requires 'schema' key".to_string(),
            location: yaml.source_info.clone(),
        }
    })?;

    // Parse the inner schema (gets inner annotations)
    let inner_schema = from_yaml(schema_yaml)?;

    // Parse annotations from the OUTER wrapper
    let outer_annotations = parse_annotations(yaml)?;

    // Merge outer with inner (outer overrides inner)
    let inner_annotations = inner_schema.annotations().clone();
    let merged_annotations = merge_annotations(inner_annotations, outer_annotations);

    // Apply merged annotations to the schema
    Ok(inner_schema.with_annotations(merged_annotations))
}
```

### 4. Add Tests

```rust
#[test]
fn test_schema_wrapper_with_outer_annotations() {
    let yaml = quarto_yaml::parse(
        r#"
schema:
  anyOf:
    - boolean
    - string
description: "Outer description"
completions: ["true", "false", "auto"]
hidden: true
"#,
    )
    .unwrap();

    let schema = Schema::from_yaml(&yaml).unwrap();

    match schema {
        Schema::AnyOf(s) => {
            assert_eq!(s.annotations.description, Some("Outer description".to_string()));
            assert_eq!(
                s.annotations.completions,
                Some(vec!["true".to_string(), "false".to_string(), "auto".to_string()])
            );
            assert_eq!(s.annotations.hidden, Some(true));
        }
        _ => panic!("Expected AnyOf schema"),
    }
}

#[test]
fn test_schema_wrapper_annotation_override() {
    let yaml = quarto_yaml::parse(
        r#"
schema:
  string:
    description: "Inner description"
    completions: ["inner1", "inner2"]
description: "Outer description"
completions: ["outer1", "outer2"]
"#,
    )
    .unwrap();

    let schema = Schema::from_yaml(&yaml).unwrap();

    match schema {
        Schema::String(s) => {
            // Outer should override inner
            assert_eq!(s.annotations.description, Some("Outer description".to_string()));
            assert_eq!(
                s.annotations.completions,
                Some(vec!["outer1".to_string(), "outer2".to_string()])
            );
        }
        _ => panic!("Expected String schema"),
    }
}
```

## Scope Considerations

### Should We Apply This Pattern Everywhere?

The quarto-cli code applies double `setBaseSchemaProperties` in multiple places:
- `convertFromString` (lines 124-138)
- `convertFromNumber` (lines 140-151)
- `convertFromBoolean` (similar pattern)
- etc.

**Question**: Should we implement this for ALL schema types, not just the schema wrapper?

**Answer**: Start with just the schema wrapper (k-245), then decide if we need it elsewhere. The schema wrapper is the most explicit case where this is expected.

### Alternative: Single Annotation Layer

Could we simplify by parsing annotations only from the OUTERMOST layer?

**No** - The quarto-cli pattern explicitly supports nested annotation layers. The inner schema might have its own annotations that should be preserved if the outer layer doesn't override them.

## Estimated Effort

- **Helper functions**: 30 minutes
- **Schema methods**: 30 minutes
- **Update wrapper parser**: 15 minutes
- **Tests**: 30 minutes
- **Testing with real schemas**: 30 minutes
- **Documentation update**: 15 minutes

**Total**: ~2.5 hours

## Files to Modify

1. `src/schema/annotations.rs` - Add `merge_annotations()` helper
2. `src/schema/mod.rs` - Add `with_annotations()` and `annotations()` methods to Schema impl
3. `src/schema/parsers/wrappers.rs` - Update `parse_schema_wrapper()`
4. `tests/` - Add comprehensive tests
5. `SCHEMA-FROM-YAML.md` - Document the behavior

## Open Questions

1. **Should we implement this for other schema types too?**
   - The quarto-cli code does double application for string, number, boolean, etc.
   - Start with schema wrapper only, expand if needed

2. **Tag merging semantics?**
   - quarto-cli merges tags (outer overrides inner for same key)
   - Confirm this is the desired behavior

3. **Additional completions vs completions?**
   - `additionalCompletions` should MERGE with existing
   - `completions` should OVERWRITE
   - Currently we don't have `additionalCompletions` support (that's k-250)
   - For now, just handle `completions` as overwrite

## References

- quarto-cli source: `external-sources/quarto-cli/src/core/lib/yaml-schema/from-yaml.ts`
- Current implementation: `src/schema/parsers/wrappers.rs`
- Annotations: `src/schema/annotations.rs`
- Schema types: `src/schema/types.rs`
