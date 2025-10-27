# Phase 1 Audit: Schema::from_yaml() Implementation

**Date**: 2025-10-27
**Issue**: k-239 - bd-8 Phase 1: Audit existing Schema::from_yaml() implementation
**Auditor**: Claude Code (Sonnet 4.5)
**Files Audited**: `private-crates/quarto-yaml-validation/src/schema.rs` (~1300 lines)

## Executive Summary

The existing Rust `Schema::from_yaml()` implementation is **partially complete** but **missing critical quarto-cli patterns**. Out of 12 identified YAML syntax patterns from quarto-cli, **7 are fully implemented**, **0 are partially implemented**, and **5 are completely missing**.

### High-Level Status

✅ **IMPLEMENTED (7/12)**:
1. Literal short forms (`string`, `boolean`, `number`, etc.)
2. Object forms with properties (e.g., `boolean: { description: "..." }`)
3. Inline enum arrays (`[val1, val2, val3]`)
4. anyOf/allOf dual syntax (array and object forms)
5. Object schemas (with properties, required, closed, patternProperties)
6. ref schemas (`ref: schema/base`)
7. Array schemas (`array: { items: string }`)

❌ **MISSING (5/12)**:
8. **arrayOf** quarto syntax (`arrayOf: string`, `arrayOf: { schema: string, length: 5 }`)
9. **maybeArrayOf** quarto extension (`maybeArrayOf: string` → expands to anyOf)
10. **record** quarto extension (`record: { properties: ... }` → closed object)
11. **pattern** as schema type (`pattern: "^[a-z]+$"`)
12. **schema** explicit wrapper (`schema: { anyOf: [...] }`)

### Critical Gaps

**Highest Priority**:
- **arrayOf**: Used extensively in quarto-cli schemas (>50 occurrences in definitions.yml alone)
- **maybeArrayOf**: Common pattern for "value or array of values" (e.g., `auto: "docs/"` or `auto: ["docs/", "posts/"]`)

**High Priority**:
- **record**: Used for dictionary/map types where all values have the same type
- **pattern**: Alternative to `string: { pattern: "..." }` used in schema meta-schemas

**Medium Priority**:
- **schema**: Wrapper for adding properties without type nesting

### Additional Findings

1. **Missing nested property application**: quarto-cli applies `setBaseSchemaProperties()` twice for nested object forms. Current Rust implementation only applies annotations once.

2. **Missing `required: "all"` support**: quarto-cli supports `required: "all"` as shorthand. Rust only supports string arrays.

3. **Missing `super`/baseSchema inheritance**: quarto-cli supports object schema inheritance via `super: { resolveRef: schema/base }`. Not implemented in Rust.

4. **Missing `resolveRef` vs `ref` distinction**: quarto-cli has two reference types - `ref` (lazy) and `resolveRef` (immediate). Rust only has `ref`.

5. **Missing `propertyNames` support**: Used in closed schemas to enumerate valid keys. Present in quarto-cli's ObjectSchema but not implemented in parser.

6. **Missing `namingConvention` validation**: quarto-cli validates property names against conventions (dash-case, snake_case, etc.). No support in Rust.

---

## Detailed Pattern-by-Pattern Analysis

### Pattern 1: Literal Short Forms ✅ IMPLEMENTED

**quarto-cli**: `from-yaml.ts` lines 458-471

**YAML Examples**:
```yaml
boolean
string
number
path
any
null
```

**Rust Implementation**: `parse_short_form()` lines 312-339

**Status**: ✅ **FULLY IMPLEMENTED**

**Code**:
```rust
fn parse_short_form(s: &str, _location: &SourceInfo) -> SchemaResult<Schema> {
    match s {
        "boolean" => Ok(Schema::Boolean(BooleanSchema { ... })),
        "number" => Ok(Schema::Number(NumberSchema { ... })),
        "string" | "path" => Ok(Schema::String(StringSchema { ... })),
        "null" => Ok(Schema::Null(NullSchema { ... })),
        "any" => Ok(Schema::Any(AnySchema { ... })),
        _ => Err(SchemaError::InvalidType(s.to_string())),
    }
}
```

**Notes**:
- Correctly handles `path` as alias for `string`
- All basic types supported
- Returns default (empty) annotations - **this is a gap** (see Pattern 2)

---

### Pattern 2: Object Forms with Properties ✅ IMPLEMENTED (with gaps)

