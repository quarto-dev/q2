# Comprehensive Plan for bd-8: YAML Schema Deserialization from quarto-cli YAML Files

**Date**: 2025-10-27
**Issue**: bd-8 - YAML schema deserialization: Add Deserialize impl for Schema enum
**Blocks**: bd-9 - SchemaField and schema file loading

## Executive Summary

This document provides a comprehensive plan for implementing full quarto-cli YAML schema syntax support in Rust. After deep analysis of the quarto-cli codebase, particularly `from-yaml.ts`, the schema YAML files, and TypeScript type definitions, I've identified the exact patterns we need to support and how to port them to Rust.

**Key Finding**: The current Rust `Schema::from_yaml()` implementation (~1300 lines) already exists but may not handle all quarto-cli syntax variations. We need to audit it against the patterns found in quarto-cli and ensure complete compatibility.

## Background: How quarto-cli Handles YAML Schemas

### Build Process Flow

1. **Loading** (`ensureSchemaResources()` in `yaml-schema.ts`):
   - Reads all `.yml` files from `resources/schema/`
   - Stores as raw YAML in `YamlIntelligenceResources`

2. **Conversion** (`convertFromYaml()` in `from-yaml.ts`):
   - Converts YAML descriptions → TypeScript Schema objects
   - Handles multiple syntax forms (short, object, nested)
   - Applies base schema properties (annotations, completions, tags)

3. **Schema Field Processing** (`convertFromFieldsObject()` in `from-yaml.ts`):
   - Converts field arrays (with `name:`, `schema:`, `description:`) → property maps
   - Used for document/cell/project schemas

4. **Build Output** (`buildIntelligenceResources()` in `build-schema-file.ts`):
   - Pre-compiles all schemas
   - Exports to JSON for runtime loading (performance)
   - Generates TypeScript types, JSON Schema, and Zod schemas

### Key quarto-cli Types

**TypeScript Schema Union** (from `types.ts`):
```typescript
type Schema =
  | FalseSchema    // literal false
  | TrueSchema     // literal true
  | BooleanSchema  // { type: "boolean", ...annotations }
  | NumberSchema   // { type: "number" | "integer", minimum?, maximum?, ...}
  | StringSchema   // { type: "string", pattern?, ...}
  | NullSchema     // { type: "null", ...}
  | EnumSchema     // { type: "enum", enum: JSONValue[], ...}
  | AnySchema      // { type: "any", ...} (not in JSON Schema)
  | AnyOfSchema    // { type: "anyOf", anyOf: Schema[], ...}
  | AllOfSchema    // { type: "allOf", allOf: Schema[], ...}
  | ArraySchema    // { type: "array", items?: Schema, ...}
  | ObjectSchema   // { type: "object", properties?, required?, ...}
  | RefSchema      // { type: "ref", $ref: string, ...}
```

**SchemaAnnotations** (metadata on all schemas):
```typescript
interface SchemaAnnotations {
  $id?: string;                    // For references
  documentation?: string | {       // For HTML docs
    short?: string;
    long?: string;
  };
  description?: string;            // For error messages
  errorMessage?: string;           // Custom error override
  hidden?: boolean;                // Hide from completions
  completions?: string[];          // Completion values
  cachedCompletions?: Completion[]; // Runtime cache
  tags?: Record<string, unknown>;  // Arbitrary metadata
  exhaustiveCompletions?: boolean; // Auto-suggest next
}
```

**SchemaField** (from field-based YAML files):
```typescript
interface SchemaField {
  name: string;                    // Field name
  schema: ConcreteSchema;          // The schema
  hidden?: boolean;
  default?: any;
  alias?: string;                  // Alternative field name
  disabled?: string[];             // Disabled for formats
  enabled?: string[];              // Enabled for formats
  description: string | {          // Documentation
    short: string;
    long: string;
  };
  tags?: Record<string, any>;      // Metadata
}
```

## YAML Syntax Patterns (from quarto-cli)

### Pattern 1: Literal Short Forms

**YAML**:
```yaml
boolean
string
number
path
any
null
```

