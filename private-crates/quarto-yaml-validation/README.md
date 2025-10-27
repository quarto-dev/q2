# quarto-yaml-validation

A Rust library for validating YAML data against schemas defined in quarto-cli's YAML schema format.

## Features

- **Schema Parsing**: Parse schemas from YAML using quarto-cli's syntax
- **quarto-cli Compatibility**: Full support for all patterns used in quarto-cli schema files
- **Type Safety**: Strongly-typed Rust representation of schemas
- **Source Tracking**: Maintains source location information for error reporting
- **Comprehensive Testing**: 100% success rate parsing real quarto-cli schemas

## Supported Schema Patterns

### Primitive Types
- `string`, `number`, `boolean`, `null`, `any`, `path`
- Validation constraints (minLength, maximum, pattern, etc.)

### Collections
- **Enum**: Fixed set of allowed values
- **Array**: Heterogeneous arrays with item schemas
- **arrayOf**: Homogeneous arrays (quarto extension)
- **maybeArrayOf**: Value OR array of values (quarto extension)

### Objects
- **object**: Standard key-value mappings with properties
- **record**: Closed objects with all properties required (quarto extension)
- **required: "all"**: Auto-expand to all property keys

### Combinators
- **anyOf**: Match any subschema
- **allOf**: Match all subschemas

### Advanced
- **ref**: Schema references
- **schema wrapper**: Add annotations without nesting
- **Annotations**: descriptions, completions, tags, etc.

## Quick Start

```rust
use quarto_yaml_validation::Schema;

// Parse a schema from YAML
let yaml_text = r#"
object:
  properties:
    name: string
    age: number
  required: [name]
"#;

let yaml = quarto_yaml::parse(yaml_text)?;
let schema = Schema::from_yaml(&yaml)?;

// Access schema information
match schema {
    Schema::Object(obj) => {
        println!("Object with {} properties", obj.properties.len());
        println!("Required: {:?}", obj.required);
    }
    _ => unreachable!(),
}
```

## Validation

```rust
use quarto_yaml_validation::{Schema, SchemaRegistry, ValidationDiagnostic, validate};
use quarto_source_map::SourceContext;

// Parse schema and document
let schema_yaml = quarto_yaml::parse(r#"
object:
  properties:
    age:
      number:
        minimum: 0
        maximum: 150
  required: [age]
"#)?;
let schema = Schema::from_yaml(&schema_yaml)?;

let doc = quarto_yaml::parse_file("age: 200", "user.yaml")?;

// Create SourceContext for error location tracking
let mut source_ctx = SourceContext::new();
source_ctx.add_file("user.yaml".to_string(), Some("age: 200".to_string()));

// Validate
let registry = SchemaRegistry::new();
match validate(&doc, &schema, &registry, &source_ctx) {
    Ok(()) => println!("✓ Valid"),
    Err(error) => {
        // Convert to diagnostic for rich error reporting
        let diagnostic = ValidationDiagnostic::from_validation_error(&error, &source_ctx);

        // Get human-readable text with source highlighting
        eprintln!("{}", diagnostic.to_text(&source_ctx));

        // Or get machine-readable JSON
        let json = diagnostic.to_json();
        println!("{}", serde_json::to_string_pretty(&json)?);
    }
}
```

### Error Output Formats

**Text Output (ariadne-style):**
```
Error: [Q-1-16] YAML Validation Failed
   ╭─[user.yaml:1:6]
   │
 1 │ age: 200
   │      ─┬─ Number 200 is out of range (max: 150)
   │       ╰─── violates constraint
───╯
✖ At document path: `age`
ℹ Schema constraint: object > properties > age > number
? Check the allowed value range in the schema?
```

**JSON Output:**
```json
{
  "error_kind": {
    "type": "NumberOutOfRange",
    "data": {
      "value": 200,
      "minimum": null,
      "maximum": 150,
      "exclusive_minimum": null,
      "exclusive_maximum": null
    }
  },
  "code": "Q-1-16",
  "message": "Number 200 is out of range (max: 150)",
  "instance_path": [{"type": "Key", "value": "age"}],
  "schema_path": ["object", "properties", "age", "number"],
  "source_range": {
    "filename": "user.yaml",
    "start_offset": 5,
    "end_offset": 8,
    "start_line": 1,
    "start_column": 6,
    "end_line": 1,
    "end_column": 9
  }
}
```

## Documentation

- **[SCHEMA-FROM-YAML.md](./SCHEMA-FROM-YAML.md)**: Complete YAML syntax reference with examples
  - All supported patterns
  - Real-world examples from quarto-cli
  - Pattern correspondence table (YAML → Rust)
  - Usage guide
- **[JSON-OUTPUT-SCHEMA.md](./JSON-OUTPUT-SCHEMA.md)**: JSON error output format specification
  - Complete field reference
  - Error type catalog
  - TypeScript/Python type definitions
  - LSP integration examples

## Testing

The library includes comprehensive tests against real quarto-cli schema files:

```bash
cargo test --package quarto-yaml-validation
```

**Test Coverage**:
- 56 total tests (43 unit + 13 integration)
- 100% success parsing quarto-cli schemas:
  - document-execute.yml: 12/12 schemas
  - document-text.yml: 7/7 schemas
  - document-website.yml: 8/8 schemas

## Architecture

The codebase is organized into focused modules:

```
src/
├── schema/
│   ├── mod.rs                  # Schema enum and public API
│   ├── types.rs                # Schema struct definitions
│   ├── parser.rs               # Entry point: from_yaml()
│   ├── annotations.rs          # Annotation parsing
│   ├── helpers.rs              # Helper functions
│   └── parsers/
│       ├── primitive.rs        # boolean, number, string, etc.
│       ├── enum.rs             # Enum schemas
│       ├── arrays.rs           # Array and arrayOf
│       ├── objects.rs          # Object and record
│       ├── combinators.rs      # anyOf, allOf, maybeArrayOf
│       ├── ref.rs              # References
│       └── wrappers.rs         # Schema wrappers
├── validator.rs                # Validation logic (future)
└── error.rs                    # Error types
```

## Status

**Production Ready**: All critical quarto-cli patterns implemented and tested.

### Completed (P0/P1 - High Priority)
- ✅ All primitive types
- ✅ Enum (inline and explicit)
- ✅ Array schemas
- ✅ arrayOf (simple and with length)
- ✅ maybeArrayOf
- ✅ Object schemas
- ✅ record (both forms)
- ✅ required: "all"
- ✅ anyOf / allOf
- ✅ References
- ✅ Schema wrappers
- ✅ Annotations

### Future Enhancements (P2/P3 - Lower Priority)
- Nested property extraction (double setBaseSchemaProperties)
- Schema inheritance (super/baseSchema)
- resolveRef vs ref distinction
- propertyNames validation
- namingConvention validation
- additionalCompletions
- Pattern as schema type

## License

Part of the Kyoto/Quarto project.