**quarto-cli**: `from-yaml.ts` `convertFromBoolean()`, `convertFromNumber()`, etc.

**YAML Examples**:
```yaml
boolean:
  description: "Enable or disable"
  completions: [true, false]

string:
  pattern: "^[a-z]+$"
  description: "Lowercase only"

number:
  minimum: 0
  maximum: 100
```

**Rust Implementation**: `parse_boolean_schema()`, `parse_number_schema()`, `parse_string_schema()` lines 407-442

**Status**: ✅ **IMPLEMENTED** but with annotation extraction gaps

**Code Example** (boolean):
```rust
fn parse_boolean_schema(yaml: &YamlWithSourceInfo) -> SchemaResult<Schema> {
    let annotations = parse_annotations(yaml)?;
    Ok(Schema::Boolean(BooleanSchema { annotations }))
}
```

**Annotations Extracted** (`parse_annotations()` line 711-721):
```rust
SchemaAnnotations {
    id: get_hash_string(yaml, "$id")?,
    description: get_hash_string(yaml, "description")?,
    documentation: get_hash_string(yaml, "documentation")?,
    error_message: get_hash_string(yaml, "errorMessage")?,
    hidden: get_hash_bool(yaml, "hidden")?,
    completions: get_hash_string_array(yaml, "completions")?,
    tags: get_hash_tags(yaml)?,
}
```

**Gap Identified**:
- ❌ **Missing `additionalCompletions`** (quarto-cli line 64-66)
- ❌ **Missing nested property extraction** - quarto-cli calls `setBaseSchemaProperties()` twice:
  ```typescript
  return setBaseSchemaProperties(
    yaml,                    // Outer properties
    setBaseSchemaProperties(
      yaml.boolean,          // Inner properties
      booleanS
    )
  );
  ```
  The Rust version only extracts from the value (inner), not the outer YAML object.

**Impact**: Medium - some schemas may lose metadata from outer wrapper

---

### Pattern 3: Inline Enum Arrays ✅ IMPLEMENTED

**quarto-cli**: `from-yaml.ts` `convertFromEnum()` lines 242-255

**YAML Examples**:
```yaml
# Inline at top level
[value1, value2, value3]

# Inside enum key
enum: [value1, value2, value3]

# Explicit form
enum:
  values: [plain, webtex, gladtex]
```

**Rust Implementation**:
- `parse_inline_enum()` lines 385-403 (top-level arrays)
- `parse_enum_schema()` lines 454-492 (both forms)

**Status**: ✅ **FULLY IMPLEMENTED**

**Code**:
```rust
fn parse_enum_schema(yaml: &YamlWithSourceInfo) -> SchemaResult<Schema> {
    let annotations = parse_annotations(yaml)?;

    let values = if let Some(values_yaml) = yaml.get_hash_value("values") {
        // Explicit form: enum: { values: [...] }
        ...
    } else {
        // Inline form: enum: [val1, val2, val3]
        ...
    };

    Ok(Schema::Enum(EnumSchema { annotations, values }))
}
```

**Notes**: Correctly uses `serde_json::Value` for enum values, matching quarto-cli

---

### Pattern 4: anyOf/allOf Dual Syntax ✅ IMPLEMENTED

**quarto-cli**: `from-yaml.ts` `convertFromAnyOf()`, `convertFromAllOf()` lines 224-221

**YAML Syntax A** (Array form):
```yaml
anyOf:
  - boolean
  - string
```

**YAML Syntax B** (Object form):
```yaml
anyOf:
  schemas:
    - boolean
    - string
  description: "Boolean or string"
```

**Rust Implementation**: `parse_anyof_schema()` lines 494-528, `parse_allof_schema()` lines 530-562

**Status**: ✅ **FULLY IMPLEMENTED**

**Code**:
```rust
fn parse_anyof_schema(yaml: &YamlWithSourceInfo) -> SchemaResult<Schema> {
    let annotations = parse_annotations(yaml)?;

    let schemas = if let Some(schemas_yaml) = yaml.get_hash_value("schemas") {
        // Explicit form: anyOf: { schemas: [...] }
        ...
    } else {
        // Inline form: anyOf: [...]
        ...
    };

    Ok(Schema::AnyOf(AnyOfSchema { annotations, schemas }))
}
```

**Gap**: Same nested property extraction issue as Pattern 2 (missing outer-level properties)

---

