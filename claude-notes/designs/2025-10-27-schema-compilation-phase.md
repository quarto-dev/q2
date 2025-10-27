# Schema Compilation Phase Design

**Created**: 2025-10-27
**Context**: k-246 (schema inheritance implementation)
**Status**: Design approved

## The Problem

In quarto-cli (TypeScript), schema parsing has immediate access to a SchemaRegistry, allowing `resolveRef` to return actual schemas during parsing:

```typescript
function lookup(yaml: any): ConcreteSchema {
  if (!hasSchemaDefinition(yaml.resolveRef)) {
    throw new Error(`lookup of key ${yaml.resolveRef} failed`);
  }
  return getSchemaDefinition(yaml.resolveRef)!;  // Returns ACTUAL schema
}
```

In Rust, we want parsing to be stateless (no registry required), but schemas with inheritance are **incomplete** without resolving their base schemas.

## The Solution: Two-Phase Processing

### Phase 1: Parsing (No Registry Required)

Parse YAML → Schema AST

- Short forms: `"boolean"` → `Schema::Boolean(...)`
- Object forms: `{boolean: {...}}` → `Schema::Boolean(...)`
- References: `{ref: "foo"}` → `Schema::Ref(RefSchema { reference: "foo", eager: false })`
- Eager references: `{resolveRef: "foo"}` → `Schema::Ref(RefSchema { reference: "foo", eager: true })`
- Inheritance: `{super: {...}}` → Stored in `ObjectSchema.base_schema`

**Result**: Uncompiled Schema AST (may contain references, may be structurally incomplete)

### Phase 2: Compilation (Registry Required)

Schema AST + Registry → Compiled Schema

- Resolve eager references (`eager: true`)
- Merge object inheritance (`base_schema`)
- Recursively compile nested schemas
- Validate structural completeness

**Result**: Compiled Schema (structurally complete, may still contain lazy refs)

### Phase 3: Validation (Registry Required)

Compiled Schema + Data + Registry → Validation Result

- Resolve lazy references (`eager: false`) on demand
- Support circular references
- Check data against schemas

**Result**: Validation errors or success

## Why Two Different Reference Types?

The **eager flag** determines **when** a reference must be resolved:

| Feature | `ref` (eager=false) | `resolveRef` (eager=true) |
|---------|---------------------|---------------------------|
| **Parsed as** | `Schema::Ref { eager: false }` | `Schema::Ref { eager: true }` |
| **Resolved at** | Validation time | Compilation time |
| **Purpose** | Schema constraint reference | Schema structure dependency |
| **Schema complete without it?** | ✅ Yes | ❌ No |
| **Supports circular refs?** | ✅ Yes | ❌ No (would cause infinite recursion) |
| **Used in** | Property types | `super` field, structural composition |

### Example: eager=false (Lazy Reference)

```yaml
- id: person
  object:
    properties:
      name: string
      parent:
        ref: person  # Lazy - can be circular!
```

**Compilation**:
```rust
// ref stays as Schema::Ref - schema is structurally complete
Schema::Object(ObjectSchema {
    properties: {
        "name" => Schema::String(...),
        "parent" => Schema::Ref(RefSchema {
            reference: "person",
            eager: false  // ← Stays as ref
        })
    },
    ...
})
```

**Validation**:
```rust
// When validating data.parent, look up "person" schema and validate recursively
match &schema {
    Schema::Ref(r) if !r.eager => {
        let target = registry.resolve(&r.reference)?;
        validate(data, target, registry)?;  // Resolve now
    }
    ...
}
```

### Example: eager=true (Eager Reference)

```yaml
- id: social-metadata
  object:
    properties:
      title: string
      description: string

- id: twitter-card
  object:
    super:
      resolveRef: social-metadata  # Eager - MUST resolve to know properties!
    properties:
      card-style:
        enum: [summary, summary_large_image]
```

**Before Compilation**:
```rust
// Schema is incomplete - we don't know what properties it has!
Schema::Object(ObjectSchema {
    base_schema: Some(vec![
        Schema::Ref(RefSchema {
            reference: "social-metadata",
            eager: true  // ← MUST resolve before schema is usable
        })
    ]),
    properties: {
        "card-style" => Schema::Enum(...)
    },
    ...
})
```