**TypeScript**:
```typescript
// convertFromYaml() lines 458-471
if (yaml === "boolean") return booleanS;
if (yaml === "string") return stringS;
if (yaml === "path") return stringS;  // path is alias for string
if (yaml === "number") return numberS;
if (yaml === "any") return anyS();
if (yaml === null) return nullS;
```

**Use Case**: Simplest schema declarations (e.g., `properties: { title: string }`)

---

### Pattern 2: Object Forms with Properties

**YAML**:
```yaml
boolean:
  description: "Allow or disallow something"
  completions: [true, false]
  hidden: false

string:
  pattern: "^[a-z]+$"
  description: "Lowercase letters only"

number:
  minimum: 0
  maximum: 100
  description: "A percentage"
```

**TypeScript**:
```typescript
// convertFromBoolean(), convertFromString(), convertFromNumber()
// All call setBaseSchemaProperties() to add:
// - id, hidden, tags, errorDescription
// - description (stored in tags.description)
// - completions, additionalCompletions
```

**Rust Mapping**:
- `setBaseSchemaProperties()` → populate `SchemaAnnotations` struct
- Extract nested properties for type-specific fields (pattern, minimum, etc.)

---

### Pattern 3: Inline Enum Arrays

**YAML**:
```yaml
enum: [value1, value2, value3]
# OR
[value1, value2, value3]  # Inline shorthand
```

**TypeScript**:
```typescript
// convertFromEnum() lines 242-255
if (schema.hasOwnProperty("values")) {
  // Object form: { enum: { values: [...] } }
  return enumS(...schema.values);
} else {
  // Array form: { enum: [...] }
  return enumS(...schema);
}
```

**Examples** (from `definitions.yml`):
```yaml
- id: math-methods
  enum:
    values: [plain, webtex, gladtex, mathml, mathjax, katex]

- id: page-column
  enum: [body, body-outset, page, margin, screen]
```

---

### Pattern 4: anyOf / allOf with Two Syntaxes

**YAML Syntax A** (Array form - most common):
```yaml
anyOf:
  - boolean
  - string
  - enum: [null]
```

**YAML Syntax B** (Object form with `schemas` key):
```yaml
anyOf:
  schemas:
    - boolean
    - string
  description: "A boolean or string"
```

**TypeScript**:
```typescript
// convertFromAnyOf() lines 224-238
if (yaml.anyOf.schemas) {
  // Object form: { anyOf: { schemas: [...], description: "..." } }
  const inner = yaml.anyOf.schemas.map(convertFromYaml);
  return setBaseSchemaProperties(
    yaml,
    setBaseSchemaProperties(yaml.anyOf, anyOfS(...inner))
  );
} else {
  // Array form: { anyOf: [...] }
  const inner = yaml.anyOf.map(convertFromYaml);
  return setBaseSchemaProperties(yaml, anyOfS(...inner));
}
```

**Note**: `setBaseSchemaProperties` is called twice when using object form:
1. Once on `yaml.anyOf` (nested properties)
2. Once on `yaml` (outer properties)

---

### Pattern 5: arrayOf with Two Forms

**YAML Syntax A** (Simple):
```yaml
arrayOf: string
```

**YAML Syntax B** (With schema key and length):
```yaml
arrayOf:
  schema: string
  length: 5  # Fixed-length arrays
```

**TypeScript**:
```typescript
// convertFromArrayOf() lines 190-203
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
```

**Quarto Extension**: The `length` property isn't in standard JSON Schema. In quarto-cli, it maps to `minItems` and `maxItems` both set to the same value.

---

### Pattern 6: maybeArrayOf (Quarto-Specific)

**YAML**:
```yaml
maybeArrayOf: string
```

**TypeScript** (lines 177-187):
```typescript
// Expands to anyOf(T, arrayOf(T))
const inner = convertFromYaml(yaml.maybeArrayOf);
const schema = tagSchema(
  anyOfS(inner, arrayOfS(inner)),
  { "complete-from": ["anyOf", 0] }  // Complete from first option
);
return setBaseSchemaProperties(yaml, schema);
```

**Use Case**: Fields that accept either a single value or an array (e.g., `auto: "docs/"` or `auto: ["docs/", "posts/"]`)

**Rust Decision**: Treat as syntactic sugar that expands to `anyOf` during parsing.

---

### Pattern 7: Object Schemas

