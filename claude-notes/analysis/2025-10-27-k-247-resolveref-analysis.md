# k-247: resolveRef vs ref Analysis

Created: 2025-10-27

## The Distinction in quarto-cli

From `external-sources/quarto-cli/src/core/lib/yaml-schema/from-yaml.ts`:

### `ref` (line ~360)
```typescript
function convertFromRef(yaml: any): ConcreteSchema {
  return setBaseSchemaProperties(yaml, refS(yaml.ref, `be ${yaml.ref}`));
}
```

- Creates a **reference schema** using `refS()`
- The reference is **lazy** - it's not resolved immediately
- Returns a schema object that **points to** another schema
- Used for **deferred resolution** (resolve later during validation)

### `resolveRef` (line 430)
```typescript
function lookup(yaml: any): ConcreteSchema {
  if (!hasSchemaDefinition(yaml.resolveRef)) {
    throw new Error(`lookup of key ${yaml.resolveRef} in definitions failed`);
  }
  return getSchemaDefinition(yaml.resolveRef)!;
}
```

- **Immediately looks up** the schema from the registry
- Returns the **actual schema**, not a reference
- Throws error if the schema doesn't exist
- Used for **eager resolution** (resolve now during parsing)

## Key Difference

| Feature | `ref` | `resolveRef` |
|---------|-------|--------------|
| Resolution timing | Lazy (during validation) | Eager (during parsing) |
| Returns | Reference object | Actual schema |
| Error handling | Deferred to validation | Immediate on parse |
| Use case | Forward references, circular deps | Simple lookups |

## Example Usage

### Using `ref`:
```yaml
properties:
  parent:
    ref: schema/person  # Lazy - creates reference
```

Result: `Schema::Ref(RefSchema { reference: "schema/person" })`

### Using `resolveRef`:
```yaml
properties:
  parent:
    resolveRef: schema/person  # Eager - inlines the actual schema
```

Result: The actual `schema/person` schema inlined (e.g., `Schema::Object(...)`)

## Current Implementation

In `src/schema/parsers/ref.rs`:

```rust
pub(in crate::schema) fn parse_ref_schema(yaml: &YamlWithSourceInfo) -> SchemaResult<Schema> {
    let annotations = parse_annotations(yaml)?;

    // Try both "ref" and "$ref" keys
    let reference = yaml
        .get_hash_value("ref")
        .or_else(|| yaml.get_hash_value("$ref"))
        .and_then(|v| v.yaml.as_str())
        .ok_or_else(|| SchemaError::InvalidStructure {
            message: "ref schema requires 'ref' or '$ref' key with string value".to_string(),
            location: yaml.source_info.clone(),
        })?;

    Ok(Schema::Ref(RefSchema {
        annotations,
        reference: reference.to_string(),
    }))
}
```

**Problem**: We currently treat both `ref` and `$ref` the same way (lazy reference). We don't support `resolveRef` at all.

## Implementation Plan

### Option 1: Add resolveRef Support (Full Implementation)

Add a new `resolveRef` parser that:
1. Extracts the reference ID
2. Looks it up in the SchemaRegistry
3. Returns the resolved schema (not a reference)

**Pros**:
- Supports the full quarto-cli pattern
- Enables eager resolution for simple lookups
- Clearer error messages (fail fast at parse time)

**Cons**:
- Requires SchemaRegistry to be available during parsing
- More complex - need to thread registry through parsers
- May need architectural changes

### Option 2: Parse resolveRef as a Special Ref (Minimal)

Parse `resolveRef` into a `Ref` schema but mark it for eager resolution:

```rust
pub struct RefSchema {
    pub annotations: SchemaAnnotations,
    pub reference: String,
    pub eager: bool,  // NEW: true for resolveRef, false for ref
}
```

**Pros**:
- Simple to implement
- No architectural changes
- Resolution logic can be added later

**Cons**:
- Doesn't actually resolve eagerly during parsing
- Just marks intent for future use

### Option 3: Document Current Behavior (Punt)

Document that we treat `resolveRef` the same as `ref` and defer true distinction to when validation is implemented.

**Pros**:
- No code changes
- Simpler for now

**Cons**:
- Missing quarto-cli compatibility
- Potential issues if schemas rely on eager resolution

## Recommendation: Option 2 (Minimal with Flag)