**After Compilation**:
```rust
// Schema is complete - base properties merged in
Schema::Object(ObjectSchema {
    base_schema: None,  // Merged and removed
    properties: {
        "title" => Schema::String(...),        // ← From base
        "description" => Schema::String(...),  // ← From base
        "card-style" => Schema::Enum(...),     // ← From derived
    },
    required: ["title"],  // ← From base
    ...
})
```

## API Design

### Stateless Parsing

```rust
use quarto_yaml_validation::Schema;
use quarto_yaml;

// Parse without registry
let yaml = quarto_yaml::parse(schema_yaml_string)?;
let schema = Schema::from_yaml(&yaml)?;

// Schema may be incomplete (has eager refs, has base_schema)
```

### Schema Compilation

```rust
use quarto_yaml_validation::{Schema, SchemaRegistry};

let mut registry = SchemaRegistry::new();

// Register all schemas (uncompiled)
registry.register("base-schema".to_string(), base_schema);
registry.register("derived-schema".to_string(), derived_schema);

// Compile schemas (resolve eager refs, merge inheritance)
let compiled = derived_schema.compile(&registry)?;

// Now schema is structurally complete and usable
```

### Implementation

```rust
impl Schema {
    /// Compile a schema by resolving eager references and merging inheritance.
    ///
    /// This creates a structurally complete schema suitable for validation.
    /// Lazy references (eager=false) are kept as references and resolved
    /// during validation.
    ///
    /// # Arguments
    /// * `registry` - Schema registry for resolving references
    ///
    /// # Returns
    /// A compiled schema with all eager references resolved
    ///
    /// # Errors
    /// Returns error if:
    /// - An eager reference cannot be resolved
    /// - Base schema is not an ObjectSchema
    /// - Circular eager references detected
    pub fn compile(&self, registry: &SchemaRegistry) -> SchemaResult<Schema> {
        match self {
            // Object with inheritance - must merge base schemas
            Schema::Object(obj) if obj.base_schema.is_some() => {
                // Compile base schemas first (recursive)
                let base_schemas = obj.base_schema.as_ref().unwrap();
                let compiled_bases: SchemaResult<Vec<_>> = base_schemas
                    .iter()
                    .map(|s| s.compile(registry))
                    .collect();
                let compiled_bases = compiled_bases?;

                // Merge with derived schema
                let merged = merge_object_schemas(&compiled_bases, obj, registry)?;

                // Result has no base_schema (it's been merged)
                Ok(Schema::Object(merged))
            }

            // Eager reference - must resolve now
            Schema::Ref(r) if r.eager => {
                let resolved = registry.resolve(&r.reference)
                    .ok_or_else(|| SchemaError::InvalidStructure {
                        message: format!(
                            "Cannot resolve eager reference '{}' - not found in registry",
                            r.reference
                        ),
                        location: SourceInfo::default(),
                    })?;

                // Recursively compile the resolved schema
                resolved.compile(registry)
            }

            // Lazy reference - keep as is for validation time
            Schema::Ref(_) => Ok(self.clone()),

            // Recursively compile nested schemas in containers
            Schema::AnyOf(anyof) => {
                let compiled_schemas: SchemaResult<Vec<_>> = anyof.schemas
                    .iter()
                    .map(|s| s.compile(registry))
                    .collect();
                Ok(Schema::AnyOf(AnyOfSchema {
                    annotations: anyof.annotations.clone(),
                    schemas: compiled_schemas?,
                }))
            }

            Schema::AllOf(allof) => {
                let compiled_schemas: SchemaResult<Vec<_>> = allof.schemas
                    .iter()
                    .map(|s| s.compile(registry))
                    .collect();
                Ok(Schema::AllOf(AllOfSchema {
                    annotations: allof.annotations.clone(),
                    schemas: compiled_schemas?,
                }))
            }

            Schema::Array(arr) => {
                let compiled_items = if let Some(items) = &arr.items {
                    Some(Box::new(items.compile(registry)?))
                } else {
                    None
                };
                Ok(Schema::Array(ArraySchema {
                    annotations: arr.annotations.clone(),
                    items: compiled_items,
                    min_items: arr.min_items,
                    max_items: arr.max_items,
                    unique_items: arr.unique_items,
                }))
            }

            Schema::Object(obj) => {
                // Object without inheritance - compile nested property schemas
                let mut compiled_properties = HashMap::new();
                for (key, prop_schema) in &obj.properties {
                    compiled_properties.insert(key.clone(), prop_schema.compile(registry)?);
                }

                let mut compiled_pattern_properties = HashMap::new();
                for (pattern, prop_schema) in &obj.pattern_properties {
                    compiled_pattern_properties.insert(
                        pattern.clone(),
                        prop_schema.compile(registry)?
                    );
                }

                let compiled_additional = if let Some(ap) = &obj.additional_properties {
                    Some(Box::new(ap.compile(registry)?))
                } else {
                    None
                };

                let compiled_property_names = if let Some(pn) = &obj.property_names {
                    Some(Box::new(pn.compile(registry)?))
                } else {
                    None
                };

                Ok(Schema::Object(ObjectSchema {
                    annotations: obj.annotations.clone(),
                    properties: compiled_properties,
                    pattern_properties: compiled_pattern_properties,
                    additional_properties: compiled_additional,
                    required: obj.required.clone(),
                    min_properties: obj.min_properties,
                    max_properties: obj.max_properties,
                    closed: obj.closed,
                    property_names: compiled_property_names,
                    naming_convention: obj.naming_convention.clone(),
                    base_schema: None,  // No inheritance at this level
                }))
            }

            // Primitives don't need compilation
            Schema::False
            | Schema::True
            | Schema::Boolean(_)
            | Schema::Number(_)
            | Schema::String(_)
            | Schema::Null(_)
            | Schema::Enum(_)
            | Schema::Any(_) => Ok(self.clone()),
        }
    }
}
```