### Pattern 5: Object Schemas ✅ IMPLEMENTED (with gaps)

**quarto-cli**: `from-yaml.ts` `convertFromObject()` lines 284-427

**YAML Example**:
```yaml
object:
  properties:
    title: string
    author: string
  required: [title]
  closed: true
  additionalProperties: string
  patternProperties:
    "^x-": string
  super:
    resolveRef: schema/base
```

**Rust Implementation**: `parse_object_schema()` lines 584-691

**Status**: ✅ **MOSTLY IMPLEMENTED** with notable gaps

**Implemented Features**:
- ✅ properties (recursive schema parsing)
- ✅ patternProperties (recursive schema parsing)
- ✅ additionalProperties
- ✅ required (array of strings)
- ✅ minProperties / maxProperties
- ✅ closed flag

**Missing Features**:
- ❌ `required: "all"` shorthand (quarto-cli line 414-415)
- ❌ `super` / `baseSchema` inheritance (quarto-cli lines 407-413)
- ❌ `propertyNames` enum generation for closed schemas (quarto-cli lines 376-394)
- ❌ `namingConvention` validation (quarto-cli lines 288-360)

**Impact**:
- High for `required: "all"` (common pattern)
- Medium for `super` (used in meta-schemas)
- Low for `propertyNames` and `namingConvention` (advanced features)

---

### Pattern 6: ref Schemas ✅ IMPLEMENTED

**quarto-cli**: `from-yaml.ts` `convertFromRef()` lines 172-174

**YAML Example**:
```yaml
ref: schema/base
# OR
ref: date
```

**Rust Implementation**: `parse_ref_schema()` lines 693-705

**Status**: ✅ **FULLY IMPLEMENTED**

**Code**:
```rust
fn parse_ref_schema(yaml: &YamlWithSourceInfo) -> SchemaResult<Schema> {
    let reference = yaml.yaml.as_str().map(|s| s.to_string())?;
    Ok(Schema::Ref(RefSchema {
        annotations: Default::default(),
        reference,
    }))
}
```

**Gap**: Doesn't extract annotations from parent object (same nested property issue)

---

### Pattern 7: Array Schemas ✅ IMPLEMENTED

**quarto-cli**: Uses `arrayOfS()` helper (not `convertFromArrayOf`)

**YAML Example**:
```yaml
array:
  items: string
  minItems: 1
  maxItems: 10
  uniqueItems: true
```

**Rust Implementation**: `parse_array_schema()` lines 564-582

**Status**: ✅ **FULLY IMPLEMENTED**

**Code**:
```rust
fn parse_array_schema(yaml: &YamlWithSourceInfo) -> SchemaResult<Schema> {
    let annotations = parse_annotations(yaml)?;
    let items = if let Some(items_yaml) = yaml.get_hash_value("items") {
        Some(Box::new(Schema::from_yaml(items_yaml)?))
    } else {
        None
    };
    let min_items = get_hash_usize(yaml, "minItems")?;
    let max_items = get_hash_usize(yaml, "maxItems")?;
    let unique_items = get_hash_bool(yaml, "uniqueItems")?;

    Ok(Schema::Array(ArraySchema { ... }))
}
```

**Note**: This handles the `array: { items: ... }` form but **NOT** the quarto `arrayOf: T` shorthand (Pattern 8)

---

### Pattern 8: arrayOf (Quarto Syntax) ❌ MISSING

**quarto-cli**: `from-yaml.ts` `convertFromArrayOf()` lines 190-203

**YAML Syntax A** (Simple):
```yaml
arrayOf: string
```

**YAML Syntax B** (With schema and length):
```yaml
arrayOf:
  schema: string
  length: 5  # Fixed-length arrays
```

**Rust Implementation**: ❌ **NOT IMPLEMENTED**

**quarto-cli Code**:
```typescript
function convertFromArrayOf(yaml: any): ConcreteSchema {
  if (yaml.arrayOf.schema) {
    const result = arrayOfS(convertFromYaml(yaml.arrayOf.schema));
    return setBaseSchemaProperties(
      yaml,
      setBaseSchemaProperties(yaml.arrayOf, result)
    );
  } else {
    return setBaseSchemaProperties(
      yaml,
      arrayOfS(convertFromYaml(yaml.arrayOf))
    );
  }
}
```