**YAML**:
```yaml
object:
  properties:
    title: string
    author: string
    date:
      anyOf:
        - string
        - object:
            properties:
              value: string
              format: string
  required: [title]
  # OR
  required: all  # All properties required
  closed: true   # No additional properties
  additionalProperties: string  # Type for extra props
  patternProperties:
    "^x-": string  # Regex-matched props
  super:
    resolveRef: schema/base  # Inheritance
```

**TypeScript** (lines 284-427):
```typescript
// Complex function handling:
// - properties (recursive convertFromYaml on values)
// - patternProperties (recursive convertFromYaml)
// - propertyNames (enum for closed schemas)
// - additionalProperties (can be false or schema)
// - super/baseSchema (inheritance, can be array)
// - required (array or "all")
// - closed (sets propertyNames to enum of property keys)
// - namingConvention (ignore, dash-case, underscore_case, capitalizationCase)
```

**Naming Convention Feature**: quarto-cli validates property names against patterns:
- `dash-case` / `kebab-case`: `foo-bar-baz`
- `underscore_case` / `snake_case`: `foo_bar_baz`
- `capitalizationCase` / `camelCase`: `fooBarBaz`
- `ignore`: No validation

**Closed Schemas**: When `closed: true`, sets `propertyNames` to an enum of the property keys and requires `namingConvention: ignore`.

---

### Pattern 8: record (Quarto-Specific)

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

**TypeScript** (lines 258-281):
```typescript
// Converts to closed object with required: all
if (yaml.record.properties) {
  return convertFromObject({
    object: {
      properties: yaml.record.properties,
      closed: true,
      required: "all"
    }
  });
} else {
  return convertFromObject({
    object: {
      properties: yaml.record,
      closed: true,
      required: "all"
    }
  });
}
```

**Difference from Object**:
- `record`: All values must have the same type, all keys required, closed
- `object`: Flexible properties, some required, can be open

**Use Case**: Dictionary/map types where all values share a type.

---

### Pattern 9: ref and resolveRef

**YAML (ref)**:
```yaml
ref: schema/base
# OR
ref: date
```

**YAML (resolveRef)** - used in `super:`:
```yaml
object:
  super:
    resolveRef: schema/base
  properties:
    # ... more properties
```

**TypeScript**:
```typescript
// convertFromRef() lines 172-174
return setBaseSchemaProperties(yaml, refS(yaml.ref, `be ${yaml.ref}`));

// lookup() lines 430-435 (for resolveRef)
if (!hasSchemaDefinition(yaml.resolveRef)) {
  throw new Error(`lookup of key ${yaml.resolveRef} failed`);
}
return getSchemaDefinition(yaml.resolveRef);
```

**Difference**:
- `ref`: Creates a RefSchema that references another schema by ID
- `resolveRef`: Immediately looks up and returns the referenced schema (used for inheritance)

---

### Pattern 10: pattern (String Pattern Matching)

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

**TypeScript** (lines 145-154):
```typescript
if (typeof yaml.pattern === "string") {
  return setBaseSchemaProperties(yaml, regexSchema(yaml.pattern));
} else {
  return setBaseSchemaProperties(
    yaml,
    setBaseSchemaProperties(yaml.pattern, regexSchema(yaml.pattern.regex))
  );
}
```

**Note**: In quarto-cli's schema system, `pattern` is a schema type (not just a property of string schemas).

---

### Pattern 11: schema (Explicit Schema Wrapper)

**YAML**:
```yaml
schema:
  anyOf:
    - boolean
    - string
description: "Documentation for this schema"
```

**TypeScript** (lines 115-118):
```typescript
const schema = convertFromYaml(yaml.schema);
return setBaseSchemaProperties(yaml, schema);
```

**Use Case**: When you want to add properties (description, completions, etc.) to a schema without nesting under a type key.

---

### Pattern 12: Field-Based Schema Files

**YAML** (e.g., `document-execute.yml`):
```yaml
- name: engine
  schema:
    string:
      completions: [jupyter, knitr, julia]
  description: "Engine used for executable code blocks."

- name: cache
  tags:
    execute-only: true
  schema:
    anyOf:
      - boolean
      - enum: [refresh]
  default: false
  description:
    short: "Cache results of computations."
    long: |
      Cache results...
```