### Validation with Compilation

```rust
use quarto_yaml_validation::{Schema, SchemaRegistry, validate};

// Parse all schemas
let base = Schema::from_yaml(&base_yaml)?;
let derived = Schema::from_yaml(&derived_yaml)?;

// Build registry
let mut registry = SchemaRegistry::new();
registry.register("base".to_string(), base);
registry.register("derived".to_string(), derived.clone());

// Compile the schema we want to use
let compiled = derived.compile(&registry)?;

// Validate data against compiled schema
let data = quarto_yaml::parse(&data_yaml)?;
let errors = validate(&data, &compiled, &registry)?;
```

## Benefits of This Design

### 1. Stateless Parsing
- No registry needed during parsing
- Easier to test parsers
- Can parse schemas before knowing what they reference

### 2. Explicit Compilation
- Clear separation of concerns
- Can inspect uncompiled schemas
- Can defer compilation until needed

### 3. Performance
- Compilation happens once per schema
- Compiled schemas are ready to use
- Lazy refs resolved only when validating actual data

### 4. Circular Reference Support
- Lazy refs can be circular (person → parent: ref person)
- Eager refs cannot be circular (caught during compilation)
- Clear error messages for each case

### 5. Matches quarto-cli Semantics
- `resolveRef` → eager resolution (compilation time)
- `ref` → lazy resolution (validation time)
- Same behavior, different timing

## Workflow Comparison

### quarto-cli (TypeScript)

```
Parse → (registry available) → Resolve eagerly → Ready to validate
         ↓
    resolveRef returns actual schema immediately
```

### Rust (This Design)

```
Parse → Register → Compile → Validate
        ↓          ↓          ↓
   no registry  resolve eager  resolve lazy
                merge inherit  on demand
```

## Migration Notes

### Phase 1: k-246 (Current)
- ✅ Add `base_schema` field
- ✅ Parse `super` field
- ✅ Add `eager` flag to RefSchema
- ✅ Implement merging logic
- ⚠️ Manual compilation: Users call `merge_object_schemas()` explicitly