**Expected Behavior**:
- `arrayOf: string` → `ArraySchema { items: StringSchema, ... }`
- `arrayOf: { schema: string, length: 5 }` → `ArraySchema { items: StringSchema, min_items: 5, max_items: 5 }`

**Impact**: 🔴 **CRITICAL** - Used in >50 places in quarto-cli schemas

**Examples from quarto-cli**:
```yaml
# definitions.yml line 18
- id: pandoc-format-request-headers
  arrayOf:
    arrayOf:
      schema: string
      length: 2

# definitions.yml line 59
- id: pandoc-shortcodes
  arrayOf: path
```

---

### Pattern 9: maybeArrayOf ❌ MISSING

**quarto-cli**: `from-yaml.ts` `convertFromMaybeArrayOf()` lines 177-187

**YAML Example**:
```yaml
maybeArrayOf: string
```

**Expands to**:
```typescript
anyOf(inner, arrayOf(inner))
```

**With special tag**:
```typescript
tagSchema(schema, { "complete-from": ["anyOf", 0] })
```

**Rust Implementation**: ❌ **NOT IMPLEMENTED**

**quarto-cli Code**:
```typescript
function convertFromMaybeArrayOf(yaml: any): ConcreteSchema {
  const inner = convertFromYaml(yaml.maybeArrayOf);
  const schema = tagSchema(
    anyOfS(inner, arrayOfS(inner)),
    { "complete-from": ["anyOf", 0] }  // Complete from first option
  );
  return setBaseSchemaProperties(yaml, schema);
}
```

**Impact**: 🟡 **HIGH** - Common pattern in quarto-cli for flexible schemas

**Examples from quarto-cli**:
```yaml
# definitions.yml line 90
auto:
  anyOf:
    - boolean
    - maybeArrayOf: string
```

---

### Pattern 10: record ❌ MISSING

**quarto-cli**: `from-yaml.ts` `convertFromRecord()` lines 258-281

**YAML Syntax A**:
```yaml
record:
  properties:
    key1: string
    key2: string
```

**YAML Syntax B**:
```yaml
record:
  key1: string
  key2: string
```

**Converts to**:
```typescript
convertFromObject({
  object: {
    properties: yaml.record.properties || yaml.record,
    closed: true,
    required: "all"
  }
})
```

**Rust Implementation**: ❌ **NOT IMPLEMENTED**

**Impact**: 🟡 **MEDIUM** - Used for dictionary/map types

**Examples from quarto-cli**:
```yaml
# definitions.yml (filters)
- record:
    type:
      enum: [citeproc]
```

---

### Pattern 11: pattern (as Schema Type) ❌ MISSING

**quarto-cli**: `from-yaml.ts` `convertFromPattern()` lines 145-154

**YAML Syntax A**:
```yaml
pattern: "^[a-z]+$"
```

**YAML Syntax B**:
```yaml
pattern:
  regex: "^[a-z]+$"
  description: "Lowercase letters only"
```

**Rust Implementation**: ❌ **NOT IMPLEMENTED**

**Note**: This is different from `string: { pattern: "..." }`. In quarto-cli schemas, `pattern` can be a top-level schema type.

**quarto-cli Code**:
```typescript
function convertFromPattern(yaml: any): ConcreteSchema {
  if (typeof yaml.pattern === "string") {
    return setBaseSchemaProperties(yaml, regexSchema(yaml.pattern));
  } else {
    return setBaseSchemaProperties(
      yaml,
      setBaseSchemaProperties(yaml.pattern, regexSchema(yaml.pattern.regex))
    );
  }
}
```

**Impact**: 🟢 **LOW** - Primarily used in meta-schemas (schema.yml)

**Examples**:
```yaml
# schema.yml line 78
- id: schema/explicit-pattern-string
  object:
    required: [pattern]
    properties:
      pattern: string
```

---

### Pattern 12: schema (Explicit Wrapper) ❌ MISSING

**quarto-cli**: `from-yaml.ts` `convertFromSchema()` lines 115-118

**YAML Example**:
```yaml
schema:
  anyOf:
    - boolean
    - string
description: "A boolean or string"
```

**Purpose**: Add properties (description, completions, etc.) to a schema without nesting under a type key.

**Rust Implementation**: ❌ **NOT IMPLEMENTED**

**quarto-cli Code**:
```typescript
function convertFromSchema(yaml: any): ConcreteSchema {
  const schema = convertFromYaml(yaml.schema);
  return setBaseSchemaProperties(yaml, schema);
}
```