**TypeScript** (`SchemaField` interface + `convertFromFieldsObject()`):
```typescript
export interface SchemaField {
  name: string;
  schema: ConcreteSchema;
  hidden?: boolean;
  default?: any;
  alias?: string;
  disabled?: string[];  // Format restrictions
  enabled?: string[];
  description: string | { short: string; long: string };
  tags?: Record<string, any>;
}
```

**Processing**:
1. Parse array of field definitions
2. Extract `name`, `schema`, metadata
3. Convert `schema` using `convertFromYaml()`
4. Apply field-level annotations
5. Create `properties` object mapping field names to schemas

**Rust Structure Needed**:
```rust
pub struct SchemaField {
    pub name: String,
    pub schema: Schema,
    pub hidden: Option<bool>,
    pub default: Option<YamlWithSourceInfo>,
    pub alias: Option<String>,
    pub disabled: Option<Vec<String>>,
    pub enabled: Option<Vec<String>>,
    pub description: Option<Description>,
    pub tags: HashMap<String, serde_json::Value>,
}

pub enum Description {
    Short(String),
    Long { short: String, long: String },
}
```

---

## Current State in Rust

### What We Have

**File**: `private-crates/quarto-yaml-validation/src/schema.rs` (~1300 lines)

**Implemented**:
- `Schema::from_yaml(yaml: &YamlWithSourceInfo) -> SchemaResult<Schema>` (line 288)
- Parsing logic for most schema types
- Schema enum with 13 variants (matches quarto-cli)

**Schema Enum**:
```rust
pub enum Schema {
    False,
    True,
    Boolean(BooleanSchema),
    Number(NumberSchema),
    String(StringSchema),
    Null(NullSchema),
    Enum(EnumSchema),
    Any(AnySchema),
    AnyOf(AnyOfSchema),
    AllOf(AllOfSchema),
    Array(ArraySchema),
    Object(ObjectSchema),
    Ref(RefSchema),
}
```

### What We Need to Audit

**Critical Questions**:

1. **Does `Schema::from_yaml()` handle all 12 syntax patterns above?**
   - Short forms (pattern 1)
   - Object forms with properties (pattern 2)
   - Inline enum arrays (pattern 3)
   - anyOf/allOf dual syntax (pattern 4)
   - arrayOf dual syntax (pattern 5)
   - maybeArrayOf expansion (pattern 6)
   - Object schemas with all features (pattern 7)
   - record conversion (pattern 8)
   - ref/resolveRef (pattern 9)
   - pattern schemas (pattern 10)
   - explicit schema wrapper (pattern 11)

2. **Does it correctly apply `setBaseSchemaProperties` equivalent?**
   - Extract `id`, `hidden`, `tags`, `errorDescription`
   - Extract `description` (goes to both `tags.description` and `schema.description`)
   - Extract `completions`, `additionalCompletions`
   - Handle nested property extraction (double `setBaseSchemaProperties` calls)

3. **Object schema features**:
   - `required: "all"` support?
   - `closed: true` → `propertyNames` conversion?
   - `super` / `baseSchema` inheritance?
   - `patternProperties` support?
   - `namingConvention` validation?

4. **Quarto-specific features**:
   - `maybeArrayOf` → `anyOf(T, arrayOf(T))` expansion?
   - `record` → closed object conversion?
   - `path` → string alias?

### What's Definitely Missing

**SchemaField Processing** (belongs in bd-9):
```rust
// Not yet implemented:
pub struct SchemaField { ... }
pub fn load_schema_file(path: &Path) -> Result<Vec<SchemaField>, SchemaError>
pub fn convert_from_fields_object(fields: Vec<SchemaField>) -> HashMap<String, Schema>
```

---

## Implementation Plan for bd-8

### Phase 1: Audit Existing Implementation (Day 1-2)

**Goal**: Determine what's already working and what needs fixing.

**Tasks**:
1. **Read `Schema::from_yaml()` implementation thoroughly**
   - Document which patterns it handles
   - Note any deviations from quarto-cli behavior

2. **Create test cases for all 12 patterns**
   - One test per pattern from the analysis above
   - Include actual YAML from quarto-cli schema files
   - Compare output structure to what quarto-cli would produce