### Phase 2: Future Enhancement
- Add `Schema::compile()` method
- Automatic recursive compilation
- Better error messages with source locations
- Cycle detection for eager refs

### Phase 3: Future Enhancement
- `SchemaRegistry::compile_all()` - compile entire registry
- Lazy compilation (compile on first use)
- Compilation caching

## Open Questions

### Q1: Should compilation be automatic in the registry?

**Option A**: Explicit
```rust
let schema = registry.resolve("foo")?;
let compiled = schema.compile(&registry)?;
```

**Option B**: Automatic
```rust
let compiled = registry.resolve_compiled("foo")?;
```

**Decision**: Start with Option A (explicit) for k-246. Can add Option B later as convenience.

### Q2: Should we validate during compilation?

For example, check that all property names match naming conventions, all required fields exist, etc.?

**Decision**: No. Compilation only resolves structure. Validation happens separately with data.

### Q3: How to handle compilation errors?

Should they be SchemaError or a new CompilationError type?

**Decision**: Use SchemaError for now. Can refine later if needed.

### Q4: Should compiled schemas be a different type?

```rust
pub struct CompiledSchema(Schema);  // Newtype wrapper
```

**Pros**: Type safety - can't use uncompiled schema for validation
**Cons**: More complex API, conversion overhead

**Decision**: Use plain Schema for now. Can add newtype later if needed.

## Examples

### Example 1: Simple Inheritance

```yaml
# base.yml
id: base-person
object:
  properties:
    name: string
    email: string
  required: [name]

# derived.yml
id: employee
object:
  super:
    resolveRef: base-person
  properties:
    employee-id: number
  required: [employee-id]
```

```rust
// Parse
let base = Schema::from_yaml(&base_yaml)?;
let derived = Schema::from_yaml(&derived_yaml)?;

// Register
let mut registry = SchemaRegistry::new();
registry.register("base-person".to_string(), base);
registry.register("employee".to_string(), derived.clone());

// Compile
let compiled = derived.compile(&registry)?;

// compiled now has:
// properties: { name, email, employee-id }
// required: [name, employee-id]
```

### Example 2: Circular References (Allowed)

```yaml
id: person
object:
  properties:
    name: string
    spouse:
      ref: person  # Lazy - stays as ref
    children:
      arrayOf:
        ref: person  # Lazy - stays as ref
```

```rust
let schema = Schema::from_yaml(&yaml)?;
let mut registry = SchemaRegistry::new();
registry.register("person".to_string(), schema.clone());

// Compilation succeeds - lazy refs stay as refs
let compiled = schema.compile(&registry)?;

// Validation handles circular refs by looking up on demand
validate(&data, &compiled, &registry)?;
```

### Example 3: Invalid Eager Circular Reference (Error)

```yaml
id: broken
object:
  super:
    resolveRef: broken  # ERROR: Circular eager reference!
```

```rust
let schema = Schema::from_yaml(&yaml)?;
let mut registry = SchemaRegistry::new();
registry.register("broken".to_string(), schema.clone());

// Compilation fails with clear error
let result = schema.compile(&registry);
assert!(result.is_err());
// Error: "Circular eager reference detected: broken -> broken"
```

## Testing Strategy

### Unit Tests
- ✅ Parse schemas with `ref` and `resolveRef`
- ✅ Compile simple inheritance
- ✅ Compile multiple inheritance
- ✅ Compile nested schemas (anyOf, allOf, array, object)
- ✅ Error: Missing eager reference
- ✅ Error: Non-object base schema
- ✅ Error: Circular eager reference (future)

### Integration Tests
- ✅ Real quarto-cli schemas with inheritance
- ✅ Circular lazy references work
- ✅ Complex nested inheritance chains

## References

- **k-246**: Schema inheritance implementation issue
- **k-247**: resolveRef vs ref analysis
- **quarto-cli source**: `src/core/lib/yaml-schema/common.ts` (objectSchema function)
- **quarto-cli source**: `src/core/lib/yaml-schema/from-yaml.ts` (lookup function)