**Impact**: 🟡 **MEDIUM** - Used in field-based schemas (document-*.yml, cell-*.yml)

**Examples from quarto-cli**:
```yaml
# document-execute.yml line 2
- name: engine
  schema:
    string:
      completions: [jupyter, knitr, julia]
  description: "Engine used for executable code blocks."
```

**Note**: The `name` key indicates this is a SchemaField (Pattern 12 from comprehensive plan), which is bd-9's scope. However, the `schema:` wrapper pattern itself should be supported in bd-8.

---

## Additional Gaps

### Gap 1: Nested Property Extraction ⚠️ PARTIAL

**Issue**: quarto-cli applies `setBaseSchemaProperties()` twice for object forms:

```typescript
// Outer properties applied first
return setBaseSchemaProperties(
  yaml,
  // Inner properties applied second
  setBaseSchemaProperties(yaml.anyOf, anyOfS(...inner))
);
```

**Rust Behavior**: Only extracts from inner value

**Impact**: 🟡 **MEDIUM** - May lose metadata from outer wrapper

**Example**:
```yaml
anyOf:
  schemas:
    - boolean
    - string
  description: "Inner description"
description: "Outer description"
completions: ["value1", "value2"]
```

Current Rust would only extract "Inner description" and miss "Outer description" and completions.

---

### Gap 2: required: "all" ❌ MISSING

**Issue**: quarto-cli supports `required: "all"` as shorthand

**quarto-cli Code** (line 414-415):
```typescript
if (schema["required"] === "all") {
  params.required = Object.keys(schema.properties || {});
}
```

**Rust Code**: Lines 653-675 only handle array form

**Impact**: 🟡 **HIGH** - Common pattern in quarto-cli

**Fix Required**: Check for string "all" and expand to array of all property keys

---

### Gap 3: super/baseSchema Inheritance ❌ MISSING

**Issue**: quarto-cli supports object inheritance via `super` field

**quarto-cli Code** (lines 407-413):
```typescript
if (schema["super"]) {
  if (Array.isArray(schema["super"])) {
    params.baseSchema = schema["super"].map((s) => convertFromYaml(s));
  } else {
    params.baseSchema = convertFromYaml(schema["super"]);
  }
}
```

**Rust**: Not implemented, no `baseSchema` field in `ObjectSchema`

**Impact**: 🟡 **MEDIUM** - Used in meta-schemas for inheritance

**Fix Required**:
1. Add `base_schema: Option<Vec<Schema>>` to `ObjectSchema`
2. Parse `super` field
3. Implement schema merging during validation

---

### Gap 4: resolveRef vs ref ❌ MISSING

**Issue**: quarto-cli has two reference types

**quarto-cli Code** (lines 430-435):
```typescript
function lookup(yaml: any): ConcreteSchema {
  if (!hasSchemaDefinition(yaml.resolveRef)) {
    throw new Error(`lookup of key ${yaml.resolveRef} failed`);
  }
  return getSchemaDefinition(yaml.resolveRef);
}
```

**Difference**:
- `ref: schema/base` → Creates RefSchema (lazy resolution)
- `resolveRef: schema/base` → Immediately returns the referenced schema

**Rust**: Only has `ref`

**Impact**: 🟡 **MEDIUM** - Used in `super` for inheritance

**Fix Required**: Add `resolveRef` case in `parse_object_form()` that immediately resolves

---

### Gap 5: additionalCompletions ❌ MISSING

**Issue**: quarto-cli supports both `completions` and `additionalCompletions`

**quarto-cli Code** (lines 64-69):
```typescript
if (yaml.additionalCompletions) {
  schema = completeSchema(schema, ...yaml.additionalCompletions);
}
if (yaml.completions) {
  schema = completeSchemaOverwrite(schema, ...yaml.completions);
}
```

**Rust**: `SchemaAnnotations` only has `completions`

**Impact**: 🟢 **LOW** - Rarely used

---

## Test Matrix