3. **Test with real schema files**
   - Try parsing `definitions.yml`
   - Try parsing `document-execute.yml` schemas (just the schema part, not the field structure)
   - Document any failures or discrepancies

**Deliverable**: Audit report documenting:
- ✅ What works
- ❌ What's broken
- ⚠️ What's partially working
- 🆕 What's missing

---

### Phase 2: Fix Identified Gaps (Day 3-5)

**Priority Order**:

1. **Critical** (blocks common use cases):
   - anyOf/allOf dual syntax (if broken)
   - Object properties with recursive schemas
   - Enum inline arrays
   - ref schemas

2. **High** (used in many quarto-cli schemas):
   - maybeArrayOf expansion
   - record → object conversion
   - arrayOf dual syntax
   - pattern schemas

3. **Medium** (important for completeness):
   - setBaseSchemaProperties equivalent (annotations)
   - Double property extraction (nested object forms)
   - `required: "all"` support
   - `super` inheritance

4. **Low** (nice to have):
   - closed object → propertyNames conversion
   - namingConvention validation
   - patternProperties

**For Each Fix**:
1. Write failing test case (from real quarto-cli YAML)
2. Implement fix
3. Verify test passes
4. Add comprehensive comments explaining the quarto-cli correspondence

**Example Test**:
```rust
#[test]
fn test_maybe_array_of_expansion() {
    let yaml = parse("maybeArrayOf: string", None).unwrap();
    let schema = Schema::from_yaml(&yaml).unwrap();

    // Should expand to anyOf(string, arrayOf(string))
    match schema {
        Schema::AnyOf(anyof) => {
            assert_eq!(anyof.schemas.len(), 2);
            assert!(matches!(anyof.schemas[0], Schema::String(_)));
            assert!(matches!(anyof.schemas[1], Schema::Array(_)));

            // Check tag for completion hint
            assert!(anyof.annotations.tags
                .as_ref()
                .and_then(|t| t.get("complete-from"))
                .is_some());
        }
        _ => panic!("Expected AnyOf schema"),
    }
}
```

---

### Phase 3: Comprehensive Testing (Day 6)

**Real-World Tests**:

1. **Parse all quarto-cli definition schemas**:
```rust
#[test]
fn test_parse_quarto_cli_definitions() {
    let path = PathBuf::from("../external-sources/quarto-cli/src/resources/schema/definitions.yml");
    let yaml = parse_file(&path).unwrap();

    let definitions = yaml.as_array().unwrap();
    for (i, def) in definitions.iter().enumerate() {
        let schema = Schema::from_yaml(def);
        assert!(schema.is_ok(), "Failed to parse definition {}: {:?}", i, schema.err());
    }
}
```

2. **Parse schemas from document-execute.yml**:
```rust
#[test]
fn test_parse_document_execute_schemas() {
    // Extract just the `schema:` values from field definitions
    // and verify they parse correctly
}
```

3. **Round-trip test**: Parse YAML → Schema → validate behavior matches expected

**Deliverable**: All quarto-cli schemas parse successfully.

---

### Phase 4: Documentation (Day 7)

**Create documentation**:

1. **API Documentation**:
   - Document each pattern `Schema::from_yaml()` handles
   - Provide examples from quarto-cli
   - Note any intentional deviations

2. **Comparison Document**:
   - Table mapping quarto-cli patterns → Rust code
   - Reference quarto-cli line numbers
   - Explain differences (if any)

3. **Usage Guide**:
   - How to use `Schema::from_yaml()`
   - Common patterns
   - Error handling

**File**: `private-crates/quarto-yaml-validation/SCHEMA-FROM-YAML.md`

---

## Separation from bd-9

**bd-8 Scope**: Schema parsing from YAML (single schema objects)
- `Schema::from_yaml()` handles all patterns
- Works on individual schema definitions
- Tested with schema YAML structures

**bd-9 Scope** (next issue): SchemaField and file loading
- `SchemaField` struct and parsing
- `load_schema_file()` for reading YAML arrays
- `convert_from_fields_object()` for building property maps
- Schema registry for ref resolution

**Boundary**: bd-8 completes when we can parse any individual schema from YAML. bd-9 handles the field-level structure and file I/O.