**Rationale**:
1. We don't have validation implemented yet, so we can't actually use the distinction
2. Adding the flag preserves the semantic difference for future use
3. We can parse quarto-cli schemas correctly without architectural changes
4. When we implement validation, we'll know which refs need eager vs lazy resolution

## Implementation Steps

### 1. Update RefSchema Type

```rust
// src/schema/types.rs
pub struct RefSchema {
    pub annotations: SchemaAnnotations,
    pub reference: String,
    pub eager: bool,  // true for resolveRef, false for ref/$ref
}
```

### 2. Update parse_ref_schema

```rust
// src/schema/parsers/ref.rs
pub(in crate::schema) fn parse_ref_schema(yaml: &YamlWithSourceInfo) -> SchemaResult<Schema> {
    let annotations = parse_annotations(yaml)?;

    // Check which key is present
    let (reference, eager) = if let Some(ref_val) = yaml.get_hash_value("resolveRef") {
        (
            ref_val.yaml.as_str().ok_or_else(|| SchemaError::InvalidStructure {
                message: "resolveRef requires string value".to_string(),
                location: ref_val.source_info.clone(),
            })?,
            true,  // resolveRef is eager
        )
    } else {
        let ref_val = yaml
            .get_hash_value("ref")
            .or_else(|| yaml.get_hash_value("$ref"))
            .ok_or_else(|| SchemaError::InvalidStructure {
                message: "ref schema requires 'ref', '$ref', or 'resolveRef' key".to_string(),
                location: yaml.source_info.clone(),
            })?;

        (
            ref_val.yaml.as_str().ok_or_else(|| SchemaError::InvalidStructure {
                message: "ref requires string value".to_string(),
                location: ref_val.source_info.clone(),
            })?,
            false,  // ref/$ref is lazy
        )
    };

    Ok(Schema::Ref(RefSchema {
        annotations,
        reference: reference.to_string(),
        eager,
    }))
}
```

### 3. Update parser.rs Dispatcher

```rust
// src/schema/parser.rs
// Add "resolveRef" to the match in parse_object_form
match key {
    // ... existing cases ...
    "ref" | "$ref" | "resolveRef" => parse_ref_schema(&first_entry.value),
    // ...
}
```

Wait, actually the current code calls `parse_ref_schema(&first_entry.value)` but we need to pass the whole item to check which key was used. Let me reconsider...

Actually, better approach: check the key in the dispatcher and pass a flag:

### Revised Implementation

Keep it simple - update the parser to detect the key used:

```rust
match key {
    "ref" | "$ref" => parse_ref_schema(&first_entry.value, false),
    "resolveRef" => parse_ref_schema(&first_entry.value, true),
    // ...
}
```

Then parse_ref_schema signature becomes:
```rust
pub(in crate::schema) fn parse_ref_schema(
    yaml: &YamlWithSourceInfo,
    eager: bool,
) -> SchemaResult<Schema>
```

### 4. Add Tests

```rust
#[test]
fn test_ref_lazy() {
    let yaml = quarto_yaml::parse("ref: schema/base").unwrap();
    let schema = Schema::from_yaml(&yaml).unwrap();
    match schema {
        Schema::Ref(r) => {
            assert_eq!(r.reference, "schema/base");
            assert_eq!(r.eager, false);
        }
        _ => panic!("Expected Ref schema"),
    }
}

#[test]
fn test_resolve_ref_eager() {
    let yaml = quarto_yaml::parse("resolveRef: schema/base").unwrap();
    let schema = Schema::from_yaml(&yaml).unwrap();
    match schema {
        Schema::Ref(r) => {
            assert_eq!(r.reference, "schema/base");
            assert_eq!(r.eager, true);
        }
        _ => panic!("Expected Ref schema"),
    }
}
```

## Estimated Effort

- Update RefSchema: 5 minutes
- Update parser dispatcher: 10 minutes
- Update parse_ref_schema: 15 minutes
- Add tests: 20 minutes
- Test with existing schemas: 10 minutes

**Total**: ~1 hour

## Future Work

When implementing validation (future):
- Eager refs (`eager: true`) should be resolved immediately when constructing the validator
- Lazy refs (`eager: false`) can remain as references for the validator to resolve lazily
- This enables circular dependencies to work correctly (lazy) while simple lookups are fast (eager)