| Pattern # | Pattern Name | Implemented | Priority | Test Status |
|-----------|--------------|-------------|----------|-------------|
| 1 | Literal short forms | ✅ Yes | - | ⏳ Pending |
| 2 | Object forms with properties | ⚠️ Partial | P2 | ⏳ Pending |
| 3 | Inline enum arrays | ✅ Yes | - | ⏳ Pending |
| 4 | anyOf/allOf dual syntax | ⚠️ Partial | P2 | ⏳ Pending |
| 5 | Object schemas | ⚠️ Partial | P1 | ⏳ Pending |
| 6 | ref schemas | ✅ Yes | - | ⏳ Pending |
| 7 | Array schemas | ✅ Yes | - | ⏳ Pending |
| 8 | **arrayOf** | ❌ No | **P0** | ⏳ Pending |
| 9 | **maybeArrayOf** | ❌ No | **P1** | ⏳ Pending |
| 10 | **record** | ❌ No | **P1** | ⏳ Pending |
| 11 | **pattern** | ❌ No | P3 | ⏳ Pending |
| 12 | **schema** wrapper | ❌ No | **P1** | ⏳ Pending |

**Additional Gaps**:
- Nested property extraction (P2)
- `required: "all"` (P1)
- `super` inheritance (P2)
- `resolveRef` vs `ref` (P2)
- `additionalCompletions` (P3)
- `propertyNames` (P3)
- `namingConvention` (P3)

---

## Priority Classification

### P0 - Critical (Blocks basic usage)
1. **arrayOf**: Used in >50 places in quarto-cli schemas
   - **Why**: Core pattern for array types
   - **Impact**: Cannot parse most quarto-cli schemas without this

### P1 - High (Many quarto-cli schemas affected)
1. **maybeArrayOf**: Common "value or array" pattern
   - **Why**: Used for flexible user input (single value or multiple)
   - **Impact**: ~20+ schemas in quarto-cli use this

2. **record**: Dictionary/map type pattern
   - **Why**: Used for uniform key-value structures
   - **Impact**: ~10+ schemas in quarto-cli

3. **schema** wrapper: Field-based schema pattern
   - **Why**: Used in all document-*.yml and cell-*.yml files
   - **Impact**: Cannot parse field-based schemas (bd-9 dependency)

4. **required: "all"**: Common shorthand
   - **Why**: Frequently used in closed objects
   - **Impact**: ~15+ schemas in quarto-cli

### P2 - Medium (Completeness and correctness)
1. **Nested property extraction**: Metadata loss
   - **Why**: Ensures all annotations are preserved
   - **Impact**: Some schemas may lose completions, descriptions

2. **super** inheritance: Meta-schema support
   - **Why**: Used in schema.yml for schema composition
   - **Impact**: Cannot parse meta-schemas correctly

3. **resolveRef**: Immediate resolution
   - **Why**: Required for inheritance to work
   - **Impact**: Blocks `super` implementation

### P3 - Low (Advanced features)
1. **pattern** as schema type
2. **propertyNames**
3. **namingConvention**
4. **additionalCompletions**

---

## Recommendations for Phase 2

### Immediate Actions (P0)

1. **Implement arrayOf** (est. 2-3 hours)
   - Add `"arrayOf"` case to `parse_object_form()`
   - Handle both simple and complex forms
   - Map `length` to `min_items`/`max_items`

### High Priority (P1)

2. **Implement maybeArrayOf** (est. 1-2 hours)
   - Add `"maybeArrayOf"` case
   - Expand to `anyOf(T, arrayOf(T))`
   - Add "complete-from" tag

3. **Implement record** (est. 1-2 hours)
   - Add `"record"` case
   - Convert to closed object with `required: "all"`

4. **Implement schema wrapper** (est. 1 hour)
   - Add `"schema"` case
   - Extract schema and apply outer properties

5. **Implement required: "all"** (est. 1 hour)
   - Modify `parse_object_schema()`
   - Check for string "all"
   - Expand to property keys

### Medium Priority (P2)

6. **Fix nested property extraction** (est. 2-3 hours)
   - Modify all type parsers
   - Apply properties from both outer and inner YAML
   - Careful merge logic (outer takes precedence? or merge?)

7. **Implement super inheritance** (est. 3-4 hours)
   - Add `base_schema` field to `ObjectSchema`
   - Parse `super` field
   - Design schema merging strategy

8. **Implement resolveRef** (est. 1-2 hours)
   - Add `"resolveRef"` case
   - Requires SchemaRegistry parameter
   - Design: how to pass registry to `from_yaml()`?

---

## Testing Strategy for Phase 2

For each fix:

1. **Create minimal test YAML** from quarto-cli examples
2. **Write test that fails** before implementation
3. **Implement feature**
4. **Verify test passes**
5. **Add comprehensive tests** with edge cases