---

## Success Criteria

bd-8 is complete when:

1. ✅ All 12 YAML patterns from quarto-cli are supported
2. ✅ All schemas in `definitions.yml` parse successfully
3. ✅ All schemas in `document-execute.yml` parse successfully (just the schema values, not the field structure)
4. ✅ Tests verify correct structure matching quarto-cli behavior
5. ✅ Documentation explains pattern correspondences
6. ✅ No regressions in existing validation tests

---

## Open Design Questions

### Q1: Enum Value Representation

**quarto-cli**: Uses `serde_json::Value` for enum values (can be string, number, boolean, null)

**Rust Options**:
A. Use `serde_json::Value` (matches quarto-cli exactly)
B. Use custom enum: `EnumValue { String, Number, Bool, Null }`

**Recommendation**: Option A (use `serde_json::Value`) for exact compatibility.

---

### Q2: maybeArrayOf Treatment

**Options**:
A. Keep as separate Schema variant
B. Expand to `anyOf(T, arrayOf(T))` during parsing
C. Treat as special case in validation only

**Recommendation**: Option B (expand during parsing) - simpler, matches quarto-cli exactly.

---

### Q3: record vs Object

**Options**:
A. Keep as separate Schema variant
B. Convert to ObjectSchema during parsing with `closed: true, required: all`

**Recommendation**: Option B (convert to ObjectSchema) - one less variant, matches quarto-cli.

---

### Q4: Naming Convention Validation

**Implementation**:
- Store in `ObjectSchema.naming_convention: Option<NamingConvention>`
- Validate during validation phase (not parsing)

**Enum**:
```rust
pub enum NamingConvention {
    Ignore,
    DashCase,
    UnderscoreCase,
    CapitalizationCase,
}
```

---

### Q5: Source Location Tracking

**Current**: `YamlWithSourceInfo` provides source tracking for YAML elements

**Question**: Should we preserve source locations in Schema structs?

**Options**:
A. Store `SourceInfo` in `SchemaAnnotations`
B. Keep separate parallel structure (like validation errors)
C. Don't store (can reconstruct from YAML if needed)

**Recommendation**: Option C for now (simplify), Option A if we need schema source locations for error reporting.

---

## Dependencies

**Crates**:
- `quarto-yaml` - YamlWithSourceInfo parsing ✅ (exists)
- `yaml-rust2` - Raw YAML access ✅ (via quarto-yaml)
- `serde_json` - For enum value storage ✅ (in use)
- `thiserror` - Error types ✅ (in use)

**Files**:
- `private-crates/quarto-yaml-validation/src/schema.rs` - Audit and fix
- `private-crates/quarto-yaml-validation/src/error.rs` - Already exists
- `private-crates/quarto-yaml-validation/src/tests.rs` - Add comprehensive tests

---

## Timeline

**Estimated**: 1 week

- **Day 1-2**: Audit existing implementation, create test matrix
- **Day 3-5**: Fix identified gaps, implement missing patterns
- **Day 6**: Comprehensive testing with real quarto-cli schemas
- **Day 7**: Documentation and polish

---

## References

**quarto-cli Files Analyzed**:
- `src/core/schema/build-schema-file.ts` - Build process
- `src/core/schema/yaml-schema.ts` - Resource loading
- `src/core/lib/yaml-schema/from-yaml.ts` - **CRITICAL** - conversion logic (750 lines)
- `src/core/lib/yaml-schema/definitions.ts` - Schema definition loading
- `src/core/lib/yaml-schema/types.ts` - TypeScript type definitions
- `src/resources/schema/definitions.yml` - Reusable schema definitions
- `src/resources/schema/document-execute.yml` - Example field-based schema
- `src/resources/schema/schema.yml` - Meta-schema (schemas for schemas)

**Rust Files**:
- `private-crates/quarto-yaml-validation/src/schema.rs` - Current implementation
- `crates/quarto-yaml/src/yaml_with_source_info.rs` - YAML parsing infrastructure

**Design Documents**:
- `claude-notes/yaml-schema-from-yaml-design.md` - Original design (needs updating)
- `claude-notes/yaml-schema-revision-summary.md` - YAML 1.2 requirement rationale
