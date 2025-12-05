# Schema YAML Syntax Reference

This document describes the YAML syntax for defining schemas in `quarto-yaml-validation`. This syntax is compatible with quarto-cli's schema system and supports all patterns used in quarto-cli schema files.

## Table of Contents

- [Overview](#overview)
- [Quick Reference](#quick-reference)
- [Primitive Types](#primitive-types)
- [Enum Types](#enum-types)
- [Array Types](#array-types)
- [Object Types](#object-types)
- [Combinators](#combinators)
- [References](#references)
- [Schema Wrappers](#schema-wrappers)
- [Annotations](#annotations)
- [Pattern Correspondence Table](#pattern-correspondence-table)

## Overview

The schema system uses YAML to define validation rules for configuration data. Schemas can be defined in three main forms:

1. **Short form**: Simple string like `"boolean"`, `"string"`, `"number"`
2. **Object form**: Hash with schema type key like `{boolean: {...}}`, `{string: {...}}`
3. **Inline arrays**: Arrays for enum values like `[val1, val2, val3]`

## Quick Reference

```yaml
# Primitive types
string                    # Simple string
number                    # Numeric value
boolean                   # True/false
null                      # Null value
any                       # Any value
path                      # File path (alias for string)

# Enum
enum: [value1, value2]    # Inline enum
[value1, value2, value3]  # Alternative inline form

# Arrays
array:                    # Heterogeneous array
  items: string
arrayOf: string           # Homogeneous array (all items same type)
maybeArrayOf: string      # Value OR array of values

# Objects
object:                   # Key-value mapping
  properties:
    name: string
  required: [name]
record:                   # Shorthand for closed object with all properties required
  name: string
  age: number

# Combinators
anyOf: [string, number]   # Match any subschema
allOf: [schema1, schema2] # Match all subschemas

# References
ref: schema/base          # Reference to another schema

# Schema wrapper
schema: string            # Add annotations without nesting
```

## Primitive Types

### String

```yaml
# Short form
string

# Object form with validation
string:
  minLength: 1
  maxLength: 100
  pattern: "^[a-z]+$"
  description: "A lowercase string"
```

**Rust mapping**: `Schema::String(StringSchema { ... })`

### Number

```yaml
# Short form
number

# Object form with validation
number:
  minimum: 0
  maximum: 100
  exclusiveMinimum: 0
  exclusiveMaximum: 100
  multipleOf: 5
  description: "A number between 0 and 100"
```

**Rust mapping**: `Schema::Number(NumberSchema { ... })`

### Boolean

```yaml
# Short form
boolean

# Object form with annotations
boolean:
  description: "Enable feature"
  default: false
```

**Rust mapping**: `Schema::Boolean(BooleanSchema { ... })`

### Null, Any, Path

```yaml
null      # Only matches null
any       # Matches any value
path      # File path (same as string)
```

**Rust mapping**:
- `Schema::Null(NullSchema { ... })`
- `Schema::Any(AnySchema { ... })`
- `Schema::String(StringSchema { ... })` (for path)

## Enum Types

Enums define a fixed set of allowed values.

### Inline Array Form

```yaml
# Simplest form - array at top level
[value1, value2, value3]
```

### Explicit Form

```yaml
enum:
  values: [red, green, blue]
  description: "Color choices"
```

**Real-world example** (from quarto-cli):
```yaml
# document-text.yml - wrap option
enum: [auto, none, preserve]
```

**Rust mapping**: `Schema::Enum(EnumSchema { values, ... })`

## Array Types

### Array (Heterogeneous)

Standard JSON Schema array with explicit items schema:

```yaml
array:
  items: string
  minItems: 1
  maxItems: 10
  uniqueItems: true
```

**Rust mapping**: `Schema::Array(ArraySchema { ... })`

### arrayOf (Homogeneous)

**Quarto extension** - shorthand for arrays where all items have the same type.

```yaml
# Simple form
arrayOf: string

# With length constraint
arrayOf:
  schema: string
  length: 2

# Nested arrays
arrayOf:
  arrayOf:
    schema: string
    length: 2
```

**Real-world examples** (from quarto-cli):
```yaml
# definitions.yml - pandoc-shortcodes
arrayOf: path

# definitions.yml - pandoc-format-request-headers
arrayOf:
  arrayOf:
    schema: string
    length: 2

# document-execute.yml - julia exeflags
arrayOf: string
```

**Rust mapping**: `Schema::Array(ArraySchema { items: Some(Box::new(inner)), ... })`
- Simple form: Items set to inner schema
- With length: Both `min_items` and `max_items` set to `length`

### maybeArrayOf

**Quarto extension** - value can be either T or an array of T. Expands to `anyOf: [T, arrayOf(T)]`.

```yaml
# Simple form
maybeArrayOf: string

# Accepts: "value" OR ["value1", "value2"]
```

**Real-world example** (from quarto-cli):
```yaml
# definitions.yml - contents-auto
auto:
  anyOf:
    - boolean
    - maybeArrayOf: string
```

**Rust mapping**: `Schema::AnyOf(AnyOfSchema { schemas: [inner, array_of_inner], ... })`
- Includes `complete-from` tag for IDE support

## Object Types

### Object

Standard JSON Schema object with properties:

```yaml
object:
  properties:
    name: string
    age: number
    email:
      string:
        pattern: "^.+@.+$"
  patternProperties:
    "^x-": string
  additionalProperties: boolean
  required: [name]
  closed: true
  minProperties: 1
  maxProperties: 10
```

**Special feature - required: all**:
```yaml
object:
  properties:
    foo: string
    bar: number
    baz: boolean
  required: all  # Expands to [foo, bar, baz]
```

**Real-world example** (from quarto-cli):
```yaml
# document-execute.yml - kernelspec
object:
  properties:
    display_name:
      string:
        description: The name to display in the UI.
    language:
      string:
        description: The name of the language the kernel implements.
    name:
      string:
        description: The name of the kernel.
  required: all
```

**Rust mapping**: `Schema::Object(ObjectSchema { ... })`

### record

**Quarto extension** - shorthand for a closed object where all properties are required.

```yaml
# Form 1: Explicit properties
record:
  properties:
    type: string
    value: number

# Form 2: Shorthand (properties inferred)
record:
  type: string
  value: number
```

Both forms expand to:
```yaml
object:
  properties:
    type: string
    value: number
  required: [type, value]
  closed: true
```

**Real-world example** (from quarto-cli):
```yaml
# definitions.yml - pandoc-format-filters
arrayOf:
  anyOf:
    - path
    - object:
        properties:
          type: string
          path: path
        required: [path]
    - record:
        type:
          enum: [citeproc]
```

**Rust mapping**: `Schema::Object(ObjectSchema { closed: true, required: all_keys, ... })`

## Combinators

### anyOf

Validates if **any** of the subschemas matches:

```yaml
# Inline array form
anyOf: [string, number, boolean]

# Explicit form with annotations
anyOf:
  schemas: [string, number]
  description: "String or number"
```

**Real-world example** (from quarto-cli):
```yaml
# definitions.yml - date
anyOf:
  - string
  - object:
      properties:
        value: string
        format: string
      required: [value]
```

**Rust mapping**: `Schema::AnyOf(AnyOfSchema { schemas, ... })`

### allOf

Validates if **all** of the subschemas match:

```yaml
# Inline array form
allOf: [schema1, schema2]

# Explicit form
allOf:
  schemas: [schema1, schema2]
  description: "Must match both"
```

**Rust mapping**: `Schema::AllOf(AllOfSchema { schemas, ... })`

## References

Reference another schema by identifier:

```yaml
ref: schema/base
```

Alternative syntax:
```yaml
$ref: schema/base
```

**Rust mapping**: `Schema::Ref(RefSchema { reference: "schema/base", ... })`

## Schema Wrappers

The `schema` key allows adding annotations to a schema without nesting under a type key.

### Without Schema Wrapper

```yaml
anyOf:
  - boolean
  - string
description: "A boolean or string"
completions: ["true", "false", "auto"]
```

This requires parsing the entire hash to extract the schema type.

### With Schema Wrapper

```yaml
schema:
  anyOf:
    - boolean
    - string
description: "A boolean or string"
completions: ["true", "false", "auto"]
```

Cleaner separation when the schema is complex.

**Real-world example** (from quarto-cli):
```yaml
# document-text.yml - eol field
schema:
  enum: [lf, crlf, native]
description: "Manually specify line endings"

# document-execute.yml - julia env
schema:
  arrayOf: string
  description: Environment variables to pass to the Julia worker process.
```

**Rust mapping**: Transparent - parses inner schema and applies outer annotations

## Annotations

All schema types support these annotation fields:

```yaml
description:
  short: "Brief description"
  long: |
    Longer multiline
    description

completions: [value1, value2]  # IDE completion suggestions
hidden: true                   # Hide from UI
default: defaultValue          # Default value

tags:
  category: input
  custom-key: custom-value
```

**Rust mapping**: All annotations stored in `SchemaAnnotations` struct

## Pattern Correspondence Table

| YAML Pattern | Rust Type | Status | Notes |
|--------------|-----------|--------|-------|
| `string` | `Schema::String` | ✅ Complete | Short form |
| `string: {minLength: 1}` | `Schema::String` | ✅ Complete | Object form with validation |
| `number` | `Schema::Number` | ✅ Complete | Short form |
| `number: {minimum: 0}` | `Schema::Number` | ✅ Complete | Object form with validation |
| `boolean` | `Schema::Boolean` | ✅ Complete | Short form |
| `null` | `Schema::Null` | ✅ Complete | Short form |
| `any` | `Schema::Any` | ✅ Complete | Short form |
| `path` | `Schema::String` | ✅ Complete | Alias for string |
| `[val1, val2]` | `Schema::Enum` | ✅ Complete | Inline enum |
| `enum: {values: [...]}` | `Schema::Enum` | ✅ Complete | Explicit enum |
| `array: {items: T}` | `Schema::Array` | ✅ Complete | Standard array |
| `arrayOf: T` | `Schema::Array` | ✅ Complete | Quarto extension (P0) |
| `arrayOf: {schema: T, length: N}` | `Schema::Array` | ✅ Complete | With length constraint |
| `maybeArrayOf: T` | `Schema::AnyOf` | ✅ Complete | Quarto extension (P1) |
| `object: {properties: {...}}` | `Schema::Object` | ✅ Complete | Standard object |
| `object: {required: all}` | `Schema::Object` | ✅ Complete | Auto-expand required (P1) |
| `record: {...}` | `Schema::Object` | ✅ Complete | Quarto extension (P1) |
| `anyOf: [...]` | `Schema::AnyOf` | ✅ Complete | Combinator |
| `allOf: [...]` | `Schema::AllOf` | ✅ Complete | Combinator |
| `ref: id` | `Schema::Ref` | ✅ Complete | Reference |
| `schema: T` | (transparent) | ✅ Complete | Schema wrapper (P1) |

### Not Yet Implemented (P2/P3)

| YAML Pattern | Priority | Notes |
|--------------|----------|-------|
| Nested property extraction | P2 | Double setBaseSchemaProperties pattern |
| `super: base` inheritance | P2 | Schema inheritance |
| `resolveRef` vs `ref` | P2 | Reference resolution distinction |
| `propertyNames` | P2 | Property name validation |
| `namingConvention` | P2 | Naming convention validation |
| `additionalCompletions` | P2 | Additional completion sources |
| `pattern` as schema type | P3 | Pattern-based validation as type |

## Usage Examples

### Basic Validation

```rust
use quarto_yaml_validation::Schema;

// Parse a schema from YAML
let yaml_text = r#"
string:
  minLength: 1
  maxLength: 100
"#;
let yaml = quarto_yaml::parse(yaml_text)?;
let schema = Schema::from_yaml(&yaml)?;

// Use schema for validation (future API)
// let result = schema.validate(&data);
```

### Working with Complex Schemas

```rust
// Parse a complex anyOf schema
let yaml_text = r#"
anyOf:
  - string
  - object:
      properties:
        value: string
        format: string
      required: [value]
"#;
let yaml = quarto_yaml::parse(yaml_text)?;
let schema = Schema::from_yaml(&yaml)?;

// Access schema information
match schema {
    Schema::AnyOf(anyof) => {
        println!("AnyOf with {} alternatives", anyof.schemas.len());
    }
    _ => unreachable!(),
}
```

### Loading quarto-cli Schemas

```rust
// Load and parse a quarto-cli schema file
let yaml_content = std::fs::read_to_string("document-execute.yml")?;
let yaml = quarto_yaml::parse(&yaml_content)?;

// The file is an array of field definitions
let items = yaml.as_array().expect("Expected array");

for item in items {
    let name = item
        .get_hash_value("name")
        .and_then(|v| v.yaml.as_str())
        .unwrap_or("<unknown>");

    if let Some(schema_yaml) = item.get_hash_value("schema") {
        let schema = Schema::from_yaml(schema_yaml)?;
        println!("Parsed schema for field: {}", name);
    }
}
```