Example test template:
```rust
#[test]
fn test_arrayof_simple() {
    let yaml = parse("arrayOf: string").unwrap();
    let schema = Schema::from_yaml(&yaml).unwrap();

    match schema {
        Schema::Array(arr) => {
            assert!(arr.items.is_some());
            match arr.items.as_ref().unwrap().as_ref() {
                Schema::String(_) => {},
                _ => panic!("Expected String schema"),
            }
        }
        _ => panic!("Expected Array schema"),
    }
}
```

---

## Files to Modify in Phase 2

1. **`schema.rs`**: Add missing pattern parsers
   - Lines ~368-381: Add cases to `parse_object_form()`
   - New functions: `parse_arrayof_schema()`, `parse_maybe_arrayof_schema()`, etc.

2. **`schema.rs`**: Fix existing parsers
   - Modify type parsers for nested property extraction
   - Modify `parse_object_schema()` for `required: "all"` and `super`

3. **`tests.rs`** or new test file: Add comprehensive tests
   - One test per pattern
   - Edge cases
   - Real quarto-cli examples

---

## Estimated Effort

| Category | Tasks | Estimated Time |
|----------|-------|----------------|
| P0 - arrayOf | 1 pattern | 2-3 hours |
| P1 - High priority | 4 patterns | 5-6 hours |
| P2 - Medium priority | 3 features | 6-9 hours |
| Testing | All patterns | 4-6 hours |
| **Total** | | **17-24 hours** |

Spread across 3-5 days with focused sessions.

---

## Open Questions for Implementation

1. **Nested property extraction**: Should outer properties override inner, merge, or supplement?
   - **Recommendation**: Outer supplements (only set if not already set by inner)

2. **resolveRef implementation**: Requires SchemaRegistry
   - **Option A**: Add `registry: &SchemaRegistry` parameter to `from_yaml()`
   - **Option B**: Defer resolution until validation phase
   - **Recommendation**: Option B (defer) to keep parsing simple

3. **super/baseSchema**: How to merge inherited schemas?
   - **Recommendation**: Study quarto-cli's `objectS()` implementation

4. **pattern as type**: Should we keep as StringSchema with pattern, or separate variant?
   - **Recommendation**: Keep as StringSchema (it's just sugar)

---

## Conclusion

The existing `Schema::from_yaml()` implementation provides a solid foundation but is missing **critical quarto-cli patterns**, particularly `arrayOf`, `maybeArrayOf`, `record`, and the `schema` wrapper. These patterns are used extensively in quarto-cli's schema files.

**Estimated completion**: With focused effort, Phase 2 can implement all P0 and P1 missing patterns within 2-3 days. P2 features (nested extraction, inheritance) may take an additional 2-3 days.

**Next Steps**:
1. Create test cases for all 12 patterns (use real quarto-cli YAML)
2. Verify current implementation with tests
3. Implement P0 pattern (arrayOf) first
4. Continue with P1 patterns
5. Address P2 gaps

**Risk**: The `super`/`baseSchema` inheritance feature is complex and may require significant design work. Consider deferring to separate issue if it blocks progress.

---

## Appendix A: quarto-cli Reference Lines

**from-yaml.ts** key functions:
- Line 59: `setBaseSchemaProperties()`
- Line 110: `convertFromNull()`
- Line 115: `convertFromSchema()`
- Line 124: `convertFromString()`
- Line 145: `convertFromPattern()`
- Line 157: `convertFromPath()`
- Line 162: `convertFromNumber()`
- Line 167: `convertFromBoolean()`
- Line 172: `convertFromRef()`
- Line 177: `convertFromMaybeArrayOf()`
- Line 190: `convertFromArrayOf()`
- Line 206: `convertFromAllOf()`
- Line 224: `convertFromAnyOf()`
- Line 242: `convertFromEnum()`
- Line 258: `convertFromRecord()`
- Line 284: `convertFromObject()`
- Line 430: `lookup()` (resolveRef)
- Line 456: `convertFromYaml()` main entry point

---

## Appendix B: Example Test Cases (To Be Created)

Saved for Phase 2 implementation. Each pattern will have:
- Minimal test from quarto-cli
- Complex test with nested features
- Edge case tests
- Integration test with real schema files

These tests will be written to disk in Phase 2 and used for test-driven development.
